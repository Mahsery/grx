#[allow(dead_code)]
#[path = "src/syntax.rs"]
mod syntax;

#[allow(dead_code)]
#[path = "src/completions.rs"]
mod completions;

fn get_build_timestamp() -> String {
    if let Ok(output) = std::process::Command::new("date")
        .args(["+%Y-%m-%d %H:%M:%S"])
        .output()
        && output.status.success()
    {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = now / 86400;
    let secs = now % 86400;
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("{days}d {hours:02}:{minutes:02}:{seconds:02} UTC")
}

fn get_git_commit() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if output.status.success() {
        let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !hash.is_empty() {
            return Some(hash);
        }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");

    let manifest = std::fs::read_to_string("Cargo.toml").unwrap_or_default();
    let mut base_version = env!("CARGO_PKG_VERSION").to_string();

    for line in manifest.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("version") || !trimmed.contains('=') {
            continue;
        }
        if let Some(v) = trimmed.split('"').nth(1) {
            base_version = v.to_string();
            break;
        }
    }

    if let Some((clean, _)) = base_version.split_once('+') {
        base_version = clean.to_string();
    }

    let timestamp = get_build_timestamp();
    let version = if let Some(commit) = get_git_commit() {
        format!("{base_version} ({commit}, built {timestamp})")
    } else {
        format!("{base_version} (built {timestamp})")
    };

    println!("cargo:rustc-env=GRX_BUILD_VERSION={version}");

    if let Some(out_dir) = std::env::var_os("OUT_DIR") {
        let out_path = std::path::Path::new(&out_dir);
        let fish = completions::generate_fish_completions();
        let bash = completions::generate_bash_completions();
        let zsh = completions::generate_zsh_completions();

        let _ = std::fs::write(out_path.join("grx.fish"), &fish);
        let _ = std::fs::write(out_path.join("grx.bash"), &bash);
        let _ = std::fs::write(out_path.join("_grx"), &zsh);
    }
}
