use super::OutputFormatter;
use super::json::escape_json;
use super::theme::{
    SpanKind, base64_encode, byte_to_char_column, is_utf8_char_boundary, render_text,
};
use crate::core::MatchRecord;
use std::io::{self, Write};
use std::path::Path;

impl OutputFormatter {
    pub(crate) fn format_line_content_with_kinds(
        &self,
        line_byte_offset: usize,
        line_bytes: &[u8],
        spans: &[(usize, usize, SpanKind)],
    ) -> (String, Vec<(usize, usize, SpanKind)>) {
        if memchr::memchr(0x00, line_bytes).is_some() {
            let primary_spans: Vec<(usize, usize)> = spans
                .iter()
                .filter(|(_, _, k)| *k == SpanKind::Primary)
                .map(|&(s, e, _)| (s, e))
                .collect();
            if !self.allow_binary {
                return ("[Binary file matches]".to_string(), Vec::new());
            } else if !self.raw_binary_text {
                return (
                    Self::format_mini_hexdump(
                        line_byte_offset,
                        line_bytes,
                        &primary_spans,
                        self.should_use_color(),
                    ),
                    Vec::new(),
                );
            }
        }

        let limit = if self.only_matching {
            0
        } else {
            self.max_columns
                .unwrap_or(if self.is_tty { 1000 } else { 0 })
        };
        let (mut start, mut end) = (0, line_bytes.len());
        let boundaries: Vec<usize> = if limit > 0 {
            (0..line_bytes.len())
                .filter(|&index| is_utf8_char_boundary(line_bytes[index]))
                .chain(std::iter::once(line_bytes.len()))
                .collect()
        } else {
            Vec::new()
        };
        if limit > 0 && boundaries.len().saturating_sub(1) > limit {
            let (first, last, _) = spans.first().copied().unwrap_or((0, 0, SpanKind::Primary));
            let mid = first + last.saturating_sub(first) / 2;
            let middle = boundaries.partition_point(|&offset| offset < mid);
            let first = middle
                .saturating_sub(limit / 2)
                .min(boundaries.len() - 1 - limit);
            start = boundaries[first];
            end = boundaries[first + limit];
        }
        let (text, offsets) = render_text(&line_bytes[start..end], self.is_tty);
        let prefix = if start > 0 {
            format!("... [omitted {start} bytes] ")
        } else {
            String::new()
        };
        let mut mapped = Vec::new();
        for &(from, to, kind) in spans {
            if from < to && to > start && from < end {
                let left = from.saturating_sub(start);
                let mut right = to.min(end) - start;

                while right < offsets.len() - 1 && right > 0 && offsets[right] == offsets[right - 1]
                {
                    right += 1;
                }
                mapped.push((
                    prefix.len() + offsets[left],
                    prefix.len() + offsets[right],
                    kind,
                ));
            }
        }
        let mut result = prefix;
        result.push_str(&text);
        if end < line_bytes.len() {
            result.push_str(&format!(" ... [omitted {} bytes]", line_bytes.len() - end));
        }
        (result, mapped)
    }

    pub fn format_line_content(
        &self,
        line_byte_offset: usize,
        line_bytes: &[u8],
        spans: &[(usize, usize)],
    ) -> (String, Vec<(usize, usize)>) {
        let tagged: Vec<_> = spans
            .iter()
            .map(|&(s, e)| (s, e, SpanKind::Primary))
            .collect();
        let (text, mapped) =
            self.format_line_content_with_kinds(line_byte_offset, line_bytes, &tagged);
        (text, mapped.into_iter().map(|(s, e, _)| (s, e)).collect())
    }

    fn write_plain_records(
        &self,
        out: &mut dyn Write,
        path: &Path,
        records: &[MatchRecord<'_>],
    ) -> io::Result<()> {
        let delim = if self.null_separator { b'\0' } else { b'\n' };
        let has_context = records
            .iter()
            .any(|r| r.is_context && r.near_spans.is_empty());
        let mut previous = 0;
        for record in records {
            if has_context
                && previous > 0
                && record.line_number > previous + 1
                && !self.only_matching
            {
                out.write_all(b"--")?;
                out.write_all(&[delim])?;
            }
            previous = record.line_number;
            let is_near_only = record.match_spans.is_empty() && !record.near_spans.is_empty();
            let sep = if record.is_context && !is_near_only {
                '-'
            } else {
                ':'
            };
            let mut write_line = |bytes: &[u8], offset: usize| -> io::Result<()> {
                if !self.no_filename {
                    out.write_all(path.as_os_str().as_encoded_bytes())?;
                    write!(out, "{sep}")?;
                }
                if self.show_line_numbers {
                    write!(out, "{}{sep}", record.line_number)?;
                }
                if self.show_column && (!record.is_context || is_near_only) {
                    write!(
                        out,
                        "{}{sep}",
                        byte_to_char_column(record.line_bytes, offset)
                    )?;
                }
                if self.show_byte_offset {
                    write!(
                        out,
                        "{}{sep}",
                        record.line_byte_offset + if self.only_matching { offset } else { 0 }
                    )?;
                }
                out.write_all(bytes)?;
                out.write_all(&[delim])
            };
            if self.only_matching {
                if !record.is_context {
                    for &(start, end) in &record.match_spans {
                        if start < end
                            && let Some(bytes) = record.line_bytes.get(start..end)
                        {
                            write_line(bytes, start)?;
                        }
                    }
                }
            } else {
                let first_offset = record
                    .match_spans
                    .first()
                    .or_else(|| record.near_spans.first())
                    .map_or(0, |span| span.0);
                write_line(record.line_bytes, first_offset)?;
            }
        }
        Ok(())
    }

    pub fn format_mini_hexdump(
        base_offset: usize,
        bytes: &[u8],
        spans: &[(usize, usize)],
        use_color: bool,
    ) -> String {
        let mut out = String::new();

        let chunk_len = bytes.len().min(64);
        let slice = &bytes[..chunk_len];

        let mut row_start = 0;
        while row_start < slice.len() {
            let row_end = (row_start + 16).min(slice.len());
            let row_bytes = &slice[row_start..row_end];
            let abs_row_addr = base_offset + row_start;

            if use_color {
                let _ = std::fmt::Write::write_fmt(
                    &mut out,
                    format_args!("\x1b[34m0x{:08x}:\x1b[0m ", abs_row_addr),
                );
            } else {
                let _ =
                    std::fmt::Write::write_fmt(&mut out, format_args!("0x{:08x}: ", abs_row_addr));
            }

            for col in 0..16 {
                if col == 8 {
                    out.push(' ');
                }
                if col < row_bytes.len() {
                    let byte_offset = row_start + col;
                    let is_matched = spans
                        .iter()
                        .any(|&(s, e)| byte_offset >= s && byte_offset < e);
                    let b = row_bytes[col];

                    if use_color && is_matched {
                        let _ = std::fmt::Write::write_fmt(
                            &mut out,
                            format_args!("\x1b[1;31m{:02x}\x1b[0m ", b),
                        );
                    } else {
                        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{:02x} ", b));
                    }
                } else {
                    out.push_str("   ");
                }
            }

            out.push_str(" |");
            for (col, &b) in row_bytes.iter().enumerate() {
                let byte_offset = row_start + col;
                let is_matched = spans
                    .iter()
                    .any(|&(s, e)| byte_offset >= s && byte_offset < e);
                let ch = if (0x20..=0x7e).contains(&b) {
                    b as char
                } else {
                    '.'
                };

                if use_color && is_matched {
                    let _ = std::fmt::Write::write_fmt(
                        &mut out,
                        format_args!("\x1b[1;31m{}\x1b[0m", ch),
                    );
                } else {
                    out.push(ch);
                }
            }
            out.push('|');

            row_start += 16;
            if row_start < slice.len() {
                out.push('\n');
            }
        }

        if bytes.len() > chunk_len {
            out.push_str(&format!(" ... [omitted {} bytes]", bytes.len() - chunk_len));
        }

        out
    }

    pub fn format_file_matches_with_column(
        &self,
        path: &Path,
        matches: &[MatchRecord<'_>],
        column: Option<&str>,
        buffer: &mut Vec<u8>,
    ) -> io::Result<()> {
        let path = super::display_path(path);
        if self.quiet || (matches.is_empty() && !self.count_only && !self.count_matches) {
            return Ok(());
        }

        let delim = if self.null_separator { "\0" } else { "\n" };
        let path_str = path.to_string_lossy();
        let file_start = std::time::Instant::now();

        if self.json_output && !matches.is_empty() {
            let esc_path = escape_json(&path_str);
            let col_json = if let Some(col) = column {
                format!(",\"column\":\"{}\"", escape_json(col))
            } else {
                String::new()
            };
            #[cfg(unix)]
            let path_json = {
                use std::os::unix::ffi::OsStrExt;
                let raw_bytes = path.as_os_str().as_bytes();
                if std::str::from_utf8(raw_bytes).is_err() {
                    let b64 = base64_encode(raw_bytes);
                    format!("{{\"bytes\":\"{b64}\",\"text\":\"{esc_path}\"}}")
                } else {
                    format!("{{\"text\":\"{esc_path}\"}}")
                }
            };
            #[cfg(not(unix))]
            let path_json = format!("{{\"text\":\"{esc_path}\"}}");

            writeln!(
                buffer,
                "{{\"data\":{{\"path\":{path_json}{col_json}}},\"type\":\"begin\"}}"
            )?;

            let mut match_count = 0;
            let mut matched_lines = 0;
            for m in matches {
                let line_json = json_bytes(m.source_line_bytes);
                let abs_offset = m.line_byte_offset;

                if m.is_context {
                    writeln!(
                        buffer,
                        "{{\"data\":{{\"absolute_offset\":{abs_offset},\"line_number\":{},\"lines\":{line_json},\"path\":{path_json},\"submatches\":[]}},\"type\":\"context\"}}",
                        m.line_number
                    )?;
                } else {
                    matched_lines += 1;
                    match_count += m.occurrence_count;

                    let mut submatches = Vec::with_capacity(m.match_spans.len());
                    for (start, end) in &m.match_spans {
                        if *start <= m.line_bytes.len()
                            && *end <= m.line_bytes.len()
                            && *start < *end
                        {
                            let match_json = json_bytes(&m.line_bytes[*start..*end]);
                            submatches.push(format!(
                                "{{\"end\":{end},\"match\":{match_json},\"start\":{start}}}"
                            ));
                        }
                    }

                    writeln!(
                        buffer,
                        "{{\"data\":{{\"absolute_offset\":{abs_offset},\"line_number\":{},\"lines\":{line_json},\"path\":{path_json},\"submatches\":[{}]}},\"type\":\"match\"}}",
                        m.line_number,
                        submatches.join(",")
                    )?;
                }
            }

            let file_elapsed = file_start.elapsed();
            let bytes_searched = matches
                .first()
                .and_then(|record| record.bytes_searched)
                .map(|size| size as u64)
                .unwrap_or_else(|| {
                    std::fs::metadata(path)
                        .map(|m| m.len())
                        .unwrap_or_else(|_| {
                            matches
                                .last()
                                .map(|m| (m.line_byte_offset + m.line_bytes.len()) as u64)
                                .unwrap_or(0)
                        })
                });
            let bytes_printed = buffer.len();
            let human = format!("{:.6}s", file_elapsed.as_secs_f64());
            writeln!(
                buffer,
                "{{\"data\":{{\"binary_offset\":null,\"path\":{path_json},\"stats\":{{\"bytes_printed\":{bytes_printed},\"bytes_searched\":{bytes_searched},\"elapsed\":{{\"human\":\"{human}\",\"nanos\":{},\"secs\":{}}},\"matched_lines\":{matched_lines},\"matches\":{match_count},\"searches\":1,\"searches_with_match\":1}}}},\"type\":\"end\"}}",
                file_elapsed.as_nanos(),
                file_elapsed.as_secs()
            )?;

            return Ok(());
        }

        if self.files_with_matches {
            if self.null_separator {
                buffer.write_all(path.as_os_str().as_encoded_bytes())?;
                buffer.write_all(b"\0")?;
            } else {
                let use_color = self.should_use_color();
                let col_prefix = match column {
                    Some(col) if use_color => format!("\x1b[38;5;108m{col}\x1b[0m  "),
                    Some(col) => format!("{col}  "),
                    None => String::new(),
                };
                buffer.write_all(col_prefix.as_bytes())?;
                buffer.write_all(path.as_os_str().as_encoded_bytes())?;
                buffer.write_all(b"\n")?;
            }
            return Ok(());
        }

        if self.count_matches {
            if self.json_output {
                return Ok(());
            }
            let total_spans: usize = matches
                .iter()
                .filter(|m| !m.is_context)
                .map(|m| m.occurrence_count)
                .sum();
            if self.no_filename {
                write!(buffer, "{total_spans}{delim}")?;
            } else {
                buffer.write_all(path.as_os_str().as_encoded_bytes())?;
                write!(buffer, ":{total_spans}{delim}")?;
            }
            return Ok(());
        }

        if self.count_only {
            if self.json_output {
                return Ok(());
            }
            let count = matches.iter().filter(|m| !m.is_context).count();
            if self.no_filename {
                write!(buffer, "{count}{delim}")?;
            } else {
                buffer.write_all(path.as_os_str().as_encoded_bytes())?;
                write!(buffer, ":{count}{delim}")?;
            }
            return Ok(());
        }

        let use_color = self.should_use_color();
        if !self.is_tty
            && !use_color
            && (!self.show_heading || self.no_filename)
            && !self.should_use_hyperlinks()
            && !self.show_line_len
            && column.is_none()
            && self.max_columns.is_none_or(|limit| limit == 0)
            && matches.iter().all(|m| !m.line_bytes.contains(&0))
        {
            self.write_plain_records(buffer, path, matches)?;
            return Ok(());
        }

        if self.show_heading && !self.no_filename {
            let first_line = matches.first().map(|m| m.line_number).unwrap_or(1);
            let link_url = self.build_hyperlink_url(path, first_line, 1);
            let display_header = self.format_hyperlink(&link_url, &path_str);

            let col_prefix = match column {
                Some(col) if use_color => format!("\x1b[38;5;108m{col}\x1b[0m  "),
                Some(col) => format!("{col}  "),
                None => String::new(),
            };

            if use_color {
                writeln!(buffer, "{col_prefix}\x1b[1;36m{display_header}\x1b[0m")?;
            } else {
                writeln!(buffer, "{col_prefix}{display_header}")?;
            }

            let has_context = matches
                .iter()
                .any(|m| m.is_context && m.near_spans.is_empty());
            let mut last_line = 0;
            for m in matches {
                if has_context && last_line > 0 && m.line_number > last_line + 1 {
                    if use_color {
                        writeln!(buffer, "\x1b[38;5;242m--\x1b[0m")?;
                    } else {
                        writeln!(buffer, "--")?;
                    }
                }
                last_line = m.line_number;

                let is_binary_match =
                    !self.raw_binary_text && memchr::memchr(0x00, m.line_bytes).is_some();

                let mut spans_with_kinds: Vec<(usize, usize, SpanKind)> = Vec::new();
                for &(s, e) in &m.match_spans {
                    spans_with_kinds.push((s, e, SpanKind::Primary));
                }
                for &(s, e) in &m.near_spans {
                    let overlaps_primary = m.match_spans.iter().any(|&(ps, pe)| s < pe && e > ps);
                    if !overlaps_primary {
                        spans_with_kinds.push((s, e, SpanKind::Near));
                    }
                }
                spans_with_kinds.sort_by_key(|&(s, _, _)| s);

                let (line_text, active_spans) = self.format_line_content_with_kinds(
                    m.line_byte_offset,
                    m.line_bytes,
                    &spans_with_kinds,
                );

                if is_binary_match {
                    writeln!(buffer, "{line_text}")?;
                    continue;
                }

                if self.only_matching {
                    for ((start, end), &(source_start, _)) in active_spans
                        .iter()
                        .filter(|(_, _, kind)| *kind == SpanKind::Primary)
                        .map(|&(s, e, _)| (s, e))
                        .zip(m.match_spans.iter().filter(|(a, b)| a < b))
                    {
                        if start < end && line_text.get(start..end).is_some() {
                            let span = &line_text[start..end];
                            if self.show_line_len {
                                let len_str = format!("{:>5}", span.len());
                                if use_color {
                                    write!(buffer, "\x1b[38;5;108m{len_str}\x1b[0m  ")?;
                                } else {
                                    write!(buffer, "{len_str}  ")?;
                                }
                            }
                            if self.show_line_numbers {
                                write!(buffer, "{}:", m.line_number)?;
                            }
                            if self.show_column {
                                let col = byte_to_char_column(m.line_bytes, source_start);
                                write!(buffer, "{col}:")?;
                            }
                            if self.show_byte_offset {
                                write!(buffer, "{}:", m.line_byte_offset + source_start)?;
                            }
                            if use_color {
                                writeln!(buffer, "\x1b[1;33m{span}\x1b[0m")?;
                            } else {
                                writeln!(buffer, "{span}")?;
                            }
                        }
                    }
                    continue;
                }

                let is_near_only = m.match_spans.is_empty() && !m.near_spans.is_empty();

                if self.show_line_len && (!m.is_context || is_near_only) {
                    let len_str = format!("{:>5}", m.line_bytes.len());
                    if use_color {
                        write!(buffer, "\x1b[38;5;108m{len_str}\x1b[0m  ")?;
                    } else {
                        write!(buffer, "{len_str}  ")?;
                    }
                }

                let sep = if m.is_context && !is_near_only {
                    '-'
                } else {
                    ':'
                };

                if self.show_column || self.show_byte_offset {
                    if self.show_line_numbers {
                        let line_url = self.build_hyperlink_url(path, m.line_number, 1);
                        let linked_num =
                            self.format_hyperlink(&line_url, &m.line_number.to_string());
                        if use_color {
                            if m.is_context && !is_near_only {
                                write!(buffer, "\x1b[38;5;242m{linked_num}{sep}\x1b[0m")?;
                            } else if is_near_only {
                                write!(
                                    buffer,
                                    "{}{linked_num}\x1b[0m{sep}",
                                    self.colors.near_match_highlight
                                )?;
                            } else {
                                write!(
                                    buffer,
                                    "{}{linked_num}\x1b[0m{sep}",
                                    self.colors.line_number
                                )?;
                            }
                        } else {
                            write!(buffer, "{linked_num}{sep}")?;
                        }
                    }
                    if self.show_column && (!m.is_context || is_near_only) {
                        let col = m
                            .match_spans
                            .first()
                            .or_else(|| m.near_spans.first())
                            .map(|s| byte_to_char_column(m.line_bytes, s.0))
                            .unwrap_or(1);
                        write!(buffer, "{col}{sep}")?;
                    }
                    if self.show_byte_offset {
                        write!(buffer, "{}{sep}", m.line_byte_offset)?;
                    }
                } else if self.show_line_numbers {
                    let line_url = self.build_hyperlink_url(path, m.line_number, 1);
                    let line_num_str = format!("{:>5}", m.line_number);
                    let linked_num = self.format_hyperlink(&line_url, &line_num_str);

                    if use_color {
                        if m.is_context && !is_near_only {
                            write!(buffer, "\x1b[38;5;242m{linked_num}{sep}\x1b[0m ")?;
                        } else if is_near_only {
                            write!(
                                buffer,
                                "{}{linked_num}\x1b[0m\x1b[38;5;242m{sep}\x1b[0m ",
                                self.colors.near_match_highlight
                            )?;
                        } else {
                            write!(
                                buffer,
                                "{}{linked_num}\x1b[0m\x1b[38;5;242m{sep}\x1b[0m ",
                                self.colors.line_number
                            )?;
                        }
                    } else {
                        write!(buffer, "{linked_num}{sep} ")?;
                    }
                }

                if use_color && !active_spans.is_empty() {
                    let mut last_idx = 0;
                    for (start, end, kind) in &active_spans {
                        if *start > last_idx && line_text.get(last_idx..*start).is_some() {
                            write!(buffer, "{}", &line_text[last_idx..*start])?;
                        }
                        if *start >= last_idx
                            && *start < *end
                            && line_text.get(*start..*end).is_some()
                        {
                            let color = match kind {
                                SpanKind::Primary => &self.colors.match_highlight,
                                SpanKind::Near => &self.colors.near_match_highlight,
                            };
                            write!(buffer, "{color}{}\x1b[0m", &line_text[*start..*end])?;
                            last_idx = *end;
                        }
                    }
                    if last_idx < line_text.len() {
                        write!(buffer, "{}", &line_text[last_idx..])?;
                    }
                    writeln!(buffer)?;
                } else if use_color && m.is_context {
                    writeln!(buffer, "{}{line_text}\x1b[0m", self.colors.context)?;
                } else if line_text.contains('\n') {
                    let indent = if self.show_line_numbers && self.show_column {
                        "           "
                    } else if self.show_line_numbers {
                        "       "
                    } else {
                        "  "
                    };
                    let indented = line_text.replace('\n', &format!("\n{indent}"));
                    writeln!(buffer, "{indented}")?;
                } else {
                    writeln!(buffer, "{line_text}")?;
                }
            }

            writeln!(buffer)?;
        } else {
            let has_context = matches
                .iter()
                .any(|m| m.is_context && m.near_spans.is_empty());
            let mut last_line = 0;
            for m in matches {
                if has_context && last_line > 0 && m.line_number > last_line + 1 {
                    if use_color {
                        write!(buffer, "\x1b[38;5;242m--\x1b[0m{delim}")?;
                    } else {
                        write!(buffer, "--{delim}")?;
                    }
                }
                last_line = m.line_number;

                let is_binary_match =
                    !self.raw_binary_text && memchr::memchr(0x00, m.line_bytes).is_some();

                let mut spans_with_kinds: Vec<(usize, usize, SpanKind)> = Vec::new();
                for &(s, e) in &m.match_spans {
                    spans_with_kinds.push((s, e, SpanKind::Primary));
                }
                for &(s, e) in &m.near_spans {
                    let overlaps_primary = m.match_spans.iter().any(|&(ps, pe)| s < pe && e > ps);
                    if !overlaps_primary {
                        spans_with_kinds.push((s, e, SpanKind::Near));
                    }
                }
                spans_with_kinds.sort_by_key(|&(s, _, _)| s);

                let (line_text, active_spans) = self.format_line_content_with_kinds(
                    m.line_byte_offset,
                    m.line_bytes,
                    &spans_with_kinds,
                );

                let col_color = &self.colors.column;
                let path_color = &self.colors.path;
                let line_color = &self.colors.line_number;
                let match_color = &self.colors.match_highlight;
                let near_match_color = &self.colors.near_match_highlight;
                let ctx_color = &self.colors.context;

                let col_prefix = match column {
                    Some(col) if use_color => format!("{col_color}{col}\x1b[0m  "),
                    Some(col) => format!("{col}  "),
                    None => String::new(),
                };

                if is_binary_match {
                    if !col_prefix.is_empty() {
                        write!(buffer, "{col_prefix}")?;
                    }
                    if !self.no_filename {
                        if use_color {
                            write!(buffer, "{path_color}{path_str}\x1b[0m: ")?;
                        } else {
                            write!(buffer, "{path_str}: ")?;
                        }
                    }
                    write!(buffer, "{line_text}{delim}")?;
                    continue;
                }

                if self.only_matching {
                    for ((start, end), &(source_start, _)) in active_spans
                        .iter()
                        .filter(|(_, _, kind)| *kind == SpanKind::Primary)
                        .map(|&(s, e, _)| (s, e))
                        .zip(m.match_spans.iter().filter(|(a, b)| a < b))
                    {
                        if start < end && line_text.get(start..end).is_some() {
                            let span = &line_text[start..end];
                            if !col_prefix.is_empty() {
                                write!(buffer, "{col_prefix}")?;
                            }
                            if self.show_line_len {
                                let len_str = format!("{:>5}", span.len());
                                if use_color {
                                    write!(buffer, "{col_color}{len_str}\x1b[0m  ")?;
                                } else {
                                    write!(buffer, "{len_str}  ")?;
                                }
                            }
                            if !self.no_filename {
                                if use_color {
                                    write!(buffer, "{path_color}{path_str}\x1b[0m:")?;
                                } else {
                                    write!(buffer, "{path_str}:")?;
                                }
                            }
                            if self.show_line_numbers {
                                if use_color {
                                    write!(buffer, "{line_color}{}\x1b[0m:", m.line_number)?;
                                } else {
                                    write!(buffer, "{}:", m.line_number)?;
                                }
                            }
                            if self.show_column {
                                let col = byte_to_char_column(m.line_bytes, source_start);
                                write!(buffer, "{col}:")?;
                            }
                            if self.show_byte_offset {
                                write!(buffer, "{}:", m.line_byte_offset + source_start)?;
                            }
                            if use_color {
                                write!(buffer, "{match_color}{span}\x1b[0m{delim}")?;
                            } else {
                                write!(buffer, "{span}{delim}")?;
                            }
                        }
                    }
                    continue;
                }

                let is_near_only = m.match_spans.is_empty() && !m.near_spans.is_empty();

                let sep = if m.is_context && !is_near_only {
                    '-'
                } else {
                    ':'
                };

                if !col_prefix.is_empty() {
                    write!(buffer, "{col_prefix}")?;
                }
                if self.show_line_len && (!m.is_context || is_near_only) {
                    let len_str = format!("{:>5}", m.line_bytes.len());
                    if use_color {
                        write!(buffer, "{col_color}{len_str}\x1b[0m  ")?;
                    } else {
                        write!(buffer, "{len_str}  ")?;
                    }
                }

                if !self.no_filename {
                    if use_color {
                        write!(buffer, "{path_color}{path_str}\x1b[0m{sep}")?;
                    } else {
                        write!(buffer, "{path_str}{sep}")?;
                    }
                }
                if self.show_line_numbers {
                    if use_color {
                        if m.is_context && !is_near_only {
                            write!(buffer, "\x1b[38;5;242m{}\x1b[0m{sep}", m.line_number)?;
                        } else if is_near_only {
                            write!(buffer, "{}{}\x1b[0m{sep}", near_match_color, m.line_number)?;
                        } else {
                            write!(buffer, "{line_color}{}\x1b[0m{sep}", m.line_number)?;
                        }
                    } else {
                        write!(buffer, "{}{sep}", m.line_number)?;
                    }
                }
                if self.show_column && (!m.is_context || is_near_only) {
                    let col = m
                        .match_spans
                        .first()
                        .or_else(|| m.near_spans.first())
                        .map(|s| byte_to_char_column(m.line_bytes, s.0))
                        .unwrap_or(1);
                    write!(buffer, "{col}{sep}")?;
                }
                if self.show_byte_offset {
                    write!(buffer, "{}{sep}", m.line_byte_offset)?;
                }

                if use_color && !active_spans.is_empty() {
                    let mut last_idx = 0;
                    for (start, end, kind) in &active_spans {
                        if *start > last_idx && line_text.get(last_idx..*start).is_some() {
                            write!(buffer, "{}", &line_text[last_idx..*start])?;
                        }
                        if *start >= last_idx
                            && *start < *end
                            && line_text.get(*start..*end).is_some()
                        {
                            let color = match kind {
                                SpanKind::Primary => match_color,
                                SpanKind::Near => near_match_color,
                            };
                            write!(buffer, "{color}{}\x1b[0m", &line_text[*start..*end])?;
                            last_idx = *end;
                        }
                    }
                    if last_idx < line_text.len() {
                        write!(buffer, "{}", &line_text[last_idx..])?;
                    }
                    write!(buffer, "{delim}")?;
                } else if use_color && m.is_context {
                    write!(buffer, "{ctx_color}{line_text}\x1b[0m{delim}")?;
                } else if line_text.contains('\n') {
                    let indent = "    ";
                    let indented = line_text.replace('\n', &format!("\n{indent}"));
                    write!(buffer, "{indented}{delim}")?;
                } else {
                    write!(buffer, "{line_text}{delim}")?;
                }
            }
        }

        Ok(())
    }

    pub fn print_file_matches_with_column(
        &mut self,
        path: &Path,
        matches: &[MatchRecord<'_>],
        column: Option<&str>,
    ) -> io::Result<()> {
        let mut buffer = Vec::with_capacity(1024);
        self.format_file_matches_with_column(path, matches, column, &mut buffer)?;
        if !buffer.is_empty() {
            self.account_json_payload(&buffer);
            self.writer.write_all(&buffer)?;
        }
        Ok(())
    }

    pub fn print_file_matches(
        &mut self,
        path: &Path,
        matches: &[MatchRecord<'_>],
    ) -> io::Result<()> {
        self.print_file_matches_with_column(path, matches, None)
    }
}

fn json_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => format!("{{\"text\":\"{}\"}}", escape_json(text)),
        Err(_) => format!("{{\"bytes\":\"{}\"}}", base64_encode(bytes)),
    }
}
