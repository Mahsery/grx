use std::fs;
use tempfile::tempdir;

fn grx_bin() -> &'static str {
    env!("CARGO_BIN_EXE_grx")
}

fn has_tool(bin: &str) -> bool {
    #[cfg(windows)]
    let cmd = "where";
    #[cfg(not(windows))]
    let cmd = "which";

    std::process::Command::new(cmd)
        .arg(bin)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn create_sample_corpus() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempdir().unwrap();
    let file = tmp.path().join("corpus.txt");
    fs::write(
        &file,
        b"apple pie\nBanana smoothie\ncherry tart\nAPPLE crumble\nbanana split\ndate prune\nfoo.bar baz\n",
    )
    .unwrap();
    (tmp, file)
}

#[test]
fn test_golden_literal_match_grep_and_rg() {
    let (_tmp, path) = create_sample_corpus();
    let path_str = path.to_str().unwrap();

    let grx_out = std::process::Command::new(grx_bin())
        .args([
            "--color=never",
            "--no-heading",
            "-N",
            "-s",
            "apple",
            path_str,
        ])
        .output()
        .unwrap();
    assert_eq!(grx_out.status.code(), Some(0));
    let expected = format!("{path_str}:apple pie\n");
    assert_eq!(
        String::from_utf8_lossy(&grx_out.stdout),
        expected,
        "grx stdout matches expected output"
    );

    if has_tool("grep") {
        let grep_out = std::process::Command::new("grep")
            .args(["-H", "apple", path_str])
            .output()
            .unwrap();
        assert_eq!(grep_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, grep_out.stdout);
    }

    if has_tool("rg") {
        let rg_out = std::process::Command::new("rg")
            .args([
                "-H",
                "--color=never",
                "--no-heading",
                "-N",
                "-s",
                "apple",
                path_str,
            ])
            .output()
            .unwrap();
        assert_eq!(rg_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, rg_out.stdout);
    }
}

#[test]
fn test_golden_case_insensitive_grep_and_rg() {
    let (_tmp, path) = create_sample_corpus();
    let path_str = path.to_str().unwrap();

    let grx_out = std::process::Command::new(grx_bin())
        .args([
            "--color=never",
            "--no-heading",
            "-N",
            "-i",
            "apple",
            path_str,
        ])
        .output()
        .unwrap();
    assert_eq!(grx_out.status.code(), Some(0));
    let expected = format!("{path_str}:apple pie\n{path_str}:APPLE crumble\n");
    assert_eq!(
        String::from_utf8_lossy(&grx_out.stdout),
        expected,
        "grx stdout matches expected output"
    );

    if has_tool("grep") {
        let grep_out = std::process::Command::new("grep")
            .args(["-H", "-i", "apple", path_str])
            .output()
            .unwrap();
        assert_eq!(grep_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, grep_out.stdout);
    }

    if has_tool("rg") {
        let rg_out = std::process::Command::new("rg")
            .args([
                "-H",
                "--color=never",
                "--no-heading",
                "-N",
                "-i",
                "apple",
                path_str,
            ])
            .output()
            .unwrap();
        assert_eq!(rg_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, rg_out.stdout);
    }
}

#[test]
fn test_golden_line_numbers_grep_and_rg() {
    let (_tmp, path) = create_sample_corpus();
    let path_str = path.to_str().unwrap();

    let grx_out = std::process::Command::new(grx_bin())
        .args([
            "--color=never",
            "--no-heading",
            "-n",
            "-i",
            "banana",
            path_str,
        ])
        .output()
        .unwrap();
    assert_eq!(grx_out.status.code(), Some(0));
    let expected = format!("{path_str}:2:Banana smoothie\n{path_str}:5:banana split\n");
    assert_eq!(
        String::from_utf8_lossy(&grx_out.stdout),
        expected,
        "grx stdout matches expected output"
    );

    if has_tool("grep") {
        let grep_out = std::process::Command::new("grep")
            .args(["-H", "-n", "-i", "banana", path_str])
            .output()
            .unwrap();
        assert_eq!(grep_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, grep_out.stdout);
    }

    if has_tool("rg") {
        let rg_out = std::process::Command::new("rg")
            .args([
                "-H",
                "--color=never",
                "--no-heading",
                "-n",
                "-i",
                "banana",
                path_str,
            ])
            .output()
            .unwrap();
        assert_eq!(rg_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, rg_out.stdout);
    }
}

#[test]
fn test_golden_count_matches_grep_and_rg() {
    let (_tmp, path) = create_sample_corpus();
    let path_str = path.to_str().unwrap();

    let grx_out = std::process::Command::new(grx_bin())
        .args(["-c", "-i", "banana", path_str])
        .output()
        .unwrap();
    assert_eq!(grx_out.status.code(), Some(0));
    let expected = format!("{path_str}:2\n");
    assert_eq!(
        String::from_utf8_lossy(&grx_out.stdout),
        expected,
        "grx stdout matches expected output"
    );

    if has_tool("grep") {
        let grep_out = std::process::Command::new("grep")
            .args(["-H", "-c", "-i", "banana", path_str])
            .output()
            .unwrap();
        assert_eq!(grep_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, grep_out.stdout);
    }

    if has_tool("rg") {
        let rg_out = std::process::Command::new("rg")
            .args(["-H", "--color=never", "-c", "-i", "banana", path_str])
            .output()
            .unwrap();
        assert_eq!(rg_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, rg_out.stdout);
    }
}

#[test]
fn test_golden_invert_match_grep_and_rg() {
    let (_tmp, path) = create_sample_corpus();
    let path_str = path.to_str().unwrap();

    let grx_out = std::process::Command::new(grx_bin())
        .args([
            "--color=never",
            "--no-heading",
            "-N",
            "-v",
            "-i",
            "apple",
            path_str,
        ])
        .output()
        .unwrap();
    assert_eq!(grx_out.status.code(), Some(0));
    let expected = format!(
        "{path_str}:Banana smoothie\n{path_str}:cherry tart\n{path_str}:banana split\n{path_str}:date prune\n{path_str}:foo.bar baz\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&grx_out.stdout),
        expected,
        "grx stdout matches expected output"
    );

    if has_tool("grep") {
        let grep_out = std::process::Command::new("grep")
            .args(["-H", "-v", "-i", "apple", path_str])
            .output()
            .unwrap();
        assert_eq!(grep_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, grep_out.stdout);
    }

    if has_tool("rg") {
        let rg_out = std::process::Command::new("rg")
            .args([
                "-H",
                "--color=never",
                "--no-heading",
                "-N",
                "-v",
                "-i",
                "apple",
                path_str,
            ])
            .output()
            .unwrap();
        assert_eq!(rg_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, rg_out.stdout);
    }
}

#[test]
fn test_golden_only_matching_grep_and_rg() {
    let (_tmp, path) = create_sample_corpus();
    let path_str = path.to_str().unwrap();

    let grx_out = std::process::Command::new(grx_bin())
        .args([
            "--color=never",
            "--no-heading",
            "-N",
            "-o",
            "-i",
            "apple",
            path_str,
        ])
        .output()
        .unwrap();
    assert_eq!(grx_out.status.code(), Some(0));
    let expected = format!("{path_str}:apple\n{path_str}:APPLE\n");
    assert_eq!(
        String::from_utf8_lossy(&grx_out.stdout),
        expected,
        "grx stdout matches expected output"
    );

    if has_tool("grep") {
        let grep_out = std::process::Command::new("grep")
            .args(["-H", "-o", "-i", "apple", path_str])
            .output()
            .unwrap();
        assert_eq!(grep_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, grep_out.stdout);
    }

    if has_tool("rg") {
        let rg_out = std::process::Command::new("rg")
            .args([
                "-H",
                "--color=never",
                "--no-heading",
                "-N",
                "-o",
                "-i",
                "apple",
                path_str,
            ])
            .output()
            .unwrap();
        assert_eq!(rg_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, rg_out.stdout);
    }
}

#[test]
fn test_golden_files_with_matches_grep_and_rg() {
    let (_tmp, path) = create_sample_corpus();
    let path_str = path.to_str().unwrap();

    let grx_out = std::process::Command::new(grx_bin())
        .args(["-l", "-i", "apple", path_str])
        .output()
        .unwrap();
    assert_eq!(grx_out.status.code(), Some(0));
    let expected = format!("{path_str}\n");
    assert_eq!(
        String::from_utf8_lossy(&grx_out.stdout),
        expected,
        "grx stdout matches expected output"
    );

    if has_tool("grep") {
        let grep_out = std::process::Command::new("grep")
            .args(["-l", "-i", "apple", path_str])
            .output()
            .unwrap();
        assert_eq!(grep_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, grep_out.stdout);
    }

    if has_tool("rg") {
        let rg_out = std::process::Command::new("rg")
            .args(["-l", "-i", "apple", path_str])
            .output()
            .unwrap();
        assert_eq!(rg_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, rg_out.stdout);
    }
}

#[test]
fn test_golden_fixed_strings_grep_and_rg() {
    let (_tmp, path) = create_sample_corpus();
    let path_str = path.to_str().unwrap();

    let grx_out = std::process::Command::new(grx_bin())
        .args([
            "--color=never",
            "--no-heading",
            "-N",
            "-F",
            "foo.bar",
            path_str,
        ])
        .output()
        .unwrap();
    assert_eq!(grx_out.status.code(), Some(0));
    let expected = format!("{path_str}:foo.bar baz\n");
    assert_eq!(
        String::from_utf8_lossy(&grx_out.stdout),
        expected,
        "grx stdout matches expected output"
    );

    if has_tool("grep") {
        let grep_out = std::process::Command::new("grep")
            .args(["-H", "-F", "foo.bar", path_str])
            .output()
            .unwrap();
        assert_eq!(grep_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, grep_out.stdout);
    }

    if has_tool("rg") {
        let rg_out = std::process::Command::new("rg")
            .args([
                "-H",
                "--color=never",
                "--no-heading",
                "-N",
                "-F",
                "foo.bar",
                path_str,
            ])
            .output()
            .unwrap();
        assert_eq!(rg_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, rg_out.stdout);
    }
}

#[test]
fn test_golden_word_regexp_grep_and_rg() {
    let (_tmp, path) = create_sample_corpus();
    let path_str = path.to_str().unwrap();

    let grx_out = std::process::Command::new(grx_bin())
        .args(["--color=never", "--no-heading", "-N", "-w", "pie", path_str])
        .output()
        .unwrap();
    assert_eq!(grx_out.status.code(), Some(0));
    let expected = format!("{path_str}:apple pie\n");
    assert_eq!(
        String::from_utf8_lossy(&grx_out.stdout),
        expected,
        "grx stdout matches expected output"
    );

    if has_tool("grep") {
        let grep_out = std::process::Command::new("grep")
            .args(["-H", "-w", "pie", path_str])
            .output()
            .unwrap();
        assert_eq!(grep_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, grep_out.stdout);
    }

    if has_tool("rg") {
        let rg_out = std::process::Command::new("rg")
            .args([
                "-H",
                "--color=never",
                "--no-heading",
                "-N",
                "-w",
                "pie",
                path_str,
            ])
            .output()
            .unwrap();
        assert_eq!(rg_out.status.code(), Some(0));
        assert_eq!(grx_out.stdout, rg_out.stdout);
    }
}

#[test]
fn test_golden_exit_codes_parity() {
    let (_tmp, path) = create_sample_corpus();
    let path_str = path.to_str().unwrap();

    let grx_match = std::process::Command::new(grx_bin())
        .args(["apple", path_str])
        .output()
        .unwrap();
    assert_eq!(grx_match.status.code(), Some(0));

    let grx_nomatch = std::process::Command::new(grx_bin())
        .args(["nonexistent_symbol_xyz", path_str])
        .output()
        .unwrap();
    assert_eq!(grx_nomatch.status.code(), Some(1));

    let grx_err = std::process::Command::new(grx_bin())
        .args(["apple", "/nonexistent/path/for/grx/test"])
        .output()
        .unwrap();
    assert_eq!(grx_err.status.code(), Some(2));

    if has_tool("grep") {
        let grep_match = std::process::Command::new("grep")
            .args(["apple", path_str])
            .output()
            .unwrap();
        let grep_nomatch = std::process::Command::new("grep")
            .args(["nonexistent_symbol_xyz", path_str])
            .output()
            .unwrap();
        let grep_err = std::process::Command::new("grep")
            .args(["apple", "/nonexistent/path/for/grx/test"])
            .output()
            .unwrap();
        assert_eq!(grx_match.status.code(), grep_match.status.code());
        assert_eq!(grx_nomatch.status.code(), grep_nomatch.status.code());
        assert_eq!(grx_err.status.code(), grep_err.status.code());
    }

    if has_tool("rg") {
        let rg_match = std::process::Command::new("rg")
            .args(["apple", path_str])
            .output()
            .unwrap();
        let rg_nomatch = std::process::Command::new("rg")
            .args(["nonexistent_symbol_xyz", path_str])
            .output()
            .unwrap();
        let rg_err = std::process::Command::new("rg")
            .args(["apple", "/nonexistent/path/for/grx/test"])
            .output()
            .unwrap();
        assert_eq!(grx_match.status.code(), rg_match.status.code());
        assert_eq!(grx_nomatch.status.code(), rg_nomatch.status.code());
        assert_eq!(grx_err.status.code(), rg_err.status.code());
    }
}
