use memchr::{memchr, memrchr};

#[inline]
pub fn is_binary_with_probe_size(bytes: &[u8], probe_size: usize) -> bool {
    let probe_len = bytes.len().min(probe_size);
    memchr(0x00, &bytes[..probe_len]).is_some()
}

#[inline]
pub fn is_binary(bytes: &[u8]) -> bool {
    is_binary_with_probe_size(bytes, 1024)
}

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

#[inline]
pub fn find_line_bounds(
    bytes: &[u8],
    match_offset: usize,
    last_newline: usize,
    current_line: usize,
) -> (usize, usize, usize) {
    let line_start = match memrchr(b'\n', &bytes[..match_offset]) {
        Some(pos) => pos + 1,
        None => 0,
    };

    let line_num = if line_start > last_newline {
        let count = memchr::Memchr::new(b'\n', &bytes[last_newline..line_start]).count();
        current_line + count
    } else {
        current_line
    };

    let line_end = match memchr(b'\n', &bytes[match_offset..]) {
        Some(pos) => match_offset + pos,
        None => bytes.len(),
    };

    let clean_end = if line_end > line_start && bytes[line_end - 1] == b'\r' {
        line_end - 1
    } else {
        line_end
    };

    (line_start, clean_end, line_num)
}

pub(crate) struct Lines<'a> {
    bytes: &'a [u8],
    newlines: memchr::Memchr<'a>,
    offset: usize,
    number: usize,
}

impl<'a> Lines<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
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

pub mod boolean;
pub mod context;
pub mod fuzzy;
pub mod matcher;

pub use boolean::*;
pub use context::*;
pub use matcher::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{MatchRecord, Matcher};
    use crate::dsl::{QueryExpr, SearchPattern};
    use std::io;

    fn exact(text: &str) -> QueryExpr {
        QueryExpr::Pattern(SearchPattern::ExactMatch {
            text: text.into(),
            case_sensitive: None,
        })
    }

    #[test]
    fn exact_targets_whole_lines_and_boolean_branches() {
        let expr = QueryExpr::Or(Box::new(exact("codex")), Box::new(exact("memory")));
        let compiled = CompiledExpr::from_query_expr(&expr).unwrap();
        assert!(compiled.eval_on_line(b"codex"));
        assert!(compiled.eval_on_line(b"memory"));
        assert!(!compiled.eval_on_line(b"prefix codex"));
        assert!(!compiled.eval_on_line(b"codex "));
        let mut spans = Vec::new();
        compiled.collect_spans(b"codex", &mut spans);
        assert_eq!(spans, vec![(0, 5)]);
        let sensitive =
            CompiledExpr::from_query_expr_with_case(&exact("école"), Some(true)).unwrap();
        let insensitive =
            CompiledExpr::from_query_expr_with_case(&exact("école"), Some(false)).unwrap();
        assert!(!sensitive.eval_on_line("École".as_bytes()));
        assert!(insensitive.eval_on_line("École".as_bytes()));
        assert!(!insensitive.eval_on_line("École école".as_bytes()));
        let literal = CompiledExpr::from_query_expr(&QueryExpr::Pattern(
            SearchPattern::ExactLiteral("codex".into()),
        ))
        .unwrap();
        assert!(literal.eval_on_line(b"prefix codex suffix"));
    }

    #[test]
    fn fuzzy_boolean_scores_only_matching_positive_branches() {
        let fuzzy = QueryExpr::Pattern(SearchPattern::Fuzzy {
            text: "mmry".into(),
            case_sensitive: None,
        });
        let positive = CompiledExpr::from_query_expr(&fuzzy).unwrap();
        assert!(positive.has_fuzzy());
        let score = positive.score_on_line(b"memory").unwrap();
        let and = CompiledExpr::from_query_expr(&QueryExpr::And(
            Box::new(fuzzy.clone()),
            Box::new(exact("memory")),
        ))
        .unwrap();
        assert_eq!(and.score_on_line(b"memory"), Some(score));
        assert_eq!(and.score_on_line(b"memory elsewhere"), None);
        let not = CompiledExpr::from_query_expr(&QueryExpr::Not(Box::new(fuzzy))).unwrap();
        assert!(!not.has_fuzzy());
        assert_eq!(not.score_on_line(b"other"), Some(0));
        let mut spans = Vec::new();
        not.collect_spans(b"other", &mut spans);
        assert!(spans.is_empty());
        let override_sensitive = CompiledExpr::from_query_expr_with_case(
            &QueryExpr::Pattern(SearchPattern::Fuzzy {
                text: "mmry".into(),
                case_sensitive: None,
            }),
            Some(true),
        )
        .unwrap();
        assert!(!override_sensitive.eval_on_line(b"MEMORY"));
    }

    #[test]
    fn fuzzy_occurrence_counts_do_not_count_highlight_fragments() {
        let fuzzy = QueryExpr::Pattern(SearchPattern::Fuzzy {
            text: "mmry".into(),
            case_sensitive: None,
        });
        let matcher = BooleanEngine::try_new(fuzzy.clone()).unwrap();
        let search = crate::execution::BufferSearch {
            matcher: &matcher,
            options: crate::execution::SearchOptions::default(),
        };
        let result = search.search(b"memory\nmemory\n").unwrap().unwrap();
        assert_eq!(result.matched_lines, 2);
        assert_eq!(result.matches, 2);
        assert_eq!(result.records[0].match_spans, vec![(0, 1), (2, 3), (4, 6)]);
        assert_eq!(result.records[0].occurrence_count, 1);
        assert_eq!(result.records[0].to_owned().occurrence_count, 1);

        let mixed = QueryExpr::Or(
            Box::new(QueryExpr::Pattern(SearchPattern::Fuzzy {
                text: "mry".into(),
                case_sensitive: None,
            })),
            Box::new(QueryExpr::Pattern(SearchPattern::ExactLiteral(
                "memory".into(),
            ))),
        );
        let matcher = BooleanEngine::try_new(mixed).unwrap();
        let search = crate::execution::BufferSearch {
            matcher: &matcher,
            options: crate::execution::SearchOptions {
                before_context: 1,
                after_context: 1,
                ..Default::default()
            },
        };
        let result = search
            .search(b"before\nmemory memory\nafter\n")
            .unwrap()
            .unwrap();
        assert_eq!(result.matched_lines, 1);
        assert_eq!(result.matches, 2);
        let record = &result.records[1];
        assert!(record.match_spans.len() > record.occurrence_count);
        assert_eq!(record.occurrence_count, 2);
        assert_eq!(result.records[0].occurrence_count, 0);
        assert_eq!(result.records[2].occurrence_count, 0);
    }

    #[test]
    fn boolean_regex_capture_highlights_are_not_distinct_occurrences() {
        let expr = QueryExpr::And(
            Box::new(QueryExpr::Pattern(SearchPattern::Regex(
                "(foo)(bar)".into(),
            ))),
            Box::new(QueryExpr::Pattern(SearchPattern::ExactLiteral(
                "baz".into(),
            ))),
        );
        let matcher = BooleanEngine::try_new(expr).unwrap();
        let search = crate::execution::BufferSearch {
            matcher: &matcher,
            options: Default::default(),
        };
        let result = search.search(b"foobar baz\n").unwrap().unwrap();
        assert_eq!(result.matches, 2);
        assert_eq!(result.records[0].match_spans, vec![(0, 3), (3, 6), (7, 10)]);
        assert_eq!(result.records[0].occurrence_count, 2);

        let inverted = BooleanEngine::try_new(QueryExpr::Not(Box::new(exact("absent")))).unwrap();
        let mut sink = VecMatchSink::default();
        inverted.find_matches(b"selected\n", &mut sink).unwrap();
        assert_eq!(sink.matches[0].occurrence_count, 1);
        assert!(sink.matches[0].match_spans.is_empty());
    }

    #[test]
    fn ordinary_context_expansion_preserves_primary_provenance() {
        let query = crate::dsl::DslParser::parse(["unsafe", "near:2,SAFETY"]).unwrap();
        let matcher = BooleanEngine::try_new(query.expr.unwrap())
            .unwrap()
            .with_proximity_filters(&query.proximity_filters);
        let bytes = b"outside\nSAFETY\nbridge\nunsafe\nafter\n";
        let search = crate::execution::BufferSearch {
            matcher: &matcher,
            options: crate::execution::SearchOptions {
                before_context: 1,
                after_context: 1,
                has_proximity: true,
                ..Default::default()
            },
        };
        let result = search.search(bytes).unwrap().unwrap();

        assert_eq!(
            result
                .records
                .iter()
                .map(|record| record.line_number)
                .collect::<Vec<_>>(),
            vec![2, 3, 4, 5]
        );
        let selected = std::collections::BTreeSet::from([4]);
        let mut owned: Vec<_> = result
            .records
            .iter()
            .map(|record| record.to_owned())
            .collect();
        owned.retain_mut(|record| record.retain_for_primary(&selected));
        assert_eq!(owned.len(), 4);
        assert!(
            owned
                .iter()
                .filter(|record| record.is_context)
                .all(|record| record.occurrence_count == 0)
        );
        assert!(
            owned
                .iter()
                .flat_map(|record| &record.context_origins)
                .all(|origin| origin.primary_line == 4)
        );
    }

    #[test]
    fn literal_occurrence_counts_follow_appended_spans() {
        let mut sink = VecMatchSink::default();
        SimdLiteralMatcher::new("foo", true)
            .find_matches(b"foo foo foo\n", &mut sink)
            .unwrap();
        assert_eq!(sink.matches[0].occurrence_count, 3);
    }

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

        let matcher = SimdLiteralMatcher::with_smart_case("connect", true);
        let mut sink = VecMatchSink::default();
        let count = matcher.find_matches(data, &mut sink).unwrap();
        assert_eq!(count, 2);
        assert_eq!(sink.matches[0].line_number, 1);
        assert_eq!(sink.matches[1].line_number, 2);

        let matcher_upper = SimdLiteralMatcher::with_smart_case("CONNECT", true);
        let mut sink_upper = VecMatchSink::default();
        let count_upper = matcher_upper.find_matches(data, &mut sink_upper).unwrap();
        assert_eq!(count_upper, 1);
        assert_eq!(sink_upper.matches[0].line_number, 2);
    }

    #[test]
    fn test_simd_literal_case_insensitive_with_uppercase_query() {
        let data = b"line 1: simple test\nline 2: Simple test\nline 3: SIMPLE test\n";

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

        let hex_pat = vec![Some(0x48), None, Some(0xE5)];
        let matcher = HexMatcher::new(hex_pat);
        let mut sink = VecMatchSink::default();
        let count = matcher.find_matches(data, &mut sink).unwrap();

        assert_eq!(count, 1);
        assert_eq!(sink.matches[0].line_byte_offset, 8);
        assert_eq!(sink.matches[0].line_number, 9);

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

        let literal_matcher = SimdLiteralMatcher::new("foo", true);
        let mut sink = VecMatchSink::default();
        let count = literal_matcher.find_matches(data, &mut sink).unwrap();
        assert_eq!(count, 4);
        assert_eq!(sink.matches.len(), 2);
        assert_eq!(sink.matches[0].line_number, 1);
        assert_eq!(sink.matches[0].match_spans, vec![(0, 3), (8, 11), (16, 19)]);
        assert_eq!(sink.matches[1].line_number, 3);
        assert_eq!(sink.matches[1].match_spans, vec![(0, 3)]);

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
        assert_eq!(count, 3);
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

        assert_eq!(sink.matches[0].match_spans, vec![(16, 20), (25, 30)]);
    }

    #[test]
    fn test_boolean_engine_proximity_matching() {
        let buffer = b"line 1: header\nline 2: // SAFETY: valid pointer\nline 3: unsafe { *ptr }\nline 4: other code\nline 5: unsafe { *other }\nline 6: footer\n";
        let expr = QueryExpr::Pattern(SearchPattern::Literal {
            text: "unsafe".to_string(),
            case_sensitive: Some(true),
        });

        let prox_filter = crate::dsl::ProximityFilter {
            term: "SAFETY".to_string(),
            window: 1,
            inverted: false,
        };
        let engine_pos = BooleanEngine::new(expr.clone()).with_proximity_filters(&[prox_filter]);
        let mut sink_pos = VecMatchSink::default();
        let count_pos = engine_pos.find_matches(buffer, &mut sink_pos).unwrap();
        assert_eq!(count_pos, 1);
        assert_eq!(sink_pos.matches.len(), 2);
        assert_eq!(sink_pos.matches[0].line_number, 2);
        assert!(!sink_pos.matches[0].near_spans.is_empty());
        assert_eq!(sink_pos.matches[1].line_number, 3);
        assert!(!sink_pos.matches[1].match_spans.is_empty());

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

        assert_eq!(&sink.matches[0].line_bytes[start..end], b"foo");
    }

    #[test]
    fn test_collect_spans_for_case_insensitive_ascii_literals() {
        let expr = CompiledExpr::Literal(SimdLiteralMatcher::new("error", false));
        let line = b"Found ERROR and another Error here";
        let mut spans = Vec::new();
        expr.collect_spans(line, &mut spans);
        assert_eq!(spans, vec![(6, 11), (24, 29)]);

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
