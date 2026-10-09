use super::OutputFormatter;
use super::json::escape_json;
use super::theme::{base64_encode, get_fd_file_style};
use std::io::{self, Write};
use std::path::Path;

impl OutputFormatter {
    pub(crate) fn styled_basename(
        &self,
        name: &str,
        trailing: &str,
        prefix: &str,
        suffix: &str,
        can_highlight: bool,
    ) -> String {
        let mut spans = Vec::new();
        if can_highlight && let Some(ref matcher) = self.name_matcher {
            let mut matcher_spans = Vec::new();
            matcher.collect_spans(name.as_bytes(), &mut matcher_spans);
            spans.extend(matcher_spans.into_iter().filter(|&(start, end)| {
                name.is_char_boundary(start) && name.is_char_boundary(end)
            }));
        }
        if spans.is_empty() {
            return format!("{prefix}{name}{trailing}{suffix}");
        }
        spans.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
        for (start, end) in spans {
            if let Some(last) = merged.last_mut()
                && start <= last.1
            {
                last.1 = last.1.max(end);
                continue;
            }
            merged.push((start, end));
        }

        let mut styled = String::new();
        let mut offset = 0;
        for (start, end) in merged {
            if start > offset {
                styled.push_str(prefix);
                styled.push_str(&name[offset..start]);
                styled.push_str(suffix);
            }
            styled.push_str(&self.colors.name_match_highlight);
            styled.push_str(&name[start..end]);
            styled.push_str("\x1b[0m");
            offset = end;
        }
        if offset < name.len() || !trailing.is_empty() {
            styled.push_str(prefix);
            styled.push_str(&name[offset..]);
            styled.push_str(trailing);
            styled.push_str(suffix);
        }
        styled
    }

    pub fn format_file_without_match_with_column(
        &self,
        path: &Path,
        column: Option<&str>,
        null_separator: bool,
        buffer: &mut Vec<u8>,
    ) -> io::Result<()> {
        let path = super::display_path(path);
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

    pub fn print_file_without_match(
        &mut self,
        path: &Path,
        null_separator: bool,
    ) -> io::Result<()> {
        self.print_file_without_match_with_column(path, None, null_separator)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn format_entry_with_column(
        &self,
        path: &Path,
        is_dir: bool,
        is_symlink: bool,
        column: Option<&str>,
        line_number: Option<usize>,
        null_separator: bool,
        buffer: &mut Vec<u8>,
    ) -> io::Result<()> {
        let path = super::display_path(path);
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
            let ln_json = if let Some(ln) = line_number {
                format!(",\"line_number\":{ln}")
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
                        "{{\"data\":{{\"path\":{{\"bytes\":\"{b64}\",\"text\":\"{esc_path}\"}}{col_json}{ln_json}}},\"type\":\"entry\"}}"
                    )?;
                    return Ok(());
                }
            }
            writeln!(
                buffer,
                "{{\"data\":{{\"path\":{{\"text\":\"{esc_path}\"}}{col_json}{ln_json}}},\"type\":\"entry\"}}"
            )?;
            return Ok(());
        }
        if null_separator || self.null_separator {
            if let Some(ln) = line_number {
                write!(buffer, "{ln}:")?;
            }
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

        let line_prefix = match line_number {
            Some(ln) if use_color => format!("{}{ln}\x1b[0m:", self.colors.line_number),
            Some(ln) => format!("{ln}:"),
            None => String::new(),
        };

        let col_prefix = match column {
            Some(col) if use_color => format!("\x1b[38;5;108m{col}\x1b[0m  "),
            Some(col) => format!("{col}  "),
            None => String::new(),
        };

        let prefix_sep = if !line_prefix.is_empty() && !col_prefix.is_empty() {
            " "
        } else {
            ""
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
                if !line_prefix.is_empty() {
                    buffer.write_all(line_prefix.as_bytes())?;
                    if !col_prefix.is_empty() {
                        buffer.write_all(b" ")?;
                    }
                }
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

            let can_highlight = path.file_name().and_then(|name| name.to_str()).is_some();
            if is_dir {
                let name = self.styled_basename(
                    &file_name,
                    "/",
                    "\x1b[38;5;81m",
                    "\x1b[0m",
                    can_highlight,
                );
                format!("{dim_parent}{name}")
            } else if is_symlink {
                let name = self.styled_basename(
                    &file_name,
                    "",
                    "\x1b[38;5;203m",
                    "\x1b[0m",
                    can_highlight,
                );
                format!("{dim_parent}{name}")
            } else {
                let (prefix, suffix) = get_fd_file_style(path, &file_name);
                let name = self.styled_basename(&file_name, "", prefix, suffix, can_highlight);
                format!("{dim_parent}{name}")
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

        writeln!(buffer, "{line_prefix}{prefix_sep}{col_prefix}{display_str}")?;
        Ok(())
    }

    pub fn format_entry_path_styled(
        &self,
        path: &Path,
        is_dir: bool,
        is_symlink: bool,
        line_number: Option<usize>,
        null_separator: bool,
        buffer: &mut Vec<u8>,
    ) -> io::Result<()> {
        self.format_entry_with_column(
            path,
            is_dir,
            is_symlink,
            None,
            line_number,
            null_separator,
            buffer,
        )
    }

    pub fn print_entry_with_column(
        &mut self,
        path: &Path,
        is_dir: bool,
        is_symlink: bool,
        column: Option<&str>,
        line_number: Option<usize>,
        null_separator: bool,
    ) -> io::Result<()> {
        let mut buffer = Vec::new();
        self.format_entry_with_column(
            path,
            is_dir,
            is_symlink,
            column,
            line_number,
            null_separator,
            &mut buffer,
        )?;
        if !buffer.is_empty() {
            self.account_json_payload(&buffer);
            self.writer.write_all(&buffer)?;
        }
        Ok(())
    }

    pub fn print_entry_path_styled(
        &mut self,
        path: &Path,
        is_dir: bool,
        is_symlink: bool,
        null_separator: bool,
    ) -> io::Result<()> {
        self.print_entry_with_column(path, is_dir, is_symlink, None, None, null_separator)
    }

    pub fn print_entry_path(&mut self, path: &Path, null_separator: bool) -> io::Result<()> {
        self.print_entry_path_styled(path, false, false, null_separator)
    }
}
