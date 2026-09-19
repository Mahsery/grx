mod compat;

use clap::Parser;
use compat::{CommandBackend, CompatibilityCommand};
use grx::cli::Cli;
use grx::config::Config;
use grx::engine::Engine;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(unix)]
    unsafe {
        // Initialize user environment locale for locale-aware date/time formatting (e.g. eza-style strftime)
        libc::setlocale(libc::LC_TIME, c"".as_ptr());
    }

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if let Some(backend) = CompatibilityCommand::from_args(&args) {
        return run_backend(backend);
    }

    // Initialize diagnostic logger to stderr (RUST_LOG=trace,debug,info,warn,error)
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();

    let cli = Cli::parse();

    let custom_config = cli
        .config
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(Path::new);

    // Check if --config was passed without arguments or --edit-config was requested
    if cli.edit_config || cli.config.as_deref() == Some("") {
        match Config::open_in_editor(custom_config) {
            Ok(code) => return ExitCode::from(code as u8),
            Err(err) => {
                eprintln!("grx error: failed to launch text editor: {err}");
                return ExitCode::from(2);
            }
        }
    }

    let config = if let Some(path) = custom_config {
        match Config::load_explicit(path) {
            Ok(config) => config,
            Err(err) => {
                eprintln!("grx error: {err}");
                return ExitCode::from(2);
            }
        }
    } else {
        Config::load_from_paths(None)
    };

    // If a compatibility mode (grep or git-grep) was configured and not overridden by CLI, dispatch
    let effective_mode = cli.mode.unwrap_or(config.mode);
    if let Some(backend) = match effective_mode {
        grx::config::SearchMode::Grep => CompatibilityCommand::for_mode("grep", &args),
        grx::config::SearchMode::GitGrep => CompatibilityCommand::for_mode("git-grep", &args),
        grx::config::SearchMode::Dsl => None,
    } {
        return run_backend(backend);
    }

    let mut engine = Engine::new(config, cli);
    match engine.run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(err) => {
            eprintln!("grx error: {err}");
            ExitCode::from(2)
        }
    }
}

fn run_backend(backend: CompatibilityCommand) -> ExitCode {
    match backend.run() {
        Ok(status) => {
            #[cfg(unix)]
            let code = {
                use std::os::unix::process::ExitStatusExt;
                status
                    .code()
                    .unwrap_or_else(|| 128 + status.signal().unwrap_or(1))
            };
            #[cfg(not(unix))]
            let code = status.code().unwrap_or(2);
            ExitCode::from(code as u8)
        }
        Err(err) => {
            eprintln!("grx: cannot run compatibility tool: {err}");
            ExitCode::from(2)
        }
    }
}
