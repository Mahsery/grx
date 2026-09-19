use crate::config::{ColorChoice, HyperlinkChoice};
use crate::core::{DirEntry, EntrySink, MatchRecord, Printer};
use std::io::{self, IsTerminal, Write};
use std::path::Path;

/// Formats and renders search matches to standard output.
pub struct OutputFormatter {
    writer: Box<dyn Write + Send + Sync>,
    is_tty: bool,
    color: ColorChoice,
    hyperlinks: HyperlinkChoice,
    hyperlink_format: String,
    show_line_numbers: bool,
    show_column: bool,
    show_line_len: bool,
    show_byte_offset: bool,
    show_heading: bool,
    null_separator: bool,
    only_matching: bool,
    quiet: bool,
    files_with_matches: bool,
    count_only: bool,
    count_matches: bool,
    json_output: bool,
    no_filename: bool,
    hostname: String,
    max_columns: Option<usize>,
    allow_binary: bool,
    raw_binary_text: bool,
    json_bytes_printed: u64,
    pub colors: crate::config::ColorTheme,
}

#[inline]
fn is_utf8_char_boundary(b: u8) -> bool {
    (b & 0xC0) != 0x80
}

#[allow(dead_code)]
fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 3) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 15) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Render display text while retaining a map from source bytes to display boundaries.
fn render_text(bytes: &[u8], terminal: bool) -> (String, Vec<usize>) {
    let mut text = String::new();
    let mut offsets = vec![0; bytes.len() + 1];
    let mut source = 0;
    for chunk in bytes.utf8_chunks() {
        for ch in chunk.valid().chars() {
            let end = source + ch.len_utf8();
            offsets[source..end].fill(text.len());
            if terminal && ch.is_ascii_control() && ch != '\t' && ch != '\n' {
                text.push('^');
                text.push(((ch as u8) ^ 0x40) as char);
            } else {
                text.push(ch);
            }
            source = end;
            offsets[source] = text.len();
        }
        if !chunk.invalid().is_empty() {
            let end = source + chunk.invalid().len();
            offsets[source..end].fill(text.len());
            text.push('\u{fffd}');
            source = end;
            offsets[source] = text.len();
        }
    }
    (text, offsets)
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if c.is_ascii_control() => {
                let _ = std::fmt::Write::write_fmt(&mut out, format_args!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

fn json_payload_bytes(bytes: &[u8]) -> u64 {
    bytes
        .split_inclusive(|byte| *byte == b'\n')
        .filter(|record| {
            memchr::memmem::find(record, b"\"type\":\"end\"").is_none()
                && memchr::memmem::find(record, b"\"type\":\"summary\"").is_none()
        })
        .map(|record| record.len() as u64)
        .sum()
}

/// Calculates 1-based Unicode character column from a byte offset in a line.
pub fn byte_to_char_column(line_bytes: &[u8], byte_offset: usize) -> usize {
    let limit = byte_offset.min(line_bytes.len());
    let prefix = &line_bytes[..limit];
    match std::str::from_utf8(prefix) {
        Ok(s) => s.chars().count() + 1,
        Err(_) => String::from_utf8_lossy(prefix).chars().count() + 1,
    }
}

fn get_fd_file_style(path: &Path, file_name: &str) -> (&'static str, &'static str) {
    let lower_name = file_name.to_ascii_lowercase();

    // Compound archive extensions
    if lower_name.ends_with(".tar.gz")
        || lower_name.ends_with(".tar.bz2")
        || lower_name.ends_with(".tar.xz")
        || lower_name.ends_with(".tar.zst")
    {
        return ("\x1b[4;38;5;203m", "\x1b[0m");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = path.metadata()
            && meta.permissions().mode() & 0o111 != 0
        {
            return ("\x1b[1;38;5;203m", "\x1b[0m");
        }
    }
    #[cfg(not(unix))]
    let _ = path;

    let ext = lower_name.rsplit('.').next().unwrap_or("");
    if ext == lower_name {
        // No extension
        return ("", "");
    }

    match ext {
        // Archives & compressed: underlined coral
        "zip" | "gz" | "tar" | "xz" | "bz2" | "7z" | "zst" | "rar" | "tgz" | "deb" | "rpm" => {
            ("\x1b[4;38;5;203m", "\x1b[0m")
        }
        // Executables & scripts (Windows or script extensions): bold coral
        "exe" | "bat" | "cmd" | "sh" | "bash" | "zsh" | "fish" => ("\x1b[1;38;5;203m", "\x1b[0m"),
        // Source code: vibrant green 48
        "rs" | "py" | "c" | "h" | "cpp" | "hpp" | "cc" | "cxx" | "js" | "ts" | "jsx" | "tsx"
        | "go" | "java" | "rb" | "php" | "cs" | "swift" | "kt" | "kts" | "lua" | "r" | "scala"
        | "elm" | "hs" | "clj" | "zig" | "nim" | "vue" | "svelte" | "sql" => {
            ("\x1b[38;5;48m", "\x1b[0m")
        }
        // Data & configuration: yellow-green 149
        "json" | "toml" | "yaml" | "yml" | "xml" | "csv" | "tsv" | "ini" | "conf" | "lock"
        | "env" | "properties" => ("\x1b[38;5;149m", "\x1b[0m"),
        // Documents & text: warm khaki 185
        "md" | "txt" | "rst" | "org" | "pdf" | "doc" | "docx" | "tex" | "log" => {
            ("\x1b[38;5;185m", "\x1b[0m")
        }
        // Media & images: orange 208
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" | "bmp" | "mp4" | "mkv" | "avi"
        | "mov" | "mp3" | "flac" | "wav" | "ogg" => ("\x1b[38;5;208m", "\x1b[0m"),
        _ => ("", ""),
    }
}

impl OutputFormatter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        writer: Box<dyn Write + Send + Sync>,
        color: ColorChoice,
        hyperlinks: HyperlinkChoice,
        hyperlink_format: String,
        show_line_numbers: bool,
        show_heading: bool,
        null_separator: bool,
        only_matching: bool,
        quiet: bool,
        files_with_matches: bool,
        count_only: bool,
        no_filename: bool,
        max_columns: Option<usize>,
        allow_binary: bool,
    ) -> Self {
        let is_tty = io::stdout().is_terminal();
        let hostname = get_hostname();

        Self {
            writer,
            is_tty,
            color,
            hyperlinks,
            hyperlink_format,
            show_line_numbers,
            show_column: false,
            show_line_len: false,
            show_byte_offset: false,
            show_heading,
            null_separator,
            only_matching,
            quiet,
            files_with_matches,
            count_only,
            count_matches: false,
            json_output: false,
            no_filename,
            hostname,
            max_columns,
            allow_binary,
            raw_binary_text: false,
            json_bytes_printed: 0,
            colors: crate::config::ColorTheme::default(),
        }
    }

    fn account_json_payload(&mut self, bytes: &[u8]) {
        if self.json_output {
            self.json_bytes_printed = self
                .json_bytes_printed
                .saturating_add(json_payload_bytes(bytes));
        }
    }

    pub fn with_colors(mut self, colors: crate::config::ColorTheme) -> Self {
        self.colors = colors;
        self
    }

    pub fn with_raw_binary_text(mut self, raw_binary_text: bool) -> Self {
        self.raw_binary_text = raw_binary_text;
        self
    }

    pub fn with_column(mut self, show_column: bool) -> Self {
        self.show_column = show_column;
        self
    }

    pub fn with_line_len(mut self, show_line_len: bool) -> Self {
        self.show_line_len = show_line_len;
        self
    }

    pub fn with_byte_offset(mut self, show_byte_offset: bool) -> Self {
        self.show_byte_offset = show_byte_offset;
        self
    }

    pub fn with_count_matches(mut self, count_matches: bool) -> Self {
        self.count_matches = count_matches;
        self
    }

    pub fn with_json(mut self, json_output: bool) -> Self {
        self.json_output = json_output;
        self
    }

    /// Format and window a matching line. If max_columns is set and the line exceeds
    /// the limit, a window centered around the first match span is produced with
    /// truncation annotations. Null bytes are formatted as mini-hexdump or suppressed.
    pub fn format_line_content(
        &self,
        line_byte_offset: usize,
        line_bytes: &[u8],
        spans: &[(usize, usize)],
    ) -> (String, Vec<(usize, usize)>) {
        // If line contains null bytes and binary search was not explicitly enabled, suppress raw output
        if memchr::memchr(0x00, line_bytes).is_some() {
            if !self.allow_binary {
                return ("[Binary file matches]".to_string(), Vec::new());
            } else if !self.raw_binary_text {
                return (
                    Self::format_mini_hexdump(
                        line_byte_offset,
                        line_bytes,
                        spans,
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
        if limit > 0 && line_bytes.len() > limit {
            let (first, last) = spans.first().copied().unwrap_or((0, 0));
            let mid = first + last.saturating_sub(first) / 2;
            start = mid
                .saturating_sub(limit / 2)
                .min(line_bytes.len().saturating_sub(limit));
            end = start.saturating_add(limit).min(line_bytes.len());
            while start > 0 && !is_utf8_char_boundary(line_bytes[start]) {
                start -= 1;
            }
            while end < line_bytes.len() && !is_utf8_char_boundary(line_bytes[end]) {
                end += 1;
            }
        }
        let (text, offsets) = render_text(&line_bytes[start..end], self.is_tty);
        let prefix = if start > 0 {
            format!("... [omitted {start} bytes] ")
        } else {
            String::new()
        };
        let mut mapped = Vec::new();
        for &(from, to) in spans {
            if from < to && to > start && from < end {
                let left = from.saturating_sub(start);
                let mut right = to.min(end) - start;
                // A byte-oriented regex may end inside a displayed character.
                while right < offsets.len() - 1 && right > 0 && offsets[right] == offsets[right - 1]
                {
                    right += 1;
                }
                mapped.push((prefix.len() + offsets[left], prefix.len() + offsets[right]));
            }
        }
        let mut result = prefix;
        result.push_str(&text);
        if end < line_bytes.len() {
            result.push_str(&format!(" ... [omitted {} bytes]", line_bytes.len() - end));
        }
        (result, mapped)
    }

    /// Write source bytes directly when no display transformation was requested.
    fn write_plain_records(
        &self,
        out: &mut dyn Write,
        path: &Path,
        records: &[MatchRecord<'_>],
    ) -> io::Result<()> {
        let delim = if self.null_separator { b'\0' } else { b'\n' };
        let has_context = records.iter().any(|r| r.is_context);
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
            let sep = if record.is_context { '-' } else { ':' };
            let mut write_line = |bytes: &[u8], offset: usize| -> io::Result<()> {
                if !self.no_filename {
                    out.write_all(path.as_os_str().as_encoded_bytes())?;
                    write!(out, "{sep}")?;
                }
                if self.show_line_numbers {
                    write!(out, "{}{sep}", record.line_number)?;
                }
                if self.show_column && !record.is_context {
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
                write_line(
                    record.line_bytes,
                    record.match_spans.first().map_or(0, |span| span.0),
                )?;
            }
        }
        Ok(())
    }

    /// Renders a binary buffer slice as a 16-byte aligned mini-hexdump with match highlighting.
    pub fn format_mini_hexdump(
        base_offset: usize,
        bytes: &[u8],
        spans: &[(usize, usize)],
        use_color: bool,
    ) -> String {
        let mut out = String::new();
        // Display up to 64 bytes per binary record
        let chunk_len = bytes.len().min(64);
        let slice = &bytes[..chunk_len];

        let mut row_start = 0;
        while row_start < slice.len() {
            let row_end = (row_start + 16).min(slice.len());
            let row_bytes = &slice[row_start..row_end];
            let abs_row_addr = base_offset + row_start;

            // Address offset column: 0x0001a400:
            if use_color {
                let _ = std::fmt::Write::write_fmt(
                    &mut out,
                    format_args!("\x1b[34m0x{:08x}:\x1b[0m ", abs_row_addr),
                );
            } else {
                let _ =
                    std::fmt::Write::write_fmt(&mut out, format_args!("0x{:08x}: ", abs_row_addr));
            }

            // Hex bytes column: 16 hex byte pairs with split at 8
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

            // ASCII representation column: |...|
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

    #[inline]
    pub fn should_use_color(&self) -> bool {
        match self.color {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                if let Ok(force) = std::env::var("CLICOLOR_FORCE")
                    && force != "0"
                {
                    return true;
                }
                if std::env::var_os("NO_COLOR").is_some() {
                    false
                } else if let Ok(clicolor) = std::env::var("CLICOLOR") {
                    if clicolor == "0" { false } else { self.is_tty }
                } else {
                    self.is_tty
                }
            }
        }
    }

    #[inline]
    fn should_use_hyperlinks(&self) -> bool {
        match self.hyperlinks {
            HyperlinkChoice::Always => true,
            HyperlinkChoice::Never => false,
            HyperlinkChoice::Auto => self.is_tty,
        }
    }

    fn build_hyperlink_url(&self, path: &Path, line: usize, col: usize) -> String {
        let abs_path = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_string();

        let format = if self.hyperlink_format.is_empty() {
            "file://{host}{path}#{line}:{col}"
        } else {
            &self.hyperlink_format
        };

        format
            .replace("{host}", &self.hostname)
            .replace("{path}", &abs_path)
            .replace("{line}", &line.to_string())
            .replace("{col}", &col.to_string())
    }

    fn build_directory_hyperlink_url(&self, path: &Path) -> String {
        let abs_path = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_string();

        if self.hyperlink_format.is_empty() || self.hyperlink_format.starts_with("file://") {
            format!("file://{}{abs_path}", self.hostname)
        } else {
            let base_format = self
                .hyperlink_format
                .split('#')
                .next()
                .unwrap_or(&self.hyperlink_format);
            let base_format = base_format
                .trim_end_matches(":{line}:{col}")
                .trim_end_matches(":{line}");
            base_format
                .replace("{host}", &self.hostname)
                .replace("{path}", &abs_path)
        }
    }

    fn format_hyperlink(&self, url: &str, text: &str) -> String {
        if self.should_use_hyperlinks() {
            format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
        } else {
            text.to_string()
        }
    }

    /// Print final ripgrep-compatible JSON summary record.
    pub fn print_json_summary(
        &mut self,
        total_matches: usize,
        matched_lines: usize,
        files_with_matches: usize,
        total_files: usize,
        bytes_searched: u64,
        elapsed: std::time::Duration,
    ) -> io::Result<()> {
        self.print_json_summary_with_skipped(
            total_matches,
            matched_lines,
            files_with_matches,
            total_files,
            bytes_searched,
            0,
            elapsed,
        )
    }

    /// Print final ripgrep-compatible JSON summary record including skipped binary files count.
    #[allow(clippy::too_many_arguments)]
    pub fn print_json_summary_with_skipped(
        &mut self,
        total_matches: usize,
        matched_lines: usize,
        files_with_matches: usize,
        total_files: usize,
        bytes_searched: u64,
        skipped_binaries: usize,
        elapsed: std::time::Duration,
    ) -> io::Result<()> {
        let mut buffer = Vec::new();
        let human = format!("{:.6}s", elapsed.as_secs_f64());
        writeln!(
            buffer,
            "{{\"data\":{{\"elapsed_total\":{{\"human\":\"{human}\",\"nanos\":{},\"secs\":{}}},\"stats\":{{\"binary_files_skipped\":{skipped_binaries},\"bytes_printed\":{},\"bytes_searched\":{bytes_searched},\"elapsed\":{{\"human\":\"{human}\",\"nanos\":{},\"secs\":{}}},\"matched_lines\":{matched_lines},\"matches\":{total_matches},\"searches\":{total_files},\"searches_with_match\":{files_with_matches}}}}},\"type\":\"summary\"}}",
            elapsed.as_nanos(),
            elapsed.as_secs(),
            self.json_bytes_printed,
            elapsed.as_nanos(),
            elapsed.as_secs()
        )?;
        self.writer.write_all(&buffer)?;
        Ok(())
    }

    /// Print aggregate execution statistics summary.
    pub fn print_stats_summary(
        &mut self,
        total_matches: usize,
        matched_lines: usize,
        files_with_matches: usize,
        total_files: usize,
        bytes_searched: u64,
        elapsed: std::time::Duration,
    ) -> io::Result<()> {
        self.print_stats_summary_with_skipped(
            total_matches,
            matched_lines,
            files_with_matches,
            total_files,
            bytes_searched,
            0,
            elapsed,
        )
    }

    /// Print aggregate execution statistics summary including skipped binary counts and throughput.
    #[allow(clippy::too_many_arguments)]
    pub fn print_stats_summary_with_skipped(
        &mut self,
        total_matches: usize,
        matched_lines: usize,
        files_with_matches: usize,
        total_files: usize,
        bytes_searched: u64,
        skipped_binaries: usize,
        elapsed: std::time::Duration,
    ) -> io::Result<()> {
        let mut buffer = Vec::new();
        writeln!(buffer)?;
        writeln!(buffer, "{total_matches} matches")?;
        writeln!(buffer, "{matched_lines} matched lines")?;
        writeln!(buffer, "{files_with_matches} files contained matches")?;
        writeln!(buffer, "{total_files} files searched")?;
        writeln!(buffer, "{bytes_searched} bytes searched")?;
        if skipped_binaries > 0 {
            writeln!(buffer, "{skipped_binaries} binary files skipped")?;
        }
        let secs = elapsed.as_secs_f64();
        if bytes_searched > 0 && secs > 0.0 {
            let mb_per_sec = (bytes_searched as f64) / (1_048_576.0 * secs);
            writeln!(buffer, "{:.6} seconds total ({mb_per_sec:.2} MB/s)", secs)?;
        } else {
            writeln!(buffer, "{:.6} seconds total", secs)?;
        }
        self.writer.write_all(&buffer)?;
        Ok(())
    }
}

/// Format file modification time in eza-compatible style (%e %b %H:%M for recent < 182 days, %e %b  %Y for older),
/// fixed 12 characters wide right-aligned.
#[allow(deprecated)]
pub fn format_mtime_eza(mtime: std::time::SystemTime) -> String {
    let now = std::time::SystemTime::now();
    let is_recent = match now.duration_since(mtime) {
        Ok(dur) => dur.as_secs() < 182 * 86400,
        Err(_) => false,
    };

    #[cfg(unix)]
    {
        let secs = match mtime.duration_since(std::time::UNIX_EPOCH) {
            Ok(dur) => dur.as_secs() as libc::time_t,
            Err(e) => -(e.duration().as_secs() as libc::time_t),
        };
        let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
        let tm_ptr = unsafe { libc::localtime_r(&secs, tm.as_mut_ptr()) };
        if !tm_ptr.is_null() {
            let tm = unsafe { tm.assume_init() };
            let fmt_str = if is_recent {
                c"%e %b %H:%M".as_ptr()
            } else {
                c"%e %b  %Y".as_ptr()
            };
            let mut buf = [0u8; 64];
            let len = unsafe {
                libc::strftime(
                    buf.as_mut_ptr() as *mut libc::c_char,
                    buf.len(),
                    fmt_str,
                    &tm,
                )
            };
            if len > 0 {
                let s = String::from_utf8_lossy(&buf[..len]);
                let char_count = s.chars().count();
                if char_count < 12 {
                    return format!("{}{s}", " ".repeat(12 - char_count));
                } else {
                    return s.into_owned();
                }
            }
        }
    }

    #[cfg(not(unix))]
    {
        if let Ok(dur) = mtime.duration_since(std::time::UNIX_EPOCH) {
            let total_secs = dur.as_secs();
            let days_since_epoch = (total_secs / 86400) as i64;
            let time_of_day = (total_secs % 86400) as u32;
            let hour = time_of_day / 3600;
            let min = (time_of_day % 3600) / 60;

            // Howard Hinnant's algorithm to convert days since 1970-01-01 to civil (year, month, day)
            let z = days_since_epoch + 719468;
            let era = if z >= 0 { z } else { z - 146096 } / 146097;
            let doe = (z - era * 146097) as u32;
            let yoe = (doe - doe / 1029 + doe / 1095 - doe / 146095) / 365;
            let y = yoe as i32 + era as i32 * 400;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            let year = if m <= 2 { y + 1 } else { y };

            const MONTHS: [&str; 12] = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            let mon_idx = (m - 1).min(11) as usize;
            let mon = MONTHS[mon_idx];

            let s = if is_recent {
                format!("{d:>2} {mon} {hour:02}:{min:02}")
            } else {
                format!("{d:>2} {mon}  {year:>4}")
            };
            let char_count = s.chars().count();
            if char_count < 12 {
                return format!("{}{s}", " ".repeat(12 - char_count));
            } else {
                return s;
            }
        }
    }

    let secs = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs:>12}")
}

/// Format file size in eza-compatible style (directories: "    -", bytes < 1024, 1.4k, 11k, 1.2M, 1.5G),
/// fixed 5 characters wide right-aligned.
pub fn format_size_eza(size: u64, is_dir: bool) -> String {
    if is_dir {
        return "    -".to_string();
    }
    if size < 1024 {
        return format!("{size:>5}");
    }
    let kib = size as f64 / 1024.0;
    if kib < 9.95 {
        return format!("{kib:>4.1}k");
    }
    if size < 1024 * 1024 {
        let kib_round = kib.round() as u64;
        return format!("{kib_round:>4}k");
    }
    let mib = size as f64 / (1024.0 * 1024.0);
    if mib < 9.95 {
        return format!("{mib:>4.1}M");
    }
    if size < 1024 * 1024 * 1024 {
        let mib_round = mib.round() as u64;
        return format!("{mib_round:>4}M");
    }
    let gib = size as f64 / (1024.0 * 1024.0 * 1024.0);
    if gib < 9.95 {
        format!("{gib:>4.1}G")
    } else {
        let gib_round = gib.round() as u64;
        format!("{gib_round:>4}G")
    }
}

impl OutputFormatter {
    /// Format a single file path that had no matches into a buffer (for -L / --files-without-match).
    pub fn format_file_without_match_with_column(
        &self,
        path: &Path,
        column: Option<&str>,
        null_separator: bool,
        buffer: &mut Vec<u8>,
    ) -> io::Result<()> {
        if self.quiet {
            return Ok(());
        }
        if self.json_output {
            let esc_path = escape_json(&path.to_string_lossy());
            let col_json = if let Some(col) = column {
                format!(",\"column\":\"{}\"", escape_json(col))
            } else {
                String::new()
            };
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                let raw_bytes = path.as_os_str().as_bytes();
                if std::str::from_utf8(raw_bytes).is_err() {
                    let b64 = base64_encode(raw_bytes);
                    writeln!(
                        buffer,
                        "{{\"data\":{{\"path\":{{\"bytes\":\"{b64}\",\"text\":\"{esc_path}\"}}{col_json}}},\"type\":\"file_without_match\"}}"
                    )?;
                    return Ok(());
                }
            }
            writeln!(
                buffer,
                "{{\"data\":{{\"path\":{{\"text\":\"{esc_path}\"}}{col_json}}},\"type\":\"file_without_match\"}}"
            )?;
            return Ok(());
        }
        if null_separator || self.null_separator {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                buffer.write_all(path.as_os_str().as_bytes())?;
                buffer.write_all(b"\0")?;
                return Ok(());
            }
            #[cfg(not(unix))]
            {
                write!(buffer, "{}\0", path.to_string_lossy())?;
                return Ok(());
            }
        }

        let use_color = self.should_use_color();
        let col_prefix = match column {
            Some(col) if use_color => format!("\x1b[38;5;108m{col}\x1b[0m  "),
            Some(col) => format!("{col}  "),
            None => String::new(),
        };

        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            if !col_prefix.is_empty() {
                buffer.write_all(col_prefix.as_bytes())?;
            }
            buffer.write_all(path.as_os_str().as_bytes())?;
            buffer.write_all(b"\n")?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            writeln!(buffer, "{col_prefix}{}", path.to_string_lossy())
        }
    }

    /// Print a single file path that had no matches (for -L / --files-without-match),
    /// with an optional sort column.
    pub fn print_file_without_match_with_column(
        &mut self,
        path: &Path,
        column: Option<&str>,
        null_separator: bool,
    ) -> io::Result<()> {
        let mut buffer = Vec::new();
        self.format_file_without_match_with_column(path, column, null_separator, &mut buffer)?;
        if !buffer.is_empty() {
            self.account_json_payload(&buffer);
            self.writer.write_all(&buffer)?;
        }
        Ok(())
    }

    /// Print a single file path that had no matches (for -L / --files-without-match).
    pub fn print_file_without_match(
        &mut self,
        path: &Path,
        null_separator: bool,
    ) -> io::Result<()> {
        self.print_file_without_match_with_column(path, None, null_separator)
    }

    /// Format a single discovered entry path with an optional formatted column into a buffer.
    pub fn format_entry_with_column(
        &self,
        path: &Path,
        is_dir: bool,
        is_symlink: bool,
        column: Option<&str>,
        null_separator: bool,
        buffer: &mut Vec<u8>,
    ) -> io::Result<()> {
        if self.quiet {
            return Ok(());
        }
        if self.json_output {
            let esc_path = escape_json(&path.to_string_lossy());
            let col_json = if let Some(col) = column {
                format!(",\"column\":\"{}\"", escape_json(col))
            } else {
                String::new()
            };
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                let raw_bytes = path.as_os_str().as_bytes();
                if std::str::from_utf8(raw_bytes).is_err() {
                    let b64 = base64_encode(raw_bytes);
                    writeln!(
                        buffer,
                        "{{\"data\":{{\"path\":{{\"bytes\":\"{b64}\",\"text\":\"{esc_path}\"}}{col_json}}},\"type\":\"entry\"}}"
                    )?;
                    return Ok(());
                }
            }
            writeln!(
                buffer,
                "{{\"data\":{{\"path\":{{\"text\":\"{esc_path}\"}}{col_json}}},\"type\":\"entry\"}}"
            )?;
            return Ok(());
        }
        if null_separator || self.null_separator {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                buffer.write_all(path.as_os_str().as_bytes())?;
                buffer.write_all(b"\0")?;
                return Ok(());
            }
            #[cfg(not(unix))]
            {
                write!(buffer, "{}\0", path.to_string_lossy())?;
                return Ok(());
            }
        }

        let use_color = self.should_use_color();
        let use_links = self.should_use_hyperlinks();

        let col_prefix = match column {
            Some(col) if use_color => format!("\x1b[38;5;108m{col}\x1b[0m  "),
            Some(col) => format!("{col}  "),
            None => String::new(),
        };

        if !use_color && !use_links {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                let bytes = path.as_os_str().as_bytes();
                let clean_bytes = if bytes.ends_with(b"/") && bytes.len() > 1 {
                    &bytes[..bytes.len() - 1]
                } else {
                    bytes
                };
                if !col_prefix.is_empty() {
                    buffer.write_all(col_prefix.as_bytes())?;
                }
                buffer.write_all(clean_bytes)?;
                if is_dir {
                    buffer.write_all(b"/")?;
                }
                buffer.write_all(b"\n")?;
                return Ok(());
            }
        }

        let clean_path = path.to_string_lossy();
        let clean = clean_path.trim_end_matches(['/', '\\']);
        let p = Path::new(clean);
        let parent_opt = p.parent().filter(|p| !p.as_os_str().is_empty());
        let file_name = p.file_name().unwrap_or(p.as_os_str()).to_string_lossy();

        let parent_display = if let Some(parent) = parent_opt {
            let s = parent.to_string_lossy();
            if s.ends_with('/') || s.ends_with('\\') {
                s.into_owned()
            } else {
                #[cfg(windows)]
                {
                    format!("{s}\\")
                }
                #[cfg(not(windows))]
                {
                    format!("{s}/")
                }
            }
        } else {
            String::new()
        };

        let styled = if use_color {
            let dim_parent = if parent_display.is_empty() {
                String::new()
            } else {
                format!("\x1b[38;5;81m{parent_display}\x1b[0m")
            };

            if is_dir {
                format!("{dim_parent}\x1b[38;5;81m{file_name}/\x1b[0m")
            } else if is_symlink {
                format!("{dim_parent}\x1b[38;5;203m{file_name}\x1b[0m")
            } else {
                let (prefix, suffix) = get_fd_file_style(path, &file_name);
                if prefix.is_empty() {
                    format!("{dim_parent}{file_name}")
                } else {
                    format!("{dim_parent}{prefix}{file_name}{suffix}")
                }
            }
        } else if is_dir {
            format!("{clean}/")
        } else {
            clean.to_string()
        };

        let display_str = if use_links {
            let link_url = if is_dir {
                self.build_directory_hyperlink_url(path)
            } else {
                self.build_hyperlink_url(path, 1, 1)
            };
            self.format_hyperlink(&link_url, &styled)
        } else {
            styled
        };

        writeln!(buffer, "{col_prefix}{display_str}")?;
        Ok(())
    }

    /// Format a single discovered entry path into a buffer with fd-style coloring and hyperlinks.
    pub fn format_entry_path_styled(
        &self,
        path: &Path,
        is_dir: bool,
        is_symlink: bool,
        null_separator: bool,
        buffer: &mut Vec<u8>,
    ) -> io::Result<()> {
        self.format_entry_with_column(path, is_dir, is_symlink, None, null_separator, buffer)
    }

    /// Print a single discovered entry path with an optional formatted column (e.g. date modified, size, len)
    /// preceding the path.
    pub fn print_entry_with_column(
        &mut self,
        path: &Path,
        is_dir: bool,
        is_symlink: bool,
        column: Option<&str>,
        null_separator: bool,
    ) -> io::Result<()> {
        let mut buffer = Vec::new();
        self.format_entry_with_column(
            path,
            is_dir,
            is_symlink,
            column,
            null_separator,
            &mut buffer,
        )?;
        if !buffer.is_empty() {
            self.account_json_payload(&buffer);
            self.writer.write_all(&buffer)?;
        }
        Ok(())
    }

    /// Print a single discovered entry path (for file discovery mode) with fd-style coloring and hyperlinks.
    pub fn print_entry_path_styled(
        &mut self,
        path: &Path,
        is_dir: bool,
        is_symlink: bool,
        null_separator: bool,
    ) -> io::Result<()> {
        self.print_entry_with_column(path, is_dir, is_symlink, None, null_separator)
    }

    /// Print a single discovered entry path (for file discovery mode).
    pub fn print_entry_path(&mut self, path: &Path, null_separator: bool) -> io::Result<()> {
        self.print_entry_path_styled(path, false, false, null_separator)
    }

    /// Format matches for a single file into a buffer with an optional sort column.
    pub fn format_file_matches_with_column(
        &self,
        path: &Path,
        matches: &[MatchRecord<'_>],
        column: Option<&str>,
        buffer: &mut Vec<u8>,
    ) -> io::Result<()> {
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
                let line_str = String::from_utf8_lossy(m.line_bytes);
                let esc_line = escape_json(&line_str);
                let abs_offset = m.line_byte_offset;

                if m.is_context {
                    writeln!(
                        buffer,
                        "{{\"data\":{{\"absolute_offset\":{abs_offset},\"line_number\":{},\"lines\":{{\"text\":\"{esc_line}\\n\"}},\"path\":{path_json},\"submatches\":[]}},\"type\":\"context\"}}",
                        m.line_number
                    )?;
                } else {
                    matched_lines += 1;
                    match_count += m.match_spans.len();

                    let mut submatches = Vec::with_capacity(m.match_spans.len());
                    for (start, end) in &m.match_spans {
                        if *start <= m.line_bytes.len()
                            && *end <= m.line_bytes.len()
                            && *start < *end
                        {
                            let match_text = String::from_utf8_lossy(&m.line_bytes[*start..*end]);
                            let esc_match = escape_json(&match_text);
                            submatches.push(format!(
                                "{{\"end\":{end},\"match\":{{\"text\":\"{esc_match}\"}},\"start\":{start}}}"
                            ));
                        }
                    }

                    writeln!(
                        buffer,
                        "{{\"data\":{{\"absolute_offset\":{abs_offset},\"line_number\":{},\"lines\":{{\"text\":\"{esc_line}\\n\"}},\"path\":{path_json},\"submatches\":[{}]}},\"type\":\"match\"}}",
                        m.line_number,
                        submatches.join(",")
                    )?;
                }
            }

            let file_elapsed = file_start.elapsed();
            let bytes_searched = std::fs::metadata(path)
                .map(|m| m.len())
                .unwrap_or_else(|_| {
                    matches
                        .last()
                        .map(|m| (m.line_byte_offset + m.line_bytes.len()) as u64)
                        .unwrap_or(0)
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
                write!(buffer, "{path_str}\0")?;
            } else {
                let use_color = self.should_use_color();
                let col_prefix = match column {
                    Some(col) if use_color => format!("\x1b[38;5;108m{col}\x1b[0m  "),
                    Some(col) => format!("{col}  "),
                    None => String::new(),
                };
                writeln!(buffer, "{col_prefix}{path_str}")?;
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
                .map(|m| m.match_spans.len())
                .sum();
            if self.no_filename {
                write!(buffer, "{total_spans}{delim}")?;
            } else {
                write!(buffer, "{path_str}:{total_spans}{delim}")?;
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
                write!(buffer, "{path_str}:{count}{delim}")?;
            }
            return Ok(());
        }

        let use_color = self.should_use_color();
        if !self.is_tty
            && !use_color
            && !self.should_use_hyperlinks()
            && !self.show_line_len
            && column.is_none()
            && self.max_columns.is_none_or(|limit| limit == 0)
            && matches.iter().all(|m| !m.line_bytes.contains(&0))
        {
            self.write_plain_records(buffer, path, matches)?;
            return Ok(());
        }

        // Interactive TTY mode with headings
        if self.is_tty && self.show_heading && !self.no_filename {
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

            let has_context = matches.iter().any(|m| m.is_context);
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

                let is_binary_match = memchr::memchr(0x00, m.line_bytes).is_some();
                let (line_text, active_spans) =
                    self.format_line_content(m.line_byte_offset, m.line_bytes, &m.match_spans);

                if is_binary_match {
                    writeln!(buffer, "{line_text}")?;
                    continue;
                }

                if self.only_matching {
                    for ((start, end), &(source_start, _)) in active_spans
                        .iter()
                        .zip(m.match_spans.iter().filter(|(a, b)| a < b))
                    {
                        if *start < *end && line_text.get(*start..*end).is_some() {
                            let span = &line_text[*start..*end];
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

                if self.show_line_len && !m.is_context {
                    let len_str = format!("{:>5}", m.line_bytes.len());
                    if use_color {
                        write!(buffer, "\x1b[38;5;108m{len_str}\x1b[0m  ")?;
                    } else {
                        write!(buffer, "{len_str}  ")?;
                    }
                }

                let sep = if m.is_context { '-' } else { ':' };

                if self.show_column || self.show_byte_offset {
                    if self.show_line_numbers {
                        let line_url = self.build_hyperlink_url(path, m.line_number, 1);
                        let linked_num =
                            self.format_hyperlink(&line_url, &m.line_number.to_string());
                        if use_color {
                            if m.is_context {
                                write!(buffer, "\x1b[38;5;242m{linked_num}{sep}\x1b[0m")?;
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
                    if self.show_column && !m.is_context {
                        let col = m
                            .match_spans
                            .first()
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
                        if m.is_context {
                            write!(buffer, "\x1b[38;5;242m{linked_num}{sep}\x1b[0m ")?;
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

                // Render line with highlighted spans
                if use_color && !active_spans.is_empty() {
                    let mut last_idx = 0;
                    for (start, end) in &active_spans {
                        if *start > last_idx && line_text.get(last_idx..*start).is_some() {
                            write!(buffer, "{}", &line_text[last_idx..*start])?;
                        }
                        if *start >= last_idx
                            && *start < *end
                            && line_text.get(*start..*end).is_some()
                        {
                            write!(
                                buffer,
                                "{}{}\x1b[0m",
                                self.colors.match_highlight,
                                &line_text[*start..*end]
                            )?;
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

            // Trailing newline between files on TTY
            writeln!(buffer)?;
        } else {
            // Standard POSIX / Piped format: [path:][line:][col:][offset:]content
            let has_context = matches.iter().any(|m| m.is_context);
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

                let is_binary_match = memchr::memchr(0x00, m.line_bytes).is_some();
                let (line_text, active_spans) =
                    self.format_line_content(m.line_byte_offset, m.line_bytes, &m.match_spans);

                let col_color = &self.colors.column;
                let path_color = &self.colors.path;
                let line_color = &self.colors.line_number;
                let match_color = &self.colors.match_highlight;
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
                        .zip(m.match_spans.iter().filter(|(a, b)| a < b))
                    {
                        if *start < *end && line_text.get(*start..*end).is_some() {
                            let span = &line_text[*start..*end];
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

                let sep = if m.is_context { '-' } else { ':' };

                if !col_prefix.is_empty() {
                    write!(buffer, "{col_prefix}")?;
                }
                if self.show_line_len && !m.is_context {
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
                        if m.is_context {
                            write!(buffer, "\x1b[38;5;242m{}\x1b[0m{sep}", m.line_number)?;
                        } else {
                            write!(buffer, "{line_color}{}\x1b[0m{sep}", m.line_number)?;
                        }
                    } else {
                        write!(buffer, "{}{sep}", m.line_number)?;
                    }
                }
                if self.show_column && !m.is_context {
                    let col = m
                        .match_spans
                        .first()
                        .map(|s| byte_to_char_column(m.line_bytes, s.0))
                        .unwrap_or(1);
                    write!(buffer, "{col}{sep}")?;
                }
                if self.show_byte_offset {
                    write!(buffer, "{}{sep}", m.line_byte_offset)?;
                }

                if use_color && !active_spans.is_empty() {
                    let mut last_idx = 0;
                    for (start, end) in &active_spans {
                        if *start > last_idx && line_text.get(last_idx..*start).is_some() {
                            write!(buffer, "{}", &line_text[last_idx..*start])?;
                        }
                        if *start >= last_idx
                            && *start < *end
                            && line_text.get(*start..*end).is_some()
                        {
                            write!(buffer, "{match_color}{}\x1b[0m", &line_text[*start..*end])?;
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

    /// Print matches for a single file with an optional sort column preceding the output.
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

    /// Atomically write pre-formatted raw bytes to the underlying writer.
    pub fn write_raw_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        if !bytes.is_empty() {
            self.account_json_payload(bytes);
            self.writer.write_all(bytes)?;
        }
        Ok(())
    }

    /// Create a detached copy of this formatter configured with identical settings,
    /// writing to `std::io::sink()`. This allows thread-local buffer formatting without
    /// holding locks on shared output writers.
    pub fn clone_detached(&self) -> Self {
        Self {
            writer: Box::new(std::io::sink()),
            is_tty: self.is_tty,
            color: self.color,
            hyperlinks: self.hyperlinks,
            hyperlink_format: self.hyperlink_format.clone(),
            show_line_numbers: self.show_line_numbers,
            show_column: self.show_column,
            show_line_len: self.show_line_len,
            show_byte_offset: self.show_byte_offset,
            show_heading: self.show_heading,
            null_separator: self.null_separator,
            only_matching: self.only_matching,
            quiet: self.quiet,
            files_with_matches: self.files_with_matches,
            count_only: self.count_only,
            count_matches: self.count_matches,
            json_output: self.json_output,
            no_filename: self.no_filename,
            hostname: self.hostname.clone(),
            max_columns: self.max_columns,
            allow_binary: self.allow_binary,
            raw_binary_text: self.raw_binary_text,
            json_bytes_printed: 0,
            colors: self.colors.clone(),
        }
    }

    /// Print matches for a single file.
    pub fn print_file_matches(
        &mut self,
        path: &Path,
        matches: &[MatchRecord<'_>],
    ) -> io::Result<()> {
        self.print_file_matches_with_column(path, matches, None)
    }
}

impl EntrySink for OutputFormatter {
    fn on_entry(&mut self, entry: &DirEntry) -> io::Result<()> {
        let path = entry.full_path();
        self.print_entry_path_styled(&path, entry.is_dir, entry.is_symlink, false)
    }
}

impl Printer for OutputFormatter {
    fn print_file_matches(&mut self, path: &Path, matches: &[MatchRecord<'_>]) -> io::Result<()> {
        self.print_file_matches_with_column(path, matches, None)
    }

    fn print_summary(&mut self, total_matches: usize, total_files: usize) -> io::Result<()> {
        let mut buffer = Vec::new();
        writeln!(
            buffer,
            "{total_matches} matches across {total_files} files."
        )?;
        self.writer.write_all(&buffer)?;
        Ok(())
    }
}

/// Helper to get local machine hostname for OSC 8 hyperlinks.
fn get_hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "localhost".to_string())
}

#[cfg(test)]
impl Default for OutputFormatter {
    fn default() -> Self {
        Self {
            writer: Box::new(Vec::<u8>::new()),
            is_tty: false,
            color: ColorChoice::Never,
            hyperlinks: HyperlinkChoice::Never,
            hyperlink_format: String::new(),
            show_line_numbers: true,
            show_column: false,
            show_line_len: false,
            show_byte_offset: false,
            show_heading: false,
            null_separator: false,
            only_matching: false,
            quiet: false,
            files_with_matches: false,
            count_only: false,
            count_matches: false,
            json_output: false,
            no_filename: false,
            hostname: "test".to_string(),
            max_columns: None,
            allow_binary: false,
            raw_binary_text: false,
            json_bytes_printed: 0,
            colors: crate::config::ColorTheme::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuffer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn count_excludes_context_and_reports_zero() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            count_only: true,
            ..OutputFormatter::default()
        };
        formatter
            .print_file_matches(
                Path::new("input"),
                &[
                    MatchRecord::context(1, 0, b"before"),
                    MatchRecord::new(2, 7, b"hit", vec![(0, 3)]),
                ],
            )
            .unwrap();
        formatter
            .print_file_matches(Path::new("empty"), &[])
            .unwrap();
        assert_eq!(*buf.0.lock().unwrap(), b"input:1\nempty:0\n");
    }

    #[test]
    fn preserve_invalid_utf8_bytes_and_match_offsets() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            no_filename: true,
            show_line_numbers: false,
            ..OutputFormatter::default()
        };
        let records = [MatchRecord::new(1, 0, b"\xffneedle", vec![(1, 7)])];
        formatter
            .print_file_matches(Path::new("input"), &records)
            .unwrap();
        formatter.only_matching = true;
        formatter
            .print_file_matches(Path::new("input"), &records)
            .unwrap();
        assert_eq!(*buf.0.lock().unwrap(), b"\xffneedle\nneedle\n");
        formatter.is_tty = true;
        let (display, spans) = formatter.format_line_content(0, b"\xff\x1bneedle", &[(2, 8)]);
        assert_eq!(&display[spans[0].0..spans[0].1], "needle");
    }

    #[test]
    fn test_posix_piped_output_format() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            ..OutputFormatter::default()
        };

        let matches = vec![MatchRecord::new(
            42,
            100,
            b"let conn = connect();",
            vec![(11, 18)],
        )];

        formatter
            .print_file_matches(Path::new("src/main.rs"), &matches)
            .unwrap();

        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert_eq!(output, "src/main.rs:42:let conn = connect();\n");
    }

    #[test]
    fn test_colored_non_heading_format() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            color: ColorChoice::Always,
            ..OutputFormatter::default()
        };

        let matches = vec![MatchRecord::new(
            7,
            100,
            b"fn main() -> ExitCode {",
            vec![(0, 2)],
        )];

        formatter
            .print_file_matches(Path::new("src/main.rs"), &matches)
            .unwrap();

        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert_eq!(
            output,
            "\x1b[35msrc/main.rs\x1b[0m:\x1b[32m7\x1b[0m:\x1b[1;31mfn\x1b[0m main() -> ExitCode {\n"
        );
    }

    #[test]
    fn test_json_lines_output_format() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            json_output: true,
            ..OutputFormatter::default()
        };

        let matches = vec![MatchRecord::new(
            7,
            116,
            b"fn main() -> ExitCode {",
            vec![(0, 7)],
        )];

        formatter
            .print_file_matches(Path::new("src/main.rs"), &matches)
            .unwrap();

        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(output.contains("\"type\":\"begin\""));
        assert!(output.contains("\"type\":\"match\""));
        assert!(output.contains("\"type\":\"end\""));
        assert!(output.contains("\"path\":{\"text\":\"src/main.rs\"}"));
        assert!(output.contains("\"match\":{\"text\":\"fn main\"}"));
        assert!(output.contains("\"line_number\":7"));
        assert!(output.contains("\"absolute_offset\":116"));
    }

    #[test]
    fn json_summary_reports_accumulated_payload_bytes() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            json_output: true,
            ..OutputFormatter::default()
        };
        let matches = vec![MatchRecord::new(1, 0, b"needle", vec![(0, 6)])];

        formatter
            .print_file_matches(Path::new("input.txt"), &matches)
            .unwrap();
        let before_summary = buf.0.lock().unwrap().clone();
        let expected = json_payload_bytes(&before_summary);
        assert!(expected > 0);

        formatter
            .print_json_summary(1, 1, 1, 1, 6, std::time::Duration::from_millis(1))
            .unwrap();
        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        let summary = output.lines().last().unwrap();
        assert!(summary.contains("\"type\":\"summary\""));
        assert!(summary.contains(&format!("\"bytes_printed\":{expected}")));
        assert!(summary.contains("\"binary_files_skipped\":0"));

        let buf_skipped = SharedBuffer::default();
        let mut formatter_skipped = OutputFormatter {
            writer: Box::new(buf_skipped.clone()),
            json_output: true,
            ..OutputFormatter::default()
        };
        formatter_skipped
            .print_json_summary_with_skipped(1, 1, 1, 1, 6, 5, std::time::Duration::from_millis(1))
            .unwrap();
        let out_skipped = String::from_utf8(buf_skipped.0.lock().unwrap().clone()).unwrap();
        assert!(out_skipped.contains("\"binary_files_skipped\":5"));
    }

    #[test]
    fn test_stats_summary_output_format() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            ..OutputFormatter::default()
        };

        formatter
            .print_stats_summary(10, 5, 2, 4, 2048, std::time::Duration::from_millis(15))
            .unwrap();

        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(output.contains("10 matches"));
        assert!(output.contains("5 matched lines"));
        assert!(output.contains("2 files contained matches"));
        assert!(output.contains("4 files searched"));
        assert!(output.contains("2048 bytes searched"));
        assert!(output.contains("MB/s"));
        assert!(output.contains("seconds total"));

        let buf_skipped = SharedBuffer::default();
        let mut formatter_skipped = OutputFormatter {
            writer: Box::new(buf_skipped.clone()),
            ..OutputFormatter::default()
        };
        formatter_skipped
            .print_stats_summary_with_skipped(
                10,
                5,
                2,
                4,
                2048,
                3,
                std::time::Duration::from_millis(15),
            )
            .unwrap();
        let out_skipped = String::from_utf8(buf_skipped.0.lock().unwrap().clone()).unwrap();
        assert!(out_skipped.contains("3 binary files skipped"));
    }

    #[test]
    fn test_column_number_format() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            show_column: true,
            ..OutputFormatter::default()
        };

        let matches = vec![MatchRecord::new(
            42,
            100,
            b"let conn = connect();",
            vec![(11, 18)],
        )];

        formatter
            .print_file_matches(Path::new("src/main.rs"), &matches)
            .unwrap();

        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert_eq!(output, "src/main.rs:42:12:let conn = connect();\n");
    }

    #[test]
    fn test_column_number_unicode_characters() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            show_column: true,
            ..OutputFormatter::default()
        };

        // "🦀 match": '🦀' takes 4 bytes (0..4), ' ' is byte 4, "match" is bytes 5..10
        let matches = vec![MatchRecord::new(1, 0, "🦀 match".as_bytes(), vec![(5, 10)])];

        formatter
            .print_file_matches(Path::new("unicode.txt"), &matches)
            .unwrap();

        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        // Character column: '🦀' is char 1, ' ' is char 2, 'm' is char 3 -> column 3
        assert_eq!(output, "unicode.txt:1:3:🦀 match\n");
    }

    #[test]
    fn test_byte_offset_format() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            show_byte_offset: true,
            ..OutputFormatter::default()
        };

        let matches = vec![MatchRecord::new(
            42,
            100,
            b"let conn = connect();",
            vec![(11, 18)],
        )];

        formatter
            .print_file_matches(Path::new("src/main.rs"), &matches)
            .unwrap();

        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert_eq!(output, "src/main.rs:42:100:let conn = connect();\n");
    }

    #[test]
    fn test_column_and_byte_offset_format() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            show_column: true,
            show_byte_offset: true,
            ..OutputFormatter::default()
        };

        let matches = vec![MatchRecord::new(
            42,
            100,
            b"let conn = connect();",
            vec![(11, 18)],
        )];

        formatter
            .print_file_matches(Path::new("src/main.rs"), &matches)
            .unwrap();

        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert_eq!(output, "src/main.rs:42:12:100:let conn = connect();\n");
    }

    #[test]
    fn test_count_matches_format() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            count_matches: true,
            ..OutputFormatter::default()
        };

        // File with 2 matches on line 1, and 1 match on line 5 (total 3 matches, 2 matched lines)
        let matches = vec![
            MatchRecord::new(
                1,
                0,
                b"first match and second match",
                vec![(0, 5), (16, 21)],
            ),
            MatchRecord::new(5, 50, b"third match", vec![(0, 5)]),
        ];

        formatter
            .print_file_matches(Path::new("counted.rs"), &matches)
            .unwrap();

        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert_eq!(output, "counted.rs:3\n");
    }

    #[test]
    fn test_only_matching_format() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            only_matching: true,
            ..OutputFormatter::default()
        };

        let matches = vec![
            MatchRecord::new(10, 50, b"hello world here", vec![(6, 11)]), // "world"
        ];

        formatter
            .print_file_matches(Path::new("file.txt"), &matches)
            .unwrap();

        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert_eq!(output, "file.txt:10:world\n");
    }

    #[test]
    fn test_files_with_matches_format() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            files_with_matches: true,
            ..OutputFormatter::default()
        };

        let matches = vec![MatchRecord::new(1, 0, b"match line", vec![(0, 5)])];

        formatter
            .print_file_matches(Path::new("target.rs"), &matches)
            .unwrap();

        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert_eq!(output, "target.rs\n");
    }

    #[test]
    fn test_count_only_format() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            count_only: true,
            ..OutputFormatter::default()
        };

        let matches = vec![
            MatchRecord::new(1, 0, b"first match", vec![(0, 5)]),
            MatchRecord::new(5, 50, b"second match", vec![(0, 6)]),
        ];

        formatter
            .print_file_matches(Path::new("counted.rs"), &matches)
            .unwrap();

        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert_eq!(output, "counted.rs:2\n");
    }

    #[test]
    fn test_context_lines_format() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            ..OutputFormatter::default()
        };

        let matches = vec![
            MatchRecord::context(1, 0, b"context before"),
            MatchRecord::new(2, 20, b"matched line", vec![(0, 7)]),
            MatchRecord::context(3, 40, b"context after"),
            // Gap
            MatchRecord::new(10, 100, b"second matched line", vec![(0, 6)]),
        ];

        formatter
            .print_file_matches(Path::new("ctx.rs"), &matches)
            .unwrap();

        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert_eq!(
            output,
            "ctx.rs-1-context before\nctx.rs:2:matched line\nctx.rs-3-context after\n--\nctx.rs:10:second matched line\n"
        );
    }

    #[test]
    fn test_max_columns_line_truncation() {
        let buf = SharedBuffer::default();
        let formatter = OutputFormatter {
            writer: Box::new(buf),
            is_tty: true,
            max_columns: Some(40),
            ..OutputFormatter::default()
        };

        // Create a 200-byte line with a match in the middle
        let mut line = vec![b'a'; 200];
        line[100..106].copy_from_slice(b"TARGET");
        let spans = vec![(100, 106)];

        let (text, active_spans) = formatter.format_line_content(0, &line, &spans);
        assert!(text.contains("TARGET"));
        assert!(text.contains("... [omitted"));
        assert!(!active_spans.is_empty());
        let (s, e) = active_spans[0];
        assert_eq!(&text[s..e], "TARGET");
    }

    #[test]
    fn test_control_character_sanitization() {
        let buf = SharedBuffer::default();
        let formatter = OutputFormatter {
            writer: Box::new(buf),
            is_tty: true,
            ..OutputFormatter::default()
        };

        let raw = b"hello\rworld\x1b[31mred";
        let (text, _) = formatter.format_line_content(0, raw, &[]);
        assert!(!text.contains('\r'));
        assert!(!text.contains('\x1b'));
        assert!(text.contains("^M"));
        assert!(text.contains("^["));
    }

    #[test]
    fn test_null_byte_binary_suppression() {
        let buf = SharedBuffer::default();
        let formatter = OutputFormatter {
            writer: Box::new(buf),
            ..OutputFormatter::default()
        };

        let raw = b"sqlite header\x00data matching query";
        let (text, spans) = formatter.format_line_content(0, raw, &[(20, 25)]);
        assert_eq!(text, "[Binary file matches]");
        assert!(spans.is_empty());
    }

    #[test]
    fn test_mini_hexdump_formatting() {
        let buf = SharedBuffer::default();
        let formatter = OutputFormatter {
            writer: Box::new(buf),
            allow_binary: true,
            ..OutputFormatter::default()
        };

        let raw = b"\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let (text, spans) = formatter.format_line_content(0x1000, raw, &[(1, 4)]);
        assert!(spans.is_empty());
        assert!(text.contains("0x00001000:"));
        assert!(text.contains("7f 45 4c 46"));
        assert!(text.contains("|.ELF"));
    }

    #[test]
    fn test_print_entry_path_normal_and_null() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            ..OutputFormatter::default()
        };

        formatter
            .print_entry_path(Path::new("src/main.rs"), false)
            .unwrap();
        assert_eq!(*buf.0.lock().unwrap(), b"src/main.rs\n");

        buf.0.lock().unwrap().clear();
        formatter
            .print_entry_path(Path::new("src/lib.rs"), true)
            .unwrap();
        assert_eq!(*buf.0.lock().unwrap(), b"src/lib.rs\0");
    }

    #[test]
    fn test_print_entry_path_quiet_suppression() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            quiet: true,
            ..OutputFormatter::default()
        };

        formatter
            .print_entry_path(Path::new("src/main.rs"), false)
            .unwrap();
        assert!(buf.0.lock().unwrap().is_empty());
    }

    #[test]
    fn test_print_entry_path_json_format() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            json_output: true,
            ..OutputFormatter::default()
        };

        formatter
            .print_entry_path(Path::new("src/main.rs"), false)
            .unwrap();
        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert_eq!(
            output.trim(),
            "{\"data\":{\"path\":{\"text\":\"src/main.rs\"}},\"type\":\"entry\"}"
        );
    }

    #[test]
    fn test_print_entry_path_color_and_hyperlinks() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            color: ColorChoice::Always,
            hyperlinks: HyperlinkChoice::Never,
            ..OutputFormatter::default()
        };

        let sep = std::path::MAIN_SEPARATOR;
        // Source code file (.rs) with parent directory: parent 81, code 48
        formatter
            .print_entry_path_styled(Path::new("src/main.rs"), false, false, false)
            .unwrap();
        assert_eq!(
            *buf.0.lock().unwrap(),
            format!("\x1b[38;5;81msrc{sep}\x1b[0m\x1b[38;5;48mmain.rs\x1b[0m\n").into_bytes()
        );

        // Subdirectory: parent 81, dir 81
        buf.0.lock().unwrap().clear();
        formatter
            .print_entry_path_styled(Path::new("src/nested"), true, false, false)
            .unwrap();
        assert_eq!(
            *buf.0.lock().unwrap(),
            format!("\x1b[38;5;81msrc{sep}\x1b[0m\x1b[38;5;81mnested/\x1b[0m\n").into_bytes()
        );

        // Symlink: parent 81, link 203
        buf.0.lock().unwrap().clear();
        formatter
            .print_entry_path_styled(Path::new("src/link.rs"), false, true, false)
            .unwrap();
        assert_eq!(
            *buf.0.lock().unwrap(),
            format!("\x1b[38;5;81msrc{sep}\x1b[0m\x1b[38;5;203mlink.rs\x1b[0m\n").into_bytes()
        );

        // Root-level directory: 81
        buf.0.lock().unwrap().clear();
        formatter
            .print_entry_path_styled(Path::new("src"), true, false, false)
            .unwrap();
        assert_eq!(*buf.0.lock().unwrap(), b"\x1b[38;5;81msrc/\x1b[0m\n");

        // Config file (.toml): 149
        buf.0.lock().unwrap().clear();
        formatter
            .print_entry_path_styled(Path::new("Cargo.toml"), false, false, false)
            .unwrap();
        assert_eq!(*buf.0.lock().unwrap(), b"\x1b[38;5;149mCargo.toml\x1b[0m\n");

        // Markdown file (.md): 185
        buf.0.lock().unwrap().clear();
        formatter
            .print_entry_path_styled(Path::new("README.md"), false, false, false)
            .unwrap();
        assert_eq!(*buf.0.lock().unwrap(), b"\x1b[38;5;185mREADME.md\x1b[0m\n");

        // Archive file (.tar.gz): 4;38;5;203m
        buf.0.lock().unwrap().clear();
        formatter
            .print_entry_path_styled(Path::new("archive.tar.gz"), false, false, false)
            .unwrap();
        assert_eq!(
            *buf.0.lock().unwrap(),
            b"\x1b[4;38;5;203marchive.tar.gz\x1b[0m\n"
        );

        // Uncolored mode appends slash to directory
        buf.0.lock().unwrap().clear();
        formatter.color = ColorChoice::Never;
        formatter
            .print_entry_path_styled(Path::new("src"), true, false, false)
            .unwrap();
        assert_eq!(*buf.0.lock().unwrap(), b"src/\n");

        // Hyperlinks enabled
        buf.0.lock().unwrap().clear();
        formatter.hyperlinks = HyperlinkChoice::Always;
        formatter
            .print_entry_path_styled(Path::new("src/main.rs"), false, false, false)
            .unwrap();
        let out_str = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(out_str.starts_with("\x1b]8;;"));
        assert!(out_str.contains("file://"));
        assert!(out_str.ends_with("\x1b]8;;\x1b\\\n"));
    }

    #[test]
    fn test_entry_sink_implementation() {
        use std::path::PathBuf;
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            ..OutputFormatter::default()
        };

        let entry = DirEntry::new(
            PathBuf::from("src"),
            b"main.rs".to_vec(),
            false,
            false,
            None,
        );
        formatter.on_entry(&entry).unwrap();
        let expected = format!("src{}main.rs\n", std::path::MAIN_SEPARATOR);
        assert_eq!(*buf.0.lock().unwrap(), expected.as_bytes());
    }

    #[test]
    #[cfg(unix)]
    fn test_non_utf8_path_raw_bytes_and_json() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            color: ColorChoice::Never,
            hyperlinks: HyperlinkChoice::Never,
            ..OutputFormatter::default()
        };

        #[cfg(unix)]
        {
            let non_utf8_os = OsStr::from_bytes(b"bad-\xff.txt");
            let p = Path::new(non_utf8_os);

            // 1. Unstyled redirected text must preserve exact raw bytes
            formatter
                .print_entry_path_styled(p, false, false, false)
                .unwrap();
            assert_eq!(*buf.0.lock().unwrap(), b"bad-\xff.txt\n");

            // 2. print_file_without_match must preserve exact raw bytes
            buf.0.lock().unwrap().clear();
            formatter.print_file_without_match(p, false).unwrap();
            assert_eq!(*buf.0.lock().unwrap(), b"bad-\xff.txt\n");

            // 3. JSON format must include base64 bytes for non-UTF-8 path
            buf.0.lock().unwrap().clear();
            formatter.json_output = true;
            formatter
                .print_entry_path_styled(p, false, false, false)
                .unwrap();
            let json_str = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
            assert!(json_str.contains("\"bytes\":\"YmFkLf8udHh0\""));
            assert!(json_str.contains("\"type\":\"entry\""));

            // 4. print_file_matches JSON format must include base64 bytes for non-UTF-8 path
            buf.0.lock().unwrap().clear();
            let matches = vec![MatchRecord::new(1, 0, b"content match", vec![(0, 7)])];
            formatter.print_file_matches(p, &matches).unwrap();
            let json_matches = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
            assert!(json_matches.contains("\"bytes\":\"YmFkLf8udHh0\""));
            assert!(json_matches.contains("\"type\":\"begin\""));
            assert!(json_matches.contains("\"type\":\"match\""));
            assert!(json_matches.contains("\"type\":\"end\""));
        }
    }

    #[test]
    fn test_format_size_eza() {
        assert_eq!(format_size_eza(0, true), "    -");
        assert_eq!(format_size_eza(100, true), "    -");
        assert_eq!(format_size_eza(0, false), "    0");
        assert_eq!(format_size_eza(860, false), "  860");
        assert_eq!(format_size_eza(1023, false), " 1023");
        assert_eq!(format_size_eza(1024, false), " 1.0k");
        assert_eq!(format_size_eza(1433, false), " 1.4k");
        assert_eq!(format_size_eza(10 * 1024, false), "  10k");
        assert_eq!(format_size_eza(100 * 1024, false), " 100k");
        assert_eq!(format_size_eza(1024 * 1024, false), " 1.0M");
        assert_eq!(format_size_eza(12 * 1024 * 1024, false), "  12M");
        assert_eq!(format_size_eza(1024 * 1024 * 1024, false), " 1.0G");
        assert_eq!(format_size_eza(15 * 1024 * 1024 * 1024, false), "  15G");
    }

    #[test]
    fn test_format_mtime_eza() {
        let now = std::time::SystemTime::now();
        let recent = format_mtime_eza(now);
        assert_eq!(recent.chars().count(), 12);

        let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1577836800); // 2020-01-01
        let old_str = format_mtime_eza(old);
        assert_eq!(old_str.chars().count(), 12);
        assert!(old_str.contains("2020"));
    }

    #[test]
    fn test_print_entry_with_column() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            color: ColorChoice::Never,
            ..OutputFormatter::default()
        };

        // 1. With column
        formatter
            .print_entry_with_column(
                Path::new("src/main.rs"),
                false,
                false,
                Some(" 6 Sep 23:31"),
                false,
            )
            .unwrap();
        assert_eq!(*buf.0.lock().unwrap(), b" 6 Sep 23:31  src/main.rs\n");

        // 2. Null separator ignores column
        buf.0.lock().unwrap().clear();
        formatter
            .print_entry_with_column(
                Path::new("src/main.rs"),
                false,
                false,
                Some(" 6 Sep 23:31"),
                true,
            )
            .unwrap();
        assert_eq!(*buf.0.lock().unwrap(), b"src/main.rs\0");

        // 3. Without column
        buf.0.lock().unwrap().clear();
        formatter
            .print_entry_with_column(Path::new("src/main.rs"), false, false, None, false)
            .unwrap();
        assert_eq!(*buf.0.lock().unwrap(), b"src/main.rs\n");
    }

    #[test]
    fn test_print_file_without_match_with_column() {
        let buf = SharedBuffer::default();
        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            color: ColorChoice::Never,
            ..OutputFormatter::default()
        };

        formatter
            .print_file_without_match_with_column(Path::new("test.txt"), Some(" 1.4k"), false)
            .unwrap();
        assert_eq!(*buf.0.lock().unwrap(), b" 1.4k  test.txt\n");

        buf.0.lock().unwrap().clear();
        formatter
            .print_file_without_match_with_column(Path::new("test.txt"), Some(" 1.4k"), true)
            .unwrap();
        assert_eq!(*buf.0.lock().unwrap(), b"test.txt\0");

        // 3. JSON Lines mode for files without match
        buf.0.lock().unwrap().clear();
        formatter.json_output = true;
        formatter
            .print_file_without_match_with_column(Path::new("test.txt"), Some(" 1.4k"), false)
            .unwrap();
        let json = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(json.contains("\"type\":\"file_without_match\""));
        assert!(json.contains("\"text\":\"test.txt\""));
        assert!(json.contains("\"column\":\" 1.4k\""));

        // 4. JSON Lines mode suppresses plaintext count leakage for empty matches
        buf.0.lock().unwrap().clear();
        formatter.count_only = true;
        formatter
            .print_file_matches_with_column(Path::new("test.txt"), &[], None)
            .unwrap();
        assert!(
            buf.0.lock().unwrap().is_empty(),
            "JSON mode must never leak plaintext count:0 lines"
        );
    }

    #[test]
    fn test_format_line_content_respects_raw_binary_text() {
        let mut formatter = OutputFormatter {
            allow_binary: true,
            raw_binary_text: false,
            ..OutputFormatter::default()
        };

        let line_with_null = b"hello\0world";
        let (output_hexdump, _) = formatter.format_line_content(0, line_with_null, &[(0, 5)]);
        assert!(output_hexdump.contains("00000000:"));

        // When raw_binary_text is enabled (e.g. grx -a / binary_handling = "search"), output should be formatted as text
        formatter.raw_binary_text = true;
        let (output_text, _) = formatter.format_line_content(0, line_with_null, &[(0, 5)]);
        assert!(!output_text.contains("00000000:"));
        assert!(output_text.contains("hello"));
    }

    #[test]
    fn test_should_use_color_respects_no_color_environment() {
        let _lock = crate::ops::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let formatter = OutputFormatter {
            color: ColorChoice::Auto,
            is_tty: true,
            ..OutputFormatter::default()
        };

        unsafe {
            std::env::set_var("NO_COLOR", "1");
        }
        assert!(!formatter.should_use_color());

        unsafe {
            std::env::remove_var("NO_COLOR");
        }
    }

    #[test]
    fn test_should_use_color_respects_clicolor_standards() {
        let _lock = crate::ops::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let piped_formatter = OutputFormatter {
            color: ColorChoice::Auto,
            is_tty: false,
            ..OutputFormatter::default()
        };
        let tty_formatter = OutputFormatter {
            color: ColorChoice::Auto,
            is_tty: true,
            ..OutputFormatter::default()
        };

        // CLICOLOR_FORCE=1 forces color even when piped and when NO_COLOR is present
        unsafe {
            std::env::set_var("CLICOLOR_FORCE", "1");
            std::env::set_var("NO_COLOR", "1");
        }
        assert!(piped_formatter.should_use_color());

        // CLICOLOR=0 disables color on TTY
        unsafe {
            std::env::remove_var("CLICOLOR_FORCE");
            std::env::remove_var("NO_COLOR");
            std::env::set_var("CLICOLOR", "0");
        }
        assert!(!tty_formatter.should_use_color());

        // Clean up
        unsafe {
            std::env::remove_var("CLICOLOR");
        }
    }

    #[test]
    fn test_custom_color_palette_formatting() {
        let buf = SharedBuffer::default();
        let custom_theme = crate::config::ColorTheme {
            path: "\x1b[34m".to_string(),        // blue
            line_number: "\x1b[36m".to_string(), // cyan
            column: "\x1b[33m".to_string(),
            match_highlight: "\x1b[1;32m".to_string(), // bold green
            context: "\x1b[37m".to_string(),
        };

        let mut formatter = OutputFormatter {
            writer: Box::new(buf.clone()),
            color: ColorChoice::Always,
            colors: custom_theme,
            ..OutputFormatter::default()
        };

        let matches = vec![MatchRecord::new(
            42,
            100,
            b"fn solve() -> bool {",
            vec![(3, 8)],
        )];

        formatter
            .print_file_matches(Path::new("src/solver.rs"), &matches)
            .unwrap();

        let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert_eq!(
            output,
            "\x1b[34msrc/solver.rs\x1b[0m:\x1b[36m42\x1b[0m:fn \x1b[1;32msolve\x1b[0m() -> bool {\n"
        );
    }
}
