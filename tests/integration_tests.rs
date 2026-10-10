use clap::Parser;
use grx::cli::Cli;
use grx::config::Config;
use grx::engine::Engine;
use std::fs;
use tempfile::tempdir;
#[test]
fn test_engine_exit_code_0_match_found() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("match.txt");
    fs::write(&file_path, b"hello world\nanother line\n").unwrap();

    let cli = Cli::parse_from([
        "grx",
        "--hidden",
        "-q",
        "world",
        &format!("root:{}", tmp.path().to_str().unwrap()),
    ]);
    let config = Config::new_with_defaults();
    let mut engine = Engine::new(config, cli);

    let exit_code = engine.run().unwrap();
    assert_eq!(exit_code, 0, "Should return 0 when matches are found");
}

#[test]
fn test_engine_exit_code_1_no_match() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("nomatch.txt");
    fs::write(&file_path, b"hello world\nanother line\n").unwrap();

    let cli = Cli::parse_from([
        "grx",
        "--hidden",
        "-q",
        "foobar_absent_term",
        &format!("root:{}", tmp.path().to_str().unwrap()),
    ]);
    let config = Config::new_with_defaults();
    let mut engine = Engine::new(config, cli);

    let exit_code = engine.run().unwrap();
    assert_eq!(exit_code, 1, "Should return 1 when no matches are found");
}

#[test]
fn test_engine_exit_code_2_nonexistent_file() {
    let tmp = tempdir().unwrap();
    let nonexistent = tmp.path().join("nonexistent_dir").join("ghost.txt");

    let cli = Cli::parse_from([
        "grx",
        "--hidden",
        "pattern",
        &format!("root:{}", nonexistent.to_str().unwrap()),
    ]);
    let config = Config::new_with_defaults();
    let mut engine = Engine::new(config, cli);

    let exit_code = engine.run().unwrap();
    assert_eq!(
        exit_code, 2,
        "Should return 2 when target path does not exist"
    );
}

#[test]
fn test_engine_crlf_and_no_newline_eof() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("crlf.txt");

    fs::write(
        &file_path,
        b"first line\r\nsecond match without trailing newline",
    )
    .unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "second",
            &format!("root:{}", tmp.path().to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("second match without trailing newline"));
    assert!(!stdout.contains("first line"));
}

#[test]
fn test_engine_invalid_utf8_corpus() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("binary_like.bin");

    let mut bytes = vec![0xFF, 0xFE, 0xFD];
    bytes.extend_from_slice(b"\nvalid search target here\n");
    bytes.push(0x80);
    fs::write(&file_path, &bytes).unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "-a",
            "target",
            &format!("root:{}", tmp.path().to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("valid search target here"));
}

#[test]
#[cfg(unix)]
fn test_engine_symlink_cycle_traversal() {
    let tmp = tempdir().unwrap();
    let sub = tmp.path().join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("file.txt"), b"needle inside directory\n").unwrap();

    std::os::unix::fs::symlink(&sub, sub.join("loop")).unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "needle",
            "--follow",
            &format!("root:{}", tmp.path().to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("needle inside directory"));
}

#[test]
fn test_engine_files_without_match_flag() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("match.txt"), b"target needle\n").unwrap();
    fs::write(tmp.path().join("other.txt"), b"unrelated content here\n").unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "-L",
            "needle",
            &format!("root:{}", tmp.path().to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(
        output.status.code(),
        Some(0),
        "Files without match flag should exit 0 when non-matching files exist"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("other.txt"));
    assert!(!stdout.contains("match.txt"));

    let output_json = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "-L",
            "--json",
            "needle",
            &format!("root:{}", tmp.path().to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");
    assert_eq!(output_json.status.code(), Some(0));
    let stdout_json = String::from_utf8_lossy(&output_json.stdout);
    for line in stdout_json.lines() {
        assert!(
            line.starts_with('{') && line.ends_with('}'),
            "Every output line in --json mode must be a valid JSON object: {line}"
        );
    }
    assert!(stdout_json.contains("\"type\":\"file_without_match\""));
    assert!(stdout_json.contains("other.txt"));
    assert!(!stdout_json.contains("match.txt"));

    let output_count_json = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "-c",
            "--json",
            "nonexistent_pattern",
            &format!("root:{}", tmp.path().to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");
    let stdout_count_json = String::from_utf8_lossy(&output_count_json.stdout);
    for line in stdout_count_json.lines() {
        assert!(
            line.starts_with('{') && line.ends_with('}'),
            "Every output line in -c --json mode must be a valid JSON object: {line}"
        );
    }
}

#[test]
fn test_engine_dsl_max_count_limiting() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("many.txt");
    let content = "match line\n".repeat(50);
    fs::write(&file_path, content.as_bytes()).unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "match",
            "max:5",
            &format!("root:{}", tmp.path().to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let count = stdout.lines().filter(|l| l.contains("match line")).count();
    assert_eq!(
        count, 5,
        "max:5 should strictly limit matching output lines to 5"
    );
}

#[test]
#[cfg(unix)]
fn test_engine_broken_pipe_clean_exit() {
    use std::io::Read;
    use std::process::Stdio;

    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("corpus.txt");
    let content = "pipeline test match line\n".repeat(10_000);
    fs::write(&file_path, content.as_bytes()).unwrap();

    let grx_bin = env!("CARGO_BIN_EXE_grx");
    let mut child = std::process::Command::new(grx_bin)
        .args([
            "--hidden",
            "match",
            &format!("root:{}", tmp.path().to_str().unwrap()),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn grx");

    let mut stdout = child.stdout.take().expect("Failed to take stdout pipe");
    let mut buf = [0u8; 64];
    let n = stdout
        .read(&mut buf)
        .expect("Failed to read from child stdout");
    assert!(n > 0, "Expected to read initial output from child");
    drop(stdout);

    let output = child.wait_with_output().expect("Failed to wait on child");
    assert_eq!(
        output.status.code(),
        Some(0),
        "grx must exit cleanly with code 0 on broken pipe, got {:?}",
        output.status.code()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty(),
        "Broken pipe must not emit error messages to stderr: {stderr}"
    );
}

#[test]
#[cfg(unix)]
fn test_engine_traversal_permission_error_suppression() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempdir().unwrap();
    let unreadable = tmp.path().join("unreadable.txt");
    fs::write(&unreadable, b"secret content").unwrap();
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();

    let cli = Cli::parse_from([
        "grx",
        "--hidden",
        "-q",
        "nomatch",
        &format!("root:{}", tmp.path().to_str().unwrap()),
    ]);
    let config = Config::new_with_defaults();
    let mut engine = Engine::new(config, cli);
    let exit_code = engine.run().unwrap();
    assert_eq!(
        exit_code, 1,
        "Should exit 1 (no match) with benign errors suppressed"
    );

    let cli = Cli::parse_from([
        "grx",
        "--hidden",
        "--no-ignore-messages",
        "nomatch",
        &format!("root:{}", tmp.path().to_str().unwrap()),
    ]);
    let config = Config::new_with_defaults();
    let mut engine = Engine::new(config, cli);
    let exit_code = engine.run().unwrap();
    assert_eq!(
        exit_code, 2,
        "Should exit 2 when --no-ignore-messages is set"
    );

    let _ = fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644));
}

#[test]
fn test_engine_no_messages_flag_suppresses_nonexistent_root() {
    let tmp = tempdir().unwrap();
    let nonexistent = tmp.path().join("ghost_file.txt");

    let cli = Cli::parse_from([
        "grx",
        "--hidden",
        "--no-messages",
        "pattern",
        &format!("root:{}", nonexistent.to_str().unwrap()),
    ]);
    let config = Config::new_with_defaults();
    let mut engine = Engine::new(config, cli);
    let exit_code = engine.run().unwrap();
    assert_eq!(
        exit_code, 2,
        "Suppressing diagnostics must not hide an explicit missing-root error"
    );
}

#[test]
fn test_cli_exit_code_2_precedence_over_matches() {
    let tmp = tempdir().unwrap();
    let valid_file = tmp.path().join("valid.txt");
    fs::write(&valid_file, b"target match line\n").unwrap();
    let ghost = tmp.path().join("does_not_exist.txt");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "target",
            &format!("root:{}", valid_file.to_str().unwrap()),
            &format!("root:{}", ghost.to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(
        output.status.code(),
        Some(2),
        "Exit code 2 (error) must take precedence over matches when a target path fails"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("target match line"),
        "Stdout should contain match from valid file"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No such file or directory"),
        "Stderr should report missing file error"
    );
}

#[test]
fn test_cli_positional_target_nonexistent_returns_exit_code_2() {
    let tmp = tempdir().unwrap();
    let ghost = tmp.path().join("ghost_target.txt");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "search_pattern",
            &format!("root:{}", ghost.to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ghost_target.txt"));
    assert!(stderr.contains("No such file or directory"));
}

#[test]
fn test_cli_column_unicode_characters() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("unicode.txt");

    fs::write(&file_path, "🦀 crab rustacean\n").unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "--column",
            "--no-heading",
            "--color",
            "never",
            "crab",
            &format!("root:{}", file_path.to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(":1:3:🦀 crab"),
        "Should show 1-based Unicode character column 3, got: {}",
        stdout
    );
}

#[test]
fn test_cli_top_limit_exact_match_count() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("many_lines.txt");
    let content = "repeatable match line\n".repeat(50);
    fs::write(&file_path, content.as_bytes()).unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "--no-heading",
            "--color",
            "never",
            "repeatable",
            "max:5",
            &format!("root:{}", file_path.to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let matched_lines = stdout.lines().filter(|l| l.contains("repeatable")).count();
    assert_eq!(
        matched_lines, 5,
        "max:5 DSL expression must limit output to exactly 5 matching lines, got {}",
        matched_lines
    );
}

#[test]
fn test_cli_np_proj_excludes_projects_directory() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    let gcc_dir = root
        .join("Data")
        .join("Projects")
        .join("gcc")
        .join("libgrust");
    let cproj_dir = root.join("Data").join("Projects").join("cprojects");
    let docs_dir = root.join("Documents").join("research");

    fs::create_dir_all(&gcc_dir).unwrap();
    fs::create_dir_all(&cproj_dir).unwrap();
    fs::create_dir_all(&docs_dir).unwrap();

    fs::write(gcc_dir.join("mod.rs"), b"pub fn sysctl() {}\n").unwrap();
    fs::write(cproj_dir.join("hello.rs"), b"pub fn shape() {}\n").unwrap();
    fs::write(docs_dir.join("quests.rs"), b"pub fn finish() {}\n").unwrap();

    let output_np = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "--no-heading",
            "--color",
            "never",
            "t:rs",
            "np:proj",
            "in:fn",
            &format!("root:{}", root.to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output_np.status.code(), Some(0));
    let stdout_np = String::from_utf8_lossy(&output_np.stdout);
    assert!(stdout_np.contains("quests.rs"), "Should match quests.rs");
    assert!(
        !stdout_np.contains("Projects"),
        "Should exclude Data/Projects via np:proj"
    );
    assert!(
        !stdout_np.contains("cprojects"),
        "Should exclude cprojects via np:proj"
    );

    let output_no = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "--no-heading",
            "--color",
            "never",
            "t:rs",
            "np:Projects/",
            "in:fn",
            &format!("root:{}", root.to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output_no.status.code(), Some(0));
    let stdout_no = String::from_utf8_lossy(&output_no.stdout);
    assert!(stdout_no.contains("quests.rs"), "Should match quests.rs");
    assert!(
        !stdout_no.contains("Projects"),
        "Should exclude Data/Projects via np:Projects/"
    );
    assert!(
        !stdout_no.contains("cprojects"),
        "Should exclude cprojects via np:Projects/"
    );

    let output_no_proj = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "--no-heading",
            "--color",
            "never",
            "t:rs",
            "np:proj/",
            "in:fn",
            &format!("root:{}", root.to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output_no_proj.status.code(), Some(0));
    let stdout_no_proj = String::from_utf8_lossy(&output_no_proj.stdout);
    assert!(
        stdout_no_proj.contains("Projects"),
        "np:proj/ should not exclude Projects directory without component match"
    );
}

#[test]
fn test_cli_proximity_search_near_and_no_near() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("audit.rs");
    let content = "\
// Line 1
// SAFETY: this pointer is guaranteed valid
let ptr = from_raw(p); // Line 3
foo(); // Line 4
bar(); // Line 5
baz(); // Line 6
qux(); // Line 7
quux(); // Line 8
let dangerous = from_raw(bad); // Line 9 (isolated without safety comment)
";
    fs::write(&file_path, content).unwrap();

    let output_near = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "--no-heading",
            "--color",
            "never",
            "from_raw",
            "near:2,SAFETY",
            &format!("root:{}", file_path.to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output_near.status.code(), Some(0));
    let stdout_near = String::from_utf8_lossy(&output_near.stdout);
    assert!(stdout_near.contains("let ptr = from_raw(p)"));
    assert!(!stdout_near.contains("let dangerous = from_raw(bad)"));

    let output_no_near = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "--no-heading",
            "--color",
            "never",
            "from_raw",
            "no-near:2,SAFETY",
            &format!("root:{}", file_path.to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output_no_near.status.code(), Some(0));
    let stdout_no_near = String::from_utf8_lossy(&output_no_near.stdout);
    assert!(!stdout_no_near.contains("let ptr = from_raw(p)"));
    assert!(stdout_no_near.contains("let dangerous = from_raw(bad)"));
}

#[test]
fn test_cli_proximity_infix_near() {
    let tmp = tempdir().unwrap();
    let file = tmp.path().join("near.rs");
    fs::write(&file, "// SAFETY: aligned\nlet ptr = from_raw(p);\nfoo();\nbar();\nbaz();\nlet dangerous = from_raw(bad);\n").unwrap();
    let root = format!("root:{}", file.display());
    let run = |filter: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .args([
                "--hidden",
                "--no-heading",
                "--color=never",
                "from_raw",
                filter,
            ])
            .arg(&root)
            .output()
            .unwrap()
    };
    let near = run("near:2,SAFETY");
    assert_eq!(near.status.code(), Some(0));
    let text = String::from_utf8_lossy(&near.stdout);
    assert!(text.contains("let ptr = from_raw(p)"));
    assert!(!text.contains("let dangerous = from_raw(bad)"));
    let far = run("no-near:2,SAFETY");
    assert_eq!(far.status.code(), Some(0));
    let text = String::from_utf8_lossy(&far.stdout);
    assert!(!text.contains("let ptr = from_raw(p)"));
    assert!(text.contains("let dangerous = from_raw(bad)"));
    assert_eq!(run("NEAR:2").status.code(), Some(2));
}

#[test]
fn test_cli_proximity_display_formatting_and_bridging() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("audit.rs");
    let content = "\
// Line 1: prelude
// Line 2: SAFETY: this pointer is valid
let intermediate = compute(); // Line 3: bridge context
let ptr = from_raw(p); // Line 4: primary match
// Line 5: end
";
    fs::write(&file_path, content).unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "--no-heading",
            "--color",
            "always",
            "from_raw",
            "near:2,SAFETY",
            &format!("root:{}", file_path.to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("2\x1b[0m:") && stdout.contains("\x1b[1;36mSAFETY\x1b[0m"),
        "stdout should format near match line with colon and cyan highlight: {stdout}"
    );

    assert!(
        stdout.contains("3\x1b[0m-") && stdout.contains("let intermediate = compute()"),
        "stdout should bridge intermediate line with hyphen separator: {stdout}"
    );

    assert!(
        stdout.contains("4\x1b[0m:") && stdout.contains("\x1b[1;31mfrom_raw\x1b[0m"),
        "stdout should format primary match line with colon and red highlight: {stdout}"
    );

    let output_count = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "-c",
            "from_raw",
            "near:2,SAFETY",
            &format!("root:{}", file_path.to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output_count.status.code(), Some(0));
    let stdout_count = String::from_utf8_lossy(&output_count.stdout);
    assert!(
        stdout_count.trim().ends_with(":1"),
        "count should be 1 for primary match, got: {stdout_count}"
    );

    let output_span_count = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "--count-matches",
            "from_raw",
            "near:2,SAFETY",
            &format!("root:{}", file_path.to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output_span_count.status.code(), Some(0));
    let stdout_span_count = String::from_utf8_lossy(&output_span_count.stdout);
    assert!(
        stdout_span_count.trim().ends_with(":1"),
        "match spans count should be 1, got: {stdout_span_count}"
    );

    let sameline_path = tmp.path().join("sameline.rs");
    fs::write(
        &sameline_path,
        "let ptr = from_raw(p); // SAFETY: checked\n",
    )
    .unwrap();

    let output_same = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "--no-heading",
            "--color",
            "always",
            "from_raw",
            "near:2,SAFETY",
            &format!("root:{}", sameline_path.to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output_same.status.code(), Some(0));
    let stdout_same = String::from_utf8_lossy(&output_same.stdout);
    assert!(
        stdout_same.contains("\x1b[1;31mfrom_raw\x1b[0m")
            && stdout_same.contains("\x1b[1;36mSAFETY\x1b[0m"),
        "same-line proximity must highlight primary red and near cyan on same line: {stdout_same}"
    );
}

#[test]
fn test_cli_fuzzy_token_permutation_search() {
    let tmp = tempdir().unwrap();
    let file = tmp.path().join("fuzzy.txt");
    fs::write(&file, "memory\nmxxmxxrxxy\nmy_rem\nunrelated\n").unwrap();
    let root = format!("root:{}", file.display());
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .args(["--hidden", "--no-heading", "--color=never"])
            .args(args)
            .arg(&root)
            .output()
            .unwrap()
    };
    let fuzzy = run(&["fz:mmry"]);
    assert_eq!(
        fuzzy.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&fuzzy.stderr)
    );
    let text = String::from_utf8_lossy(&fuzzy.stdout);
    assert!(text.contains("memory"));
    assert!(text.contains("mxxmxxrxxy"));
    assert!(!text.contains("my_rem"));
    assert!(!text.contains("unrelated"));
    for args in [vec!["-Z", "mmry"], vec!["--fuzzy", "mmry"]] {
        let alias = run(&args);
        assert_eq!(alias.status.code(), Some(0));
        assert_eq!(alias.stdout, fuzzy.stdout);
    }
    assert_eq!(run(&["%%mmry"]).status.code(), Some(2));
}

#[test]
fn test_cli_fuzzy_empty_token_shell_quote_diagnostic() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("commas.txt"), "alpha,beta\nalpha beta\n").unwrap();
    let run = |pattern: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["root:.", "--no-heading", "--color=never", pattern])
            .output()
            .unwrap()
    };
    let empty = run("fz:");
    assert_eq!(empty.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&empty.stderr).contains("requires a nonempty subsequence"));
    let comma = run("fz:a,b");
    assert_eq!(comma.status.code(), Some(0));
    let text = String::from_utf8_lossy(&comma.stdout);
    assert!(text.contains("alpha,beta"));
    assert!(!text.contains("alpha beta"));
}

fn normalize_lines(stdout: &[u8]) -> Vec<String> {
    let s = String::from_utf8_lossy(stdout);
    let mut lines: Vec<String> = s
        .lines()
        .map(|l| l.trim().replace('\\', "/"))
        .filter(|l| !l.is_empty())
        .collect();
    lines.sort();
    lines
}

fn raw_lines(stdout: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(stdout)
        .lines()
        .map(|l| {
            l.trim()
                .trim_start_matches("./")
                .trim_start_matches(".\\")
                .replace('\\', "/")
        })
        .filter(|l| !l.is_empty())
        .collect()
}

fn normalize_nul(stdout: &[u8]) -> Vec<String> {
    let s = String::from_utf8_lossy(stdout);
    let mut entries: Vec<String> = s
        .split('\0')
        .map(|l| {
            l.trim()
                .trim_start_matches("./")
                .trim_start_matches(".\\")
                .replace('\\', "/")
        })
        .filter(|l| !l.is_empty())
        .collect();
    entries.sort();
    entries
}

fn setup_unified_acceptance_corpus() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let tmp_path = tmp.path();

    let doc_dir = tmp_path.join("Documents");
    std::fs::create_dir_all(&doc_dir).unwrap();
    std::fs::write(doc_dir.join("report.md"), b"annual revenue: 100M\n").unwrap();
    std::fs::write(
        doc_dir.join("annual_report_2026.pdf"),
        b"annual revenue summary\n",
    )
    .unwrap();
    std::fs::write(doc_dir.join("other_2026.txt"), b"just notes\n").unwrap();

    let src_dir = tmp_path.join("src");
    let nested_dir = src_dir.join("nested");
    let inner_dir = nested_dir.join("inner");
    std::fs::create_dir_all(&inner_dir).unwrap();
    std::fs::write(
        src_dir.join("main.rs"),
        b"// GRX_MARKER_TEST\nfn main() {\n    println!(\"hello\");\n}\n",
    )
    .unwrap();
    std::fs::write(
        src_dir.join("lib.rs"),
        b"// GRX_MARKER_TEST\npub fn helper() {}\n",
    )
    .unwrap();
    std::fs::write(
        nested_dir.join("deep.rs"),
        b"// GRX_MARKER_TEST\n// deep file\n",
    )
    .unwrap();

    let cache_dir = tmp_path.join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join("temp.cache"), b"cache content\n").unwrap();

    let build_dir = tmp_path.join("build");
    std::fs::create_dir_all(&build_dir).unwrap();
    std::fs::write(build_dir.join("artifact.o"), b"compiled code\n").unwrap();

    let vendor_build_dir = tmp_path.join("vendor").join("build");
    std::fs::create_dir_all(&vendor_build_dir).unwrap();
    std::fs::write(vendor_build_dir.join("vendor_artifact.o"), b"vendor code\n").unwrap();

    let rs_dir = tmp_path.join("rs");
    std::fs::create_dir_all(&rs_dir).unwrap();
    std::fs::write(rs_dir.join("test.txt"), b"test in rs directory\n").unwrap();

    #[cfg(unix)]
    std::fs::write(tmp_path.join("p:path"), b"needle here in colon file\n").unwrap();

    let log_dir = tmp_path.join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    let app_log = log_dir.join("app.log");
    let f = std::fs::File::create(&app_log).unwrap();
    f.set_len(11 * 1024 * 1024).unwrap();
    std::fs::write(log_dir.join("small.log"), b"small log file\n").unwrap();

    #[cfg(unix)]
    {
        let target = src_dir.join("main.rs");
        let link = tmp_path.join("link_to_main.rs");
        let _ = std::os::unix::fs::symlink(&target, &link);

        let broken = tmp_path.join("broken_link");
        let _ = std::os::unix::fs::symlink("nonexistent_target", &broken);
    }

    tmp
}

#[test]
fn test_acceptance_case_01_in_report() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out = std::process::Command::new(grx_bin)
        .args(["kind:file", "report"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out.stdout),
        vec!["Documents/annual_report_2026.pdf", "Documents/report.md"]
    );
}

#[test]
fn test_acceptance_case_02_in_report_type_pdf() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out = std::process::Command::new(grx_bin)
        .args(["kind:file", "report", "t:pdf"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out.stdout),
        vec!["Documents/annual_report_2026.pdf"]
    );
}

#[test]
fn test_acceptance_case_03_in_report_path_documents() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out = std::process::Command::new(grx_bin)
        .args(["kind:file", "report", "root:Documents/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out.stdout),
        vec!["Documents/annual_report_2026.pdf", "Documents/report.md"]
    );
}

#[test]
fn test_acceptance_case_04_content_search_with_in_and_type() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out = std::process::Command::new(grx_bin)
        .args(["in:annual revenue", "t:pdf"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("annual revenue summary"));
}

#[test]
fn test_acceptance_case_05_nonexistent_filter_returns_exit_1() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out = std::process::Command::new(grx_bin)
        .args(["kind:file", "something_something"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}

#[test]
fn test_acceptance_case_06_repeated_in_narrows_with_and() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out = std::process::Command::new(grx_bin)
        .args(["kind:file", "report", "AND", "2026"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out.stdout),
        vec!["Documents/annual_report_2026.pdf"]
    );
}

#[test]
fn test_acceptance_case_07_exact_in_equals_matches_basename() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out = std::process::Command::new(grx_bin)
        .args(["kind:file", "=report.md", "root:Documents/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(normalize_lines(&out.stdout), vec!["Documents/report.md"]);
}

#[test]
fn test_acceptance_case_08_path_src_and_type_rs() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out = std::process::Command::new(grx_bin)
        .args(["root:src/", "t:rs"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out.stdout),
        vec!["src/lib.rs", "src/main.rs", "src/nested/deep.rs"]
    );
}

#[test]
fn test_acceptance_case_09_boolean_and_with_type_and_path() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out = std::process::Command::new(grx_bin)
        .args(["t:rs", "in:fn", "AND", "in:main", "root:src/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("fn main()"));
    assert!(!stdout.contains("pub fn helper()"));
}

#[test]
#[cfg(unix)]
fn test_acceptance_case_10_literal_colon_filename_target() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out = std::process::Command::new(grx_bin)
        .args(["needle", "root:p:path"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("needle here in colon file"));
}

#[test]
fn test_acceptance_case_11_kind_dir_filter() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out = std::process::Command::new(grx_bin)
        .args(["kind:dir", "cache"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(normalize_lines(&out.stdout), vec!["cache/"]);
}

#[test]
fn test_acceptance_case_12_kind_dir_depth_limiting() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out_d1 = std::process::Command::new(grx_bin)
        .args(["kind:dir", "root:src/", "d:1"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_d1.status.code(), Some(0));
    assert_eq!(normalize_lines(&out_d1.stdout), vec!["src/nested/"]);

    let out_d2 = std::process::Command::new(grx_bin)
        .args(["kind:dir", "root:src/", "d:2"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_d2.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out_d2.stdout),
        vec!["src/nested/", "src/nested/inner/"]
    );
}

#[test]
#[cfg(unix)]
fn test_acceptance_case_13_kind_link_filter() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out = std::process::Command::new(grx_bin)
        .args(["kind:link", "root:."])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out.stdout),
        vec!["broken_link", "link_to_main.rs"]
    );
}

#[test]
fn test_acceptance_case_14_larger_size_filter() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out = std::process::Command::new(grx_bin)
        .args(["t:log", "larger:10MiB"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(normalize_lines(&out.stdout), vec!["logs/app.log"]);
}

#[test]
fn test_acceptance_case_15_newer_time_filter() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out = std::process::Command::new(grx_bin)
        .args(["root:src/", "t:rs", "newer:7d"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out.stdout),
        vec!["src/lib.rs", "src/main.rs", "src/nested/deep.rs"]
    );
}

#[test]
fn test_acceptance_case_16_kind_dir_not_path_and_name_anchors() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out = std::process::Command::new(grx_bin)
        .args(["kind:dir", "build", "np:vendor/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(normalize_lines(&out.stdout), vec!["build/"]);

    let out_anchor_start = std::process::Command::new(grx_bin)
        .args(["kind:file", "^report"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_anchor_start.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out_anchor_start.stdout),
        vec!["Documents/report.md"]
    );

    let out_anchor_end = std::process::Command::new(grx_bin)
        .args(["kind:file", "pdf$"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_anchor_end.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out_anchor_end.stdout),
        vec!["Documents/annual_report_2026.pdf"]
    );

    let out_seq = std::process::Command::new(grx_bin)
        .args(["kind:file", "annual..pdf"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_seq.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out_seq.stdout),
        vec!["Documents/annual_report_2026.pdf"]
    );

    let out_trs = std::process::Command::new(grx_bin)
        .args(["t:rs"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_trs.status.code(), Some(0));
    let trs_files = normalize_lines(&out_trs.stdout);
    assert!(!trs_files.iter().any(|f| f.starts_with("rs/")));

    let out_bare = std::process::Command::new(grx_bin).output().unwrap();
    assert_eq!(out_bare.status.code(), Some(2));
    let help_bare = format!(
        "{}{}",
        String::from_utf8_lossy(&out_bare.stdout),
        String::from_utf8_lossy(&out_bare.stderr)
    );
    assert!(
        help_bare.contains("USAGE:")
            || help_bare.contains("help")
            || help_bare.contains("Try 'grx --help'")
    );

    use std::io::Write;
    let mut child = std::process::Command::new(grx_bin)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        let _ = stdin.write_all(b"piped content without args\n");
    }
    let out_piped = child.wait_with_output().unwrap();
    assert_eq!(out_piped.status.code(), Some(2));

    let out_q_match = std::process::Command::new(grx_bin)
        .args(["-q", "kind:file", "report"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_q_match.status.code(), Some(0));
    assert!(out_q_match.stdout.is_empty());

    let out_q_nomatch = std::process::Command::new(grx_bin)
        .args(["-q", "kind:file", "nonexistent_pattern"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_q_nomatch.status.code(), Some(1));
    assert!(out_q_nomatch.stdout.is_empty());

    let out_nul = std::process::Command::new(grx_bin)
        .args(["-0", "kind:file", "report", "root:Documents/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_nul.status.code(), Some(0));
    assert!(out_nul.stdout.contains(&0u8));
    assert_eq!(
        normalize_nul(&out_nul.stdout),
        vec!["Documents/annual_report_2026.pdf", "Documents/report.md"]
    );

    let err_dir_content = std::process::Command::new(grx_bin)
        .args(["-e", "content_pattern", "kind:dir"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(err_dir_content.status.code(), Some(2));

    let err_link_content = std::process::Command::new(grx_bin)
        .args(["-e", "content_pattern", "kind:link"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(err_link_content.status.code(), Some(2));

    let ok_dir_bare = std::process::Command::new(grx_bin)
        .args(["kind:dir", "nested"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(ok_dir_bare.status.code(), Some(0));
    assert_eq!(normalize_lines(&ok_dir_bare.stdout), vec!["src/nested/"]);

    let err_stdin_discovery = std::process::Command::new(grx_bin)
        .args(["kind:file", "report", "-"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(err_stdin_discovery.status.code(), Some(2));
    let err_stderr = String::from_utf8_lossy(&err_stdin_discovery.stderr);
    assert!(err_stderr.contains("stdin"));

    let err_ctx_only = std::process::Command::new(grx_bin)
        .args(["ctx:2"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(err_ctx_only.status.code(), Some(2));

    let err_size_dir = std::process::Command::new(grx_bin)
        .args(["kind:dir", "larger:10MiB"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(err_size_dir.status.code(), Some(2));

    let disc_parity = std::process::Command::new(grx_bin)
        .args(["root:src/", "t:rs"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    let content_parity = std::process::Command::new(grx_bin)
        .args(["-l", "in:GRX_MARKER_TEST", "root:src/", "t:rs"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(disc_parity.status.code(), Some(0));
    assert_eq!(content_parity.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&disc_parity.stdout),
        normalize_lines(&content_parity.stdout)
    );
}

#[test]
fn test_acceptance_case_17_discovery_colors_and_hyperlinks() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out_color = std::process::Command::new(grx_bin)
        .args([
            "--color=always",
            "kind:file",
            "=report.md",
            "root:Documents/",
        ])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_color.status.code(), Some(0));
    let color_stdout = String::from_utf8_lossy(&out_color.stdout);
    let sep = std::path::MAIN_SEPARATOR;
    assert!(color_stdout.contains(&format!(
        "\x1b[38;5;81mDocuments{sep}\x1b[0m\x1b[30;48;5;220mreport.md\x1b[0m"
    )));

    let out_dir_color = std::process::Command::new(grx_bin)
        .args(["--color=always", "kind:dir", "cache"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_dir_color.status.code(), Some(0));
    let dir_color_stdout = String::from_utf8_lossy(&out_dir_color.stdout);
    assert!(dir_color_stdout.contains("\x1b[30;48;5;220mcache\x1b[0m\x1b[38;5;81m/\x1b[0m"));

    let out_links = std::process::Command::new(grx_bin)
        .args([
            "--hyperlinks=always",
            "kind:file",
            "report",
            "root:Documents/",
        ])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_links.status.code(), Some(0));
    let links_stdout = String::from_utf8_lossy(&out_links.stdout);
    assert!(links_stdout.contains("\x1b]8;;file://"));
    assert!(links_stdout.contains("\x1b]8;;\x1b\\"));
}

#[test]
fn test_acceptance_case_18_path_separator_warning_for_in_and_ni() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("paths.txt"),
        "include src/file.rs\nnothing\n",
    )
    .unwrap();
    let slash = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(tmp.path())
        .args(["root:.", "--color=never", "in:src/"])
        .output()
        .unwrap();
    assert_eq!(slash.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&slash.stdout).contains("include src/file.rs"));
    assert!(slash.stderr.is_empty());
    let retired = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(tmp.path())
        .arg("ni:some/dir")
        .output()
        .unwrap();
    assert_eq!(retired.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&retired.stderr).contains("ni: is retired"));
}

#[test]
fn test_acceptance_case_19_dir_and_file_exact_selectors() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out_dir_src = std::process::Command::new(grx_bin)
        .args(["kind:dir", "src", "np:data"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_dir_src.status.code(), Some(0));
    assert_eq!(normalize_lines(&out_dir_src.stdout), vec!["src/"]);

    let out_dir_exact_tail = std::process::Command::new(grx_bin)
        .args(["--color=always", "kind:dir", "=src", "tail:5"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_dir_exact_tail.status.code(), Some(0));
    let dir_tail_str = String::from_utf8_lossy(&out_dir_exact_tail.stdout);
    assert!(dir_tail_str.contains("\x1b[30;48;5;220msrc\x1b[0m\x1b[38;5;81m/\x1b[0m"));

    let out_file_main = std::process::Command::new(grx_bin)
        .args(["kind:file", "=main.rs"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_file_main.status.code(), Some(0));
    assert_eq!(normalize_lines(&out_file_main.stdout), vec!["src/main.rs"]);
}

#[test]
fn test_kind_position_distinguishes_names_from_content() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("needle_name.txt"), b"unrelated\n").unwrap();
    fs::write(root.join("body_only.txt"), b"needle\n").unwrap();
    fs::write(root.join("needle_bin.dat"), b"\0unrelated\0").unwrap();
    fs::write(root.join("body_bin.dat"), b"\0needle\0").unwrap();
    fs::create_dir(root.join("needle_dir")).unwrap();

    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(root)
            .args(args)
            .output()
            .unwrap()
    };

    let files_by_name = run(&["kind:file", "needle"]);
    assert_eq!(files_by_name.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&files_by_name.stdout),
        vec!["needle_bin.dat", "needle_name.txt"]
    );

    let exact_file_name = run(&["kind:file", "=needle_name.txt"]);
    assert_eq!(exact_file_name.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&exact_file_name.stdout),
        vec!["needle_name.txt"]
    );

    let files_by_content = run(&["-l", "-e", "needle", "kind:file"]);
    assert_eq!(files_by_content.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&files_by_content.stdout),
        vec!["body_only.txt"]
    );

    let explicit_content = run(&["-l", "-e", "needle", "kind:file"]);
    assert_eq!(explicit_content.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&explicit_content.stdout),
        vec!["body_only.txt"]
    );

    let binaries_by_name = run(&["kind:bin", "needle"]);
    assert_eq!(binaries_by_name.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&binaries_by_name.stdout),
        vec!["needle_bin.dat"]
    );

    let binaries_by_content = run(&["-l", "-e", "needle", "kind:bin"]);
    assert_eq!(binaries_by_content.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&binaries_by_content.stdout),
        vec!["body_bin.dat"]
    );

    let text_by_name = run(&["kind:text", "needle"]);
    assert_eq!(text_by_name.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&text_by_name.stdout),
        vec!["needle_name.txt"]
    );

    let dirs_by_name = run(&["kind:dir", "needle"]);
    assert_eq!(dirs_by_name.status.code(), Some(0));
    assert_eq!(normalize_lines(&dirs_by_name.stdout), vec!["needle_dir/"]);

    let dirs_by_at_name = run(&["kind:dir", "=needle_dir"]);
    assert_eq!(dirs_by_at_name.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&dirs_by_at_name.stdout),
        vec!["needle_dir/"]
    );

    let dirs_by_eq = run(&["kind:dir", "=needle_dir"]);
    assert_eq!(dirs_by_eq.status.code(), Some(0));
    assert_eq!(normalize_lines(&dirs_by_eq.stdout), vec!["needle_dir/"]);

    let files_by_name_reverse = run(&["needle", "kind:file"]);
    assert_eq!(files_by_name_reverse.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&files_by_name_reverse.stdout),
        vec!["needle_bin.dat", "needle_name.txt"]
    );

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("body_only.txt", root.join("needle_link")).unwrap();
        let links_by_name = run(&["kind:link", "needle"]);
        assert_eq!(links_by_name.status.code(), Some(0));
        assert_eq!(normalize_lines(&links_by_name.stdout), vec!["needle_link"]);

        let links_by_at = run(&["kind:link", "=needle_link"]);
        assert_eq!(links_by_at.status.code(), Some(0));
        assert_eq!(normalize_lines(&links_by_at.stdout), vec!["needle_link"]);

        let links_by_name_reverse = run(&["needle", "kind:link"]);
        assert_eq!(links_by_name_reverse.status.code(), Some(0));
        assert_eq!(
            normalize_lines(&links_by_name_reverse.stdout),
            vec!["needle_link"]
        );

        let invalid_link_content = run(&["-e", "needle", "kind:link"]);
        assert_eq!(invalid_link_content.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&invalid_link_content.stderr).contains("content constraints")
        );
    }

    let invalid_dir_content = run(&["-e", "needle", "kind:dir"]);
    assert_eq!(invalid_dir_content.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&invalid_dir_content.stderr).contains("content constraints"));

    let boolean_name = run(&["kind:file", "needle", "OR", "body"]);
    assert_eq!(boolean_name.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&boolean_name.stdout),
        vec![
            "body_bin.dat",
            "body_only.txt",
            "needle_bin.dat",
            "needle_name.txt"
        ]
    );

    let boolean_name_reverse = run(&["needle", "OR", "body", "kind:file"]);
    assert_eq!(boolean_name_reverse.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&boolean_name_reverse.stdout),
        vec![
            "body_bin.dat",
            "body_only.txt",
            "needle_bin.dat",
            "needle_name.txt"
        ]
    );

    let strings = run(&["str:4", "kind:bin"]);
    assert_eq!(strings.status.code(), Some(0));
    let strings_output = String::from_utf8_lossy(&strings.stdout);
    assert!(strings_output.contains("body_bin.dat:1:needle"));
    assert!(strings_output.contains("needle_bin.dat:1:unrelated"));
}

#[test]
fn test_discovery_highlights_matched_basename_without_changing_machine_output() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir(root.join("scripts")).unwrap();
    fs::write(root.join("scripts/bump_version.py"), b"rs\n").unwrap();
    fs::write(root.join("scripts/rsrs.rs"), b"nothing here\n").unwrap();

    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(root)
            .args(args)
            .output()
            .unwrap()
    };

    let colored = run(&["--color=always", "kind:file", "rs"]);
    assert_eq!(colored.status.code(), Some(0));
    let displayed = String::from_utf8(colored.stdout).unwrap();
    assert!(displayed.contains("\x1b[38;5;81mscripts/\x1b[0m"));
    assert!(displayed.contains("bump_ve\x1b[0m\x1b[30;48;5;220mrs\x1b[0m"));
    assert!(displayed.contains("\x1b[30;48;5;220mrsrs\x1b[0m"));
    assert!(!displayed.contains("\x1b[30;48;5;220mscripts"));

    let sorted = run(&["--color=always", "kind:file", "rs", "sort:path"]);
    assert_eq!(sorted.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&sorted.stdout).contains("\x1b[30;48;5;220mrs\x1b[0m"));

    let case = run(&["--color=always", "-i", "kind:file", "RS"]);
    assert_eq!(case.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&case.stdout).contains("\x1b[30;48;5;220mrs\x1b[0m"));

    let wildcard = run(&["--color=always", "kind:file", "bump..ion"]);
    assert_eq!(wildcard.status.code(), Some(0));
    let wildcard_text = String::from_utf8_lossy(&wildcard.stdout);
    assert!(wildcard_text.contains("\x1b[30;48;5;220mbump\x1b[0m"));
    assert!(wildcard_text.contains("\x1b[30;48;5;220mion\x1b[0m"));
    assert!(!wildcard_text.contains("\x1b[30;48;5;220m_version"));

    let selectors = run(&["--color=always", "kind:file", "bump", "AND", "ion"]);
    assert_eq!(selectors.status.code(), Some(0));
    let selectors_text = String::from_utf8_lossy(&selectors.stdout);
    assert!(selectors_text.contains("\x1b[30;48;5;220mbump\x1b[0m"));
    assert!(selectors_text.contains("\x1b[30;48;5;220mion\x1b[0m"));

    let linked = run(&["--color=always", "--hyperlinks=always", "kind:file", "rs"]);
    assert_eq!(linked.status.code(), Some(0));
    let linked_text = String::from_utf8_lossy(&linked.stdout);
    assert!(linked_text.contains("\x1b]8;;file://"));
    assert!(linked_text.contains("\x1b[30;48;5;220mrs\x1b[0m"));

    let content = run(&["--color=always", "-e", "rs", "kind:file"]);
    assert_eq!(content.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&content.stdout).contains("\x1b[1;31mrs\x1b[0m"));

    let plain = run(&["kind:file", "rs"]);
    assert_eq!(plain.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&plain.stdout),
        vec!["scripts/bump_version.py", "scripts/rsrs.rs"]
    );
    assert!(!plain.stdout.contains(&0x1b));

    let nul = run(&["--color=always", "-0", "kind:file", "rs"]);
    assert_eq!(nul.status.code(), Some(0));
    assert!(!nul.stdout.contains(&0x1b));
    assert!(nul.stdout.contains(&0));

    let json = run(&["--color=always", "--json", "kind:file", "rs"]);
    assert_eq!(json.status.code(), Some(0));
    assert!(!json.stdout.contains(&0x1b));
    assert!(String::from_utf8_lossy(&json.stdout).contains("scripts/bump_version.py"));
}

#[test]
fn test_acceptance_case_20_head_tail_mutual_exclusivity() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out_head_tail_dsl = std::process::Command::new(grx_bin)
        .args(["head:5", "tail:5"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_head_tail_dsl.status.code(), Some(2));
    let head_tail_err = String::from_utf8_lossy(&out_head_tail_dsl.stderr);
    assert!(head_tail_err.contains("head") && head_tail_err.contains("tail"));

    let out_head_tail_cli = std::process::Command::new(grx_bin)
        .args(["--head", "5", "--tail", "5", "pattern"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_head_tail_cli.status.code(), Some(2));

    fs::write(tmp_path.join("limit.txt"), "pattern one\npattern two\n").unwrap();
    let out_m_tail = std::process::Command::new(grx_bin)
        .args(["-m", "1", "tail:1", "pattern", "root:limit.txt"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_m_tail.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out_m_tail.stdout).contains("pattern one"));
    assert!(!String::from_utf8_lossy(&out_m_tail.stdout).contains("pattern two"));
}

#[test]
fn test_acceptance_case_21_sort_keys_in_discovery_and_content() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out_sort_size = std::process::Command::new(grx_bin)
        .args(["root:logs/", "sort:size"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_sort_size.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_sort_size.stdout),
        vec!["15  logs/small.log", "11M  logs/app.log"]
    );

    let out_sort_size_desc = std::process::Command::new(grx_bin)
        .args(["root:logs/", "sort:-size"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_sort_size_desc.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_sort_size_desc.stdout),
        vec!["11M  logs/app.log", "15  logs/small.log"]
    );

    let out_sort_largest = std::process::Command::new(grx_bin)
        .args(["root:logs/", "sort:largest"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_sort_largest.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_sort_largest.stdout),
        vec!["11M  logs/app.log", "15  logs/small.log"]
    );

    let out_sort_smallest = std::process::Command::new(grx_bin)
        .args(["root:logs/", "sort:smallest"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_sort_smallest.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_sort_smallest.stdout),
        vec!["15  logs/small.log", "11M  logs/app.log"]
    );

    let out_cli_sort_rev = std::process::Command::new(grx_bin)
        .args(["--sort", "size", "--reverse", "root:logs/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_cli_sort_rev.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_cli_sort_rev.stdout),
        vec!["11M  logs/app.log", "15  logs/small.log"]
    );

    let out_sort_len = std::process::Command::new(grx_bin)
        .args(["root:logs/", "sort:len"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_sort_len.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_sort_len.stdout),
        vec!["12  logs/app.log", "14  logs/small.log"]
    );

    std::fs::write(
        tmp_path.join("lines.txt"),
        b"very long line with match\nshort match\nmedium line with match\n",
    )
    .unwrap();
    let out_sort_linelen = std::process::Command::new(grx_bin)
        .args([
            "--no-heading",
            "-N",
            "match",
            "root:lines.txt",
            "sort:line-len",
        ])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_sort_linelen.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_sort_linelen.stdout),
        vec![
            "11  lines.txt:short match",
            "22  lines.txt:medium line with match",
            "25  lines.txt:very long line with match",
        ]
    );

    let out_sort_linelen_desc = std::process::Command::new(grx_bin)
        .args([
            "--no-heading",
            "-N",
            "match",
            "root:lines.txt",
            "sort:-line-len",
        ])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_sort_linelen_desc.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_sort_linelen_desc.stdout),
        vec![
            "25  lines.txt:very long line with match",
            "22  lines.txt:medium line with match",
            "11  lines.txt:short match",
        ]
    );
}

#[test]
fn test_acceptance_case_22_tail_limit_over_bulk_files() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let bulk_dir = tmp_path.join("bulk");
    std::fs::create_dir_all(&bulk_dir).unwrap();
    for i in 1..=10 {
        std::fs::write(bulk_dir.join(format!("file{:02}.txt", i)), b"content\n").unwrap();
    }
    let out_tail_limit = std::process::Command::new(grx_bin)
        .args(["root:bulk/", "sort:path", "tail:3"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_tail_limit.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_tail_limit.stdout),
        vec!["bulk/file08.txt", "bulk/file09.txt", "bulk/file10.txt"]
    );
}

#[test]
fn test_acceptance_case_23_name_collision_dir_vs_file() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let collision_dir = tmp_path.join("collision");
    std::fs::create_dir_all(collision_dir.join("shared_name")).unwrap();
    std::fs::write(collision_dir.join("regular_file"), b"hello\n").unwrap();

    let out_collision_dir = std::process::Command::new(grx_bin)
        .args(["root:collision/", "kind:dir", "=shared_name"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_collision_dir.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out_collision_dir.stdout),
        vec!["collision/shared_name/"]
    );

    let out_collision_file = std::process::Command::new(grx_bin)
        .args(["root:collision/", "kind:file", "=regular_file"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_collision_file.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out_collision_file.stdout),
        vec!["collision/regular_file"]
    );
}

#[test]
fn test_acceptance_case_24_dir_trailing_slash_support() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out_dir_slash = std::process::Command::new(grx_bin)
        .args(["kind:dir", "src"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_dir_slash.status.code(), Some(0));
    assert_eq!(normalize_lines(&out_dir_slash.stdout), vec!["src/"]);

    let out_deprecated = std::process::Command::new(grx_bin)
        .args(["dir:src/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_deprecated.status.code(), Some(2));
}

#[test]
fn test_acceptance_case_25_trailing_flag_rejection_and_sort_syntax() {
    let tmp = setup_unified_acceptance_corpus();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(args)
            .output()
            .unwrap()
    };
    let trailing = run(&["FN", "root:src/main.rs", "-i", "--color=never"]);
    assert_eq!(
        trailing.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&trailing.stderr)
    );
    assert!(String::from_utf8_lossy(&trailing.stdout).contains("fn "));
    let leading = run(&["-i", "--color=never", "FN", "root:src/main.rs"]);
    assert_eq!(trailing.stdout, leading.stdout);
    let sort = run(&["kind:file", "=main.rs", "--sort=size"]);
    assert_eq!(sort.status.code(), Some(0));
    assert_eq!(normalize_lines(&sort.stdout), vec!["56  src/main.rs"]);
}

#[test]
fn test_acceptance_case_26_sort_count_and_stats_telemetry() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let density_dir = tmp_path.join("density");
    std::fs::create_dir_all(&density_dir).unwrap();
    std::fs::write(density_dir.join("low.txt"), b"needle once\nother line\n").unwrap();
    std::fs::write(
        density_dir.join("high.txt"),
        b"needle one\nneedle two\nneedle three\n",
    )
    .unwrap();

    let out_sort_count = std::process::Command::new(grx_bin)
        .args([
            "--no-heading",
            "-N",
            "needle",
            "root:density/",
            "sort:count",
        ])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_sort_count.status.code(), Some(0));
    let count_lines = raw_lines(&out_sort_count.stdout);
    assert_eq!(count_lines.len(), 4);
    assert!(count_lines[0].contains("density/high.txt"));
    assert!(count_lines[1].contains("density/high.txt"));
    assert!(count_lines[2].contains("density/high.txt"));
    assert!(count_lines[3].contains("density/low.txt"));
    assert!(count_lines[0].starts_with("3  density/high.txt"));
    assert!(count_lines[3].starts_with("1  density/low.txt"));

    let out_stats_tail = std::process::Command::new(grx_bin)
        .args([
            "--stats",
            "--no-heading",
            "needle",
            "root:density/",
            "tail:2",
        ])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_stats_tail.status.code(), Some(0));
    let stats_out = String::from_utf8_lossy(&out_stats_tail.stdout);
    assert!(stats_out.contains("2 matches"));
    assert!(stats_out.contains("2 matched lines"));
}

#[test]
fn test_acceptance_case_27_nth_and_pick() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out_nth_1 = std::process::Command::new(grx_bin)
        .args(["root:logs/", "sort:path", "--nth", "1"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(
        out_nth_1.status.code(),
        Some(0),
        "out_nth_1 failed: {}",
        String::from_utf8_lossy(&out_nth_1.stderr)
    );
    assert_eq!(raw_lines(&out_nth_1.stdout), vec!["logs/app.log"]);

    let out_nth_2 = std::process::Command::new(grx_bin)
        .args(["root:logs/", "sort:path", "--nth", "2"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_nth_2.status.code(), Some(0));
    assert_eq!(raw_lines(&out_nth_2.stdout), vec!["logs/small.log"]);

    let out_nth_neg1 = std::process::Command::new(grx_bin)
        .args(["root:logs/", "sort:path", "--nth", "-1"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_nth_neg1.status.code(), Some(0));
    assert_eq!(raw_lines(&out_nth_neg1.stdout), vec!["logs/small.log"]);

    let out_nth_neg2 = std::process::Command::new(grx_bin)
        .args(["root:logs/", "sort:path", "--nth", "-2"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_nth_neg2.status.code(), Some(0));
    assert_eq!(raw_lines(&out_nth_neg2.stdout), vec!["logs/app.log"]);

    let out_dsl_nth = std::process::Command::new(grx_bin)
        .args(["root:logs/", "sort:path", "nth:2"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_dsl_nth.status.code(), Some(0));
    assert_eq!(raw_lines(&out_dsl_nth.stdout), vec!["logs/small.log"]);

    let out_oob = std::process::Command::new(grx_bin)
        .args(["root:logs/", "sort:path", "--nth", "99"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_oob.status.code(), Some(1));
    assert!(out_oob.stdout.is_empty());

    let out_exec = std::process::Command::new(grx_bin)
        .args([
            "root:logs/",
            "sort:path",
            "--nth",
            "2",
            "-X",
            "echo",
            "FILE:{}",
        ])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_exec.status.code(), Some(0));
    let exec_str = String::from_utf8_lossy(&out_exec.stdout);
    assert!(exec_str.contains("FILE:logs/small.log"));
    assert!(!exec_str.contains("app.log"));

    let out_zero = std::process::Command::new(grx_bin)
        .args(["kind:file", "rs", "--nth", "0"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_zero.status.code(), Some(2));
    let err_str = String::from_utf8_lossy(&out_zero.stderr);
    assert!(err_str.contains("cannot be 0"));

    let out_conf = std::process::Command::new(grx_bin)
        .args(["kind:file", "rs", "--nth", "1", "--head", "5"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_conf.status.code(), Some(2));

    let out_pick_non_tty = std::process::Command::new(grx_bin)
        .args(["kind:file", "rs", "--pick"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_pick_non_tty.status.code(), Some(2));
    let pick_err = String::from_utf8_lossy(&out_pick_non_tty.stderr);
    assert!(pick_err.contains("--pick requires an interactive terminal"));

    let out_pick_first = std::process::Command::new(grx_bin)
        .args(["root:logs/", "sort:path", "--pick"])
        .env("GRX_TEST_PICK_KEYS", "\n")
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_pick_first.status.code(), Some(0));
    assert_eq!(raw_lines(&out_pick_first.stdout), vec!["logs/app.log"]);

    let out_pick_second_x = std::process::Command::new(grx_bin)
        .args([
            "root:logs/",
            "sort:path",
            "--pick",
            "-X",
            "echo",
            "PICKED:{}",
        ])
        .env("GRX_TEST_PICK_KEYS", "\x1b[B\n")
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_pick_second_x.status.code(), Some(0));
    let pick_second_str = String::from_utf8_lossy(&out_pick_second_x.stdout);
    assert!(pick_second_str.contains("PICKED:logs/small.log"));

    let out_pick_cancel = std::process::Command::new(grx_bin)
        .args([
            "root:logs/",
            "sort:path",
            "--pick",
            "-X",
            "echo",
            "SHOULD_NOT_RUN:{}",
        ])
        .env("GRX_TEST_PICK_KEYS", "\x1b")
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_pick_cancel.status.code(), Some(130));
    let pick_cancel_str = String::from_utf8_lossy(&out_pick_cancel.stdout);
    assert!(!pick_cancel_str.contains("SHOULD_NOT_RUN"));
}

#[test]
fn test_acceptance_case_28_line_number_and_nth_parity() {
    let grx_bin = env!("CARGO_BIN_EXE_grx");
    let tmp = tempfile::tempdir().unwrap();
    let tmp_path = tmp.path();

    std::fs::create_dir_all(tmp_path.join("items")).unwrap();
    std::fs::write(tmp_path.join("items/alpha.txt"), "a").unwrap();
    std::fs::write(tmp_path.join("items/beta.txt"), "b").unwrap();
    std::fs::write(tmp_path.join("items/gamma.txt"), "g").unwrap();

    let out_n = std::process::Command::new(grx_bin)
        .args(["kind:file", "root:items/", "--line-number"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_n.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_n.stdout),
        vec!["1:items/alpha.txt", "2:items/beta.txt", "3:items/gamma.txt"]
    );

    let out_long = std::process::Command::new(grx_bin)
        .args(["kind:file", "root:items/", "--line-number"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_long.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_long.stdout),
        vec!["1:items/alpha.txt", "2:items/beta.txt", "3:items/gamma.txt"]
    );

    let out_dsl = std::process::Command::new(grx_bin)
        .args(["kind:file", "root:items/", "--line-number"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_dsl.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_dsl.stdout),
        vec!["1:items/alpha.txt", "2:items/beta.txt", "3:items/gamma.txt"]
    );

    let out_nth_2 = std::process::Command::new(grx_bin)
        .args(["kind:file", "root:items/", "--nth", "2"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_nth_2.status.code(), Some(0));
    assert_eq!(raw_lines(&out_nth_2.stdout), vec!["items/beta.txt"]);

    let out_nth_3 = std::process::Command::new(grx_bin)
        .args(["kind:file", "root:items/", "--nth", "3"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_nth_3.status.code(), Some(0));
    assert_eq!(raw_lines(&out_nth_3.stdout), vec!["items/gamma.txt"]);
}

#[test]
fn test_engine_compositional_correctness() {
    use std::io::Write;
    let grx_bin = env!("CARGO_BIN_EXE_grx");
    let tmp = tempfile::tempdir().unwrap();
    let tmp_path = tmp.path();

    {
        let mut child_sort = std::process::Command::new(grx_bin)
            .args(["--no-heading", "-N", "hit", "sort:path"])
            .current_dir(tmp_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child_sort
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"hit pipe\n")
            .unwrap();
        let out_sort = child_sort.wait_with_output().unwrap();
        assert_eq!(out_sort.status.code(), Some(0));
        let text = String::from_utf8_lossy(&out_sort.stdout);
        assert!(text.contains("hit pipe"));

        let mut child_tail = std::process::Command::new(grx_bin)
            .args(["--no-heading", "-N", "hit", "tail:1"])
            .current_dir(tmp_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child_tail
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"hit pipe\n")
            .unwrap();
        let out_tail = child_tail.wait_with_output().unwrap();
        assert_eq!(out_tail.status.code(), Some(0));
        let text_tail = String::from_utf8_lossy(&out_tail.stdout);
        assert!(text_tail.contains("hit pipe"));
    }

    {
        let ctx_file = tmp_path.join("context.txt");
        fs::write(&ctx_file, b"before\nhit\nafter\n").unwrap();

        let out_tail1 = std::process::Command::new(grx_bin)
            .args([
                "--no-heading",
                "-N",
                "-A",
                "1",
                "hit",
                "root:context.txt",
                "tail:1",
            ])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_tail1.status.code(), Some(0));
        let text1 = String::from_utf8_lossy(&out_tail1.stdout);
        assert!(text1.contains("hit"));
        assert!(text1.contains("after"));

        let out_head1 = std::process::Command::new(grx_bin)
            .args([
                "--no-heading",
                "-N",
                "-B",
                "1",
                "hit",
                "root:context.txt",
                "head:1",
                "sort:path",
            ])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_head1.status.code(), Some(0));
        let text2 = String::from_utf8_lossy(&out_head1.stdout);
        assert!(text2.contains("before"));
        assert!(text2.contains("hit"));

        let out_tail0 = std::process::Command::new(grx_bin)
            .args(["--no-heading", "-N", "hit", "root:context.txt", "tail:0"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_tail0.status.code(), Some(1));
        assert!(out_tail0.stdout.is_empty());
    }

    {
        let file_a = tmp_path.join("a.txt");
        let file_b = tmp_path.join("b.txt");
        fs::write(&file_a, b"hit a1\nhit a2\n").unwrap();
        fs::write(&file_b, b"hit b1\nhit b2\n").unwrap();

        let out_m_sort = std::process::Command::new(grx_bin)
            .args([
                "--no-heading",
                "-N",
                "-m",
                "1",
                "hit",
                "root:a.txt",
                "root:b.txt",
                "sort:path",
            ])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_m_sort.status.code(), Some(0));
        let lines = raw_lines(&out_m_sort.stdout);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("hit a1"));
        assert!(lines[1].contains("hit b1"));
    }

    {
        let empty_pat = tmp_path.join("empty.patterns");
        fs::write(&empty_pat, b"").unwrap();

        let out_empty_pat = std::process::Command::new(grx_bin)
            .args(["-f", "empty.patterns", "root:."])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_empty_pat.status.code(), Some(1));
        assert!(out_empty_pat.stdout.is_empty());
    }

    #[cfg(unix)]
    {
        let target_file = tmp_path.join("target_sym.txt");
        fs::write(&target_file, b"content").unwrap();
        let link_file = tmp_path.join("link_sym.txt");
        let _ = fs::remove_file(&link_file);
        std::os::unix::fs::symlink("target_sym.txt", &link_file).unwrap();

        let out_sym = std::process::Command::new(grx_bin)
            .args(["kind:link", "link_sym"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_sym.status.code(), Some(0));
        let lines = raw_lines(&out_sym.stdout);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("link_sym.txt"));
    }

    #[cfg(unix)]
    {
        let rep_dir = tmp_path.join("case_test");
        fs::create_dir_all(&rep_dir).unwrap();
        fs::write(rep_dir.join("report.txt"), b"").unwrap();
        fs::write(rep_dir.join("REPORT.txt"), b"").unwrap();

        let out_i = std::process::Command::new(grx_bin)
            .args(["-i", "kind:file", "REPORT", "root:case_test/"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_i.status.code(), Some(0));
        let lines_i = raw_lines(&out_i.stdout);
        assert_eq!(lines_i.len(), 2);

        let out_s = std::process::Command::new(grx_bin)
            .args(["-s", "kind:file", "report", "root:case_test/"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_s.status.code(), Some(0));
        let lines_s = raw_lines(&out_s.stdout);
        assert_eq!(lines_s.len(), 1);
        assert!(lines_s[0].contains("report.txt"));
    }

    {
        let flags_file = tmp_path.join("flags.txt");
        fs::write(&flags_file, b"hit F\nhit iv\nhit other\n").unwrap();

        let out_f = std::process::Command::new(grx_bin)
            .args(["hit", "-F", "root:flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_f.status.code(), Some(0));
        assert_eq!(raw_lines(&out_f.stdout).len(), 3);
        assert!(out_f.stderr.is_empty());

        let out_iv = std::process::Command::new(grx_bin)
            .args(["hit", "-iv", "root:flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_iv.status.code(), Some(1));
        assert!(out_iv.stdout.is_empty());
        assert!(out_iv.stderr.is_empty());
    }

    {
        use std::io::Read;
        let mut child = std::process::Command::new(grx_bin)
            .args(["kind:file", "txt", "sort:path"])
            .current_dir(tmp_path)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(mut reader) = child.stdout.take() {
            let mut buf = [0u8; 4];
            let _ = reader.read(&mut buf);
            drop(reader);
        }
        let status = child.wait().unwrap();
        assert_eq!(status.code(), Some(0));
    }

    {
        let disc_dir = tmp_path.join("disc_limits");
        fs::create_dir_all(&disc_dir).unwrap();
        for i in 0..5 {
            fs::write(disc_dir.join(format!("file_{i}.txt")), b"").unwrap();
        }

        let out_stats = std::process::Command::new(grx_bin)
            .args(["--stats", "kind:file", "txt", "root:disc_limits/", "head:1"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_stats.status.code(), Some(0));
        let stats_str = String::from_utf8_lossy(&out_stats.stdout);
        assert!(stats_str.contains("1 matches"), "must report 1 matches");
        assert!(
            stats_str.contains("1 matched lines"),
            "must report 1 matched lines"
        );
        let non_stats_lines: Vec<&str> = stats_str
            .lines()
            .filter(|line| {
                !line.is_empty()
                    && !line.ends_with("searched")
                    && !line.ends_with("matches")
                    && !line.ends_with("matched lines")
                    && !line.ends_with("contained matches")
                    && !line.contains("seconds")
                    && !line.contains("millis")
            })
            .collect();
        assert_eq!(
            non_stats_lines.len(),
            1,
            "must emit exactly 1 entry before stats"
        );

        let out_zero = std::process::Command::new(grx_bin)
            .args(["kind:file", "txt", "root:disc_limits/", "head:0"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_zero.status.code(), Some(1));
        assert!(out_zero.stdout.is_empty());
    }

    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let non_utf8_dir = tmp_path.join("non_utf8");
        fs::create_dir_all(&non_utf8_dir).unwrap();
        let bad_path = non_utf8_dir.join(OsStr::from_bytes(b"bad-\xff.txt"));
        fs::write(&bad_path, b"content").unwrap();

        let out_raw = std::process::Command::new(grx_bin)
            .args(["kind:file", "bad", "root:non_utf8/"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_raw.status.code(), Some(0));
        assert!(
            out_raw
                .stdout
                .windows(b"bad-\xff.txt".len())
                .any(|w| w == b"bad-\xff.txt")
        );
        assert!(
            !out_raw
                .stdout
                .windows(b"bad-\xef\xbf\xbd.txt".len())
                .any(|w| w == b"bad-\xef\xbf\xbd.txt")
        );
    }

    #[cfg(unix)]
    {
        let broken_link = tmp_path.join("broken_link");
        let _ = fs::remove_file(&broken_link);
        std::os::unix::fs::symlink("nonexistent_target", &broken_link).unwrap();

        let out_broken = std::process::Command::new(grx_bin)
            .args(["kind:link", "root:broken_link"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_broken.status.code(), Some(0));
        let stderr = String::from_utf8_lossy(&out_broken.stderr);
        assert!(!stderr.contains("No such file or directory"));
        let stdout = String::from_utf8_lossy(&out_broken.stdout);
        assert!(stdout.contains("broken_link"));
    }

    {
        let _empty_pat = tmp_path.join("empty.patterns");
        let out_l_tail0 = std::process::Command::new(grx_bin)
            .args(["-L", "hit", "root:empty.patterns", "tail:0"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_l_tail0.status.code(), Some(1));
        assert!(out_l_tail0.stdout.is_empty());

        let out_l_tail1 = std::process::Command::new(grx_bin)
            .args(["-L", "hit", "root:empty.patterns", "tail:1"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_l_tail1.status.code(), Some(0));
        let lines_l = raw_lines(&out_l_tail1.stdout);
        assert_eq!(lines_l.len(), 1);
        assert!(lines_l[0].contains("empty.patterns"));
    }
}

#[test]
fn test_engine_corner_cases_and_limits() {
    let grx_bin = env!("CARGO_BIN_EXE_grx");
    let tmp = tempfile::tempdir().unwrap();
    let tmp_path = tmp.path();

    {
        let empty_pat = tmp_path.join("empty.pat");
        fs::write(&empty_pat, b"").unwrap();
        let yes_file = tmp_path.join("yes.txt");
        fs::write(&yes_file, b"hello world\n").unwrap();

        let out_existing = std::process::Command::new(grx_bin)
            .args(["-f", "empty.pat", "root:yes.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_existing.status.code(), Some(1));
        assert!(out_existing.stdout.is_empty());

        let out_missing = std::process::Command::new(grx_bin)
            .args(["-f", "empty.pat", "root:missing.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_missing.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&out_missing.stderr);
        assert!(
            stderr.contains("root:missing.txt") || stderr.contains("No such file or directory")
        );
    }

    {
        let out_v = std::process::Command::new(grx_bin)
            .args([
                "--no-heading",
                "-N",
                "-v",
                "-f",
                "empty.pat",
                "root:yes.txt",
            ])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_v.status.code(), Some(0));
        let text_v = String::from_utf8_lossy(&out_v.stdout);
        assert!(text_v.contains("hello world"));

        let out_cv = std::process::Command::new(grx_bin)
            .args(["-c", "-v", "-f", "empty.pat", "root:yes.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_cv.status.code(), Some(0));
        let text_cv = String::from_utf8_lossy(&out_cv.stdout);
        assert!(text_cv.contains('1'));
    }

    {
        let ctx_file = tmp_path.join("ctx_adjacent.txt");
        fs::write(
            &ctx_file,
            b"hit line 1\nhit line 2\nhit line 3\nnormal line 4\n",
        )
        .unwrap();

        let out_adjacent_head = std::process::Command::new(grx_bin)
            .args([
                "--no-heading",
                "-n",
                "-A",
                "1",
                "hit",
                "root:ctx_adjacent.txt",
                "head:1",
            ])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_adjacent_head.status.code(), Some(0));
        let text_head = String::from_utf8_lossy(&out_adjacent_head.stdout);
        assert!(text_head.contains("1:hit line 1"));
        assert!(text_head.contains("2-hit line 2"));
        assert!(!text_head.contains("3-hit line 3") && !text_head.contains("3:hit line 3"));

        let out_adjacent_tail = std::process::Command::new(grx_bin)
            .args([
                "--no-heading",
                "-n",
                "-B",
                "1",
                "hit",
                "root:ctx_adjacent.txt",
                "tail:1",
            ])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_adjacent_tail.status.code(), Some(0));
        let text_tail = String::from_utf8_lossy(&out_adjacent_tail.stdout);
        assert!(text_tail.contains("2-hit line 2"));
        assert!(text_tail.contains("3:hit line 3"));
        assert!(!text_tail.contains("1-hit line 1") && !text_tail.contains("1:hit line 1"));

        let out_overlap = std::process::Command::new(grx_bin)
            .args([
                "--no-heading",
                "-n",
                "-A",
                "3",
                "hit",
                "root:ctx_adjacent.txt",
                "head:1",
            ])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_overlap.status.code(), Some(0));
        let text_overlap = String::from_utf8_lossy(&out_overlap.stdout);
        assert!(text_overlap.contains("1:hit line 1"));
        assert!(text_overlap.contains("2-hit line 2"));
        assert!(text_overlap.contains("3-hit line 3"));
        assert!(text_overlap.contains("4-normal line 4"));
    }

    {
        let search_file = tmp_path.join("search_flags.txt");
        fs::write(&search_file, b"alpha beta\n").unwrap();

        let out_attached_m = std::process::Command::new(grx_bin)
            .args(["alpha", "-m1", "root:search_flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_attached_m.status.code(), Some(0));
        assert_eq!(raw_lines(&out_attached_m.stdout).len(), 1);

        let out_attached_c = std::process::Command::new(grx_bin)
            .args(["alpha", "-C2", "root:search_flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_attached_c.status.code(), Some(0));

        let out_eq_flag = std::process::Command::new(grx_bin)
            .args(["in:alpha", "-t=txt", "root:search_flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_eq_flag.status.code(), Some(0));

        let out_neg_namespace = std::process::Command::new(grx_bin)
            .args(["alpha", "ns:m1", "root:search_flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_neg_namespace.status.code(), Some(0));

        let out_neg_at = std::process::Command::new(grx_bin)
            .args(["alpha", "ns:@m1", "root:search_flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_neg_at.status.code(), Some(2));

        let out_neg_literal = std::process::Command::new(grx_bin)
            .args(["alpha", "-@m1", "root:search_flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_neg_literal.status.code(), Some(2));
    }

    #[cfg(unix)]
    {
        let sym_dir = tmp_path.join("sym_depth_dir");
        fs::create_dir_all(&sym_dir).unwrap();
        let target_file = sym_dir.join("target.txt");
        fs::write(&target_file, b"content").unwrap();
        let link_file = sym_dir.join("link.txt");
        let _ = fs::remove_file(&link_file);
        std::os::unix::fs::symlink("target.txt", &link_file).unwrap();

        let out_sym_d0 = std::process::Command::new(grx_bin)
            .args(["d:0", "root:sym_depth_dir/"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_sym_d0.status.code(), Some(0));
        let stdout_sym = String::from_utf8_lossy(&out_sym_d0.stdout);
        assert!(
            stdout_sym.contains("link.txt"),
            "d:0 must include root file symlink link.txt"
        );
        assert!(
            stdout_sym.contains("target.txt"),
            "d:0 must include target.txt"
        );
    }

    {
        let f1 = tmp_path.join("nomatch1.txt");
        let f2 = tmp_path.join("nomatch2.txt");
        fs::write(&f1, b"hello\n").unwrap();
        fs::write(&f2, b"world\n").unwrap();

        let out_l_stats = std::process::Command::new(grx_bin)
            .args([
                "-L",
                "--stats",
                "missing_term",
                "root:nomatch1.txt",
                "root:nomatch2.txt",
                "sort:path",
                "head:1",
            ])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_l_stats.status.code(), Some(0));
        let stats_output = String::from_utf8_lossy(&out_l_stats.stdout);

        let non_stats_lines: Vec<&str> = stats_output
            .lines()
            .filter(|line| {
                !line.is_empty()
                    && !line.ends_with("searched")
                    && !line.ends_with("matches")
                    && !line.ends_with("matched lines")
                    && !line.ends_with("contained matches")
                    && !line.contains("seconds")
                    && !line.contains("millis")
            })
            .collect();
        assert_eq!(non_stats_lines.len(), 1, "must emit exactly 1 file name");
        assert_eq!(non_stats_lines[0], "nomatch1.txt");

        assert!(stats_output.contains("0 matches"), "must report 0 matches");
        assert!(
            stats_output.contains("0 matched lines"),
            "must report 0 matched lines"
        );
        assert!(
            stats_output.contains("0 files contained matches"),
            "must report 0 files contained matches"
        );
        assert!(
            stats_output.contains("1 files searched"),
            "head:1 stops after the first eligible nonmatching file; stats report actual work"
        );
    }
}

#[test]
fn test_sort_column_formatting_and_completion_generation() {
    let grx_bin = env!("CARGO_BIN_EXE_grx");
    let tmp = tempfile::tempdir().unwrap();
    let tmp_path = tmp.path();

    let sample_file = tmp_path.join("sample.txt");
    fs::write(&sample_file, b"sample content of 26 bytes\n").unwrap();

    let out_mod = std::process::Command::new(grx_bin)
        .args(["root:sample.txt", "sort:modified"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_mod.status.code(), Some(0));
    let stdout_mod = String::from_utf8_lossy(&out_mod.stdout);
    let mod_line = stdout_mod.lines().next().expect("expected output line");
    assert!(
        mod_line.len() >= 14,
        "line should be at least 14 chars for 12-char date + 2 spaces"
    );
    assert_eq!(
        &mod_line[12..14],
        "  ",
        "12-char date column must be followed by two spaces"
    );
    assert!(
        mod_line.ends_with("sample.txt"),
        "line must end with file path"
    );

    let out_size = std::process::Command::new(grx_bin)
        .args(["root:sample.txt", "sort:size"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_size.status.code(), Some(0));
    let stdout_size = String::from_utf8_lossy(&out_size.stdout);
    let size_line = stdout_size.lines().next().expect("expected output line");
    assert_eq!(
        size_line, "   27  sample.txt",
        "27 byte file must format as 5-char right-aligned size '   27' followed by 2 spaces"
    );

    let out_null = std::process::Command::new(grx_bin)
        .args(["-0", "root:sample.txt", "sort:size"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_null.status.code(), Some(0));
    assert_eq!(
        out_null.stdout, b"sample.txt\0",
        "null mode must emit pure null-terminated paths without sort columns"
    );

    let out_unsorted = std::process::Command::new(grx_bin)
        .args(["root:sample.txt"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_unsorted.status.code(), Some(0));
    let stdout_unsorted = String::from_utf8_lossy(&out_unsorted.stdout);
    assert_eq!(
        stdout_unsorted.trim(),
        "sample.txt",
        "unsorted search should emit bare path without columns"
    );

    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let comp_dir = manifest_dir.join("completions");

    let fish_path = comp_dir.join("grx.fish");
    let bash_path = comp_dir.join("grx.bash");
    let zsh_path = comp_dir.join("_grx");

    assert!(fish_path.exists(), "completions/grx.fish must exist");
    assert!(bash_path.exists(), "completions/grx.bash must exist");
    assert!(zsh_path.exists(), "completions/_grx must exist");

    let fish_content = fs::read_to_string(&fish_path).unwrap();
    assert!(fish_content.contains("sort:"));
    assert!(fish_content.contains("sort:"));
    assert!(fish_content.contains("-l sort"));

    let bash_content = fs::read_to_string(&bash_path).unwrap();
    assert!(bash_content.contains("--sort"));
    assert!(bash_content.contains("sort:*"));

    let zsh_content = fs::read_to_string(&zsh_path).unwrap();
    assert!(zsh_content.contains("--sort"));
    assert!(zsh_content.contains("size bytes"));

    let out_bad_shell = std::process::Command::new(grx_bin)
        .args(["--completions", "powershell"])
        .output()
        .unwrap();
    assert_eq!(out_bad_shell.status.code(), Some(2));
    let err_str = String::from_utf8_lossy(&out_bad_shell.stderr);
    assert!(err_str.contains("Unsupported shell 'powershell'"));
    assert!(err_str.contains("Supported shells: fish, bash, zsh"));
}

#[test]
fn test_config_binary_handling_and_buffer_size_wiring() {
    let grx_bin = env!("CARGO_BIN_EXE_grx");
    let tmp = tempfile::tempdir().unwrap();
    let tmp_path = tmp.path();

    let bin_file = tmp_path.join("data.bin");
    let mut bin_data = b"preamble \x00 magic_token \x00 postscript\n".to_vec();
    bin_data.extend(vec![0u8; 100]);
    fs::write(&bin_file, &bin_data).unwrap();

    let out_default = std::process::Command::new(grx_bin)
        .args([
            "--hidden",
            "magic_token",
            &format!("root:{}", tmp_path.to_str().unwrap()),
        ])
        .output()
        .unwrap();
    assert_eq!(
        out_default.status.code(),
        Some(1),
        "Default should skip binary file"
    );

    let config_file = tmp_path.join("custom_config.toml");
    let config_toml = r#"
[search]
binary-handling = "search"
buffer-size-bytes = 32768
"#;
    fs::write(&config_file, config_toml).unwrap();

    let out_search = std::process::Command::new(grx_bin)
        .args([
            "--hidden",
            "--config",
            config_file.to_str().unwrap(),
            "magic_token",
            &format!("root:{}", tmp_path.to_str().unwrap()),
        ])
        .output()
        .unwrap();
    assert_eq!(
        out_search.status.code(),
        Some(0),
        "Configured binary-handling = 'search' should search binary file"
    );

    let probe_file = tmp_path.join("probe_test.txt");
    let mut probe_data = vec![b'a'; 100];
    probe_data.push(0x00);
    probe_data.extend_from_slice(b" probe_token\n");
    fs::write(&probe_file, &probe_data).unwrap();

    let probe_config_file = tmp_path.join("probe_config.toml");
    let probe_config_toml = r#"
[search]
binary-null-probe-bytes = 50
walker-buffer-size-bytes = 32768
"#;
    fs::write(&probe_config_file, probe_config_toml).unwrap();

    let out_probe = std::process::Command::new(grx_bin)
        .args([
            "--hidden",
            "--config",
            probe_config_file.to_str().unwrap(),
            "probe_token",
            &format!("root:{}", probe_file.to_str().unwrap()),
        ])
        .output()
        .unwrap();
    assert_eq!(
        out_probe.status.code(),
        Some(0),
        "With probe length 50, null byte at offset 100 is not detected in probe, so file is searched as text"
    );
}

#[test]
#[cfg(unix)]
fn test_traversal_reports_unreadable_subdirectories() {
    use std::os::unix::fs::PermissionsExt;
    let grx_bin = env!("CARGO_BIN_EXE_grx");
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let readable_dir = root.join("readable");
    fs::create_dir(&readable_dir).unwrap();
    let hit_file = readable_dir.join("hit.txt");
    fs::write(&hit_file, b"content_token_here\n").unwrap();

    let unreadable_dir = root.join("unreadable");
    fs::create_dir(&unreadable_dir).unwrap();
    fs::write(unreadable_dir.join("secret.txt"), b"restricted content\n").unwrap();
    let mut perms = fs::metadata(&unreadable_dir).unwrap().permissions();
    perms.set_mode(0o000);
    let _ = fs::set_permissions(&unreadable_dir, perms);

    let out = std::process::Command::new(grx_bin)
        .args([
            "--hidden",
            "content_token_here",
            &format!("root:{}", root.to_str().unwrap()),
        ])
        .output()
        .expect("Failed to execute grx binary");

    let access_denied = fs::read_dir(&unreadable_dir).is_err();
    let mut restore_perms = fs::metadata(&unreadable_dir).unwrap().permissions();
    restore_perms.set_mode(0o755);
    let _ = fs::set_permissions(&unreadable_dir, restore_perms);

    assert_eq!(
        out.status.code(),
        Some(if access_denied { 2 } else { 0 }),
        "Search must report incomplete traversal"
    );
    if access_denied {
        assert!(!out.stderr.is_empty());
    } else {
        assert!(String::from_utf8_lossy(&out.stdout).contains("content_token_here"));
    }
}

#[test]
fn test_discovery_positional_targets_and_flags() {
    let grx_bin = env!("CARGO_BIN_EXE_grx");
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let sub = root.join("mysubdir");
    fs::create_dir(&sub).unwrap();
    let rs_file = sub.join("special_target.rs");
    fs::write(&rs_file, b"fn main() {}\n").unwrap();
    let txt_file = sub.join("notes.txt");
    fs::write(&txt_file, b"random notes\n").unwrap();

    let out_in = std::process::Command::new(grx_bin)
        .args([
            "--hidden",
            "kind:file",
            "special",
            &format!("root:{}", sub.to_str().unwrap()),
        ])
        .output()
        .unwrap();
    assert_eq!(out_in.status.code(), Some(0));
    let stdout_in = String::from_utf8_lossy(&out_in.stdout);
    assert!(stdout_in.contains("special_target.rs"));

    let out_t = std::process::Command::new(grx_bin)
        .args([
            "--hidden",
            "-t",
            "rs",
            &format!("root:{}", sub.to_str().unwrap()),
        ])
        .output()
        .unwrap();
    assert_eq!(out_t.status.code(), Some(0));
    let stdout_t = String::from_utf8_lossy(&out_t.stdout);
    assert!(stdout_t.contains("special_target.rs"));
    assert!(!stdout_t.contains("notes.txt"));

    let out_g = std::process::Command::new(grx_bin)
        .args([
            "--hidden",
            "-g",
            "*target*",
            &format!("root:{}", sub.to_str().unwrap()),
        ])
        .output()
        .unwrap();
    assert_eq!(out_g.status.code(), Some(0));
    let stdout_g = String::from_utf8_lossy(&out_g.stdout);
    assert!(stdout_g.contains("special_target.rs"));
    assert!(!stdout_g.contains("notes.txt"));
}

#[test]
fn test_streaming_inverted_search_match_counts_in_stats() {
    let grx_bin = env!("CARGO_BIN_EXE_grx");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let inv_file = root.join("inverted.txt");
    fs::write(&inv_file, b"match1\nskip\nmatch2\nskip\nmatch3\n").unwrap();

    let out_stats = std::process::Command::new(grx_bin)
        .args([
            "--hidden",
            "-v",
            "--stats",
            "skip",
            &format!("root:{}", inv_file.to_str().unwrap()),
        ])
        .output()
        .unwrap();
    assert_eq!(out_stats.status.code(), Some(0));
    let stdout_stats = String::from_utf8_lossy(&out_stats.stdout);
    assert!(
        stdout_stats.contains("3 matches"),
        "Inverted search should report 3 matches in stats: {stdout_stats}"
    );
}

#[test]
#[cfg(unix)]
fn test_positional_file_target_symlinks() {
    let grx_bin = env!("CARGO_BIN_EXE_grx");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let target_file = root.join("target_file.txt");
    fs::write(&target_file, b"find_me_in_symlink\n").unwrap();
    let link_file = root.join("link_to_file.txt");
    std::os::unix::fs::symlink(&target_file, &link_file).unwrap();
    let out_link = std::process::Command::new(grx_bin)
        .args([
            "--hidden",
            "find_me_in_symlink",
            &format!("root:{}", link_file.to_str().unwrap()),
        ])
        .output()
        .unwrap();
    assert_eq!(out_link.status.code(), Some(0));
    let stdout_link = String::from_utf8_lossy(&out_link.stdout);
    assert!(stdout_link.contains("find_me_in_symlink"));
}

#[test]
fn test_gitignore_slashless_glob_in_subdirectories() {
    let grx_bin = env!("CARGO_BIN_EXE_grx");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let git_dir = root.join("repo");
    fs::create_dir(&git_dir).unwrap();
    let gitignore = git_dir.join(".gitignore");
    fs::write(&gitignore, b"debug_*.txt\n").unwrap();
    let sub = git_dir.join("sub");
    fs::create_dir(&sub).unwrap();
    let sub_ignored = sub.join("debug_1.txt");
    fs::write(&sub_ignored, b"ignored debug content\n").unwrap();
    let sub_kept = sub.join("other.txt");
    fs::write(&sub_kept, b"kept content\n").unwrap();

    let out_glob = std::process::Command::new(grx_bin)
        .args([
            "--hidden",
            "content",
            &format!("root:{}", git_dir.to_str().unwrap()),
        ])
        .output()
        .unwrap();
    let stdout_glob = String::from_utf8_lossy(&out_glob.stdout);
    assert!(!stdout_glob.contains("debug_1.txt"));
    assert!(stdout_glob.contains("other.txt"));
}

#[test]
fn test_dot_ignore_precedence_over_gitignore() {
    let grx_bin = env!("CARGO_BIN_EXE_grx");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let git_dir = root.join("repo");
    fs::create_dir(&git_dir).unwrap();
    let gitignore = git_dir.join(".gitignore");
    fs::write(&gitignore, b"debug_*.txt\n").unwrap();
    let sub = git_dir.join("sub");
    fs::create_dir(&sub).unwrap();
    let sub_ignored = sub.join("debug_1.txt");
    fs::write(&sub_ignored, b"ignored debug content\n").unwrap();
    let sub_kept = sub.join("other.txt");
    fs::write(&sub_kept, b"kept content\n").unwrap();

    let dot_ignore = git_dir.join(".ignore");
    fs::write(&dot_ignore, b"!debug_*.txt\n").unwrap();
    let out_prec = std::process::Command::new(grx_bin)
        .args([
            "--hidden",
            "content",
            &format!("root:{}", git_dir.to_str().unwrap()),
        ])
        .output()
        .unwrap();
    let stdout_prec = String::from_utf8_lossy(&out_prec.stdout);
    assert!(
        stdout_prec.contains("debug_1.txt"),
        ".ignore should override .gitignore to un-ignore debug_1.txt"
    );
}

#[test]
fn test_dsl_double_colon_prefix_and_exclamation_dir_exclusion() {
    let tmp = tempdir().unwrap();
    fs::create_dir(tmp.path().join("sub")).unwrap();
    fs::write(
        tmp.path().join("main.rs"),
        "use str::from_utf8;\nkept content\n",
    )
    .unwrap();
    fs::write(tmp.path().join("sub/other.rs"), "excluded content\n").unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["root:.", "--color=never"])
            .args(args)
            .output()
            .unwrap()
    };
    assert_eq!(run(&["str::from_utf8"]).status.code(), Some(2));
    let literal = run(&["lit:str::from_utf8"]);
    assert_eq!(literal.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&literal.stdout).contains("use str::from_utf8"));
    let exclude = run(&["content", "np:sub/"]);
    assert_eq!(exclude.status.code(), Some(0));
    let text = String::from_utf8_lossy(&exclude.stdout);
    assert!(text.contains("kept content"));
    assert!(!text.contains("excluded content"));
    assert_eq!(run(&["content", "!sub/"]).status.code(), Some(2));
}

#[test]
fn test_binary_string_head_limits_logical_records() {
    let tmp = tempdir().unwrap();
    let file = tmp.path().join("strings.bin");
    fs::write(&file, b"hit-one\0hit-two\0hit-three\0").unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "--no-heading",
            "--color",
            "never",
            "hit",
            "str:4",
            "head:2",
            &format!("root:{}", file.to_str().unwrap()),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hit-one"));
    assert!(stdout.contains("hit-two"));
    assert!(!stdout.contains("hit-three"));
    assert_eq!(stdout.matches("hit-").count(), 2);
}

#[test]
fn test_proximity_accepts_guards_after_anchor_at_exact_windows() {
    let tmp = tempdir().unwrap();
    let file = tmp.path().join("forward-proximity.txt");
    fs::write(
        &file,
        b"target_two\nfiller\nGUARD_TWO\ntarget_three\nfiller\nfiller\nGUARD_THREE\n",
    )
    .unwrap();

    for (pattern, filter) in [
        ("target_two", "near:2,GUARD_TWO"),
        ("target_three", "near:3,GUARD_THREE"),
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .args([
                "--hidden",
                "-q",
                pattern,
                filter,
                &format!("root:{}", file.to_str().unwrap()),
            ])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "forward proximity failed for {filter}"
        );
    }
}

#[test]
fn test_fuzzy_permutations_reject_partial_token_lines() {
    let tmp = tempdir().unwrap();
    let file = tmp.path().join("tokens.txt");
    fs::write(
        &file,
        "alpha beta gamma\ngamma alpha beta\nalpha beta\ngamma\nunrelated\n",
    )
    .unwrap();
    let root = format!("root:{}", file.display());
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .args(["--hidden", "--no-heading", "--color=never"])
            .args(args)
            .arg(&root)
            .output()
            .unwrap()
    };
    let all = run(&["alpha", "AND", "beta", "AND", "gamma"]);
    assert_eq!(all.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&all.stdout).lines().count(), 2);
    let text = String::from_utf8_lossy(&all.stdout);
    assert!(text.contains("alpha beta gamma"));
    assert!(text.contains("gamma alpha beta"));
    let ordered = run(&["fz:gamalphbet"]);
    assert_eq!(ordered.status.code(), Some(0));
    let text = String::from_utf8_lossy(&ordered.stdout);
    assert!(text.contains("gamma alpha beta"));
    assert!(!text.contains("alpha beta gamma"));
    assert_eq!(run(&["fz:alpha,beta,gamma"]).status.code(), Some(1));
}

#[test]
fn test_boolean_or_executes_for_dsl_and_repeated_regexp_flags() {
    let tmp = tempdir().unwrap();
    let file = tmp.path().join("boolean-or.txt");
    fs::write(&file, b"alpha only\nbeta only\ngamma only\n").unwrap();

    let root_arg = format!("root:{}", file.display());
    for args in [
        vec!["alpha", "OR", "beta", &root_arg],
        vec!["-e", "alpha", "-e", "beta", &root_arg],
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .args(["--no-heading", "--color", "never"])
            .args(args)
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(0));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("alpha only"));
        assert!(stdout.contains("beta only"));
        assert!(!stdout.contains("gamma only"));
    }
}

#[test]
fn test_inverted_search_returns_one_when_every_line_matches() {
    let tmp = tempdir().unwrap();
    let file = tmp.path().join("all-match.txt");
    fs::write(&file, b"needle one\nneedle two\nneedle three\n").unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--hidden",
            "-v",
            "needle",
            &format!("root:{}", file.to_str().unwrap()),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn test_explicit_config_path_failures_exit_with_diagnostics() {
    let tmp = tempdir().unwrap();
    let missing = tmp.path().join("missing.toml");
    let invalid = tmp.path().join("invalid.toml");
    fs::write(&invalid, b"[search\n").unwrap();

    for (path, diagnostic) in [
        (&missing, "failed to read config"),
        (&invalid, "failed to parse config"),
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .args(["--config", path.to_str().unwrap()])
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(diagnostic), "unexpected stderr: {stderr}");
        assert!(stderr.contains(path.file_name().unwrap().to_str().unwrap()));
    }
}

#[test]
fn test_prefix_typo_diagnostics_and_help_synchronization() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("sample.rs"), "let x = \"ext:rs\";\n").unwrap();
    let run = |term: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["root:.", "--color=never", term])
            .output()
            .unwrap()
    };
    let typo = run("ext:rs");
    assert_eq!(typo.status.code(), Some(2));
    let error = String::from_utf8_lossy(&typo.stderr);
    assert!(error.contains("unknown prefix 'ext:'"));
    assert!(error.contains("lit:ext:rs"));
    let escaped = run("lit:ext:rs");
    assert_eq!(escaped.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&escaped.stdout).contains("let x = \"ext:rs\";"));
    let help = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .arg("--help")
        .output()
        .unwrap();
    assert_eq!(help.status.code(), Some(0));
    let text = String::from_utf8_lossy(&help.stdout);
    for syntax in grx::dsl::DSL_SYNTAX {
        assert!(
            text.contains(&format!("{}:", syntax.prefix)),
            "missing {} in help",
            syntax.prefix
        );
    }
    assert!(!text.contains("/regex/, re:<regex>"));
}

#[test]
fn test_inline_actions_mv_and_undo() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let data_home = root.join(".data");

    let d1 = root.join("hellocpp");
    let d2 = root.join("xmake_cpp_learn");
    let d3 = root.join("cmake-cpp-learn");
    let dest = root.join("cpp-projects");

    fs::create_dir(&d1).unwrap();
    fs::create_dir(&d2).unwrap();
    fs::create_dir(&d3).unwrap();
    fs::create_dir(&dest).unwrap();
    fs::write(d1.join("hello.cpp"), b"// hello").unwrap();

    let status = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["kind:dir", "cpp", "d:1", "mv:cpp-projects/"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));

    assert!(!d1.exists());
    assert!(!d2.exists());
    assert!(!d3.exists());
    assert!(dest.join("hellocpp").exists());
    assert!(dest.join("hellocpp/hello.cpp").exists());
    assert!(dest.join("xmake_cpp_learn").exists());
    assert!(dest.join("cmake-cpp-learn").exists());

    assert!(dest.exists());

    let undo_status = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .arg("undo")
        .status()
        .unwrap();
    assert_eq!(undo_status.code(), Some(0));

    assert!(d1.exists());
    assert!(d1.join("hello.cpp").exists());
    assert!(d2.exists());
    assert!(d3.exists());
    assert!(!dest.join("hellocpp").exists());
}

#[test]
fn test_inline_actions_cp_dry_run_and_undo() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let data_home = root.join(".data");

    let f1 = root.join("test1.txt");
    let f2 = root.join("test2.txt");
    let backup = root.join("backup");

    fs::write(&f1, b"content 1").unwrap();
    fs::write(&f2, b"content 2").unwrap();

    let dry_output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["kind:file", "test", "t:txt", "dry:", "cp:backup/"])
        .output()
        .unwrap();
    assert_eq!(dry_output.status.code(), Some(0));
    assert!(!backup.exists());

    let cp_status = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["kind:file", "test", "t:txt", "cp:backup/"])
        .status()
        .unwrap();
    assert_eq!(cp_status.code(), Some(0));
    assert!(f1.exists());
    assert!(f2.exists());
    assert!(backup.join("test1.txt").exists());
    assert!(backup.join("test2.txt").exists());

    let undo_status = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .arg("undo")
        .status()
        .unwrap();
    assert_eq!(undo_status.code(), Some(0));
    assert!(f1.exists());
    assert!(f2.exists());
    assert!(!backup.join("test1.txt").exists());
    assert!(!backup.join("test2.txt").exists());
}

#[test]
fn test_inline_actions_rm_and_undo() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let data_home = root.join(".data");

    let f = root.join("trashme.log");
    fs::write(&f, b"secret data to recover").unwrap();

    let rm_status = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["kind:file", "trashme", "trash:"])
        .status()
        .unwrap();
    assert_eq!(rm_status.code(), Some(0));
    assert!(!f.exists());

    let undo_status = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .arg("undo")
        .status()
        .unwrap();
    assert_eq!(undo_status.code(), Some(0));
    assert!(f.exists());
    assert_eq!(fs::read(&f).unwrap(), b"secret data to recover");

    let deprecated_rm_status = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .args(["kind:file", "trashme", "rm:"])
        .status()
        .unwrap();
    assert_eq!(deprecated_rm_status.code(), Some(2));
}

#[test]
#[cfg(unix)]
fn test_exec_and_exec_batch_flags() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    let f1 = root.join("file_one.txt");
    let f2 = root.join("file_two.txt");
    fs::write(&f1, b"1").unwrap();
    fs::write(&f2, b"2").unwrap();

    let exec_output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .args(["kind:file", "t:txt", "--exec", "touch", "{}.created"])
        .output()
        .unwrap();
    assert_eq!(exec_output.status.code(), Some(0));
    assert!(root.join("file_one.txt.created").exists());
    assert!(root.join("file_two.txt.created").exists());

    let batch_status = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .args(["kind:file", "created", "-X", "rm"])
        .status()
        .unwrap();
    assert_eq!(batch_status.code(), Some(0));
    assert!(!root.join("file_one.txt.created").exists());
    assert!(!root.join("file_two.txt.created").exists());
}

#[test]
fn test_content_action_safety_requires_files_with_matches() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let data_home = root.join(".data");
    let file = root.join("notes.txt");
    fs::write(&file, b"TODO: fix this later\n").unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["TODO", "trash:", "root:."])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot be run directly on content search matches without -l"));
    assert!(file.exists());

    let output_l = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["-l", "TODO", "trash:", "root:."])
        .output()
        .unwrap();
    assert_eq!(output_l.status.code(), Some(0));
    assert!(!file.exists());
}

#[test]
fn test_ops_wal_disk_roundtrip_multi_transaction() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let data_home = root.join(".data");
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let file_a = root.join("alpha.txt");
    let file_b = root.join("beta.txt");
    fs::write(&file_a, b"alpha content\n").unwrap();
    fs::write(&file_b, b"beta content\n").unwrap();

    let sub1 = root.join("sub1");
    let sub2 = root.join("sub2");
    fs::create_dir_all(&sub1).unwrap();
    fs::create_dir_all(&sub2).unwrap();

    let out_tx1 = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["kind:file", "alpha.txt", "mv:sub1/"])
        .output()
        .unwrap();
    assert_eq!(out_tx1.status.code(), Some(0));
    assert!(!file_a.exists());
    assert!(sub1.join("alpha.txt").exists());

    let stdout1 = String::from_utf8_lossy(&out_tx1.stdout);
    let tx1_id = stdout1
        .split("id: ")
        .nth(1)
        .and_then(|s| s.split(')').next())
        .expect("Should capture transaction 1 ID")
        .to_string();

    let out_tx2 = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["kind:file", "beta.txt", "cp:sub2/"])
        .output()
        .unwrap();
    assert_eq!(out_tx2.status.code(), Some(0));
    assert!(file_b.exists());
    assert!(sub2.join("beta.txt").exists());

    let out_list = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["undo", "--list"])
        .output()
        .unwrap();
    assert_eq!(out_list.status.code(), Some(0));
    let list_stdout = String::from_utf8_lossy(&out_list.stdout);
    assert!(list_stdout.contains(&tx1_id));
    assert!(list_stdout.contains("Moved"));
    assert!(list_stdout.contains("Copied"));

    let out_undo1 = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["undo", &tx1_id])
        .output()
        .unwrap();
    assert_eq!(out_undo1.status.code(), Some(0));
    assert!(file_a.exists());
    assert!(!sub1.join("alpha.txt").exists());

    assert!(sub2.join("beta.txt").exists());

    let out_undo2 = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["undo"])
        .output()
        .unwrap();
    assert_eq!(out_undo2.status.code(), Some(0));
    assert!(!sub2.join("beta.txt").exists());
    assert!(file_b.exists());
}

#[test]
fn test_ops_force_copy_undo_restores_overwritten_backup() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let data_home = root.join(".data");
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let src_file = root.join("file1.txt");
    fs::write(&src_file, b"source new content\n").unwrap();

    let backup_dir = root.join("backup");
    fs::create_dir_all(&backup_dir).unwrap();
    let dst_file = backup_dir.join("file1.txt");
    fs::write(&dst_file, b"pre-existing old content\n").unwrap();

    let out_fail = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["kind:file", "file1.txt", "np:backup/", "cp:backup/"])
        .output()
        .unwrap();
    assert_eq!(out_fail.status.code(), Some(2));
    assert_eq!(fs::read(&dst_file).unwrap(), b"pre-existing old content\n");

    let out_force = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args([
            "--force",
            "kind:file",
            "file1.txt",
            "np:backup/",
            "cp:backup/",
        ])
        .output()
        .unwrap();
    assert_eq!(out_force.status.code(), Some(0));
    assert_eq!(fs::read(&dst_file).unwrap(), b"source new content\n");

    let out_undo = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["undo"])
        .output()
        .unwrap();
    assert_eq!(out_undo.status.code(), Some(0));
    assert_eq!(fs::read(&dst_file).unwrap(), b"pre-existing old content\n");
}

#[test]
fn test_search_boolean_ascii_case_insensitive_spans() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let test_file = root.join("sample.txt");
    fs::write(&test_file, b"AlphaBeta and GAMMA_DELTA found here\n").unwrap();

    let out = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args([
            "-i",
            "--json",
            "alphabeta",
            "AND",
            "gamma_delta",
            "root:sample.txt",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);

    let match_line = stdout
        .lines()
        .find(|l| l.contains("\"type\":\"match\""))
        .expect("Expected match line in JSON stream");

    assert!(match_line.contains("\"match\":{\"text\":\"AlphaBeta\"}"));
    assert!(match_line.contains("\"match\":{\"text\":\"GAMMA_DELTA\"}"));
    assert!(match_line.contains("\"start\":0") && match_line.contains("\"end\":9"));
    assert!(match_line.contains("\"start\":14") && match_line.contains("\"end\":25"));
}

#[test]
fn test_cli_action_flags_move_copy_trash_and_clean_trash() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let data_home = root.join(".data");
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let f1 = root.join("file1.txt");
    let f2 = root.join("file2.txt");
    let dest_dir = root.join("moved_files");
    let copy_dir = root.join("copied_files");

    fs::write(&f1, b"needle in haystack\n").unwrap();
    fs::write(&f2, b"other text\n").unwrap();

    let conflict_out = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["--move", "d1/", "--copy", "d2/", "needle"])
        .output()
        .unwrap();
    assert_eq!(conflict_out.status.code(), Some(2));
    let err_str = String::from_utf8_lossy(&conflict_out.stderr);
    assert!(err_str.contains("Conflicting") && err_str.contains("action"));

    let cp_status = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args([
            "--copy",
            copy_dir.to_str().unwrap(),
            "-l",
            "needle",
            "root:.",
        ])
        .status()
        .unwrap();
    assert_eq!(cp_status.code(), Some(0));
    assert!(f1.exists());
    assert!(copy_dir.join("file1.txt").exists());

    let mv_status = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args([
            "--move",
            dest_dir.to_str().unwrap(),
            "-l",
            "needle",
            "root:file1.txt",
        ])
        .status()
        .unwrap();
    assert_eq!(mv_status.code(), Some(0));
    assert!(!f1.exists());
    assert!(dest_dir.join("file1.txt").exists());

    let undo_status = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .arg("undo")
        .status()
        .unwrap();
    assert_eq!(undo_status.code(), Some(0));
    assert!(f1.exists());
    assert!(!dest_dir.join("file1.txt").exists());

    let trash_status = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["--trash", "-l", "needle", "root:."])
        .status()
        .unwrap();
    assert_eq!(trash_status.code(), Some(0));
    assert!(!f1.exists());

    let clean_out = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .arg("--clean-trash")
        .output()
        .unwrap();
    assert_eq!(clean_out.status.code(), Some(0));
    let clean_str = String::from_utf8_lossy(&clean_out.stdout);
    assert!(clean_str.contains("successfully purged"));
}

#[test]
fn test_kind_bin_and_bin_mode_discovery_and_content() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let text_file = root.join("hello.txt");
    fs::write(&text_file, b"sample text with needle\n").unwrap();

    let bin_file = root.join("binary.dat");
    fs::write(&bin_file, b"\x7fELF\0sample binary with needle\0\xff").unwrap();

    let out_kind_bin = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["kind:bin", "root:."])
        .output()
        .unwrap();
    assert_eq!(out_kind_bin.status.code(), Some(0));
    let stdout_bin = String::from_utf8_lossy(&out_kind_bin.stdout);
    assert!(stdout_bin.contains("binary.dat"));
    assert!(!stdout_bin.contains("hello.txt"));

    let out_bin_shorthand = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["bin:", "root:."])
        .output()
        .unwrap();
    assert_eq!(out_bin_shorthand.status.code(), Some(2));
    let err_shorthand = String::from_utf8_lossy(&out_bin_shorthand.stderr);
    assert!(err_shorthand.contains("unknown prefix \'bin:\'"));

    let out_kind_text = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["kind:text", "root:."])
        .output()
        .unwrap();
    assert_eq!(out_kind_text.status.code(), Some(0));
    let stdout_text = String::from_utf8_lossy(&out_kind_text.stdout);
    assert!(stdout_text.contains("hello.txt"));
    assert!(!stdout_text.contains("binary.dat"));

    let out_search_bin = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["--no-heading", "-l", "-e", "needle", "kind:bin", "root:."])
        .output()
        .unwrap();
    assert_eq!(out_search_bin.status.code(), Some(0));
    let search_bin_str = String::from_utf8_lossy(&out_search_bin.stdout);
    assert!(search_bin_str.contains("binary.dat"));
    assert!(!search_bin_str.contains("hello.txt"));

    let out_search_bin_shorthand = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["--no-heading", "-l", "needle", "bin:", "root:."])
        .output()
        .unwrap();
    assert_eq!(out_search_bin_shorthand.status.code(), Some(2));
    let search_err = String::from_utf8_lossy(&out_search_bin_shorthand.stderr);
    assert!(search_err.contains("unknown prefix \'bin:\'"));

    let out_search_text = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["--no-heading", "-l", "-e", "needle", "kind:text", "root:."])
        .output()
        .unwrap();
    assert_eq!(out_search_text.status.code(), Some(0));
    let search_text_str = String::from_utf8_lossy(&out_search_text.stdout);
    assert!(search_text_str.contains("hello.txt"));
    assert!(!search_text_str.contains("binary.dat"));

    let code_file = root.join("code.rs");
    fs::write(&code_file, b"let bin = 42;\nlet val = 99;\n").unwrap();

    let out_bin = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["--no-heading", "bin", "root:."])
        .output()
        .unwrap();
    assert_eq!(out_bin.status.code(), Some(0));
    let bin_str = String::from_utf8_lossy(&out_bin.stdout);
    assert!(bin_str.contains("let bin = 42;"));
    assert!(!bin_str.contains("let val = 99;"));

    let out_plus_bin = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["--no-heading", "+bin", "root:."])
        .output()
        .unwrap();
    assert_eq!(out_plus_bin.status.code(), Some(2));
}

#[test]
fn test_tilde_path_resolution_cli() {
    let grx_bin = env!("CARGO_BIN_EXE_grx");
    let mock_home = tempfile::tempdir().unwrap();
    let projects_dir = mock_home.path().join("Data").join("Projects");
    fs::create_dir_all(&projects_dir).unwrap();
    let target_file = projects_dir.join("sample.rs");
    fs::write(&target_file, b"pub fn main_fn() -> i32 { 42 }\n").unwrap();

    let out = std::process::Command::new(grx_bin)
        .env("HOME", mock_home.path())
        .args(["main..fn", "root:~/Data/Projects"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("main_fn"));

    let out_pat = std::process::Command::new(grx_bin)
        .env("HOME", mock_home.path())
        .current_dir(mock_home.path())
        .args(["~"])
        .output()
        .unwrap();
    assert_eq!(out_pat.status.code(), Some(1));
}

#[test]
fn test_hyphen_terms_parentheses_and_unix_disambiguation() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("flags.txt"),
        "CFLAGS = -Wall -Wextra\nLDFLAGS = -lm\nCSS = :root !important\n-Wall\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["root:.", "--no-heading", "--color=never"])
            .args(args)
            .output()
            .unwrap()
    };
    assert_eq!(run(&["-Wall"]).status.code(), Some(2));
    let literal = run(&["lit:-Wall"]);
    assert_eq!(literal.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&literal.stdout).contains("CFLAGS = -Wall -Wextra"));
    let exact = run(&["=-Wall"]);
    assert_eq!(exact.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&exact.stdout).lines().count(), 1);
    assert!(!String::from_utf8_lossy(&exact.stdout).contains("CFLAGS"));
    let grouped = run(&["(lit:-Wall OR lit:-lm)"]);
    assert_eq!(grouped.status.code(), Some(0));
    let text = String::from_utf8_lossy(&grouped.stdout);
    assert!(text.contains("CFLAGS"));
    assert!(text.contains("LDFLAGS"));
    assert!(!text.contains("CSS"));
    let excluded = run(&["NOT", "lit:-Wall"]);
    assert_eq!(excluded.status.code(), Some(0));
    let text = String::from_utf8_lossy(&excluded.stdout);
    assert!(!text.contains("CFLAGS"));
    assert!(text.contains("LDFLAGS"));
    let ns = run(&["LAGS", "ns:lit:-Wall"]);
    assert_eq!(ns.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&ns.stdout).contains("CFLAGS"));
    assert!(String::from_utf8_lossy(&ns.stdout).contains("LDFLAGS"));
    assert_eq!(run(&["ns:lit:-Wall"]).status.code(), Some(2));
    assert_eq!(run(&["--", "-Wall"]).stdout, literal.stdout);
    assert_eq!(run(&["@-Wall"]).status.code(), Some(2));
    for term in ["lit::root", "lit:!important"] {
        assert_eq!(run(&[term]).status.code(), Some(0));
    }
}

#[test]
fn test_cli_proximity_with_head_and_tail_slicing() {
    let tmp = tempdir().unwrap();
    let file1 = tmp.path().join("a.rs");
    let content1 = "\
// File 1 header
// SAFETY: first check
let val_one = from_raw(1);
// padding
// padding
// SAFETY: second check
let bridge_one = step();
let val_two = from_raw(2);
// File 1 footer
";
    fs::write(&file1, content1).unwrap();

    let file2 = tmp.path().join("b.rs");
    let content2 = "\
// File 2 header
// SAFETY: third check
let val_three = from_raw(3);
// padding
// padding
// SAFETY: fourth check
let bridge_two = step();
let val_four = from_raw(4);
// File 2 footer
";
    fs::write(&file2, content2).unwrap();

    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out_head1 = std::process::Command::new(grx_bin)
        .args([
            "--hidden",
            "from_raw",
            "near:2,SAFETY",
            "head:1",
            &format!("root:{}", file1.to_str().unwrap()),
        ])
        .output()
        .unwrap();
    assert_eq!(out_head1.status.code(), Some(0));
    let stdout_head1 = String::from_utf8_lossy(&out_head1.stdout);
    assert!(
        stdout_head1.contains("SAFETY: first check"),
        "head:1 must retain SAFETY line: {stdout_head1}"
    );
    assert!(
        stdout_head1.contains("from_raw(1)"),
        "head:1 must retain primary match: {stdout_head1}"
    );
    assert!(
        !stdout_head1.contains("from_raw(2)"),
        "head:1 must not contain second match: {stdout_head1}"
    );
    assert!(
        !stdout_head1.contains("SAFETY: second check"),
        "head:1 must not contain second SAFETY: {stdout_head1}"
    );

    let out_tail1 = std::process::Command::new(grx_bin)
        .args([
            "--hidden",
            "from_raw",
            "near:2,SAFETY",
            "tail:1",
            &format!("root:{}", file1.to_str().unwrap()),
        ])
        .output()
        .unwrap();
    assert_eq!(out_tail1.status.code(), Some(0));
    let stdout_tail1 = String::from_utf8_lossy(&out_tail1.stdout);
    assert!(
        stdout_tail1.contains("from_raw(2)"),
        "tail:1 must retain last primary match: {stdout_tail1}"
    );
    assert!(
        stdout_tail1.contains("SAFETY: second check"),
        "tail:1 must retain second SAFETY line: {stdout_tail1}"
    );
    assert!(
        stdout_tail1.contains("bridge_one = step()"),
        "tail:1 must retain bridge line: {stdout_tail1}"
    );
    assert!(
        !stdout_tail1.contains("from_raw(1)"),
        "tail:1 must not contain first match: {stdout_tail1}"
    );
    assert!(
        !stdout_tail1.contains("SAFETY: first check"),
        "tail:1 must not contain first SAFETY: {stdout_tail1}"
    );

    let out_head3 = std::process::Command::new(grx_bin)
        .args(["from_raw", "near:2,SAFETY", "sort:path", "head:3", "root:."])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(out_head3.status.code(), Some(0));
    let stdout_head3 = String::from_utf8_lossy(&out_head3.stdout);

    assert!(stdout_head3.contains("from_raw(1)"));
    assert!(stdout_head3.contains("SAFETY: first check"));
    assert!(stdout_head3.contains("from_raw(2)"));
    assert!(stdout_head3.contains("SAFETY: second check"));
    assert!(stdout_head3.contains("bridge_one = step()"));
    assert!(stdout_head3.contains("from_raw(3)"));
    assert!(stdout_head3.contains("SAFETY: third check"));

    assert!(!stdout_head3.contains("from_raw(4)"));
    assert!(!stdout_head3.contains("SAFETY: fourth check"));

    let out_tail3 = std::process::Command::new(grx_bin)
        .args(["from_raw", "near:2,SAFETY", "sort:path", "tail:3", "root:."])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(out_tail3.status.code(), Some(0));
    let stdout_tail3 = String::from_utf8_lossy(&out_tail3.stdout);

    assert!(!stdout_tail3.contains("from_raw(1)"));
    assert!(!stdout_tail3.contains("SAFETY: first check"));
    assert!(stdout_tail3.contains("from_raw(2)"));
    assert!(stdout_tail3.contains("SAFETY: second check"));
    assert!(stdout_tail3.contains("bridge_one = step()"));
    assert!(stdout_tail3.contains("from_raw(3)"));
    assert!(stdout_tail3.contains("SAFETY: third check"));
    assert!(stdout_tail3.contains("from_raw(4)"));
    assert!(stdout_tail3.contains("SAFETY: fourth check"));
    assert!(stdout_tail3.contains("bridge_two = step()"));

    let out_tail_count = std::process::Command::new(grx_bin)
        .args([
            "--hidden",
            "-c",
            "from_raw",
            "near:2,SAFETY",
            "tail:1",
            &format!("root:{}", file1.to_str().unwrap()),
        ])
        .output()
        .unwrap();
    assert_eq!(out_tail_count.status.code(), Some(0));
    let stdout_tail_count = String::from_utf8_lossy(&out_tail_count.stdout);
    assert!(
        stdout_tail_count.trim().ends_with(":1"),
        "count must be 1, got: {stdout_tail_count}"
    );
}

#[test]
fn test_cli_colon_terms_and_special_char_patterns() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("code.txt"), "let x = new:\nCommand::new()\nTODO: fix edge cases\nvisit http://localhost:8080\nmapping = foo:bar\n.btn { color: red !important; }\nprintf(\"%d\", count);\nnew:\n").unwrap();
    let run = |term: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["root:.", "--no-heading", "--color=never", term])
            .output()
            .unwrap()
    };
    for (term, expected) in [
        ("new:", "let x = new:"),
        (":new", "Command::new()"),
        ("TODO:", "TODO: fix"),
        ("http:", "http://localhost"),
        ("foo:bar", "mapping = foo:bar"),
        ("!important", "!important"),
        ("%d", "printf"),
    ] {
        let escaped = format!("lit:{term}");
        let out = run(&escaped);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{term}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stdout).contains(expected));
    }
    let exact = run("=new:");
    assert_eq!(exact.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&exact.stdout).lines().count(), 1);
    assert!(!String::from_utf8_lossy(&exact.stdout).contains("let x"));
    for term in ["new:", "foo:bar", "ext:rs", "dir:src", "@new:"] {
        assert_eq!(run(term).status.code(), Some(2), "{term}");
    }
}

#[test]
fn test_bare_and_trailing_sort_and_reverse() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out_trailing_bare = std::process::Command::new(grx_bin)
        .args(["t:rs", "-n", "--sort", "root:src/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_trailing_bare.status.code(), Some(0));
    let lines_trailing_bare = raw_lines(&out_trailing_bare.stdout);
    assert!(!lines_trailing_bare.is_empty());
    assert!(lines_trailing_bare[0].starts_with("1:"));

    let out_leading_bare = std::process::Command::new(grx_bin)
        .args(["--sort", "t:rs", "-n", "root:src/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_leading_bare.status.code(), Some(0));
    let lines_leading_bare = raw_lines(&out_leading_bare.stdout);
    assert_eq!(lines_trailing_bare, lines_leading_bare);

    let out_trailing_size = std::process::Command::new(grx_bin)
        .args(["t:rs", "--sort", "size", "-n", "root:src/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_trailing_size.status.code(), Some(0));

    let out_leading_size = std::process::Command::new(grx_bin)
        .args(["--sort", "size", "t:rs", "-n", "root:src/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_leading_size.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_trailing_size.stdout),
        raw_lines(&out_leading_size.stdout)
    );

    let out_trailing_desc = std::process::Command::new(grx_bin)
        .args(["t:rs", "--sort=-size", "-n", "root:src/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_trailing_desc.status.code(), Some(0));

    let out_trailing_size_rev = std::process::Command::new(grx_bin)
        .args(["t:rs", "--sort", "size", "--reverse", "-n", "root:src/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_trailing_size_rev.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_trailing_desc.stdout),
        raw_lines(&out_trailing_size_rev.stdout)
    );

    let out_trailing_rev = std::process::Command::new(grx_bin)
        .args(["t:rs", "-n", "--reverse", "root:src/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_trailing_rev.status.code(), Some(0));
    let lines_trailing_rev = raw_lines(&out_trailing_rev.stdout);
    assert!(!lines_trailing_rev.is_empty());

    if lines_trailing_bare.len() > 1 {
        assert_ne!(lines_trailing_bare, lines_trailing_rev);
    }
}

#[test]
fn in_selects_basenames_independently_of_types_content_and_name_expressions() {
    let tmp = tempdir().unwrap();
    fs::create_dir(tmp.path().join("codex")).unwrap();
    for (path, body) in [
        ("codex/memory.rs", "needle\n"),
        ("codex/memory_summary.md", "unrelated\n"),
        ("codex/other.rs", "needle memory\n"),
        ("codex/rs_notes.md", "needle\n"),
    ] {
        fs::write(tmp.path().join(path), body).unwrap();
    }
    let cases: &[(&[&str], &[&str])] = &[
        (
            &["kind:file", "memory"],
            &["codex/memory.rs", "codex/memory_summary.md"],
        ),
        (&["-l", "t:rs", "memory"], &["codex/other.rs"]),
        (&["kind:file", "t:rs", "memory"], &["codex/memory.rs"]),
        (&["-l", "in:memory"], &["codex/other.rs"]),
        (
            &["-l", "kind:file", "memory", "p:codex", "in:needle"],
            &["codex/memory.rs"],
        ),
        (
            &["-l", "t:rs", "in:needle"],
            &["codex/memory.rs", "codex/other.rs"],
        ),
        (
            &["-l", "kind:file", "=memory.rs", "in:=needle"],
            &["codex/memory.rs"],
        ),
        (
            &["kind:file", "memory", "AND", "summary"],
            &["codex/memory_summary.md"],
        ),
        (
            &["-l", "-e", "needle", "kind:file", "memory"],
            &["codex/memory.rs"],
        ),
        (&["kind:file", "fz:msm"], &["codex/memory_summary.md"]),
    ];
    for (args, expected) in cases {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["root:.", "--color=never"])
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(normalize_lines(&out.stdout), *expected, "{args:?}");
    }
}

fn isolated_grx(root: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_grx"));
    command
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("settings"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env_remove("GRX_CONFIG")
        .env_remove("RUST_LOG");
    command
}

#[test]
fn selectors_filter_actual_files_without_retargeting_content() {
    let tmp = tempdir().unwrap();
    for (name, body) in [
        ("report.rs", "unrelated\n"),
        ("scope/body.rs", "needle Send\n"),
        ("scope/report.py", "needle\n"),
        ("other/body.rs", "needle debug\n"),
        ("other/report.txt", "unrelated\n"),
        ("report-dir.rs/nested.rs", "needle\n"),
        ("build,cache/comma.rs", "needle\n"),
    ] {
        let path = tmp.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    let cases: &[(&[&str], &[&str])] = &[
        (
            &["t:rs", "needle"],
            &[
                "build,cache/comma.rs",
                "other/body.rs",
                "report-dir.rs/nested.rs",
                "scope/body.rs",
            ],
        ),
        (&["needle", "t:rs", "p:scope/"], &["scope/body.rs"]),
        (&["nt:rs", "needle"], &["scope/report.py"]),
        (
            &["(t:rs OR t:py)", "needle", "p:scope/"],
            &["scope/body.rs", "scope/report.py"],
        ),
        (
            &["t:rs?py", "needle", "p:scope/"],
            &["scope/body.rs", "scope/report.py"],
        ),
        (
            &["-t", "rs", "-t", "py", "needle", "p:scope/"],
            &["scope/body.rs", "scope/report.py"],
        ),
        (&["-g", "*.rs", "needle", "p:scope/"], &["scope/body.rs"]),
        (&["t:rs", "t:py", "needle"], &[]),
        (&["t:rs", "needle", "ns:debug", "p:other/"], &[]),
        (&["t:rs", "needle", "in:Send"], &["scope/body.rs"]),
        (
            &["kind:file", "report"],
            &["other/report.txt", "report.rs", "scope/report.py"],
        ),
        (&["report", "k:file", "t:rs"], &["report.rs"]),
        (&["kind:file", "report", "in:needle"], &["scope/report.py"]),
        (&["in:needle", "report", "kind:file"], &["scope/report.py"]),
        (
            &["kind:file", "report", "AND", "NOT", "re:.*[.]txt$"],
            &["report.rs", "scope/report.py"],
        ),
        (&["kind:dir", "report"], &["report-dir.rs/"]),
        (&["k:dir", "report"], &["report-dir.rs/"]),
        (&["kind:file", "fz:rprt", "p:scope/"], &["scope/report.py"]),
        (
            &["kind:file", "-Z", "rprt", "p:scope/"],
            &["scope/report.py"],
        ),
        (
            &["t:rs", "needle", "np:scope/", "np:other/"],
            &["build,cache/comma.rs", "report-dir.rs/nested.rs"],
        ),
        (
            &["t:rs", "needle", "p:build,cache/"],
            &["build,cache/comma.rs"],
        ),
        (&["t:rs", "p:scope/"], &["scope/body.rs"]),
        (&["kind:file", "=report.rs"], &["report.rs"]),
    ];
    for (args, names) in cases {
        let out = isolated_grx(tmp.path())
            .args([
                "--color=never",
                "--hyperlinks=never",
                "-l",
                "sort:path",
                "root:.",
            ])
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(if names.is_empty() { 1 } else { 0 }),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty(), "{args:?}");
        let expected = names
            .iter()
            .map(|name| format!("{}\n", name.replace('/', std::path::MAIN_SEPARATOR_STR)))
            .collect::<String>();
        assert_eq!(out.stdout, expected.as_bytes(), "{args:?}");
    }
}

#[test]
fn pattern_operators_select_known_lines_with_decoys() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("lines.txt"), "two words\n=foo\nkind:file\nAND\nfoo..bar\nfooXYbar\nbar foo\n(foo OR bar)\nfoo bar\nfoo baz\nbar qux\nfoo\nand or not\np:path\na:b\n main \nin:needle\nMain\n").unwrap();
    let cases: &[(&[&str], &str)] = &[
        (&["two words"], "two words\n"),
        (&["two", "words"], "two words\n"),
        (&["=foo"], "foo\n"),
        (&["=kind:file"], "kind:file\n"),
        (&["= main "], " main \n"),
        (&["lit:=foo"], "=foo\n"),
        (&["lit:kind:file"], "kind:file\n"),
        (&["lit:AND"], "AND\n"),
        (&["lit:p:path"], "p:path\n"),
        (&["lit:a:b"], "a:b\n"),
        (&["and", "or", "not"], "and or not\n"),
        (&["foo..bar"], "foo..bar\nfooXYbar\n(foo OR bar)\nfoo bar\n"),
        (&["-F", "foo..bar"], "foo..bar\n"),
        (&["-F", "in:foo..bar"], "foo..bar\n"),
        (
            &["-E", "foo.*bar"],
            "foo..bar\nfooXYbar\n(foo OR bar)\nfoo bar\n",
        ),
        (
            &["-E", "in:foo.*bar"],
            "foo..bar\nfooXYbar\n(foo OR bar)\nfoo bar\n",
        ),
        (&["re:^foo$"], "foo\n"),
        (&["fz:twwrds"], "two words\n"),
        (
            &["((foo OR bar) AND NOT (baz OR qux))"],
            "=foo\nfoo..bar\nfooXYbar\nbar foo\n(foo OR bar)\nfoo bar\nfoo\n",
        ),
        (
            &[r#"("two words" OR lit:"kind:file")"#],
            "two words\nkind:file\n",
        ),
        (
            &["foo", "AND", "bar", "OR", "=AND"],
            "AND\nfoo..bar\nfooXYbar\nbar foo\n(foo OR bar)\nfoo bar\n",
        ),
        (&["-w", "-s", "main"], " main \n"),
        (&["-w", "-i", "main"], " main \nMain\n"),
        (&["-S", "main"], " main \nMain\n"),
        (&["-S", "Main"], "Main\n"),
        (&["--", "in:needle"], "in:needle\n"),
    ];
    for (args, expected) in cases {
        let out = isolated_grx(tmp.path())
            .args([
                "--color=never",
                "--hyperlinks=never",
                "-I",
                "-N",
                "root:lines.txt",
            ])
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty(), "{args:?}");
        assert_eq!(out.stdout, expected.as_bytes(), "{args:?}");
    }
}

#[test]
fn multiple_pattern_sources_and_empty_pattern_files_select_expected_lines() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("lines.txt"),
        "alpha\nbeta\ngamma\nalphabet\n",
    )
    .unwrap();
    fs::write(tmp.path().join("patterns"), "=alpha\n=beta\n").unwrap();
    fs::write(tmp.path().join("empty"), "").unwrap();
    let cases: &[(&[&str], i32, &str)] = &[
        (&["-e", "=alpha", "-e", "=beta"], 0, "alpha\nbeta\n"),
        (&["-f", "patterns"], 0, "alpha\nbeta\n"),
        (&["-f", "empty"], 1, ""),
        (&["-v", "-f", "empty"], 0, "alpha\nbeta\ngamma\nalphabet\n"),
        (&["kind:file", "=lines.txt", "-e", "=alpha"], 0, "alpha\n"),
        (&["kind:file", "=patterns", "-e", "=alpha"], 1, ""),
    ];
    for (args, status, expected) in cases {
        let out = isolated_grx(tmp.path())
            .args([
                "--color=never",
                "--hyperlinks=never",
                "-I",
                "-N",
                "root:lines.txt",
            ])
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(*status),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty(), "{args:?}");
        assert_eq!(out.stdout, expected.as_bytes(), "{args:?}");
    }
}

#[test]
fn visibility_and_ignore_switches_change_the_emitted_file_set() {
    let tmp = tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("ignored/nested")).unwrap();
    fs::create_dir(tmp.path().join(".git")).unwrap();
    for name in ["visible.txt", ".hidden.txt", "ignored/nested/file.txt"] {
        fs::write(tmp.path().join(name), "needle\n").unwrap();
    }
    fs::write(tmp.path().join(".gitignore"), "ignored/\n").unwrap();
    let cases: &[(&[&str], &[&str])] = &[
        (&[], &["visible.txt"]),
        (&["--hidden"], &[".hidden.txt", "visible.txt"]),
        (&["-u"], &["ignored/nested/file.txt", "visible.txt"]),
        (
            &["-uu"],
            &[".hidden.txt", "ignored/nested/file.txt", "visible.txt"],
        ),
        (
            &["--hidden", "-u"],
            &[".hidden.txt", "ignored/nested/file.txt", "visible.txt"],
        ),
    ];
    for (args, names) in cases {
        let out = isolated_grx(tmp.path())
            .args(["-l", "needle", "sort:path", "root:."])
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let expected = names
            .iter()
            .map(|name| format!("{}\n", name.replace('/', std::path::MAIN_SEPARATOR_STR)))
            .collect::<String>();
        assert_eq!(out.stdout, expected.as_bytes(), "{args:?}");
    }
}

#[test]
fn byte_size_predicates_have_strict_boundaries_and_binary_units() {
    let tmp = tempdir().unwrap();
    for (name, bytes) in [
        ("a", 49),
        ("b", 50),
        ("c", 1024),
        ("d", 1025),
        ("e", 10241),
        ("f", 2147483649),
    ] {
        fs::File::create(tmp.path().join(name))
            .unwrap()
            .set_len(bytes)
            .unwrap();
    }
    let cases: &[(&[&str], &str)] = &[
        (&["smaller:50"], "a\n"),
        (&["larger:50", "smaller:1025B"], "c\n"),
        (&["larger:1KiB"], "d\ne\nf\n"),
        (&["larger:10k"], "e\nf\n"),
        (&["larger:10M"], "f\n"),
        (&["larger:2GB"], "f\n"),
        (&["smaller:2GB"], "a\nb\nc\nd\ne\n"),
    ];
    for (args, expected) in cases {
        let out = isolated_grx(tmp.path())
            .args(["kind:file", "sort:path", "root:."])
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, expected.as_bytes(), "{args:?}");
    }
}

#[test]
fn age_predicates_select_files_from_known_modification_times() {
    let tmp = tempdir().unwrap();
    let now = std::time::SystemTime::now();
    for (name, seconds) in [
        ("fresh", 1),
        ("minutes", 120),
        ("hours", 7200),
        ("ancient", 604801),
    ] {
        let file = fs::File::create(tmp.path().join(name)).unwrap();
        file.set_modified(now - std::time::Duration::from_secs(seconds))
            .unwrap();
    }
    for (query, expected) in [
        ("newer:7d", "fresh\nhours\nminutes\n"),
        ("older:1h", "ancient\nhours\n"),
        ("newer:30m", "fresh\nminutes\n"),
        ("older:45s", "ancient\nhours\nminutes\n"),
    ] {
        let out = isolated_grx(tmp.path())
            .args(["kind:file", "sort:path", query, "root:."])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{query}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, expected.as_bytes(), "{query}");
    }
}

#[test]
fn limits_apply_to_actual_records_after_per_file_selection() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("a.txt"),
        "needle a1\nneedle a2\nneedle a3\n",
    )
    .unwrap();
    fs::write(tmp.path().join("b.txt"), "needle b1\nneedle b2\n").unwrap();
    let cases: &[(&[&str], &str)] = &[
        (&["max:1"], "a.txt:needle a1\nb.txt:needle b1\n"),
        (&["max:1", "head:2"], "a.txt:needle a1\nb.txt:needle b1\n"),
        (&["max:1", "tail:1"], "b.txt:needle b1\n"),
        (&["max:1", "nth:2"], "b.txt:needle b1\n"),
        (&["-m", "1", "--head", "1"], "a.txt:needle a1\n"),
        (&["--tail", "2"], "b.txt:needle b1\nb.txt:needle b2\n"),
        (&["--nth", "-2"], "b.txt:needle b1\n"),
        (&["-n", "max:1"], "a.txt:1:needle a1\nb.txt:1:needle b1\n"),
    ];
    for (args, expected) in cases {
        let mut command = isolated_grx(tmp.path());
        if !args.contains(&"-n") {
            command.arg("-N");
        }
        let out = command
            .args([
                "--color=never",
                "--no-heading",
                "needle",
                "root:.",
                "sort:path",
            ])
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, expected.as_bytes(), "{args:?}");
    }
}

#[test]
fn literal_roots_with_punctuation_and_streams_have_distinct_sources() {
    use std::io::Write;
    let tmp = tempdir().unwrap();
    for name in ["a space,comma)", "-", "two words"] {
        fs::write(tmp.path().join(name), "from_file\n").unwrap();
        let root = format!("root:{name}");
        let out = isolated_grx(tmp.path())
            .args(["-I", "-N", "=from_file", &root])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, b"from_file\n");
    }
    let mut child = isolated_grx(tmp.path())
        .args(["-I", "-N", "=from_stream", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"from_file\nfrom_stream\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"from_stream\n");
}

#[test]
fn json_records_report_observed_matches_offsets_and_payload_bytes() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("input.txt"),
        "🦀 needle needle\ncontext\nneedle\n",
    )
    .unwrap();
    let out = isolated_grx(tmp.path())
        .args([
            "--color=never",
            "--json",
            "--stats",
            "-C",
            "1",
            "needle",
            "root:input.txt",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut events = Vec::new();
    let mut expected_bytes = 0u64;
    let mut final_stats = None;
    for line in out.stdout.split_inclusive(|byte| *byte == b'\n') {
        let value: serde_json::Value = serde_json::from_slice(line).unwrap();
        match value["type"].as_str().unwrap() {
            "begin" | "match" | "context" => expected_bytes += line.len() as u64,
            "summary" => final_stats = Some(value["data"]["stats"].clone()),
            "end" => (),
            kind => panic!("unexpected JSON record: {kind}"),
        }
        if let Some(number) = value["data"]["line_number"].as_u64() {
            events.push((value["type"].as_str().unwrap().to_owned(), number));
        }
    }
    assert_eq!(
        events,
        [
            ("match".into(), 1),
            ("context".into(), 2),
            ("match".into(), 3)
        ]
    );
    let stats = final_stats.unwrap();
    assert_eq!(stats["matches"], 3);
    assert_eq!(stats["matched_lines"], 2);
    assert_eq!(stats["bytes_searched"], 34);
    assert_eq!(stats["bytes_printed"], expected_bytes);
    let out = isolated_grx(tmp.path())
        .args([
            "--color=never",
            "--no-heading",
            "-n",
            "--column",
            "-b",
            "-o",
            "needle",
            "root:input.txt",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        out.stdout,
        b"input.txt:1:3:5:needle\ninput.txt:1:10:12:needle\ninput.txt:3:1:27:needle\n"
    );
}

#[test]
fn configuration_changes_search_results_and_alias_selection() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("config.toml"),
        "[search]\nsmart-case = false\n[types]\nproto = [\"*.proto\"]\n",
    )
    .unwrap();
    fs::write(tmp.path().join("schema.proto"), "needle\nNeedle\n").unwrap();
    fs::write(tmp.path().join("protocol.rs"), "needle\n").unwrap();
    for (args, expected) in [
        (vec!["-l", "t:proto", "needle"], "schema.proto\n"),
        (vec!["-l", "nt:proto", "needle"], "protocol.rs\n"),
        (vec!["-I", "-N", "needle", "p:=schema.proto"], "needle\n"),
        (
            vec!["-I", "-N", "-S", "needle", "p:=schema.proto"],
            "needle\nNeedle\n",
        ),
        (vec!["-l", "np:proto", "needle"], ""),
    ] {
        let out = isolated_grx(tmp.path())
            .args(["--config", "config.toml", "sort:path", "root:."])
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(if expected.is_empty() { 1 } else { 0 }),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, expected.as_bytes(), "{args:?}");
    }
}

#[test]
fn config_initialization_and_storage_paths_respect_child_xdg_locations() {
    let tmp = tempdir().unwrap();
    let out = isolated_grx(tmp.path())
        .arg("--init-config")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let config = tmp.path().join("settings/grx/config.toml");
    assert!(config.is_file());
    let bytes = fs::read(&config).unwrap();
    let refused = isolated_grx(tmp.path())
        .arg("--init-config")
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(2));
    assert_eq!(fs::read(&config).unwrap(), bytes);
    fs::write(&config, "# keep this until --force\n").unwrap();
    let forced = isolated_grx(tmp.path())
        .args(["--init-config", "--force"])
        .output()
        .unwrap();
    assert_eq!(forced.status.code(), Some(0));
    assert_eq!(fs::read(&config).unwrap(), bytes);
    let located = isolated_grx(tmp.path())
        .arg("--config-path")
        .output()
        .unwrap();
    assert_eq!(located.status.code(), Some(0));
    assert_eq!(located.stdout, format!("{}\n", config.display()).as_bytes());
    let paths = isolated_grx(tmp.path()).arg("--paths").output().unwrap();
    assert_eq!(paths.status.code(), Some(0));
    let text = String::from_utf8(paths.stdout).unwrap();
    for (label, relative) in [
        ("Config Dir", "settings/grx"),
        ("Data Dir", "data/grx"),
        ("Journal File", "data/grx/journal.jsonl"),
        ("Cache Dir", "cache/grx"),
        ("State Dir", "state/grx"),
    ] {
        let line = text
            .lines()
            .find(|line| line.trim_start().starts_with(&format!("{label}:")))
            .unwrap();
        assert_eq!(
            line.split_once(':').unwrap().1.trim(),
            tmp.path().join(relative).display().to_string()
        );
    }
}

#[test]
fn completion_installation_creates_the_requested_shell_artifact() {
    let tmp = tempdir().unwrap();
    for (shell, relative, header, alias) in [
        (
            "fish",
            "settings/fish/completions/grx.fish",
            "complete -c grx -f",
            "-a \"k:\"",
        ),
        (
            "bash",
            "data/bash-completion/completions/grx",
            "_grx_completions()",
            "kind: k: t:",
        ),
        ("zsh", "home/.zfunc/_grx", "#compdef grx", "k\\:["),
    ] {
        let out = isolated_grx(tmp.path())
            .args(["--install-completions", shell])
            .env_remove("ZDOTDIR")
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{shell}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let installed = tmp.path().join(relative);
        let body = fs::read_to_string(&installed).unwrap();
        assert!(body.trim_start().starts_with(header), "{shell}");
        assert!(body.contains(alias), "{shell}: missing public k: alias");
        assert!(String::from_utf8_lossy(&out.stdout).contains(&installed.display().to_string()));
        if shell == "bash" {
            let loaded = std::process::Command::new("bash")
                .args([
                    "--noprofile",
                    "--norc",
                    "-c",
                    "source \"$1\"; complete -p grx",
                    "grx-test",
                ])
                .arg(installed)
                .output()
                .unwrap();
            assert_eq!(
                loaded.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&loaded.stderr)
            );
            assert_eq!(loaded.stdout, b"complete -F _grx_completions grx\n");
        }
    }
}

#[cfg(unix)]
#[test]
fn native_mode_from_configuration_preserves_literal_arguments() {
    let tmp = tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("settings/grx")).unwrap();
    fs::write(
        tmp.path().join("settings/grx/config.toml"),
        "mode = \"grep\"\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("input.txt"),
        "needle\nkind:file\n$(touch marker)\n",
    )
    .unwrap();
    for (args, expected) in [
        (vec!["-n", "needle", "input.txt"], "1:needle\n"),
        (vec!["-n", "-F", "kind:file", "input.txt"], "2:kind:file\n"),
        (
            vec!["-n", "-F", "$(touch marker)", "input.txt"],
            "3:$(touch marker)\n",
        ),
    ] {
        let out = isolated_grx(tmp.path()).args(&args).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, expected.as_bytes(), "{args:?}");
        assert!(!tmp.path().join("marker").exists());
    }
}

#[cfg(unix)]
#[test]
fn rename_and_chmod_flags_change_only_the_selected_file() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("old.rs"), "selected\n").unwrap();
    fs::write(tmp.path().join("old.txt"), "decoy\n").unwrap();
    let renamed = isolated_grx(tmp.path())
        .args(["kind:file", "=old.rs", "--rename", "old/new", "root:."])
        .output()
        .unwrap();
    assert_eq!(
        renamed.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&renamed.stderr)
    );
    assert!(!tmp.path().join("old.rs").exists());
    assert_eq!(fs::read(tmp.path().join("new.rs")).unwrap(), b"selected\n");
    assert_eq!(fs::read(tmp.path().join("old.txt")).unwrap(), b"decoy\n");
    fs::set_permissions(tmp.path().join("new.rs"), fs::Permissions::from_mode(0o600)).unwrap();
    let chmod = isolated_grx(tmp.path())
        .args(["kind:file", "=new.rs", "--chmod", "755", "root:."])
        .output()
        .unwrap();
    assert_eq!(
        chmod.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&chmod.stderr)
    );
    assert_eq!(
        fs::metadata(tmp.path().join("new.rs"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(fs::read(tmp.path().join("old.txt")).unwrap(), b"decoy\n");
}

#[test]
fn reader_thresholds_and_file_size_limits_preserve_complete_match_bytes() {
    let tmp = tempdir().unwrap();
    let mut body = b"needle first\n".to_vec();
    body.extend_from_slice("hay\n".repeat(512).as_bytes());
    body.extend_from_slice(b"needle last \xff\n");
    fs::write(tmp.path().join("input.txt"), body).unwrap();
    for threshold in ["0", "1", "4096", "1048576"] {
        let out = isolated_grx(tmp.path())
            .args([
                "-a",
                "-I",
                "-N",
                "needle",
                "root:input.txt",
                "--mmap-threshold",
                threshold,
            ])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{threshold}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            out.stdout, b"needle first\nneedle last \xff\n",
            "{threshold}"
        );
    }
    let limited = isolated_grx(tmp.path())
        .args([
            "-a",
            "-I",
            "-N",
            "needle",
            "root:input.txt",
            "--max-file-size",
            "5",
        ])
        .output()
        .unwrap();
    assert_eq!(limited.status.code(), Some(1));
    assert!(limited.stdout.is_empty());
    assert!(limited.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn entry_kinds_classify_real_contents_and_links_not_extensions() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("binary.txt"), b"needle\0\n").unwrap();
    fs::write(tmp.path().join("plain.bin"), b"needle\n").unwrap();
    fs::create_dir(tmp.path().join("directory.txt")).unwrap();
    std::os::unix::fs::symlink("binary.txt", tmp.path().join("link.txt")).unwrap();
    std::os::unix::fs::symlink("missing", tmp.path().join("broken.txt")).unwrap();
    for (kind, expected) in [
        ("file", "binary.txt\nplain.bin\n"),
        ("bin", "binary.txt\n"),
        ("text", "plain.bin\n"),
        ("dir", "directory.txt/\n"),
        ("link", "broken.txt\nlink.txt\n"),
    ] {
        for prefix in ["kind", "k"] {
            let selector = format!("{prefix}:{kind}");
            let out = isolated_grx(tmp.path())
                .args([&selector, "sort:path", "root:."])
                .output()
                .unwrap();
            assert_eq!(
                out.status.code(),
                Some(0),
                "{selector}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.stdout, expected.as_bytes(), "{selector}");
        }
    }
}

#[test]
fn sort_flags_order_known_file_sizes_and_content_records() {
    let tmp = tempdir().unwrap();
    for (name, contents) in [
        ("a.txt", "needle aa\nneedle\nprefix needle zzz\n"),
        ("b.txt", "needle\n"),
        ("c.txt", "needle\nneedle\n"),
    ] {
        fs::write(tmp.path().join(name), contents).unwrap();
    }
    let cases: &[(&[&str], &[&str])] = &[
        (&["kind:file", "sort:size"], &["b.txt", "c.txt", "a.txt"]),
        (
            &["kind:file", "--sort", "size", "--reverse", "sort:bytes"],
            &["a.txt", "c.txt", "b.txt"],
        ),
        (
            &[
                "kind:file",
                "--sort",
                "size",
                "--sort-reverse",
                "sort:bytes",
            ],
            &["a.txt", "c.txt", "b.txt"],
        ),
        (&["needle", "sort:count"], &["a.txt", "c.txt", "b.txt"]),
        (&["needle", "sort:-count"], &["b.txt", "c.txt", "a.txt"]),
    ];
    for (args, expected) in cases {
        let out = isolated_grx(tmp.path())
            .args(["--json", "root:."])
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let paths = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .filter_map(|line| {
                let value: serde_json::Value = serde_json::from_str(line).unwrap();
                if value["type"] == "begin" || value["type"] == "entry" {
                    value["data"]["path"]["text"].as_str().map(str::to_owned)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(paths, *expected, "{args:?}");
    }
    for (sort, expected) in [
        ("sort:len", vec![2, 1, 3]),
        ("sort:-len", vec![3, 1, 2]),
        ("sort:-line", vec![3, 2, 1]),
    ] {
        let out = isolated_grx(tmp.path())
            .args(["--json", "needle", sort, "root:a.txt"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0));
        let numbers = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .filter_map(|line| {
                let value: serde_json::Value = serde_json::from_str(line).unwrap();
                (value["type"] == "match").then(|| value["data"]["line_number"].as_u64().unwrap())
            })
            .collect::<Vec<_>>();
        assert_eq!(numbers, expected, "{sort}");
    }
}

#[test]
fn invalid_queries_fail_before_traversal_or_filesystem_actions() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("needle.txt"), "needle\n").unwrap();
    let cases: &[&[&str]] = &[
        &["!target/"],
        &["type:rs"],
        &["no:proto"],
        &["@foo"],
        &["larger:bad"],
        &["smaller:"],
        &["older:7w"],
        &["newer:bad"],
        &["fz:"],
        &["in:"],
        &["needle", "AND"],
        &["("],
        &["()"],
        &["kind:file", "kind:dir"],
        &["kind:dir", "t:rs"],
        &["kind:dir", "in:needle"],
        &["kind:file", "near:1,needle"],
        &["head:1", "tail:2"],
        &["nth:0"],
        &["nth:1", "pick:"],
        &["kind:file", "sort:line"],
        &["kind:file", "sort:count"],
        &["kind:file", "needle", "cp:copied", "mv:moved"],
        &["needle", "d:1", "d:2"],
        &["needle", "-s", "-i"],
        &["needle", "--hidden", "no:dots"],
        &["needle", "yes:cache", "no:cache"],
        &["kind:file", "needle", "OR", "in:needle"],
    ];
    for args in cases {
        let out = isolated_grx(tmp.path())
            .arg("root:.")
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(!out.stderr.is_empty(), "{args:?}");
        assert_eq!(
            fs::read(tmp.path().join("needle.txt")).unwrap(),
            b"needle\n"
        );
        assert!(!tmp.path().join("copied").exists());
        assert!(!tmp.path().join("moved").exists());
    }
}

#[cfg(unix)]
#[test]
fn benchmark_entry_points_reject_partial_or_bounded_timing() {
    let scripts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts");
    let cases: &[(&[&str], &str)] = &[
        (
            &["--case", "source_literal"],
            "unrecognized arguments: --case",
        ),
        (&["--suite", "source"], "unrecognized arguments: --suite"),
        (&["--runs", "1"], "unrecognized arguments: --runs"),
        (&["--max-runs", "1"], "unrecognized arguments: --max-runs"),
        (&["--warmup", "0"], "at least one warmup is required"),
        (
            &["--min-runs", "1"],
            "at least 10 measured runs are required",
        ),
    ];
    for (program, script) in [
        ("bash", "bench_hyperfine.sh"),
        ("python3", "bench_comparison.py"),
    ] {
        for (args, expected_error) in cases {
            let out = std::process::Command::new(program)
                .arg(scripts.join(script))
                .args([
                    "--grx",
                    env!("CARGO_BIN_EXE_grx"),
                    "--rg",
                    env!("CARGO_BIN_EXE_grx"),
                ])
                .args(*args)
                .env_remove("RUNS")
                .output()
                .unwrap();
            assert_eq!(out.status.code(), Some(2), "{script} {args:?}");
            assert!(
                out.stdout.is_empty(),
                "must reject before benchmark startup: {script} {args:?}"
            );
            assert!(
                String::from_utf8_lossy(&out.stderr).contains(expected_error),
                "{script} {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
    let out = std::process::Command::new("bash")
        .arg(scripts.join("bench_hyperfine.sh"))
        .env("RUNS", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("RUNS is retired"));

    let python = std::process::Command::new("python3")
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .unwrap();
    assert!(python.status.success());
    let python_path = String::from_utf8(python.stdout).unwrap();
    let out = std::process::Command::new(python_path.trim())
        .arg(scripts.join("bench_comparison.py"))
        .args([
            "--grx",
            env!("CARGO_BIN_EXE_grx"),
            "--rg",
            env!("CARGO_BIN_EXE_grx"),
        ])
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(
        out.stdout.is_empty(),
        "missing Hyperfine must never start a fallback timer"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("executable not found: hyperfine"));
}

#[test]
fn cli_reports_clean_relative_paths_in_every_output_mode() {
    let tmp = tempdir().unwrap();
    fs::create_dir(tmp.path().join("nested")).unwrap();
    fs::write(tmp.path().join("main.rs"), "needle\n").unwrap();
    fs::write(tmp.path().join("nested/main.py"), "needle\n").unwrap();
    fs::write(tmp.path().join("other.txt"), "unrelated\n").unwrap();
    let cases: &[(&[&str], &[u8])] = &[
        (
            &["-N", "--no-heading", "needle"],
            b"main.rs:needle\nnested/main.py:needle\n",
        ),
        (&["-l", "needle"], b"main.rs\nnested/main.py\n"),
        (&["-l", "needle", "root:./"], b"main.rs\nnested/main.py\n"),
        (&["-L", "needle"], b"other.txt\n"),
        (&["-l", "-0", "needle"], b"main.rs\0nested/main.py\0"),
        (&["-L", "-0", "needle"], b"other.txt\0"),
        (&["-c", "needle", "root:./main.rs"], b"main.rs:1\n"),
        (
            &["--heading", "-N", "needle", "root:./main.rs"],
            b"main.rs\nneedle\n\n",
        ),
        (&["kind:file", "main"], b"main.rs\nnested/main.py\n"),
        (&["kind:file", "main", "-0"], b"main.rs\0nested/main.py\0"),
    ];
    for (args, expected) in cases {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_grx"));
        command
            .current_dir(tmp.path())
            .args(["--color=never", "sort:path"])
            .args(*args);
        if !args.iter().any(|arg| arg.starts_with("root:")) {
            command.arg("root:.");
        }
        let out = command.output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, *expected, "{args:?}");
    }
    for args in [
        vec!["--json", "needle"],
        vec!["--json", "-L", "needle"],
        vec!["--json", "kind:file", "main"],
    ] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["sort:path", "root:."])
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0));
        let mut paths = std::collections::BTreeSet::new();
        for line in out
            .stdout
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
        {
            let record: serde_json::Value = serde_json::from_slice(line).unwrap();
            if let Some(path) = record["data"]["path"]["text"].as_str() {
                paths.insert(path.to_owned());
            }
        }
        let expected = if args.contains(&"-L") {
            vec!["other.txt"]
        } else {
            vec!["main.rs", "nested/main.py"]
        };
        assert_eq!(paths.into_iter().collect::<Vec<_>>(), expected, "{args:?}");
    }
}

#[test]
fn cli_kind_short_alias_selects_names_and_validates_conflicts() {
    let tmp = tempdir().unwrap();
    fs::create_dir(tmp.path().join("nested")).unwrap();
    fs::write(tmp.path().join("main.rs"), "unrelated\n").unwrap();
    fs::write(tmp.path().join("nested/main.py"), "needle\n").unwrap();
    fs::write(tmp.path().join("other.rs"), "main\n").unwrap();
    let cases: &[(&[&str], &[u8])] = &[
        (&["k:file", "main"], b"main.rs\nnested/main.py\n"),
        (&["main", "k:file", "t:rs"], b"main.rs\n"),
        (&["k:dir", "nested"], b"nested/\n"),
        (&["k:text", "main"], b"main.rs\nnested/main.py\n"),
        (&["-l", "k:file", "main", "in:needle"], b"nested/main.py\n"),
        (
            &["k:file", "kind:file", "main"],
            b"main.rs\nnested/main.py\n",
        ),
    ];
    for (args, expected) in cases {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["--color=never", "sort:path"])
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, *expected, "{args:?}");
    }
    for args in [
        vec!["k:file", "kind:dir", "main"],
        vec!["k:dir", "t:rs"],
        vec!["k:dir", "in:needle"],
        vec!["k:"],
        vec!["k:unknown"],
    ] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(!out.stderr.is_empty(), "{args:?}");
    }
}

#[test]
fn cli_path_cleanup_preserves_absolute_parent_and_hidden_paths() {
    let tmp = tempdir().unwrap();
    fs::create_dir(tmp.path().join("nested")).unwrap();
    fs::create_dir(tmp.path().join(".hidden")).unwrap();
    fs::write(tmp.path().join("main.rs"), "needle\n").unwrap();
    fs::write(tmp.path().join(".hidden/main.rs"), "needle\n").unwrap();
    let absolute = tmp.path().join("main.rs");
    for (cwd, root, expected) in [
        (
            tmp.path(),
            format!("root:{}", absolute.display()),
            absolute.display().to_string(),
        ),
        (
            tmp.path().join("nested").as_path(),
            "root:../main.rs".to_owned(),
            "../main.rs".to_owned(),
        ),
        (
            tmp.path(),
            "root:./.hidden/main.rs".to_owned(),
            ".hidden/main.rs".to_owned(),
        ),
    ] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(cwd)
            .args(["--color=never", "-l", "-uu", "needle", &root])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{root}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, format!("{expected}\n").into_bytes());
    }
}

#[cfg(unix)]
#[test]
fn cli_clean_paths_preserve_non_utf8_filename_bytes() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join(OsStr::from_bytes(b"bad-\xff.rs")),
        "needle\n",
    )
    .unwrap();
    let cases: &[(&[&str], &[u8])] = &[
        (&["-l", "-0", "needle"], b"bad-\xff.rs\0"),
        (&["-l", "needle"], b"bad-\xff.rs\n"),
        (&["-c", "needle"], b"bad-\xff.rs:1\n"),
        (&["--count-matches", "needle"], b"bad-\xff.rs:1\n"),
        (&["k:file", "-0"], b"bad-\xff.rs\0"),
    ];
    for (args, expected) in cases {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["root:."])
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{args:?}");
        assert_eq!(out.stdout, *expected, "{args:?}");
    }
}

#[test]
fn type_filters_search_contents_unless_kind_explicitly_selects_names() {
    let tmp = tempdir().unwrap();
    for (name, body) in [
        ("core.rs", "pub trait Example: Send {}\n"),
        ("trait.rs", "unrelated\n"),
        ("integration_tests.rs", "unrelated\n"),
        ("other.py", "pub trait Example: Send {}\n"),
        ("debug.rs", "pub trait Example: debug {}\n"),
    ] {
        fs::write(tmp.path().join(name), body).unwrap();
    }
    let cases: &[(&[&str], &[&str])] = &[
        (&["-l", "t:rs", "trait"], &["core.rs", "debug.rs"]),
        (&["-l", "trait", "t:rs"], &["core.rs", "debug.rs"]),
        (&["-l", "t:rs", "fz:trait"], &["core.rs", "debug.rs"]),
        (&["-l", "nt:py", "trait"], &["core.rs", "debug.rs"]),
        (&["-l", "t:rs", "trait", "ns:debug"], &["core.rs"]),
        (&["-l", "t:rs", "trait", "in:Send"], &["core.rs"]),
        (&["-l", "t:rs", "re:pub.*Send"], &["core.rs"]),
        (&["-l", "t:rs", "=pub trait Example: Send {}"], &["core.rs"]),
        (
            &["-l", "t:rs", "OR", "t:py", "trait"],
            &["core.rs", "debug.rs", "other.py"],
        ),
        (&["kind:file", "t:rs", "trait"], &["trait.rs"]),
        (&["trait", "t:rs", "kind:file"], &["trait.rs"]),
        (&["kind:file", "nt:py", "trait"], &["trait.rs"]),
        (
            &["kind:file", "t:rs", "fz:trait"],
            &["integration_tests.rs", "trait.rs"],
        ),
        (&["kind:file", "t:rs", "trait", "in:Send"], &[]),
        (
            &["t:rs"],
            &["core.rs", "debug.rs", "integration_tests.rs", "trait.rs"],
        ),
    ];
    for (args, expected) in cases {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["--color=never", "sort:path"])
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(if expected.is_empty() { 1 } else { 0 }),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(normalize_lines(&out.stdout), *expected, "{args:?}");
    }
}

fn setup_modal_contract_corpus() -> tempfile::TempDir {
    let tmp = tempdir().unwrap();
    for dir in [
        "a/codex",
        "b/codex",
        "codexish",
        ".codex",
        "ignored",
        "a/codex/nested",
        "directory.rs",
    ] {
        fs::create_dir_all(tmp.path().join(dir)).unwrap();
    }
    for (path, body) in [
        ("a/codex/memory.rs", "other\n"),
        ("a/codex/body.rs", "codex\ncodexish\nerror panic\n"),
        ("a/codex/nested/memory.py", "TODO\nerror\npanic\n"),
        ("b/codex/memory.py", "codex\n"),
        ("codexish/memory.rs", "other\n"),
        (".codex/memory.rs", "other\n"),
        ("codex_file.rs", "other\n"),
        ("ignored/memory.rs", "codex\n"),
        ("memory.rs", "other\n"),
        ("mxxmxxrxxy.rs", "other\n"),
        ("my_rem.rs", "other\n"),
        ("fuzzy.txt", "mxxmxxrxxy\nmemory\nmy_rem\n"),
    ] {
        fs::write(tmp.path().join(path), body).unwrap();
    }
    fs::create_dir(tmp.path().join(".git")).unwrap();
    fs::write(tmp.path().join(".gitignore"), "ignored/\n").unwrap();
    tmp
}

#[test]
fn modal_contract_distinguishes_names_contents_and_paths() {
    let tmp = setup_modal_contract_corpus();
    let cases: &[(&[&str], &[&str])] = &[
        (&["kind:dir", "=codex"], &["a/codex/", "b/codex/"]),
        (
            &["kind:file", "t:rs", "=memory.rs"],
            &["a/codex/memory.rs", "codexish/memory.rs", "memory.rs"],
        ),
        (&["-l", "t:rs", "in:=codex"], &["a/codex/body.rs"]),
        (
            &["-l", "kind:file", "=body.rs", "in:codex"],
            &["a/codex/body.rs"],
        ),
        (
            &["kind:file", "p:codex/", "=memory.rs"],
            &["a/codex/memory.rs"],
        ),
        (
            &["-l", "codex", "p:codex/"],
            &["a/codex/body.rs", "b/codex/memory.py"],
        ),
        (
            &["t:rs", "p:codex/"],
            &["a/codex/body.rs", "a/codex/memory.rs"],
        ),
        (
            &["-uuu", "t:rs", "p:codex"],
            &[
                ".codex/memory.rs",
                "a/codex/body.rs",
                "a/codex/memory.rs",
                "codex_file.rs",
                "codexish/memory.rs",
            ],
        ),
        (
            &["kind:file", "root:a", "root:a/codex", "=memory.rs"],
            &["a/codex/memory.rs"],
        ),
        (
            &["kind:file", "root:a/codex", "d:0"],
            &["a/codex/body.rs", "a/codex/memory.rs"],
        ),
        (&["-i", "t:rs", "p:CODEX/", "np:A/CODEX/"], &[]),
    ];
    for (args, expected) in cases {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["--color=never"])
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(if expected.is_empty() { 1 } else { 0 }),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(normalize_lines(&out.stdout), *expected, "{args:?}");
    }
}

#[test]
fn modal_contract_boolean_scopes_and_invalid_combinations() {
    let tmp = setup_modal_contract_corpus();
    for args in [
        vec!["kind:file", "t:rs", "OR", "t:py", "memory"],
        vec!["kind:file", "t:rs?py", "memory"],
        vec!["memory", "t:rs?py", "kind:file"],
    ] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["--color=never"])
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            normalize_lines(&out.stdout),
            vec![
                "a/codex/memory.rs",
                "a/codex/nested/memory.py",
                "b/codex/memory.py",
                "codexish/memory.rs",
                "memory.rs"
            ]
        );
    }
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(tmp.path())
        .args(["-l", "t:rs?py", "in:error", "AND", "in:panic"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(normalize_lines(&out.stdout), vec!["a/codex/body.rs"]);
    let grouped = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(tmp.path())
        .args(["kind:file", "(memory OR body)", "p:codex/", "NOT", "nested"])
        .output()
        .unwrap();
    assert_eq!(
        grouped.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&grouped.stderr)
    );

    assert_eq!(
        normalize_lines(&grouped.stdout),
        vec![
            "a/codex/body.rs",
            "a/codex/memory.rs",
            "a/codex/nested/memory.py",
            "b/codex/memory.py"
        ]
    );
    for args in [
        vec!["kind:dir", "t:rs", "codex"],
        vec!["kind:link", "t:rs"],
        vec!["kind:file", "d:2", "codex", "kind:dir"],
        vec!["kind:dir", "in:codex"],
        vec!["kind:file", "memory", "near:1,TODO"],
        vec!["kind:file", "max:1"],
        vec!["kind:file", "sort:count"],
        vec!["kind:file", "memory", "OR", "in:codex"],
        vec!["kind:file", "re:[", "AND", "=not-present"],
        vec!["kind:file", "@memory"],
        vec!["in:"],
        vec!["fz:"],
        vec!["ns:TODO"],
        vec!["t:rs", "in:codex", "-"],
    ] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "must reject {args:?}: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn modal_contract_exact_fuzzy_ranking_and_flag_positions() {
    let tmp = setup_modal_contract_corpus();
    for args in [
        vec!["kind:file", "fz:mmry", "root:.", "d:0", "--color=never"],
        vec!["--color=never", "-Z", "mmry", "kind:file", "root:.", "d:0"],
    ] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(raw_lines(&out.stdout), vec!["memory.rs", "mxxmxxrxxy.rs"]);
    }
    let content = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(tmp.path())
        .args(["-I", "-N", "root:fuzzy.txt", "in:fz:mmry"])
        .output()
        .unwrap();
    assert_eq!(content.status.code(), Some(0));
    assert_eq!(raw_lines(&content.stdout), vec!["memory", "mxxmxxrxxy"]);
    let exact = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(tmp.path())
        .args(["-I", "-N", "root:a/codex/body.rs", "in:=codex"])
        .output()
        .unwrap();
    assert_eq!(exact.status.code(), Some(0));
    assert_eq!(raw_lines(&exact.stdout), vec!["codex"]);
    #[cfg(unix)]
    {
        fs::write(tmp.path().join("kind:file"), "reserved\n").unwrap();
        let literal = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["kind:file", "lit:kind:file"])
            .output()
            .unwrap();
        assert_eq!(literal.status.code(), Some(0));
        assert_eq!(raw_lines(&literal.stdout), vec!["kind:file"]);
    }
}

#[test]
fn modal_contract_review_regressions_exact_paths_unicode_counts_and_indices() {
    let tmp = setup_modal_contract_corpus();
    fs::create_dir(tmp.path().join("école")).unwrap();
    fs::write(tmp.path().join("école/memory.rs"), "memory\n").unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["--color=never"])
            .args(args)
            .output()
            .unwrap()
    };
    let exact_path = run(&["kind:file", "root:a/codex", "p:=memory.rs"]);
    assert_eq!(exact_path.status.code(), Some(0));
    assert_eq!(raw_lines(&exact_path.stdout), vec!["a/codex/memory.rs"]);
    let unicode = run(&["-i", "kind:file", "p:ÉCOLE/"]);
    assert_eq!(unicode.status.code(), Some(0));
    assert_eq!(raw_lines(&unicode.stdout), vec!["école/memory.rs"]);
    for flag in ["-c", "--count-matches"] {
        let count = run(&[flag, "-I", "root:fuzzy.txt", "in:fz:mmry"]);
        assert_eq!(count.status.code(), Some(0));
        assert_eq!(raw_lines(&count.stdout), vec!["2"]);
    }
    for args in [
        vec!["kind:file", "nth:-9223372036854775808"],
        vec!["in:other", "nth:-9223372036854775808"],
    ] {
        let out = run(&args);

        assert_eq!(
            out.status.code(),
            Some(if usize::BITS == 64 { 1 } else { 2 })
        );
    }
    let invalid = run(&["kind:file", "memory", "AND", "AND", "p:codex"]);
    assert_eq!(invalid.status.code(), Some(2));
    let files = run(&["p:codex/"]);
    assert_eq!(files.status.code(), Some(0));
    assert!(
        raw_lines(&files.stdout)
            .iter()
            .all(|path| !path.ends_with('/'))
    );
}

#[test]
fn modal_contract_actions_require_complete_selection_and_preserve_command_boundary() {
    let tmp = setup_modal_contract_corpus();
    let failed_action = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(tmp.path())
        .env("XDG_DATA_HOME", tmp.path().join(".state"))
        .args(["kind:file", "root:memory.rs", "root:missing", "cp:copied/"])
        .output()
        .unwrap();
    assert_eq!(failed_action.status.code(), Some(2));
    assert!(
        !tmp.path().join("copied").exists(),
        "no action may run on an incomplete selection"
    );
    assert!(tmp.path().join("memory.rs").exists());
    #[cfg(unix)]
    {
        let command = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args([
                "kind:file",
                "=memory.rs",
                "root:.",
                "d:0",
                "--exec",
                "echo",
                "--",
                "picked:{/}",
            ])
            .output()
            .unwrap();
        assert_eq!(
            command.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&command.stderr)
        );
        assert_eq!(raw_lines(&command.stdout), vec!["-- picked:memory.rs"]);
    }
}

#[test]
#[cfg(unix)]
fn modal_contract_head_stops_content_reads_and_stdin_is_explicit() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("a.txt"), "needle\n").unwrap();
    fs::write(tmp.path().join("z.txt"), "needle\n").unwrap();
    fs::set_permissions(tmp.path().join("z.txt"), fs::Permissions::from_mode(0o000)).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(tmp.path())
        .args(["--no-ignore-messages", "needle", "root:.", "head:1"])
        .output()
        .unwrap();
    fs::set_permissions(tmp.path().join("z.txt"), fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "head must stop before reading z.txt: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("a.txt"));
    assert!(!String::from_utf8_lossy(&out.stdout).contains("z.txt"));
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(tmp.path())
        .args(["-I", "-N", "in:=needle", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"needle\nneedle suffix\n")
        .unwrap();
    let stdin = child.wait_with_output().unwrap();
    assert_eq!(stdin.status.code(), Some(0));
    assert_eq!(raw_lines(&stdin.stdout), vec!["needle"]);
}

#[test]
fn modal_contract_matching_flags_share_the_same_pattern_language() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("lines.txt"),
        "call()\ncall() suffix\nfoo..bar\nfooxbar\n(foo OR bar)\npubenum\nmemory\n",
    )
    .unwrap();
    let cases: &[(&[&str], &[&str])] = &[
        (&["=call()"], &["call()"]),
        (&["-e", "=call()"], &["call()"]),
        (&["-e", "fz:mmry"], &["memory"]),
        (&["-F", "foo..bar"], &["foo..bar"]),
        (&["-F", "in:foo..bar"], &["foo..bar"]),
        (&["-F", "(foo OR bar)"], &["(foo OR bar)"]),
        (&["-E", "(pub|enum){2}"], &["pubenum"]),
        (&["-E", "in:(pub|enum){2}"], &["pubenum"]),
        (&["lit:(foo OR bar)"], &["(foo OR bar)"]),
        (
            &["(re:call\\(\\) OR =pubenum)"],
            &["call()", "call() suffix", "pubenum"],
        ),
    ];
    for (args, expected) in cases {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["-I", "-N", "root:lines.txt"])
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(raw_lines(&out.stdout), *expected, "{args:?}");
    }
    fs::write(tmp.path().join("-"), "literal_file\n").unwrap();
    let literal_root = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(tmp.path())
        .args(["-I", "-N", "root:-", "in:=literal_file"])
        .output()
        .unwrap();
    assert_eq!(literal_root.status.code(), Some(0));
    assert_eq!(raw_lines(&literal_root.stdout), vec!["literal_file"]);
    for args in [
        vec!["in:memory", "d:0", "d:1"],
        vec!["in:memory", "max:1", "max:2"],
        vec!["in:memory", "-i", "-s"],
        vec!["in:memory", "--sort=size", "sort:path"],
        vec!["in:memory", "--max-count=1", "max:2"],
        vec!["t:rs??py"],
        vec!["t:rs?"],
        vec!["in:memory", "near:1,other", "sort:len"],
    ] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["root:lines.txt"])
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "must reject {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn parallel_content_preserves_order_limits_context_and_counts() {
    let tmp = tempdir().unwrap();
    for index in 0..24 {
        let block = format!(
            "memory needle first {index}\npadding\nmemory needle second {index}\nerror panic\nSAFETY: checked\nunsafe pointer;\n"
        );
        fs::write(
            tmp.path().join(format!("part_{index:03}.rs")),
            block.repeat(8),
        )
        .unwrap();
    }
    for query in [
        vec!["t:rs", "in:needle"],
        vec!["t:rs", "in:error", "AND", "in:panic"],
        vec!["t:rs", "in:fz:mmry"],
        vec!["t:rs", "in:needle", "ctx:1"],
        vec!["t:rs", "in:unsafe", "near:2,SAFETY"],
        vec!["t:rs", "in:needle", "head:3"],
        vec!["t:rs", "in:unsafe", "near:2,SAFETY", "head:2"],
        vec!["t:rs", "in:needle", "tail:3"],
        vec!["t:rs", "in:needle", "nth:-1"],
        vec!["t:rs", "in:needle", "-c"],
        vec!["t:rs", "in:needle", "-l"],
    ] {
        let run = |threads: &str| {
            std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
                .current_dir(tmp.path())
                .args([
                    "--color=never",
                    "--no-heading",
                    "-N",
                    "-s",
                    "root:.",
                    "-j",
                    threads,
                ])
                .args(&query)
                .output()
                .unwrap()
        };
        let serial = run("1");
        let parallel = run("4");
        assert_eq!(
            serial.status.code(),
            Some(0),
            "{query:?}: {}",
            String::from_utf8_lossy(&serial.stderr)
        );
        assert_eq!(parallel.status.code(), serial.status.code(), "{query:?}");
        assert_eq!(
            parallel.stdout, serial.stdout,
            "thread count changed {query:?}"
        );
        assert!(parallel.stderr.is_empty() && serial.stderr.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn scoped_question_mark_lists_select_and_exclude_independent_alternatives() {
    let tmp = tempdir().unwrap();
    for name in ["alpha.txt", "beta.txt", "other.txt", "a,b.txt", "a?b.txt"] {
        fs::write(tmp.path().join(name), "needle\n").unwrap();
    }
    let cases: &[(&[&str], &[&str])] = &[
        (&["p:alpha?beta"], &["alpha.txt", "beta.txt"]),
        (&["np:alpha?beta"], &["a,b.txt", "a?b.txt", "other.txt"]),
        (&["p:alpha?beta", "np:beta"], &["alpha.txt"]),
        (
            &["(np:alpha?beta OR p:alpha)"],
            &["a,b.txt", "a?b.txt", "alpha.txt", "other.txt"],
        ),
        (&["(p:alpha?beta AND np:beta)"], &["alpha.txt"]),
        (&["p:a,b"], &["a,b.txt"]),
        (&[r"p:a\?b"], &["a?b.txt"]),
        (
            &[r"np:a\?b"],
            &["a,b.txt", "alpha.txt", "beta.txt", "other.txt"],
        ),
        (&["p:re:^(alpha|beta)[.]txt$"], &["alpha.txt", "beta.txt"]),
        (&[r"p:re:a\?b"], &["a?b.txt"]),
        (&["p:lit:a?b"], &["a?b.txt"]),
        (&["kind:file", "a?b"], &["a?b.txt"]),
    ];
    for (args, expected) in cases {
        let mut command = isolated_grx(tmp.path());
        if !args.contains(&"kind:file") {
            command.args(["-l", "needle"]);
        }
        let out = command
            .args(["root:.", "sort:path"])
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(raw_lines(&out.stdout), *expected, "{args:?}");
    }
    for (name, contents) in [
        ("one.rs", "needle\n"),
        ("two.py", "needle\n"),
        ("three.txt", "needle\n"),
        ("comma.rs,py", "needle\n"),
        ("question.r?s", "needle\n"),
    ] {
        fs::write(tmp.path().join(name), contents).unwrap();
    }
    for (query, expected) in [
        ("t:rs?py", vec!["one.rs", "two.py"]),
        ("t:rs,py", vec!["comma.rs,py"]),
        (r"t:r\?s", vec!["question.r?s"]),
        (
            "nt:rs?py",
            vec![
                "a,b.txt",
                "a?b.txt",
                "alpha.txt",
                "beta.txt",
                "comma.rs,py",
                "other.txt",
                "question.r?s",
                "three.txt",
            ],
        ),
    ] {
        let out = isolated_grx(tmp.path())
            .args(["-l", "needle", "root:.", "sort:path", query])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{query}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(raw_lines(&out.stdout), expected, "{query}");
    }
    for query in [
        "p:?alpha",
        "p:alpha?",
        "np:alpha??beta",
        "t:rs?",
        "nt:?py",
        "ns:debug?",
    ] {
        let out = isolated_grx(tmp.path())
            .args(["needle", "root:.", query])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{query}");
        assert!(out.stdout.is_empty(), "{query}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("Empty"),
            "{query}"
        );
    }
}

#[test]
fn exclusion_lists_do_not_reinterpret_content_or_explicit_pattern_languages() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("lines.txt"), "needle clean\nneedle debug\nneedle trace\nneedle debug trace\nneedle a?b\nneedle a,b\nneedle x:y\n").unwrap();
    let cases: &[(&[&str], &str)] = &[
        (
            &["needle", "ns:debug?trace"],
            "needle clean\nneedle a?b\nneedle a,b\nneedle x:y\n",
        ),
        (
            &["needle", "ns:debug", "ns:trace"],
            "needle clean\nneedle a?b\nneedle a,b\nneedle x:y\n",
        ),
        (
            &["needle", "(ns:debug?trace OR in:debug)"],
            "needle clean\nneedle debug\nneedle debug trace\nneedle a?b\nneedle a,b\nneedle x:y\n",
        ),
        (
            &["needle", r"ns:a\?b"],
            "needle clean\nneedle debug\nneedle trace\nneedle debug trace\nneedle a,b\nneedle x:y\n",
        ),
        (
            &["needle", "ns:re:debug?"],
            "needle clean\nneedle trace\nneedle a?b\nneedle a,b\nneedle x:y\n",
        ),
        (
            &["needle", "ns:lit:a?b"],
            "needle clean\nneedle debug\nneedle trace\nneedle debug trace\nneedle a,b\nneedle x:y\n",
        ),
        (&["a?b"], "needle a?b\n"),
        (&["in:a?b"], "needle a?b\n"),
        (&["needle?clean"], ""),
        (&["in:needle?clean"], ""),
        (&["a,b"], "needle a,b\n"),
    ];
    for (args, expected) in cases {
        let out = isolated_grx(tmp.path())
            .args([
                "-I",
                "-N",
                "--color=never",
                "--hyperlinks=never",
                "root:lines.txt",
            ])
            .args(*args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(if expected.is_empty() { 1 } else { 0 }),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, expected.as_bytes(), "{args:?}");
    }
}

#[test]
fn removed_setting_prefixes_are_errors_and_cli_replacements_still_work() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("input.txt"), "needle\nNeedle\n").unwrap();
    for prefix in ["yes", "no"] {
        for value in [
            "dots",
            "ignore",
            "case",
            "cache",
            "ln",
            "line-number",
            "num",
            "#",
        ] {
            let token = format!("{prefix}:{value}");
            let out = isolated_grx(tmp.path())
                .args(["needle", "root:input.txt", &token])
                .output()
                .unwrap();
            assert_eq!(out.status.code(), Some(2), "{token}");
            assert!(out.stdout.is_empty(), "{token}");
            assert!(
                String::from_utf8_lossy(&out.stderr).contains("unknown prefix"),
                "{token}"
            );
        }
    }
    for token in [
        "ln:",
        "num:",
        "line-number:",
        "no-ln:",
        "no-num:",
        "no-line-number:",
        "--no-follow",
    ] {
        let out = isolated_grx(tmp.path())
            .args(["needle", "root:input.txt", token])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{token}");
        assert!(out.stdout.is_empty(), "{token}");
    }
    for (args, expected) in [
        (vec!["-s", "-N", "Needle"], "Needle\n"),
        (vec!["-i", "-N", "Needle"], "needle\nNeedle\n"),
        (vec!["-n", "Needle"], "2:Needle\n"),
        (vec!["--no-line-number", "Needle"], "Needle\n"),
    ] {
        let out = isolated_grx(tmp.path())
            .args([
                "-I",
                "--color=never",
                "--hyperlinks=never",
                "root:input.txt",
            ])
            .args(args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(out.stdout, expected.as_bytes());
    }
    for args in [
        vec!["--help"],
        vec!["--tutorial"],
        vec!["--dump-config"],
        vec!["--completions", "fish"],
        vec!["--completions", "bash"],
        vec!["--completions", "zsh"],
    ] {
        let out = isolated_grx(tmp.path()).args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(0));
        let text = String::from_utf8(out.stdout).unwrap();
        for retired in ["yes:", "no:", "ln:", "no\\:", "yes\\:"] {
            assert!(
                !text.contains(retired),
                "{args:?} still advertises {retired}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn path_filter_escapes_preserve_colons_backslashes_and_directory_boundaries() {
    let tmp = tempdir().unwrap();
    for name in [
        "colon:name.txt",
        r"C:\work\file.txt",
        r"slash\name.txt",
        "question?.txt",
    ] {
        fs::write(tmp.path().join(name), "needle\n").unwrap();
    }
    for dir in ["alpha", "beta", "alphabet", "keep"] {
        fs::create_dir(tmp.path().join(dir)).unwrap();
        fs::write(tmp.path().join(dir).join("file.txt"), "needle\n").unwrap();
    }
    for (query, expected) in [
        ("p:colon:name", vec!["colon:name.txt"]),
        (r"p:C:\work\file", vec![r"C:\work\file.txt"]),
        (r"p:slash\name", vec![r"slash\name.txt"]),
        (r"p:question\?", vec!["question?.txt"]),
        ("p:alpha/?beta/", vec!["alpha/file.txt", "beta/file.txt"]),
        ("p:alpha/?beta/ np:alpha/", vec!["beta/file.txt"]),
    ] {
        let out = isolated_grx(tmp.path())
            .args(["-l", "needle", "sort:path", "root:."])
            .args(query.split(' '))
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{query}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8(out.stdout)
                .unwrap()
                .lines()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
            expected,
            "{query}"
        );
    }
    let out = isolated_grx(tmp.path())
        .args([
            "-l",
            "needle",
            "sort:path",
            "root:.",
            "np:alpha/?beta/",
            "p:file.txt",
        ])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>(),
        vec![r"C:\work\file.txt", "alphabet/file.txt", "keep/file.txt"]
    );
    let out = isolated_grx(tmp.path())
        .args([
            "-l",
            "needle",
            "sort:path",
            "root:.",
            "(np:alpha/?beta/ OR p:alpha/)",
            "p:file.txt",
        ])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>(),
        vec![
            r"C:\work\file.txt",
            "alpha/file.txt",
            "alphabet/file.txt",
            "keep/file.txt"
        ]
    );
}

#[test]
#[cfg(target_os = "linux")]
fn reported_small_walker_buffers_preserve_inventory() {
    let tmp = tempdir().unwrap();
    fs::create_dir(tmp.path().join("files")).unwrap();
    for name in [
        "a.rs",
        "long_filename_that_needs_a_larger_dirent.toml",
        "z.txt",
    ] {
        fs::write(tmp.path().join("files").join(name), b"needle\n").unwrap();
    }
    for size in [1, 32, 64, 65536] {
        fs::write(
            tmp.path().join("config.toml"),
            format!("[search]\nwalker-buffer-size-bytes = {size}\n"),
        )
        .unwrap();
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args([
                "--config",
                "config.toml",
                "--color=never",
                "kind:file",
                "root:files",
                "sort:path",
            ])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{size}: {:?}", out.stderr);
        assert_eq!(
            out.stdout,
            b"files/a.rs\nfiles/long_filename_that_needs_a_larger_dirent.toml\nfiles/z.txt\n"
        );
        assert!(out.stderr.is_empty());
    }
}

#[test]
#[cfg(target_os = "linux")]
fn reported_procfs_reads_ignore_mmap_threshold() {
    let expected = fs::read("/proc/version").unwrap();
    for threshold in ["0", "1", "65536"] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .args([
                "--color=never",
                "--no-heading",
                "-N",
                "--no-filename",
                "lit:Linux",
                "root:/proc/version",
                "--mmap-threshold",
                threshold,
            ])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{threshold}: {:?}", out.stderr);
        assert_eq!(out.stdout, expected);
    }
}

#[test]
fn reported_globs_filter_paths_directories_and_types() {
    let tmp = tempdir().unwrap();
    fs::create_dir(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("src/main.rs"), b"version\n").unwrap();
    fs::write(tmp.path().join("src/settings.toml"), b"version\n").unwrap();
    fs::write(tmp.path().join("Cargo.toml"), b"version\n").unwrap();
    for (args, expected) in [
        (
            vec!["kind:file", "-g", "!*.rs", "root:src"],
            "src/settings.toml\n",
        ),
        (vec!["kind:file", "-g", "!Cargo*", "root:Cargo.toml"], ""),
        (
            vec!["kind:file", "-g", "*.rs", "-g", "!src/*", "root:src"],
            "",
        ),
        (
            vec!["kind:file", "-g", "src/*.rs", "root:."],
            "src/main.rs\n",
        ),
        (vec!["kind:dir", "-g", "src*", "root:."], "src/\n"),
        (
            vec!["kind:file", "-g", "Cargo.?oml", "root:Cargo.toml"],
            "Cargo.toml\n",
        ),
        (
            vec!["kind:file", "-t", "rs", "-g", "*.toml", "root:src"],
            "",
        ),
        (
            vec!["version", "-t", "rs", "-g", "*.toml", "root:Cargo.toml"],
            "",
        ),
        (
            vec!["version", "t:rs", "-g", "*.toml", "root:Cargo.toml"],
            "",
        ),
    ] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
            .current_dir(tmp.path())
            .args(["--color=never", "sort:path"])
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(if expected.is_empty() { 1 } else { 0 }),
            "{args:?}: {:?}",
            out.stderr
        );
        assert_eq!(out.stdout, expected.as_bytes(), "{args:?}");
    }
}

#[test]
fn reported_action_plans_reject_collisions_and_impossible_destinations() {
    let tmp = tempdir().unwrap();
    for dir in ["one", "two", "dest"] {
        fs::create_dir(tmp.path().join(dir)).unwrap();
    }
    fs::write(tmp.path().join("one/mod.rs"), b"one").unwrap();
    fs::write(tmp.path().join("two/mod.rs"), b"two").unwrap();
    fs::write(tmp.path().join("target.txt"), b"target").unwrap();
    for force in [false, true] {
        for action in ["--copy", "--move"] {
            for (roots, destination, diagnostic) in [
                (vec!["root:one", "root:two"], "dest", "Multiple sources"),
                (vec!["root:one"], "target.txt", "not a directory"),
                (vec!["root:one"], "target.txt/subdir", "not a directory"),
                (vec!["root:one/mod.rs"], "one", "same"),
            ] {
                let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_grx"));
                command
                    .current_dir(tmp.path())
                    .args([
                        "--color=never",
                        "kind:file",
                        "=mod.rs",
                        "--dry-run",
                        action,
                        destination,
                    ])
                    .args(&roots);
                if force {
                    command.arg("--force");
                }
                let out = command.output().unwrap();
                assert_eq!(
                    out.status.code(),
                    Some(2),
                    "{action} {destination}: {:?}",
                    out.stdout
                );
                assert!(
                    String::from_utf8_lossy(&out.stderr).contains(diagnostic),
                    "{action} {destination}: {:?}",
                    out.stderr
                );
            }
        }
    }
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(tmp.path())
        .args([
            "kind:file",
            "=mod.rs",
            "root:one",
            "rename:mod.rs/",
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Invalid rename destination"));
    assert_eq!(fs::read(tmp.path().join("one/mod.rs")).unwrap(), b"one");
    assert_eq!(fs::read(tmp.path().join("two/mod.rs")).unwrap(), b"two");
    assert_eq!(fs::read_dir(tmp.path().join("dest")).unwrap().count(), 0);
}

fn reported_stdin(args: &[&str], input: &[u8]) -> std::process::Output {
    use std::io::Write;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args(["--color=never", "--no-heading", "-I"])
        .args(if args.contains(&"-n") {
            Vec::new()
        } else {
            vec!["-N"]
        })
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("CLI regression exceeded ten seconds: {args:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn reported_word_case_and_line_semantics() {
    for (args, input, expected) in [
        (vec!["-w", "lit:."], ".\na.a\n . \n", ".\n . \n"),
        (vec!["-w", "lit:C++"], "C++\n C++ \nxC++y\n", "C++\n C++ \n"),
        (vec!["-i", "lit:k"], "k\nK\n", "k\nK\n"),
        (vec!["-i", "lit:K"], "k\nK\n", "k\nK\n"),
        (vec!["-i", "lit:s"], "s\nſ\n", "s\nſ\n"),
        (vec!["-i", "lit:ſ"], "s\nſ\n", "s\nſ\n"),
        (vec!["-i", "lit:K", "AND", "lit:"], "k\nK\n", "k\nK\n"),
        (
            vec!["-S", "re:^needle"],
            "NEEDLE\nneedle\n",
            "NEEDLE\nneedle\n",
        ),
        (
            vec!["-S", "-E", "^needle"],
            "NEEDLE\nneedle\n",
            "NEEDLE\nneedle\n",
        ),
        (vec!["-S", "re:^Needle"], "Needle\nneedle\n", "Needle\n"),
        (
            vec!["-i", "needle", "near:1,UPPER"],
            "needle\nupper\n",
            "needle\nupper\n",
        ),
        (vec!["-s", "needle", "near:1,upper"], "needle\nUPPER\n", ""),
        (
            vec!["-i", "needle", "no-near:1,UPPER"],
            "needle\nupper\n",
            "",
        ),
    ] {
        let out = reported_stdin(&args, input.as_bytes());
        assert_eq!(
            out.status.code(),
            Some(if expected.is_empty() { 1 } else { 0 }),
            "{args:?}: {:?}",
            out.stderr
        );
        assert_eq!(out.stdout, expected.as_bytes(), "{args:?}");
    }
    for args in [
        vec!["-w", "fz:needle"],
        vec!["-x", "fz:needle"],
        vec!["-w", "hex:62"],
        vec!["lit:alpha\nbeta"],
        vec!["lit:alpha\nbeta", "AND", "lit:"],
        vec!["lit:alpha\r"],
        vec!["lit:alpha\r", "AND", "lit:"],
    ] {
        let out = reported_stdin(&args, b"alpha\r\nbeta\nxneedley\n");
        assert_eq!(out.status.code(), Some(2), "{args:?}: {:?}", out.stdout);
        assert!(!out.stderr.is_empty());
    }
}

#[test]
fn reported_explicit_regex_overrides_configured_fixed_strings() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("config.toml"),
        b"[search]\nfixed-strings = true\n",
    )
    .unwrap();
    fs::write(tmp.path().join("input.txt"), b"abc\n[a-z]+\n").unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(tmp.path())
        .args([
            "--config",
            "config.toml",
            "--color=never",
            "--no-heading",
            "-I",
            "-N",
            "-E",
            "[a-z]+",
            "root:input.txt",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"abc\n[a-z]+\n");
}

#[test]
fn reported_long_boolean_expressions_return_errors() {
    for args in [
        std::iter::repeat_n("NOT", 4000)
            .chain(["alpha"])
            .collect::<Vec<_>>(),
        std::iter::repeat_n(["alpha", "OR"], 10000)
            .flatten()
            .chain(["beta"])
            .collect(),
        vec!["("; 4000]
            .into_iter()
            .chain(["alpha"])
            .chain(vec![")"; 4000])
            .collect(),
    ] {
        let out = reported_stdin(&args, b"alpha\nbeta\n");
        assert_eq!(out.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&out.stderr).contains("complexity"));
    }
    let args = std::iter::repeat_n("NOT", 64)
        .chain(["alpha"])
        .collect::<Vec<_>>();
    let out = reported_stdin(&args, b"alpha\nbeta\n");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"alpha\n");
    let patterns = ["-e", "alpha"].repeat(4000);
    let out = reported_stdin(&patterns, b"alpha\n");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("complexity"));
}

#[test]
fn reported_fuzzy_repetitive_matches_complete_with_correct_spans() {
    let pattern = format!("fz:{}", "a".repeat(4000));
    for input in [format!("{}\n", "a".repeat(4000))] {
        for (flag, expected) in [
            ("-q", Vec::new()),
            ("-o", format!("{}\n", "a".repeat(4000)).into_bytes()),
            ("--count-matches", b"1\n".to_vec()),
        ] {
            let out = reported_stdin(&[flag, &pattern], input.as_bytes());
            assert_eq!(out.status.code(), Some(0), "{flag}: {:?}", out.stderr);
            assert_eq!(out.stdout, expected);
        }
    }
}

#[test]
fn reported_hex_context_and_counts_use_source_lines() {
    let out = reported_stdin(&["hex:62", "ctx:1", "-n"], b"a\nb\nc\n");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"1-a\n2:b\n3-c\n");
    for (flag, expected) in [
        ("-c", b"1\n".as_slice()),
        ("--count-matches", b"3\n".as_slice()),
        ("-o", b"b\nb\nb\n".as_slice()),
    ] {
        let out = reported_stdin(&["hex:62", flag], b"bbb\n");
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(out.stdout, expected);
    }
    let out = reported_stdin(&["hex:62", "-n", "-b", "--column"], b"a\nb\nc\n");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"2:1:2:b\n");
}

#[test]
fn reported_binary_text_preserves_extraction_and_positions() {
    for input in [b"xx\0needleZZ\n".as_slice(), b"xxneedleZZ\n".as_slice()] {
        let out = reported_stdin(&["-a", "-o", "needle"], input);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(out.stdout, b"needle\n");
    }
    let out = reported_stdin(&["-a", "-n", "--column", "-b", "needle"], b"xx\0needleZZ\n");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"1:4:0:xx\0needleZZ\n");
}

#[test]
fn reported_max_columns_counts_characters() {
    for (input, expected) in [
        ("ééé\n", "éé ... [omitted 2 bytes]\n"),
        ("eee\n", "ee ... [omitted 1 bytes]\n"),
    ] {
        let out = reported_stdin(&["-M", "2", "lit:"], input.as_bytes());
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(out.stdout, expected.as_bytes());
        let full = reported_stdin(&["-M", "2", "--no-truncate", "lit:"], input.as_bytes());
        assert_eq!(full.stdout, input.as_bytes());
    }
}

#[test]
fn reported_json_preserves_source_bytes_terminators_offsets_and_size() {
    for input in [
        b"needle".as_slice(),
        b"needle\r\nneedle\r\n",
        b"one\nneedle\nthree\n",
    ] {
        let out = reported_stdin(&["--json", "needle", "ctx:1"], input);
        assert_eq!(out.status.code(), Some(0));
        let records: Vec<serde_json::Value> = out
            .stdout
            .split(|&byte| byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        let mut emitted = Vec::new();
        for record in &records {
            let data = &record["data"];
            match record["type"].as_str().unwrap() {
                "match" | "context" => {
                    let payload = data["lines"]["text"].as_str().unwrap().as_bytes();
                    let offset = data["absolute_offset"].as_u64().unwrap() as usize;
                    assert_eq!(payload, &input[offset..offset + payload.len()]);
                    emitted.extend_from_slice(payload);
                    for submatch in data["submatches"].as_array().unwrap() {
                        let start = submatch["start"].as_u64().unwrap() as usize;
                        let end = submatch["end"].as_u64().unwrap() as usize;
                        assert_eq!(&payload[start..end], b"needle");
                        assert_eq!(submatch["match"]["text"], "needle");
                    }
                }
                "end" => assert_eq!(data["stats"]["bytes_searched"], input.len()),
                "summary" => assert_eq!(data["stats"]["bytes_searched"], input.len()),
                _ => {}
            }
        }
        assert_eq!(emitted, input);
    }
    let out = reported_stdin(&["--json", "-a", "needle"], b"\xffneedle\n");
    assert_eq!(out.status.code(), Some(0));
    let record: serde_json::Value = out
        .stdout
        .split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<serde_json::Value>(line).unwrap())
        .find(|record| record["type"] == "match")
        .unwrap();
    assert_eq!(
        record["data"]["lines"],
        serde_json::json!({"bytes":"/25lZWRsZQo="})
    );
    assert_eq!(
        record["data"]["submatches"],
        serde_json::json!([{"start":1,"end":7,"match":{"text":"needle"}}])
    );
    let out = reported_stdin(&["--json", "-a", "hex:ff"], b"\xffneedle\n");
    let record = out
        .stdout
        .split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<serde_json::Value>(line).unwrap())
        .find(|record| record["type"] == "match")
        .unwrap();
    assert_eq!(
        record["data"]["submatches"],
        serde_json::json!([{"start":0,"end":1,"match":{"bytes":"/w=="}}])
    );
}
