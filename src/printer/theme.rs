use std::path::Path;

#[inline]
pub(crate) fn is_utf8_char_boundary(b: u8) -> bool {
    (b & 0xC0) != 0x80
}

#[allow(dead_code)]
pub(crate) fn base64_encode(bytes: &[u8]) -> String {
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

pub(crate) fn render_text(bytes: &[u8], terminal: bool) -> (String, Vec<usize>) {
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

pub fn byte_to_char_column(line_bytes: &[u8], byte_offset: usize) -> usize {
    let limit = byte_offset.min(line_bytes.len());
    let prefix = &line_bytes[..limit];
    match std::str::from_utf8(prefix) {
        Ok(s) => s.chars().count() + 1,
        Err(_) => String::from_utf8_lossy(prefix).chars().count() + 1,
    }
}

pub(crate) fn get_fd_file_style(path: &Path, file_name: &str) -> (&'static str, &'static str) {
    let lower_name = file_name.to_ascii_lowercase();

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
        return ("", "");
    }

    match ext {
        "zip" | "gz" | "tar" | "xz" | "bz2" | "7z" | "zst" | "rar" | "tgz" | "deb" | "rpm" => {
            ("\x1b[4;38;5;203m", "\x1b[0m")
        }

        "exe" | "bat" | "cmd" | "sh" | "bash" | "zsh" | "fish" => ("\x1b[1;38;5;203m", "\x1b[0m"),

        "rs" | "py" | "c" | "h" | "cpp" | "hpp" | "cc" | "cxx" | "js" | "ts" | "jsx" | "tsx"
        | "go" | "java" | "rb" | "php" | "cs" | "swift" | "kt" | "kts" | "lua" | "r" | "scala"
        | "elm" | "hs" | "clj" | "zig" | "nim" | "vue" | "svelte" | "sql" => {
            ("\x1b[38;5;48m", "\x1b[0m")
        }

        "json" | "toml" | "yaml" | "yml" | "xml" | "csv" | "tsv" | "ini" | "conf" | "lock"
        | "env" | "properties" => ("\x1b[38;5;149m", "\x1b[0m"),

        "md" | "txt" | "rst" | "org" | "pdf" | "doc" | "docx" | "tex" | "log" => {
            ("\x1b[38;5;185m", "\x1b[0m")
        }

        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" | "bmp" | "mp4" | "mkv" | "avi"
        | "mov" | "mp3" | "flac" | "wav" | "ogg" => ("\x1b[38;5;208m", "\x1b[0m"),
        _ => ("", ""),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    Primary,
    Near,
}

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

pub fn format_size_eza(size: u64, is_dir: bool) -> String {
    if is_dir {
        return "    -".to_string();
    }
    if size < 1024 {
        return format!("{size:>5}");
    }
    for &(factor, unit) in &[
        (1024.0, 'k'),
        (1024.0 * 1024.0, 'M'),
        (1024.0 * 1024.0 * 1024.0, 'G'),
    ] {
        if size < (factor * 1024.0) as u64 || unit == 'G' {
            let val = size as f64 / factor;
            return if val < 9.95 {
                format!("{val:>4.1}{unit}")
            } else {
                format!("{:>4}{unit}", val.round() as u64)
            };
        }
    }
    unreachable!()
}

pub(crate) fn get_hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "localhost".to_string())
}
