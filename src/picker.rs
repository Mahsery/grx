#[cfg(unix)]
use std::collections::BTreeSet;
use std::fmt::Display;
#[cfg(unix)]
use std::io::{self, IsTerminal, Read, Write};

pub fn pick_items<T: Display>(items: &[T]) -> Result<Vec<usize>, String> {
    if items.is_empty() {
        return Ok(Vec::new());
    }

    #[cfg(unix)]
    {
        pick_items_unix(items)
    }

    #[cfg(not(unix))]
    {
        pick_items_fallback(items)
    }
}

#[cfg(not(unix))]
fn pick_items_fallback<T: Display>(_items: &[T]) -> Result<Vec<usize>, String> {
    Err("--pick is currently supported on Unix terminals".into())
}

#[cfg(unix)]
struct RawModeGuard {
    fd: libc::c_int,
    original: libc::termios,
}

#[cfg(unix)]
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
        }
    }
}

#[cfg(unix)]
enum Key {
    Char(char),
    Enter,
    Tab,
    Space,
    Backspace,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Esc,
    Cancel,
    ClearFilter,
}

#[cfg(unix)]
fn read_key(fd: libc::c_int, reader: &mut impl Read) -> Option<Key> {
    let mut buf = [0u8; 1];
    if reader.read_exact(&mut buf).is_err() {
        return None;
    }
    match buf[0] {
        3 | 4 => Some(Key::Cancel),
        13 | 10 => Some(Key::Enter),
        9 => Some(Key::Tab),
        32 => Some(Key::Space),
        127 | 8 => Some(Key::Backspace),
        21 => Some(Key::ClearFilter),
        14 => Some(Key::Down),
        16 => Some(Key::Up),
        27 => {
            let mut pfd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let ret = unsafe { libc::poll(&mut pfd, 1, 25) };
            if ret > 0 && (pfd.revents & libc::POLLIN) != 0 {
                let mut seq1 = [0u8; 1];
                if reader.read_exact(&mut seq1).is_ok() && (seq1[0] == b'[' || seq1[0] == b'O') {
                    let mut seq2 = [0u8; 1];
                    if reader.read_exact(&mut seq2).is_ok() {
                        match seq2[0] {
                            b'A' => return Some(Key::Up),
                            b'B' => return Some(Key::Down),
                            b'C' => return Some(Key::Down),
                            b'D' => return Some(Key::Up),
                            b'H' => return Some(Key::Home),
                            b'F' => return Some(Key::End),
                            b'1'..=b'9' => {
                                let mut seq3 = [0u8; 1];
                                if reader.read_exact(&mut seq3).is_ok() && seq3[0] == b'~' {
                                    match seq2[0] {
                                        b'5' => return Some(Key::PageUp),
                                        b'6' => return Some(Key::PageDown),
                                        b'1' | b'7' => return Some(Key::Home),
                                        b'4' | b'8' => return Some(Key::End),
                                        b'3' => return Some(Key::Backspace),
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Some(Key::Esc)
        }
        b if (32..=126).contains(&b) => Some(Key::Char(b as char)),
        _ => None,
    }
}

#[cfg(unix)]
fn get_terminal_size(fd: libc::c_int) -> (usize, usize) {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_row > 0 && ws.ws_col > 0 {
            (ws.ws_row as usize, ws.ws_col as usize)
        } else {
            (24, 80)
        }
    }
}

#[cfg(unix)]
fn pick_items_simulated<T: Display>(items: &[T], test_keys: &str) -> Result<Vec<usize>, String> {
    let items_str: Vec<String> = items.iter().map(|it| it.to_string()).collect();
    let mut selected: BTreeSet<usize> = BTreeSet::new();
    let mut filter = String::new();
    let mut cursor: usize = 0;

    let mut chars = test_keys.chars().peekable();
    while let Some(ch) = chars.next() {
        let filter_lower = filter.to_lowercase();
        let filtered: Vec<usize> = if filter.is_empty() {
            (0..items_str.len()).collect()
        } else {
            (0..items_str.len())
                .filter(|&idx| items_str[idx].to_lowercase().contains(&filter_lower))
                .collect()
        };

        if cursor >= filtered.len() {
            cursor = filtered.len().saturating_sub(1);
        }

        match ch {
            '\n' | '\r' => {
                if selected.is_empty() {
                    if !filtered.is_empty() {
                        return Ok(vec![filtered[cursor]]);
                    } else {
                        return Ok(Vec::new());
                    }
                } else {
                    return Ok(selected.into_iter().collect());
                }
            }
            '\t' | ' ' => {
                if !filtered.is_empty() {
                    let item_idx = filtered[cursor];
                    if selected.contains(&item_idx) {
                        selected.remove(&item_idx);
                    } else {
                        selected.insert(item_idx);
                    }
                    if cursor + 1 < filtered.len() {
                        cursor += 1;
                    }
                }
            }
            '\x1b' => {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    match chars.next() {
                        Some('A') => {
                            if cursor > 0 {
                                cursor -= 1;
                            } else if !filtered.is_empty() {
                                cursor = filtered.len() - 1;
                            }
                        }
                        Some('B') if !filtered.is_empty() => {
                            if cursor + 1 < filtered.len() {
                                cursor += 1;
                            } else {
                                cursor = 0;
                            }
                        }
                        _ => {}
                    }
                } else {
                    return Err("Selection canceled".to_string());
                }
            }
            c => {
                filter.push(c);
            }
        }
    }

    if selected.is_empty() {
        let filter_lower = filter.to_lowercase();
        let filtered: Vec<usize> = if filter.is_empty() {
            (0..items_str.len()).collect()
        } else {
            (0..items_str.len())
                .filter(|&idx| items_str[idx].to_lowercase().contains(&filter_lower))
                .collect()
        };
        if !filtered.is_empty() {
            Ok(vec![filtered[cursor]])
        } else {
            Ok(Vec::new())
        }
    } else {
        Ok(selected.into_iter().collect())
    }
}

#[cfg(unix)]
fn pick_items_unix<T: Display>(items: &[T]) -> Result<Vec<usize>, String> {
    use std::fs::OpenOptions;
    use std::os::unix::io::AsRawFd;

    if let Ok(test_keys) = std::env::var("GRX_TEST_PICK_KEYS") {
        return pick_items_simulated(items, &test_keys);
    }

    if !io::stdin().is_terminal() && !io::stdout().is_terminal() && !io::stderr().is_terminal() {
        return Err("--pick requires an interactive terminal (TTY)".to_string());
    }

    let tty_in = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .or_else(|_| {
            if io::stdin().is_terminal() && io::stdout().is_terminal() {
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open("/dev/stderr")
            } else {
                Err(io::Error::other("No interactive terminal"))
            }
        })
        .map_err(|_| "--pick requires an interactive terminal (TTY)".to_string())?;

    let fd = tty_in.as_raw_fd();
    let mut tty_out = tty_in
        .try_clone()
        .map_err(|e| format!("Failed to clone TTY descriptor: {e}"))?;
    let mut tty_reader = tty_in;

    let original = unsafe {
        let mut orig = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut orig) != 0 {
            return Err("Failed to get terminal attributes".into());
        }
        orig
    };

    let mut raw = original;
    raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
    raw.c_cc[libc::VMIN] = 1;
    raw.c_cc[libc::VTIME] = 0;

    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
        return Err("Failed to set raw terminal mode".into());
    }
    let _guard = RawModeGuard { fd, original };

    let _ = tty_out.write_all(b"\x1b[?25l");
    let _ = tty_out.flush();

    let items_str: Vec<String> = items.iter().map(|it| it.to_string()).collect();
    let mut selected: BTreeSet<usize> = BTreeSet::new();
    let mut filter = String::new();
    let mut cursor: usize = 0;
    let mut scroll_offset: usize = 0;
    let mut prev_lines_rendered: usize = 0;

    let cleanup_ui = |out: &mut std::fs::File, lines: usize| {
        if lines > 0 {
            let mut buf = String::new();
            for i in 0..lines {
                buf.push_str("\r\x1b[2K");
                if i + 1 < lines {
                    buf.push_str("\x1b[1A");
                }
            }
            buf.push_str("\r\x1b[?25h");
            let _ = out.write_all(buf.as_bytes());
            let _ = out.flush();
        } else {
            let _ = out.write_all(b"\x1b[?25h");
            let _ = out.flush();
        }
    };

    let result = loop {
        let (rows, cols) = get_terminal_size(fd);
        let filter_lower = filter.to_lowercase();
        let filtered: Vec<usize> = if filter.is_empty() {
            (0..items_str.len()).collect()
        } else {
            (0..items_str.len())
                .filter(|&idx| items_str[idx].to_lowercase().contains(&filter_lower))
                .collect()
        };

        if cursor >= filtered.len() {
            cursor = filtered.len().saturating_sub(1);
        }

        let max_items = rows.saturating_sub(4).clamp(3, 15);
        let page_size = max_items.min(filtered.len());

        if cursor < scroll_offset {
            scroll_offset = cursor;
        } else if cursor >= scroll_offset + page_size {
            scroll_offset = cursor + 1 - page_size;
        }

        let mut render_buf = String::new();
        if prev_lines_rendered > 0 {
            for i in 0..prev_lines_rendered {
                render_buf.push_str("\r\x1b[2K");
                if i + 1 < prev_lines_rendered {
                    render_buf.push_str("\x1b[1A");
                }
            }
        }

        let mut lines_count = 0;

        render_buf.push_str(&format!(
            "\r\x1b[1;36mgrx pick:\x1b[0m [filter: \"\x1b[1;33m{}\x1b[0m\"] ({}/{} matches) \x1b[2m(Enter: pick, Space: mark, Esc: cancel)\x1b[0m\r\n",
            filter,
            filtered.len(),
            items_str.len()
        ));
        lines_count += 1;

        if filtered.is_empty() {
            render_buf.push_str("\r\x1b[2m  (no matches for filter)\x1b[0m\r\n");
            lines_count += 1;
        } else {
            let end_idx = (scroll_offset + page_size).min(filtered.len());
            for (offset_idx, &item_idx) in filtered[scroll_offset..end_idx].iter().enumerate() {
                let idx = scroll_offset + offset_idx;
                let is_marked = selected.contains(&item_idx);
                let is_cursor = idx == cursor;

                let mark_str = if is_marked {
                    "\x1b[1;32m[*]\x1b[0m "
                } else {
                    "[ ] "
                };
                let pointer_str = if is_cursor {
                    "\x1b[1;36m>\x1b[0m "
                } else {
                    "  "
                };

                let mut text = items_str[item_idx].clone();
                let available_width = cols.saturating_sub(10).max(10);
                if text.len() > available_width {
                    text.truncate(available_width.saturating_sub(3));
                    text.push_str("...");
                }

                if is_cursor {
                    render_buf.push_str(&format!(
                        "\r{}{}\x1b[1;37m{}\x1b[0m\x1b[K\r\n",
                        pointer_str, mark_str, text
                    ));
                } else {
                    render_buf
                        .push_str(&format!("\r{}{}{}\x1b[K\r\n", pointer_str, mark_str, text));
                }
                lines_count += 1;
            }

            if end_idx < filtered.len() {
                render_buf.push_str(&format!(
                    "\r\x1b[2m  ... ({} more) ...\x1b[0m\x1b[K\r\n",
                    filtered.len() - end_idx
                ));
                lines_count += 1;
            }
        }

        let _ = tty_out.write_all(render_buf.as_bytes());
        let _ = tty_out.flush();
        prev_lines_rendered = lines_count;

        match read_key(fd, &mut tty_reader) {
            Some(Key::Enter) => {
                if selected.is_empty() {
                    if !filtered.is_empty() {
                        break Ok(vec![filtered[cursor]]);
                    } else {
                        break Ok(Vec::new());
                    }
                } else {
                    break Ok(selected.into_iter().collect());
                }
            }
            Some(Key::Esc) | Some(Key::Cancel) => {
                break Err("Selection canceled".to_string());
            }
            Some(Key::Up) => {
                if cursor > 0 {
                    cursor -= 1;
                } else if !filtered.is_empty() {
                    cursor = filtered.len() - 1;
                }
            }
            Some(Key::Down) => {
                if !filtered.is_empty() {
                    if cursor + 1 < filtered.len() {
                        cursor += 1;
                    } else {
                        cursor = 0;
                    }
                }
            }
            Some(Key::PageUp) => {
                cursor = cursor.saturating_sub(max_items);
            }
            Some(Key::PageDown) => {
                if !filtered.is_empty() {
                    cursor = (cursor + max_items).min(filtered.len() - 1);
                }
            }
            Some(Key::Home) => {
                cursor = 0;
            }
            Some(Key::End) => {
                if !filtered.is_empty() {
                    cursor = filtered.len() - 1;
                }
            }
            Some(Key::Space) | Some(Key::Tab) => {
                if !filtered.is_empty() {
                    let item_idx = filtered[cursor];
                    if selected.contains(&item_idx) {
                        selected.remove(&item_idx);
                    } else {
                        selected.insert(item_idx);
                    }
                    if cursor + 1 < filtered.len() {
                        cursor += 1;
                    }
                }
            }
            Some(Key::Backspace) => {
                filter.pop();
            }
            Some(Key::ClearFilter) => {
                filter.clear();
            }
            Some(Key::Char(c)) => {
                filter.push(c);
            }
            None => {}
        }
    };

    cleanup_ui(&mut tty_out, prev_lines_rendered);
    result
}
