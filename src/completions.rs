use std::fs;
use std::io;
use std::path::PathBuf;

const REGISTRY_START: &str = "# BEGIN DSL REGISTRY";
const REGISTRY_END: &str = "# END DSL REGISTRY";

fn with_registry(template: &str, entries: &str) -> String {
    let (before, remaining) = template
        .split_once(REGISTRY_START)
        .expect("completion template registry start");
    let (_, after) = remaining
        .split_once(REGISTRY_END)
        .expect("completion template registry end");
    format!("{before}{REGISTRY_START}\n{entries}{REGISTRY_END}{after}")
}

pub fn generate_fish_completions() -> String {
    let entries: String = crate::syntax::DSL_SYNTAX
        .iter()
        .map(|entry| {
            format!(
                "complete -c grx -a \"{}:\" -d \"{}\"\n",
                entry.prefix, entry.description
            )
        })
        .collect();
    with_registry(include_str!("../completions/grx.fish"), &entries)
}

pub fn generate_bash_completions() -> String {
    let tokens = crate::syntax::DSL_SYNTAX
        .iter()
        .map(|entry| format!("{}:", entry.prefix))
        .collect::<Vec<_>>()
        .join(" ");
    with_registry(
        include_str!("../completions/grx.bash"),
        &format!("    local dsl_tokens=\"{tokens} AND OR NOT undo\"\n    "),
    )
}

pub fn generate_zsh_completions() -> String {
    let entries: String = crate::syntax::DSL_SYNTAX
        .iter()
        .map(|entry| {
            format!(
                "                '{}\\:[{}]'\n",
                entry.prefix, entry.description
            )
        })
        .collect();
    with_registry(
        include_str!("../completions/_grx"),
        &format!(
            "            dsl_tokens=(\n{entries}                'AND[Boolean conjunction]' 'OR[Boolean alternative]' 'NOT[Boolean negation]' 'undo[undo latest action]'\n            )\n            "
        ),
    )
}

pub fn detect_shell(requested: Option<&str>) -> Result<&'static str, String> {
    if let Some(sh) = requested {
        let clean = sh.trim().to_ascii_lowercase();
        if clean.contains("fish") {
            return Ok("fish");
        } else if clean.contains("zsh") {
            return Ok("zsh");
        } else if clean.contains("bash") {
            return Ok("bash");
        } else {
            return Err(format!(
                "Unsupported shell '{sh}'. Supported shells: fish, bash, zsh"
            ));
        }
    }

    if let Ok(shell_env) = std::env::var("SHELL") {
        let clean = shell_env.to_ascii_lowercase();
        if clean.contains("fish") {
            return Ok("fish");
        } else if clean.contains("zsh") {
            return Ok("zsh");
        } else if clean.contains("bash") {
            return Ok("bash");
        }
    }

    Ok("fish")
}

pub fn resolve_completion_path(shell: &str) -> Result<PathBuf, io::Error> {
    match shell {
        "fish" => {
            let config_home = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
                if !xdg.trim().is_empty() {
                    PathBuf::from(xdg.trim())
                } else {
                    fallback_home_config()?
                }
            } else {
                fallback_home_config()?
            };
            Ok(config_home.join("fish/completions/grx.fish"))
        }
        "bash" => {
            let data_home = if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
                if !xdg.trim().is_empty() {
                    PathBuf::from(xdg.trim())
                } else {
                    fallback_home_data()?
                }
            } else {
                fallback_home_data()?
            };
            Ok(data_home.join("bash-completion/completions/grx"))
        }
        "zsh" => {
            if let Ok(zdotdir) = std::env::var("ZDOTDIR")
                && !zdotdir.trim().is_empty()
            {
                return Ok(PathBuf::from(zdotdir.trim()).join(".zfunc/_grx"));
            }
            if let Ok(home) = std::env::var("HOME")
                && !home.trim().is_empty()
            {
                return Ok(PathBuf::from(home.trim()).join(".zfunc/_grx"));
            }
            #[cfg(windows)]
            {
                if let Ok(userprofile) = std::env::var("USERPROFILE")
                    && !userprofile.trim().is_empty()
                {
                    return Ok(PathBuf::from(userprofile.trim()).join(".zfunc/_grx"));
                }
            }
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Could not locate HOME, USERPROFILE, or ZDOTDIR for zsh completions",
            ))
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Unsupported shell '{other}'. Supported shells: fish, bash, zsh"),
        )),
    }
}

fn fallback_home_config() -> Result<PathBuf, io::Error> {
    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return Ok(PathBuf::from(home.trim()).join(".config"));
    }
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA")
            && !appdata.trim().is_empty()
        {
            return Ok(PathBuf::from(appdata.trim()));
        }
        if let Ok(userprofile) = std::env::var("USERPROFILE")
            && !userprofile.trim().is_empty()
        {
            return Ok(PathBuf::from(userprofile.trim()).join(".config"));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "HOME environment variable not set",
    ))
}

fn fallback_home_data() -> Result<PathBuf, io::Error> {
    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return Ok(PathBuf::from(home.trim()).join(".local/share"));
    }
    #[cfg(windows)]
    {
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA")
            && !localappdata.trim().is_empty()
        {
            return Ok(PathBuf::from(localappdata.trim()));
        }
        if let Ok(userprofile) = std::env::var("USERPROFILE")
            && !userprofile.trim().is_empty()
        {
            return Ok(PathBuf::from(userprofile.trim()).join(".local/share"));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "HOME environment variable not set",
    ))
}

pub fn install_completion_script(
    requested_shell: Option<&str>,
) -> Result<(PathBuf, &'static str), io::Error> {
    let shell = detect_shell(requested_shell)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let target_path = resolve_completion_path(shell)?;

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let script = match shell {
        "fish" => generate_fish_completions(),
        "bash" => generate_bash_completions(),
        "zsh" => generate_zsh_completions(),
        _ => unreachable!(),
    };

    fs::write(&target_path, script)?;
    Ok((target_path, shell))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_shell() {
        assert_eq!(detect_shell(Some("fish")), Ok("fish"));
        assert_eq!(detect_shell(Some("/bin/zsh")), Ok("zsh"));
        assert_eq!(detect_shell(Some("bash")), Ok("bash"));
        assert!(detect_shell(Some("invalid_shell")).is_err());
    }
}
