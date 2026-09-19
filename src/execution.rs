//! Buffer search shared by filesystem inputs and standard input.
use crate::core::{MatchRecord, MatchSink, Matcher};
use crate::search::{expand_context_lines, extract_printable_strings, is_binary_with_probe_size};
use std::io;

#[derive(Clone, Copy, Default)]
pub struct SearchOptions {
    pub max_count: Option<usize>,
    pub before_context: usize,
    pub after_context: usize,
    pub skip_binary: bool,
    pub only_binary: bool,
    pub binary_strings_min_len: Option<usize>,
    pub binary_null_probe_bytes: usize,
}

pub struct SearchResult<'a> {
    pub records: Vec<MatchRecord<'a>>,
    pub matched_lines: usize,
    pub matches: usize,
}

/// Borrow a matcher so callers can share compiled expressions across workers.
pub struct BufferSearch<'m> {
    pub matcher: &'m dyn Matcher,
    pub options: SearchOptions,
}

struct LimitedCollector<'a> {
    records: Vec<MatchRecord<'a>>,
    limit: Option<usize>,
    stopped: bool,
}

impl<'a> MatchSink<'a> for LimitedCollector<'a> {
    fn on_match(&mut self, record: MatchRecord<'a>) -> io::Result<()> {
        self.records.push(record);
        if self.limit.is_some_and(|limit| self.records.len() >= limit) {
            self.stopped = true;
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "match limit reached",
            ));
        }
        Ok(())
    }
}

impl<'m> BufferSearch<'m> {
    fn collect<'a>(
        &self,
        bytes: &'a [u8],
        limit: Option<usize>,
    ) -> io::Result<Vec<MatchRecord<'a>>> {
        let mut sink = LimitedCollector {
            records: Vec::new(),
            limit,
            stopped: false,
        };
        if limit != Some(0) {
            match self.matcher.find_matches(bytes, &mut sink) {
                Err(err) if !(sink.stopped && err.kind() == io::ErrorKind::Interrupted) => {
                    return Err(err);
                }
                _ => {}
            }
        }
        Ok(sink.records)
    }

    /// None means the input was excluded by binary policy, not searched without matches.
    pub fn search<'a>(&self, bytes: &'a [u8]) -> io::Result<Option<SearchResult<'a>>> {
        let probe_size = if self.options.binary_null_probe_bytes > 0 {
            self.options.binary_null_probe_bytes
        } else {
            1024
        };
        let binary = is_binary_with_probe_size(bytes, probe_size);
        if (self.options.skip_binary && binary) || (self.options.only_binary && !binary) {
            return Ok(None);
        }
        let mut records =
            if let Some(min_len) = self.options.binary_strings_min_len.filter(|_| binary) {
                let mut records = Vec::new();
                for (offset, string) in extract_printable_strings(bytes, min_len) {
                    let remaining = self
                        .options
                        .max_count
                        .map(|n| n.saturating_sub(records.len()));
                    if remaining == Some(0) {
                        break;
                    }
                    for mut record in self.collect(string, remaining)? {
                        // Extracted strings are logical output records rather
                        // than source lines. Give each one a stable ordinal so
                        // global head/tail selection does not collapse every
                        // binary string onto the matcher's synthetic line 1.
                        record.line_number = records.len() + 1;
                        record.line_byte_offset += offset;
                        records.push(record);
                    }
                }
                records
            } else {
                self.collect(bytes, self.options.max_count)?
            };
        let matched_lines = records.len();
        let matches = records.iter().map(|r| r.match_spans.len().max(1)).sum();
        if matched_lines > 0
            && !binary
            && (self.options.before_context > 0 || self.options.after_context > 0)
        {
            records = expand_context_lines(
                bytes,
                &records,
                self.options.before_context,
                self.options.after_context,
            );
        }
        Ok(Some(SearchResult {
            records,
            matched_lines,
            matches,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::MatchSink;
    use crate::search::SimdLiteralMatcher;

    struct InvertedStubMatcher;
    impl Matcher for InvertedStubMatcher {
        fn find_matches<'a>(
            &self,
            _buffer: &'a [u8],
            sink: &mut dyn MatchSink<'a>,
        ) -> io::Result<usize> {
            // Emulates an inverted match where line has 0 sub-line match spans
            sink.on_match(MatchRecord::new(1, 0, b"inverted line", Vec::new()))?;
            Ok(1)
        }
    }

    #[test]
    fn test_buffer_search_inverted_empty_spans_counts_as_match() {
        let matcher = InvertedStubMatcher;
        let searcher = BufferSearch {
            matcher: &matcher,
            options: SearchOptions::default(),
        };
        let result = searcher.search(b"inverted line\n").unwrap().unwrap();
        assert_eq!(result.matched_lines, 1);
        assert_eq!(result.matches, 1);
    }

    #[test]
    fn binary_string_matches_receive_unique_record_ordinals() {
        let matcher = SimdLiteralMatcher::new("hit", true);
        let searcher = BufferSearch {
            matcher: &matcher,
            options: SearchOptions {
                binary_strings_min_len: Some(4),
                ..SearchOptions::default()
            },
        };

        let result = searcher
            .search(b"hit-one\0hit-two\0hit-three\0")
            .unwrap()
            .unwrap();
        let ordinals: Vec<_> = result.records.iter().map(|r| r.line_number).collect();
        assert_eq!(ordinals, vec![1, 2, 3]);
    }
}
