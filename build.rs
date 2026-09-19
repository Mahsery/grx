// build.rs — Injects full SemVer string with build metadata into the compiled binary
// and automatically generates shell completions for fish, bash, and zsh.

#[allow(dead_code)]
#[path = "src/completions.rs"]
mod completions;

fn main() {
    // Re-run if Cargo.toml or completions change
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=src/completions.rs");

    let manifest = std::fs::read_to_string("Cargo.toml").unwrap_or_default();
    let mut version = env!("CARGO_PKG_VERSION").to_string();

    for line in manifest.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("version") || !trimmed.contains('=') {
            continue;
        }
        if let Some(v) = trimmed.split('"').nth(1) {
            version = v.to_string();
            break;
        }
    }

    println!("cargo:rustc-env=GRX_BUILD_VERSION={version}");

    // Automatically generate shell completions into OUT_DIR for packaging systems
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
