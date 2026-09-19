//! Explicit native-tool modes. Arguments after the mode selector belong to the tool.
use std::ffi::{OsStr, OsString};
use std::io;
use std::process::{Command, ExitStatus};

/// A command backend keeps argument construction separate from process execution.
pub trait CommandBackend {
    fn command(&self) -> Command;

    fn run(&self) -> io::Result<ExitStatus> {
        self.command().status()
    }
}

pub struct CompatibilityCommand<'a> {
    program: &'static str,
    prefix: &'static [&'static str],
    args: &'a [OsString],
}

impl<'a> CompatibilityCommand<'a> {
    /// Only inspect the leading selector; never reinterpret a native pattern or flag.
    pub fn from_args(args: &'a [OsString]) -> Option<Self> {
        let first = args.first()?;
        let (mode, rest) = if first == "--mode" {
            (args.get(1)?.as_os_str(), &args[2..])
        } else {
            let mode = first.to_str()?.strip_prefix("--mode=")?;
            (OsStr::new(mode), &args[1..])
        };
        let (program, prefix): (_, &'static [&'static str]) = match mode.to_str()? {
            "grep" | "posix" => ("grep", &[]),
            "git-grep" => ("git", &["grep"]),
            _ => return None,
        };
        Some(Self {
            program,
            prefix,
            args: rest,
        })
    }

    /// Construct a compatibility command directly for a configured mode name.
    pub fn for_mode(mode_name: &str, args: &'a [OsString]) -> Option<Self> {
        let (program, prefix): (_, &'static [&'static str]) = match mode_name {
            "grep" | "posix" => ("grep", &[]),
            "git-grep" => ("git", &["grep"]),
            _ => return None,
        };
        Some(Self {
            program,
            prefix,
            args,
        })
    }
}

impl CommandBackend for CompatibilityCommand<'_> {
    fn command(&self) -> Command {
        let mut command = Command::new(self.program);
        command.args(self.prefix).args(self.args);
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserve_native_arguments_without_shell_interpretation() {
        for mode in ["grep", "posix", "git-grep"] {
            let args: Vec<OsString> = ["--mode", mode, "-e", "--mode", "--", "file with spaces"]
                .into_iter()
                .map(Into::into)
                .collect();
            let backend = CompatibilityCommand::from_args(&args).unwrap();
            let command = backend.command();
            let actual: Vec<_> = command.get_args().collect();
            let offset = usize::from(mode == "git-grep");
            assert_eq!(
                &actual[offset..],
                args[2..].iter().map(|s| s.as_os_str()).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn leave_dsl_and_nonleading_selectors_to_the_cli() {
        for args in [
            vec!["foo", "--mode", "grep"],
            vec!["--mode", "dsl"],
            vec!["-e", "--mode=grep"],
            vec!["--", "--mode=grep"],
        ] {
            let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
            assert!(CompatibilityCommand::from_args(&args).is_none());
        }
        let args = vec![OsString::from("--mode=grep"), OsString::from("-h")];
        assert!(CompatibilityCommand::from_args(&args).is_some());
    }

    #[test]
    fn for_mode_constructs_valid_backend() {
        let args: Vec<OsString> = vec![OsString::from("-i"), OsString::from("pattern")];
        let backend = CompatibilityCommand::for_mode("grep", &args).unwrap();
        let cmd = backend.command();
        assert_eq!(cmd.get_program(), "grep");
        assert_eq!(
            cmd.get_args().collect::<Vec<_>>(),
            args.iter().map(|s| s.as_os_str()).collect::<Vec<_>>()
        );

        let backend_git = CompatibilityCommand::for_mode("git-grep", &args).unwrap();
        let cmd_git = backend_git.command();
        assert_eq!(cmd_git.get_program(), "git");
        let args_git: Vec<_> = cmd_git.get_args().collect();
        assert_eq!(args_git[0], "grep");
        assert_eq!(
            &args_git[1..],
            args.iter().map(|s| s.as_os_str()).collect::<Vec<_>>()
        );

        assert!(CompatibilityCommand::for_mode("dsl", &args).is_none());
        assert!(CompatibilityCommand::for_mode("unknown", &args).is_none());
    }
}
