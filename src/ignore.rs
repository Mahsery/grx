use crate::core::{FilterDecision, IgnoreFilter};
use std::fs;
use std::path::{Path, PathBuf};

/// Classification of ignore pattern for ultra-fast matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleKind {
    /// Extension match: e.g. *.o, *.ko, *.a, *.bin (no slashes, single '*.' prefix).
    Extension(Vec<u8>),
    /// Exact basename match: e.g. tags, TAGS, vmlinux (no slashes, no wildcards).
    Exact(Vec<u8>),
    /// Complex glob or path pattern requiring full glob evaluation.
    Complex,
}

/// A compiled ignore rule parsed from a .gitignore, .ignore file, or CLI parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoreRule {
    /// Relative directory scope where this rule was defined (empty for global).
    pub scope: PathBuf,
    /// Glob pattern or path prefix.
    pub pattern: String,
    /// Pre-parsed byte representation of pattern for zero-allocation matching.
    pub pattern_bytes: Vec<u8>,
    /// Fast rule classification.
    pub kind: RuleKind,
    /// If true, this is an inclusion/negation rule (started with '!').
    pub is_negation: bool,
    /// If true, matches directories only (ended with '/').
    pub dir_only: bool,
    /// Cached check whether pattern contains wildcards (* or ?).
    pub has_glob: bool,
    /// If true, the pattern was anchored at the root of the scope (started with '/').
    pub is_rooted: bool,
}

impl IgnoreRule {
    pub fn parse(line: &str, scope: PathBuf) -> Option<Self> {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }

        let (is_negation, raw_pattern) = if let Some(rest) = trimmed.strip_prefix('!') {
            (true, rest)
        } else {
            (false, trimmed)
        };

        let dir_only = raw_pattern.ends_with('/');
        let is_rooted = raw_pattern.starts_with('/');
        let pattern = raw_pattern
            .trim_end_matches('/')
            .trim_start_matches('/')
            .to_string();
        let pattern_bytes = pattern.as_bytes().to_vec();
        let has_glob = pattern.contains('*') || pattern.contains('?');

        let kind = if is_rooted {
            RuleKind::Complex
        } else if pattern.starts_with("*.")
            && !pattern[2..].contains('*')
            && !pattern[2..].contains('?')
            && !pattern.contains('/')
        {
            RuleKind::Extension(pattern.as_bytes()[2..].to_vec())
        } else if !pattern.contains('*') && !pattern.contains('?') && !pattern.contains('/') {
            RuleKind::Exact(pattern.as_bytes().to_vec())
        } else {
            RuleKind::Complex
        };

        Some(Self {
            scope,
            pattern,
            pattern_bytes,
            kind,
            is_negation,
            dir_only,
            has_glob,
            is_rooted,
        })
    }

    /// Fast basename check for extension or exact rules without path overhead.
    #[inline]
    pub fn matches_name(&self, name: &[u8], is_dir: bool) -> Option<bool> {
        if self.dir_only && !is_dir {
            return Some(false);
        }
        match &self.kind {
            RuleKind::Extension(ext) => {
                if is_dir {
                    Some(false)
                } else if name.len() > ext.len() + 1
                    && name.ends_with(ext)
                    && name[name.len() - ext.len() - 1] == b'.'
                {
                    Some(true)
                } else {
                    Some(false)
                }
            }
            RuleKind::Exact(target) => Some(name == target.as_slice()),
            RuleKind::Complex => None,
        }
    }

    /// Check if a path relative to the scope matches this rule using raw byte slices (zero-alloc).
    pub fn matches_bytes(&self, relative_path: &[u8], is_dir: bool) -> bool {
        if self.dir_only && !is_dir {
            return false;
        }

        if self.is_rooted || self.pattern.contains('/') || self.pattern.contains('\\') {
            if self.has_glob {
                glob_match_bytes(&self.pattern_bytes, relative_path)
            } else {
                path_equals_normalized(&self.pattern_bytes, relative_path)
            }
        } else if self.has_glob {
            let basename = relative_path
                .rsplit(|&b| b == b'/' || b == b'\\')
                .next()
                .unwrap_or(relative_path);
            glob_match_bytes(&self.pattern_bytes, basename)
        } else {
            path_matches_component(relative_path, &self.pattern_bytes)
        }
    }

    /// Check if a path relative to the scope matches this rule.
    pub fn matches(&self, relative_path: &Path, is_dir: bool) -> bool {
        self.matches_bytes(relative_path.as_os_str().as_encoded_bytes(), is_dir)
    }
}

/// Helper comparing two raw byte path slices treating '/' and '\\' as equivalent.
#[inline]
pub fn path_equals_normalized(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(&x, &y)| glob_byte_matches(x, y))
}

/// Helper comparing two raw byte path slices treating '/' and '\\' as equivalent, case-insensitive.
#[inline]
pub fn path_equals_normalized_ci(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(&x, &y)| glob_byte_matches_ci(x, y))
}

/// Check if path equals pat or contains pat as a path component (bounded by '/' or '\\').
/// Operates directly on byte slices with zero heap allocations.
pub fn path_matches_component(path: &[u8], pat: &[u8]) -> bool {
    if pat.is_empty() {
        return false;
    }
    if path_equals_normalized(path, pat) {
        return true;
    }
    let p_len = path.len();
    let pat_len = pat.len();
    if p_len <= pat_len {
        return false;
    }

    #[inline]
    fn is_sep(b: u8) -> bool {
        b == b'/' || b == b'\\'
    }

    // Check if path starts with pat followed by '/' or '\'
    if path.starts_with(pat) && is_sep(path[pat_len]) {
        return true;
    }
    // Check if path ends with '/' or '\' followed by pat
    if path.ends_with(pat) && is_sep(path[p_len - pat_len - 1]) {
        return true;
    }
    // Check if path contains "[sep]pat[sep]"
    let target_len = pat_len + 2;
    if p_len >= target_len {
        for i in 0..=(p_len - target_len) {
            if is_sep(path[i])
                && is_sep(path[i + 1 + pat_len])
                && &path[i + 1..i + 1 + pat_len] == pat
            {
                return true;
            }
        }
    }
    false
}

#[inline]
fn glob_byte_matches(p: u8, t: u8) -> bool {
    p == t || ((p == b'/' || p == b'\\') && (t == b'/' || t == b'\\'))
}

#[inline]
fn glob_byte_matches_ci(p: u8, t: u8) -> bool {
    glob_byte_matches(p, t) || p.eq_ignore_ascii_case(&t)
}

/// Zero-dependency fast glob matcher supporting `*` and `?` over raw byte slices.
pub fn glob_match_bytes(p_bytes: &[u8], t_bytes: &[u8]) -> bool {
    let mut p_idx = 0;
    let mut t_idx = 0;
    let mut star_p = None;
    let mut star_t = 0;

    while t_idx < t_bytes.len() {
        if p_idx < p_bytes.len()
            && (p_bytes[p_idx] == b'?' || glob_byte_matches(p_bytes[p_idx], t_bytes[t_idx]))
        {
            p_idx += 1;
            t_idx += 1;
        } else if p_idx < p_bytes.len() && p_bytes[p_idx] == b'*' {
            star_p = Some(p_idx);
            star_t = t_idx;
            p_idx += 1;
        } else if let Some(sp) = star_p {
            p_idx = sp + 1;
            star_t += 1;
            t_idx = star_t;
        } else {
            return false;
        }
    }

    while p_idx < p_bytes.len() && p_bytes[p_idx] == b'*' {
        p_idx += 1;
    }

    p_idx == p_bytes.len()
}

/// Zero-dependency fast glob matcher supporting `*` and `?` over raw byte slices, ASCII case-insensitive.
pub fn glob_match_bytes_case_insensitive(p_bytes: &[u8], t_bytes: &[u8]) -> bool {
    let mut p_idx = 0;
    let mut t_idx = 0;
    let mut star_p = None;
    let mut star_t = 0;

    while t_idx < t_bytes.len() {
        if p_idx < p_bytes.len()
            && (p_bytes[p_idx] == b'?' || glob_byte_matches_ci(p_bytes[p_idx], t_bytes[t_idx]))
        {
            p_idx += 1;
            t_idx += 1;
        } else if p_idx < p_bytes.len() && p_bytes[p_idx] == b'*' {
            star_p = Some(p_idx);
            star_t = t_idx;
            p_idx += 1;
        } else if let Some(sp) = star_p {
            p_idx = sp + 1;
            star_t += 1;
            t_idx = star_t;
        } else {
            return false;
        }
    }

    while p_idx < p_bytes.len() && p_bytes[p_idx] == b'*' {
        p_idx += 1;
    }

    p_idx == p_bytes.len()
}

/// Simple, zero-dependency fast glob matcher supporting `*` and `?`.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), text.as_bytes())
}

/// Check if haystack contains needle, respecting smart-case (case-sensitive if needle has uppercase, case-insensitive otherwise).
#[inline]
pub fn bytes_contains_smart_case(haystack: &[u8], needle: &[u8], has_uppercase: bool) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }
    if has_uppercase {
        haystack.windows(needle.len()).any(|w| w == needle)
    } else {
        haystack
            .windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle))
    }
}

/// Check if path contains pattern as an exact path component, respecting smart-case.
/// A match occurs when pattern appears bounded by path boundaries or slashes (`/` or `\`).
#[inline]
pub fn path_contains_component_smart_case(
    path: &[u8],
    pattern: &[u8],
    has_uppercase: bool,
) -> bool {
    if pattern.is_empty() {
        return false;
    }
    if path.len() < pattern.len() {
        return false;
    }
    let mut start = 0;
    while start + pattern.len() <= path.len() {
        let end = start + pattern.len();
        let valid_before = start == 0 || path[start - 1] == b'/' || path[start - 1] == b'\\';
        let valid_after = end == path.len() || path[end] == b'/' || path[end] == b'\\';
        if valid_before && valid_after {
            let slice = &path[start..end];
            let matches = if has_uppercase {
                slice == pattern
            } else {
                slice.eq_ignore_ascii_case(pattern)
            };
            if matches {
                return true;
            }
        }
        start += 1;
    }
    false
}

/// Hierarchical ignore engine combining global excludes, CLI flags, type filters,
/// and discovered `.gitignore` / `.ignore` files.
#[derive(Debug, Clone, Default)]
pub struct GitignoreEngine {
    /// Explicit path exclusions (e.g. from CLI or config: target/, node_modules/).
    pub default_excludes: Vec<String>,
    /// Explicit user path exclusions (e.g. from CLI/DSL: `np:proj`, `no-path:steam`, `no:proj/`).
    /// These perform smart-case substring and segment matching without requiring shell wildcards.
    pub path_excludes: Vec<String>,
    /// File type or extension inclusions (e.g. rs, toml, *.c).
    pub type_includes: Vec<String>,
    /// File type or extension exclusions (e.g. c, h).
    pub type_excludes: Vec<String>,
    /// Dynamic rules loaded from `.gitignore` or `.ignore` files.
    pub rules: Vec<IgnoreRule>,
    /// Respect VCS ignore files (.gitignore, .ignore).
    pub respect_ignore: bool,
    /// Search hidden files.
    pub search_hidden: bool,
}

impl GitignoreEngine {
    pub fn new(
        default_excludes: Vec<String>,
        type_includes: Vec<String>,
        type_excludes: Vec<String>,
        respect_ignore: bool,
        search_hidden: bool,
    ) -> Self {
        Self {
            default_excludes,
            path_excludes: Vec::new(),
            type_includes,
            type_excludes,
            rules: Vec::new(),
            respect_ignore,
            search_hidden,
        }
    }

    /// Builder method to set user path exclusions.
    pub fn with_path_excludes(mut self, path_excludes: Vec<String>) -> Self {
        self.path_excludes = path_excludes;
        self
    }

    /// Load global ignore rules from user dotfiles and XDG locations:
    /// 1. $XDG_CONFIG_HOME/git/ignore (or ~/.config/git/ignore) - lowest precedence
    /// 2. ~/.ignore - higher precedence
    /// 3. $XDG_CONFIG_HOME/grx/ignore (or ~/.config/grx/ignore) - highest precedence
    pub fn load_global_ignore_files(&mut self) {
        if !self.respect_ignore {
            return;
        }

        // 1. Git global ignore: $XDG_CONFIG_HOME/git/ignore or ~/.config/git/ignore
        if let Some(git_ignore) = crate::config::Config::global_git_ignore_path() {
            self.load_ignore_file(&git_ignore, Path::new(""));
        }

        // 2. ~/.ignore
        if let Ok(home) = std::env::var("HOME")
            && !home.trim().is_empty()
        {
            let dot_ignore = PathBuf::from(home.trim()).join(".ignore");
            if dot_ignore.is_file() {
                self.load_ignore_file(&dot_ignore, Path::new(""));
            }
        }

        // 3. Grx user ignore: $XDG_CONFIG_HOME/grx/ignore or ~/.config/grx/ignore
        let grx_ignore = crate::config::Config::global_ignore_path();
        if grx_ignore.is_file() {
            self.load_ignore_file(&grx_ignore, Path::new(""));
        }
    }

    /// Helper to read and parse rules from an arbitrary ignore file path.
    pub fn load_ignore_file(&mut self, path: &Path, scope: &Path) {
        if let Ok(content) = fs::read_to_string(path) {
            for line in content.lines() {
                if let Some(rule) = IgnoreRule::parse(line, scope.to_path_buf()) {
                    self.rules.push(rule);
                }
            }
        }
    }

    /// Load ignore rules from `.gitignore` or `.ignore` in a newly entered directory.
    /// Loaded in order `.gitignore` then `.ignore` so `.ignore` takes higher precedence in `iter().rev()`.
    pub fn load_dir_ignore_files(&mut self, dir_path: &Path) {
        if !self.respect_ignore {
            return;
        }
        for filename in &[".gitignore", ".ignore"] {
            let path = dir_path.join(filename);
            if path.is_file() {
                self.load_ignore_file(&path, dir_path);
            }
        }
    }

    /// Check if dir contains .gitignore or .ignore. If so, return a new engine with those rules appended.
    /// Otherwise, return None (signifying parent engine can be reused).
    pub fn extend_for_dir(&self, dir: &Path) -> Option<Self> {
        if !self.respect_ignore {
            return None;
        }
        let mut new_rules = Vec::new();
        for filename in &[".gitignore", ".ignore"] {
            let path = dir.join(filename);
            if path.is_file()
                && let Ok(content) = fs::read_to_string(&path)
            {
                for line in content.lines() {
                    if let Some(rule) = IgnoreRule::parse(line, dir.to_path_buf()) {
                        new_rules.push(rule);
                    }
                }
            }
        }

        if new_rules.is_empty() {
            None
        } else {
            let mut cloned = self.clone();
            cloned.rules.extend(new_rules);
            Some(cloned)
        }
    }

    /// Check if filename bytes match an extension or glob in the list (zero-allocation).
    #[inline]
    pub fn matches_ext_list_bytes(name: &[u8], exts: &[String]) -> bool {
        for ext in exts {
            if (ext.contains('*') || ext.contains('?')) && glob_match_bytes(ext.as_bytes(), name) {
                return true;
            }
            let clean = ext.trim_start_matches('.').trim_start_matches('*');
            let clean_bytes = clean.as_bytes();
            if name.eq_ignore_ascii_case(clean_bytes) {
                return true;
            }
            if clean_bytes.starts_with(b".") {
                if name.len() >= clean_bytes.len()
                    && name[name.len() - clean_bytes.len()..].eq_ignore_ascii_case(clean_bytes)
                {
                    return true;
                }
            } else if name.len() > clean_bytes.len()
                && name[name.len() - clean_bytes.len()..].eq_ignore_ascii_case(clean_bytes)
                && name[name.len() - clean_bytes.len() - 1] == b'.'
            {
                return true;
            }
        }
        false
    }

    /// Check if filename has an extension matching any in the list (backward compatibility wrapper).
    pub fn matches_ext_list(name: &str, exts: &[String]) -> bool {
        Self::matches_ext_list_bytes(name.as_bytes(), exts)
    }
}

impl IgnoreFilter for GitignoreEngine {
    fn for_dir(&self, dir: &Path) -> Option<std::sync::Arc<dyn IgnoreFilter>> {
        self.extend_for_dir(dir)
            .map(|e| std::sync::Arc::new(e) as std::sync::Arc<dyn IgnoreFilter>)
    }

    fn filter(&self, parent: &Path, name: &[u8], is_dir: bool) -> FilterDecision {
        // Fast zero-alloc hidden file check (names starting with '.')
        if !self.search_hidden && name.starts_with(b".") && name != b"." && name != b".." {
            return if is_dir {
                FilterDecision::SkipDir
            } else {
                FilterDecision::Exclude
            };
        }

        // Fast default / CLI excludes check against basename first
        let mut full_path: Option<PathBuf> = None;
        let mut clean_path_bytes: Option<Vec<u8>> = None;

        for excl in &self.default_excludes {
            let clean_excl = excl.trim_end_matches('/');
            let clean_excl_bytes = clean_excl.as_bytes();
            let has_uppercase = clean_excl_bytes.iter().any(|b| b.is_ascii_uppercase());

            if clean_excl.contains('*') || clean_excl.contains('?') {
                let matches_name = if has_uppercase {
                    glob_match_bytes(clean_excl_bytes, name)
                } else {
                    glob_match_bytes_case_insensitive(clean_excl_bytes, name)
                };
                if matches_name {
                    return if is_dir {
                        FilterDecision::SkipDir
                    } else {
                        FilterDecision::Exclude
                    };
                }
                let p_bytes = clean_path_bytes.get_or_insert_with(|| {
                    let p = full_path.get_or_insert_with(|| {
                        let name_str = String::from_utf8_lossy(name);
                        parent.join(name_str.as_ref())
                    });
                    let s = p.to_string_lossy();
                    s.trim_start_matches("./").as_bytes().to_vec()
                });
                let matches_path = if has_uppercase {
                    glob_match_bytes(clean_excl_bytes, p_bytes)
                } else {
                    glob_match_bytes_case_insensitive(clean_excl_bytes, p_bytes)
                };
                if matches_path {
                    return if is_dir {
                        FilterDecision::SkipDir
                    } else {
                        FilterDecision::Exclude
                    };
                }
            } else if if has_uppercase {
                name == clean_excl_bytes
            } else {
                name.eq_ignore_ascii_case(clean_excl_bytes)
            } {
                return if is_dir {
                    FilterDecision::SkipDir
                } else {
                    FilterDecision::Exclude
                };
            } else if clean_excl.contains('/') || clean_excl.contains('\\') {
                let p_bytes = clean_path_bytes.get_or_insert_with(|| {
                    let p = full_path.get_or_insert_with(|| {
                        let name_str = String::from_utf8_lossy(name);
                        parent.join(name_str.as_ref())
                    });
                    let s = p.to_string_lossy();
                    let trimmed = s
                        .strip_prefix("./")
                        .or_else(|| s.strip_prefix(".\\"))
                        .unwrap_or(&s);
                    trimmed.as_bytes().to_vec()
                });
                let matches_exact = if has_uppercase {
                    path_equals_normalized(p_bytes.as_slice(), clean_excl_bytes)
                        || (p_bytes.len() > clean_excl_bytes.len()
                            && p_bytes.ends_with(clean_excl_bytes)
                            && (p_bytes[p_bytes.len() - clean_excl_bytes.len() - 1] == b'/'
                                || p_bytes[p_bytes.len() - clean_excl_bytes.len() - 1] == b'\\'))
                } else {
                    path_equals_normalized_ci(p_bytes.as_slice(), clean_excl_bytes)
                        || (p_bytes.len() > clean_excl_bytes.len()
                            && p_bytes[p_bytes.len() - clean_excl_bytes.len()..]
                                .eq_ignore_ascii_case(clean_excl_bytes)
                            && (p_bytes[p_bytes.len() - clean_excl_bytes.len() - 1] == b'/'
                                || p_bytes[p_bytes.len() - clean_excl_bytes.len() - 1] == b'\\'))
                };
                if matches_exact {
                    return if is_dir {
                        FilterDecision::SkipDir
                    } else {
                        FilterDecision::Exclude
                    };
                }
            }
        }

        // Check user-specified path exclusions (np:proj, no-path:steam, no:proj/, etc.)
        // These support shell-safe substring and segment matching without requiring glob wildcards.
        for excl in &self.path_excludes {
            let only_dir = excl.ends_with('/') || excl.ends_with('\\');
            if only_dir && !is_dir {
                continue;
            }
            let clean_excl = excl.trim_end_matches(['/', '\\']);
            let clean_excl_bytes = clean_excl.as_bytes();
            let has_uppercase = clean_excl_bytes.iter().any(|b| b.is_ascii_uppercase());

            if clean_excl.contains('*') || clean_excl.contains('?') {
                let matches_name = if has_uppercase {
                    glob_match_bytes(clean_excl_bytes, name)
                } else {
                    glob_match_bytes_case_insensitive(clean_excl_bytes, name)
                };
                if matches_name {
                    return if is_dir {
                        FilterDecision::SkipDir
                    } else {
                        FilterDecision::Exclude
                    };
                }
                let p_bytes = clean_path_bytes.get_or_insert_with(|| {
                    let p = full_path.get_or_insert_with(|| {
                        let name_str = String::from_utf8_lossy(name);
                        parent.join(name_str.as_ref())
                    });
                    let s = p.to_string_lossy();
                    let trimmed = s
                        .strip_prefix("./")
                        .or_else(|| s.strip_prefix(".\\"))
                        .unwrap_or(&s);
                    trimmed.as_bytes().to_vec()
                });
                let matches_path = if has_uppercase {
                    glob_match_bytes(clean_excl_bytes, p_bytes)
                } else {
                    glob_match_bytes_case_insensitive(clean_excl_bytes, p_bytes)
                };
                if matches_path {
                    return if is_dir {
                        FilterDecision::SkipDir
                    } else {
                        FilterDecision::Exclude
                    };
                }
            } else if only_dir {
                // Directory component matching:
                // When a directory exclusion was specified with a trailing slash (e.g. no:bin/ or !bin/),
                // enforce component boundary matching rather than arbitrary substring matching,
                // preventing "no:bin/" from unintentionally excluding "combine/" or "cabin/".
                let matches_component = if !clean_excl.contains('/') && !clean_excl.contains('\\') {
                    if has_uppercase {
                        name == clean_excl_bytes
                    } else {
                        name.eq_ignore_ascii_case(clean_excl_bytes)
                    }
                } else {
                    false
                };

                if matches_component {
                    return FilterDecision::SkipDir;
                }

                // Check full/relative path for multi-component directory exclusion or nested boundary
                let p_bytes = clean_path_bytes.get_or_insert_with(|| {
                    let p = full_path.get_or_insert_with(|| {
                        let name_str = String::from_utf8_lossy(name);
                        parent.join(name_str.as_ref())
                    });
                    let s = p.to_string_lossy();
                    let trimmed = s
                        .strip_prefix("./")
                        .or_else(|| s.strip_prefix(".\\"))
                        .unwrap_or(&s);
                    trimmed.as_bytes().to_vec()
                });

                if path_contains_component_smart_case(p_bytes, clean_excl_bytes, has_uppercase) {
                    return FilterDecision::SkipDir;
                }
            } else {
                // Substring matching: check if directory or file name contains exclude pattern
                if bytes_contains_smart_case(name, clean_excl_bytes, has_uppercase) {
                    return if is_dir {
                        FilterDecision::SkipDir
                    } else {
                        FilterDecision::Exclude
                    };
                }
                // Or if full path contains exclude pattern (e.g. np:Data/Projects or np:vendor/)
                let p_bytes = clean_path_bytes.get_or_insert_with(|| {
                    let p = full_path.get_or_insert_with(|| {
                        let name_str = String::from_utf8_lossy(name);
                        parent.join(name_str.as_ref())
                    });
                    let s = p.to_string_lossy();
                    let trimmed = s
                        .strip_prefix("./")
                        .or_else(|| s.strip_prefix(".\\"))
                        .unwrap_or(&s);
                    trimmed.as_bytes().to_vec()
                });
                if bytes_contains_smart_case(p_bytes, clean_excl_bytes, has_uppercase) {
                    return if is_dir {
                        FilterDecision::SkipDir
                    } else {
                        FilterDecision::Exclude
                    };
                }
            }
        }

        // If it's a regular file, apply filetype exclusions and inclusions directly on bytes
        if !is_dir {
            // Type exclusions (e.g. no:c,h)
            if !self.type_excludes.is_empty()
                && Self::matches_ext_list_bytes(name, &self.type_excludes)
            {
                return FilterDecision::Exclude;
            }

            // Type inclusions (e.g. :rs, :toml)
            if !self.type_includes.is_empty()
                && !Self::matches_ext_list_bytes(name, &self.type_includes)
            {
                return FilterDecision::Exclude;
            }
        }

        // Apply hierarchical gitignore rules (in reverse order: latest rule takes precedence)
        for rule in self.rules.iter().rev() {
            // Fast path: pure extension or exact basename rules without path overhead
            if let Some(matched) = rule.matches_name(name, is_dir) {
                if matched {
                    if rule.is_negation {
                        return FilterDecision::Include;
                    } else {
                        return if is_dir {
                            FilterDecision::SkipDir
                        } else {
                            FilterDecision::Exclude
                        };
                    }
                }
                continue;
            }

            // Slow path: complex globs or scoped patterns requiring relative path resolution
            let p = full_path.get_or_insert_with(|| {
                let name_str = String::from_utf8_lossy(name);
                parent.join(name_str.as_ref())
            });
            if let Ok(rel) = p.strip_prefix(&rule.scope)
                && rule.matches_bytes(rel.as_os_str().as_encoded_bytes(), is_dir)
            {
                if rule.is_negation {
                    return FilterDecision::Include;
                } else {
                    return if is_dir {
                        FilterDecision::SkipDir
                    } else {
                        FilterDecision::Exclude
                    };
                }
            }
        }

        FilterDecision::Include
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_matcher() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("*.min.js", "bundle.min.js"));
        assert!(!glob_match("*.rs", "main.c"));
        assert!(glob_match("tar*", "target"));
        assert!(glob_match("tar*", "tarball"));
        assert!(glob_match("t?st", "test"));
        assert!(glob_match("t?st", "tast"));
        assert!(!glob_match("t?st", "toast"));
    }

    #[test]
    fn test_ignore_rule_parsing() {
        let rule = IgnoreRule::parse("target/", PathBuf::from("/repo")).unwrap();
        assert_eq!(rule.pattern, "target");
        assert!(rule.dir_only);
        assert!(!rule.is_negation);

        let neg_rule = IgnoreRule::parse("!*.rs", PathBuf::from("/repo")).unwrap();
        assert_eq!(neg_rule.pattern, "*.rs");
        assert!(!neg_rule.dir_only);
        assert!(neg_rule.is_negation);
    }

    #[test]
    fn test_gitignore_engine_filtering() {
        let engine = GitignoreEngine::new(
            vec!["target/".to_string(), "node_modules/".to_string()],
            vec!["rs".to_string()],
            vec!["min.js".to_string()],
            true,
            false,
        );

        let parent = Path::new("/workspace");

        // target/ dir should be SkipDir
        assert_eq!(
            engine.filter(parent, b"target", true),
            FilterDecision::SkipDir
        );

        // .git dir should be SkipDir because hidden files are off
        assert_eq!(
            engine.filter(parent, b".git", true),
            FilterDecision::SkipDir
        );

        // Rust file should be included
        assert_eq!(
            engine.filter(parent, b"main.rs", false),
            FilterDecision::Include
        );

        // C file should be excluded because type_includes is ["rs"]
        assert_eq!(
            engine.filter(parent, b"main.c", false),
            FilterDecision::Exclude
        );
    }

    #[test]
    fn test_matches_ext_list_exact_name() {
        let types = vec!["Makefile".to_string(), "rs".to_string()];
        assert!(GitignoreEngine::matches_ext_list_bytes(b"Makefile", &types));
        assert!(GitignoreEngine::matches_ext_list_bytes(b"main.rs", &types));
        assert!(!GitignoreEngine::matches_ext_list_bytes(b"main.c", &types));
    }

    #[test]
    fn test_matches_ext_list_wildcard_globs() {
        let globs = vec!["*test*".to_string(), "foo_*.rs".to_string()];
        assert!(GitignoreEngine::matches_ext_list_bytes(
            b"test_helper.rs",
            &globs
        ));
        assert!(GitignoreEngine::matches_ext_list_bytes(
            b"my_test.py",
            &globs
        ));
        assert!(GitignoreEngine::matches_ext_list_bytes(
            b"foo_bar.rs",
            &globs
        ));
        assert!(!GitignoreEngine::matches_ext_list_bytes(
            b"bar_foo.rs",
            &globs
        ));
    }

    #[test]
    fn test_load_ignore_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ignore_file = temp_dir.path().join(".ignore");
        fs::write(&ignore_file, "custom_secret/\n*.tmp\n!important.tmp\n").unwrap();

        let mut engine = GitignoreEngine::new(Vec::new(), Vec::new(), Vec::new(), true, false);
        engine.load_ignore_file(&ignore_file, temp_dir.path());
        assert_eq!(engine.rules.len(), 3);
        assert_eq!(engine.rules[0].pattern, "custom_secret");
        assert!(engine.rules[0].dir_only);
        assert_eq!(engine.rules[1].pattern, "*.tmp");
        assert!(engine.rules[2].is_negation);
    }

    #[test]
    fn test_extend_for_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let sub_dir = temp_dir.path().join("sub_repo");
        fs::create_dir_all(&sub_dir).unwrap();
        let gitignore = sub_dir.join(".gitignore");
        fs::write(&gitignore, "models/\n*.cache\n").unwrap();

        let engine = GitignoreEngine::new(Vec::new(), Vec::new(), Vec::new(), true, false);

        // Parent dir has no .gitignore, should return None
        assert!(engine.extend_for_dir(temp_dir.path()).is_none());

        // sub_dir has .gitignore, should return Some with new rules
        let scoped = engine.extend_for_dir(&sub_dir).expect("should load rules");
        assert_eq!(scoped.rules.len(), 2);
        assert_eq!(scoped.rules[0].pattern, "models");
        assert_eq!(
            scoped.filter(&sub_dir, b"models", true),
            FilterDecision::SkipDir
        );
    }

    #[test]
    fn test_fast_extension_and_exact_rules() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ignore_file = temp_dir.path().join(".gitignore");
        fs::write(&ignore_file, "*.o\n*.tar.gz\ntags\n!special.o\n").unwrap();

        let mut engine = GitignoreEngine::new(Vec::new(), Vec::new(), Vec::new(), true, false);
        engine.load_ignore_file(&ignore_file, temp_dir.path());

        // *.o should exclude ordinary .o files
        assert_eq!(
            engine.filter(temp_dir.path(), b"vmlinux.o", false),
            FilterDecision::Exclude
        );

        // !special.o should be included (negation)
        assert_eq!(
            engine.filter(temp_dir.path(), b"special.o", false),
            FilterDecision::Include
        );

        // *.tar.gz compound extension check
        assert_eq!(
            engine.filter(temp_dir.path(), b"archive.tar.gz", false),
            FilterDecision::Exclude
        );
        assert_eq!(
            engine.filter(temp_dir.path(), b"archive.gz", false),
            FilterDecision::Include
        );

        // tags exact basename check
        assert_eq!(
            engine.filter(temp_dir.path(), b"tags", false),
            FilterDecision::Exclude
        );
        assert_eq!(
            engine.filter(temp_dir.path(), b"TAGS", false),
            FilterDecision::Include
        );
    }

    #[test]
    fn test_path_excludes_smart_case() {
        let parent = Path::new("/workspace");

        // Shell-safe substring matching without wildcards: "steam" matches "SteamLibrary" and "steamapps"
        let engine_sub = GitignoreEngine::new(Vec::new(), Vec::new(), Vec::new(), true, false)
            .with_path_excludes(vec!["steam".to_string()]);

        assert_eq!(
            engine_sub.filter(parent, b"SteamLibrary", true),
            FilterDecision::SkipDir
        );
        assert_eq!(
            engine_sub.filter(parent, b"steamapps", true),
            FilterDecision::SkipDir
        );
        assert_eq!(
            engine_sub.filter(parent, b"other", true),
            FilterDecision::Include
        );

        // Substring matching with "proj": matches "Projects" and "cprojects"
        let engine_proj = GitignoreEngine::new(Vec::new(), Vec::new(), Vec::new(), true, false)
            .with_path_excludes(vec!["proj".to_string()]);

        assert_eq!(
            engine_proj.filter(parent, b"Projects", true),
            FilterDecision::SkipDir
        );
        assert_eq!(
            engine_proj.filter(parent, b"cprojects", true),
            FilterDecision::SkipDir
        );
        assert_eq!(
            engine_proj.filter(parent, b"target", true),
            FilterDecision::Include
        );

        // Trailing slash "proj/" matches directory component "proj", but does NOT match "Projects" or regular file "proj.rs"
        let engine_dir_only = GitignoreEngine::new(Vec::new(), Vec::new(), Vec::new(), true, false)
            .with_path_excludes(vec!["proj/".to_string()]);

        assert_eq!(
            engine_dir_only.filter(parent, b"proj", true),
            FilterDecision::SkipDir
        );
        assert_eq!(
            engine_dir_only.filter(parent, b"Projects", true),
            FilterDecision::Include
        );
        assert_eq!(
            engine_dir_only.filter(parent, b"proj.rs", false),
            FilterDecision::Include
        );

        // Component boundary matching for directory exclusions: "bin/" matches "bin" and "target/bin", but NOT "combine" or "cabin"
        let engine_bin = GitignoreEngine::new(Vec::new(), Vec::new(), Vec::new(), true, false)
            .with_path_excludes(vec!["bin/".to_string()]);

        assert_eq!(
            engine_bin.filter(parent, b"bin", true),
            FilterDecision::SkipDir
        );
        assert_eq!(
            engine_bin.filter(parent, b"combine", true),
            FilterDecision::Include
        );
        assert_eq!(
            engine_bin.filter(parent, b"cabin", true),
            FilterDecision::Include
        );
        assert_eq!(
            engine_bin.filter(parent.join("target").as_path(), b"bin", true),
            FilterDecision::SkipDir
        );
        assert_eq!(
            engine_bin.filter(parent.join("src").as_path(), b"combine", true),
            FilterDecision::Include
        );

        // Uppercase "Steam" matches SteamLibrary, but not steamapps (smart-case)
        let engine_upper_sub =
            GitignoreEngine::new(Vec::new(), Vec::new(), Vec::new(), true, false)
                .with_path_excludes(vec!["Steam".to_string()]);

        assert_eq!(
            engine_upper_sub.filter(parent, b"SteamLibrary", true),
            FilterDecision::SkipDir
        );
        assert_eq!(
            engine_upper_sub.filter(parent, b"steamapps", true),
            FilterDecision::Include
        );

        // Lowercase glob *steam* in default_excludes (backward compatibility)
        let engine_glob = GitignoreEngine::new(
            vec!["*steam*".to_string()],
            Vec::new(),
            Vec::new(),
            true,
            false,
        );
        assert_eq!(
            engine_glob.filter(parent, b"SteamLibrary", true),
            FilterDecision::SkipDir
        );
        assert_eq!(
            engine_glob.filter(parent, b"steamapps", true),
            FilterDecision::SkipDir
        );

        // Exact directory name in default_excludes matches exact directory
        let engine_exact = GitignoreEngine::new(
            vec!["target/".to_string()],
            Vec::new(),
            Vec::new(),
            true,
            false,
        );
        assert_eq!(
            engine_exact.filter(parent, b"target", true),
            FilterDecision::SkipDir
        );
        assert_eq!(
            engine_exact.filter(parent, b"targeted_ads", true),
            FilterDecision::Include
        );
    }

    #[test]
    fn test_slashless_glob_and_rooted_patterns() {
        let rule_glob = IgnoreRule::parse("debug_*.txt", PathBuf::from("")).unwrap();
        // Matches in root
        assert!(rule_glob.matches_bytes(b"debug_output.txt", false));
        // Matches in subdirectory
        assert!(rule_glob.matches_bytes(b"sub/debug_output.txt", false));
        assert!(rule_glob.matches_bytes(b"a/b/debug_test.txt", false));
        assert!(!rule_glob.matches_bytes(b"sub/other.txt", false));

        let rule_rooted = IgnoreRule::parse("/build", PathBuf::from("")).unwrap();
        // Matches at root
        assert!(rule_rooted.matches_bytes(b"build", true));
        // Does NOT match in subdirectory
        assert!(!rule_rooted.matches_bytes(b"sub/build", true));
    }

    #[test]
    fn test_ignore_precedence_over_gitignore() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        // .gitignore ignores *.log
        fs::write(dir.join(".gitignore"), b"*.log\n").unwrap();
        // .ignore un-ignores important.log
        fs::write(dir.join(".ignore"), b"!important.log\n").unwrap();

        let mut engine = GitignoreEngine::new(Vec::new(), Vec::new(), Vec::new(), true, false);
        engine.load_dir_ignore_files(dir);

        // important.log should be included because .ignore takes precedence over .gitignore
        let decision = engine.filter(dir, b"important.log", false);
        assert_eq!(decision, FilterDecision::Include);

        // other.log should still be excluded by .gitignore
        let decision_other = engine.filter(dir, b"other.log", false);
        assert_eq!(decision_other, FilterDecision::Exclude);
    }

    #[test]
    fn test_matches_ext_list_bytes_case_insensitive() {
        let exts = vec!["rs".to_string(), "py".to_string(), "c".to_string()];
        assert!(GitignoreEngine::matches_ext_list_bytes(b"main.rs", &exts));
        assert!(GitignoreEngine::matches_ext_list_bytes(b"MAIN.RS", &exts));
        assert!(GitignoreEngine::matches_ext_list_bytes(b"script.Py", &exts));
        assert!(GitignoreEngine::matches_ext_list_bytes(b"CODE.C", &exts));
        assert!(!GitignoreEngine::matches_ext_list_bytes(b"file.txt", &exts));
    }

    #[test]
    fn test_windows_path_separators_in_glob_and_component_matching() {
        // Component matching with backslashes
        assert!(path_matches_component(b"target\\debug\\grx.exe", b"target"));
        assert!(path_matches_component(
            b"sub\\node_modules\\pkg\\index.js",
            b"node_modules"
        ));
        assert!(path_matches_component(b"foo/bar\\baz", b"bar"));
        assert!(!path_matches_component(b"target_dir\\file.txt", b"target"));

        // Glob matching with mixed / and \
        assert!(glob_match("src/*.rs", "src\\main.rs"));
        assert!(glob_match("src\\*.rs", "src/main.rs"));
        assert!(glob_match(
            "tests/**/test.rs",
            "tests\\nested\\deep\\test.rs"
        ));

        // Gitignore rule matching with Windows path
        let rule = IgnoreRule::parse("target/", PathBuf::from("")).unwrap();
        assert!(rule.matches_bytes(b"target", true));
        assert!(rule.matches_bytes(b"target\\debug", true));

        let rule_pattern = IgnoreRule::parse("*.rs", PathBuf::from("")).unwrap();
        assert!(rule_pattern.matches_bytes(b"src\\main.rs", false));
        assert!(rule_pattern.matches_bytes(b"src/main.rs", false));
    }
}
