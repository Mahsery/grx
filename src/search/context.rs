use crate::core::{ContextOrigin, MatchRecord, MatchSink, OwnedMatchRecord};
use std::io;

pub fn expand_context_lines<'a>(
    buffer: &'a [u8],
    matches: &[MatchRecord<'a>],
    before_context: usize,
    after_context: usize,
) -> Vec<MatchRecord<'a>> {
    if matches.is_empty() || (before_context == 0 && after_context == 0) {
        return matches.to_vec();
    }

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
        let clean_end = if buffer[buffer.len() - 1] == b'\r' {
            buffer.len() - 1
        } else {
            buffer.len()
        };
        line_spans.push((line_start, clean_end));
    }

    let total_lines = line_spans.len();
    if total_lines == 0 {
        return matches.to_vec();
    }

    if matches.iter().any(|m| m.line_number > total_lines) {
        return matches.to_vec();
    }

    struct ContextEntry {
        occurrence_count: usize,
        match_spans: Vec<(usize, usize)>,
        near_spans: Vec<(usize, usize)>,
        context_origins: Vec<ContextOrigin>,
        is_context: bool,
    }

    let mut match_map: std::collections::BTreeMap<usize, ContextEntry> =
        std::collections::BTreeMap::new();

    for m in matches {
        let line_idx = m.line_number.saturating_sub(1);
        match_map
            .entry(line_idx)
            .and_modify(|e| {
                e.occurrence_count += m.occurrence_count;
                e.match_spans.extend(m.match_spans.iter().copied());
                e.near_spans.extend(m.near_spans.iter().copied());
                e.context_origins.extend(m.context_origins.iter().cloned());
                if !m.is_context {
                    e.is_context = false;
                }
            })
            .or_insert_with(|| ContextEntry {
                occurrence_count: m.occurrence_count,
                match_spans: m.match_spans.clone(),
                near_spans: m.near_spans.clone(),
                context_origins: m.context_origins.clone(),
                is_context: m.is_context,
            });

        if m.is_context {
            continue;
        }
        let start_ctx = line_idx.saturating_sub(before_context);
        let end_ctx = line_idx
            .saturating_add(1)
            .saturating_add(after_context)
            .min(total_lines);
        for ctx_idx in start_ctx..end_ctx {
            if ctx_idx == line_idx {
                continue;
            }
            match_map
                .entry(ctx_idx)
                .or_insert_with(|| ContextEntry {
                    occurrence_count: 0,
                    match_spans: Vec::new(),
                    near_spans: Vec::new(),
                    context_origins: Vec::new(),
                    is_context: true,
                })
                .context_origins
                .push(ContextOrigin {
                    primary_line: m.line_number,
                    near_spans: Vec::new(),
                });
        }
    }

    let mut expanded = Vec::with_capacity(match_map.len());
    for (line_idx, entry) in match_map {
        if line_idx >= total_lines {
            continue;
        }
        let (start, end) = line_spans[line_idx];
        let line_bytes = &buffer[start..end];
        let line_number = line_idx + 1;

        let mut rec = if entry.is_context {
            MatchRecord::context(line_number, start, line_bytes)
        } else {
            MatchRecord::new(line_number, start, line_bytes, entry.match_spans)
        };
        rec.occurrence_count = entry.occurrence_count;
        rec.near_spans = entry.near_spans;
        rec.context_origins = entry.context_origins;
        expanded.push(rec);
    }

    expanded
}

#[derive(Default)]
pub struct VecMatchSink {
    pub matches: Vec<OwnedMatchRecord>,
}

impl<'a> MatchSink<'a> for VecMatchSink {
    fn on_match(&mut self, record: MatchRecord<'a>) -> io::Result<()> {
        self.matches.push(record.to_owned());
        Ok(())
    }
}
