use crate::core::{MatchRecord, MatchSink, Matcher};
use memchr::memmem::Finder;
use memchr::{memchr, memchr2};
use regex::bytes::Regex;
use std::io;

use super::{Lines, find_line_bounds};

pub struct ExactMatcher {
    text: String,
    case_sensitive: bool,
}

impl ExactMatcher {
    pub fn new(text: &str, case_sensitive: bool) -> Self {
        Self {
            text: text.into(),
            case_sensitive,
        }
    }

    pub fn is_match(&self, bytes: &[u8]) -> bool {
        if self.case_sensitive {
            return bytes == self.text.as_bytes();
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            return false;
        };
        let mut expected = self.text.chars();
        let mut actual = text.chars();
        loop {
            match (expected.next(), actual.next()) {
                (None, None) => return true,
                (Some(a), Some(b)) if super::fuzzy::chars_equal(a, b, false) => {}
                _ => return false,
            }
        }
    }
}

pub struct SimdLiteralMatcher {
    pub(crate) needle: Vec<u8>,
    pub(crate) case_sensitive: bool,
    pub(crate) needle_lower: Vec<u8>,
    pub(crate) finder: Option<Finder<'static>>,
    pub(crate) case_insensitive_regex: Option<Regex>,
    pub(crate) first_byte: u8,
    pub(crate) first_byte_alt: Option<u8>,
}

impl SimdLiteralMatcher {
    pub fn new(needle_str: &str, case_sensitive: bool) -> Self {
        let needle = needle_str.as_bytes().to_vec();
        let needle_lower = needle_str.to_ascii_lowercase().into_bytes();
        let (first_byte, first_byte_alt) = if needle.is_empty() {
            (0, None)
        } else if !case_sensitive {
            let lower = needle[0].to_ascii_lowercase();
            let upper = needle[0].to_ascii_uppercase();
            if lower != upper {
                (lower, Some(upper))
            } else {
                (lower, None)
            }
        } else {
            (needle[0], None)
        };

        let finder = if case_sensitive && !needle.is_empty() {
            Some(Finder::new(&needle).into_owned())
        } else {
            None
        };

        let case_insensitive_regex = if !case_sensitive && !needle.is_empty() {
            Regex::new(&format!("(?i){}", regex::escape(needle_str))).ok()
        } else {
            None
        };

        Self {
            needle,
            case_sensitive,
            needle_lower,
            finder,
            case_insensitive_regex,
            first_byte,
            first_byte_alt,
        }
    }

    pub fn with_smart_case(needle_str: &str, smart_case: bool) -> Self {
        let has_upper = needle_str.chars().any(|c| c.is_uppercase());
        let case_sensitive = if smart_case { has_upper } else { true };
        Self::new(needle_str, case_sensitive)
    }

    #[inline]
    pub(crate) fn matches_at(&self, window: &[u8]) -> bool {
        if window.len() < self.needle.len() {
            return false;
        }
        if self.case_sensitive {
            &window[..self.needle.len()] == self.needle.as_slice()
        } else {
            window[..self.needle.len()].eq_ignore_ascii_case(&self.needle_lower)
        }
    }

    pub fn collect_spans(&self, line: &[u8], spans: &mut Vec<(usize, usize)>) {
        if let Some(ref re) = self.case_insensitive_regex {
            spans.extend(re.find_iter(line).map(|m| (m.start(), m.end())));
        } else if let Some(ref finder) = self.finder {
            spans.extend(
                finder
                    .find_iter(line)
                    .map(|start| (start, start + self.needle.len())),
            );
        } else if !self.needle.is_empty() {
            let mut cursor = 0;
            while cursor + self.needle.len() <= line.len() {
                let next_cand = if let Some(alt) = self.first_byte_alt {
                    memchr2(self.first_byte, alt, &line[cursor..])
                } else {
                    memchr(self.first_byte, &line[cursor..])
                };

                let cand_rel = match next_cand {
                    Some(offset) => offset,
                    None => break,
                };

                let match_idx = cursor + cand_rel;
                if self.matches_at(&line[match_idx..]) {
                    spans.push((match_idx, match_idx + self.needle.len()));
                    cursor = match_idx + self.needle.len();
                } else {
                    cursor = match_idx + 1;
                }
            }
        }
    }
}

impl Matcher for SimdLiteralMatcher {
    fn find_matches<'a>(
        &self,
        buffer: &'a [u8],
        sink: &mut dyn MatchSink<'a>,
    ) -> io::Result<usize> {
        if self.needle.is_empty() {
            return RegexMatcher::new("(?m)^", false)
                .map_err(io::Error::other)?
                .find_matches(buffer, sink);
        }
        if self.case_insensitive_regex.is_none() && buffer.len() < self.needle.len() {
            return Ok(0);
        }

        let mut count = 0;
        let mut cursor = 0;
        let mut last_newline = 0;
        let mut current_line = 1;
        let mut current_record: Option<MatchRecord<'a>> = None;

        let mut push_match = |line_num: usize,
                              line_start: usize,
                              line_end: usize,
                              rel_start: usize,
                              rel_end: usize,
                              sink: &mut dyn MatchSink<'a>|
         -> io::Result<()> {
            if let Some(ref mut rec) = current_record {
                if rec.line_byte_offset == line_start {
                    rec.match_spans.push((rel_start, rel_end));
                    rec.occurrence_count += 1;
                    return Ok(());
                }
                let prev = current_record.replace(MatchRecord::new(
                    line_num,
                    line_start,
                    &buffer[line_start..line_end],
                    vec![(rel_start, rel_end)],
                ));
                if let Some(p) = prev {
                    sink.on_match(p)?;
                }
            } else {
                current_record = Some(MatchRecord::new(
                    line_num,
                    line_start,
                    &buffer[line_start..line_end],
                    vec![(rel_start, rel_end)],
                ));
            }
            Ok(())
        };

        if let Some(ref finder) = self.finder {
            while cursor + self.needle.len() <= buffer.len() {
                let match_rel = match finder.find(&buffer[cursor..]) {
                    Some(offset) => offset,
                    None => break,
                };

                let match_idx = cursor + match_rel;
                let (line_start, line_end, line_num) =
                    find_line_bounds(buffer, match_idx, last_newline, current_line);

                last_newline = line_start;
                current_line = line_num;

                let rel_start = match_idx - line_start;
                let rel_end = (rel_start + self.needle.len()).min(line_end - line_start);

                count += 1;
                push_match(line_num, line_start, line_end, rel_start, rel_end, sink)?;

                cursor = match_idx + self.needle.len();
            }
        } else if let Some(ref re) = self.case_insensitive_regex {
            for m in re.find_iter(buffer) {
                let match_idx = m.start();
                let match_end = m.end();
                let (line_start, line_end, line_num) =
                    find_line_bounds(buffer, match_idx, last_newline, current_line);

                last_newline = line_start;
                current_line = line_num;

                let rel_start = match_idx - line_start;
                let rel_end = (match_end - line_start).min(line_end - line_start);

                count += 1;
                push_match(line_num, line_start, line_end, rel_start, rel_end, sink)?;
            }
        } else {
            while cursor + self.needle.len() <= buffer.len() {
                let next_cand = if let Some(alt) = self.first_byte_alt {
                    memchr2(self.first_byte, alt, &buffer[cursor..])
                } else {
                    memchr(self.first_byte, &buffer[cursor..])
                };

                let cand_rel = match next_cand {
                    Some(offset) => offset,
                    None => break,
                };

                let match_idx = cursor + cand_rel;

                if self.matches_at(&buffer[match_idx..]) {
                    let (line_start, line_end, line_num) =
                        find_line_bounds(buffer, match_idx, last_newline, current_line);

                    last_newline = line_start;
                    current_line = line_num;

                    let rel_start = match_idx - line_start;
                    let rel_end = (rel_start + self.needle.len()).min(line_end - line_start);

                    count += 1;
                    push_match(line_num, line_start, line_end, rel_start, rel_end, sink)?;

                    cursor = match_idx + self.needle.len();
                } else {
                    cursor = match_idx + 1;
                }
            }
        }

        if let Some(rec) = current_record {
            sink.on_match(rec)?;
        }

        Ok(count)
    }
}

pub struct AllLinesMatcher;

impl Matcher for AllLinesMatcher {
    fn find_matches<'a>(
        &self,
        buffer: &'a [u8],
        sink: &mut dyn MatchSink<'a>,
    ) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }

        let mut count = 0;
        let mut line_num = 1;
        let mut start = 0;

        for nl in memchr::Memchr::new(b'\n', buffer) {
            let end = if nl > start && buffer[nl - 1] == b'\r' {
                nl - 1
            } else {
                nl
            };

            if end > start {
                let rec =
                    MatchRecord::new(line_num, start, &buffer[start..end], vec![(0, end - start)]);
                sink.on_match(rec)?;
                count += 1;
            }

            start = nl + 1;
            line_num += 1;
        }

        if start < buffer.len() {
            let end = if buffer.last() == Some(&b'\r') {
                buffer.len() - 1
            } else {
                buffer.len()
            };
            if end > start {
                let rec =
                    MatchRecord::new(line_num, start, &buffer[start..end], vec![(0, end - start)]);
                sink.on_match(rec)?;
                count += 1;
            }
        }

        Ok(count)
    }
}

pub struct NeverMatcher;

impl Matcher for NeverMatcher {
    fn find_matches<'a>(
        &self,
        _buffer: &'a [u8],
        _sink: &mut dyn MatchSink<'a>,
    ) -> io::Result<usize> {
        Ok(0)
    }
}

pub struct RegexMatcher {
    regex: Regex,
}

impl RegexMatcher {
    pub fn new(pattern: &str, case_insensitive: bool) -> Result<Self, regex::Error> {
        let regex = regex::bytes::RegexBuilder::new(pattern)
            .case_insensitive(case_insensitive)
            .multi_line(true)
            .build()?;
        Ok(Self { regex })
    }
}

impl Matcher for RegexMatcher {
    fn find_matches<'a>(
        &self,
        buffer: &'a [u8],
        sink: &mut dyn MatchSink<'a>,
    ) -> io::Result<usize> {
        if buffer.is_empty() || !self.regex.is_match(buffer) {
            return Ok(0);
        }

        let mut count = 0;
        for (number, offset, line) in Lines::new(buffer) {
            let spans: Vec<_> = self
                .regex
                .find_iter(line)
                .map(|m| (m.start(), m.end()))
                .collect();
            if !spans.is_empty() {
                count += spans.len();
                sink.on_match(MatchRecord::new(number, offset, line, spans))?;
            }
        }
        Ok(count)
    }
}

pub struct HexMatcher {
    pub(crate) pattern: Vec<Option<u8>>,
    first_fixed_byte: Option<(usize, u8)>,
}

impl HexMatcher {
    pub fn new(pattern: Vec<Option<u8>>) -> Self {
        let first_fixed_byte = pattern
            .iter()
            .enumerate()
            .find_map(|(idx, opt)| opt.map(|b| (idx, b)));

        Self {
            pattern,
            first_fixed_byte,
        }
    }

    #[inline]
    fn matches_at(&self, window: &[u8]) -> bool {
        if window.len() < self.pattern.len() {
            return false;
        }
        for (i, opt) in self.pattern.iter().enumerate() {
            if let Some(expected) = opt
                && window[i] != *expected
            {
                return false;
            }
        }
        true
    }

    #[inline]
    pub(crate) fn find_from(&self, buffer: &[u8], mut cursor: usize) -> Option<usize> {
        if self.pattern.is_empty() || buffer.len() < self.pattern.len() {
            return None;
        }

        while cursor + self.pattern.len() <= buffer.len() {
            let offset = if let Some((first_idx, first_val)) = self.first_fixed_byte {
                let search_start = cursor + first_idx;
                let rel = memchr(first_val, &buffer[search_start..])?;
                search_start + rel - first_idx
            } else {
                cursor
            };

            if offset + self.pattern.len() > buffer.len() {
                return None;
            }
            if self.matches_at(&buffer[offset..]) {
                return Some(offset);
            }
            cursor = offset + 1;
        }
        None
    }
}

impl Matcher for HexMatcher {
    fn find_matches<'a>(
        &self,
        buffer: &'a [u8],
        sink: &mut dyn MatchSink<'a>,
    ) -> io::Result<usize> {
        if self.pattern.is_empty() || buffer.len() < self.pattern.len() {
            return Ok(0);
        }

        let mut count = 0;
        let mut cursor = 0;
        let mut current: Option<MatchRecord<'a>> = None;
        let mut previous_offset = 0;
        let mut previous_line = 1;
        while let Some(offset) = self.find_from(buffer, cursor) {
            let (start, end, number) =
                find_line_bounds(buffer, offset, previous_offset, previous_line);
            let match_end = offset + self.pattern.len();
            if match_end > end {
                return Err(io::Error::other(
                    "Hex matches spanning line separators are unsupported",
                ));
            }
            previous_offset = start;
            previous_line = number;
            let span = (offset - start, match_end - start);
            if let Some(record) = current
                .as_mut()
                .filter(|record| record.line_byte_offset == start)
            {
                record.match_spans.push(span);
                record.occurrence_count += 1;
            } else {
                if let Some(record) = current.take() {
                    sink.on_match(record)?;
                }
                current = Some(MatchRecord::new(
                    number,
                    start,
                    &buffer[start..end],
                    vec![span],
                ));
            }
            count += 1;
            cursor = match_end;
        }
        if let Some(record) = current {
            sink.on_match(record)?;
        }

        Ok(count)
    }
}
