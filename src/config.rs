use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum SearchMode {
    #[default]
    Dsl,

    Grep,

    GitGrep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum HyperlinkChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BinaryHandling {
    #[default]
    Skip,
    Search,
    HexDump,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SearchSettings {
    pub smart_case: bool,

    pub fixed_strings: bool,

    pub max_depth: Option<usize>,

    pub follow_symlinks: bool,

    pub hidden: bool,

    pub no_ignore: bool,

    pub binary_detection: bool,

    pub binary_handling: BinaryHandling,

    pub threads: usize,

    pub mmap_threshold_bytes: u64,

    pub max_file_size_bytes: Option<u64>,

    pub buffer_size_bytes: usize,

    pub binary_null_probe_bytes: usize,

    pub walker_buffer_size_bytes: usize,

    pub suppress_errors: bool,

    pub regular_files_only: bool,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            smart_case: true,
            fixed_strings: false,
            max_depth: None,
            follow_symlinks: false,
            hidden: false,
            no_ignore: false,
            binary_detection: true,
            binary_handling: BinaryHandling::Skip,
            threads: 0,
            mmap_threshold_bytes: 64 * 1024,
            max_file_size_bytes: None,
            buffer_size_bytes: 64 * 1024,
            binary_null_probe_bytes: 1024,
            walker_buffer_size_bytes: 64 * 1024,
            suppress_errors: true,
            regular_files_only: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct OutputSettings {
    pub color: ColorChoice,

    pub hyperlinks: HyperlinkChoice,

    pub hyperlink_format: String,

    pub line_numbers: bool,

    pub heading: bool,

    pub context_before: usize,

    pub context_after: usize,

    pub null_separator: bool,

    pub colors: ColorTheme,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ColorTheme {
    pub path: String,

    pub line_number: String,

    pub column: String,

    pub match_highlight: String,

    pub near_match_highlight: String,

    pub name_match_highlight: String,

    pub context: String,
}

impl Default for ColorTheme {
    fn default() -> Self {
        Self {
            path: "\x1b[35m".to_string(),
            line_number: "\x1b[32m".to_string(),
            column: "\x1b[38;5;108m".to_string(),
            match_highlight: "\x1b[1;31m".to_string(),
            near_match_highlight: "\x1b[1;36m".to_string(),
            name_match_highlight: "\x1b[30;48;5;220m".to_string(),
            context: "\x1b[38;5;250m".to_string(),
        }
    }
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            color: ColorChoice::Auto,
            hyperlinks: HyperlinkChoice::Auto,
            hyperlink_format: "file://{host}{path}#{line}:{col}".to_string(),
            line_numbers: true,
            heading: true,
            context_before: 0,
            context_after: 0,
            null_separator: false,
            colors: ColorTheme::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
#[derive(Default)]
pub struct JournalSettings {
    pub enabled: bool,

    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    pub mode: SearchMode,

    pub search: SearchSettings,

    pub output: OutputSettings,

    pub journal: JournalSettings,

    pub types: BTreeMap<String, Vec<String>>,

    pub default_excludes: Vec<String>,
}

impl Config {
    fn load_file(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|err| format!("failed to read config '{}': {err}", path.display()))?;
        let mut cfg = toml::from_str::<Config>(&content)
            .map_err(|err| format!("failed to parse config '{}': {err}", path.display()))?;
        let default_cfg = Self::new_with_defaults();
        for (key, value) in default_cfg.types {
            cfg.types.entry(key).or_insert(value);
        }
        Ok(cfg)
    }

    #[inline]
    pub fn is_known_type(&self, alias: &str) -> bool {
        self.types.contains_key(alias)
    }

    #[inline]
    pub fn expand_type_alias(&self, alias: &str) -> Option<Vec<String>> {
        self.types.get(alias).cloned()
    }

    pub fn new_with_defaults() -> Self {
        const DEFAULT_TYPES: &[(&str, &[&str])] = &[
            ("rs", &["*.rs"]),
            ("rust", &["*.rs"]),
            ("c", &["*.c", "*.h"]),
            ("cpp", &["*.cpp", "*.cc", "*.cxx", "*.hpp", "*.h"]),
            ("c++", &["*.cpp", "*.cc", "*.cxx", "*.hpp", "*.h"]),
            ("py", &["*.py", "*.pyi"]),
            ("python", &["*.py", "*.pyi"]),
            ("go", &["*.go"]),
            ("golang", &["*.go"]),
            ("js", &["*.js", "*.mjs", "*.cjs"]),
            ("javascript", &["*.js", "*.mjs", "*.cjs"]),
            ("ts", &["*.ts", "*.tsx", "*.mts", "*.cts"]),
            ("typescript", &["*.ts", "*.tsx", "*.mts", "*.cts"]),
            ("toml", &["*.toml"]),
            ("json", &["*.json"]),
            ("yaml", &["*.yaml", "*.yml"]),
            ("yml", &["*.yaml", "*.yml"]),
            ("md", &["*.md", "*.markdown"]),
            ("markdown", &["*.md", "*.markdown"]),
            ("sh", &["*.sh", "*.bash", "*.zsh", "*.fish"]),
            ("shell", &["*.sh", "*.bash", "*.zsh", "*.fish"]),
            ("bash", &["*.bash", "*.sh"]),
            ("fish", &["*.fish"]),
            ("zsh", &["*.zsh"]),
            ("java", &["*.java"]),
            ("kotlin", &["*.kt", "*.kts"]),
            ("kt", &["*.kt", "*.kts"]),
            ("zig", &["*.zig"]),
            ("lua", &["*.lua"]),
            ("sql", &["*.sql"]),
            ("html", &["*.html", "*.htm"]),
            ("css", &["*.css", "*.scss", "*.sass", "*.less"]),
            (
                "web",
                &[
                    "*.html", "*.css", "*.scss", "*.js", "*.ts", "*.jsx", "*.tsx", "*.vue",
                    "*.svelte",
                ],
            ),
            (
                "code",
                &[
                    "*.rs", "*.c", "*.h", "*.cpp", "*.hpp", "*.py", "*.go", "*.js", "*.ts",
                    "*.tsx", "*.jsx", "*.java", "*.zig", "*.lua", "*.sh",
                ],
            ),
            (
                "data",
                &[
                    "*.json", "*.yaml", "*.yml", "*.toml", "*.csv", "*.tsv", "*.xml",
                ],
            ),
            ("doc", &["*.md", "*.rst", "*.txt", "*.adoc"]),
        ];

        const DEFAULT_EXCLUDES: &[&str] = &[
            "target/",
            "node_modules/",
            ".git/",
            ".hg/",
            ".svn/",
            "build/",
            "dist/",
            ".idea/",
            ".vscode/",
            ".cache/",
            "Cache/",
            "CachedData/",
        ];

        let types = DEFAULT_TYPES
            .iter()
            .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
            .collect();

        let default_excludes = DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect();

        Self {
            mode: SearchMode::Dsl,
            search: SearchSettings::default(),
            output: OutputSettings::default(),
            journal: JournalSettings::default(),
            types,
            default_excludes,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new_with_defaults()
    }
}

impl Config {
    pub fn locate_config_path(custom_path: Option<&Path>) -> Option<PathBuf> {
        if let Some(path) = custom_path {
            let expanded = Self::expand_tilde(path);
            if expanded.is_file() {
                return Some(expanded);
            }
        }

        if let Ok(env_path) = std::env::var("GRX_CONFIG")
            && !env_path.trim().is_empty()
        {
            let p = Self::expand_tilde(Path::new(env_path.trim()));
            if p.is_file() {
                return Some(p);
            }
        }

        for local_name in &["grx.toml", ".grx.toml"] {
            let p = Path::new(local_name);
            if p.is_file() {
                return Some(p.to_path_buf());
            }
        }

        let xdg_path = Self::config_file_path();
        if xdg_path.is_file() {
            return Some(xdg_path);
        }

        if let Ok(home) = std::env::var("HOME")
            && !home.trim().is_empty()
        {
            let classic = PathBuf::from(home.trim()).join(".grx.toml");
            if classic.is_file() {
                return Some(classic);
            }
        }

        None
    }

    pub fn load_from_paths(custom_path: Option<&Path>) -> Self {
        if let Some(resolved) = Self::locate_config_path(custom_path)
            && let Ok(cfg) = Self::load_file(&resolved)
        {
            return cfg;
        }

        if let Some(path) = custom_path {
            let expanded = Self::expand_tilde(path);
            if let Ok(cfg) = Self::load_file(&expanded) {
                return cfg;
            }
        }

        Self::new_with_defaults()
    }

    pub fn load_explicit(path: &Path) -> Result<Self, String> {
        let expanded = Self::expand_tilde(path);
        Self::load_file(&expanded)
    }

    pub fn user_home_dir() -> Option<PathBuf> {
        if let Ok(home) = std::env::var("HOME")
            && !home.trim().is_empty()
        {
            return Some(PathBuf::from(home.trim()));
        }
        #[cfg(windows)]
        {
            if let Ok(userprofile) = std::env::var("USERPROFILE")
                && !userprofile.trim().is_empty()
            {
                return Some(PathBuf::from(userprofile.trim()));
            }
        }
        None
    }

    pub fn expand_tilde<P: AsRef<Path>>(path: P) -> PathBuf {
        let p = path.as_ref();
        let s = p.to_string_lossy();
        if s == "~" {
            if let Some(home) = Self::user_home_dir() {
                return home;
            }
        } else if let Some(rest) = s.strip_prefix("~/")
            && let Some(home) = Self::user_home_dir()
        {
            return home.join(rest);
        }
        #[cfg(windows)]
        {
            if let Some(rest) = s.strip_prefix("~\\")
                && let Some(home) = Self::user_home_dir()
            {
                return home.join(rest);
            }
        }
        p.to_path_buf()
    }

    pub fn expand_tilde_str(path_str: &str) -> String {
        if path_str == "~" {
            if let Some(home) = Self::user_home_dir() {
                return home.to_string_lossy().into_owned();
            }
        } else if let Some(rest) = path_str.strip_prefix("~/")
            && let Some(home) = Self::user_home_dir()
        {
            let mut s = home.join(rest).to_string_lossy().into_owned();
            if (rest.ends_with('/') || rest.ends_with('\\'))
                && !s.ends_with('/')
                && !s.ends_with('\\')
            {
                s.push('/');
            }
            return s;
        }
        #[cfg(windows)]
        {
            if let Some(rest) = path_str.strip_prefix("~\\")
                && let Some(home) = Self::user_home_dir()
            {
                let mut s = home.join(rest).to_string_lossy().into_owned();
                if (rest.ends_with('/') || rest.ends_with('\\'))
                    && !s.ends_with('/')
                    && !s.ends_with('\\')
                {
                    s.push('\\');
                }
                return s;
            }
        }
        path_str.to_string()
    }

    fn resolve_app_dir(xdg_var: &str, win_var: &str, subdirs: &[&str]) -> PathBuf {
        if let Ok(xdg) = std::env::var(xdg_var)
            && !xdg.trim().is_empty()
        {
            return PathBuf::from(xdg.trim()).join("grx");
        }
        #[cfg(windows)]
        {
            if let Ok(appdata) = std::env::var(win_var)
                && !appdata.trim().is_empty()
            {
                return PathBuf::from(appdata.trim()).join("grx");
            }
        }
        #[cfg(not(windows))]
        let _ = win_var;

        if let Some(home) = Self::user_home_dir() {
            let mut p = home;
            for part in subdirs {
                p.push(part);
            }
            return p;
        }
        let mut p = PathBuf::new();
        for part in subdirs {
            p.push(part);
        }
        p
    }

    pub fn config_dir() -> PathBuf {
        Self::resolve_app_dir("XDG_CONFIG_HOME", "APPDATA", &[".config", "grx"])
    }

    pub fn config_file_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    pub fn data_dir() -> PathBuf {
        Self::resolve_app_dir("XDG_DATA_HOME", "LOCALAPPDATA", &[".local", "share", "grx"])
    }

    pub fn cache_dir() -> PathBuf {
        Self::resolve_app_dir("XDG_CACHE_HOME", "LOCALAPPDATA", &[".cache", "grx"])
    }

    pub fn state_dir() -> PathBuf {
        Self::resolve_app_dir(
            "XDG_STATE_HOME",
            "LOCALAPPDATA",
            &[".local", "state", "grx"],
        )
    }

    pub fn journal_file_path() -> PathBuf {
        Self::data_dir().join("journal.jsonl")
    }

    pub fn global_ignore_path() -> PathBuf {
        Self::config_dir().join("ignore")
    }

    pub fn global_git_ignore_path() -> Option<PathBuf> {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
            && !xdg.trim().is_empty()
        {
            let p = PathBuf::from(xdg.trim()).join("git").join("ignore");
            if p.is_file() {
                return Some(p);
            }
        }
        if let Ok(home) = std::env::var("HOME")
            && !home.trim().is_empty()
        {
            let p = PathBuf::from(home.trim())
                .join(".config")
                .join("git")
                .join("ignore");
            if p.is_file() {
                return Some(p);
            }
        }
        #[cfg(windows)]
        {
            if let Ok(userprofile) = std::env::var("USERPROFILE")
                && !userprofile.trim().is_empty()
            {
                let p1 = PathBuf::from(userprofile.trim())
                    .join(".config")
                    .join("git")
                    .join("ignore");
                if p1.is_file() {
                    return Some(p1);
                }
                let p2 = PathBuf::from(userprofile.trim()).join(".gitignore");
                if p2.is_file() {
                    return Some(p2);
                }
            }
        }
        None
    }

    pub fn init_default_config(overwrite: bool) -> std::io::Result<PathBuf> {
        let path = Self::config_file_path();
        if path.is_file() && !overwrite {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("Configuration file already exists at {}", path.display()),
            ));
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&path, Self::generate_commented_toml())?;
        Ok(path)
    }

    pub fn open_in_editor(custom_path: Option<&Path>) -> std::io::Result<i32> {
        let path = if let Some(p) = custom_path {
            Self::expand_tilde(p)
        } else {
            let p = Self::config_file_path();
            if !p.is_file() {
                let _ = Self::init_default_config(false);
            }
            p
        };

        let editor = std::env::var("VISUAL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                std::env::var("EDITOR")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
            })
            .unwrap_or_else(|| {
                #[cfg(windows)]
                {
                    "notepad".to_string()
                }
                #[cfg(not(windows))]
                {
                    let candidates = ["xdg-open", "nano", "vi"];
                    for cand in candidates {
                        if std::process::Command::new("which")
                            .arg(cand)
                            .output()
                            .map(|o| o.status.success())
                            .unwrap_or(false)
                        {
                            return cand.to_string();
                        }
                    }
                    "nano".to_string()
                }
            });

        #[cfg(windows)]
        let status = if editor.contains(' ') {
            std::process::Command::new("cmd")
                .arg("/c")
                .arg(format!("{} \"{}\"", editor, path.display()))
                .status()?
        } else {
            std::process::Command::new(&editor).arg(&path).status()?
        };

        #[cfg(not(windows))]
        let status = if editor.contains(' ') {
            std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("{} \"{}\"", editor, path.display()))
                .status()?
        } else {
            std::process::Command::new(&editor).arg(&path).status()?
        };

        Ok(status.code().unwrap_or(0))
    }

    pub fn generate_commented_toml() -> String {
        r##"

# mode = "dsl"

# default-excludes = ["target/", "node_modules/", ".git/", ".hg/", ".svn/", "build/", "dist/", ".idea/", ".vscode/"]

[search]
# smart-case = true

# fixed-strings = false

# max-depth = 10

# follow-symlinks = false

# hidden = false

# no-ignore = false

# binary-detection = true

# binary-handling = "skip"

# Thread pool worker count (0 = automatic; content batches amortize thread startup).
# threads = 0

# mmap-threshold-bytes = 65536

# max-file-size-bytes = 104857600  # 100 MB

# buffer-size-bytes = 65536

# binary-null-probe-bytes = 1024

# walker-buffer-size-bytes = 65536

# suppress-errors = true

# regular-files-only = true

[output]
# color = "auto"

# hyperlinks = "auto"

# hyperlink-format = "file://{host}{path}#{line}:{col}"

# line-numbers = true

# heading = true

# context-before = 0

# context-after = 0

# null-separator = false

[output.colors]
# path = "\u001b[35m"

# line-number = "\u001b[32m"

# column = "\u001b[38;5;108m"

# match-highlight = "\u001b[1;31m"

# name-match-highlight = "\u001b[30;48;5;220m"

# context = "\u001b[38;5;250m"

[journal]
# enabled = false

# path = "~/.local/share/grx/journal.jsonl"

[types]
# rs = ["*.rs"]
# c = ["*.c", "*.h"]
# cpp = ["*.cpp", "*.cc", "*.cxx", "*.hpp", "*.h"]
# py = ["*.py", "*.pyi"]
# go = ["*.go"]
# toml = ["*.toml"]
# json = ["*.json"]
# yaml = ["*.yaml", "*.yml"]
# md = ["*.md", "*.markdown"]
# web = ["*.html", "*.css", "*.scss", "*.js", "*.ts", "*.jsx", "*.tsx", "*.vue", "*.svelte"]
# code = ["*.rs", "*.c", "*.h", "*.cpp", "*.hpp", "*.py", "*.go", "*.js", "*.ts", "*.java", "*.zig"]
# data = ["*.json", "*.yaml", "*.yml", "*.toml", "*.csv", "*.tsv", "*.xml"]
# doc = ["*.md", "*.rst", "*.txt", "*.adoc"]
"##.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = Config::new_with_defaults();
        assert_eq!(cfg.mode, SearchMode::Dsl);
        assert!(cfg.search.smart_case);
        assert!(cfg.search.suppress_errors);
        assert!(cfg.search.regular_files_only);
        assert_eq!(cfg.search.mmap_threshold_bytes, 65536);
        assert!(cfg.types.contains_key("rs"));
        assert!(cfg.types.contains_key("rust"));
        assert_eq!(cfg.types.get("rs"), cfg.types.get("rust"));
        assert!(cfg.types.contains_key("py"));
        assert!(cfg.types.contains_key("python"));
        assert_eq!(cfg.types.get("py"), cfg.types.get("python"));
        assert!(cfg.types.contains_key("toml"));
        assert!(cfg.default_excludes.contains(&"target/".to_string()));
        assert!(cfg.default_excludes.contains(&".cache/".to_string()));
        assert!(cfg.default_excludes.contains(&"Cache/".to_string()));
    }

    #[test]
    fn test_commented_toml_is_valid() {
        let template = Config::generate_commented_toml();

        let parsed_default: Result<Config, _> = toml::from_str(&template);
        assert!(
            parsed_default.is_ok(),
            "Commented template should parse as valid TOML"
        );

        let mut in_default_excludes = false;
        let mut uncommented = String::new();
        for line in template.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("default-excludes = [") {
                in_default_excludes = true;
                uncommented.push_str(line);
                uncommented.push('\n');
                continue;
            }
            if in_default_excludes {
                if trimmed == "]" {
                    in_default_excludes = false;
                    uncommented.push_str(line);
                    uncommented.push('\n');
                    continue;
                }
                if trimmed.starts_with('#') {
                    let item = trimmed.trim_start_matches('#').trim();
                    if item.starts_with('"') && item.ends_with(',') {
                        uncommented.push_str("  ");
                        uncommented.push_str(item);
                        uncommented.push('\n');
                        continue;
                    }
                }
            }
            if let Some(after_hash) = trimmed.strip_prefix("# ") {
                let after_hash = after_hash.trim();
                if let Some((k, v)) = after_hash.split_once('=') {
                    let k_trim = k.trim();
                    if !k_trim.is_empty()
                        && k_trim
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                        && k_trim
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_alphabetic())
                    {
                        uncommented.push_str(k_trim);
                        uncommented.push_str(" = ");
                        uncommented.push_str(v.trim());
                        uncommented.push('\n');
                        continue;
                    }
                }
            } else if !trimmed.starts_with('#') {
                uncommented.push_str(line);
                uncommented.push('\n');
            }
        }
        let parsed: Result<Config, _> = toml::from_str(&uncommented);
        assert!(
            parsed.is_ok(),
            "Uncommented template must parse cleanly into Config schema: {:?}",
            parsed.err()
        );
        let cfg = parsed.unwrap();
        assert_eq!(cfg.mode, SearchMode::Dsl);
        assert!(cfg.search.smart_case);
        assert_eq!(cfg.search.mmap_threshold_bytes, 65536);
        assert_eq!(cfg.search.binary_null_probe_bytes, 1024);
        assert_eq!(cfg.search.walker_buffer_size_bytes, 65536);
        assert_eq!(cfg.output.color, ColorChoice::Auto);
        assert_eq!(cfg.output.colors.path, "\x1b[35m");
        assert_eq!(cfg.output.colors.line_number, "\x1b[32m");
        assert_eq!(cfg.output.colors.column, "\x1b[38;5;108m");
        assert_eq!(cfg.output.colors.match_highlight, "\x1b[1;31m");
        assert_eq!(cfg.output.colors.name_match_highlight, "\x1b[30;48;5;220m");
        assert_eq!(cfg.output.colors.context, "\x1b[38;5;250m");
        assert!(cfg.default_excludes.contains(&"target/".to_string()));
    }

    #[test]
    fn test_custom_color_theme_parsing() {
        let toml_str = r#"
            [output.colors]
            path = "\u001b[1;34m"
            line-number = "\u001b[33m"
            column = "\u001b[36m"
            match-highlight = "\u001b[1;32m"
            name-match-highlight = "\u001b[30;48;5;214m"
            context = "\u001b[37m"
        "#;
        let cfg: Config = toml::from_str(toml_str).expect("failed to parse TOML");
        assert_eq!(cfg.output.colors.path, "\x1b[1;34m");
        assert_eq!(cfg.output.colors.line_number, "\x1b[33m");
        assert_eq!(cfg.output.colors.column, "\x1b[36m");
        assert_eq!(cfg.output.colors.match_highlight, "\x1b[1;32m");
        assert_eq!(cfg.output.colors.name_match_highlight, "\x1b[30;48;5;214m");
        assert_eq!(cfg.output.colors.context, "\x1b[37m");

        let old_theme: Config =
            toml::from_str("[output.colors]\nmatch-highlight = \"\\u001b[1;32m\"\n")
                .expect("existing color configuration should remain valid");
        assert_eq!(
            old_theme.output.colors.name_match_highlight,
            "\x1b[30;48;5;220m"
        );
    }

    #[test]
    fn explicit_config_load_reports_missing_and_invalid_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let missing = temp_dir.path().join("missing.toml");
        let missing_error = Config::load_explicit(&missing).unwrap_err();
        assert!(missing_error.contains("failed to read config"));
        assert!(missing_error.contains("missing.toml"));

        let invalid = temp_dir.path().join("invalid.toml");
        fs::write(&invalid, "[search\n").unwrap();
        let invalid_error = Config::load_explicit(&invalid).unwrap_err();
        assert!(invalid_error.contains("failed to parse config"));
        assert!(invalid_error.contains("invalid.toml"));
    }

    #[test]
    fn test_tilde_expansion_behavior() {
        if let Some(home) = Config::user_home_dir() {
            assert_eq!(Config::expand_tilde("~"), home);
            assert_eq!(Config::expand_tilde_str("~"), home.to_string_lossy());

            assert_eq!(Config::expand_tilde("~/Projects"), home.join("Projects"));
            assert_eq!(
                Config::expand_tilde_str("~/Projects"),
                home.join("Projects").to_string_lossy()
            );

            let trailing = Config::expand_tilde_str("~/Projects/");
            assert!(trailing.ends_with('/'));
            assert_eq!(
                trailing.trim_end_matches('/'),
                home.join("Projects")
                    .to_string_lossy()
                    .trim_end_matches('/')
            );
        }

        assert_eq!(
            Config::expand_tilde("file.txt~"),
            PathBuf::from("file.txt~")
        );
        assert_eq!(Config::expand_tilde_str("foo~bar"), "foo~bar");
        assert_eq!(Config::expand_tilde("./~"), PathBuf::from("./~"));
        assert_eq!(Config::expand_tilde_str("*~"), "*~");
        assert_eq!(Config::expand_tilde_str("~other/dir"), "~other/dir");
    }
}
