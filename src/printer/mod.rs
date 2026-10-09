pub mod content;
pub mod discovery;
pub mod json;
#[cfg(test)]
mod tests;
pub mod theme;

pub use theme::{SpanKind, byte_to_char_column, format_mtime_eza, format_size_eza};

use crate::config::{ColorChoice, HyperlinkChoice};
use crate::core::{DirEntry, EntrySink, MatchRecord, Printer};
use json::json_payload_bytes;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use theme::get_hostname;

pub(crate) fn display_path(path: &Path) -> &Path {
    path.strip_prefix(".")
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .unwrap_or(path)
}

pub struct OutputFormatter {
    pub(crate) writer: Box<dyn Write + Send + Sync>,
    pub(crate) is_tty: bool,
    pub(crate) color: ColorChoice,
    pub(crate) hyperlinks: HyperlinkChoice,
    pub(crate) hyperlink_format: String,
    pub(crate) show_line_numbers: bool,
    pub(crate) show_column: bool,
    pub(crate) show_line_len: bool,
    pub(crate) show_byte_offset: bool,
    pub(crate) show_heading: bool,
    pub(crate) null_separator: bool,
    pub(crate) only_matching: bool,
    pub(crate) quiet: bool,
    pub(crate) files_with_matches: bool,
    pub(crate) count_only: bool,
    pub(crate) count_matches: bool,
    pub(crate) json_output: bool,
    pub(crate) no_filename: bool,
    pub(crate) hostname: String,
    pub(crate) max_columns: Option<usize>,
    pub(crate) allow_binary: bool,
    pub(crate) raw_binary_text: bool,
    pub(crate) json_bytes_printed: u64,
    pub colors: crate::config::ColorTheme,
    pub(crate) name_matcher: Option<std::sync::Arc<crate::search::CompiledExpr>>,
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
            name_matcher: None,
        }
    }

    pub(crate) fn account_json_payload(&mut self, bytes: &[u8]) {
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

    pub fn with_name_matcher(
        mut self,
        name_matcher: Option<std::sync::Arc<crate::search::CompiledExpr>>,
    ) -> Self {
        self.name_matcher = name_matcher;
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
    pub(crate) fn should_use_hyperlinks(&self) -> bool {
        match self.hyperlinks {
            HyperlinkChoice::Always => true,
            HyperlinkChoice::Never => false,
            HyperlinkChoice::Auto => self.is_tty,
        }
    }

    pub(crate) fn build_hyperlink_url(&self, path: &Path, line: usize, col: usize) -> String {
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

    pub(crate) fn build_directory_hyperlink_url(&self, path: &Path) -> String {
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

    pub(crate) fn format_hyperlink(&self, url: &str, text: &str) -> String {
        if self.should_use_hyperlinks() {
            format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
        } else {
            text.to_string()
        }
    }

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

    pub fn write_raw_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        if !bytes.is_empty() {
            self.account_json_payload(bytes);
            self.writer.write_all(bytes)?;
        }
        Ok(())
    }

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
            name_matcher: self.name_matcher.clone(),
        }
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
            name_matcher: None,
        }
    }
}
