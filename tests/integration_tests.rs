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

    let cli = Cli::parse_from(["grx", "-q", "world", tmp.path().to_str().unwrap()]);
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
        "-q",
        "foobar_absent_term",
        tmp.path().to_str().unwrap(),
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

    let cli = Cli::parse_from(["grx", "pattern", nonexistent.to_str().unwrap()]);
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
    // Mixed CRLF and no trailing newline at EOF
    fs::write(
        &file_path,
        b"first line\r\nsecond match without trailing newline",
    )
    .unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args(["second", tmp.path().to_str().unwrap()])
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
    // Invalid UTF-8 sequence followed by match term
    let mut bytes = vec![0xFF, 0xFE, 0xFD];
    bytes.extend_from_slice(b"\nvalid search target here\n");
    bytes.push(0x80);
    fs::write(&file_path, &bytes).unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args(["-a", "target", tmp.path().to_str().unwrap()])
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

    // Create a circular symlink
    std::os::unix::fs::symlink(&sub, sub.join("loop")).unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args(["needle", "--follow", tmp.path().to_str().unwrap()])
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
        .args(["-L", "needle", tmp.path().to_str().unwrap()])
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

    // In JSON Lines mode, -L must emit valid JSON Lines records, not plaintext
    let output_json = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args(["-L", "--json", "needle", tmp.path().to_str().unwrap()])
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

    // In JSON Lines mode with -c, non-matching files must never emit plaintext 'path:0' lines
    let output_count_json = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "-c",
            "--json",
            "nonexistent_pattern",
            tmp.path().to_str().unwrap(),
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
        .args(["match", "max:5", tmp.path().to_str().unwrap()])
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
        .args(["match", tmp.path().to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn grx");

    // Read only a small initial chunk, then drop stdout to close the pipe while grx continues producing output
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

    // Default search: benign error suppressed, exits 1 (no match)
    let cli = Cli::parse_from(["grx", "-q", "nomatch", tmp.path().to_str().unwrap()]);
    let config = Config::new_with_defaults();
    let mut engine = Engine::new(config, cli);
    let exit_code = engine.run().unwrap();
    assert_eq!(
        exit_code, 1,
        "Should exit 1 (no match) with benign errors suppressed"
    );

    // With --no-ignore-messages: error not suppressed, exits 2 (error)
    let cli = Cli::parse_from([
        "grx",
        "--no-ignore-messages",
        "nomatch",
        tmp.path().to_str().unwrap(),
    ]);
    let config = Config::new_with_defaults();
    let mut engine = Engine::new(config, cli);
    let exit_code = engine.run().unwrap();
    assert_eq!(
        exit_code, 2,
        "Should exit 2 when --no-ignore-messages is set"
    );

    // Clean up permissions so tempdir can be removed cleanly
    let _ = fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644));
}

#[test]
fn test_engine_no_messages_flag_suppresses_nonexistent_root() {
    let tmp = tempdir().unwrap();
    let nonexistent = tmp.path().join("ghost_file.txt");

    let cli = Cli::parse_from([
        "grx",
        "--no-messages",
        "pattern",
        nonexistent.to_str().unwrap(),
    ]);
    let config = Config::new_with_defaults();
    let mut engine = Engine::new(config, cli);
    let exit_code = engine.run().unwrap();
    assert_eq!(
        exit_code, 1,
        "Should exit 1 instead of 2 when --no-messages is supplied"
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
            "target",
            valid_file.to_str().unwrap(),
            ghost.to_str().unwrap(),
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
        .args(["search_pattern", ghost.to_str().unwrap()])
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
    // '🦀' is a 4-byte UTF-8 char at column 1, ' ' is column 2, 'c' begins at column 3
    fs::write(&file_path, "🦀 crab rustacean\n").unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--column",
            "--no-heading",
            "--color",
            "never",
            "crab",
            file_path.to_str().unwrap(),
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
            "--no-heading",
            "--color",
            "never",
            "repeatable",
            "max:5",
            file_path.to_str().unwrap(),
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

    // Create directories mimicking real user layout:
    // 1. Data/Projects/gcc/libgrust/mod.rs
    // 2. Data/Projects/cprojects/hello.rs
    // 3. Documents/research/quests.rs
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

    // 1. Test unquoted `np:proj` without wildcards
    let output_np = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--no-heading",
            "--color",
            "never",
            "t:rs",
            "np:proj",
            "fn",
            root.to_str().unwrap(),
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

    // 2. Test unquoted `np:Projects/` without wildcards (component boundary directory exclusion)
    let output_no = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--no-heading",
            "--color",
            "never",
            "t:rs",
            "np:Projects/",
            "fn",
            root.to_str().unwrap(),
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

    // 3. Test that `np:proj/` (with trailing slash) enforces component boundary and does NOT exclude Projects/
    let output_no_proj = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--no-heading",
            "--color",
            "never",
            "t:rs",
            "np:proj/",
            "fn",
            root.to_str().unwrap(),
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

    // 1. near:2,SAFETY should match line 3 (within 2 lines of SAFETY) and NOT line 9
    let output_near = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--no-heading",
            "--color",
            "never",
            "from_raw",
            "near:2,SAFETY",
            file_path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output_near.status.code(), Some(0));
    let stdout_near = String::from_utf8_lossy(&output_near.stdout);
    assert!(stdout_near.contains("let ptr = from_raw(p)"));
    assert!(!stdout_near.contains("let dangerous = from_raw(bad)"));

    // 2. no-near:2,SAFETY should match line 9 and NOT line 3
    let output_no_near = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--no-heading",
            "--color",
            "never",
            "from_raw",
            "no-near:2,SAFETY",
            file_path.to_str().unwrap(),
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
    let file_path = tmp.path().join("infix.rs");
    let content = "\
// Line 1
// SAFETY: caller ensures alignment
let ptr = from_raw(p); // Line 3
foo();
bar();
baz();
let dangerous = from_raw(bad); // Line 7
";
    fs::write(&file_path, content).unwrap();

    // Infix NEAR:2
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--no-heading",
            "--color",
            "never",
            "from_raw",
            "NEAR:2",
            "SAFETY",
            file_path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("let ptr = from_raw(p)"));
    assert!(!stdout.contains("let dangerous = from_raw(bad)"));

    // Infix NOT NEAR:2
    let output_not = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--no-heading",
            "--color",
            "never",
            "from_raw",
            "NOT",
            "NEAR:2",
            "SAFETY",
            file_path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output_not.status.code(), Some(0));
    let stdout_not = String::from_utf8_lossy(&output_not.stdout);
    assert!(!stdout_not.contains("let ptr = from_raw(p)"));
    assert!(stdout_not.contains("let dangerous = from_raw(bad)"));
}

#[test]
fn test_cli_fuzzy_token_permutation_search() {
    let tmp = tempdir().unwrap();
    let file_path = tmp.path().join("fuzzy.rs");
    let content = "\
fn from_ptr_err(x: u32) -> Result<(), ()> {}
fn from_err_ptr(y: u32) -> Result<(), ()> {}
fn err_ptr_from(z: u32) -> Result<(), ()> {}
fn other_func(w: u32) -> Result<(), ()> {}
";
    fs::write(&file_path, content).unwrap();

    // 1. DSL fz:from,ptr,err
    let output_fz = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--no-heading",
            "--color",
            "never",
            "fz:from,ptr,err",
            file_path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output_fz.status.code(), Some(0));
    let stdout_fz = String::from_utf8_lossy(&output_fz.stdout);
    assert!(stdout_fz.contains("from_ptr_err"));
    assert!(stdout_fz.contains("from_err_ptr"));
    assert!(stdout_fz.contains("err_ptr_from"));
    assert!(!stdout_fz.contains("other_func"));

    // 2. DSL fz: prefix with snake_case auto-splitting: fz:from_ptr_err
    let output_fz2 = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--no-heading",
            "--color",
            "never",
            "fz:from_ptr_err",
            file_path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output_fz2.status.code(), Some(0));
    let stdout_fz2 = String::from_utf8_lossy(&output_fz2.stdout);
    assert!(stdout_fz2.contains("from_ptr_err"));
    assert!(stdout_fz2.contains("from_err_ptr"));
    assert!(stdout_fz2.contains("err_ptr_from"));
    assert!(!stdout_fz2.contains("other_func"));

    // 3. Deprecated %% prefix returns helpful error
    let output_deprecated_pct = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args(["%%from_ptr_err", file_path.to_str().unwrap()])
        .output()
        .expect("Failed to execute grx binary");
    assert_eq!(output_deprecated_pct.status.code(), Some(2));
    let err_pct = String::from_utf8_lossy(&output_deprecated_pct.stderr);
    assert!(err_pct.contains(
        "Fuzzy prefix in '%%from_ptr_err' is deprecated. Use canonical 'fz:from_ptr_err'"
    ));

    // 4. CLI flag -Z / --fuzzy
    let output_cli = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "-Z",
            "from,ptr,err",
            "--no-heading",
            "--color",
            "never",
            file_path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output_cli.status.code(), Some(0));
    let stdout_cli = String::from_utf8_lossy(&output_cli.stdout);
    assert!(stdout_cli.contains("from_ptr_err"));
    assert!(stdout_cli.contains("from_err_ptr"));
    assert!(stdout_cli.contains("err_ptr_from"));
    assert!(!stdout_cli.contains("other_func"));
}

#[test]
fn test_cli_fuzzy_empty_token_shell_quote_diagnostic() {
    // When user types `grx fz:` (which happens when shell strips quotes), grx outputs helpful diagnostic
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args(["fz:"])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("fuzzy search pattern received an empty argument"));

    // When user types `fz:,from,ptr` (which happens when shell strips quotes)
    let output2 = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args(["fz:,from,ptr"])
        .output()
        .expect("Failed to execute grx binary");

    assert_eq!(output2.status.code(), Some(2));
    let stderr2 = String::from_utf8_lossy(&output2.stderr);
    assert!(stderr2.contains("fuzzy search pattern received an empty token"));
}

fn normalize_lines(stdout: &[u8]) -> Vec<String> {
    let s = String::from_utf8_lossy(stdout);
    let mut lines: Vec<String> = s
        .lines()
        .map(|l| {
            l.trim()
                .trim_start_matches("./")
                .trim_start_matches(".\\")
                .replace('\\', "/")
        })
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
        .args(["in:report"])
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
        .args(["in:report", "t:pdf"])
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
        .args(["in:report", "p:Documents/"])
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
        .args(["annual revenue", "in:report", "t:pdf"])
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
        .args(["in:something_something"])
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
        .args(["in:report", "in:2026"])
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
        .args(["in:=report.md", "p:Documents/"])
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
        .args(["p:src/", "t:rs"])
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
        .args(["t:rs", "fn", "AND", "main", "p:src/"])
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
        .args(["needle", "p:p:path"])
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
        .args(["kind:dir", "in:cache"])
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
        .args(["kind:dir", "p:src/", "d:1"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_d1.status.code(), Some(0));
    assert_eq!(normalize_lines(&out_d1.stdout), vec!["src/nested/"]);

    let out_d2 = std::process::Command::new(grx_bin)
        .args(["kind:dir", "p:src/", "d:2"])
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
        .args(["kind:link", "p:."])
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
        .args(["p:src/", "t:rs", "newer:7d"])
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
        .args(["kind:dir", "in:build", "np:vendor/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(normalize_lines(&out.stdout), vec!["build/"]);

    // Name pattern language: ^ anchor, $ anchor, .. sequence
    let out_anchor_start = std::process::Command::new(grx_bin)
        .args(["in:^report"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_anchor_start.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out_anchor_start.stdout),
        vec!["Documents/report.md"]
    );

    let out_anchor_end = std::process::Command::new(grx_bin)
        .args(["in:pdf$"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_anchor_end.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out_anchor_end.stdout),
        vec!["Documents/annual_report_2026.pdf"]
    );

    let out_seq = std::process::Command::new(grx_bin)
        .args(["in:annual..pdf"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_seq.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out_seq.stdout),
        vec!["Documents/annual_report_2026.pdf"]
    );

    // Directory named rs does not affect t:rs file selection
    let out_trs = std::process::Command::new(grx_bin)
        .args(["t:rs"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_trs.status.code(), Some(0));
    let trs_files = normalize_lines(&out_trs.stdout);
    assert!(!trs_files.iter().any(|f| f.starts_with("rs/")));

    // Bare invocation: exits 2 unconditionally, even when stdin is redirected
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

    // Quiet mode (-q) in discovery
    let out_q_match = std::process::Command::new(grx_bin)
        .args(["-q", "in:report"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_q_match.status.code(), Some(0));
    assert!(out_q_match.stdout.is_empty());

    let out_q_nomatch = std::process::Command::new(grx_bin)
        .args(["-q", "in:nonexistent_pattern"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_q_nomatch.status.code(), Some(1));
    assert!(out_q_nomatch.stdout.is_empty());

    // NUL delimiter (-0)
    let out_nul = std::process::Command::new(grx_bin)
        .args(["-0", "in:report", "p:Documents/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_nul.status.code(), Some(0));
    assert!(out_nul.stdout.contains(&0u8));
    assert_eq!(
        normalize_nul(&out_nul.stdout),
        vec!["Documents/annual_report_2026.pdf", "Documents/report.md"]
    );

    // Mutual exclusivity and validation errors: exit code 2
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

    // Bare pattern under kind:dir is promoted to directory name filter
    let ok_dir_bare = std::process::Command::new(grx_bin)
        .args(["kind:dir", "nested"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(ok_dir_bare.status.code(), Some(0));
    assert_eq!(normalize_lines(&ok_dir_bare.stdout), vec!["src/nested/"]);

    let err_stdin_discovery = std::process::Command::new(grx_bin)
        .args(["in:report", "-"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(err_stdin_discovery.status.code(), Some(2));
    let err_stderr = String::from_utf8_lossy(&err_stdin_discovery.stderr);
    assert!(err_stderr.contains("stdin is not a directory inventory"));

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

    // Selection parity: discovery candidates vs content search candidates
    let disc_parity = std::process::Command::new(grx_bin)
        .args(["p:src/", "t:rs"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    let content_parity = std::process::Command::new(grx_bin)
        .args(["-l", "GRX_MARKER_TEST", "p:src/", "t:rs"])
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
        .args(["--color=always", "in:=report.md", "p:Documents/"])
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
        .args(["--color=always", "kind:dir", "in:cache"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_dir_color.status.code(), Some(0));
    let dir_color_stdout = String::from_utf8_lossy(&out_dir_color.stdout);
    assert!(dir_color_stdout.contains("\x1b[30;48;5;220mcache\x1b[0m\x1b[38;5;81m/\x1b[0m"));

    let out_links = std::process::Command::new(grx_bin)
        .args(["--hyperlinks=always", "in:report", "p:Documents/"])
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
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out_slash_warn = std::process::Command::new(grx_bin)
        .args(["in:src/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    let slash_stderr = String::from_utf8_lossy(&out_slash_warn.stderr);
    assert!(slash_stderr.contains(
        "grx warning: 'in:src/' contains a path separator; 'in:' matches file basenames only"
    ));
    assert!(slash_stderr.contains("Did you mean 'p:src/' for directories?"));

    let out_ni_warn = std::process::Command::new(grx_bin)
        .args(["ni:some/dir"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    let ni_stderr = String::from_utf8_lossy(&out_ni_warn.stderr);
    assert!(ni_stderr.contains("grx warning: 'ni:some/dir' contains a path separator"));
    assert!(ni_stderr.contains("Did you mean 'np:some/dir' for directories?"));
}

#[test]
fn test_acceptance_case_19_dir_and_file_exact_selectors() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out_dir_src = std::process::Command::new(grx_bin)
        .args(["kind:dir", "in:src", "np:data"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_dir_src.status.code(), Some(0));
    assert_eq!(normalize_lines(&out_dir_src.stdout), vec!["src/"]);

    let out_dir_exact_tail = std::process::Command::new(grx_bin)
        .args(["--color=always", "kind:dir", "in:=src", "tail:5"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_dir_exact_tail.status.code(), Some(0));
    let dir_tail_str = String::from_utf8_lossy(&out_dir_exact_tail.stdout);
    assert!(dir_tail_str.contains("\x1b[30;48;5;220msrc\x1b[0m\x1b[38;5;81m/\x1b[0m"));

    let out_file_main = std::process::Command::new(grx_bin)
        .args(["kind:file", "in:=main.rs"])
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

    let files_by_content = run(&["-l", "needle", "kind:file"]);
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

    let binaries_by_content = run(&["-l", "needle", "kind:bin"]);
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

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("body_only.txt", root.join("needle_link")).unwrap();
        let links_by_name = run(&["kind:link", "needle"]);
        assert_eq!(links_by_name.status.code(), Some(0));
        assert_eq!(normalize_lines(&links_by_name.stdout), vec!["needle_link"]);

        let invalid_link_content = run(&["needle", "kind:link"]);
        assert_eq!(invalid_link_content.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&invalid_link_content.stderr).contains("content search cannot")
        );
    }

    let invalid_dir_content = run(&["needle", "kind:dir"]);
    assert_eq!(invalid_dir_content.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&invalid_dir_content.stderr).contains("content search cannot"));

    let boolean_name = run(&["kind:file", "needle", "OR", "body"]);
    assert_eq!(boolean_name.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&boolean_name.stderr)
            .contains("Put the content pattern before kind:")
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

    let selectors = run(&["--color=always", "in:bump", "in:ion", "kind:file"]);
    assert_eq!(selectors.status.code(), Some(0));
    let selectors_text = String::from_utf8_lossy(&selectors.stdout);
    assert!(selectors_text.contains("\x1b[30;48;5;220mbump\x1b[0m"));
    assert!(selectors_text.contains("\x1b[30;48;5;220mion\x1b[0m"));

    let linked = run(&["--color=always", "--hyperlinks=always", "kind:file", "rs"]);
    assert_eq!(linked.status.code(), Some(0));
    let linked_text = String::from_utf8_lossy(&linked.stdout);
    assert!(linked_text.contains("\x1b]8;;file://"));
    assert!(linked_text.contains("\x1b[30;48;5;220mrs\x1b[0m"));

    let content = run(&["--color=always", "rs", "kind:file"]);
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

    let out_m_tail = std::process::Command::new(grx_bin)
        .args(["-m", "5", "tail:5", "pattern"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_m_tail.status.code(), Some(2));
}

#[test]
fn test_acceptance_case_21_sort_keys_in_discovery_and_content() {
    let tmp = setup_unified_acceptance_corpus();
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out_sort_size = std::process::Command::new(grx_bin)
        .args(["p:logs/", "sort:size"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_sort_size.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_sort_size.stdout),
        vec!["15  logs/small.log", "11M  logs/app.log"]
    );

    let out_sort_size_desc = std::process::Command::new(grx_bin)
        .args(["p:logs/", "sort:-size"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_sort_size_desc.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_sort_size_desc.stdout),
        vec!["11M  logs/app.log", "15  logs/small.log"]
    );

    let out_sort_largest = std::process::Command::new(grx_bin)
        .args(["p:logs/", "sort:largest"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_sort_largest.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_sort_largest.stdout),
        vec!["11M  logs/app.log", "15  logs/small.log"]
    );

    let out_sort_smallest = std::process::Command::new(grx_bin)
        .args(["p:logs/", "sort:smallest"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_sort_smallest.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_sort_smallest.stdout),
        vec!["15  logs/small.log", "11M  logs/app.log"]
    );

    let out_cli_sort_rev = std::process::Command::new(grx_bin)
        .args(["--sort", "size", "--reverse", "p:logs/"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_cli_sort_rev.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_cli_sort_rev.stdout),
        vec!["11M  logs/app.log", "15  logs/small.log"]
    );

    let out_sort_len = std::process::Command::new(grx_bin)
        .args(["p:logs/", "sort:len"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_sort_len.status.code(), Some(0));
    assert_eq!(
        raw_lines(&out_sort_len.stdout),
        vec!["12  logs/app.log", "14  logs/small.log"]
    );

    // Content sort by line length
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
            "p:lines.txt",
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
            "p:lines.txt",
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
        .args(["p:bulk/", "sort:path", "tail:3"])
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
        .args(["p:collision/", "kind:dir", "in:=shared_name"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_collision_dir.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out_collision_dir.stdout),
        vec!["collision/shared_name/"]
    );

    let out_collision_file = std::process::Command::new(grx_bin)
        .args(["p:collision/", "kind:file", "in:=regular_file"])
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
        .args(["kind:dir", "in:src"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_dir_slash.status.code(), Some(0));
    assert_eq!(normalize_lines(&out_dir_slash.stdout), vec!["src/"]);

    // Deprecated dir:src/ returns error
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
    let tmp_path = tmp.path();
    let grx_bin = env!("CARGO_BIN_EXE_grx");

    let out_trailing_i = std::process::Command::new(grx_bin)
        .args(["fn", "src/main.rs", "-i"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_trailing_i.status.code(), Some(2));
    let trailing_err = String::from_utf8_lossy(&out_trailing_i.stderr);
    assert!(trailing_err.contains("CLI flag '-i' was placed after positional search arguments"));

    let out_trailing_sort = std::process::Command::new(grx_bin)
        .args(["kind:file", "in:=main.rs", "--sort=size"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_trailing_sort.status.code(), Some(0));
    assert_eq!(
        normalize_lines(&out_trailing_sort.stdout),
        vec!["56  src/main.rs"]
    );
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
        .args(["--no-heading", "-N", "needle", "p:density/", "sort:count"])
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
        .args(["--stats", "--no-heading", "needle", "p:density/", "tail:2"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_stats_tail.status.code(), Some(0));
    let stats_out = String::from_utf8_lossy(&out_stats_tail.stdout);
    assert!(stats_out.contains("2 matches"));
    assert!(stats_out.contains("2 matched lines"));
}

#[test]
fn test_engine_compositional_correctness() {
    use std::io::Write;
    let grx_bin = env!("CARGO_BIN_EXE_grx");
    let tmp = tempfile::tempdir().unwrap();
    let tmp_path = tmp.path();

    // Case 1: Sorting and tail with stdin input
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

    // Case 2: Buffered limits with context lines
    {
        let ctx_file = tmp_path.join("context.txt");
        fs::write(&ctx_file, b"before\nhit\nafter\n").unwrap();

        // tail:1 should retain the match line 'hit' and its after context 'after'
        let out_tail1 = std::process::Command::new(grx_bin)
            .args([
                "--no-heading",
                "-N",
                "-A",
                "1",
                "hit",
                "p:context.txt",
                "tail:1",
            ])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_tail1.status.code(), Some(0));
        let text1 = String::from_utf8_lossy(&out_tail1.stdout);
        assert!(text1.contains("hit"));
        assert!(text1.contains("after"));

        // head:1 with sort:path should retain 'before' and 'hit'
        let out_head1 = std::process::Command::new(grx_bin)
            .args([
                "--no-heading",
                "-N",
                "-B",
                "1",
                "hit",
                "p:context.txt",
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

        // tail:0 should exit 1 without records
        let out_tail0 = std::process::Command::new(grx_bin)
            .args(["--no-heading", "-N", "hit", "p:context.txt", "tail:0"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_tail0.status.code(), Some(1));
        assert!(out_tail0.stdout.is_empty());
    }

    // Case 3: -m per-file limit preserved under sorting
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
                "p:a.txt",
                "p:b.txt",
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

    // Case 4: Empty pattern file remains a content search request (never discovery)
    {
        let empty_pat = tmp_path.join("empty.patterns");
        fs::write(&empty_pat, b"").unwrap();

        let out_empty_pat = std::process::Command::new(grx_bin)
            .args(["-f", "empty.patterns", "p:."])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_empty_pat.status.code(), Some(1));
        assert!(out_empty_pat.stdout.is_empty());
    }

    // Case 5: Unified discovery emits file symlinks at most once
    #[cfg(unix)]
    {
        let target_file = tmp_path.join("target_sym.txt");
        fs::write(&target_file, b"content").unwrap();
        let link_file = tmp_path.join("link_sym.txt");
        let _ = fs::remove_file(&link_file);
        std::os::unix::fs::symlink("target_sym.txt", &link_file).unwrap();

        let out_sym = std::process::Command::new(grx_bin)
            .args(["in:link_sym"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_sym.status.code(), Some(0));
        let lines = raw_lines(&out_sym.stdout);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("link_sym.txt"));
    }

    // Case 6: CLI case flags reach filename predicates
    #[cfg(unix)]
    {
        let rep_dir = tmp_path.join("case_test");
        fs::create_dir_all(&rep_dir).unwrap();
        fs::write(rep_dir.join("report.txt"), b"").unwrap();
        fs::write(rep_dir.join("REPORT.txt"), b"").unwrap();

        // -i matches both report.txt and REPORT.txt
        let out_i = std::process::Command::new(grx_bin)
            .args(["-i", "in:REPORT", "kind:file", "p:case_test/"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_i.status.code(), Some(0));
        let lines_i = raw_lines(&out_i.stdout);
        assert_eq!(lines_i.len(), 2);

        // -s matches only report.txt
        let out_s = std::process::Command::new(grx_bin)
            .args(["-s", "in:report", "kind:file", "p:case_test/"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_s.status.code(), Some(0));
        let lines_s = raw_lines(&out_s.stdout);
        assert_eq!(lines_s.len(), 1);
        assert!(lines_s[0].contains("report.txt"));
    }

    // Case 7: Trailing CLI flags rejected with exit code 2
    {
        let flags_file = tmp_path.join("flags.txt");
        fs::write(&flags_file, b"hit F\nhit iv\nhit other\n").unwrap();

        let out_f = std::process::Command::new(grx_bin)
            .args(["hit", "-F", "p:flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_f.status.code(), Some(2));
        let err_f = String::from_utf8_lossy(&out_f.stderr);
        assert!(err_f.contains("CLI flag '-F' was placed after positional search arguments"));

        let out_iv = std::process::Command::new(grx_bin)
            .args(["hit", "-iv", "p:flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_iv.status.code(), Some(2));
        let err_iv = String::from_utf8_lossy(&out_iv.stderr);
        assert!(err_iv.contains("CLI flag '-iv' was placed after positional search arguments"));
    }

    // Case 8: Buffered output broken-pipe clean exit 0
    {
        use std::io::Read;
        let mut child = std::process::Command::new(grx_bin)
            .args(["in:txt", "sort:path"])
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

    // Case 9: Discovery limits produce accurate counters and exit 1 on head:0
    {
        let disc_dir = tmp_path.join("disc_limits");
        fs::create_dir_all(&disc_dir).unwrap();
        for i in 0..5 {
            fs::write(disc_dir.join(format!("file_{i}.txt")), b"").unwrap();
        }

        let out_stats = std::process::Command::new(grx_bin)
            .args(["--stats", "in:txt", "p:disc_limits/", "head:1"])
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
            .args(["in:txt", "p:disc_limits/", "head:0"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_zero.status.code(), Some(1));
        assert!(out_zero.stdout.is_empty());
    }

    // Case 10: Styling does not corrupt non-UTF-8 paths in redirected text
    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let non_utf8_dir = tmp_path.join("non_utf8");
        fs::create_dir_all(&non_utf8_dir).unwrap();
        let bad_path = non_utf8_dir.join(OsStr::from_bytes(b"bad-\xff.txt"));
        fs::write(&bad_path, b"content").unwrap();

        let out_raw = std::process::Command::new(grx_bin)
            .args(["in:bad", "p:non_utf8/"])
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

    // Case 11: Broken symlink root preflight does not fail with nonexistent root error
    #[cfg(unix)]
    {
        let broken_link = tmp_path.join("broken_link");
        let _ = fs::remove_file(&broken_link);
        std::os::unix::fs::symlink("nonexistent_target", &broken_link).unwrap();

        let out_broken = std::process::Command::new(grx_bin)
            .args(["kind:link", "p:broken_link"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_broken.status.code(), Some(0));
        let stderr = String::from_utf8_lossy(&out_broken.stderr);
        assert!(!stderr.contains("No such file or directory"));
        let stdout = String::from_utf8_lossy(&out_broken.stdout);
        assert!(stdout.contains("broken_link"));
    }

    // Case 12: Limits apply to files-without-match output
    {
        let _empty_pat = tmp_path.join("empty.patterns");
        let out_l_tail0 = std::process::Command::new(grx_bin)
            .args(["-L", "hit", "p:empty.patterns", "tail:0"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_l_tail0.status.code(), Some(1));
        assert!(out_l_tail0.stdout.is_empty());

        let out_l_tail1 = std::process::Command::new(grx_bin)
            .args(["-L", "hit", "p:empty.patterns", "tail:1"])
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

    // Case 1: Empty pattern files lose positional targets
    // -f empty.pat missing.txt must fail with exit code 2 (not search stdin or current dir)
    // -f empty.pat yes.txt must search yes.txt and exit 1 (0 matches)
    {
        let empty_pat = tmp_path.join("empty.pat");
        fs::write(&empty_pat, b"").unwrap();
        let yes_file = tmp_path.join("yes.txt");
        fs::write(&yes_file, b"hello world\n").unwrap();

        // Target existing file: exits 1 (no matches in yes.txt)
        let out_existing = std::process::Command::new(grx_bin)
            .args(["-f", "empty.pat", "yes.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_existing.status.code(), Some(1));
        assert!(out_existing.stdout.is_empty());

        // Target nonexistent file: exits 2 (usage/root validation error)
        let out_missing = std::process::Command::new(grx_bin)
            .args(["-f", "empty.pat", "missing.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_missing.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&out_missing.stderr);
        assert!(stderr.contains("missing.txt") || stderr.contains("No such file or directory"));
    }

    // Case 2: Empty pattern sets ignore inversion (-v -f empty.pat)
    // Inverted empty pattern matches every line
    {
        let out_v = std::process::Command::new(grx_bin)
            .args(["--no-heading", "-N", "-v", "-f", "empty.pat", "yes.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_v.status.code(), Some(0));
        let text_v = String::from_utf8_lossy(&out_v.stdout);
        assert!(text_v.contains("hello world"));

        let out_cv = std::process::Command::new(grx_bin)
            .args(["-c", "-v", "-f", "empty.pat", "yes.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_cv.status.code(), Some(0));
        let text_cv = String::from_utf8_lossy(&out_cv.stdout);
        assert!(text_cv.contains('1'));
    }

    // Case 3: Limited context omits nearby matching lines
    // Unselected matches within retained context window must become context lines
    {
        let ctx_file = tmp_path.join("ctx_adjacent.txt");
        fs::write(
            &ctx_file,
            b"hit line 1\nhit line 2\nhit line 3\nnormal line 4\n",
        )
        .unwrap();

        // head:1 with -A 1 keeps match 1 and demotes match 2 to after-context
        let out_adjacent_head = std::process::Command::new(grx_bin)
            .args([
                "--no-heading",
                "-n",
                "-A",
                "1",
                "hit",
                "p:ctx_adjacent.txt",
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

        // tail:1 with -B 1 keeps match 3 and demotes match 2 to before-context
        let out_adjacent_tail = std::process::Command::new(grx_bin)
            .args([
                "--no-heading",
                "-n",
                "-B",
                "1",
                "hit",
                "p:ctx_adjacent.txt",
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

        // Overlapping window: head:1 with -A 3 retains line 1 as match and lines 2, 3, 4 as context
        let out_overlap = std::process::Command::new(grx_bin)
            .args([
                "--no-heading",
                "-n",
                "-A",
                "3",
                "hit",
                "p:ctx_adjacent.txt",
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

    // Case 4: Attached short options silently become negative search terms
    // Flags like -m1, -C2 placed after pattern must be rejected with exit code 2
    // Explicit negative terms like ns:m1 or -@m1 remain valid
    {
        let search_file = tmp_path.join("search_flags.txt");
        fs::write(&search_file, b"alpha beta\n").unwrap();

        let out_attached_m = std::process::Command::new(grx_bin)
            .args(["alpha", "-m1", "p:search_flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_attached_m.status.code(), Some(2));
        let err_m = String::from_utf8_lossy(&out_attached_m.stderr);
        assert!(err_m.contains("CLI flag '-m1' was placed after positional search arguments"));

        let out_attached_c = std::process::Command::new(grx_bin)
            .args(["alpha", "-C2", "p:search_flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_attached_c.status.code(), Some(2));

        let out_eq_flag = std::process::Command::new(grx_bin)
            .args(["alpha", "-t=rs", "p:search_flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_eq_flag.status.code(), Some(2));

        // Explicit negative namespace ns:m1 or ns:@m1 is allowed and matches
        let out_neg_namespace = std::process::Command::new(grx_bin)
            .args(["alpha", "ns:m1", "p:search_flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_neg_namespace.status.code(), Some(0));

        let out_neg_at = std::process::Command::new(grx_bin)
            .args(["alpha", "ns:@m1", "p:search_flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_neg_at.status.code(), Some(0));

        // Leading -@m1 is deprecated to avoid CLI flag confusion
        let out_neg_literal = std::process::Command::new(grx_bin)
            .args(["alpha", "-@m1", "p:search_flags.txt"])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_neg_literal.status.code(), Some(2));
    }

    // Case 5: Symlink depth regression (d:0 drops root-level file symlinks)
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
            .args(["d:0", "p:sym_depth_dir/"])
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

    // Case 6: Filename limits fabricate statistics
    // Limited -L search must report 0 content matches and truthful statistics
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
                "p:nomatch1.txt",
                "p:nomatch2.txt",
                "sort:path",
                "head:1",
            ])
            .current_dir(tmp_path)
            .output()
            .unwrap();
        assert_eq!(out_l_stats.status.code(), Some(0));
        let stats_output = String::from_utf8_lossy(&out_l_stats.stdout);
        // Only 1 file should be listed before stats
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

        // Content match counters report 0
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
            stats_output.contains("2 files searched"),
            "must report 2 files searched"
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

    // 1. sort:modified emits 12-char eza timestamp followed by 2 spaces and path
    let out_mod = std::process::Command::new(grx_bin)
        .args(["p:sample.txt", "sort:modified"])
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

    // 2. sort:size emits 5-char eza size followed by 2 spaces and path
    let out_size = std::process::Command::new(grx_bin)
        .args(["p:sample.txt", "sort:size"])
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

    // 3. -0 / --null suppresses column prefix to preserve raw paths for xargs -0
    let out_null = std::process::Command::new(grx_bin)
        .args(["-0", "p:sample.txt", "sort:size"])
        .current_dir(tmp_path)
        .output()
        .unwrap();
    assert_eq!(out_null.status.code(), Some(0));
    assert_eq!(
        out_null.stdout, b"sample.txt\0",
        "null mode must emit pure null-terminated paths without sort columns"
    );

    // 4. Default unsorted search does not display columns
    let out_unsorted = std::process::Command::new(grx_bin)
        .args(["p:sample.txt"])
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

    // 5. Completion files generation verification
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let comp_dir = manifest_dir.join("completions");

    let fish_path = comp_dir.join("grx.fish");
    let bash_path = comp_dir.join("grx.bash");
    let zsh_path = comp_dir.join("_grx");

    assert!(fish_path.exists(), "completions/grx.fish must exist");
    assert!(bash_path.exists(), "completions/grx.bash must exist");
    assert!(zsh_path.exists(), "completions/_grx must exist");

    let fish_content = fs::read_to_string(&fish_path).unwrap();
    assert!(fish_content.contains("sort:modified"));
    assert!(fish_content.contains("sort:size"));
    assert!(fish_content.contains("-l sort"));

    let bash_content = fs::read_to_string(&bash_path).unwrap();
    assert!(bash_content.contains("--sort"));
    assert!(bash_content.contains("sort:*"));

    let zsh_content = fs::read_to_string(&zsh_path).unwrap();
    assert!(zsh_content.contains("--sort"));
    assert!(zsh_content.contains("size bytes"));

    // 6. Unsupported shell name must fail with exit code 2 and helpful stderr diagnostic
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

    // Create a binary file with null bytes
    let bin_file = tmp_path.join("data.bin");
    let mut bin_data = b"preamble \x00 magic_token \x00 postscript\n".to_vec();
    bin_data.extend(vec![0u8; 100]);
    fs::write(&bin_file, &bin_data).unwrap();

    // By default, binary file is skipped when searched for magic_token
    let out_default = std::process::Command::new(grx_bin)
        .args(["magic_token", tmp_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(
        out_default.status.code(),
        Some(1),
        "Default should skip binary file"
    );

    // Write a config with binary-handling = "search" and buffer-size-bytes = 32768
    let config_file = tmp_path.join("custom_config.toml");
    let config_toml = r#"
[search]
binary-handling = "search"
buffer-size-bytes = 32768
"#;
    fs::write(&config_file, config_toml).unwrap();

    let out_search = std::process::Command::new(grx_bin)
        .args([
            "--config",
            config_file.to_str().unwrap(),
            "magic_token",
            tmp_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        out_search.status.code(),
        Some(0),
        "Configured binary-handling = 'search' should search binary file"
    );

    // Test custom binary-null-probe-bytes and walker-buffer-size-bytes:
    // Create a file with 100 bytes of text, then a null byte, then the search token.
    let probe_file = tmp_path.join("probe_test.txt");
    let mut probe_data = vec![b'a'; 100];
    probe_data.push(0x00);
    probe_data.extend_from_slice(b" probe_token\n");
    fs::write(&probe_file, &probe_data).unwrap();

    // With binary-null-probe-bytes = 50 (smaller than null byte offset at 100),
    // the probe won't find the null byte in the first 50 bytes, so it treats it as text and finds probe_token!
    let probe_config_file = tmp_path.join("probe_config.toml");
    let probe_config_toml = r#"
[search]
binary-null-probe-bytes = 50
walker-buffer-size-bytes = 32768
"#;
    fs::write(&probe_config_file, probe_config_toml).unwrap();

    let out_probe = std::process::Command::new(grx_bin)
        .args([
            "--config",
            probe_config_file.to_str().unwrap(),
            "probe_token",
            probe_file.to_str().unwrap(),
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
fn test_traversal_skips_unreadable_subdirectories_without_aborting() {
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
        .args(["content_token_here", root.to_str().unwrap()])
        .output()
        .expect("Failed to execute grx binary");

    let mut restore_perms = fs::metadata(&unreadable_dir).unwrap().permissions();
    restore_perms.set_mode(0o755);
    let _ = fs::set_permissions(&unreadable_dir, restore_perms);

    assert_eq!(
        out.status.code(),
        Some(0),
        "Search should succeed despite unreadable sibling dir"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("content_token_here"));
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

    // 1. in:special positional sub
    let out_in = std::process::Command::new(grx_bin)
        .args(["in:special", sub.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out_in.status.code(), Some(0));
    let stdout_in = String::from_utf8_lossy(&out_in.stdout);
    assert!(stdout_in.contains("special_target.rs"));

    // 2. -t rs positional sub
    let out_t = std::process::Command::new(grx_bin)
        .args(["-t", "rs", sub.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out_t.status.code(), Some(0));
    let stdout_t = String::from_utf8_lossy(&out_t.stdout);
    assert!(stdout_t.contains("special_target.rs"));
    assert!(!stdout_t.contains("notes.txt"));

    // 3. -g "*target*" positional sub
    let out_g = std::process::Command::new(grx_bin)
        .args(["-g", "*target*", sub.to_str().unwrap()])
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
        .args(["-v", "--stats", "skip", inv_file.to_str().unwrap()])
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
        .args(["find_me_in_symlink", link_file.to_str().unwrap()])
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
        .args(["content", git_dir.to_str().unwrap()])
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
        .args(["content", git_dir.to_str().unwrap()])
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
    let grx_bin = env!("CARGO_BIN_EXE_grx");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let code_dir = root.join("code");
    fs::create_dir(&code_dir).unwrap();
    let code_file = code_dir.join("main.rs");
    fs::write(&code_file, b"use str::from_utf8;\n").unwrap();
    let out_str = std::process::Command::new(grx_bin)
        .args(["str::from_utf8", code_dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out_str.status.code(), Some(0));
    let stdout_str = String::from_utf8_lossy(&out_str.stdout);
    assert!(stdout_str.contains("str::from_utf8"));

    let git_dir = root.join("repo");
    fs::create_dir(&git_dir).unwrap();
    let sub = git_dir.join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("other.txt"), b"kept content\n").unwrap();

    // Canonical directory exclusion np:sub/
    let out_excl = std::process::Command::new(grx_bin)
        .args(["content", "np:sub/", git_dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out_excl.status.code(), Some(1));

    // Deprecated exclamation directory exclusion !sub/ returns error
    let out_deprecated_excl = std::process::Command::new(grx_bin)
        .args(["content", "!sub/", git_dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out_deprecated_excl.status.code(), Some(2));
}

#[test]
fn test_binary_string_head_limits_logical_records() {
    let tmp = tempdir().unwrap();
    let file = tmp.path().join("strings.bin");
    fs::write(&file, b"hit-one\0hit-two\0hit-three\0").unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--no-heading",
            "--color",
            "never",
            "hit",
            "str:4",
            "head:2",
            file.to_str().unwrap(),
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
            .args(["-q", pattern, filter, file.to_str().unwrap()])
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
    let file = tmp.path().join("fuzzy-partials.txt");
    fs::write(
        &file,
        b"complete alpha beta gamma\npartial-two alpha beta\npartial-one gamma\nunrelated\n",
    )
    .unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args([
            "--no-heading",
            "--color",
            "never",
            "fz:alpha,beta,gamma",
            file.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("complete alpha beta gamma"));
    assert!(!stdout.contains("partial-two"));
    assert!(!stdout.contains("partial-one"));
}

#[test]
fn test_boolean_or_executes_for_dsl_and_repeated_regexp_flags() {
    let tmp = tempdir().unwrap();
    let file = tmp.path().join("boolean-or.txt");
    fs::write(&file, b"alpha only\nbeta only\ngamma only\n").unwrap();

    for args in [
        vec!["alpha", "OR", "beta", file.to_str().unwrap()],
        vec!["-e", "alpha", "-e", "beta", file.to_str().unwrap()],
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
        .args(["-v", "needle", file.to_str().unwrap()])
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
    let file = tmp.path().join("sample.rs");
    fs::write(&file, b"fn main() {}\nlet x = \"ext:rs\";\n").unwrap();

    // Typo prefix triggers exit code 2 and explicit suggestion on stderr
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args(["main", "ext:rs", file.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unrecognized filter prefix 'ext:'"));
    assert!(stderr.contains("Did you mean 't:rs'"));

    // Quoted prefix searches for literal text and succeeds without error
    let output_quoted = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .args(["\"ext:rs\"", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output_quoted.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output_quoted.stdout);
    assert!(stdout.contains("let x = \"ext:rs\";"));

    // Help output contains documented regex syntax
    let help_output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .arg("--help")
        .output()
        .unwrap();
    assert_eq!(help_output.status.code(), Some(0));
    let help_text = String::from_utf8_lossy(&help_output.stdout);
    assert!(help_text.contains("/regex/, re:<regex>"));
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

    // grx kind:dir in:cpp d:1 mv:cpp-projects/
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["kind:dir", "in:cpp", "d:1", "mv:cpp-projects/"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));

    // Verify directories moved into cpp-projects/
    assert!(!d1.exists());
    assert!(!d2.exists());
    assert!(!d3.exists());
    assert!(dest.join("hellocpp").exists());
    assert!(dest.join("hellocpp/hello.cpp").exists());
    assert!(dest.join("xmake_cpp_learn").exists());
    assert!(dest.join("cmake-cpp-learn").exists());
    // Dest directory was not moved into itself
    assert!(dest.exists());

    // grx undo
    let undo_status = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .arg("undo")
        .status()
        .unwrap();
    assert_eq!(undo_status.code(), Some(0));

    // Verify original state restored
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

    // 1. Dry run preview does not create backup directory or copy files
    let dry_output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["in:test", "t:txt", "dry:", "cp:backup/"])
        .output()
        .unwrap();
    assert_eq!(dry_output.status.code(), Some(0));
    assert!(!backup.exists());

    // 2. Real copy
    let cp_status = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["in:test", "t:txt", "cp:backup/"])
        .status()
        .unwrap();
    assert_eq!(cp_status.code(), Some(0));
    assert!(f1.exists());
    assert!(f2.exists());
    assert!(backup.join("test1.txt").exists());
    assert!(backup.join("test2.txt").exists());

    // 3. Revert copy
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

    // Safe remove: stages into trash
    let rm_status = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["in:trashme", "trash:"])
        .status()
        .unwrap();
    assert_eq!(rm_status.code(), Some(0));
    assert!(!f.exists());

    // Undo restores file
    let undo_status = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .arg("undo")
        .status()
        .unwrap();
    assert_eq!(undo_status.code(), Some(0));
    assert!(f.exists());
    assert_eq!(fs::read(&f).unwrap(), b"secret data to recover");

    // Deprecated rm: returns error
    let deprecated_rm_status = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .args(["in:trashme", "rm:"])
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

    // in:file t:txt --exec touch {}.created
    let exec_output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .args(["in:file", "t:txt", "--exec", "touch", "{}.created"])
        .output()
        .unwrap();
    assert_eq!(exec_output.status.code(), Some(0));
    assert!(root.join("file_one.txt.created").exists());
    assert!(root.join("file_two.txt.created").exists());

    // in:created -X rm (batch execute)
    let batch_status = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .args(["in:created", "-X", "rm"])
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

    // Content search without -l rejected
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["TODO", "trash:", "."])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot be run directly on content search matches without -l"));
    assert!(file.exists());

    // Content search with -l succeeds
    let output_l = std::process::Command::new(env!("CARGO_BIN_EXE_grx"))
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["-l", "TODO", "trash:", "."])
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

    // Transaction 1: Move alpha.txt to sub1/
    let out_tx1 = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["in:alpha.txt", "mv:sub1/"])
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

    // Transaction 2: Copy beta.txt to sub2/
    let out_tx2 = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["in:beta.txt", "cp:sub2/"])
        .output()
        .unwrap();
    assert_eq!(out_tx2.status.code(), Some(0));
    assert!(file_b.exists());
    assert!(sub2.join("beta.txt").exists());

    // List transactions via undo --list
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

    // Targeted undo of Transaction 1 by ID
    let out_undo1 = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["undo", &tx1_id])
        .output()
        .unwrap();
    assert_eq!(out_undo1.status.code(), Some(0));
    assert!(file_a.exists());
    assert!(!sub1.join("alpha.txt").exists());
    // Transaction 2 should remain applied
    assert!(sub2.join("beta.txt").exists());

    // Undo remaining Transaction 2
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

    // 1. Without --force, copy detects conflict and fails
    let out_fail = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["in:file1.txt", "np:backup/", "cp:backup/"])
        .output()
        .unwrap();
    assert_eq!(out_fail.status.code(), Some(2));
    assert_eq!(fs::read(&dst_file).unwrap(), b"pre-existing old content\n");

    // 2. With --force, copy overwrites destination
    let out_force = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["--force", "in:file1.txt", "np:backup/", "cp:backup/"])
        .output()
        .unwrap();
    assert_eq!(out_force.status.code(), Some(0));
    assert_eq!(fs::read(&dst_file).unwrap(), b"source new content\n");

    // 3. Undo restores pre-existing old content
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
            "sample.txt",
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

    // 1. Conflict validation: multiple actions must fail with exit code 2
    let conflict_out = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["--move", "d1/", "--copy", "d2/", "needle"])
        .output()
        .unwrap();
    assert_eq!(conflict_out.status.code(), Some(2));
    let err_str = String::from_utf8_lossy(&conflict_out.stderr);
    assert!(err_str.contains("Multiple file actions specified"));

    // 2. --copy flag
    let cp_status = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["--copy", copy_dir.to_str().unwrap(), "-l", "needle", "."])
        .status()
        .unwrap();
    assert_eq!(cp_status.code(), Some(0));
    assert!(f1.exists());
    assert!(copy_dir.join("file1.txt").exists());

    // 3. --move flag with undo
    let mv_status = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["--move", dest_dir.to_str().unwrap(), "-l", "needle", "."])
        .status()
        .unwrap();
    assert_eq!(mv_status.code(), Some(0));
    assert!(!f1.exists());
    assert!(dest_dir.join("file1.txt").exists());

    // Revert move via grx undo
    let undo_status = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .arg("undo")
        .status()
        .unwrap();
    assert_eq!(undo_status.code(), Some(0));
    assert!(f1.exists());
    assert!(!dest_dir.join("file1.txt").exists());

    // 4. --trash flag
    let trash_status = std::process::Command::new(grx_bin)
        .current_dir(root)
        .env("XDG_DATA_HOME", &data_home)
        .args(["--trash", "-l", "needle", "."])
        .status()
        .unwrap();
    assert_eq!(trash_status.code(), Some(0));
    assert!(!f1.exists());

    // 5. --clean-trash flag
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

    // 1. Discovery for binary files: kind:bin
    let out_kind_bin = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["kind:bin", "."])
        .output()
        .unwrap();
    assert_eq!(out_kind_bin.status.code(), Some(0));
    let stdout_bin = String::from_utf8_lossy(&out_kind_bin.stdout);
    assert!(stdout_bin.contains("binary.dat"));
    assert!(!stdout_bin.contains("hello.txt"));

    // 2. Deprecated bin: shorthand returns clear consolidation guidance
    let out_bin_shorthand = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["bin:", "."])
        .output()
        .unwrap();
    assert_eq!(out_bin_shorthand.status.code(), Some(2));
    let err_shorthand = String::from_utf8_lossy(&out_bin_shorthand.stderr);
    assert!(
        err_shorthand.contains("Prefix 'bin:' has been consolidated. Use canonical 'kind:bin'")
    );

    // 3. Discovery for text files: kind:text
    let out_kind_text = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["kind:text", "."])
        .output()
        .unwrap();
    assert_eq!(out_kind_text.status.code(), Some(0));
    let stdout_text = String::from_utf8_lossy(&out_kind_text.stdout);
    assert!(stdout_text.contains("hello.txt"));
    assert!(!stdout_text.contains("binary.dat"));

    // 4. Content search in binary files: grx "needle" kind:bin
    let out_search_bin = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["--no-heading", "-l", "needle", "kind:bin", "."])
        .output()
        .unwrap();
    assert_eq!(out_search_bin.status.code(), Some(0));
    let search_bin_str = String::from_utf8_lossy(&out_search_bin.stdout);
    assert!(search_bin_str.contains("binary.dat"));
    assert!(!search_bin_str.contains("hello.txt"));

    // 5. Content search with deprecated bin: shorthand returns consolidation guidance
    let out_search_bin_shorthand = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["--no-heading", "-l", "needle", "bin:", "."])
        .output()
        .unwrap();
    assert_eq!(out_search_bin_shorthand.status.code(), Some(2));
    let search_err = String::from_utf8_lossy(&out_search_bin_shorthand.stderr);
    assert!(search_err.contains("Prefix 'bin:' has been consolidated. Use canonical 'kind:bin'"));

    // 6. Content search in text files: grx "needle" kind:text
    let out_search_text = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["--no-heading", "-l", "needle", "kind:text", "."])
        .output()
        .unwrap();
    assert_eq!(out_search_text.status.code(), Some(0));
    let search_text_str = String::from_utf8_lossy(&out_search_text.stdout);
    assert!(search_text_str.contains("hello.txt"));
    assert!(!search_text_str.contains("binary.dat"));

    // 7. Search term "bin" matches string "bin" on line
    let code_file = root.join("code.rs");
    fs::write(&code_file, b"let bin = 42;\nlet val = 99;\n").unwrap();

    let out_bin = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["--no-heading", "bin", "."])
        .output()
        .unwrap();
    assert_eq!(out_bin.status.code(), Some(0));
    let bin_str = String::from_utf8_lossy(&out_bin.stdout);
    assert!(bin_str.contains("let bin = 42;"));
    assert!(!bin_str.contains("let val = 99;"));

    // Deprecated +bin returns error
    let out_plus_bin = std::process::Command::new(grx_bin)
        .current_dir(root)
        .args(["--no-heading", "+bin", "."])
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

    // 1. Test p:~/Data/Projects
    let out = std::process::Command::new(grx_bin)
        .env("HOME", mock_home.path())
        .args(["main..fn", "p:~/Data/Projects"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("main_fn"));

    // 2. Test literal ~ search pattern does NOT get expanded to HOME path
    let out_pat = std::process::Command::new(grx_bin)
        .env("HOME", mock_home.path())
        .current_dir(mock_home.path())
        .args(["~"])
        .output()
        .unwrap();
    assert_eq!(out_pat.status.code(), Some(1));
}
