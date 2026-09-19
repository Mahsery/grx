use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Operational mode of the search engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum SearchMode {
    /// Ergonomic smart DSL (default): auto-parses :ext, no:path, quotes, bare words.
    #[default]
    Dsl,
    /// Pure POSIX grep compatibility mode: strict positional patterns and flags only.
    Grep,
    /// Git-grep compatibility mode: pathspec matching within git trees.
    GitGrep,
}

/// Color rendering behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

/// Terminal hyperlink (OSC 8) behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum HyperlinkChoice {
    #[default]
    Auto,
    Always,
    Never,
}

/// Binary file handling strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BinaryHandling {
    #[default]
    Skip,
    Search,
    HexDump,
}

/// Search behavior tuning options.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SearchSettings {
    /// Use smart-case matching (case-insensitive unless uppercase letters are present).
    pub smart_case: bool,
    /// Treat bare patterns as fixed literal strings without regex parsing.
    pub fixed_strings: bool,
    /// Maximum recursive directory traversal depth.
    pub max_depth: Option<usize>,
    /// Follow directory symlinks during traversal.
    pub follow_symlinks: bool,
    /// Search hidden files and directories (prefixed with .).
    pub hidden: bool,
    /// Ignore VCS exclusion rules (.gitignore, .ignore).
    pub no_ignore: bool,
    /// Probe the initial buffer chunk for null bytes to identify binaries.
    pub binary_detection: bool,
    /// Strategy for handling binary files.
    pub binary_handling: BinaryHandling,
    /// Number of worker threads (0 = auto-detect based on physical CPU cores).
    pub threads: usize,
    /// File size threshold in bytes above which memory-mapping (mmap) is used.
    pub mmap_threshold_bytes: u64,
    /// Maximum file size to inspect (None = unlimited).
    pub max_file_size_bytes: Option<u64>,
    /// Reusable thread-local buffer size in bytes for small-file buffered reads.
    pub buffer_size_bytes: usize,
    /// Number of initial bytes to probe for null bytes during binary detection.
    pub binary_null_probe_bytes: usize,
    /// Reusable buffer size in bytes for directory traversal (getdents64).
    pub walker_buffer_size_bytes: usize,
    /// Suppress permission denied, unreadable file, and broken symlink errors during traversal.
    pub suppress_errors: bool,
    /// Only search regular files, skipping character devices, block devices, FIFOs, and sockets.
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
            mmap_threshold_bytes: 64 * 1024, // 64 KB
            max_file_size_bytes: None,
            buffer_size_bytes: 64 * 1024,        // 64 KB
            binary_null_probe_bytes: 1024,       // 1 KB
            walker_buffer_size_bytes: 64 * 1024, // 64 KB
            suppress_errors: true,
            regular_files_only: true,
        }
    }
}

/// Output formatting and rendering options.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct OutputSettings {
    /// Color mode.
    pub color: ColorChoice,
    /// Hyperlink emission mode (OSC 8).
    pub hyperlinks: HyperlinkChoice,
    /// OSC 8 hyperlink format URI template.
    pub hyperlink_format: String,
    /// Display line numbers on matches.
    pub line_numbers: bool,
    /// Group matches under file path headings on TTY.
    pub heading: bool,
    /// Number of lines of context before matching lines.
    pub context_before: usize,
    /// Number of lines of context after matching lines.
    pub context_after: usize,
    /// Terminate records with a null byte (for xargs -0).
    pub null_separator: bool,
    /// Customizable ANSI color styling palette.
    pub colors: ColorTheme,
}

/// Configurable ANSI color palette for search output presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ColorTheme {
    /// ANSI escape sequence or style spec for file paths (default: "\x1b[35m").
    pub path: String,
    /// ANSI escape sequence or style spec for line numbers (default: "\x1b[32m").
    pub line_number: String,
    /// ANSI escape sequence or style spec for column numbers (default: "\x1b[38;5;108m").
    pub column: String,
    /// ANSI escape sequence or style spec for matched text highlights (default: "\x1b[1;31m").
    pub match_highlight: String,
    /// ANSI escape sequence or style spec for context lines (default: "\x1b[38;5;250m").
    pub context: String,
}

impl Default for ColorTheme {
    fn default() -> Self {
        Self {
            path: "\x1b[35m".to_string(),
            line_number: "\x1b[32m".to_string(),
            column: "\x1b[38;5;108m".to_string(),
            match_highlight: "\x1b[1;31m".to_string(),
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

/// Journaling and telemetry configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
#[derive(Default)]
pub struct JournalSettings {
    /// Enable append-only structured JSONL execution logging.
    pub enabled: bool,
    /// Destination path for journal file.
    pub path: Option<PathBuf>,
}

/// Complete configuration tree for grx.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    /// Primary search operational mode.
    pub mode: SearchMode,
    /// Search engine settings.
    pub search: SearchSettings,
    /// Output presentation settings.
    pub output: OutputSettings,
    /// Journaling settings.
    pub journal: JournalSettings,
    /// Open file type mappings (alias -> list of glob patterns).
    pub types: BTreeMap<String, Vec<String>>,
    /// Default path exclusion rules.
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

    /// Check if a given type alias (e.g. "rs", "rust", "cpp") is known in the configuration.
    #[inline]
    pub fn is_known_type(&self, alias: &str) -> bool {
        self.types.contains_key(alias)
    }

    /// Expand a type alias into its corresponding file patterns / extensions.
    #[inline]
    pub fn expand_type_alias(&self, alias: &str) -> Option<Vec<String>> {
        self.types.get(alias).cloned()
    }

    /// Initialize configuration with sensible default type aliases and excludes.
    pub fn new_with_defaults() -> Self {
        let mut types = BTreeMap::new();
        types.insert("rs".to_string(), vec!["*.rs".to_string()]);
        types.insert("rust".to_string(), vec!["*.rs".to_string()]);
        types.insert("c".to_string(), vec!["*.c".to_string(), "*.h".to_string()]);
        types.insert(
            "cpp".to_string(),
            vec![
                "*.cpp".to_string(),
                "*.cc".to_string(),
                "*.cxx".to_string(),
                "*.hpp".to_string(),
                "*.h".to_string(),
            ],
        );
        types.insert(
            "c++".to_string(),
            vec![
                "*.cpp".to_string(),
                "*.cc".to_string(),
                "*.cxx".to_string(),
                "*.hpp".to_string(),
                "*.h".to_string(),
            ],
        );
        types.insert(
            "py".to_string(),
            vec!["*.py".to_string(), "*.pyi".to_string()],
        );
        types.insert(
            "python".to_string(),
            vec!["*.py".to_string(), "*.pyi".to_string()],
        );
        types.insert("go".to_string(), vec!["*.go".to_string()]);
        types.insert("golang".to_string(), vec!["*.go".to_string()]);
        types.insert(
            "js".to_string(),
            vec!["*.js".to_string(), "*.mjs".to_string(), "*.cjs".to_string()],
        );
        types.insert(
            "javascript".to_string(),
            vec!["*.js".to_string(), "*.mjs".to_string(), "*.cjs".to_string()],
        );
        types.insert(
            "ts".to_string(),
            vec![
                "*.ts".to_string(),
                "*.tsx".to_string(),
                "*.mts".to_string(),
                "*.cts".to_string(),
            ],
        );
        types.insert(
            "typescript".to_string(),
            vec![
                "*.ts".to_string(),
                "*.tsx".to_string(),
                "*.mts".to_string(),
                "*.cts".to_string(),
            ],
        );
        types.insert("toml".to_string(), vec!["*.toml".to_string()]);
        types.insert("json".to_string(), vec!["*.json".to_string()]);
        types.insert(
            "yaml".to_string(),
            vec!["*.yaml".to_string(), "*.yml".to_string()],
        );
        types.insert(
            "yml".to_string(),
            vec!["*.yaml".to_string(), "*.yml".to_string()],
        );
        types.insert(
            "md".to_string(),
            vec!["*.md".to_string(), "*.markdown".to_string()],
        );
        types.insert(
            "markdown".to_string(),
            vec!["*.md".to_string(), "*.markdown".to_string()],
        );
        types.insert(
            "sh".to_string(),
            vec![
                "*.sh".to_string(),
                "*.bash".to_string(),
                "*.zsh".to_string(),
                "*.fish".to_string(),
            ],
        );
        types.insert(
            "shell".to_string(),
            vec![
                "*.sh".to_string(),
                "*.bash".to_string(),
                "*.zsh".to_string(),
                "*.fish".to_string(),
            ],
        );
        types.insert(
            "bash".to_string(),
            vec!["*.bash".to_string(), "*.sh".to_string()],
        );
        types.insert("fish".to_string(), vec!["*.fish".to_string()]);
        types.insert("zsh".to_string(), vec!["*.zsh".to_string()]);
        types.insert("java".to_string(), vec!["*.java".to_string()]);
        types.insert(
            "kotlin".to_string(),
            vec!["*.kt".to_string(), "*.kts".to_string()],
        );
        types.insert(
            "kt".to_string(),
            vec!["*.kt".to_string(), "*.kts".to_string()],
        );
        types.insert("zig".to_string(), vec!["*.zig".to_string()]);
        types.insert("lua".to_string(), vec!["*.lua".to_string()]);
        types.insert("sql".to_string(), vec!["*.sql".to_string()]);
        types.insert(
            "html".to_string(),
            vec!["*.html".to_string(), "*.htm".to_string()],
        );
        types.insert(
            "css".to_string(),
            vec![
                "*.css".to_string(),
                "*.scss".to_string(),
                "*.sass".to_string(),
                "*.less".to_string(),
            ],
        );
        types.insert(
            "web".to_string(),
            vec![
                "*.html".to_string(),
                "*.css".to_string(),
                "*.scss".to_string(),
                "*.js".to_string(),
                "*.ts".to_string(),
                "*.jsx".to_string(),
                "*.tsx".to_string(),
                "*.vue".to_string(),
                "*.svelte".to_string(),
            ],
        );
        types.insert(
            "code".to_string(),
            vec![
                "*.rs".to_string(),
                "*.c".to_string(),
                "*.h".to_string(),
                "*.cpp".to_string(),
                "*.hpp".to_string(),
                "*.py".to_string(),
                "*.go".to_string(),
                "*.js".to_string(),
                "*.ts".to_string(),
                "*.tsx".to_string(),
                "*.jsx".to_string(),
                "*.java".to_string(),
                "*.zig".to_string(),
                "*.lua".to_string(),
                "*.sh".to_string(),
            ],
        );
        types.insert(
            "data".to_string(),
            vec![
                "*.json".to_string(),
                "*.yaml".to_string(),
                "*.yml".to_string(),
                "*.toml".to_string(),
                "*.csv".to_string(),
                "*.tsv".to_string(),
                "*.xml".to_string(),
            ],
        );
        types.insert(
            "doc".to_string(),
            vec![
                "*.md".to_string(),
                "*.rst".to_string(),
                "*.txt".to_string(),
                "*.adoc".to_string(),
            ],
        );

        let default_excludes = vec![
            "target/".to_string(),
            "node_modules/".to_string(),
            ".git/".to_string(),
            ".hg/".to_string(),
            ".svn/".to_string(),
            "build/".to_string(),
            "dist/".to_string(),
            ".idea/".to_string(),
            ".vscode/".to_string(),
            ".cache/".to_string(),
            "Cache/".to_string(),
            "CachedData/".to_string(),
        ];

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
    /// Locate the active configuration file according to precedence:
    /// 1. Custom path specified via CLI flag `--config`.
    /// 2. `$GRX_CONFIG` environment variable.
    /// 3. `./grx.toml` or `./.grx.toml` (local project root).
    /// 4. `$XDG_CONFIG_HOME/grx/config.toml` (or `~/.config/grx/config.toml`).
    /// 5. `~/.grx.toml` (classic dotfile fallback).
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

        // Check local project directory
        for local_name in &["grx.toml", ".grx.toml"] {
            let p = Path::new(local_name);
            if p.is_file() {
                return Some(p.to_path_buf());
            }
        }

        // Check XDG user config directory ($XDG_CONFIG_HOME/grx/config.toml or ~/.config/grx/config.toml)
        let xdg_path = Self::config_file_path();
        if xdg_path.is_file() {
            return Some(xdg_path);
        }

        // Check classic home dotfile (~/.grx.toml)
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

    /// Locate and load configuration according to precedence:
    /// 1. Custom path specified via CLI/env.
    /// 2. `$GRX_CONFIG` environment variable.
    /// 3. `./grx.toml` or `./.grx.toml` (local project root).
    /// 4. `$XDG_CONFIG_HOME/grx/config.toml` (or `~/.config/grx/config.toml`).
    /// 5. `~/.grx.toml` (classic dotfile fallback).
    /// 6. Built-in defaults.
    pub fn load_from_paths(custom_path: Option<&Path>) -> Self {
        if let Some(resolved) = Self::locate_config_path(custom_path)
            && let Ok(cfg) = Self::load_file(&resolved)
        {
            return cfg;
        }

        // Also check if custom_path was provided directly even if not yet checked by is_file
        if let Some(path) = custom_path {
            let expanded = Self::expand_tilde(path);
            if let Ok(cfg) = Self::load_file(&expanded) {
                return cfg;
            }
        }

        Self::new_with_defaults()
    }

    /// Load an explicitly requested configuration path without silently
    /// substituting defaults for path, permission, or TOML errors.
    pub fn load_explicit(path: &Path) -> Result<Self, String> {
        let expanded = Self::expand_tilde(path);
        Self::load_file(&expanded)
    }

    /// Resolve the user's home directory across Linux, macOS, and Windows.
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

    /// Expand leading `~` or `~/` in a path to the user's home directory.
    ///
    /// Leaves other paths (e.g. `./~`, `foo~bar`, `~user/`) untouched to avoid false positives.
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

    /// Expand leading `~` or `~/` in a path string (preserving trailing slashes).
    ///
    /// Leaves non-home paths untouched to avoid collisions.
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

    /// Distro-appropriate configuration directory ($XDG_CONFIG_HOME/grx or ~/.config/grx).
    pub fn config_dir() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
            && !xdg.trim().is_empty()
        {
            return PathBuf::from(xdg.trim()).join("grx");
        }
        #[cfg(windows)]
        {
            if let Ok(appdata) = std::env::var("APPDATA")
                && !appdata.trim().is_empty()
            {
                return PathBuf::from(appdata.trim()).join("grx");
            }
        }
        if let Ok(home) = std::env::var("HOME")
            && !home.trim().is_empty()
        {
            return PathBuf::from(home.trim()).join(".config").join("grx");
        }
        #[cfg(windows)]
        {
            if let Ok(userprofile) = std::env::var("USERPROFILE")
                && !userprofile.trim().is_empty()
            {
                return PathBuf::from(userprofile.trim())
                    .join(".config")
                    .join("grx");
            }
        }
        PathBuf::from(".config/grx")
    }

    /// Distro-appropriate user configuration file path ($XDG_CONFIG_HOME/grx/config.toml).
    pub fn config_file_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// Distro-appropriate data directory ($XDG_DATA_HOME/grx or ~/.local/share/grx).
    pub fn data_dir() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
            && !xdg.trim().is_empty()
        {
            return PathBuf::from(xdg.trim()).join("grx");
        }
        #[cfg(windows)]
        {
            if let Ok(localappdata) = std::env::var("LOCALAPPDATA")
                && !localappdata.trim().is_empty()
            {
                return PathBuf::from(localappdata.trim()).join("grx");
            }
        }
        if let Ok(home) = std::env::var("HOME")
            && !home.trim().is_empty()
        {
            return PathBuf::from(home.trim())
                .join(".local")
                .join("share")
                .join("grx");
        }
        #[cfg(windows)]
        {
            if let Ok(userprofile) = std::env::var("USERPROFILE")
                && !userprofile.trim().is_empty()
            {
                return PathBuf::from(userprofile.trim())
                    .join(".local")
                    .join("share")
                    .join("grx");
            }
        }
        PathBuf::from(".local/share/grx")
    }

    /// Distro-appropriate cache directory ($XDG_CACHE_HOME/grx or ~/.cache/grx).
    pub fn cache_dir() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CACHE_HOME")
            && !xdg.trim().is_empty()
        {
            return PathBuf::from(xdg.trim()).join("grx");
        }
        #[cfg(windows)]
        {
            if let Ok(localappdata) = std::env::var("LOCALAPPDATA")
                && !localappdata.trim().is_empty()
            {
                return PathBuf::from(localappdata.trim()).join("grx");
            }
        }
        if let Ok(home) = std::env::var("HOME")
            && !home.trim().is_empty()
        {
            return PathBuf::from(home.trim()).join(".cache").join("grx");
        }
        PathBuf::from(".cache/grx")
    }

    /// Distro-appropriate state directory ($XDG_STATE_HOME/grx or ~/.local/state/grx).
    pub fn state_dir() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_STATE_HOME")
            && !xdg.trim().is_empty()
        {
            return PathBuf::from(xdg.trim()).join("grx");
        }
        #[cfg(windows)]
        {
            if let Ok(localappdata) = std::env::var("LOCALAPPDATA")
                && !localappdata.trim().is_empty()
            {
                return PathBuf::from(localappdata.trim()).join("grx");
            }
        }
        if let Ok(home) = std::env::var("HOME")
            && !home.trim().is_empty()
        {
            return PathBuf::from(home.trim())
                .join(".local")
                .join("state")
                .join("grx");
        }
        PathBuf::from(".local/state/grx")
    }

    /// Distro-appropriate default journal file path ($XDG_DATA_HOME/grx/journal.jsonl).
    pub fn journal_file_path() -> PathBuf {
        Self::data_dir().join("journal.jsonl")
    }

    /// Distro-appropriate user global ignore file path ($XDG_CONFIG_HOME/grx/ignore).
    pub fn global_ignore_path() -> PathBuf {
        Self::config_dir().join("ignore")
    }

    /// Distro-appropriate git global ignore file path ($XDG_CONFIG_HOME/git/ignore).
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

    /// Initialize default commented configuration in the distro-appropriate location.
    /// Creates parent directories if missing.
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

    /// Open the configuration file in the system text editor.
    /// Precedence: $VISUAL -> $EDITOR -> xdg-open / notepad -> nano -> vi.
    /// If the file does not exist, it is automatically initialized with the default template first.
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

    /// Autogenerate a brutally configurable, fully commented-out TOML document.
    pub fn generate_commented_toml() -> String {
        r##"# ==============================================================================
# grx Configuration Template
# ==============================================================================
# This configuration file controls all aspects of grx search execution,
# directory traversal, output presentation, and compatibility modes.
# Every option is commented out with its default value shown.
#
# Location precedence:
#   1. Explicit flag: --config <path> or $GRX_CONFIG
#   2. Local file: ./grx.toml or ./.grx.toml
#   3. User file:  ~/.config/grx/config.toml

# ------------------------------------------------------------------------------
# Operational Mode
# ------------------------------------------------------------------------------
# Available modes:
#   "dsl"      - Smart ergonomic search DSL (e.g. `s query :rs no:target/`)
#   "grep"     - Pure POSIX grep mode (strict grep syntax, no DSL token parsing)
#   "git-grep" - Git grep pathspec compatibility
# mode = "dsl"

# ------------------------------------------------------------------------------
# Default Path Exclusions
# ------------------------------------------------------------------------------
# These directory patterns are excluded automatically unless explicitly overridden.
# default-excludes = ["target/", "node_modules/", ".git/", ".hg/", ".svn/", "build/", "dist/", ".idea/", ".vscode/"]

[search]
# Smart-case: case-insensitive if all lowercase, case-sensitive if uppercase is present.
# smart-case = true

# Treat all bare search patterns as fixed literal strings without regex parsing.
# fixed-strings = false

# Maximum recursive search depth (uncomment to cap depth).
# max-depth = 10

# Follow filesystem symlinks during directory traversal.
# follow-symlinks = false

# Search hidden files and directories (names starting with '.').
# hidden = false

# Bypass .gitignore, .ignore, and global VCS ignore files.
# no-ignore = false

# Inspect first 1KB probe for null bytes to automatically skip binary files.
# binary-detection = true

# Strategy for handling binary files: "skip", "search", or "hex-dump".
# binary-handling = "skip"

# Thread pool worker count (0 = auto-detect physical CPU cores).
# threads = 0

# File size threshold (in bytes) above which mmap is preferred over buffered read.
# Default: 65536 (64 KB).
# mmap-threshold-bytes = 65536

# Maximum file size to inspect in bytes (uncomment to limit).
# max-file-size-bytes = 104857600  # 100 MB

# Thread-local reusable buffer size in bytes for buffered reads.
# buffer-size-bytes = 65536

# Number of initial bytes to probe for null bytes during binary detection.
# binary-null-probe-bytes = 1024

# Reusable buffer size in bytes for directory traversal (getdents64).
# walker-buffer-size-bytes = 65536

# Suppress permission denied, unreadable file, and broken symlink errors during traversal.
# suppress-errors = true

# Only search regular files, skipping character devices, block devices, FIFOs, and sockets.
# regular-files-only = true

[output]
# Color rendering: "auto" (TTY only), "always", or "never".
# color = "auto"

# Terminal hyperlinks (OSC 8): "auto" (TTY only), "always", or "never".
# When active, clicking file matches in supported terminals opens the file at line.
# hyperlinks = "auto"

# Template URI for OSC 8 hyperlinks.
# Available placeholders: {host}, {path}, {line}, {col}
# hyperlink-format = "file://{host}{path}#{line}:{col}"

# Display 1-indexed line numbers beside matches.
# line-numbers = true

# Group matches under colored file path headers on interactive terminals.
# heading = true

# Default context lines before match.
# context-before = 0

# Default context lines after match.
# context-after = 0

# Terminate records with a null byte for xargs -0.
# null-separator = false

[output.colors]
# ANSI escape code for file paths.
# path = "\u001b[35m"

# ANSI escape code for line numbers.
# line-number = "\u001b[32m"

# ANSI escape code for column numbers.
# column = "\u001b[38;5;108m"

# ANSI escape code for matching text.
# match-highlight = "\u001b[1;31m"

# ANSI escape code for context lines.
# context = "\u001b[38;5;250m"

[journal]
# Enable append-only structured JSONL execution telemetry.
# enabled = false

# Destination path for execution journal records.
# path = "~/.local/share/grx/journal.jsonl"

# ------------------------------------------------------------------------------
# File Type Aliases
# ------------------------------------------------------------------------------
# Users can query these with `:alias` (e.g. `:rs`, `:web`, `:c`) or exclude with `no:alias`.
# Custom types can be freely added here.
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
        // 1. Commented template should parse as valid TOML
        let parsed_default: Result<Config, _> = toml::from_str(&template);
        assert!(
            parsed_default.is_ok(),
            "Commented template should parse as valid TOML"
        );

        // 2. Uncomment all commented key-value pairs and array items to test schema validity
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
            context = "\u001b[37m"
        "#;
        let cfg: Config = toml::from_str(toml_str).expect("failed to parse TOML");
        assert_eq!(cfg.output.colors.path, "\x1b[1;34m");
        assert_eq!(cfg.output.colors.line_number, "\x1b[33m");
        assert_eq!(cfg.output.colors.column, "\x1b[36m");
        assert_eq!(cfg.output.colors.match_highlight, "\x1b[1;32m");
        assert_eq!(cfg.output.colors.context, "\x1b[37m");
    }

    #[test]
    fn test_config_type_alias_methods() {
        let cfg = Config::new_with_defaults();
        assert!(cfg.is_known_type("rs"));
        assert!(cfg.is_known_type("rust"));
        assert!(cfg.is_known_type("py"));
        assert!(!cfg.is_known_type("nonexistent_unknown_type"));

        assert_eq!(cfg.expand_type_alias("rs"), Some(vec!["*.rs".to_string()]));
        assert_eq!(
            cfg.expand_type_alias("c"),
            Some(vec!["*.c".to_string(), "*.h".to_string()])
        );
        assert_eq!(cfg.expand_type_alias("nonexistent"), None);
    }

    #[test]
    fn test_search_mode_parsing() {
        let toml_str = r#"
            mode = "grep"
            [search]
            smart-case = false
            threads = 8
        "#;
        let cfg: Config = toml::from_str(toml_str).expect("failed to parse TOML");
        assert_eq!(cfg.mode, SearchMode::Grep);
        assert!(!cfg.search.smart_case);
        assert_eq!(cfg.search.threads, 8);
    }

    #[test]
    fn test_xdg_paths_resolution() {
        let config_dir = Config::config_dir();
        assert!(config_dir.ends_with("grx"));

        let config_file = Config::config_file_path();
        assert!(config_file.ends_with("grx/config.toml"));

        let data_dir = Config::data_dir();
        assert!(data_dir.ends_with("grx"));

        let journal_file = Config::journal_file_path();
        assert!(journal_file.ends_with("grx/journal.jsonl"));

        let cache_dir = Config::cache_dir();
        assert!(cache_dir.ends_with("grx"));

        let state_dir = Config::state_dir();
        assert!(state_dir.ends_with("grx"));
    }

    #[test]
    fn test_init_default_config_in_tempdir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let custom_cfg = temp_dir.path().join("grx/config.toml");

        // Manually write template
        fs::create_dir_all(custom_cfg.parent().unwrap()).unwrap();
        fs::write(&custom_cfg, Config::generate_commented_toml()).unwrap();
        assert!(custom_cfg.is_file());

        let loaded = Config::load_from_paths(Some(&custom_cfg));
        assert_eq!(loaded.mode, SearchMode::Dsl);
        assert!(!loaded.default_excludes.is_empty());
        assert!(loaded.default_excludes.iter().any(|e| e == "target/"));
        assert!(loaded.types.contains_key("rs"));
        assert_eq!(loaded.types.get("rs"), Some(&vec!["*.rs".to_string()]));
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
            // 1. Bare tilde
            assert_eq!(Config::expand_tilde("~"), home);
            assert_eq!(Config::expand_tilde_str("~"), home.to_string_lossy());

            // 2. Subdirectory under home
            assert_eq!(Config::expand_tilde("~/Projects"), home.join("Projects"));
            assert_eq!(
                Config::expand_tilde_str("~/Projects"),
                home.join("Projects").to_string_lossy()
            );

            // 3. Trailing slash preservation
            let trailing = Config::expand_tilde_str("~/Projects/");
            assert!(trailing.ends_with('/'));
            assert_eq!(
                trailing.trim_end_matches('/'),
                home.join("Projects")
                    .to_string_lossy()
                    .trim_end_matches('/')
            );
        }

        // 4. Infix/suffix tildes must NEVER expand (avoid collisions with backup files or filenames)
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
