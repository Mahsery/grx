use crate::core::{ContextOrigin, MatchRecord, MatchSink, Matcher};
use crate::dsl::{QueryExpr, SearchPattern};
use memchr::{memchr, memchr2};
use std::io;

use super::Lines;
use super::fuzzy::FuzzyMatcher;
use super::matcher::{ExactMatcher, HexMatcher, SimdLiteralMatcher};

#[allow(clippy::large_enum_variant)]
pub enum CompiledExpr {
    Literal(SimdLiteralMatcher),
    Exact(ExactMatcher),
    Fuzzy(FuzzyMatcher),
    Regex(regex::bytes::Regex),
    Hex(HexMatcher),
    EmptySet,
    And(Box<CompiledExpr>, Box<CompiledExpr>),
    Or(Box<CompiledExpr>, Box<CompiledExpr>),
    Not(Box<CompiledExpr>),
}

impl CompiledExpr {
    pub fn from_query_expr(expr: &QueryExpr) -> Result<Self, regex::Error> {
        Self::from_query_expr_with_case(expr, None)
    }

    pub fn from_query_expr_with_case(
        expr: &QueryExpr,
        case_override: Option<bool>,
    ) -> Result<Self, regex::Error> {
        Ok(match expr {
            QueryExpr::Pattern(p) => match p {
                SearchPattern::Literal { text, .. } => {
                    let is_cs = case_override.unwrap_or_else(|| p.is_case_sensitive());
                    CompiledExpr::Literal(SimdLiteralMatcher::new(text, is_cs))
                }
                SearchPattern::ExactLiteral(text) => {
                    let is_cs = case_override.unwrap_or(true);
                    CompiledExpr::Literal(SimdLiteralMatcher::new(text, is_cs))
                }
                SearchPattern::ExactMatch { text, .. } => CompiledExpr::Exact(ExactMatcher::new(
                    text,
                    case_override.unwrap_or_else(|| p.is_case_sensitive()),
                )),
                SearchPattern::Fuzzy { text, .. } => CompiledExpr::Fuzzy(FuzzyMatcher::new(
                    text,
                    case_override.unwrap_or_else(|| p.is_case_sensitive()),
                )),
                SearchPattern::Regex(pattern) => {
                    let mut builder = regex::bytes::RegexBuilder::new(pattern);
                    builder.multi_line(true);
                    if let Some(cs) = case_override {
                        builder.case_insensitive(!cs);
                    }
                    CompiledExpr::Regex(builder.build()?)
                }
                SearchPattern::Hex(bytes) => CompiledExpr::Hex(HexMatcher::new(bytes.clone())),
                SearchPattern::EmptySet => CompiledExpr::EmptySet,
            },
            QueryExpr::And(a, b) => CompiledExpr::And(
                Box::new(Self::from_query_expr_with_case(a, case_override)?),
                Box::new(Self::from_query_expr_with_case(b, case_override)?),
            ),
            QueryExpr::Or(a, b) => CompiledExpr::Or(
                Box::new(Self::from_query_expr_with_case(a, case_override)?),
                Box::new(Self::from_query_expr_with_case(b, case_override)?),
            ),
            QueryExpr::Not(a) => {
                CompiledExpr::Not(Box::new(Self::from_query_expr_with_case(a, case_override)?))
            }
        })
    }

    #[inline]
    pub fn eval_on_line(&self, line: &[u8]) -> bool {
        match self {
            CompiledExpr::Literal(m) => {
                m.matches_at(line) || find_substr_on_line(m, line).is_some()
            }
            CompiledExpr::Exact(m) => m.is_match(line),
            CompiledExpr::Fuzzy(m) => m.is_match(line),
            CompiledExpr::Regex(re) => re.is_match(line),
            CompiledExpr::Hex(m) => m.find_from(line, 0).is_some(),
            CompiledExpr::EmptySet => false,
            CompiledExpr::And(a, b) => a.eval_on_line(line) && b.eval_on_line(line),
            CompiledExpr::Or(a, b) => a.eval_on_line(line) || b.eval_on_line(line),
            CompiledExpr::Not(a) => !a.eval_on_line(line),
        }
    }

    fn required_anchor(&self) -> Option<AnchorRef<'_>> {
        match self {
            Self::Literal(m)
                if !m.needle.is_empty()
                    && !m.needle.contains(&b'\n')
                    && !m.needle.contains(&b'\r') =>
            {
                if let Some(ref finder) = m.finder {
                    Some(AnchorRef::Finder(finder, m.needle.len()))
                } else {
                    Some(AnchorRef::Literal(m))
                }
            }
            Self::Exact(m) => m
                .finder
                .as_ref()
                .map(|finder| AnchorRef::Finder(finder, m.text.len())),
            Self::Fuzzy(m) if m.has_ascii_anchor() => Some(AnchorRef::FuzzyAscii(m)),
            Self::And(a, b) => match (a.required_anchor(), b.required_anchor()) {
                (Some(left), Some(right)) => {
                    if right.priority() > left.priority() {
                        Some(right)
                    } else {
                        Some(left)
                    }
                }
                (Some(anchor), None) | (None, Some(anchor)) => Some(anchor),
                (None, None) => None,
            },
            _ => None,
        }
    }

    fn needs_separate_occurrence_count(&self) -> bool {
        match self {
            Self::Fuzzy(_) => true,
            Self::Regex(re) => re.captures_len() > 1,
            Self::And(a, b) | Self::Or(a, b) => {
                a.needs_separate_occurrence_count() || b.needs_separate_occurrence_count()
            }
            _ => false,
        }
    }

    pub fn score_on_line(&self, line: &[u8]) -> Option<i32> {
        match self {
            Self::Fuzzy(matcher) => matcher.score(line),
            Self::And(a, b) => Some(
                a.score_on_line(line)?
                    .saturating_add(b.score_on_line(line)?),
            ),
            Self::Or(a, b) => match (a.score_on_line(line), b.score_on_line(line)) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            },
            _ => self.eval_on_line(line).then_some(0),
        }
    }

    pub fn has_fuzzy(&self) -> bool {
        match self {
            Self::Fuzzy(_) => true,
            Self::And(a, b) | Self::Or(a, b) => a.has_fuzzy() || b.has_fuzzy(),
            _ => false,
        }
    }

    pub fn occurrence_count(&self, line: &[u8]) -> usize {
        if !self.eval_on_line(line) {
            return 0;
        }
        self.count_matched_occurrences(line)
    }

    fn count_matched_occurrences(&self, line: &[u8]) -> usize {
        if matches!(self, Self::Fuzzy(_)) {
            return 1;
        }
        let mut occurrences = Vec::new();
        self.collect_occurrences(line, &mut occurrences);
        occurrences.sort_unstable();
        occurrences.dedup();
        occurrences.len().max(1)
    }

    fn collect_occurrences(&self, line: &[u8], occurrences: &mut Vec<(usize, usize)>) {
        match self {
            Self::Fuzzy(matcher) => {
                if let Some(range) = matcher.match_range(line) {
                    occurrences.push(range);
                }
            }
            Self::Regex(regex) => {
                occurrences.extend(regex.find_iter(line).map(|m| (m.start(), m.end())))
            }
            Self::And(a, b) => {
                a.collect_occurrences(line, occurrences);
                b.collect_occurrences(line, occurrences);
            }
            Self::Or(a, b) => {
                if a.eval_on_line(line) {
                    a.collect_occurrences(line, occurrences);
                }
                if b.eval_on_line(line) {
                    b.collect_occurrences(line, occurrences);
                }
            }
            Self::Not(_) => {}
            _ => self.collect_spans(line, occurrences),
        }
    }

    pub fn collect_spans(&self, line: &[u8], spans: &mut Vec<(usize, usize)>) {
        match self {
            CompiledExpr::Literal(m) => m.collect_spans(line, spans),
            CompiledExpr::Exact(m) => {
                if m.is_match(line) {
                    spans.push((0, line.len()));
                }
            }
            CompiledExpr::Fuzzy(m) => m.collect_spans(line, spans),
            CompiledExpr::Regex(re) => {
                if re.captures_len() > 1 {
                    for caps in re.captures_iter(line) {
                        for i in 1..re.captures_len() {
                            if let Some(m) = caps.get(i) {
                                spans.push((m.start(), m.end()));
                            }
                        }
                    }
                } else {
                    spans.extend(re.find_iter(line).map(|m| (m.start(), m.end())));
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

#[derive(Clone, Copy)]
enum AnchorRef<'a> {
    Finder(&'a memchr::memmem::Finder<'static>, usize),
    Literal(&'a SimdLiteralMatcher),
    FuzzyAscii(&'a FuzzyMatcher),
}

impl<'a> AnchorRef<'a> {
    #[inline]
    fn priority(&self) -> (u8, usize) {
        match self {
            Self::Finder(_, len) => (3, *len),
            Self::Literal(m) => (2, m.needle.len()),
            Self::FuzzyAscii(_) => (1, 0),
        }
    }

    #[inline]
    fn find(&self, haystack: &[u8]) -> Option<usize> {
        match self {
            Self::Finder(finder, _) => finder.find(haystack),
            Self::Literal(matcher) => find_substr_on_line(matcher, haystack),
            Self::FuzzyAscii(matcher) => matcher.find_ascii_candidate_in_buffer(haystack),
        }
    }
}

pub struct CompiledProximityFilter {
    pub matcher: SimdLiteralMatcher,
    pub window: usize,
    pub inverted: bool,
}

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

    pub fn try_new(expr: QueryExpr) -> Result<Self, regex::Error> {
        Ok(Self {
            compiled: Ok(CompiledExpr::from_query_expr(&expr)?),
            proximity_filters: Vec::new(),
        })
    }

    pub fn with_proximity_filters(self, filters: &[crate::dsl::ProximityFilter]) -> Self {
        self.with_proximity_filters_case(filters, None)
    }

    pub fn with_proximity_filters_case(
        mut self,
        filters: &[crate::dsl::ProximityFilter],
        case: Option<bool>,
    ) -> Self {
        self.proximity_filters = filters
            .iter()
            .map(|f| {
                let matcher = SimdLiteralMatcher::new(
                    &f.term,
                    case.unwrap_or_else(|| f.term.chars().any(char::is_uppercase)),
                );
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
pub(crate) fn find_substr_on_line(matcher: &SimdLiteralMatcher, line: &[u8]) -> Option<usize> {
    if matcher.case_insensitive_regex.is_none() && line.len() < matcher.needle.len() {
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

        if self.proximity_filters.is_empty() {
            let custom_occurrences = compiled.needs_separate_occurrence_count();
            if let Some(anchor) = compiled.required_anchor() {
                let mut count = 0;
                let mut cursor = 0;
                let mut last_newline = 0;
                let mut current_line = 1;
                while cursor < buffer.len() {
                    let Some(match_rel) = anchor.find(&buffer[cursor..]) else {
                        break;
                    };
                    let match_idx = cursor + match_rel;
                    let (line_start, line_end, line_num) =
                        super::find_line_bounds(buffer, match_idx, last_newline, current_line);
                    let line = &buffer[line_start..line_end];

                    if compiled.eval_on_line(line) {
                        let mut spans = Vec::new();
                        compiled.collect_spans(line, &mut spans);
                        spans.sort_unstable();
                        spans.dedup();
                        let occ = if custom_occurrences {
                            compiled.count_matched_occurrences(line)
                        } else {
                            spans.len().max(1)
                        };
                        let mut record = MatchRecord::new(line_num, line_start, line, spans);
                        record.occurrence_count = occ;
                        sink.on_match(record)?;
                        count += 1;
                    }

                    let raw_end = if buffer.get(line_end) == Some(&b'\r') {
                        line_end + 1
                    } else {
                        line_end
                    };
                    if raw_end < buffer.len() {
                        cursor = raw_end + 1;
                        last_newline = cursor;
                        current_line = line_num + 1;
                    } else {
                        break;
                    }
                }
                return Ok(count);
            }

            let mut count = 0;
            for (number, offset, line) in Lines::new(buffer) {
                if compiled.eval_on_line(line) {
                    let mut spans = Vec::new();
                    compiled.collect_spans(line, &mut spans);
                    spans.sort_unstable();
                    spans.dedup();
                    let occ = if custom_occurrences {
                        compiled.count_matched_occurrences(line)
                    } else {
                        spans.len().max(1)
                    };
                    let mut record = MatchRecord::new(number, offset, line, spans);
                    record.occurrence_count = occ;
                    sink.on_match(record)?;
                    count += 1;
                }
            }
            return Ok(count);
        }

        let lines: Vec<(usize, usize, &'a [u8])> = Lines::new(buffer).collect();
        if lines.is_empty() {
            return Ok(0);
        }

        let mut filter_match_lines: Vec<Vec<usize>> =
            Vec::with_capacity(self.proximity_filters.len());
        for filter in &self.proximity_filters {
            let mut matches = Vec::new();
            for (line_idx, (_, _, line)) in lines.iter().enumerate() {
                if filter.matcher.matches_at(line)
                    || find_substr_on_line(&filter.matcher, line).is_some()
                {
                    matches.push(line_idx);
                }
            }

            if !filter.inverted && matches.is_empty() {
                return Ok(0);
            }
            filter_match_lines.push(matches);
        }

        struct PendingLine<'a> {
            number: usize,
            offset: usize,
            line: &'a [u8],
            match_spans: Vec<(usize, usize)>,
            near_spans: Vec<(usize, usize)>,
            context_origins: Vec<ContextOrigin>,
            is_context: bool,
        }

        let mut output_lines: std::collections::BTreeMap<usize, PendingLine<'a>> =
            std::collections::BTreeMap::new();
        let mut count = 0;

        for (line_idx, &(number, offset, line)) in lines.iter().enumerate() {
            if !compiled.eval_on_line(line) {
                continue;
            }

            let mut nearby = true;
            let mut matched_near_lines: Vec<(usize, usize)> = Vec::new();

            for (i, filter) in self.proximity_filters.iter().enumerate() {
                let start_win = line_idx.saturating_sub(filter.window);
                let end_win = line_idx.saturating_add(filter.window);
                let matches = &filter_match_lines[i];

                let start_pos = matches.partition_point(|&idx| idx < start_win);
                let has_match = start_pos < matches.len() && matches[start_pos] <= end_win;

                if has_match == filter.inverted {
                    nearby = false;
                    break;
                }

                if !filter.inverted {
                    let mut pos = start_pos;
                    while pos < matches.len() && matches[pos] <= end_win {
                        matched_near_lines.push((i, matches[pos]));
                        pos += 1;
                    }
                }
            }

            if !nearby {
                continue;
            }

            count += 1;

            let mut primary_spans = Vec::new();
            compiled.collect_spans(line, &mut primary_spans);

            let primary_entry = output_lines.entry(line_idx).or_insert_with(|| PendingLine {
                number,
                offset,
                line,
                match_spans: Vec::new(),
                near_spans: Vec::new(),
                context_origins: Vec::new(),
                is_context: false,
            });
            primary_entry.is_context = false;
            primary_entry.match_spans.extend(primary_spans);

            for (filter_idx, near_line_idx) in matched_near_lines {
                let filter = &self.proximity_filters[filter_idx];
                let (near_num, near_offset, near_line) = lines[near_line_idx];

                let mut nspans = Vec::new();
                filter.matcher.collect_spans(near_line, &mut nspans);

                let entry = output_lines
                    .entry(near_line_idx)
                    .or_insert_with(|| PendingLine {
                        number: near_num,
                        offset: near_offset,
                        line: near_line,
                        match_spans: Vec::new(),
                        near_spans: Vec::new(),
                        context_origins: Vec::new(),
                        is_context: true,
                    });
                entry.context_origins.push(ContextOrigin {
                    primary_line: number,
                    near_spans: nspans.clone(),
                });
                entry.near_spans.extend(nspans);

                let low = line_idx.min(near_line_idx) + 1;
                let high = line_idx.max(near_line_idx);
                for (bridge_idx, &(b_num, b_offset, b_line)) in
                    lines.iter().enumerate().take(high).skip(low)
                {
                    output_lines
                        .entry(bridge_idx)
                        .or_insert_with(|| PendingLine {
                            number: b_num,
                            offset: b_offset,
                            line: b_line,
                            match_spans: Vec::new(),
                            near_spans: Vec::new(),
                            context_origins: Vec::new(),
                            is_context: true,
                        })
                        .context_origins
                        .push(ContextOrigin {
                            primary_line: number,
                            near_spans: Vec::new(),
                        });
                }
            }
        }

        for (_, mut pending) in output_lines {
            pending.match_spans.sort_unstable();
            pending.match_spans.dedup();
            pending.near_spans.sort_unstable();
            pending.near_spans.dedup();

            let mut rec = if pending.is_context {
                MatchRecord::context(pending.number, pending.offset, pending.line)
            } else {
                MatchRecord::new(
                    pending.number,
                    pending.offset,
                    pending.line,
                    pending.match_spans,
                )
            };
            if !pending.is_context {
                rec.occurrence_count = compiled.count_matched_occurrences(pending.line);
            }
            rec.context_origins = pending.context_origins;
            rec.near_spans = pending.near_spans;
            sink.on_match(rec)?;
        }

        Ok(count)
    }
}
