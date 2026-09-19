use crate::core::{MatchRecord, MatchSink, Matcher};
use crate::dsl::{QueryExpr, SearchPattern};
use memchr::memmem::Finder;
use memchr::{memchr, memchr2, memrchr};
use regex::bytes::Regex;
use std::io;

/// Detect whether a byte slice represents binary data by probing for null bytes
/// within the specified probe length (in bytes).
#[inline]
pub fn is_binary_with_probe_size(bytes: &[u8], probe_size: usize) -> bool {
    let probe_len = bytes.len().min(probe_size);
    memchr(0x00, &bytes[..probe_len]).is_some()
}

/// Detect whether a byte slice represents binary data by probing for null bytes
/// within the first 1024 bytes (standard Git and Ripgrep heuristic).
#[inline]
pub fn is_binary(bytes: &[u8]) -> bool {
    is_binary_with_probe_size(bytes, 1024)
}

/// Extracts printable ASCII string spans (length >= min_len) from raw binary byte slices.
/// Returns a list of (byte_offset, string_slice).
pub fn extract_printable_strings(buffer: &[u8], min_len: usize) -> Vec<(usize, &[u8])> {
    let mut strings = Vec::new();
    let mut start = None;

    for (i, &b) in buffer.iter().enumerate() {
        if (0x20..=0x7e).contains(&b) || b == b'\t' {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take()
            && i - s >= min_len
        {
            strings.push((s, &buffer[s..i]));
        }
    }

    if let Some(s) = start
        && buffer.len() - s >= min_len
    {
        strings.push((s, &buffer[s..]));
    }

    strings
}

/// Helper to expand line boundaries around a match offset.
/// Returns (line_start_abs, line_end_abs, line_number).
#[inline]
pub fn find_line_bounds(
    bytes: &[u8],
    match_offset: usize,
    last_newline: usize,
    current_line: usize,
) -> (usize, usize, usize) {
    // Find line start (preceding newline after last_newline, or 0)
    let line_start = match memrchr(b'\n', &bytes[..match_offset]) {
        Some(pos) => pos + 1,
        None => 0,
    };

    // Calculate line number increment
    let line_num = if line_start > last_newline {
        // Count newlines between last_newline and line_start
        let count = memchr::Memchr::new(b'\n', &bytes[last_newline..line_start]).count();
        current_line + count
    } else {
        current_line
    };

    // Find line end (next newline after match_offset, or EOF)
    let line_end = match memchr(b'\n', &bytes[match_offset..]) {
        Some(pos) => match_offset + pos,
        None => bytes.len(),
    };

    // Strip trailing \r if present (CRLF handling)
    let clean_end = if line_end > line_start && bytes[line_end - 1] == b'\r' {
        line_end - 1
    } else {
        line_end
    };

    (line_start, clean_end, line_num)
}

/// SIMD-accelerated literal matcher for single strings using memchr::memmem vectorization.
pub struct SimdLiteralMatcher {
    needle: Vec<u8>,
    case_sensitive: bool,
    needle_lower: Vec<u8>,
    finder: Option<Finder<'static>>,
    case_insensitive_regex: Option<Regex>,
    first_byte: u8,
    first_byte_alt: Option<u8>,
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

        // ASCII literals stay on the memchr/memchr2 candidate path below. Keep
        // the regex fallback only for non-ASCII case folding, which cannot be
        // represented by a pair of ASCII byte candidates.
        let case_insensitive_regex = if !case_sensitive && !needle.is_empty() && !needle.is_ascii()
        {
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
    fn matches_at(&self, window: &[u8]) -> bool {
        if window.len() < self.needle.len() {
            return false;
        }
        if self.case_sensitive {
            &window[..self.needle.len()] == self.needle.as_slice()
        } else {
            window[..self.needle.len()].eq_ignore_ascii_case(&self.needle_lower)
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
        if buffer.len() < self.needle.len() {
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

/// Iterate real input lines without allocating or inventing a line after a final newline.
struct Lines<'a> {
    bytes: &'a [u8],
    newlines: memchr::Memchr<'a>,
    offset: usize,
    number: usize,
}

impl<'a> Lines<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            newlines: memchr::Memchr::new(b'\n', bytes),
            offset: 0,
            number: 0,
        }
    }
}

impl<'a> Iterator for Lines<'a> {
    type Item = (usize, usize, &'a [u8]);
    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.bytes.len() {
            return None;
        }
        let start = self.offset;
        let newline = self.newlines.next();
        let end = newline.unwrap_or(self.bytes.len());
        let clean_end = if newline.is_some() && end > start && self.bytes[end - 1] == b'\r' {
            end - 1
        } else {
            end
        };
        self.offset = newline.map_or(self.bytes.len(), |position| position + 1);
        self.number += 1;
        Some((self.number, start, &self.bytes[start..clean_end]))
    }
}

/// Matcher that matches every non-empty line (used when no search pattern is specified).
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

/// Matcher that never matches any line (used when an explicit pattern file contains zero patterns).
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

/// Fallback matcher for explicit regular expressions.
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

/// Hex pattern matcher for binary searches.
pub struct HexMatcher {
    pattern: Vec<Option<u8>>,
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
    fn find_from(&self, buffer: &[u8], mut cursor: usize) -> Option<usize> {
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

        while let Some(offset) = self.find_from(buffer, cursor) {
            let record = MatchRecord::new(
                offset + 1, // Maintain 1-indexed contract for line_number
                offset,
                &buffer[offset..(offset + self.pattern.len()).min(buffer.len())],
                vec![(0, self.pattern.len())],
            );
            sink.on_match(record)?;
            count += 1;
            cursor = offset + self.pattern.len();
        }

        Ok(count)
    }
}

/// Precompiled AST expression for zero-allocation boolean evaluation.
#[allow(clippy::large_enum_variant)]
pub enum CompiledExpr {
    Literal(SimdLiteralMatcher),
    Regex(regex::bytes::Regex),
    Hex(HexMatcher),
    EmptySet,
    And(Box<CompiledExpr>, Box<CompiledExpr>),
    Or(Box<CompiledExpr>, Box<CompiledExpr>),
    Not(Box<CompiledExpr>),
}

impl CompiledExpr {
    pub fn from_query_expr(expr: &QueryExpr) -> Result<Self, regex::Error> {
        Ok(match expr {
            QueryExpr::Pattern(p) => match p {
                SearchPattern::Literal { text, .. } => {
                    let is_cs = match expr {
                        QueryExpr::Pattern(p) => p.is_case_sensitive(),
                        _ => true,
                    };
                    CompiledExpr::Literal(SimdLiteralMatcher::new(text, is_cs))
                }
                SearchPattern::ExactLiteral(text) => {
                    CompiledExpr::Literal(SimdLiteralMatcher::new(text, true))
                }
                SearchPattern::Regex(pattern) => {
                    let re = regex::bytes::RegexBuilder::new(pattern)
                        .multi_line(true)
                        .build()?;
                    CompiledExpr::Regex(re)
                }
                SearchPattern::Hex(bytes) => CompiledExpr::Hex(HexMatcher::new(bytes.clone())),
                SearchPattern::EmptySet => CompiledExpr::EmptySet,
            },
            QueryExpr::And(a, b) => CompiledExpr::And(
                Box::new(Self::from_query_expr(a)?),
                Box::new(Self::from_query_expr(b)?),
            ),
            QueryExpr::Or(a, b) => CompiledExpr::Or(
                Box::new(Self::from_query_expr(a)?),
                Box::new(Self::from_query_expr(b)?),
            ),
            QueryExpr::Not(a) => CompiledExpr::Not(Box::new(Self::from_query_expr(a)?)),
        })
    }

    #[inline]
    pub fn eval_on_line(&self, line: &[u8]) -> bool {
        match self {
            CompiledExpr::Literal(m) => {
                m.matches_at(line) || find_substr_on_line(m, line).is_some()
            }
            CompiledExpr::Regex(re) => re.is_match(line),
            CompiledExpr::Hex(m) => m.find_from(line, 0).is_some(),
            CompiledExpr::EmptySet => false,
            CompiledExpr::And(a, b) => a.eval_on_line(line) && b.eval_on_line(line),
            CompiledExpr::Or(a, b) => a.eval_on_line(line) || b.eval_on_line(line),
            CompiledExpr::Not(a) => !a.eval_on_line(line),
        }
    }

    pub fn collect_spans(&self, line: &[u8], spans: &mut Vec<(usize, usize)>) {
        match self {
            CompiledExpr::Literal(m) => {
                if let Some(re) = &m.case_insensitive_regex {
                    spans.extend(re.find_iter(line).map(|m| (m.start(), m.end())));
                } else if let Some(finder) = &m.finder {
                    spans.extend(
                        finder
                            .find_iter(line)
                            .map(|start| (start, start + m.needle.len())),
                    );
                } else if !m.needle.is_empty() {
                    let mut cursor = 0;
                    while cursor + m.needle.len() <= line.len() {
                        let next_cand = if let Some(alt) = m.first_byte_alt {
                            memchr2(m.first_byte, alt, &line[cursor..])
                        } else {
                            memchr(m.first_byte, &line[cursor..])
                        };

                        let cand_rel = match next_cand {
                            Some(offset) => offset,
                            None => break,
                        };

                        let match_idx = cursor + cand_rel;
                        if m.matches_at(&line[match_idx..]) {
                            spans.push((match_idx, match_idx + m.needle.len()));
                            cursor = match_idx + m.needle.len();
                        } else {
                            cursor = match_idx + 1;
                        }
                    }
                }
            }
            CompiledExpr::Regex(re) => {
                for m in re.find_iter(line) {
                    spans.push((m.start(), m.end()));
                }
            }
            CompiledExpr::Hex(m) => {
                let mut cursor = 0;
                while let Some(start) = m.find_from(line, cursor) {
                    spans.push((start, start + m.pattern.len()));
                    cursor = start + m.pattern.len();
                }
            }
            CompiledExpr::EmptySet => {}
            CompiledExpr::And(a, b) => {
                a.collect_spans(line, spans);
                b.collect_spans(line, spans);
            }
            CompiledExpr::Or(a, b) => {
                if a.eval_on_line(line) {
                    a.collect_spans(line, spans);
                }
                if b.eval_on_line(line) {
                    b.collect_spans(line, spans);
                }
            }
            CompiledExpr::Not(_) => {}
        }
    }
}

/// Precompiled proximity filter for neighbourhood matching.
pub struct CompiledProximityFilter {
    pub matcher: SimdLiteralMatcher,
    pub window: usize,
    pub inverted: bool,
}

/// Composed AST matcher evaluating boolean combinations (AND, OR, NOT) with optional proximity constraints.
pub struct BooleanEngine {
    compiled: Result<CompiledExpr, regex::Error>,
    proximity_filters: Vec<CompiledProximityFilter>,
}

impl BooleanEngine {
    pub fn new(expr: QueryExpr) -> Self {
        let compiled = CompiledExpr::from_query_expr(&expr);
        Self {
            compiled,
            proximity_filters: Vec::new(),
        }
    }

    /// Compile and validate every branch before traversal begins.
    pub fn try_new(expr: QueryExpr) -> Result<Self, regex::Error> {
        Ok(Self {
            compiled: Ok(CompiledExpr::from_query_expr(&expr)?),
            proximity_filters: Vec::new(),
        })
    }

    pub fn with_proximity_filters(mut self, filters: &[crate::dsl::ProximityFilter]) -> Self {
        self.proximity_filters = filters
            .iter()
            .map(|f| {
                let matcher = SimdLiteralMatcher::with_smart_case(&f.term, true);
                CompiledProximityFilter {
                    matcher,
                    window: f.window,
                    inverted: f.inverted,
                }
            })
            .collect();
        self
    }
}

#[inline]
fn find_substr_on_line(matcher: &SimdLiteralMatcher, line: &[u8]) -> Option<usize> {
    if line.len() < matcher.needle.len() {
        return None;
    }
    if let Some(ref finder) = matcher.finder {
        return finder.find(line);
    }
    if let Some(ref re) = matcher.case_insensitive_regex {
        return re.find(line).map(|m| m.start());
    }
    let mut cursor = 0;
    while cursor + matcher.needle.len() <= line.len() {
        let next_cand = if let Some(alt) = matcher.first_byte_alt {
            memchr2(matcher.first_byte, alt, &line[cursor..])
        } else {
            memchr(matcher.first_byte, &line[cursor..])
        };
        match next_cand {
            Some(rel) => {
                let idx = cursor + rel;
                if matcher.matches_at(&line[idx..]) {
                    return Some(idx);
                }
                cursor = idx + 1;
            }
            None => break,
        }
    }
    None
}

impl Matcher for BooleanEngine {
    fn find_matches<'a>(
        &self,
        buffer: &'a [u8],
        sink: &mut dyn MatchSink<'a>,
    ) -> io::Result<usize> {
        let compiled = self
            .compiled
            .as_ref()
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err.to_string()))?;
        // Pre-scan matching line indices for active proximity filters without materializing all lines on the heap
        let mut filter_match_lines: Vec<Vec<usize>> =
            Vec::with_capacity(self.proximity_filters.len());
        for filter in &self.proximity_filters {
            let mut matches = Vec::new();
            for (number, _, line) in Lines::new(buffer) {
                if filter.matcher.matches_at(line)
                    || find_substr_on_line(&filter.matcher, line).is_some()
                {
                    matches.push(number - 1);
                }
            }
            // Short-circuit: if a non-inverted proximity filter matches nowhere, no line can satisfy it
            if !filter.inverted && matches.is_empty() {
                return Ok(0);
            }
            filter_match_lines.push(matches);
        }

        let mut cursors = vec![0usize; self.proximity_filters.len()];
        let mut count = 0;
        for (number, offset, line) in Lines::new(buffer) {
            if !compiled.eval_on_line(line) {
                continue;
            }
            let index = number - 1;
            let mut nearby = true;
            for (i, filter) in self.proximity_filters.iter().enumerate() {
                let start = index.saturating_sub(filter.window);
                let end = index.saturating_add(filter.window);
                let matches = &filter_match_lines[i];
                let cursor = &mut cursors[i];

                // Monotonically advance cursor to the first match >= start
                while *cursor < matches.len() && matches[*cursor] < start {
                    *cursor += 1;
                }

                let found = *cursor < matches.len() && matches[*cursor] <= end;
                if found == filter.inverted {
                    nearby = false;
                    break;
                }
            }
            if !nearby {
                continue;
            }
            let mut spans = Vec::new();
            compiled.collect_spans(line, &mut spans);
            spans.sort_unstable();
            spans.dedup();
            sink.on_match(MatchRecord::new(number, offset, line, spans))?;
            count += 1;
        }
        Ok(count)
    }
}

/// Given a file buffer and the list of active matches found in that buffer,
/// expand and populate surrounding context lines (before and after) without duplication.
pub fn expand_context_lines<'a>(
    buffer: &'a [u8],
    matches: &[MatchRecord<'a>],
    before_context: usize,
    after_context: usize,
) -> Vec<MatchRecord<'a>> {
    if matches.is_empty() || (before_context == 0 && after_context == 0) {
        return matches.to_vec();
    }

    // Index all line boundaries in the buffer
    let mut line_spans = Vec::new();
    let mut line_start = 0;
    for nl in memchr::Memchr::new(b'\n', buffer) {
        let clean_end = if nl > line_start && buffer[nl - 1] == b'\r' {
            nl - 1
        } else {
            nl
        };
        line_spans.push((line_start, clean_end));
        line_start = nl + 1;
    }
    if line_start < buffer.len() {
        line_spans.push((line_start, buffer.len()));
    }

    let total_lines = line_spans.len();
    if total_lines == 0 {
        return matches.to_vec();
    }

    // If any match's line_number exceeds total_lines, this is a binary/hex match with byte offsets
    if matches.iter().any(|m| m.line_number > total_lines) {
        return matches.to_vec();
    }

    // Set of match line indices (0-indexed) mapping to optional match_spans
    let mut match_map: std::collections::BTreeMap<usize, Option<Vec<(usize, usize)>>> =
        std::collections::BTreeMap::new();

    for m in matches {
        let line_idx = m.line_number.saturating_sub(1);
        match_map.insert(line_idx, Some(m.match_spans.clone()));

        // Add before context lines
        let start_ctx = line_idx.saturating_sub(before_context);
        for ctx_idx in start_ctx..line_idx {
            match_map.entry(ctx_idx).or_insert(None);
        }

        // Add after context lines
        let end_ctx = (line_idx + 1 + after_context).min(total_lines);
        for ctx_idx in (line_idx + 1)..end_ctx {
            match_map.entry(ctx_idx).or_insert(None);
        }
    }

    let mut expanded = Vec::with_capacity(match_map.len());
    for (line_idx, maybe_spans) in match_map {
        if line_idx >= total_lines {
            continue;
        }
        let (start, end) = line_spans[line_idx];
        let line_bytes = &buffer[start..end];
        let line_number = line_idx + 1;

        if let Some(spans) = maybe_spans {
            expanded.push(MatchRecord::new(line_number, start, line_bytes, spans));
        } else {
            expanded.push(MatchRecord::context(line_number, start, line_bytes));
        }
    }

    expanded
}

/// Helper match collector for test verification and buffering.
#[derive(Default)]
pub struct VecMatchSink {
    pub matches: Vec<crate::core::OwnedMatchRecord>,
}

impl<'a> MatchSink<'a> for VecMatchSink {
    fn on_match(&mut self, record: MatchRecord<'a>) -> io::Result<()> {
        self.matches.push(record.to_owned());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_and_boolean_match_real_lines_consistently() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mixed.txt");
        for bytes in [b"".as_slice(), b"\n", b"hit\r\n\nlast", b"last\r"] {
            std::fs::write(&file, bytes).unwrap();
            let input = std::fs::read(&file).unwrap();
            for pattern in ["^", "$", "hit", r"(?s)hit.*last"] {
                let mut single = VecMatchSink::default();
                let mut boolean = VecMatchSink::default();
                RegexMatcher::new(pattern, false)
                    .unwrap()
                    .find_matches(&input, &mut single)
                    .unwrap();
                BooleanEngine::try_new(QueryExpr::Pattern(SearchPattern::Regex(pattern.into())))
                    .unwrap()
                    .find_matches(&input, &mut boolean)
                    .unwrap();
                let rows = |sink: &VecMatchSink| {
                    sink.matches
                        .iter()
                        .map(|r| {
                            (
                                r.line_number,
                                r.line_byte_offset,
                                r.line_bytes.clone(),
                                r.match_spans.clone(),
                            )
                        })
                        .collect::<Vec<_>>()
                };
                assert_eq!(rows(&single), rows(&boolean), "{pattern} on {bytes:?}");
            }
        }
    }

    #[test]
    fn invalid_boolean_regex_remains_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("invalid-regex.txt");
        std::fs::write(&file, b"foo\n").unwrap();
        let bytes = std::fs::read(file).unwrap();
        let expr = QueryExpr::Or(
            Box::new(QueryExpr::Pattern(SearchPattern::ExactLiteral(
                "foo".into(),
            ))),
            Box::new(QueryExpr::Pattern(SearchPattern::Regex("[".into()))),
        );
        assert!(BooleanEngine::try_new(expr.clone()).is_err());
        let err = BooleanEngine::new(expr)
            .find_matches(&bytes, &mut VecMatchSink::default())
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn boolean_unicode_spans_follow_selection() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("unicode.txt");
        std::fs::write(&file, "École école\n").unwrap();
        let bytes = std::fs::read(file).unwrap();
        let expr = QueryExpr::Pattern(SearchPattern::Literal {
            text: "école".into(),
            case_sensitive: Some(false),
        });
        let mut sink = VecMatchSink::default();
        BooleanEngine::try_new(expr)
            .unwrap()
            .find_matches(&bytes, &mut sink)
            .unwrap();
        assert_eq!(sink.matches[0].match_spans, vec![(0, 6), (7, 13)]);
    }

    #[test]
    fn test_is_binary_detection() {
        let text_data = b"hello world\nthis is a normal text file\n";
        assert!(!is_binary(text_data));

        let binary_data = b"hello\x00world\nnull byte embedded\n";
        assert!(is_binary(binary_data));
    }

    #[test]
    fn test_simd_literal_smart_case() {
        let data = b"line 1: connect\nline 2: CONNECT\nline 3: no match\n";

        // Lowercase query -> smart case matches both
        let matcher = SimdLiteralMatcher::with_smart_case("connect", true);
        let mut sink = VecMatchSink::default();
        let count = matcher.find_matches(data, &mut sink).unwrap();
        assert_eq!(count, 2);
        assert_eq!(sink.matches[0].line_number, 1);
        assert_eq!(sink.matches[1].line_number, 2);

        // Uppercase query -> case sensitive
        let matcher_upper = SimdLiteralMatcher::with_smart_case("CONNECT", true);
        let mut sink_upper = VecMatchSink::default();
        let count_upper = matcher_upper.find_matches(data, &mut sink_upper).unwrap();
        assert_eq!(count_upper, 1);
        assert_eq!(sink_upper.matches[0].line_number, 2);
    }

    #[test]
    fn test_simd_literal_case_insensitive_with_uppercase_query() {
        let data = b"line 1: simple test\nline 2: Simple test\nline 3: SIMPLE test\n";

        // Case-insensitive search matches lowercase, capitalized, and uppercase variants
        let matcher = SimdLiteralMatcher::new("Simple", false);
        let mut sink = VecMatchSink::default();
        let count = matcher.find_matches(data, &mut sink).unwrap();
        assert_eq!(count, 3);
        assert_eq!(sink.matches.len(), 3);
        assert_eq!(sink.matches[0].line_number, 1);
        assert_eq!(sink.matches[1].line_number, 2);
        assert_eq!(sink.matches[2].line_number, 3);
    }

    #[test]
    fn ascii_case_insensitive_literals_use_simd_candidate_search() {
        let matcher = SimdLiteralMatcher::new("Needle", false);
        assert!(matcher.case_insensitive_regex.is_none());
        assert!(matcher.finder.is_none());

        let mut sink = VecMatchSink::default();
        matcher
            .find_matches(b"prefix nEeDlE suffix\n", &mut sink)
            .unwrap();
        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.matches[0].match_spans, vec![(7, 13)]);
    }

    #[test]
    fn test_boolean_and_matcher() {
        let data = b"first: auth without key\nsecond: auth with token here\nthird: only token\n";
        let expr = QueryExpr::And(
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "auth".to_string(),
                case_sensitive: None,
            })),
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "token".to_string(),
                case_sensitive: None,
            })),
        );

        let engine = BooleanEngine::new(expr);
        let mut sink = VecMatchSink::default();
        let count = engine.find_matches(data, &mut sink).unwrap();

        assert_eq!(count, 1);
        assert_eq!(sink.matches[0].line_number, 2);
        assert_eq!(sink.matches[0].line_bytes, b"second: auth with token here");
    }

    #[test]
    fn boolean_hex_atoms_match_and_report_mid_line_offsets() {
        let expr = QueryExpr::And(
            Box::new(QueryExpr::Pattern(SearchPattern::Hex(vec![Some(0x48)]))),
            Box::new(QueryExpr::Pattern(SearchPattern::Hex(vec![Some(0x89)]))),
        );
        let matcher = BooleanEngine::try_new(expr).unwrap();
        let mut sink = VecMatchSink::default();

        matcher
            .find_matches(&[0x00, 0x48, 0x89], &mut sink)
            .unwrap();

        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.matches[0].match_spans, vec![(1, 2), (2, 3)]);
    }

    #[test]
    fn test_hex_matcher() {
        let data = b"\x7fELF\x02\x01\x01\x00\x48\x89\xe5\x90\x90";
        // Search for 48 ?? e5
        let hex_pat = vec![Some(0x48), None, Some(0xE5)];
        let matcher = HexMatcher::new(hex_pat);
        let mut sink = VecMatchSink::default();
        let count = matcher.find_matches(data, &mut sink).unwrap();

        assert_eq!(count, 1);
        assert_eq!(sink.matches[0].line_byte_offset, 8);
        assert_eq!(sink.matches[0].line_number, 9); // 1-indexed contract

        // Match at offset 0
        let zero_pat = vec![Some(0x7F), Some(0x45)];
        let zero_matcher = HexMatcher::new(zero_pat);
        let mut zero_sink = VecMatchSink::default();
        let zero_count = zero_matcher.find_matches(data, &mut zero_sink).unwrap();
        assert_eq!(zero_count, 1);
        assert_eq!(zero_sink.matches[0].line_byte_offset, 0);
        assert_eq!(zero_sink.matches[0].line_number, 1);
    }

    #[test]
    fn test_expand_context_lines() {
        let buffer = b"line 1: before\nline 2: match here\nline 3: after\nline 4: far away\n";
        let matches = vec![MatchRecord::new(
            2,
            15,
            b"line 2: match here",
            vec![(8, 18)],
        )];

        let expanded = expand_context_lines(buffer, &matches, 1, 1);
        assert_eq!(expanded.len(), 3);

        assert!(expanded[0].is_context);
        assert_eq!(expanded[0].line_number, 1);
        assert_eq!(expanded[0].line_bytes, b"line 1: before");

        assert!(!expanded[1].is_context);
        assert_eq!(expanded[1].line_number, 2);
        assert_eq!(expanded[1].line_bytes, b"line 2: match here");

        assert!(expanded[2].is_context);
        assert_eq!(expanded[2].line_number, 3);
        assert_eq!(expanded[2].line_bytes, b"line 3: after");
    }

    #[test]
    fn test_multiple_matches_on_same_line() {
        let data = b"foo and foo and foo\nbar only\nfoo again\n";

        // Test SimdLiteralMatcher
        let literal_matcher = SimdLiteralMatcher::new("foo", true);
        let mut sink = VecMatchSink::default();
        let count = literal_matcher.find_matches(data, &mut sink).unwrap();
        assert_eq!(count, 4); // 3 on line 1, 1 on line 3
        assert_eq!(sink.matches.len(), 2); // 2 distinct lines
        assert_eq!(sink.matches[0].line_number, 1);
        assert_eq!(sink.matches[0].match_spans, vec![(0, 3), (8, 11), (16, 19)]);
        assert_eq!(sink.matches[1].line_number, 3);
        assert_eq!(sink.matches[1].match_spans, vec![(0, 3)]);

        // Test RegexMatcher
        let regex_matcher = RegexMatcher::new(r"foo\w*", false).unwrap();
        let mut re_sink = VecMatchSink::default();
        let re_count = regex_matcher.find_matches(data, &mut re_sink).unwrap();
        assert_eq!(re_count, 4);
        assert_eq!(re_sink.matches.len(), 2);
        assert_eq!(re_sink.matches[0].line_number, 1);
        assert_eq!(
            re_sink.matches[0].match_spans,
            vec![(0, 3), (8, 11), (16, 19)]
        );
    }

    #[test]
    fn test_extract_printable_strings() {
        let buffer =
            b"\x7fELF\x02\x01\x01\x00Hello World\x00\x00\x01\x02short\x00\xffGoodString123\x00";
        let extracted_4 = extract_printable_strings(buffer, 4);
        assert_eq!(extracted_4.len(), 3);
        assert_eq!(extracted_4[0].1, b"Hello World");
        assert_eq!(extracted_4[1].1, b"short");
        assert_eq!(extracted_4[2].1, b"GoodString123");

        let extracted_3 = extract_printable_strings(buffer, 3);
        assert_eq!(extracted_3.len(), 4);
        assert_eq!(extracted_3[0].1, b"ELF");
        assert_eq!(extracted_3[0].0, 1);
    }

    #[test]
    fn test_all_lines_matcher() {
        let buffer = b"line 1\n\nline 3\nline 4 without newline";
        let matcher = AllLinesMatcher;
        let mut sink = VecMatchSink::default();
        let count = matcher.find_matches(buffer, &mut sink).unwrap();
        assert_eq!(count, 3); // skips empty line 2
        assert_eq!(sink.matches.len(), 3);
        assert_eq!(sink.matches[0].line_number, 1);
        assert_eq!(sink.matches[0].line_bytes, b"line 1");
        assert_eq!(sink.matches[1].line_number, 3);
        assert_eq!(sink.matches[1].line_bytes, b"line 3");
        assert_eq!(sink.matches[2].line_number, 4);
        assert_eq!(sink.matches[2].line_bytes, b"line 4 without newline");
    }

    #[test]
    fn test_boolean_engine_precompiled_spans() {
        let buffer = b"first line with auth and token here\nsecond without key\n";
        let expr = QueryExpr::And(
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "auth".to_string(),
                case_sensitive: Some(true),
            })),
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "token".to_string(),
                case_sensitive: Some(true),
            })),
        );
        let engine = BooleanEngine::new(expr);
        let mut sink = VecMatchSink::default();
        let count = engine.find_matches(buffer, &mut sink).unwrap();
        assert_eq!(count, 1);
        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.matches[0].line_number, 1);
        // Verify exact span locations of individual matching tokens
        assert_eq!(sink.matches[0].match_spans, vec![(16, 20), (25, 30)]);
    }

    #[test]
    fn test_boolean_engine_proximity_matching() {
        let buffer = b"line 1: header\nline 2: // SAFETY: valid pointer\nline 3: unsafe { *ptr }\nline 4: other code\nline 5: unsafe { *other }\nline 6: footer\n";
        let expr = QueryExpr::Pattern(SearchPattern::Literal {
            text: "unsafe".to_string(),
            case_sensitive: Some(true),
        });

        // 1. Positive near: within 1 line of "SAFETY"
        // Line 3 is within 1 line of line 2 ("SAFETY").
        // Line 5 is NOT within 1 line of "SAFETY".
        let prox_filter = crate::dsl::ProximityFilter {
            term: "SAFETY".to_string(),
            window: 1,
            inverted: false,
        };
        let engine_pos = BooleanEngine::new(expr.clone()).with_proximity_filters(&[prox_filter]);
        let mut sink_pos = VecMatchSink::default();
        let count_pos = engine_pos.find_matches(buffer, &mut sink_pos).unwrap();
        assert_eq!(count_pos, 1);
        assert_eq!(sink_pos.matches.len(), 1);
        assert_eq!(sink_pos.matches[0].line_number, 3);

        // 2. Inverted no-near: NOT within 1 line of "SAFETY"
        // Line 5 is NOT near SAFETY (window = 1), so it matches!
        // Line 3 IS near SAFETY, so it is rejected!
        let prox_inv = crate::dsl::ProximityFilter {
            term: "SAFETY".to_string(),
            window: 1,
            inverted: true,
        };
        let engine_inv = BooleanEngine::new(expr).with_proximity_filters(&[prox_inv]);
        let mut sink_inv = VecMatchSink::default();
        let count_inv = engine_inv.find_matches(buffer, &mut sink_inv).unwrap();
        assert_eq!(count_inv, 1);
        assert_eq!(sink_inv.matches.len(), 1);
        assert_eq!(sink_inv.matches[0].line_number, 5);
    }

    #[test]
    fn test_compiled_expr_empty_set_and_inversion() {
        let empty = CompiledExpr::EmptySet;
        assert!(!empty.eval_on_line(b"hello world"));
        assert!(!empty.eval_on_line(b""));

        let inverted = CompiledExpr::Not(Box::new(CompiledExpr::EmptySet));
        assert!(inverted.eval_on_line(b"hello world"));
        assert!(inverted.eval_on_line(b""));
    }

    #[test]
    fn test_simd_literal_multiline_pattern_bounds_clamped() {
        let buffer = b"prefix foo\nbar suffix\n";
        let matcher = SimdLiteralMatcher::new("foo\nbar", true);
        let mut sink = VecMatchSink::default();
        let count = matcher.find_matches(buffer, &mut sink).unwrap();
        assert_eq!(count, 1);
        assert_eq!(sink.matches.len(), 1);
        assert_eq!(sink.matches[0].line_number, 1);
        let (start, end) = sink.matches[0].match_spans[0];
        // Line 1 is "prefix foo", so relative end must be clamped to line length (10), not 7 + 7 = 14
        assert_eq!(&sink.matches[0].line_bytes[start..end], b"foo");
    }

    #[test]
    fn test_collect_spans_for_case_insensitive_ascii_literals() {
        let expr = CompiledExpr::Literal(SimdLiteralMatcher::new("error", false));
        let line = b"Found ERROR and another Error here";
        let mut spans = Vec::new();
        expr.collect_spans(line, &mut spans);
        assert_eq!(spans, vec![(6, 11), (24, 29)]);

        // Boolean AND composition
        let and_expr = CompiledExpr::And(
            Box::new(CompiledExpr::Literal(SimdLiteralMatcher::new("foo", false))),
            Box::new(CompiledExpr::Literal(SimdLiteralMatcher::new("bar", false))),
        );
        let and_line = b"FOO meets BAR in foo";
        let mut and_spans = Vec::new();
        and_expr.collect_spans(and_line, &mut and_spans);
        and_spans.sort_unstable();
        and_spans.dedup();
        assert_eq!(and_spans, vec![(0, 3), (10, 13), (17, 20)]);
    }
}
