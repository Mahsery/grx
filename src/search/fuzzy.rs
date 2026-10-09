use memchr::{memchr, memchr2};

const MATCH: i32 = 16;
const GAP_OPEN: i32 = 3;
const GAP_EXTEND: i32 = 1;
const CONSECUTIVE: i32 = 4;

#[derive(Clone, Copy)]
struct State {
    score: i32,
    end: usize,
    start: usize,
    previous_end: Option<usize>,
}

pub struct FuzzyMatcher {
    pattern: Vec<char>,
    ascii_pattern: Option<Vec<u8>>,
    case_sensitive: bool,
}

impl FuzzyMatcher {
    pub fn new(pattern: &str, case_sensitive: bool) -> Self {
        Self {
            pattern: pattern.chars().collect(),
            ascii_pattern: pattern.is_ascii().then(|| {
                if case_sensitive {
                    pattern.as_bytes().to_vec()
                } else {
                    pattern.to_ascii_lowercase().into_bytes()
                }
            }),
            case_sensitive,
        }
    }

    pub fn score(&self, bytes: &[u8]) -> Option<i32> {
        let text = std::str::from_utf8(bytes).ok()?;
        if self.pattern.is_empty() {
            return Some(0);
        }
        if !self.is_subsequence(text) {
            return None;
        }
        self.best(text, self.pattern.len(), false)
            .map(|state| state.score)
    }

    pub fn match_range(&self, bytes: &[u8]) -> Option<(usize, usize)> {
        let text = std::str::from_utf8(bytes).ok()?;
        if self.pattern.is_empty() {
            return Some((0, 0));
        }
        if !self.is_subsequence(text) {
            return None;
        }
        let state = self.best(text, self.pattern.len(), false)?;
        Some((
            state.start,
            state.end + text[state.end..].chars().next()?.len_utf8(),
        ))
    }

    pub fn collect_spans(&self, bytes: &[u8], spans: &mut Vec<(usize, usize)>) {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return;
        };
        if self.pattern.is_empty() || !self.is_subsequence(text) {
            return;
        }
        let Some(mut state) = self.best(text, self.pattern.len(), false) else {
            return;
        };
        let mut selected = Vec::with_capacity(self.pattern.len());
        for length in (1..=self.pattern.len()).rev() {
            let start = state.end;
            let end = start + text[start..].chars().next().unwrap().len_utf8();
            selected.push((start, end));
            if let Some(previous) = state.previous_end {
                let prefix_end = previous + text[previous..].chars().next().unwrap().len_utf8();
                state = self.best(&text[..prefix_end], length - 1, true).unwrap();
            }
        }
        selected.reverse();
        for (start, end) in selected {
            if let Some(last) = spans.last_mut().filter(|last| last.1 == start) {
                last.1 = end;
            } else {
                spans.push((start, end));
            }
        }
    }

    fn is_subsequence(&self, text: &str) -> bool {
        if self.pattern.is_empty() {
            return true;
        }

        if let Some(pattern) = &self.ascii_pattern
            && text.is_ascii()
        {
            let mut remaining = text.as_bytes();
            for &byte in pattern {
                let upper = byte.to_ascii_uppercase();
                let position = if !self.case_sensitive && byte != upper {
                    memchr2(byte, upper, remaining)
                } else {
                    memchr(byte, remaining)
                };
                let Some(position) = position else {
                    return false;
                };
                remaining = &remaining[position + 1..];
            }
            return true;
        }
        let mut next = 0;
        for ch in text.chars() {
            if chars_equal(self.pattern[next], ch, self.case_sensitive) {
                next += 1;
                if next == self.pattern.len() {
                    return true;
                }
            }
        }
        false
    }

    fn best(&self, text: &str, length: usize, endpoint: bool) -> Option<State> {
        let mut adjacent = vec![None::<State>; length];
        let mut gaps = vec![None::<State>; length];
        let mut best = None;
        let mut previous_char: Option<char> = None;
        for (offset, ch) in text.char_indices() {
            let bonus = match previous_char {
                None => 8,
                Some(prev) if !prev.is_alphanumeric() => 8,
                Some(prev) if prev.is_lowercase() && ch.is_uppercase() => 7,
                Some(prev) if prev.is_alphabetic() && ch.is_numeric() => 7,
                _ => 0,
            };
            for index in (0..length).rev() {
                let old_adjacent = adjacent[index];
                let old_gap = gaps[index];
                let mut matched = None;
                if chars_equal(self.pattern[index], ch, self.case_sensitive) {
                    if index == 0 {
                        matched = Some(State {
                            score: MATCH + bonus * 2,
                            end: offset,
                            start: offset,
                            previous_end: None,
                        });
                    } else {
                        for (prior, consecutive) in
                            [(adjacent[index - 1], true), (gaps[index - 1], false)]
                        {
                            if let Some(prior) = prior {
                                let reward = if consecutive {
                                    bonus.max(CONSECUTIVE)
                                } else {
                                    bonus
                                };
                                let state = State {
                                    score: prior.score.saturating_add(MATCH + reward),
                                    end: offset,
                                    start: prior.start,
                                    previous_end: Some(prior.end),
                                };
                                choose(&mut matched, state);
                            }
                        }
                    }
                }
                adjacent[index] = matched;
                let mut next_gap = None;
                if let Some(mut state) = old_gap {
                    state.score = state.score.saturating_sub(GAP_EXTEND);
                    choose(&mut next_gap, state);
                }
                if let Some(mut state) = old_adjacent {
                    state.score = state.score.saturating_sub(GAP_OPEN);
                    choose(&mut next_gap, state);
                }
                gaps[index] = next_gap;
            }
            if let Some(state) = adjacent[length - 1] {
                choose(&mut best, state);
            }
            previous_char = Some(ch);
        }
        if endpoint { adjacent[length - 1] } else { best }
    }
}

fn choose(best: &mut Option<State>, candidate: State) {
    if best.is_none_or(|current| {
        candidate.score > current.score
            || (candidate.score == current.score && candidate.end < current.end)
    }) {
        *best = Some(candidate);
    }
}

pub(crate) fn chars_equal(a: char, b: char, case_sensitive: bool) -> bool {
    a == b || (!case_sensitive && a.to_lowercase().eq(b.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_subsequence_not_permutation_or_typo_matching() {
        assert!(FuzzyMatcher::new("mmry", false).score(b"memory").is_some());
        assert!(
            FuzzyMatcher::new("fzf", false)
                .score(b"fuzzy finder")
                .is_some()
        );
        assert!(FuzzyMatcher::new("ym", false).score(b"memory").is_none());
        assert!(
            FuzzyMatcher::new("memroy", false)
                .score(b"memory")
                .is_none()
        );
    }

    #[test]
    fn selects_best_alignment_instead_of_first_alignment() {
        let matcher = FuzzyMatcher::new("abc", true);
        let mut spans = Vec::new();
        matcher.collect_spans(b"a_____b_____c abc", &mut spans);
        assert_eq!(spans, vec![(14, 17)]);
        assert!(matcher.score(b"abc").unwrap() > matcher.score(b"axbyc").unwrap());
        assert!(
            FuzzyMatcher::new("ff", true)
                .score(b"fuzzy-finder")
                .unwrap()
                > FuzzyMatcher::new("ff", true).score(b"fuzzyfinder").unwrap()
        );
    }

    #[test]
    fn unicode_case_and_utf8_spans() {
        let matcher = FuzzyMatcher::new("éλ", false);
        let mut spans = Vec::new();
        matcher.collect_spans("É-x-Λ".as_bytes(), &mut spans);
        assert_eq!(spans, vec![(0, 2), (5, 7)]);
        assert!(
            FuzzyMatcher::new("éλ", true)
                .score("É-x-Λ".as_bytes())
                .is_none()
        );
        assert!(matcher.score(&[0xff]).is_none());
    }

    #[test]
    fn dynamic_programming_agrees_with_exhaustive_alignments() {
        fn alignment_score(text: &[u8], positions: &[usize]) -> i32 {
            let mut score = 0;
            for (index, &position) in positions.iter().enumerate() {
                let bonus = if position == 0 || text[position - 1] == b'-' {
                    8
                } else {
                    0
                };
                score += 16;
                if index == 0 {
                    score += bonus * 2;
                } else {
                    let gap = position - positions[index - 1] - 1;
                    if gap == 0 {
                        score += bonus.max(4);
                    } else {
                        score += bonus - 3 - (gap as i32 - 1);
                    }
                }
            }
            score
        }
        fn enumerate(
            text: &[u8],
            pattern: &[u8],
            positions: &mut Vec<usize>,
            best: &mut Option<i32>,
        ) {
            if positions.len() == pattern.len() {
                let score = alignment_score(text, positions);
                *best = Some(best.map_or(score, |previous| previous.max(score)));
                return;
            }
            let start = positions.last().map_or(0, |last| last + 1);
            for position in start..text.len() {
                if text[position] == pattern[positions.len()] {
                    positions.push(position);
                    enumerate(text, pattern, positions, best);
                    positions.pop();
                }
            }
        }
        for length in 0..=6 {
            for mut encoded in 0..3usize.pow(length) {
                let mut text = vec![b'-'; length as usize];
                for ch in &mut text {
                    *ch = b"ab-"[encoded % 3];
                    encoded /= 3;
                }
                for pattern in ["ab", "aa", "aba"] {
                    let mut expected = None;
                    enumerate(&text, pattern.as_bytes(), &mut Vec::new(), &mut expected);
                    let matcher = FuzzyMatcher::new(pattern, true);
                    assert_eq!(matcher.score(&text), expected, "{text:?}, {pattern}");
                    let mut spans = Vec::new();
                    matcher.collect_spans(&text, &mut spans);
                    if let Some(expected) = expected {
                        let positions: Vec<_> = spans
                            .into_iter()
                            .flat_map(|(start, end)| start..end)
                            .collect();
                        assert_eq!(alignment_score(&text, &positions), expected);
                    } else {
                        assert!(spans.is_empty());
                    }
                }
            }
        }
    }

    #[test]
    fn ascii_feasibility_matches_scalar_reference() {
        fn scalar(pattern: &str, text: &str, sensitive: bool) -> bool {
            let mut chars = text.chars();
            pattern
                .chars()
                .all(|needle| chars.any(|ch| chars_equal(needle, ch, sensitive)))
        }
        let alphabet = b"aAbB-?";
        for sensitive in [false, true] {
            for pattern in ["", "ab", "AA", "a-a", "?-", "aa", "Z"] {
                let matcher = FuzzyMatcher::new(pattern, sensitive);
                for length in 0..=5 {
                    for mut encoded in 0..alphabet.len().pow(length) {
                        let mut text = vec![b'-'; length as usize];
                        for ch in &mut text {
                            *ch = alphabet[encoded % alphabet.len()];
                            encoded /= alphabet.len();
                        }
                        let text = std::str::from_utf8(&text).unwrap();
                        assert_eq!(
                            matcher.is_subsequence(text),
                            scalar(pattern, text, sensitive),
                            "{pattern:?}, {text:?}, sensitive={sensitive}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn ascii_patterns_keep_unicode_candidate_folding() {
        let matcher = FuzzyMatcher::new("kk", false);
        let mut spans = Vec::new();
        matcher.collect_spans("K-k".as_bytes(), &mut spans);
        assert_eq!(spans, vec![(0, 3), (4, 5)]);
        assert!(matcher.score("K-k".as_bytes()).is_some());
        assert!(
            FuzzyMatcher::new("kk", true)
                .score("K-k".as_bytes())
                .is_none()
        );
    }

    #[test]
    fn empty_and_long_candidates_have_defined_behavior() {
        assert_eq!(FuzzyMatcher::new("", true).score(b"anything"), Some(0));
        assert_eq!(FuzzyMatcher::new("a", true).score(b""), None);
        let mut candidate = "x".repeat(100_000);
        candidate.push_str(" abc");
        let matcher = FuzzyMatcher::new("abc", true);
        assert!(matcher.score(candidate.as_bytes()).is_some());
        let mut spans = Vec::new();
        matcher.collect_spans(candidate.as_bytes(), &mut spans);
        assert_eq!(spans, vec![(100_001, 100_004)]);
    }
}
