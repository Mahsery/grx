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
fn json_summary_counts_emitted_payload_and_excludes_end_records() {
    let buf = SharedBuffer::default();
    let mut formatter = OutputFormatter {
        writer: Box::new(buf.clone()),
        json_output: true,
        ..OutputFormatter::default()
    };
    formatter
        .print_file_matches(
            Path::new("input.txt"),
            &[MatchRecord::new(1, 0, b"needle", vec![(0, 6)])],
        )
        .unwrap();
    let payload = buf.0.lock().unwrap().clone();
    let mut records = Vec::new();
    let mut expected_bytes = 0u64;
    for line in payload.split_inclusive(|byte| *byte == b'\n') {
        let record: serde_json::Value = serde_json::from_slice(line).unwrap();
        let kind = record["type"].as_str().unwrap();
        records.push(kind.to_owned());
        if kind == "begin" || kind == "match" {
            expected_bytes += line.len() as u64;
        }
    }
    assert_eq!(records, ["begin", "match", "end"]);
    formatter
        .print_json_summary_with_skipped(1, 1, 1, 1, 6, 5, std::time::Duration::from_millis(1))
        .unwrap();
    let bytes = buf.0.lock().unwrap().clone();
    let summary: serde_json::Value = serde_json::from_slice(
        bytes
            .split(|byte| *byte == b'\n')
            .rfind(|line| !line.is_empty())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(summary["type"], "summary");
    assert_eq!(summary["data"]["stats"]["bytes_printed"], expected_bytes);
    assert_eq!(summary["data"]["stats"]["binary_files_skipped"], 5);
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

    let matches = vec![MatchRecord::new(1, 0, "🦀 match".as_bytes(), vec![(5, 10)])];

    formatter
        .print_file_matches(Path::new("unicode.txt"), &matches)
        .unwrap();

    let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();

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

    let matches = vec![MatchRecord::new(10, 50, b"hello world here", vec![(6, 11)])];

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

    formatter
        .print_entry_path_styled(Path::new("src/main.rs"), false, false, false)
        .unwrap();
    assert_eq!(
        *buf.0.lock().unwrap(),
        format!("\x1b[38;5;81msrc{sep}\x1b[0m\x1b[38;5;48mmain.rs\x1b[0m\n").into_bytes()
    );

    buf.0.lock().unwrap().clear();
    formatter
        .print_entry_path_styled(Path::new("src/nested"), true, false, false)
        .unwrap();
    assert_eq!(
        *buf.0.lock().unwrap(),
        format!("\x1b[38;5;81msrc{sep}\x1b[0m\x1b[38;5;81mnested/\x1b[0m\n").into_bytes()
    );

    buf.0.lock().unwrap().clear();
    formatter
        .print_entry_path_styled(Path::new("src/link.rs"), false, true, false)
        .unwrap();
    assert_eq!(
        *buf.0.lock().unwrap(),
        format!("\x1b[38;5;81msrc{sep}\x1b[0m\x1b[38;5;203mlink.rs\x1b[0m\n").into_bytes()
    );

    buf.0.lock().unwrap().clear();
    formatter
        .print_entry_path_styled(Path::new("src"), true, false, false)
        .unwrap();
    assert_eq!(*buf.0.lock().unwrap(), b"\x1b[38;5;81msrc/\x1b[0m\n");

    buf.0.lock().unwrap().clear();
    formatter
        .print_entry_path_styled(Path::new("Cargo.toml"), false, false, false)
        .unwrap();
    assert_eq!(*buf.0.lock().unwrap(), b"\x1b[38;5;149mCargo.toml\x1b[0m\n");

    buf.0.lock().unwrap().clear();
    formatter
        .print_entry_path_styled(Path::new("README.md"), false, false, false)
        .unwrap();
    assert_eq!(*buf.0.lock().unwrap(), b"\x1b[38;5;185mREADME.md\x1b[0m\n");

    buf.0.lock().unwrap().clear();
    formatter
        .print_entry_path_styled(Path::new("archive.tar.gz"), false, false, false)
        .unwrap();
    assert_eq!(
        *buf.0.lock().unwrap(),
        b"\x1b[4;38;5;203marchive.tar.gz\x1b[0m\n"
    );

    buf.0.lock().unwrap().clear();
    formatter.color = ColorChoice::Never;
    formatter
        .print_entry_path_styled(Path::new("src"), true, false, false)
        .unwrap();
    assert_eq!(*buf.0.lock().unwrap(), b"src/\n");

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

        formatter
            .print_entry_path_styled(p, false, false, false)
            .unwrap();
        assert_eq!(*buf.0.lock().unwrap(), b"bad-\xff.txt\n");

        buf.0.lock().unwrap().clear();
        formatter.print_file_without_match(p, false).unwrap();
        assert_eq!(*buf.0.lock().unwrap(), b"bad-\xff.txt\n");

        buf.0.lock().unwrap().clear();
        formatter.json_output = true;
        formatter
            .print_entry_path_styled(p, false, false, false)
            .unwrap();
        let json_str = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(json_str.contains("\"bytes\":\"YmFkLf8udHh0\""));
        assert!(json_str.contains("\"type\":\"entry\""));

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

    let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1577836800);
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

    formatter
        .print_entry_with_column(
            Path::new("src/main.rs"),
            false,
            false,
            Some(" 6 Sep 23:31"),
            None,
            false,
        )
        .unwrap();
    assert_eq!(*buf.0.lock().unwrap(), b" 6 Sep 23:31  src/main.rs\n");

    buf.0.lock().unwrap().clear();
    formatter
        .print_entry_with_column(
            Path::new("src/main.rs"),
            false,
            false,
            Some(" 6 Sep 23:31"),
            None,
            true,
        )
        .unwrap();
    assert_eq!(*buf.0.lock().unwrap(), b"src/main.rs\0");

    buf.0.lock().unwrap().clear();
    formatter
        .print_entry_with_column(Path::new("src/main.rs"), false, false, None, None, false)
        .unwrap();
    assert_eq!(*buf.0.lock().unwrap(), b"src/main.rs\n");

    buf.0.lock().unwrap().clear();
    formatter
        .print_entry_with_column(Path::new("src/main.rs"), false, false, None, Some(3), false)
        .unwrap();
    assert_eq!(*buf.0.lock().unwrap(), b"3:src/main.rs\n");

    buf.0.lock().unwrap().clear();
    formatter
        .print_entry_with_column(
            Path::new("src/main.rs"),
            false,
            false,
            Some(" 6 Sep 23:31"),
            Some(42),
            false,
        )
        .unwrap();
    assert_eq!(*buf.0.lock().unwrap(), b"42:  6 Sep 23:31  src/main.rs\n");
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

    buf.0.lock().unwrap().clear();
    formatter.json_output = true;
    formatter
        .print_file_without_match_with_column(Path::new("test.txt"), Some(" 1.4k"), false)
        .unwrap();
    let json = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    assert!(json.contains("\"type\":\"file_without_match\""));
    assert!(json.contains("\"text\":\"test.txt\""));
    assert!(json.contains("\"column\":\" 1.4k\""));

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

    unsafe {
        std::env::set_var("CLICOLOR_FORCE", "1");
        std::env::set_var("NO_COLOR", "1");
    }
    assert!(piped_formatter.should_use_color());

    unsafe {
        std::env::remove_var("CLICOLOR_FORCE");
        std::env::remove_var("NO_COLOR");
        std::env::set_var("CLICOLOR", "0");
    }
    assert!(!tty_formatter.should_use_color());

    unsafe {
        std::env::remove_var("CLICOLOR");
    }
}

#[test]
fn test_custom_color_palette_formatting() {
    let buf = SharedBuffer::default();
    let custom_theme = crate::config::ColorTheme {
        path: "\x1b[34m".to_string(),
        line_number: "\x1b[36m".to_string(),
        column: "\x1b[33m".to_string(),
        match_highlight: "\x1b[1;32m".to_string(),
        near_match_highlight: "\x1b[1;35m".to_string(),
        name_match_highlight: "\x1b[30;48;5;220m".to_string(),
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

#[test]
fn test_near_match_color_formatting_and_bridge() {
    let buf = SharedBuffer::default();
    let mut formatter = OutputFormatter {
        writer: Box::new(buf.clone()),
        color: ColorChoice::Always,
        is_tty: false,
        ..OutputFormatter::default()
    };

    let matches = vec![
        MatchRecord::new(10, 100, b"let ptr = unsafe {", vec![(10, 16)]),
        MatchRecord::context(11, 120, b"    let val = *ptr;"),
        MatchRecord::context(12, 140, b"// SAFETY: pointer is valid").with_near_spans(vec![(3, 9)]),
    ];

    formatter
        .print_file_matches(Path::new("src/lib.rs"), &matches)
        .unwrap();

    let output = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();

    assert!(output.contains("\x1b[32m10\x1b[0m:let ptr = \x1b[1;31munsafe\x1b[0m {"));

    assert!(output.contains("\x1b[38;5;242m11\x1b[0m-\x1b[38;5;250m    let val = *ptr;\x1b[0m"));

    assert!(output.contains("\x1b[1;36m12\x1b[0m:// \x1b[1;36mSAFETY\x1b[0m: pointer is valid"));
}
