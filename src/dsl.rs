use std::path::PathBuf;

/// High-level search pattern representations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchPattern {
    /// Standard literal pattern with optional explicit case sensitivity.
    /// case_sensitive: None = smart-case (auto), Some(true) = case-sensitive, Some(false) = case-insensitive.
    Literal {
        text: String,
        case_sensitive: Option<bool>,
    },
    /// Guaranteed exact literal (quoted or prefixed with '='): no regex, case-preserving.
    ExactLiteral(String),
    /// Explicit regular expression pattern.
    Regex(String),
    /// Raw hex byte pattern with optional wildcard bytes (e.g. 48 ?? e5).
    Hex(Vec<Option<u8>>),
    /// Empty pattern set (e.g. from an empty pattern file), matching zero lines.
    EmptySet,
}

impl SearchPattern {
    /// Returns whether this pattern should match case-sensitively.
    pub fn is_case_sensitive(&self) -> bool {
        match self {
            SearchPattern::Literal {
                text,
                case_sensitive,
            } => case_sensitive.unwrap_or_else(|| text.chars().any(|c| c.is_uppercase())),
            SearchPattern::ExactLiteral(_) => true,
            SearchPattern::Regex(_) => true,
            SearchPattern::Hex(_) => true,
            SearchPattern::EmptySet => true,
        }
    }
}

/// Abstract Syntax Tree representing boolean combinations of search patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryExpr {
    /// Atomic pattern match.
    Pattern(SearchPattern),
    /// Both subexpressions must match on the same line.
    And(Box<QueryExpr>, Box<QueryExpr>),
    /// Either subexpression matches on the line.
    Or(Box<QueryExpr>, Box<QueryExpr>),
    /// Invert match: line must NOT match the subexpression.
    Not(Box<QueryExpr>),
}

/// Target filesystem entry kind constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Link,
    Bin,
    Text,
}

/// File size threshold predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizePredicate {
    Larger(u64),
    Smaller(u64),
}

impl SizePredicate {
    #[inline]
    pub fn matches(&self, size: u64) -> bool {
        match self {
            SizePredicate::Larger(val) => size > *val,
            SizePredicate::Smaller(val) => size < *val,
        }
    }
}

/// File modification age predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimePredicate {
    Newer(std::time::Duration),
    Older(std::time::Duration),
}

impl TimePredicate {
    pub fn matches(&self, mtime: std::time::SystemTime, reference: std::time::SystemTime) -> bool {
        match self {
            TimePredicate::Newer(duration) => match reference.duration_since(mtime) {
                Ok(age) => age < *duration,
                Err(_) => true, // Future timestamp is newer than any past duration threshold
            },
            TimePredicate::Older(duration) => match reference.duration_since(mtime) {
                Ok(age) => age > *duration,
                Err(_) => false,
            },
        }
    }
}

/// Compiled basename filter with anchor and wildcard sequence matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasenameFilter {
    pub raw: String,
    pub is_exact: bool,
    pub start_anchored: bool,
    pub end_anchored: bool,
    pub parts: Vec<Vec<u8>>,
    pub is_case_sensitive: bool,
}

impl BasenameFilter {
    pub fn parse(pattern: &str) -> Self {
        if let Some(rest) = pattern.strip_prefix('=') {
            return Self {
                raw: pattern.to_string(),
                is_exact: true,
                start_anchored: true,
                end_anchored: true,
                parts: vec![rest.as_bytes().to_vec()],
                is_case_sensitive: true,
            };
        }

        let (start_anchored, after_start) = if let Some(rest) = pattern.strip_prefix('^') {
            (true, rest)
        } else {
            (false, pattern)
        };

        let (end_anchored, body) = if let Some(rest) = after_start.strip_suffix('$') {
            (true, rest)
        } else {
            (false, after_start)
        };

        let has_uppercase = body.bytes().any(|b| b.is_ascii_uppercase());
        let parts: Vec<Vec<u8>> = body
            .split("..")
            .filter(|s| !s.is_empty())
            .map(|s| s.as_bytes().to_vec())
            .collect();

        Self {
            raw: pattern.to_string(),
            is_exact: false,
            start_anchored,
            end_anchored,
            parts,
            is_case_sensitive: has_uppercase,
        }
    }

    pub fn matches(&self, basename: &[u8], case_override: Option<bool>) -> bool {
        let cs = case_override.unwrap_or(self.is_case_sensitive);

        if self.is_exact {
            let target = &self.parts[0];
            return if cs {
                basename == target.as_slice()
            } else {
                basename.eq_ignore_ascii_case(target)
            };
        }

        if self.parts.is_empty() || (self.parts.len() == 1 && self.parts[0].is_empty()) {
            return if self.start_anchored && self.end_anchored {
                basename.is_empty()
            } else {
                true
            };
        }

        if self.parts.len() == 1 {
            let part = &self.parts[0];
            if self.start_anchored && self.end_anchored {
                return if cs {
                    basename == part.as_slice()
                } else {
                    basename.eq_ignore_ascii_case(part)
                };
            } else if self.start_anchored {
                if basename.len() < part.len() {
                    return false;
                }
                let prefix = &basename[..part.len()];
                return if cs {
                    prefix == part.as_slice()
                } else {
                    prefix.eq_ignore_ascii_case(part)
                };
            } else if self.end_anchored {
                if basename.len() < part.len() {
                    return false;
                }
                let suffix = &basename[basename.len() - part.len()..];
                return if cs {
                    suffix == part.as_slice()
                } else {
                    suffix.eq_ignore_ascii_case(part)
                };
            } else {
                return find_subslice(basename, part, cs).is_some();
            }
        }

        // Multiple parts separated by '..'
        let mut offset = 0;
        for (i, part) in self.parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }
            let is_first = i == 0;
            let is_last = i == self.parts.len() - 1;

            if is_first && self.start_anchored {
                if basename.len() < part.len() {
                    return false;
                }
                let prefix = &basename[..part.len()];
                let matches = if cs {
                    prefix == part.as_slice()
                } else {
                    prefix.eq_ignore_ascii_case(part)
                };
                if !matches {
                    return false;
                }
                offset = part.len();
            } else if is_last && self.end_anchored {
                if basename.len() < offset + part.len() {
                    return false;
                }
                let suffix = &basename[basename.len() - part.len()..];
                let matches = if cs {
                    suffix == part.as_slice()
                } else {
                    suffix.eq_ignore_ascii_case(part)
                };
                if !matches {
                    return false;
                }
            } else {
                let haystack = &basename[offset..];
                if let Some(pos) = find_subslice(haystack, part, cs) {
                    offset += pos + part.len();
                } else {
                    return false;
                }
            }
        }

        true
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8], case_sensitive: bool) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if haystack.len() < needle.len() {
        return None;
    }
    if case_sensitive {
        memchr::memmem::find(haystack, needle)
    } else {
        let has_alpha = needle.iter().any(|b| b.is_ascii_alphabetic());
        if !has_alpha {
            return memchr::memmem::find(haystack, needle);
        }
        let lower = needle[0].to_ascii_lowercase();
        let upper = needle[0].to_ascii_uppercase();
        let n_len = needle.len();
        for pos in memchr::memchr2_iter(lower, upper, haystack) {
            if pos + n_len <= haystack.len()
                && haystack[pos..pos + n_len].eq_ignore_ascii_case(needle)
            {
                return Some(pos);
            }
        }
        None
    }
}

fn parse_kind(s: &str) -> Result<EntryKind, String> {
    let val = s.strip_prefix("kind:").unwrap_or(s);
    match val {
        "file" => Ok(EntryKind::File),
        "dir" => Ok(EntryKind::Dir),
        "link" => Ok(EntryKind::Link),
        "bin" => Ok(EntryKind::Bin),
        "text" => Ok(EntryKind::Text),
        "f" => Err("Entry kind alias 'f' is deprecated. Use canonical 'kind:file'.".into()),
        "d" => Err("Entry kind alias 'd' is deprecated. Use canonical 'kind:dir'.".into()),
        "l" => Err("Entry kind alias 'l' is deprecated. Use canonical 'kind:link'.".into()),
        "binary" | "exe" => Err(format!(
            "Entry kind alias '{val}' is deprecated. Use canonical 'kind:bin'."
        )),
        "txt" => Err("Entry kind alias 'txt' is deprecated. Use canonical 'kind:text'.".into()),
        other => Err(format!(
            "Invalid entry kind '{other}' in '{s}': expected file, dir, link, bin, or text"
        )),
    }
}

fn parse_size_predicate(s: &str) -> Result<SizePredicate, String> {
    if let Some(rest) = s.strip_prefix("larger:") {
        parse_size_bytes(rest, s).map(SizePredicate::Larger)
    } else if let Some(rest) = s.strip_prefix("smaller:") {
        parse_size_bytes(rest, s).map(SizePredicate::Smaller)
    } else {
        Err(format!("Invalid size predicate '{s}'"))
    }
}

fn parse_size_bytes(val: &str, full: &str) -> Result<u64, String> {
    let val = val.trim();
    if val.is_empty() {
        return Err(format!("Empty size value in '{full}'"));
    }

    let (digits, multiplier) = if let Some(rest) = val.strip_suffix("GiB") {
        (rest, 1024u64 * 1024 * 1024)
    } else if let Some(rest) = val.strip_suffix("MiB") {
        (rest, 1024u64 * 1024)
    } else if let Some(rest) = val.strip_suffix("KiB") {
        (rest, 1024u64)
    } else if let Some(rest) = val.strip_suffix('B') {
        (rest, 1u64)
    } else {
        (val, 1u64)
    };

    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!(
            "Invalid size format in '{full}': expected integer with optional unit B, KiB, MiB, GiB"
        ));
    }

    let n = digits
        .parse::<u64>()
        .map_err(|_| format!("Size overflow in '{full}'"))?;
    n.checked_mul(multiplier)
        .ok_or_else(|| format!("Size overflow in '{full}'"))
}

fn parse_time_predicate(s: &str) -> Result<TimePredicate, String> {
    if let Some(rest) = s.strip_prefix("newer:") {
        parse_duration_seconds(rest, s).map(TimePredicate::Newer)
    } else if let Some(rest) = s.strip_prefix("older:") {
        parse_duration_seconds(rest, s).map(TimePredicate::Older)
    } else {
        Err(format!("Invalid time predicate '{s}'"))
    }
}

fn parse_duration_seconds(val: &str, full: &str) -> Result<std::time::Duration, String> {
    let val = val.trim();
    if val.is_empty() {
        return Err(format!("Empty time value in '{full}'"));
    }

    let (digits, unit_secs) = if let Some(rest) = val.strip_suffix('d') {
        (rest, 86400u64)
    } else if let Some(rest) = val.strip_suffix('h') {
        (rest, 3600u64)
    } else if let Some(rest) = val.strip_suffix('m') {
        (rest, 60u64)
    } else if let Some(rest) = val.strip_suffix('s') {
        (rest, 1u64)
    } else {
        return Err(format!(
            "Invalid time format in '{full}': expected unit s, m, h, or d"
        ));
    };

    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!(
            "Invalid time value in '{full}': expected integer followed by unit s, m, h, or d"
        ));
    }

    let n = digits
        .parse::<u64>()
        .map_err(|_| format!("Time overflow in '{full}'"))?;
    let secs = n
        .checked_mul(unit_secs)
        .ok_or_else(|| format!("Time overflow in '{full}'"))?;
    Ok(std::time::Duration::from_secs(secs))
}

/// Complete parsed search query containing the expression AST, target paths,
/// and file/path inclusion and exclusion filters.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    /// The search pattern expression (if any; None means list matching files).
    pub expr: Option<QueryExpr>,
    /// Whether an explicit content pattern was provided (either CLI pattern, external expr, or fuzzy).
    pub has_content_pattern: bool,
    /// Target files or directories to search (defaults to current directory if empty).
    pub targets: Vec<PathBuf>,
    /// Path/directory exclusion patterns (e.g. "target/", "node_modules/", "build/").
    pub path_excludes: Vec<String>,
    /// Path/directory inclusion patterns (e.g. "src/", "docs/").
    pub path_includes: Vec<String>,
    /// Basename pattern inclusions (e.g. "report", "^test", "foo..bar").
    pub basename_includes: Vec<String>,
    /// Compiled basename pattern inclusions.
    pub basename_filters: Vec<BasenameFilter>,
    /// Basename pattern exclusions (e.g. "report", "*.bak").
    pub basename_excludes: Vec<String>,
    /// Compiled basename pattern exclusions.
    pub basename_exclude_filters: Vec<BasenameFilter>,
    /// File type or extension inclusions (e.g. "rs", "toml", "*.c").
    pub type_includes: Vec<String>,
    /// File type or extension exclusions (e.g. "c", "h", "*.min.js").
    pub type_excludes: Vec<String>,
    /// Filesystem entry kind constraint (file, dir, link).
    pub kind: Option<EntryKind>,
    /// Size thresholds (larger:10MiB, smaller:1KiB).
    pub size_predicates: Vec<SizePredicate>,
    /// Modification age thresholds (newer:7d, older:1h).
    pub time_predicates: Vec<TimePredicate>,
    /// Search hidden files and directories (Some(true) = enable, Some(false) = disable, None = config default).
    pub search_hidden: Option<bool>,
    /// Follow directory symlinks during traversal.
    pub follow_symlinks: Option<bool>,
    /// Search cache directories (.cache/, Cache/, CachedData/).
    pub search_cache: bool,
    /// Respect .gitignore and .ignore rules (Some(true) = yes:ignore, Some(false) = no:ignore, None = cli/config default).
    pub respect_ignore: Option<bool>,
    /// Search inside binary files.
    pub include_binaries: bool,
    /// Search ONLY binary files.
    pub only_binaries: bool,
    /// Hex search mode.
    pub hex_mode: bool,
    /// Explicit case sensitivity override.
    pub case_sensitive: Option<bool>,
    /// Maximum recursive directory traversal depth (e.g. from d:0, depth:1).
    pub max_depth: Option<usize>,
    /// Maximum matching lines per file (e.g. from -m, m:10).
    pub max_count: Option<usize>,
    /// Limit results globally to first N (e.g. from --head, head:5, top:5, limit:5).
    pub head: Option<usize>,
    /// Maximum matching items to output from the end (e.g. from tail:5).
    pub tail: Option<usize>,
    /// Unified before and after context lines (e.g. from ctx:3).
    pub context: Option<usize>,
    /// Minimum length of printable strings for binary search (e.g. from str:4).
    pub binary_strings_min_len: Option<usize>,
    /// Proximity filters for neighborhood matching (near:N,pattern, no-near:N,pattern, NEAR).
    pub proximity_filters: Vec<ProximityFilter>,
    /// Fuzzy token query (e.g. from fz:, %%, or -Z).
    pub fuzzy: Option<FuzzyQuery>,
    /// Result sort order constraint (e.g. from sort:size, sort:modified, sort:len).
    pub sort: Option<SortKey>,
    /// Explicit empty pattern set from -f or -e with 0 patterns. Never matches anything and prevents discovery.
    pub empty_pattern_set: bool,
    /// Filesystem action requested via DSL (mv:<dest>, cp:<dest>, rm:, trash:).
    pub action: Option<crate::ops::ActionKind>,
    /// Simulate filesystem operations without mutating disk (dry:, --dry-run).
    pub dry_run: bool,
    /// Command execution template requested via inline DSL (--exec <CMD...>).
    pub exec: Vec<String>,
    /// Command batch execution requested via inline DSL (-X/--exec-batch <CMD...>).
    pub exec_batch: Vec<String>,
}

/// Result sorting criteria.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    /// File size ascending (smallest first).
    Size,
    /// File size descending (largest first).
    SizeDesc,
    /// Modification time descending (newest first).
    Modified,
    /// Modification time ascending (oldest first).
    ModifiedDesc,
    /// Alphabetical path ordering (a -> z).
    Path,
    /// Reverse alphabetical path ordering (z -> a).
    PathDesc,
    /// Path or line length ascending (shortest first).
    Len,
    /// Path or line length descending (longest first).
    LenDesc,
    /// Line number order.
    LineNum,
    /// Reverse line number order.
    LineNumDesc,
    /// Match count per file descending.
    Count,
    /// Match count per file ascending.
    CountDesc,
}

impl SortKey {
    /// Invert the sort direction.
    pub fn reversed(self) -> Self {
        match self {
            Self::Size => Self::SizeDesc,
            Self::SizeDesc => Self::Size,
            Self::Modified => Self::ModifiedDesc,
            Self::ModifiedDesc => Self::Modified,
            Self::Path => Self::PathDesc,
            Self::PathDesc => Self::Path,
            Self::Len => Self::LenDesc,
            Self::LenDesc => Self::Len,
            Self::LineNum => Self::LineNumDesc,
            Self::LineNumDesc => Self::LineNum,
            Self::Count => Self::CountDesc,
            Self::CountDesc => Self::Count,
        }
    }
}

/// Parse sort key string into SortKey.
pub fn parse_sort_key(key: &str, reverse: bool) -> Result<SortKey, String> {
    let key_lower = key.to_ascii_lowercase();
    let base = match key_lower.as_str() {
        "size" | "bytes" => {
            if reverse {
                SortKey::SizeDesc
            } else {
                SortKey::Size
            }
        }
        "largest" => {
            if reverse {
                SortKey::Size
            } else {
                SortKey::SizeDesc
            }
        }
        "smallest" => {
            if reverse {
                SortKey::SizeDesc
            } else {
                SortKey::Size
            }
        }
        "modified" | "time" | "age" | "date" => {
            if reverse {
                SortKey::ModifiedDesc
            } else {
                SortKey::Modified
            }
        }
        "newest" | "recent" => {
            if reverse {
                SortKey::ModifiedDesc
            } else {
                SortKey::Modified
            }
        }
        "oldest" => {
            if reverse {
                SortKey::Modified
            } else {
                SortKey::ModifiedDesc
            }
        }
        "path" | "name" => {
            if reverse {
                SortKey::PathDesc
            } else {
                SortKey::Path
            }
        }
        "len" | "length" | "path-len" | "line-len" | "linelen" => {
            if reverse {
                SortKey::LenDesc
            } else {
                SortKey::Len
            }
        }
        "shortest" => {
            if reverse {
                SortKey::LenDesc
            } else {
                SortKey::Len
            }
        }
        "longest" => {
            if reverse {
                SortKey::Len
            } else {
                SortKey::LenDesc
            }
        }
        "line" | "line-num" | "linenum" => {
            if reverse {
                SortKey::LineNumDesc
            } else {
                SortKey::LineNum
            }
        }
        "count" => {
            if reverse {
                SortKey::CountDesc
            } else {
                SortKey::Count
            }
        }
        _ => {
            return Err(format!(
                "Unrecognized sort key '{key}'. Supported keys: size, modified, path, len, line, count (e.g. sort:size, sort:-size, sort:newest, sort:shortest)"
            ));
        }
    };
    Ok(base)
}

impl Query {
    /// Returns true if this is a file discovery query (no content expression supplied).
    pub fn is_discovery(&self) -> bool {
        !self.has_content_pattern && !self.empty_pattern_set
    }

    /// Returns true if any filesystem entry selectors or filters are present.
    pub fn has_entry_selectors(&self) -> bool {
        !self.basename_filters.is_empty()
            || !self.basename_exclude_filters.is_empty()
            || !self.type_includes.is_empty()
            || !self.type_excludes.is_empty()
            || self.kind.is_some()
            || !self.size_predicates.is_empty()
            || !self.time_predicates.is_empty()
            || !self.path_excludes.is_empty()
            || !self.path_includes.is_empty()
            || !self.targets.is_empty()
    }
}

/// Proximity filter condition: pattern must appear within a specified line window of a match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProximityFilter {
    /// Target term to match in the vicinity.
    pub term: String,
    /// Number of lines before or after to inspect (window radius).
    pub window: usize,
    /// Inverted condition: target term must NOT appear in the vicinity.
    pub inverted: bool,
}

/// Fuzzy token query: matches lines containing all tokens in any permutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyQuery {
    /// Individual literal or identifier tokens to match.
    pub tokens: Vec<String>,
    /// Original unparsed input pattern.
    pub original: String,
}

/// Token generated by the DSL lexer from CLI positional arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// Result sort order (`sort:size`, `sort:modified`, etc.).
    Sort(SortKey),
    /// Boolean AND operator.
    And,
    /// Boolean OR operator.
    Or,
    /// Boolean NOT operator.
    Not,
    /// Line-level negative term match (e.g. `-forbidden`, `not:debug`).
    NegativeTerm(SearchPattern),
    /// Line-level positive term match (e.g. `+required`, `+auth`, `+w:test`).
    PositiveTerm(SearchPattern),
    /// Path or glob exclusion (from `np:path` or `no:*.min.js`).
    PathExclude(Vec<String>),
    /// Path or directory inclusion (e.g. `p:src/`, `path:crates/`).
    PathInclude(Vec<String>),
    /// Entry basename inclusion pattern (`in:...`).
    BasenameInclude(Vec<String>),
    /// Entry basename exclusion pattern (`ni:...`, `not-in:...`).
    BasenameExclude(Vec<String>),
    /// Entry kind selector (`kind:file`, `kind:dir`, `kind:link`).
    Kind(EntryKind),
    /// Directory inclusion selector (from `dir:<pattern>`).
    DirInclude(Vec<String>),
    /// File inclusion selector (from `file:<pattern>`).
    FileInclude(Vec<String>),
    /// Symlink inclusion selector (from `link:<pattern>` or `symlink:<pattern>`).
    LinkInclude(Vec<String>),
    /// File size predicate (`larger:...`, `smaller:...`).
    SizePredicate(SizePredicate),
    /// File modification time predicate (`newer:...`, `older:...`).
    TimePredicate(TimePredicate),
    /// File type exclusion (e.g. `nt:c,h`, `nt:rust`).
    TypeExclude(Vec<String>),
    /// File type inclusion (e.g. `:rs`, `only:rust`, `:c,h`).
    TypeInclude(Vec<String>),
    /// Target directory or file path.
    Target(PathBuf),
    /// Search hidden files toggle (`yes:dots`, `no:dots`).
    SearchHidden(bool),
    /// Respect .gitignore and .ignore rules (`yes:ignore`, `no:ignore`).
    RespectIgnore(bool),
    /// Follow symlinks toggle (`--follow`, `--no-follow`).
    FollowSymlinks(bool),
    /// Search cache directory toggle (`yes:cache`).
    SearchCache(bool),
    /// Binary inclusion toggle (`+bin`, `:bin`, `yes:bin`).
    IncludeBinaries,
    /// Binary exclusion toggle (`-bin`, `no:bin`).
    ExcludeBinaries,
    /// Binary-only mode toggle (`only:bin`).
    OnlyBinaries,
    /// Case-sensitivity override (`yes:case`, `no:case`).
    CaseSensitive(bool),
    /// Traversal recursion depth limit (`d:N`, `depth:N`).
    Depth(usize),
    /// Max match count per file limit (`m:N`).
    MaxCount(usize),
    /// Global head result limit (`head:N`, `top:N`, `limit:N`).
    Head(usize),
    /// Tail match count limit (`tail:N`).
    TailCount(usize),
    /// Unified context lines (`ctx:N`, `context:N`).
    Context(usize),
    /// Minimum string length for binary extraction (`str:N`, `strings:N`).
    BinaryStrings(usize),
    /// Atomic pattern term.
    Pattern(SearchPattern),
    /// Compact proximity filter (`near:N,pat`, `no-near:N,pat`, `-near:N,pat`).
    Proximity(ProximityFilter),
    /// Fuzzy token query (`fz:...`, `%%...`, `fuzzy:...`).
    Fuzzy(FuzzyQuery),
    /// Infix proximity operator (`NEAR:N` or `NEAR`).
    NearInfix(usize),
    /// Filesystem action (`mv:<dest>`, `cp:<dest>`, `rm:`, `trash:`).
    Action(crate::ops::ActionKind),
    /// Dry-run simulation mode (`dry:`, `--dry-run`).
    DryRun,
}

/// Parser for the grx search DSL.
pub struct DslParser;

impl DslParser {
    /// Parse a collection of CLI positional arguments into a complete Query.
    pub fn parse<I, S>(args: I) -> Result<Query, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::parse_with_external_expr(args, None)
    }

    /// Parse CLI positional arguments with an optional externally provided search expression (e.g. from -e or -f).
    pub fn parse_with_external_expr<I, S>(
        args: I,
        external_expr: Option<QueryExpr>,
    ) -> Result<Query, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::parse_with_compiler(args, external_expr, Self::compile_term_to_pattern)
    }

    /// Compile content terms with a caller-selected syntax before constructing the AST.
    pub fn parse_with_compiler<I, S, F>(
        args: I,
        external_expr: Option<QueryExpr>,
        compile: F,
    ) -> Result<Query, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
        F: Fn(&str, bool) -> SearchPattern,
    {
        Self::parse_with_compiler_and_base(args, external_expr, Query::default(), compile)
    }

    /// Compile content terms with a caller-selected syntax and an initial base query.
    pub fn parse_with_compiler_and_base<I, S, F>(
        args: I,
        external_expr: Option<QueryExpr>,
        query: Query,
        compile: F,
    ) -> Result<Query, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
        F: Fn(&str, bool) -> SearchPattern,
    {
        Self::parse_with_compiler_base_and_config(args, external_expr, query, None, compile)
    }

    /// Compile content terms with a caller-selected syntax, an initial base query, and optional configuration.
    pub fn parse_with_compiler_base_and_config<I, S, F>(
        args: I,
        external_expr: Option<QueryExpr>,
        mut query: Query,
        config: Option<&crate::config::Config>,
        compile: F,
    ) -> Result<Query, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
        F: Fn(&str, bool) -> SearchPattern,
    {
        let mut pattern_tokens: Vec<Token> = Vec::new();
        let mut pattern_sources: Vec<String> = Vec::new();
        let has_external = external_expr.is_some();
        let mut kind_before_pattern = false;
        let mut explicit_content_pattern = false;
        let mut raw_tokens = Vec::new();

        let mut after_double_dash = false;
        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            let s = arg.as_ref();
            if !after_double_dash && s == "--" {
                after_double_dash = true;
                continue;
            }
            if !after_double_dash && s == "--exec" {
                let cmd: Vec<String> = iter.map(|a| a.as_ref().to_string()).collect();
                if cmd.is_empty() {
                    return Err("--exec requires at least one command argument".into());
                }
                query.exec = cmd;
                break;
            }
            if !after_double_dash && (s == "-X" || s == "--exec-batch") {
                let cmd: Vec<String> = iter.map(|a| a.as_ref().to_string()).collect();
                if cmd.is_empty() {
                    return Err("-X/--exec-batch requires at least one command argument".into());
                }
                query.exec_batch = cmd;
                break;
            }
            let tok = if after_double_dash {
                Token::Pattern(compile(s, false))
            } else {
                Self::classify_with_compiler_and_config(s, config, &compile)?
            };
            raw_tokens.push((tok, s.to_string()));
        }

        let mut i = 0;
        while i < raw_tokens.len() {
            let (ref tok, ref s) = raw_tokens[i];

            // Infix NEAR operator: e.g. "NEAR" or "NEAR:5"
            if let Token::NearInfix(window) = tok {
                if pattern_tokens.is_empty() && !has_external && query.fuzzy.is_none() {
                    return Err("'NEAR/near:' proximity filter requires a base search pattern to match in the vicinity of (e.g. grx \"fn\" near:3,main)".into());
                }
                if i + 1 < raw_tokens.len() {
                    if !matches!(raw_tokens[i + 1].0, Token::Pattern(_) | Token::Target(_)) {
                        return Err("NEAR requires a search term as its right operand".into());
                    }
                    let next_term = Self::token_to_term(&raw_tokens[i + 1].0, &raw_tokens[i + 1].1);
                    // Check if preceded by NOT
                    let inverted = if !pattern_tokens.is_empty()
                        && pattern_tokens.last() == Some(&Token::Not)
                    {
                        pattern_tokens.pop();
                        pattern_sources.pop();
                        true
                    } else {
                        false
                    };
                    query.proximity_filters.push(ProximityFilter {
                        term: next_term,
                        window: *window,
                        inverted,
                    });
                    i += 2;
                    continue;
                } else {
                    return Err("Expected search pattern following 'NEAR' operator".to_string());
                }
            }

            match tok {
                Token::Action(act) => {
                    if let Some(existing) = &query.action {
                        return Err(format!(
                            "Multiple file actions specified: cannot combine '{existing:?}' with '{act:?}'"
                        ));
                    }
                    query.action = Some(act.clone());
                }
                Token::DryRun => query.dry_run = true,
                Token::Sort(s) => query.sort = Some(*s),
                Token::Proximity(prox) => query.proximity_filters.push(prox.clone()),
                Token::Fuzzy(fz) => {
                    if query.fuzzy.replace(fz.clone()).is_some() {
                        return Err("Use one fuzzy query with comma-separated tokens".into());
                    }
                }
                Token::PathExclude(pats) => query.path_excludes.extend(pats.clone()),
                Token::PathInclude(pats) => query.path_includes.extend(pats.clone()),
                Token::BasenameInclude(pats) => {
                    for pat in pats {
                        if !pat.is_empty() {
                            query.basename_includes.push(pat.clone());
                            query.basename_filters.push(BasenameFilter::parse(pat));
                        }
                    }
                }
                Token::BasenameExclude(pats) => {
                    for pat in pats {
                        if !pat.is_empty() {
                            query.basename_excludes.push(pat.clone());
                            query
                                .basename_exclude_filters
                                .push(BasenameFilter::parse(pat));
                        }
                    }
                }
                Token::Kind(k) => {
                    if pattern_tokens.is_empty() && !has_external && query.fuzzy.is_none() {
                        kind_before_pattern = true;
                    }
                    if let Some(existing) = query.kind
                        && existing != *k
                    {
                        return Err(format!(
                            "Conflicting kind filters: cannot filter for both {existing:?} and {k:?}"
                        ));
                    }
                    query.kind = Some(*k);
                    if *k == EntryKind::Bin {
                        query.include_binaries = true;
                        query.only_binaries = true;
                    } else if *k == EntryKind::Text {
                        query.include_binaries = false;
                    }
                }
                Token::DirInclude(pats) => {
                    if let Some(existing) = query.kind
                        && existing != EntryKind::Dir
                    {
                        return Err(format!(
                            "Conflicting kind filters: cannot filter for both {existing:?} and Dir"
                        ));
                    }
                    query.kind = Some(EntryKind::Dir);
                    for pat in pats {
                        if !pat.is_empty() {
                            query.basename_includes.push(pat.clone());
                            query.basename_filters.push(BasenameFilter::parse(pat));
                        }
                    }
                }
                Token::FileInclude(pats) => {
                    if let Some(existing) = query.kind
                        && existing != EntryKind::File
                    {
                        return Err(format!(
                            "Conflicting kind filters: cannot filter for both {existing:?} and File"
                        ));
                    }
                    query.kind = Some(EntryKind::File);
                    for pat in pats {
                        if !pat.is_empty() {
                            query.basename_includes.push(pat.clone());
                            query.basename_filters.push(BasenameFilter::parse(pat));
                        }
                    }
                }
                Token::LinkInclude(pats) => {
                    if let Some(existing) = query.kind
                        && existing != EntryKind::Link
                    {
                        return Err(format!(
                            "Conflicting kind filters: cannot filter for both {existing:?} and Link"
                        ));
                    }
                    query.kind = Some(EntryKind::Link);
                    for pat in pats {
                        if !pat.is_empty() {
                            query.basename_includes.push(pat.clone());
                            query.basename_filters.push(BasenameFilter::parse(pat));
                        }
                    }
                }
                Token::TailCount(t) => query.tail = Some(*t),
                Token::SizePredicate(sp) => query.size_predicates.push(*sp),
                Token::TimePredicate(tp) => query.time_predicates.push(*tp),
                Token::TypeExclude(exts) => query.type_excludes.extend(exts.clone()),
                Token::TypeInclude(exts) => query.type_includes.extend(exts.clone()),
                Token::Target(_p)
                    if s != "-"
                        && pattern_tokens.is_empty()
                        && !has_external
                        && query.fuzzy.is_none()
                        && !query.has_entry_selectors() =>
                {
                    // First positional argument is the primary search pattern unless discovery
                    // selectors are active (e.g. `grx in:hosts /etc`).
                    pattern_tokens.push(Token::Pattern(compile(s, false)));
                    pattern_sources.push(s.clone());
                }
                Token::Target(_)
                    if matches!(
                        pattern_tokens.last(),
                        Some(Token::And | Token::Or | Token::Not)
                    ) =>
                {
                    pattern_tokens.push(Token::Pattern(compile(s, false)));
                    pattern_sources.push(s.clone());
                }
                Token::Target(p) => query.targets.push(crate::config::Config::expand_tilde(p)),
                Token::SearchHidden(h) => query.search_hidden = Some(*h),
                Token::RespectIgnore(r) => query.respect_ignore = Some(*r),
                Token::FollowSymlinks(f) => query.follow_symlinks = Some(*f),
                Token::SearchCache(c) => query.search_cache = *c,
                Token::IncludeBinaries => query.include_binaries = true,
                Token::ExcludeBinaries => query.include_binaries = false,
                Token::OnlyBinaries => {
                    query.include_binaries = true;
                    query.only_binaries = true;
                    query.kind = Some(EntryKind::Bin);
                }
                Token::CaseSensitive(cs) => query.case_sensitive = Some(*cs),
                Token::Depth(d) => query.max_depth = Some(*d),
                Token::MaxCount(m) => query.max_count = Some(*m),
                Token::Head(h) => query.head = Some(*h),
                Token::Context(c) => query.context = Some(*c),
                Token::BinaryStrings(min_len) => {
                    query.binary_strings_min_len = Some(*min_len);
                    query.include_binaries = true;
                    query.has_content_pattern = true;
                }
                Token::Pattern(SearchPattern::Literal { text, .. })
                    if has_external || query.fuzzy.is_some() =>
                {
                    query
                        .targets
                        .push(crate::config::Config::expand_tilde(PathBuf::from(text)));
                }
                Token::Pattern(SearchPattern::ExactLiteral(text))
                    if has_external || query.fuzzy.is_some() =>
                {
                    query
                        .targets
                        .push(crate::config::Config::expand_tilde(PathBuf::from(text)));
                }
                Token::Pattern(SearchPattern::Literal { text, .. })
                    if !matches!(
                        pattern_tokens.last(),
                        Some(Token::And | Token::Or | Token::Not)
                    ) && !pattern_tokens.is_empty() =>
                {
                    query
                        .targets
                        .push(crate::config::Config::expand_tilde(PathBuf::from(text)));
                }
                Token::Pattern(SearchPattern::ExactLiteral(text))
                    if !matches!(
                        pattern_tokens.last(),
                        Some(Token::And | Token::Or | Token::Not)
                    ) && !pattern_tokens.is_empty() =>
                {
                    query
                        .targets
                        .push(crate::config::Config::expand_tilde(PathBuf::from(text)));
                }
                Token::Pattern(SearchPattern::Regex(_))
                    if !matches!(
                        pattern_tokens.last(),
                        Some(Token::And | Token::Or | Token::Not)
                    ) && (!pattern_tokens.is_empty()
                        || has_external
                        || query.fuzzy.is_some())
                        && !s.starts_with("re:")
                        && !s.starts_with("w:")
                        && !s.starts_with('@')
                        && !(s.starts_with('/') && s.ends_with('/')) =>
                {
                    query
                        .targets
                        .push(crate::config::Config::expand_tilde(PathBuf::from(s)));
                }
                tok => {
                    if matches!(tok, Token::Pattern(_))
                        && (s.starts_with("re:")
                            || s.starts_with("hex:")
                            || s.starts_with('@')
                            || (s.starts_with('/') && s.ends_with('/') && s.len() >= 2))
                    {
                        explicit_content_pattern = true;
                    }
                    pattern_tokens.push(tok.clone());
                    pattern_sources.push(s.clone());
                }
            }
            i += 1;
        }

        if let Some(expr) = external_expr {
            query.expr = Some(expr);
            query.has_content_pattern = true;
        } else if !pattern_tokens.is_empty() {
            // A kind named before the first search term asks for entries by name.
            // A search term before kind: (or an explicit content pattern) keeps grep semantics.
            if kind_before_pattern && !explicit_content_pattern && query.fuzzy.is_none() {
                if !query.proximity_filters.is_empty()
                    || pattern_tokens
                        .iter()
                        .any(|tok| !matches!(tok, Token::Pattern(_)))
                {
                    return Err("Boolean, negative, and proximity terms search file contents. Put the content pattern before kind:, or use in: filters for entry names.".into());
                }
                for (tok, raw) in pattern_tokens.iter().zip(&pattern_sources) {
                    let name = match tok {
                        Token::Pattern(SearchPattern::ExactLiteral(text))
                            if !raw.starts_with('=') =>
                        {
                            text.as_str()
                        }
                        _ => raw.as_str(),
                    };
                    query.basename_includes.push(name.to_string());
                    query.basename_filters.push(BasenameFilter::parse(name));
                }
            } else {
                query.expr = Some(Self::parse_expression(&pattern_tokens)?);
                query.has_content_pattern = true;
            }
        }
        if let Some(ref fz) = query.fuzzy {
            let fuzzy_expr = Self::compile_fuzzy_to_expr(&fz.tokens);
            query.expr = Some(match query.expr.take() {
                Some(expr) => QueryExpr::And(Box::new(expr), Box::new(fuzzy_expr)),
                None => fuzzy_expr,
            });
            query.has_content_pattern = true;
        }

        // Check if any hex pattern was configured
        if let Some(ref expr) = query.expr
            && Self::contains_hex(expr)
        {
            query.hex_mode = true;
            query.include_binaries = true;
        }

        if (query.head.is_some() || query.max_count.is_some()) && query.tail.is_some() {
            return Err(
                "Cannot specify both 'head' (or max count) and 'tail' limits simultaneously."
                    .into(),
            );
        }

        if !query.proximity_filters.is_empty() && query.expr.is_none() && query.fuzzy.is_none() {
            return Err("'NEAR/near:' proximity filter requires a base search pattern to match in the vicinity of (e.g. grx \"fn\" near:3,main)".into());
        }

        Ok(query)
    }

    /// Classify a single command-line argument token.
    pub fn classify_token(s: &str) -> Result<Token, String> {
        Self::classify_with_compiler(s, &Self::compile_term_to_pattern)
    }

    /// Classify a single command-line argument token with optional configuration context.
    pub fn classify_token_with_config(
        s: &str,
        config: Option<&crate::config::Config>,
    ) -> Result<Token, String> {
        Self::classify_with_compiler_and_config(s, config, &Self::compile_term_to_pattern)
    }

    fn classify_with_compiler(
        s: &str,
        compile: &impl Fn(&str, bool) -> SearchPattern,
    ) -> Result<Token, String> {
        Self::classify_with_compiler_and_config(s, None, compile)
    }

    fn classify_with_compiler_and_config(
        s: &str,
        _config: Option<&crate::config::Config>,
        compile: &impl Fn(&str, bool) -> SearchPattern,
    ) -> Result<Token, String> {
        // Boolean keywords
        if s.eq_ignore_ascii_case("and") {
            return Ok(Token::And);
        }
        if s.eq_ignore_ascii_case("or") {
            return Ok(Token::Or);
        }
        if s.eq_ignore_ascii_case("not") {
            return Ok(Token::Not);
        }

        // Infix proximity operator: "NEAR:N" or "NEAR"
        if s.eq_ignore_ascii_case("near") {
            return Ok(Token::NearInfix(3));
        }
        if let Some(rest) = s.to_ascii_uppercase().strip_prefix("NEAR:")
            && let Ok(val) = rest.parse::<usize>()
        {
            return Ok(Token::NearInfix(val));
        }

        // Fuzzy token query prefix: "fz:..." (canonical)
        if s == "fz:" {
            return Err(format!(
                "fuzzy search pattern received an empty argument ('{s}'). \
                 Wrap the argument in outer quotes: grx fz:'a,b'"
            ));
        }
        if let Some(rest) = s.strip_prefix("fz:") {
            let tokens = Self::split_fuzzy_tokens(rest, s)?;
            return Ok(Token::Fuzzy(FuzzyQuery {
                tokens,
                original: rest.to_string(),
            }));
        }
        if s == "fuzzy:" || s == "%%" || s.starts_with("fuzzy:") || s.starts_with("%%") {
            let rest = s
                .strip_prefix("fuzzy:")
                .or_else(|| s.strip_prefix("%%"))
                .unwrap_or("");
            return Err(format!(
                "Fuzzy prefix in '{s}' is deprecated. Use canonical 'fz:{rest}'."
            ));
        }

        // Inverted proximity filter: "no-near:..." (canonical)
        if let Some(rest) = s.strip_prefix("no-near:") {
            let (window, term) = Self::parse_proximity_args(rest)?;
            return Ok(Token::Proximity(ProximityFilter {
                term,
                window,
                inverted: true,
            }));
        }
        if let Some(rest) = s.strip_prefix("-near:") {
            return Err(format!(
                "Prefix '-near:{rest}' is deprecated. Use canonical 'no-near:{rest}'."
            ));
        }

        // Proximity filter: "near:..."
        if let Some(rest) = s.strip_prefix("near:") {
            let (window, term) = Self::parse_proximity_args(rest)?;
            return Ok(Token::Proximity(ProximityFilter {
                term,
                window,
                inverted: false,
            }));
        }

        // Explicit feature affirmation: "yes:..."
        if let Some(rest) = s.strip_prefix("yes:") {
            match rest.to_ascii_lowercase().as_str() {
                "dots" => return Ok(Token::SearchHidden(true)),
                "ignore" => return Ok(Token::RespectIgnore(true)),
                "case" => return Ok(Token::CaseSensitive(true)),
                "cache" => return Ok(Token::SearchCache(true)),
                "dot" | "dotfiles" | "hidden" => {
                    return Err(format!(
                        "Option 'yes:{rest}' is deprecated. Use canonical 'yes:dots' to search hidden files."
                    ));
                }
                "bin" | "binary" | "binaries" => {
                    return Err(format!(
                        "'yes:{rest}' is deprecated. Use canonical 'kind:bin' to select binary files, or 'str:4' to extract printable strings."
                    ));
                }
                other => {
                    return Err(format!(
                        "Unrecognized option 'yes:{other}'. Supported options: yes:dots, yes:ignore, yes:cache, yes:case"
                    ));
                }
            }
        }

        // Feature negation: "no:..."
        if let Some(rest) = s.strip_prefix("no:") {
            match rest.to_ascii_lowercase().as_str() {
                "dots" => return Ok(Token::SearchHidden(false)),
                "ignore" => return Ok(Token::RespectIgnore(false)),
                "case" => return Ok(Token::CaseSensitive(false)),
                "cache" => return Ok(Token::SearchCache(false)),
                "dot" | "dotfiles" | "hidden" => {
                    return Err(format!(
                        "Option 'no:{rest}' is deprecated. Use canonical 'no:dots' to exclude hidden files."
                    ));
                }
                "bin" | "binary" | "binaries" => {
                    return Err(format!(
                        "'no:{rest}' is deprecated. Use canonical 'kind:text' to select only plain text files."
                    ));
                }
                _ => {
                    if rest.ends_with('/') || rest.ends_with('\\') {
                        return Err(format!(
                            "Prefix 'no:{rest}' is deprecated. Use canonical 'np:{rest}' to exclude directory path."
                        ));
                    }
                    return Err(format!(
                        "Unrecognized negation 'no:{rest}'. Use canonical 'nt:{rest}' to exclude file types, 'np:{rest}' to exclude paths, 'ni:{rest}' to exclude basenames, or 'ns:{rest}' to exclude content lines."
                    ));
                }
            }
        }

        // Shorthand negation with "!": "!<dir>/" or "!*.<ext>"
        if let Some(rest) = s.strip_prefix('!') {
            if let Some(ext) = rest.strip_prefix("*.") {
                return Err(format!(
                    "Prefix '{s}' is deprecated and shell-sensitive. Use canonical 'nt:{ext}' to exclude file type."
                ));
            }
            return Err(format!(
                "Prefix '{s}' is deprecated and shell-sensitive. Use canonical 'np:{rest}' to exclude paths, or 'nt:{rest}' to exclude file types."
            ));
        }

        // Exclusive filetype filters: "only:..."
        if let Some(rest) = s.strip_prefix("only:") {
            if rest.eq_ignore_ascii_case("bin")
                || rest.eq_ignore_ascii_case("binary")
                || rest.eq_ignore_ascii_case("binaries")
            {
                return Err(
                    "Prefix 'only:bin' has been consolidated. Use canonical 'kind:bin' to select binary files."
                        .to_string(),
                );
            }
            if let Some(strings_len) = rest.strip_prefix("str") {
                let n = strings_len.strip_prefix(':').unwrap_or("4");
                return Err(format!(
                    "Prefix 'only:str' has been consolidated. Use canonical 'str:{n}' to extract printable strings from binaries."
                ));
            }
            if rest.eq_ignore_ascii_case("dir")
                || rest.eq_ignore_ascii_case("directory")
                || rest.eq_ignore_ascii_case("dirs")
            {
                return Err(
                    "Prefix 'only:dir' has been consolidated. Use canonical 'kind:dir' to select directories."
                        .to_string(),
                );
            }
            return Err(format!(
                "Prefix 'only:{rest}' has been consolidated. Use canonical 't:{rest}' to filter by file type."
            ));
        }

        // Traversal recursion depth limit: d:N (canonical)
        if let Some(rest) = s.strip_prefix("d:") {
            if rest.is_empty()
                || rest.starts_with('\\')
                || rest.starts_with('/')
                || rest.starts_with(':')
                || !rest.chars().all(|c| c.is_ascii_digit())
            {
                // Not a numeric depth limit (e.g. Windows drive path like d:\foo or d::item), fall through
            } else {
                return rest
                    .parse::<usize>()
                    .map(Token::Depth)
                    .map_err(|_| format!("Expected a non-negative integer in '{s}'"));
            }
        }
        if let Some(rest) = s
            .strip_prefix("depth:")
            .or_else(|| s.strip_prefix("maxdepth:"))
        {
            let prefix = if s.starts_with("depth:") {
                "depth:"
            } else {
                "maxdepth:"
            };
            return Err(format!(
                "Prefix '{prefix}{rest}' is deprecated. Use canonical 'd:{rest}' for traversal depth."
            ));
        }

        // Per-file match count limit: max:N (canonical)
        if let Some(rest) = s.strip_prefix("max:") {
            return rest
                .parse::<usize>()
                .map(Token::MaxCount)
                .map_err(|_| format!("Expected a non-negative integer in '{s}'"));
        }
        if let Some(rest) = s.strip_prefix("top:").or_else(|| s.strip_prefix("limit:")) {
            let prefix = if s.starts_with("top:") {
                "top:"
            } else {
                "limit:"
            };
            return Err(format!(
                "'{prefix}{rest}' is ambiguous. Use 'head:{rest}' to limit total results globally, or 'max:{rest}' to limit matches per file."
            ));
        }
        if let Some(rest) = s.strip_prefix("m:")
            && !rest.is_empty()
            && rest.chars().all(|c| c.is_ascii_digit())
        {
            return Err(format!(
                "Prefix 'm:{rest}' has been consolidated. Use canonical 'max:{rest}' to limit matches per file."
            ));
        }

        // Global head limit: head:N (canonical)
        if let Some(rest) = s.strip_prefix("head:") {
            if rest.starts_with(':') {
                // E.g. head::new, fall through
            } else {
                return rest
                    .parse::<usize>()
                    .map(Token::Head)
                    .map_err(|_| format!("Expected a non-negative integer in '{s}'"));
            }
        }

        // Tail match count limit: tail:N (canonical)
        if let Some(rest) = s.strip_prefix("tail:") {
            if rest.starts_with(':') {
                // E.g. tail::new, fall through
            } else {
                return rest
                    .parse::<usize>()
                    .map(Token::TailCount)
                    .map_err(|_| format!("Expected a non-negative integer in '{s}'"));
            }
        }

        // Unified context lines: ctx:N (canonical)
        if let Some(rest) = s.strip_prefix("ctx:") {
            if rest.starts_with(':') {
                // E.g. ctx::current, fall through
            } else {
                return rest
                    .parse::<usize>()
                    .map(Token::Context)
                    .map_err(|_| format!("Expected a non-negative integer in '{s}'"));
            }
        }
        if let Some(rest) = s.strip_prefix("context:") {
            return Err(format!(
                "Prefix 'context:{rest}' is deprecated. Use canonical 'ctx:{rest}' for context lines."
            ));
        }

        // Binary string extraction threshold: str:N (canonical)
        if let Some(rest) = s.strip_prefix("str:") {
            if rest.is_empty() || rest.starts_with(':') || !rest.chars().all(|c| c.is_ascii_digit())
            {
                // Not a numeric threshold (e.g. str::from_utf8 or "str::"), fall through
            } else {
                return rest
                    .parse::<usize>()
                    .map(Token::BinaryStrings)
                    .map_err(|_| format!("Expected a non-negative integer in '{s}'"));
            }
        }
        if let Some(rest) = s.strip_prefix("strings:") {
            return Err(format!(
                "Prefix 'strings:{rest}' is deprecated. Use canonical 'str:{rest}'."
            ));
        }

        // Result sort ordering: sort:key, sort:-key
        if let Some(rest) = s.strip_prefix("sort:") {
            if rest.is_empty() {
                return Err("Expected sort key after sort:".into());
            }
            let is_reversed = rest.starts_with('-');
            let key_str = rest.trim_start_matches('-');
            let key = parse_sort_key(key_str, is_reversed)?;
            return Ok(Token::Sort(key));
        }
        if let Some(rest) = s.strip_prefix("sortr:") {
            return Err(format!(
                "Prefix 'sortr:{rest}' is deprecated. Use canonical 'sort:-{rest}' to reverse sort."
            ));
        }

        // Directory inclusion selector (consolidated to kind:dir in:...)
        if let Some(rest) = s
            .strip_prefix("dir:")
            .or_else(|| s.strip_prefix("directory:"))
        {
            let prefix = if s.starts_with("dir:") {
                "dir:"
            } else {
                "directory:"
            };
            return Err(format!(
                "Prefix '{prefix}{rest}' has been consolidated. Use 'kind:dir in:{rest}' to discover directories named '{rest}', or 'p:{rest}' to target directory paths."
            ));
        }

        // File inclusion selector (consolidated to kind:file in:...)
        if let Some(rest) = s.strip_prefix("file:") {
            return Err(format!(
                "Prefix 'file:{rest}' has been consolidated. Use 'kind:file in:{rest}' to discover files named '{rest}'."
            ));
        }

        // Symlink inclusion selector (consolidated to kind:link in:...)
        if let Some(rest) = s
            .strip_prefix("link:")
            .or_else(|| s.strip_prefix("symlink:"))
        {
            let prefix = if s.starts_with("link:") {
                "link:"
            } else {
                "symlink:"
            };
            return Err(format!(
                "Prefix '{prefix}{rest}' has been consolidated. Use 'kind:link in:{rest}' to discover symlinks named '{rest}'."
            ));
        }

        // Explicit basename inclusion: "in:..." (canonical)
        if let Some(rest) = s.strip_prefix("in:") {
            if rest.is_empty() {
                return Err("Expected pattern after in:".into());
            }
            let pats: Vec<String> = rest
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect();
            if pats.is_empty() {
                return Err("Expected pattern after in:".into());
            }
            for pat in &pats {
                if pat.contains('/') || pat.contains('\\') {
                    eprintln!(
                        "grx warning: 'in:{pat}' contains a path separator; 'in:' matches file basenames only (e.g. 'in:report.md'). Did you mean 'p:{pat}' for directories?"
                    );
                }
            }
            return Ok(Token::BasenameInclude(pats));
        }

        // Explicit basename exclusion: "ni:..." (canonical)
        if let Some(rest) = s.strip_prefix("ni:") {
            if rest.is_empty() {
                return Err(format!("Expected pattern after '{s}'"));
            }
            let pats: Vec<String> = rest
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect();
            if pats.is_empty() {
                return Err(format!("Expected pattern after '{s}'"));
            }
            for pat in &pats {
                if pat.contains('/') || pat.contains('\\') {
                    eprintln!(
                        "grx warning: 'ni:{pat}' contains a path separator; 'ni:{pat}' excludes file basenames only (e.g. 'ni:test.rs'). Did you mean 'np:{pat}' for directories?"
                    );
                }
            }
            return Ok(Token::BasenameExclude(pats));
        }
        if let Some(rest) = s.strip_prefix("not-in:") {
            return Err(format!(
                "Prefix 'not-in:{rest}' is deprecated. Use canonical 'ni:{rest}' to exclude basenames."
            ));
        }

        // Explicit file type inclusion: "t:..." (canonical)
        if let Some(rest) = s.strip_prefix("t:") {
            let exts: Vec<String> = rest
                .split(',')
                .map(|e| e.trim().trim_start_matches('.').to_string())
                .filter(|e| !e.is_empty())
                .collect();
            if exts.is_empty() {
                return Err(format!("Expected file type after '{s}'"));
            }
            return Ok(Token::TypeInclude(exts));
        }
        if let Some(rest) = s.strip_prefix("type:") {
            return Err(format!(
                "Prefix 'type:{rest}' is deprecated. Use canonical 't:{rest}' to filter by file type."
            ));
        }

        // Filesystem entry kind constraint: "kind:file", "kind:dir", "kind:link", "kind:bin", "kind:text"
        if s.starts_with("kind:") {
            let kind = parse_kind(s)?;
            return Ok(Token::Kind(kind));
        }

        // Shorthand kind selector: "bin:" or "binary:"
        if s.starts_with("bin:") || s.starts_with("binary:") {
            return Err(format!(
                "Prefix '{s}' has been consolidated. Use canonical 'kind:bin' to select binary files, or 'str:4' to search text in binaries."
            ));
        }

        // File size predicates: "larger:...", "smaller:..."
        if s.starts_with("larger:") || s.starts_with("smaller:") {
            let pred = parse_size_predicate(s)?;
            return Ok(Token::SizePredicate(pred));
        }

        // File modification time predicates: "newer:...", "older:..."
        if s.starts_with("newer:") || s.starts_with("older:") {
            let pred = parse_time_predicate(s)?;
            return Ok(Token::TimePredicate(pred));
        }

        // Filesystem action operators: mv:<dest>, cp:<dest>, trash:
        if let Some(dest) = s.strip_prefix("mv:") {
            if dest.is_empty() {
                return Err(format!("Expected destination directory after '{s}'"));
            }
            return Ok(Token::Action(crate::ops::ActionKind::Move(
                crate::config::Config::expand_tilde(PathBuf::from(dest)),
            )));
        }
        if let Some(dest) = s.strip_prefix("cp:") {
            if dest.is_empty() {
                return Err(format!("Expected destination directory after '{s}'"));
            }
            return Ok(Token::Action(crate::ops::ActionKind::Copy(
                crate::config::Config::expand_tilde(PathBuf::from(dest)),
            )));
        }
        if let Some(rest) = s.strip_prefix("trash:") {
            if rest.is_empty() {
                return Ok(Token::Action(crate::ops::ActionKind::Trash));
            } else {
                return Err(format!(
                    "'{s}' takes no value. To stage entries matching '{rest}' into trash, use: grx in:{rest} trash:"
                ));
            }
        }
        if s.starts_with("rm:") {
            return Err(
                "Prefix 'rm:' has been consolidated. Use canonical 'trash:' to safely stage matching items into trash WAL.".into()
            );
        }
        if let Some(rest) = s.strip_prefix("rename:") {
            let (pat, rep) = if let Some((p, r)) = rest.split_once('/') {
                (p, r)
            } else if let Some((p, r)) = rest.split_once("->") {
                (p, r)
            } else {
                return Err(format!(
                    "Expected 'old/new' or 'old->new' pattern after 'rename:'. Got '{rest}'"
                ));
            };
            return Ok(Token::Action(crate::ops::ActionKind::Rename {
                pattern: pat.to_string(),
                replacement: rep.to_string(),
            }));
        }
        if let Some(mode) = s.strip_prefix("chmod:") {
            if mode.is_empty() {
                return Err(format!("Expected permissions mode after '{s}'"));
            }
            return Ok(Token::Action(crate::ops::ActionKind::Chmod(
                mode.to_string(),
            )));
        }
        if s == "dry:" || s == "--dry-run" || s == "--dry" {
            return Ok(Token::DryRun);
        }

        // Explicit path exclusions: "np:..." (canonical)
        if let Some(rest) = s.strip_prefix("np:") {
            if rest.is_empty() {
                return Err(format!("Expected path after '{s}'"));
            }
            let paths: Vec<String> = rest
                .split(',')
                .map(|p| crate::config::Config::expand_tilde_str(p.trim()))
                .filter(|p| !p.is_empty())
                .collect();
            if paths.is_empty() {
                return Err(format!("Expected path after '{s}'"));
            }
            return Ok(Token::PathExclude(paths));
        }
        if let Some(rest) = s.strip_prefix("no-path:") {
            return Err(format!(
                "Prefix 'no-path:{rest}' is deprecated. Use canonical 'np:{rest}' to exclude path."
            ));
        }

        // Explicit path inclusions: "p:..." (canonical)
        if let Some(rest) = s.strip_prefix("p:") {
            if rest.is_empty() {
                return Err(format!("Expected path after '{s}'"));
            }
            let paths: Vec<String> = rest
                .split(',')
                .map(|p| crate::config::Config::expand_tilde_str(p.trim()))
                .filter(|p| !p.is_empty())
                .collect();
            if paths.is_empty() {
                return Err(format!("Expected path after '{s}'"));
            }
            return Ok(Token::PathInclude(paths));
        }
        if let Some(rest) = s.strip_prefix("path:") {
            return Err(format!(
                "Prefix 'path:{rest}' is deprecated. Use canonical 'p:{rest}' to specify path."
            ));
        }

        // Explicit file type exclusions: "nt:..." (canonical)
        if let Some(rest) = s.strip_prefix("nt:") {
            let exts: Vec<String> = rest
                .split(',')
                .map(|e| e.trim().trim_start_matches('.').to_string())
                .filter(|e| !e.is_empty())
                .collect();
            if exts.is_empty() {
                return Err(format!("Expected file type after '{s}'"));
            }
            return Ok(Token::TypeExclude(exts));
        }
        if let Some(rest) = s.strip_prefix("no-type:") {
            return Err(format!(
                "Prefix 'no-type:{rest}' is deprecated. Use canonical 'nt:{rest}' to exclude file type."
            ));
        }

        // Explicit string/line content negation: "ns:..." (canonical)
        if let Some(rest) = s.strip_prefix("ns:") {
            if rest.is_empty() {
                return Err(format!("Expected string after '{s}'"));
            }
            let (is_whole, term) = if let Some(at) = rest.strip_prefix('@') {
                (true, at)
            } else {
                (false, rest)
            };
            if term.is_empty() {
                return Err(format!("Expected string after '{s}'"));
            }
            return Ok(Token::NegativeTerm(compile(term, is_whole)));
        }
        if let Some(rest) = s
            .strip_prefix("no-str:")
            .or_else(|| s.strip_prefix("no-string:"))
        {
            let prefix = if s.starts_with("no-str:") {
                "no-str:"
            } else {
                "no-string:"
            };
            return Err(format!(
                "Prefix '{prefix}{rest}' is deprecated. Use canonical 'ns:{rest}' or boolean 'NOT {rest}' to exclude matching lines."
            ));
        }
        if let Some(rest) = s.strip_prefix("not:") {
            return Err(format!(
                "Prefix 'not:{rest}' is deprecated. Use canonical 'ns:{rest}' or boolean 'NOT {rest}' to exclude matching lines."
            ));
        }

        // Whole-word term match: "@term" (canonical)
        if let Some(rest) = s.strip_prefix('@') {
            if rest.is_empty() {
                return Err(format!("Expected term after '{s}'"));
            }
            return Ok(Token::Pattern(compile(rest, true)));
        }
        if let Some(rest) = s.strip_prefix("w:") {
            return Err(format!(
                "Prefix 'w:{rest}' is deprecated. Use canonical '@{rest}' for whole-word matching."
            ));
        }

        // Deprecate ":ext" shorthand
        if let Some(rest) = s.strip_prefix(':')
            && !rest.is_empty()
            && !rest.starts_with(':')
            && !rest.contains('/')
            && !rest.contains('\\')
            && !rest.contains(' ')
            && rest
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '+' || c == ',')
        {
            return Err(format!(
                "Prefix ':{rest}' is deprecated. Use canonical 't:{rest}' to filter by file type."
            ));
        }

        // Long CLI options starting with "--"
        if s.starts_with("--") {
            if s == "--follow" {
                return Ok(Token::FollowSymlinks(true));
            } else if s == "--no-follow" {
                return Ok(Token::FollowSymlinks(false));
            } else if let Some(rest) = s.strip_prefix("--sort=") {
                let key = parse_sort_key(rest, false)?;
                return Ok(Token::Sort(key));
            } else if let Some(rest) = s.strip_prefix("--head=") {
                let n: usize = rest
                    .parse()
                    .map_err(|_| format!("Invalid limit in '{s}': expected positive integer"))?;
                return Ok(Token::Head(n));
            } else if let Some(rest) = s.strip_prefix("--tail=") {
                let n: usize = rest
                    .parse()
                    .map_err(|_| format!("Invalid limit in '{s}': expected positive integer"))?;
                return Ok(Token::TailCount(n));
            } else {
                return Err(format!(
                    "Unrecognized option '{s}'. CLI flags must precede search patterns. See 'grx --help' for available flags."
                ));
            }
        }

        // Short CLI flags starting with "-" (single flags like -i, -F, clustered like -iv, or attached values like -m1, -C2)
        if Self::is_short_cli_flag(s) {
            let rest = &s[1..];
            return Err(format!(
                "CLI flag '{s}' was placed after positional search arguments. Place CLI flags before search patterns (e.g. 'grx {s} <pattern>') or use explicit DSL syntax for negation (e.g. 'ns:{rest}')."
            ));
        }

        // Deprecate leading "-" for negative terms (e.g. `-koseoglu`)
        if s.starts_with('-') && s.len() > 1 && !s.chars().skip(1).all(|c| c.is_ascii_digit()) {
            let rest = &s[1..];
            return Err(format!(
                "Leading '-' for negative terms is deprecated to prevent collisions with CLI flags. Use canonical 'ns:{rest}' or boolean 'NOT {rest}'."
            ));
        }

        // Deprecate leading "+" for required terms
        if s.starts_with('+') && s.len() > 1 {
            let rest = &s[1..];
            return Err(format!(
                "Leading '+' for required terms ('{s}') is deprecated. In grx, multiple terms are required by default, or use boolean 'AND {rest}'."
            ));
        }

        // Shell glob files like `*.rs` or `*.toml`
        if s.starts_with("*.") && s.len() > 2 && !s[2..].contains('/') && !s[2..].contains('\\') {
            let ext = &s[2..];
            return Err(format!(
                "Wildcard pattern '{s}' is shell-sensitive. Use canonical 't:{ext}' to filter by file type."
            ));
        }

        // Quoted exact string literals or globs: "foo", 'foo', 'foo*bar', 'foo?bar'
        if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
            || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
        {
            let inner = &s[1..s.len() - 1];
            if inner.contains('*') || inner.contains('?') {
                return Ok(Token::Pattern(Self::compile_glob_to_regex(inner)));
            }
            return Ok(Token::Pattern(SearchPattern::ExactLiteral(
                inner.to_string(),
            )));
        }

        // Explicit exact literal prefixed with '='
        if let Some(rest) = s.strip_prefix('=') {
            return Ok(Token::Pattern(SearchPattern::ExactLiteral(
                rest.to_string(),
            )));
        }

        // Explicit regex: `/regex/` or `re:regex`
        if s.starts_with('/') && s.ends_with('/') && s.len() >= 2 {
            let inner = &s[1..s.len() - 1];
            return Ok(Token::Pattern(SearchPattern::Regex(inner.to_string())));
        }
        if let Some(rest) = s.strip_prefix("re:") {
            return Ok(Token::Pattern(SearchPattern::Regex(rest.to_string())));
        }

        // Hex pattern: `hex:...`
        if let Some(rest) = s.strip_prefix("hex:") {
            return Self::parse_hex_pattern(rest)
                .filter(|bytes| !bytes.is_empty())
                .map(|bytes| Token::Pattern(SearchPattern::Hex(bytes)))
                .ok_or_else(|| format!("Invalid hex pattern '{s}': use byte pairs or ??"));
        }

        // Target path or standard input '-'
        if s == "-" {
            return Ok(Token::Target(PathBuf::from("-")));
        }

        // Helper to check if string is a Windows drive path (e.g. C:\..., C:/..., C:)
        let is_win_drive = s.len() >= 2
            && s.as_bytes()[0].is_ascii_alphabetic()
            && s.as_bytes()[1] == b':'
            && (s.len() == 2 || s.as_bytes()[2] == b'\\' || s.as_bytes()[2] == b'/');

        // Detect common typo'd or mistaken filter prefixes before falling back to target or pattern search
        if !is_win_drive
            && let Some((prefix, rest)) = s.split_once(':')
            && !prefix.is_empty()
            && !rest.is_empty()
            && !rest.starts_with('/')
            && !rest.starts_with('\\')
            && !rest.starts_with(':')
            && let Some(suggestion) = Self::suggest_typo_prefix(prefix, rest)
        {
            return Err(format!(
                "Unrecognized filter prefix '{prefix}:' in '{s}'. {suggestion} (Wrap in quotes like \"{s}\" or '={s}' to search for literal text)"
            ));
        }

        // Existing directory or file path on disk, path navigators, or path-like strings with '/' or '\'
        if s == "."
            || s == ".."
            || s.starts_with("./")
            || s.starts_with("../")
            || s.starts_with(".\\")
            || s.starts_with("..\\")
            || s.starts_with('/')
            || s.starts_with('\\')
            || s.ends_with('/')
            || s.ends_with('\\')
            || s.contains('/')
            || s.contains('\\')
            || is_win_drive
        {
            return Ok(Token::Target(PathBuf::from(s)));
        }

        // Default: atomic search term with support for anchors (^term, term$), shell-safe wildcards (..), and smart-case.
        // Bare arguments that do not contain '/' or '\' are classified as patterns by default.
        // In `parse_with_external_expr`, a bare argument is converted to a target only if a pattern
        // has already been collected, or an external expression is active, and it does not follow a boolean operator.
        Ok(Token::Pattern(compile(s, false)))
    }

    /// Calculate Levenshtein edit distance between two ASCII strings.
    fn levenshtein_distance(a: &str, b: &str) -> usize {
        let a_bytes = a.as_bytes();
        let b_bytes = b.as_bytes();
        let mut prev = (0..=b_bytes.len()).collect::<Vec<usize>>();
        let mut curr = vec![0; b_bytes.len() + 1];

        for (i, &ca) in a_bytes.iter().enumerate() {
            curr[0] = i + 1;
            for (j, &cb) in b_bytes.iter().enumerate() {
                let cost = if ca == cb { 0 } else { 1 };
                curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
            }
            prev.copy_from_slice(&curr);
        }
        prev[b_bytes.len()]
    }

    /// Suggest alternative DSL syntax for common typo'd or mistaken filter prefixes.
    fn suggest_typo_prefix(prefix: &str, val: &str) -> Option<String> {
        let p_lower = prefix.to_ascii_lowercase();
        match p_lower.as_str() {
            "ext" | "extension" | "extensions" => {
                return Some(format!("Did you mean 't:{val}' for file type filtering?"));
            }
            "pth" | "ptah" | "paht" => {
                return Some(format!(
                    "Did you mean 'p:{val}' for directory or file root?"
                ));
            }
            "name" | "filename" => {
                return Some(format!(
                    "Did you mean 'in:{val}' or 'in:={val}' for entry basename filtering?"
                ));
            }
            "typ" | "types" => {
                return Some(format!("Did you mean 't:{val}' for file type filtering?"));
            }
            "dirr" | "dirs" => {
                return Some(format!(
                    "Did you mean 'kind:dir in:{val}' or 'p:{val}' for directory filtering?"
                ));
            }
            "exclude" | "excludes" => {
                return Some(format!(
                    "Did you mean 'np:{val}' (no-path) or 'nt:{val}' (no-type) for exclusions?"
                ));
            }
            "no-dir" | "nodir" | "not-path" => {
                return Some(format!("Did you mean 'np:{val}' for path exclusion?"));
            }
            "not-type" => {
                return Some(format!("Did you mean 'nt:{val}' for type exclusion?"));
            }
            "not-str" | "not-string" => {
                return Some(format!("Did you mean 'ns:{val}' for line exclusion?"));
            }
            "mov" | "move" => {
                return Some(format!(
                    "Did you mean 'mv:{val}' to move matching items into a directory?"
                ));
            }
            "copy" => {
                return Some(format!(
                    "Did you mean 'cp:{val}' to copy matching items into a directory?"
                ));
            }
            "del" | "delete" | "remove" => {
                return Some(
                    "Did you mean 'trash:' to safely stage matching items into trash?".to_string(),
                );
            }
            "head-limit" => {
                return Some(format!(
                    "Did you mean 'head:{val}' for global result limits?"
                ));
            }
            "top-limit" => {
                return Some(format!(
                    "Did you mean 'max:{val}' for per-file match limits, or 'head:{val}' for global limits?"
                ));
            }
            "context-lines" => {
                return Some(format!("Did you mean 'ctx:{val}' for context lines?"));
            }
            "size" | "sz" | "bytes" => {
                return Some(format!(
                    "Did you mean 'larger:{val}' or 'smaller:{val}' for file size filtering?"
                ));
            }
            _ => {}
        }

        const CANONICAL_PREFIXES: &[(&str, &str)] = &[
            ("t", "file type filtering (e.g. 't:rs')"),
            ("nt", "file type exclusion (e.g. 'nt:c')"),
            ("in", "entry basename filtering (e.g. 'in:report')"),
            ("ni", "entry basename exclusion (e.g. 'ni:test')"),
            ("kind", "entry kind filtering (e.g. 'kind:dir', 'kind:bin')"),
            ("p", "path root filtering (e.g. 'p:src/')"),
            ("np", "path exclusion (e.g. 'np:target/')"),
            ("ns", "line string exclusion (e.g. 'ns:debug')"),
            ("head", "global head limit (e.g. 'head:10')"),
            ("tail", "global tail limit (e.g. 'tail:10')"),
            ("max", "per-file match limit (e.g. 'max:5')"),
            ("ctx", "context lines (e.g. 'ctx:3')"),
            ("d", "traversal depth limit (e.g. 'd:1')"),
            ("str", "binary strings extraction (e.g. 'str:4')"),
            ("fz", "fuzzy token query (e.g. 'fz:foo,bar')"),
            ("near", "proximity search (e.g. 'near:3,main')"),
            (
                "no-near",
                "inverted proximity search (e.g. 'no-near:3,main')",
            ),
            ("mv", "moving matching items into a directory"),
            ("cp", "copying matching items into a directory"),
            ("trash", "safely trashing matching items"),
            ("dry", "dry-run simulation"),
            ("sort", "result sorting (e.g. 'sort:size')"),
            ("larger", "file size filtering (e.g. 'larger:10M')"),
            ("smaller", "file size filtering (e.g. 'smaller:1K')"),
            ("newer", "file age filtering (e.g. 'newer:7d')"),
            ("older", "file age filtering (e.g. 'older:24h')"),
        ];

        let mut best: Option<(&str, &str, usize)> = None;
        for &(known, desc) in CANONICAL_PREFIXES {
            let dist = Self::levenshtein_distance(&p_lower, known);
            let max_allowed = if known.len() <= 2 { 1 } else { 2 };
            if dist <= max_allowed && best.as_ref().is_none_or(|b| dist < b.2) {
                best = Some((known, desc, dist));
            }
        }

        if let Some((known, desc, _)) = best {
            Some(format!("Did you mean '{known}:{val}' for {desc}?"))
        } else {
            None
        }
    }

    /// Parse proximity arguments in format `N,pattern` or `pattern` (defaults to window = 3).
    fn parse_proximity_args(rest: &str) -> Result<(usize, String), String> {
        let rest = rest.trim();
        if rest.is_empty() {
            return Err(
                "Proximity filter cannot be empty (expected near:pattern or near:N,pattern)"
                    .to_string(),
            );
        }
        if let Some((w_str, term_str)) = rest.split_once(',') {
            let w_trimmed = w_str.trim();
            let term_trimmed = term_str.trim();
            if let Ok(w) = w_trimmed.parse::<usize>() {
                if term_trimmed.is_empty() {
                    return Err(format!(
                        "Proximity filter '{rest}' has a window but no target term"
                    ));
                }
                return Ok((w, term_trimmed.to_string()));
            }
        }
        Ok((3, rest.to_string()))
    }

    /// Returns true if the string represents a short CLI flag, flag cluster, or attached flag value
    /// placed after positional arguments (e.g. `-i`, `-F`, `-iv`, `-m1`, `-m=1`, `-C2`, `-d0`).
    fn is_short_cli_flag(s: &str) -> bool {
        if !s.starts_with('-') || s.starts_with("--") || s.len() <= 1 {
            return false;
        }
        let rest = &s[1..];

        // Explicit negative terms like `-@term` or `-w:term` represent pattern negation.
        if rest.starts_with('@') || rest.starts_with("w:") {
            return false;
        }

        const BOOLEAN_FLAGS: &[char] = &[
            'a', 'b', 'c', 'h', 'i', 'l', 'n', 'o', 'p', 'q', 'r', 's', 'u', 'v', 'w', 'x', 'E',
            'F', 'H', 'I', 'L', 'N', 'R', 'S', '0',
        ];

        if rest.chars().all(|c| BOOLEAN_FLAGS.contains(&c)) {
            return true;
        }

        let chars: Vec<char> = rest.chars().collect();
        let mut idx = 0;
        while idx < chars.len() && BOOLEAN_FLAGS.contains(&chars[idx]) {
            idx += 1;
        }

        if idx < chars.len() {
            let flag = chars[idx];
            let remaining: String = chars[idx + 1..].iter().collect();

            // Standalone value-taking flag with no argument (e.g. trailing `-m`, `-C`)
            if remaining.is_empty()
                && matches!(
                    flag,
                    'm' | 'C' | 'B' | 'A' | 'd' | 'j' | 'M' | 't' | 'T' | 'g' | 'e' | 'f' | 'Z'
                )
            {
                return true;
            }

            // Numeric value flags: -m, -C, -B, -A, -d, -j, -M (e.g. -m1, -C2, -d0, -m=1)
            if matches!(flag, 'm' | 'C' | 'B' | 'A' | 'd' | 'j' | 'M') {
                let val = remaining.strip_prefix('=').unwrap_or(&remaining);
                if !val.is_empty() && val.chars().all(|c| c.is_ascii_digit()) {
                    return true;
                }
            }

            // Any option with explicit '=': e.g. -t=rs, -g=*.rs, -e=pat, -m=1
            if matches!(
                flag,
                'm' | 'C' | 'B' | 'A' | 'd' | 'j' | 'M' | 't' | 'T' | 'g' | 'e' | 'f' | 'Z'
            ) && remaining.starts_with('=')
                && remaining.len() > 1
            {
                return true;
            }
        }

        false
    }

    /// Split a fuzzy query string into tokens, validating against shell-stripped empty quotes.
    pub fn split_fuzzy_tokens(content: &str, raw_token: &str) -> Result<Vec<String>, String> {
        let content = content.trim();
        if content.is_empty() {
            return Err(format!(
                "fuzzy search pattern received an empty argument ('{raw_token}'). \
                 If you typed unquoted quotes like %%\"\", your shell stripped them before grx received them! \
                 Wrap the argument in outer quotes: grx %%'\"\",from,ptr' or grx fz:'\"\",from,ptr'"
            ));
        }

        // Comma-separated
        if content.contains(',') {
            let mut tokens = Vec::new();
            for piece in content.split(',') {
                let trimmed = piece.trim();
                if trimmed.is_empty() {
                    return Err(format!(
                        "fuzzy search pattern received an empty token in ('{raw_token}'). \
                         If you typed unquoted quotes like %%\"\", your shell stripped them before grx received them! \
                         Wrap the argument in outer quotes: grx %%'\"\",from,ptr' or grx fz:'\"\",from,ptr'"
                    ));
                }
                tokens.push(trimmed.to_string());
            }
            return Ok(tokens);
        }

        // Whitespace-separated
        if content.contains(|c: char| c.is_whitespace()) {
            let tokens: Vec<String> = content.split_whitespace().map(|s| s.to_string()).collect();
            return Ok(tokens);
        }

        // Snake_case ('_')
        if content.contains('_') {
            let tokens: Vec<String> = content
                .split('_')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            if !tokens.is_empty() {
                return Ok(tokens);
            }
        }

        // CamelCase / Acronym splitting
        if content.chars().any(|c| c.is_uppercase()) && content.chars().any(|c| c.is_lowercase()) {
            let chars: Vec<char> = content.chars().collect();
            let mut tokens = Vec::new();
            let mut current = String::new();

            for i in 0..chars.len() {
                let c = chars[i];
                let is_boundary = if i > 0 && c.is_uppercase() {
                    let prev = chars[i - 1];
                    let next_is_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();
                    prev.is_lowercase() || (prev.is_uppercase() && next_is_lower)
                } else {
                    false
                };

                if is_boundary && !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                current.push(c);
            }
            if !current.is_empty() {
                tokens.push(current);
            }
            if tokens.len() > 1 {
                return Ok(tokens);
            }
        }

        Ok(vec![content.to_string()])
    }

    /// Extract text term from token or raw string representation.
    fn token_to_term(tok: &Token, s: &str) -> String {
        match tok {
            Token::Pattern(SearchPattern::Literal { text, .. }) => text.clone(),
            Token::Pattern(SearchPattern::ExactLiteral(text)) => text.clone(),
            Token::Pattern(SearchPattern::Regex(pat)) => pat.clone(),
            _ => s.to_string(),
        }
    }

    /// Synthesize boolean AND expression from fuzzy tokens.
    pub fn compile_fuzzy_to_expr(tokens: &[String]) -> QueryExpr {
        let mut exprs: Vec<QueryExpr> = tokens
            .iter()
            .map(|t| QueryExpr::Pattern(Self::compile_term_to_pattern(t, false)))
            .collect();
        if exprs.is_empty() {
            return QueryExpr::Pattern(SearchPattern::Literal {
                text: String::new(),
                case_sensitive: None,
            });
        }
        let mut res = exprs.remove(0);
        for next in exprs {
            res = QueryExpr::And(Box::new(res), Box::new(next));
        }
        res
    }

    /// Compile an individual DSL search term into an optimized SearchPattern.
    /// Handles line anchors (^, $), shell-safe wildcards (..), and whole-word boundaries.
    pub fn compile_term_to_pattern(mut term: &str, is_whole_word: bool) -> SearchPattern {
        let anchor_start = if term.starts_with('^') && term.len() > 1 {
            term = &term[1..];
            true
        } else {
            false
        };
        let anchor_end = if term.ends_with('$') && term.len() > 1 {
            term = &term[..term.len() - 1];
            true
        } else {
            false
        };

        let has_wildcard = term.contains("..");

        // If no anchors, no whole-word, and no wildcards, it's a standard literal!
        if !anchor_start && !anchor_end && !is_whole_word && !has_wildcard {
            return SearchPattern::Literal {
                text: term.to_string(),
                case_sensitive: None,
            };
        }

        // Build regex
        let has_upper = term.chars().any(|c| c.is_uppercase());
        let mut flags = String::from("(?");
        if !has_upper {
            flags.push('i');
        }
        if anchor_start || anchor_end {
            flags.push('m');
        }
        flags.push(')');
        let case_prefix = if flags == "(?)" { "" } else { &flags };

        let body = if has_wildcard {
            let parts: Vec<String> = term.split("..").map(regex::escape).collect();
            parts.join(".*")
        } else {
            regex::escape(term)
        };

        let mut pattern = String::new();
        pattern.push_str(case_prefix);

        if anchor_start {
            pattern.push_str(r"^\s*");
        } else if is_whole_word {
            pattern.push_str(r"\b");
        }

        pattern.push_str(&body);

        if anchor_end {
            pattern.push_str(r"\s*$");
        } else if is_whole_word {
            pattern.push_str(r"\b");
        }

        SearchPattern::Regex(pattern)
    }

    /// Convert a quoted glob pattern (e.g. `'fn *(&self)'`, `'foo?bar'`) into an equivalent regex.
    pub fn compile_glob_to_regex(mut glob: &str) -> SearchPattern {
        let anchor_start = if glob.starts_with('^') && glob.len() > 1 {
            glob = &glob[1..];
            true
        } else {
            false
        };
        let anchor_end = if glob.ends_with('$') && glob.len() > 1 {
            glob = &glob[..glob.len() - 1];
            true
        } else {
            false
        };

        let has_upper = glob.chars().any(|c| c.is_uppercase());
        let mut flags = String::from("(?");
        if !has_upper {
            flags.push('i');
        }
        if anchor_start || anchor_end {
            flags.push('m');
        }
        flags.push(')');
        let case_prefix = if flags == "(?)" { "" } else { &flags };

        let mut body = String::new();
        for c in glob.chars() {
            match c {
                '*' => body.push_str(".*"),
                '?' => body.push('.'),
                other => body.push_str(&regex::escape(&other.to_string())),
            }
        }

        let mut pattern = String::new();
        pattern.push_str(case_prefix);
        if anchor_start {
            pattern.push_str(r"^\s*");
        }
        pattern.push_str(&body);
        if anchor_end {
            pattern.push_str(r"\s*$");
        }

        SearchPattern::Regex(pattern)
    }

    /// Parse hex patterns like "4889e5" or "48 ?? e5".
    fn parse_hex_pattern(s: &str) -> Option<Vec<Option<u8>>> {
        let clean = s.replace([' ', '\t', '\n'], "");
        let mut result = Vec::new();

        let mut chars = clean.chars().peekable();
        while let (Some(c1), Some(c2)) = (chars.next(), chars.next()) {
            if c1 == '?' && c2 == '?' {
                result.push(None);
            } else {
                let hex_pair = format!("{c1}{c2}");
                let byte = u8::from_str_radix(&hex_pair, 16).ok()?;
                result.push(Some(byte));
            }
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Parse pattern tokens using standard boolean precedence (NOT > AND > OR).
    /// Implicit juxtaposition (e.g. `foo bar`) is treated as `foo AND bar`.
    fn parse_expression(tokens: &[Token]) -> Result<QueryExpr, String> {
        if matches!(tokens.last(), Some(Token::And | Token::Or)) {
            return Err("Trailing Boolean operator without operand".into());
        }
        // Split by OR
        let mut or_parts = Vec::new();
        let mut current_or_group = Vec::new();

        for tok in tokens {
            if *tok == Token::Or {
                if current_or_group.is_empty() {
                    return Err("Unexpected 'OR' at start of expression".to_string());
                }
                or_parts.push(std::mem::take(&mut current_or_group));
            } else {
                current_or_group.push(tok.clone());
            }
        }
        if !current_or_group.is_empty() {
            or_parts.push(current_or_group);
        }

        if or_parts.is_empty() {
            return Err("Empty search expression".to_string());
        }

        let mut parsed_or_exprs = Vec::new();
        for or_group in or_parts {
            parsed_or_exprs.push(Self::parse_and_sequence(&or_group)?);
        }

        let mut expr = parsed_or_exprs.remove(0);
        for next_expr in parsed_or_exprs {
            expr = QueryExpr::Or(Box::new(expr), Box::new(next_expr));
        }

        Ok(expr)
    }

    /// Parse a sequence of tokens separated by AND (or implicit adjacency).
    fn parse_and_sequence(tokens: &[Token]) -> Result<QueryExpr, String> {
        let mut and_terms = Vec::new();
        let mut i = 0;

        while i < tokens.len() {
            match &tokens[i] {
                Token::And => {
                    if i == 0 || matches!(tokens[i - 1], Token::And | Token::Or | Token::Not) {
                        return Err("AND requires a preceding operand".into());
                    }
                    i += 1;
                }
                Token::Not => {
                    // Unary NOT
                    i += 1;
                    if i >= tokens.len() {
                        return Err("Trailing 'NOT' without operand".to_string());
                    }
                    let operand = Self::parse_primary_token(&tokens[i])?;
                    and_terms.push(QueryExpr::Not(Box::new(operand)));
                    i += 1;
                }
                Token::NegativeTerm(p) => {
                    and_terms.push(QueryExpr::Not(Box::new(QueryExpr::Pattern(p.clone()))));
                    i += 1;
                }
                tok => {
                    let term = Self::parse_primary_token(tok)?;
                    and_terms.push(term);
                    i += 1;
                }
            }
        }

        if and_terms.is_empty() {
            return Err("Empty AND sequence".to_string());
        }

        let mut expr = and_terms.remove(0);
        for next_term in and_terms {
            expr = QueryExpr::And(Box::new(expr), Box::new(next_term));
        }

        Ok(expr)
    }

    fn parse_primary_token(token: &Token) -> Result<QueryExpr, String> {
        match token {
            Token::Pattern(p) | Token::PositiveTerm(p) => Ok(QueryExpr::Pattern(p.clone())),
            other => Err(format!("Unexpected token in expression: {other:?}")),
        }
    }

    fn contains_hex(expr: &QueryExpr) -> bool {
        match expr {
            QueryExpr::Pattern(SearchPattern::Hex(_)) => true,
            QueryExpr::Pattern(_) => false,
            QueryExpr::And(a, b) | QueryExpr::Or(a, b) => {
                Self::contains_hex(a) || Self::contains_hex(b)
            }
            QueryExpr::Not(a) => Self::contains_hex(a),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_raw_terms_before_applying_pattern_modes() {
        let fixed = |s: &str, _: bool| SearchPattern::ExactLiteral(s.into());
        let regex = |s: &str, _: bool| SearchPattern::Regex(s.into());
        let q = DslParser::parse_with_compiler(["a..b", "input.txt"], None, fixed).unwrap();
        assert_eq!(
            q.expr,
            Some(QueryExpr::Pattern(SearchPattern::ExactLiteral(
                "a..b".into()
            )))
        );
        assert_eq!(q.targets, vec![PathBuf::from("input.txt")]);
        let q = DslParser::parse_with_compiler(["a..b", "input.txt"], None, regex).unwrap();
        assert_eq!(
            q.expr,
            Some(QueryExpr::Pattern(SearchPattern::Regex("a..b".into())))
        );
        assert_eq!(q.targets, vec![PathBuf::from("input.txt")]);
    }

    #[test]
    fn path_namespaces_do_not_depend_on_filesystem_state() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sample");
        let token = format!("p:{}", file.display());
        let absent = DslParser::parse(["needle", &token]).unwrap();
        std::fs::write(&file, "needle\n").unwrap();
        assert_eq!(absent, DslParser::parse(["needle", &token]).unwrap());
        assert_eq!(
            DslParser::parse(["needle", "p:missing"])
                .unwrap()
                .path_includes,
            vec!["missing"]
        );
        assert_eq!(
            DslParser::parse(["needle", "in:missing"])
                .unwrap()
                .basename_includes,
            vec!["missing"]
        );
        assert_eq!(
            DslParser::parse(["needle", "nt:rs"]).unwrap().type_excludes,
            vec!["rs"]
        );
        assert_eq!(
            DslParser::parse(["needle", "np:target/"])
                .unwrap()
                .path_excludes,
            vec!["target/"]
        );
        assert_eq!(
            DslParser::parse(["needle", "np:target"])
                .unwrap()
                .path_excludes,
            vec!["target"]
        );
        assert!(DslParser::parse(["needle", "only:src/"]).is_err());
        assert!(DslParser::parse(["needle", "no:rs"]).is_err());
        assert!(DslParser::parse(["needle", "-foo/bar"]).is_err());
        let query = DslParser::parse(["needle", "ns:foo/bar"]).unwrap();
        assert!(query.path_excludes.is_empty());
        assert!(matches!(query.expr, Some(QueryExpr::And(_, _))));
    }

    #[test]
    fn test_shorthand_exclamation_negation_and_numeric_prefix_fallback() {
        // Exclamation directory shorthand: !target/ is deprecated
        let err_dir = DslParser::parse(["needle", "!target/"]).unwrap_err();
        assert!(err_dir.contains("Prefix '!target/' is deprecated and shell-sensitive"));
        let q_dir = DslParser::parse(["needle", "np:target/"]).unwrap();
        assert_eq!(q_dir.path_excludes, vec!["target/"]);

        // Exclamation extension shorthand: !*.min.js is deprecated
        let err_ext = DslParser::parse(["needle", "!*.min.js"]).unwrap_err();
        assert!(err_ext.contains("Prefix '!*.min.js' is deprecated and shell-sensitive"));
        let q_ext = DslParser::parse(["needle", "nt:min.js"]).unwrap();
        assert_eq!(q_ext.type_excludes, vec!["min.js"]);

        // str:: identifier should parse as pattern, not fail with integer parse error
        let q_str = DslParser::parse(["str::from_utf8", "src/"]).unwrap();
        assert_eq!(q_str.targets, vec![PathBuf::from("src/")]);
        assert!(matches!(
            q_str.expr,
            Some(QueryExpr::Pattern(SearchPattern::Literal { ref text, .. })) if text == "str::from_utf8"
        ));

        // str:: bare token should parse as pattern
        let q_str_bare = DslParser::parse(["str::", "src/"]).unwrap();
        assert!(matches!(
            q_str_bare.expr,
            Some(QueryExpr::Pattern(SearchPattern::Literal { ref text, .. })) if text == "str::"
        ));

        // d:: identifier should not fail integer parse
        let q_d = DslParser::parse(["d::item", "src/"]).unwrap();
        assert!(matches!(
            q_d.expr,
            Some(QueryExpr::Pattern(SearchPattern::Literal { ref text, .. })) if text == "d::item"
        ));
    }

    #[test]
    fn reject_malformed_expressions_and_modifiers() {
        for args in [
            vec!["foo", "AND"],
            vec!["foo", "OR"],
            vec!["AND", "foo"],
            vec!["foo", "AND", "AND", "bar"],
            vec!["foo", "NOT"],
            vec!["foo", "ctx:bad"],
            vec!["foo", "top:-1"],
            vec!["hex:zz"],
            vec!["NEAR", "foo"],
            vec!["foo", "NEAR", "AND"],
        ] {
            assert!(DslParser::parse(args.clone()).is_err(), "{args:?}");
        }
    }

    #[test]
    fn preserve_argument_bytes_and_boolean_path_operands() {
        for text in ["", " foo ", "foo/bar"] {
            let query = DslParser::parse([text]).unwrap();
            assert_eq!(
                query.expr,
                Some(QueryExpr::Pattern(SearchPattern::Literal {
                    text: text.into(),
                    case_sensitive: None,
                }))
            );
        }
        let query = DslParser::parse(["foo", "AND", "bar/baz"]).unwrap();
        assert!(matches!(query.expr, Some(QueryExpr::And(_, _))));
        assert!(query.targets.is_empty());
    }

    #[test]
    fn fuzzy_query_preserves_targets_and_combines_constraints() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sample.rs");
        std::fs::write(&file, "from_ptr_err\n").unwrap();
        let query = DslParser::parse(["fz:from,ptr", file.to_str().unwrap()]).unwrap();
        assert_eq!(query.targets, vec![file]);
        let query = DslParser::parse(["fz:from,ptr", "sample.rs"]).unwrap();
        assert_eq!(query.targets, vec![PathBuf::from("sample.rs")]);
        let query = DslParser::parse(["err", "fz:from,ptr"]).unwrap();
        assert!(matches!(query.expr, Some(QueryExpr::And(_, _))));
    }

    #[test]
    fn test_parse_basic_dsl() {
        let args = vec!["connect", "t:rs", "np:target/"];
        let query = DslParser::parse(args).unwrap();

        assert_eq!(
            query.expr,
            Some(QueryExpr::Pattern(SearchPattern::Literal {
                text: "connect".to_string(),
                case_sensitive: None,
            }))
        );
        assert_eq!(query.type_includes, vec!["rs"]);
        assert_eq!(query.path_excludes, vec!["target/"]);

        // Deprecated forms error with clear guidance
        let err_t = DslParser::parse(["connect", ":rs"]).unwrap_err();
        assert!(err_t.contains("Prefix ':rs' is deprecated. Use canonical 't:rs'"));
        let err_np = DslParser::parse(["connect", "no:target/"]).unwrap_err();
        assert!(err_np.contains("Prefix 'no:target/' is deprecated. Use canonical 'np:target/'"));
    }

    #[test]
    fn test_parse_exact_quotes() {
        let args = vec!["\"fn main()\""];
        let query = DslParser::parse(args).unwrap();

        assert_eq!(
            query.expr,
            Some(QueryExpr::Pattern(SearchPattern::ExactLiteral(
                "fn main()".to_string()
            )))
        );
    }

    #[test]
    fn test_parse_regex_and_globs() {
        let args = vec!["/conn_[a-z]+/", "t:toml", "np:dist/,build/"];
        let query = DslParser::parse(args).unwrap();

        assert_eq!(
            query.expr,
            Some(QueryExpr::Pattern(SearchPattern::Regex(
                "conn_[a-z]+".to_string()
            )))
        );
        assert_eq!(query.type_includes, vec!["toml"]);
        assert_eq!(query.path_excludes, vec!["dist/", "build/"]);

        let err_glob = DslParser::parse(["/conn_[a-z]+/", "*.toml"]).unwrap_err();
        assert!(err_glob.contains("Wildcard pattern '*.toml' is shell-sensitive"));
    }

    #[test]
    fn test_comma_separated_filters() {
        let args = vec![
            "pattern",
            "np:llama,unreal,cpp-projects/",
            "p:src,crates,tests/",
            "in:main,lib,mod",
            "ni:test,spec",
        ];
        let query = DslParser::parse(args).unwrap();

        assert_eq!(
            query.path_excludes,
            vec!["llama", "unreal", "cpp-projects/"]
        );
        assert_eq!(query.path_includes, vec!["src", "crates", "tests/"]);
        assert_eq!(query.basename_includes, vec!["main", "lib", "mod"]);
        assert_eq!(query.basename_excludes, vec!["test", "spec"]);
        assert!(query.expr.is_some());
    }

    #[test]
    fn test_comma_separated_dir_selector() {
        let args = vec!["kind:dir", "in:build,cache"];
        let query = DslParser::parse(args).unwrap();
        assert_eq!(query.kind, Some(EntryKind::Dir));
        assert_eq!(query.basename_includes, vec!["build", "cache"]);

        let err_dir = DslParser::parse(vec!["dir:build,cache"]).unwrap_err();
        assert!(err_dir.contains("Prefix 'dir:build,cache' has been consolidated"));
    }

    #[test]
    fn test_parse_boolean_and_or() {
        let args = vec!["auth", "AND", "token", "OR", "secret"];
        let query = DslParser::parse(args).unwrap();

        // Should parse as: (auth AND token) OR secret
        let auth = QueryExpr::Pattern(SearchPattern::Literal {
            text: "auth".to_string(),
            case_sensitive: None,
        });
        let token = QueryExpr::Pattern(SearchPattern::Literal {
            text: "token".to_string(),
            case_sensitive: None,
        });
        let secret = QueryExpr::Pattern(SearchPattern::Literal {
            text: "secret".to_string(),
            case_sensitive: None,
        });

        let expected = QueryExpr::Or(
            Box::new(QueryExpr::And(Box::new(auth), Box::new(token))),
            Box::new(secret),
        );

        assert_eq!(query.expr, Some(expected));
    }

    #[test]
    fn test_parse_hex_pattern() {
        let args = vec!["hex:48??e5"];
        let query = DslParser::parse(args).unwrap();

        assert!(query.hex_mode);
        assert!(query.include_binaries);
        assert_eq!(
            query.expr,
            Some(QueryExpr::Pattern(SearchPattern::Hex(vec![
                Some(0x48),
                None,
                Some(0xE5),
            ])))
        );
    }

    #[test]
    fn test_parse_extension_excludes() {
        let args = vec!["search_term", "nt:c,h", "t:toml", "nt:rs", "nt:py"];
        let query = DslParser::parse(args).unwrap();

        assert_eq!(query.type_excludes, vec!["c", "h", "rs", "py"]);
        assert_eq!(query.type_includes, vec!["toml"]);

        let err_colon = DslParser::parse(vec!["search_term", ":toml"]).unwrap_err();
        assert!(err_colon.contains("Prefix ':toml' is deprecated"));

        let err_no = DslParser::parse(vec!["search_term", "no:rs"]).unwrap_err();
        assert!(err_no.contains("Unrecognized negation 'no:rs'. Use canonical 'nt:rs'"));
    }

    #[test]
    fn test_parse_glob_path_excludes() {
        let args = vec!["search_term", "np:tar*", "np:engine?"];
        let query = DslParser::parse(args).unwrap();

        assert_eq!(query.path_excludes, vec!["tar*", "engine?"]);

        let err = DslParser::parse(vec!["search_term", "no:tar*"]).unwrap_err();
        assert!(err.contains("Unrecognized negation 'no:tar*'"));
    }

    #[test]
    fn test_parse_stdin_dash_target() {
        let args = vec!["apple", "-"];
        let query = DslParser::parse(args).unwrap();

        assert_eq!(query.targets, vec![PathBuf::from("-")]);
        assert_eq!(
            query.expr,
            Some(QueryExpr::Pattern(SearchPattern::Literal {
                text: "apple".to_string(),
                case_sensitive: None,
            }))
        );
    }

    #[test]
    fn test_parse_yes_dots_and_no_dots() {
        let q1 = DslParser::parse(vec!["foo", "yes:dots"]).unwrap();
        assert_eq!(q1.search_hidden, Some(true));

        let err_hidden = DslParser::parse(vec!["foo", "yes:hidden"]).unwrap_err();
        assert!(err_hidden.contains("Option 'yes:hidden' is deprecated"));

        let q3 = DslParser::parse(vec!["foo", "no:dots"]).unwrap();
        assert_eq!(q3.search_hidden, Some(false));

        let err_nohidden = DslParser::parse(vec!["foo", "no:hidden"]).unwrap_err();
        assert!(err_nohidden.contains("Option 'no:hidden' is deprecated"));

        let q_ign1 = DslParser::parse(vec!["foo", "yes:ignore"]).unwrap();
        assert_eq!(q_ign1.respect_ignore, Some(true));

        let q_ign2 = DslParser::parse(vec!["foo", "no:ignore"]).unwrap();
        assert_eq!(q_ign2.respect_ignore, Some(false));

        // Deprecated forms error cleanly
        assert!(DslParser::parse(vec!["foo", "+dot"]).is_err());
        assert!(DslParser::parse(vec!["foo", "-dot"]).is_err());
        assert!(DslParser::parse(vec!["foo", "not:bin"]).is_err());
    }

    #[test]
    fn test_parse_only_types_and_paths() {
        let q = DslParser::parse(vec!["query", "t:rust", "p:src/"]).unwrap();
        assert_eq!(q.type_includes, vec!["rust"]);
        assert_eq!(q.path_includes, vec!["src/"]);

        let err_only = DslParser::parse(vec!["query", "only:rust"]).unwrap_err();
        assert!(
            err_only.contains("Prefix 'only:rust' has been consolidated. Use canonical 't:rust'")
        );

        let q2 = DslParser::parse(vec!["query", "in:report"]).unwrap();
        assert_eq!(q2.basename_includes, vec!["report"]);
    }

    #[test]
    fn test_parse_negative_and_positive_terms() {
        // Line must contain "alpha", but not "beta"
        let q1 = DslParser::parse(vec!["alpha", "ns:beta"]).unwrap();
        let expected = QueryExpr::And(
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "alpha".to_string(),
                case_sensitive: None,
            })),
            Box::new(QueryExpr::Not(Box::new(QueryExpr::Pattern(
                SearchPattern::Literal {
                    text: "beta".to_string(),
                    case_sensitive: None,
                },
            )))),
        );
        assert_eq!(q1.expr, Some(expected.clone()));

        // Boolean: alpha NOT beta
        let q1_bool = DslParser::parse(vec!["alpha", "NOT", "beta"]).unwrap();
        assert_eq!(q1_bool.expr, Some(expected));

        // Boolean: alpha AND gamma
        let q2 = DslParser::parse(vec!["alpha", "AND", "gamma"]).unwrap();
        let expected_and = QueryExpr::And(
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "alpha".to_string(),
                case_sensitive: None,
            })),
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "gamma".to_string(),
                case_sensitive: None,
            })),
        );
        assert_eq!(q2.expr, Some(expected_and));

        // Deprecated forms error with migration guidance
        let err_neg = DslParser::parse(vec!["alpha", "-beta"]).unwrap_err();
        assert!(err_neg.contains("Leading '-' for negative terms is deprecated"));
        let err_pos = DslParser::parse(vec!["+alpha", "+gamma"]).unwrap_err();
        assert!(err_pos.contains("Leading '+' for required terms"));
        let err_not = DslParser::parse(vec!["alpha", "not:beta"]).unwrap_err();
        assert!(err_not.contains("Prefix 'not:beta' is deprecated"));
    }

    #[test]
    fn test_parse_feature_toggles() {
        let q = DslParser::parse(vec!["foo", "kind:bin", "yes:case", "yes:cache"]).unwrap();
        assert!(q.include_binaries);
        assert_eq!(q.case_sensitive, Some(true));
        assert!(q.search_cache);

        let q2 = DslParser::parse(vec!["foo", "kind:text", "no:case"]).unwrap();
        assert!(!q2.include_binaries);
        assert_eq!(q2.case_sensitive, Some(false));

        let err_yesbin = DslParser::parse(vec!["foo", "yes:bin"]).unwrap_err();
        assert!(err_yesbin.contains("'yes:bin' is deprecated"));
        let err_nobin = DslParser::parse(vec!["foo", "no:bin"]).unwrap_err();
        assert!(err_nobin.contains("'no:bin' is deprecated"));
    }

    #[test]
    fn test_parse_with_external_expr() {
        let ext = QueryExpr::Or(
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "error".to_string(),
                case_sensitive: None,
            })),
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "warn".to_string(),
                case_sensitive: None,
            })),
        );
        let q = DslParser::parse_with_external_expr(
            vec!["src/", "t:rs", "np:target/"],
            Some(ext.clone()),
        )
        .unwrap();
        assert_eq!(q.expr, Some(ext));
        assert_eq!(q.targets, vec![PathBuf::from("src/")]);
        assert_eq!(q.type_includes, vec!["rs"]);
        assert_eq!(q.path_excludes, vec!["target/"]);
    }

    #[test]
    fn test_parse_depth_limit_context_and_strings() {
        let q = DslParser::parse(vec!["foo", "d:0", "max:15", "ctx:3", "str:8", "p:src/"]).unwrap();

        assert_eq!(q.max_depth, Some(0));
        assert_eq!(q.max_count, Some(15));
        assert_eq!(q.context, Some(3));
        assert_eq!(q.binary_strings_min_len, Some(8));
        assert!(q.include_binaries);
        assert_eq!(q.path_includes, vec!["src/"]);

        // Deprecated forms error with guidance
        let err_top = DslParser::parse(vec!["top:15"]).unwrap_err();
        assert!(err_top.contains("'top:15' is ambiguous"));

        let err_depth = DslParser::parse(vec!["depth:2"]).unwrap_err();
        assert!(err_depth.contains("Prefix 'depth:2' is deprecated"));

        let err_limit = DslParser::parse(vec!["limit:100"]).unwrap_err();
        assert!(err_limit.contains("'limit:100' is ambiguous"));

        let err_context = DslParser::parse(vec!["context:5"]).unwrap_err();
        assert!(err_context.contains("Prefix 'context:5' is deprecated"));

        let err_strings = DslParser::parse(vec!["strings:16"]).unwrap_err();
        assert!(err_strings.contains("Prefix 'strings:16' is deprecated"));
    }

    #[test]
    fn test_parse_wildcards_anchors_and_whole_word() {
        // Unquoted .. wildcard
        let q1 = DslParser::parse(vec!["foo..bar"]).unwrap();
        assert_eq!(
            q1.expr,
            Some(QueryExpr::Pattern(SearchPattern::Regex(
                "(?i)foo.*bar".to_string()
            )))
        );

        // Case-preserving with uppercase
        let q2 = DslParser::parse(vec!["Foo..Bar"]).unwrap();
        assert_eq!(
            q2.expr,
            Some(QueryExpr::Pattern(SearchPattern::Regex(
                "Foo.*Bar".to_string()
            )))
        );

        // Line start and end anchors
        let q3 = DslParser::parse(vec!["^pub"]).unwrap();
        assert_eq!(
            q3.expr,
            Some(QueryExpr::Pattern(SearchPattern::Regex(
                r"(?im)^\s*pub".to_string()
            )))
        );

        let q4 = DslParser::parse(vec!["struct$"]).unwrap();
        assert_eq!(
            q4.expr,
            Some(QueryExpr::Pattern(SearchPattern::Regex(
                r"(?im)struct\s*$".to_string()
            )))
        );

        let q5 = DslParser::parse(vec!["^fn..main$"]).unwrap();
        assert_eq!(
            q5.expr,
            Some(QueryExpr::Pattern(SearchPattern::Regex(
                r"(?im)^\s*fn.*main\s*$".to_string()
            )))
        );

        // Whole-word: @ is canonical, w: is deprecated
        let q7 = DslParser::parse(vec!["@Test"]).unwrap();
        assert_eq!(
            q7.expr,
            Some(QueryExpr::Pattern(SearchPattern::Regex(
                r"\bTest\b".to_string()
            )))
        );

        let err_w = DslParser::parse(vec!["w:test"]).unwrap_err();
        assert!(err_w.contains("Prefix 'w:test' is deprecated. Use canonical '@test'"));

        // Wildcard .. with trailing target path (including nonexistent path and parent directory ..)
        let q_wc_target = DslParser::parse(vec!["foo..bar", "nonexistent_target.txt"]).unwrap();
        assert_eq!(
            q_wc_target.expr,
            Some(QueryExpr::Pattern(SearchPattern::Regex(
                "(?i)foo.*bar".to_string()
            )))
        );
        assert_eq!(
            q_wc_target.targets,
            vec![PathBuf::from("nonexistent_target.txt")]
        );

        let q_wc_parent = DslParser::parse(vec!["foo..bar", ".."]).unwrap();
        assert_eq!(
            q_wc_parent.expr,
            Some(QueryExpr::Pattern(SearchPattern::Regex(
                "(?i)foo.*bar".to_string()
            )))
        );
        assert_eq!(q_wc_parent.targets, vec![PathBuf::from("..")]);

        // Quoted glob conversion
        let q8 = DslParser::parse(vec!["'fn *(&self)'"]).unwrap();
        assert_eq!(
            q8.expr,
            Some(QueryExpr::Pattern(SearchPattern::Regex(
                r"(?i)fn .*\(\&self\)".to_string()
            )))
        );

        // Negative whole-word via canonical ns:@test
        let q9 = DslParser::parse(vec!["query", "ns:@test"]).unwrap();
        let expected_neg = QueryExpr::And(
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "query".to_string(),
                case_sensitive: None,
            })),
            Box::new(QueryExpr::Not(Box::new(QueryExpr::Pattern(
                SearchPattern::Regex(r"(?i)\btest\b".to_string()),
            )))),
        );
        assert_eq!(q9.expr, Some(expected_neg));

        // Leading -@test is deprecated
        assert!(DslParser::parse(vec!["query", "-@test"]).is_err());
    }

    #[test]
    fn test_parse_path_typo_as_target() {
        let args = vec!["auth", "src/min.rs", "srcc/"];
        let query = DslParser::parse(args).unwrap();

        assert_eq!(
            query.expr,
            Some(QueryExpr::Pattern(SearchPattern::Literal {
                text: "auth".to_string(),
                case_sensitive: None,
            }))
        );
        assert_eq!(
            query.targets,
            vec![PathBuf::from("src/min.rs"), PathBuf::from("srcc/")]
        );
    }

    #[test]
    fn test_parse_first_bare_argument_precedence_and_existing_paths() {
        // "target" exists on disk in a cargo repository.
        // As the first bare argument, it must be parsed as the search pattern, not stolen as a directory target.
        let q1 = DslParser::parse(vec!["target", "src/min.rs"]).unwrap();
        assert_eq!(
            q1.expr,
            Some(QueryExpr::Pattern(SearchPattern::Literal {
                text: "target".to_string(),
                case_sensitive: None,
            }))
        );
        assert_eq!(q1.targets, vec![PathBuf::from("src/min.rs")]);

        // When a pattern has already been specified, a subsequent existing directory ("target") is parsed as a target.
        let q2 = DslParser::parse(vec!["needle", "target"]).unwrap();
        assert_eq!(
            q2.expr,
            Some(QueryExpr::Pattern(SearchPattern::Literal {
                text: "needle".to_string(),
                case_sensitive: None,
            }))
        );
        assert_eq!(q2.targets, vec![PathBuf::from("target")]);

        // When following a boolean operator like AND, "target" is an operand pattern, not a target.
        let q3 = DslParser::parse(vec!["needle", "AND", "target"]).unwrap();
        let expected_and = QueryExpr::And(
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "needle".to_string(),
                case_sensitive: None,
            })),
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "target".to_string(),
                case_sensitive: None,
            })),
        );
        assert_eq!(q3.expr, Some(expected_and));
        assert!(q3.targets.is_empty());

        // When a pattern has already been specified, a subsequent bare word is parsed as a target
        // even if it does NOT exist on disk.
        let q4 = DslParser::parse(vec!["needle", "nonexistent_target_file.rs"]).unwrap();
        assert_eq!(
            q4.expr,
            Some(QueryExpr::Pattern(SearchPattern::Literal {
                text: "needle".to_string(),
                case_sensitive: None,
            }))
        );
        assert_eq!(
            q4.targets,
            vec![PathBuf::from("nonexistent_target_file.rs")]
        );
    }

    #[test]
    fn test_parse_positional_first_argument_slash_pattern() {
        // Lone argument with slash must be parsed as the search pattern, never a target path
        let q1 = DslParser::parse(vec!["Steam/"]).unwrap();
        assert_eq!(
            q1.expr,
            Some(QueryExpr::Pattern(SearchPattern::Literal {
                text: "Steam/".to_string(),
                case_sensitive: None,
            }))
        );
        assert!(
            q1.targets.is_empty(),
            "Lone argument with slash should have empty targets"
        );

        // Two arguments: first is pattern (even with slash), second is target
        let q2 = DslParser::parse(vec!["Steam/", "docs/"]).unwrap();
        assert_eq!(
            q2.expr,
            Some(QueryExpr::Pattern(SearchPattern::Literal {
                text: "Steam/".to_string(),
                case_sensitive: None,
            }))
        );
        assert_eq!(q2.targets, vec![PathBuf::from("docs/")]);

        // Standard order: word pattern, then directory target
        let q3 = DslParser::parse(vec!["haha", "Steam/"]).unwrap();
        assert_eq!(
            q3.expr,
            Some(QueryExpr::Pattern(SearchPattern::Literal {
                text: "haha".to_string(),
                case_sensitive: None,
            }))
        );
        assert_eq!(q3.targets, vec![PathBuf::from("Steam/")]);
    }

    #[test]
    fn test_parse_explicit_no_path_and_no_str_prefixes() {
        // np: is canonical, no-path: is deprecated
        let q1 = DslParser::parse(vec!["needle", "np:*steam*", "np:target/"]).unwrap();
        assert_eq!(q1.path_excludes, vec!["*steam*", "target/"]);
        let err_nopath = DslParser::parse(vec!["needle", "no-path:*steam*"]).unwrap_err();
        assert!(err_nopath.contains("Prefix 'no-path:*steam*' is deprecated"));

        // p: is canonical, path: is deprecated
        let q2 = DslParser::parse(vec!["needle", "p:src/", "p:crates/"]).unwrap();
        assert_eq!(q2.path_includes, vec!["src/", "crates/"]);
        let err_path = DslParser::parse(vec!["needle", "path:src/"]).unwrap_err();
        assert!(err_path.contains("Prefix 'path:src/' is deprecated"));

        // nt: is canonical, no-type: is deprecated
        let q3 = DslParser::parse(vec!["needle", "nt:c,h", "nt:rs"]).unwrap();
        assert_eq!(q3.type_excludes, vec!["c", "h", "rs"]);
        let err_notype = DslParser::parse(vec!["needle", "no-type:c,h"]).unwrap_err();
        assert!(err_notype.contains("Prefix 'no-type:c,h' is deprecated"));

        // ns: is canonical, no-str: is deprecated
        let q4 = DslParser::parse(vec!["needle", "ns:Steam/", "ns:foo"]).unwrap();
        let expected = QueryExpr::And(
            Box::new(QueryExpr::And(
                Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                    text: "needle".to_string(),
                    case_sensitive: None,
                })),
                Box::new(QueryExpr::Not(Box::new(QueryExpr::Pattern(
                    SearchPattern::Literal {
                        text: "Steam/".to_string(),
                        case_sensitive: None,
                    },
                )))),
            )),
            Box::new(QueryExpr::Not(Box::new(QueryExpr::Pattern(
                SearchPattern::Literal {
                    text: "foo".to_string(),
                    case_sensitive: None,
                },
            )))),
        );
        assert_eq!(q4.expr, Some(expected));
        let err_nostr = DslParser::parse(vec!["needle", "no-str:Steam/"]).unwrap_err();
        assert!(err_nostr.contains("Prefix 'no-str:Steam/' is deprecated"));
    }

    #[test]
    fn test_parse_fuzzy_tokens_and_quote_diagnostics() {
        // Comma-separated fuzzy query
        let q1 = DslParser::parse(vec!["fz:from,ptr,err"]).unwrap();
        assert_eq!(
            q1.fuzzy,
            Some(FuzzyQuery {
                tokens: vec!["from".to_string(), "ptr".to_string(), "err".to_string()],
                original: "from,ptr,err".to_string(),
            })
        );
        assert!(q1.expr.is_some());

        // Snake_case fuzzy query via canonical fz:
        let q2 = DslParser::parse(vec!["fz:from_ptr_err"]).unwrap();
        assert_eq!(
            q2.fuzzy,
            Some(FuzzyQuery {
                tokens: vec!["from".to_string(), "ptr".to_string(), "err".to_string()],
                original: "from_ptr_err".to_string(),
            })
        );

        // CamelCase fuzzy query via canonical fz:
        let q3 = DslParser::parse(vec!["fz:fromPtrErr"]).unwrap();
        assert_eq!(
            q3.fuzzy,
            Some(FuzzyQuery {
                tokens: vec!["from".to_string(), "Ptr".to_string(), "Err".to_string()],
                original: "fromPtrErr".to_string(),
            })
        );

        // CamelCase with acronym sequences
        let q_acronym = DslParser::parse(vec!["fz:XMLReader"]).unwrap();
        assert_eq!(
            q_acronym.fuzzy,
            Some(FuzzyQuery {
                tokens: vec!["XML".to_string(), "Reader".to_string()],
                original: "XMLReader".to_string(),
            })
        );

        let q_http = DslParser::parse(vec!["fz:HTTPServer"]).unwrap();
        assert_eq!(
            q_http.fuzzy,
            Some(FuzzyQuery {
                tokens: vec!["HTTP".to_string(), "Server".to_string()],
                original: "HTTPServer".to_string(),
            })
        );

        // Preserved quotes in fuzzy query
        let q4 = DslParser::parse(vec!["fz:\"\",from,ptr"]).unwrap();
        assert_eq!(
            q4.fuzzy.unwrap().tokens,
            vec!["\"\"".to_string(), "from".to_string(), "ptr".to_string()]
        );

        // Deprecated %% prefix returns helpful error
        let err_pct = DslParser::parse(vec!["%%from_ptr_err"]).unwrap_err();
        assert!(err_pct.contains(
            "Fuzzy prefix in '%%from_ptr_err' is deprecated. Use canonical 'fz:from_ptr_err'"
        ));

        // Empty fuzzy argument (e.g. unquoted fz:"") returns helpful error
        let err1 = DslParser::parse(vec!["fz:"]).unwrap_err();
        assert!(err1.contains("empty argument"));

        // Empty token inside comma list
        let err2 = DslParser::parse(vec!["fz:,from,ptr"]).unwrap_err();
        assert!(err2.contains("empty token"));
    }

    #[test]
    fn test_parse_proximity_filters_and_infix() {
        // Compact near:N,pattern
        let q1 = DslParser::parse(vec!["auth", "near:5,safety"]).unwrap();
        assert_eq!(
            q1.proximity_filters,
            vec![ProximityFilter {
                term: "safety".to_string(),
                window: 5,
                inverted: false,
            }]
        );

        // Compact default window near:pattern (window = 3)
        let q2 = DslParser::parse(vec!["auth", "near:safety"]).unwrap();
        assert_eq!(
            q2.proximity_filters,
            vec![ProximityFilter {
                term: "safety".to_string(),
                window: 3,
                inverted: false,
            }]
        );

        // Inverted no-near:N,pattern
        let q3 = DslParser::parse(vec!["unsafe", "no-near:5,safety"]).unwrap();
        assert_eq!(
            q3.proximity_filters,
            vec![ProximityFilter {
                term: "safety".to_string(),
                window: 5,
                inverted: true,
            }]
        );

        // Infix NEAR:5
        let q4 = DslParser::parse(vec!["auth", "NEAR:5", "safety"]).unwrap();
        assert_eq!(
            q4.proximity_filters,
            vec![ProximityFilter {
                term: "safety".to_string(),
                window: 5,
                inverted: false,
            }]
        );

        // Infix NOT NEAR:5
        let q5 = DslParser::parse(vec!["unsafe", "NOT", "NEAR:5", "safety"]).unwrap();
        assert_eq!(
            q5.proximity_filters,
            vec![ProximityFilter {
                term: "safety".to_string(),
                window: 5,
                inverted: true,
            }]
        );
    }

    #[test]
    fn test_discovery_vs_content_operation_selection() {
        let q_disc = DslParser::parse(vec!["in:report", "t:pdf"]).unwrap();
        assert!(q_disc.is_discovery());
        assert!(!q_disc.has_content_pattern);
        assert_eq!(q_disc.basename_includes, vec!["report"]);
        assert_eq!(q_disc.type_includes, vec!["pdf"]);

        let q_content = DslParser::parse(vec!["report"]).unwrap();
        assert!(!q_content.is_discovery());
        assert!(q_content.has_content_pattern);

        let q_empty = DslParser::parse(vec![""]).unwrap();
        assert!(!q_empty.is_discovery());
        assert!(q_empty.has_content_pattern);

        let q_both = DslParser::parse(vec!["annual revenue", "in:report", "t:pdf"]).unwrap();
        assert!(!q_both.is_discovery());
        assert!(q_both.has_content_pattern);
        assert_eq!(q_both.basename_includes, vec!["report"]);
        assert_eq!(q_both.type_includes, vec!["pdf"]);
    }

    #[test]
    fn test_basename_filter_patterns() {
        // in:report (substring)
        let f1 = BasenameFilter::parse("report");
        assert!(f1.matches(b"my_report.txt", None));
        assert!(f1.matches(b"report_2026", None));
        assert!(!f1.matches(b"other.txt", None));

        // in:^report (prefix)
        let f2 = BasenameFilter::parse("^report");
        assert!(f2.matches(b"report_final.txt", None));
        assert!(!f2.matches(b"annual_report.txt", None));

        // in:report$ (suffix)
        let f3 = BasenameFilter::parse("report$");
        assert!(f3.matches(b"annual_report", None));
        assert!(!f3.matches(b"report.txt", None));

        // in:^report$ (exact anchored)
        let f4 = BasenameFilter::parse("^report$");
        assert!(f4.matches(b"report", None));
        assert!(!f4.matches(b"report.txt", None));
        assert!(!f4.matches(b"my_report", None));

        // in:report..2026 (wildcard sequence)
        let f5 = BasenameFilter::parse("report..2026");
        assert!(f5.matches(b"report_q1_2026.pdf", None));
        assert!(f5.matches(b"report2026", None));
        assert!(!f5.matches(b"report_2025", None));
        assert!(!f5.matches(b"2026_report", None));

        // in:=report.md (exact literal)
        let f6 = BasenameFilter::parse("=report.md");
        assert!(f6.matches(b"report.md", None));
        assert!(!f6.matches(b"Report.md", None)); // case sensitive by default
        assert!(f6.matches(b"Report.md", Some(false))); // case override
        assert!(!f6.matches(b"my_report.md", None));

        // smart case: uppercase triggers case sensitivity
        let f7 = BasenameFilter::parse("Report");
        assert!(f7.matches(b"Report.txt", None));
        assert!(!f7.matches(b"report.txt", None));
        assert!(f7.matches(b"report.txt", Some(false))); // override

        // in:foo..$ (wildcard ending with anchor)
        let f8 = BasenameFilter::parse("foo..$");
        assert!(f8.matches(b"prefix_foo", None));
        assert!(f8.matches(b"foo", None));
        assert!(!f8.matches(b"foo_bar.txt", None));

        // in:foo..bar$ (wildcard sequence ending with anchor)
        let f9 = BasenameFilter::parse("foo..bar$");
        assert!(f9.matches(b"foo_test_bar", None));
        assert!(!f9.matches(b"foo_test_bar_baz", None));
    }

    #[test]
    fn test_find_subslice_simd() {
        assert_eq!(find_subslice(b"hello world", b"world", true), Some(6));
        assert_eq!(find_subslice(b"hello world", b"World", true), None);
        assert_eq!(find_subslice(b"hello world", b"WORLD", false), Some(6));
        assert_eq!(find_subslice(b"version 2026.09", b"2026", false), Some(8));
        assert_eq!(find_subslice(b"short", b"very long pattern", true), None);
        assert_eq!(find_subslice(b"any", b"", true), Some(0));
    }

    #[test]
    fn test_basename_excludes_ni() {
        let q = DslParser::parse(vec!["in:report", "ni:temp", "ni:*.bak"]).unwrap();
        assert_eq!(q.basename_includes, vec!["report"]);
        assert_eq!(q.basename_excludes, vec!["temp", "*.bak"]);
        assert_eq!(q.basename_exclude_filters.len(), 2);

        let err = DslParser::parse(vec!["not-in:*.bak"]).unwrap_err();
        assert!(err.contains("Prefix 'not-in:*.bak' is deprecated"));
    }

    #[test]
    fn test_type_selectors_t_and_type() {
        let q1 = DslParser::parse(vec!["t:rs"]).unwrap();
        assert_eq!(q1.type_includes, vec!["rs"]);

        let err_type = DslParser::parse(vec!["type:toml"]).unwrap_err();
        assert!(err_type.contains("Prefix 'type:toml' is deprecated. Use canonical 't:toml'"));

        let q3 = DslParser::parse(vec!["t:rs,c,cpp"]).unwrap();
        assert_eq!(q3.type_includes, vec!["rs", "c", "cpp"]);
    }

    #[test]
    fn test_kind_selectors() {
        let q_file = DslParser::parse(vec!["kind:file"]).unwrap();
        assert_eq!(q_file.kind, Some(EntryKind::File));

        let q_dir = DslParser::parse(vec!["kind:dir"]).unwrap();
        assert_eq!(q_dir.kind, Some(EntryKind::Dir));

        let q_link = DslParser::parse(vec!["kind:link"]).unwrap();
        assert_eq!(q_link.kind, Some(EntryKind::Link));

        let q_bin = DslParser::parse(vec!["kind:bin"]).unwrap();
        assert_eq!(q_bin.kind, Some(EntryKind::Bin));
        assert!(q_bin.include_binaries);
        assert!(q_bin.only_binaries);

        let err_bin = DslParser::parse(vec!["bin:"]).unwrap_err();
        assert!(err_bin.contains("Prefix 'bin:' has been consolidated. Use canonical 'kind:bin'"));

        let q_text = DslParser::parse(vec!["kind:text"]).unwrap();
        assert_eq!(q_text.kind, Some(EntryKind::Text));
        assert!(!q_text.include_binaries);

        assert!(DslParser::parse(vec!["kind:socket"]).is_err());
        assert!(DslParser::parse(vec!["kind:file", "kind:dir"]).is_err());
        assert!(DslParser::parse(vec!["kind:bin", "kind:dir"]).is_err());
        assert!(DslParser::parse(vec!["bin:attached"]).is_err());
    }

    #[test]
    fn test_size_predicates() {
        let q = DslParser::parse(vec!["larger:10MiB", "smaller:1GiB"]).unwrap();
        assert_eq!(
            q.size_predicates,
            vec![
                SizePredicate::Larger(10 * 1024 * 1024),
                SizePredicate::Smaller(1024 * 1024 * 1024),
            ]
        );

        let q_bytes = DslParser::parse(vec!["larger:1024B"]).unwrap();
        assert_eq!(q_bytes.size_predicates, vec![SizePredicate::Larger(1024)]);

        let q_raw = DslParser::parse(vec!["smaller:50"]).unwrap();
        assert_eq!(q_raw.size_predicates, vec![SizePredicate::Smaller(50)]);

        assert!(DslParser::parse(vec!["larger:10k"]).is_err());
        assert!(DslParser::parse(vec!["smaller:bad"]).is_err());
        assert!(DslParser::parse(vec!["larger:"]).is_err());
    }

    #[test]
    fn test_time_predicates() {
        let q = DslParser::parse(vec!["newer:7d", "older:1h"]).unwrap();
        assert_eq!(
            q.time_predicates,
            vec![
                TimePredicate::Newer(std::time::Duration::from_secs(7 * 86400)),
                TimePredicate::Older(std::time::Duration::from_secs(3600)),
            ]
        );

        let q_min = DslParser::parse(vec!["newer:30m"]).unwrap();
        assert_eq!(
            q_min.time_predicates,
            vec![TimePredicate::Newer(std::time::Duration::from_secs(1800))]
        );

        let q_sec = DslParser::parse(vec!["older:45s"]).unwrap();
        assert_eq!(
            q_sec.time_predicates,
            vec![TimePredicate::Older(std::time::Duration::from_secs(45))]
        );

        assert!(DslParser::parse(vec!["newer:7w"]).is_err());
        assert!(DslParser::parse(vec!["older:bad"]).is_err());
        assert!(DslParser::parse(vec!["newer:"]).is_err());
    }

    #[test]
    fn test_literal_path_namespace_and_quotes() {
        // p:"p:path" passed as single argument p:p:path
        let q = DslParser::parse(vec!["needle", "p:p:path"]).unwrap();
        assert_eq!(q.path_includes, vec!["p:path"]);

        let q2 = DslParser::parse(vec!["p:my:dir/path"]).unwrap();
        assert_eq!(q2.path_includes, vec!["my:dir/path"]);
    }

    #[test]
    fn test_unrecognized_long_options_rejected() {
        let err = DslParser::parse(vec!["--ignore-cas"]).unwrap_err();
        assert!(err.contains("Unrecognized option '--ignore-cas'"));

        let err2 = DslParser::parse(vec!["--unknown-flag"]).unwrap_err();
        assert!(err2.contains("Unrecognized option '--unknown-flag'"));

        // '--' delimiter is skipped cleanly
        let q = DslParser::parse(vec!["--", "pattern", "src/"]).unwrap();
        assert_eq!(q.targets, vec![PathBuf::from("src/")]);
    }

    #[test]
    fn test_empty_selectors_rejected() {
        assert!(DslParser::parse(vec!["p:"]).is_err());
        assert!(DslParser::parse(vec!["path:"]).is_err());
        assert!(DslParser::parse(vec!["np:"]).is_err());
        assert!(DslParser::parse(vec!["no-path:"]).is_err());
        assert!(DslParser::parse(vec!["nt:"]).is_err());
        assert!(DslParser::parse(vec!["no-type:"]).is_err());
        assert!(DslParser::parse(vec!["ns:"]).is_err());
        assert!(DslParser::parse(vec!["not:"]).is_err());
        assert!(DslParser::parse(vec!["w:"]).is_err());
        assert!(DslParser::parse(vec!["@"]).is_err());
    }

    #[test]
    fn test_dir_file_link_selectors() {
        let err_dir = DslParser::parse(vec!["dir:src"]).unwrap_err();
        assert!(err_dir.contains("Prefix 'dir:src' has been consolidated. Use 'kind:dir in:src'"));

        let err_file = DslParser::parse(vec!["file:main.rs"]).unwrap_err();
        assert!(
            err_file.contains(
                "Prefix 'file:main.rs' has been consolidated. Use 'kind:file in:main.rs'"
            )
        );

        let err_link = DslParser::parse(vec!["link:lib.so"]).unwrap_err();
        assert!(
            err_link
                .contains("Prefix 'link:lib.so' has been consolidated. Use 'kind:link in:lib.so'")
        );

        let q_dir = DslParser::parse(vec!["kind:dir", "in:src"]).unwrap();
        assert_eq!(q_dir.kind, Some(EntryKind::Dir));
        assert_eq!(q_dir.basename_includes, vec!["src"]);
        assert!(q_dir.is_discovery());

        let q_file = DslParser::parse(vec!["kind:file", "in:main.rs"]).unwrap();
        assert_eq!(q_file.kind, Some(EntryKind::File));
        assert_eq!(q_file.basename_includes, vec!["main.rs"]);

        let q_link = DslParser::parse(vec!["kind:link", "in:lib.so"]).unwrap();
        assert_eq!(q_link.kind, Some(EntryKind::Link));
        assert_eq!(q_link.basename_includes, vec!["lib.so"]);
    }

    #[test]
    fn test_head_tail_and_mutual_exclusion() {
        let q_head = DslParser::parse(vec!["in:test", "head:5"]).unwrap();
        assert_eq!(q_head.head, Some(5));
        assert_eq!(q_head.max_count, None);
        assert_eq!(q_head.tail, None);

        let q_m = DslParser::parse(vec!["in:test", "max:3"]).unwrap();
        assert_eq!(q_m.max_count, Some(3));
        assert_eq!(q_m.head, None);
        assert_eq!(q_m.tail, None);

        let err_m = DslParser::parse(vec!["in:test", "m:3"]).unwrap_err();
        assert!(err_m.contains("Prefix 'm:3' has been consolidated. Use canonical 'max:3'"));

        let q_tail = DslParser::parse(vec!["kind:dir", "in:src", "tail:10"]).unwrap();
        assert_eq!(q_tail.tail, Some(10));
        assert_eq!(q_tail.head, None);
        assert_eq!(q_tail.max_count, None);

        let err = DslParser::parse(vec!["kind:dir", "in:src", "head:5", "tail:10"]).unwrap_err();
        assert!(err.contains("Cannot specify both 'head'"));

        let err2 = DslParser::parse(vec!["in:test", "head:5", "tail:10"]).unwrap_err();
        assert!(err2.contains("Cannot specify both 'head'"));
    }

    #[test]
    fn test_sort_selectors() {
        let q_size = DslParser::parse(vec!["in:test", "sort:size"]).unwrap();
        assert_eq!(q_size.sort, Some(SortKey::Size));

        let q_size_rev = DslParser::parse(vec!["in:test", "sort:-size"]).unwrap();
        assert_eq!(q_size_rev.sort, Some(SortKey::SizeDesc));

        let err_sortr = DslParser::parse(vec!["in:test", "sortr:size"]).unwrap_err();
        assert!(
            err_sortr.contains("Prefix 'sortr:size' is deprecated. Use canonical 'sort:-size'")
        );

        let q_mod = DslParser::parse(vec!["kind:dir", "in:src", "sort:modified"]).unwrap();
        assert_eq!(q_mod.sort, Some(SortKey::Modified));

        let q_newest = DslParser::parse(vec!["kind:dir", "in:src", "sort:newest"]).unwrap();
        assert_eq!(q_newest.sort, Some(SortKey::Modified));

        let q_oldest = DslParser::parse(vec!["kind:dir", "in:src", "sort:oldest"]).unwrap();
        assert_eq!(q_oldest.sort, Some(SortKey::ModifiedDesc));

        let q_len = DslParser::parse(vec!["in:test", "sort:len"]).unwrap();
        assert_eq!(q_len.sort, Some(SortKey::Len));

        assert!(DslParser::parse(vec!["sort:unknown"]).is_err());
        assert!(DslParser::parse(vec!["sort:"]).is_err());
    }

    #[test]
    fn test_trailing_cli_flags_rejection() {
        // Single flag after positional pattern
        let err_f = DslParser::parse(vec!["hit", "-F"]).unwrap_err();
        assert!(err_f.contains("CLI flag '-F' was placed after positional search arguments"));

        // Clustered flags after positional pattern
        let err_iv = DslParser::parse(vec!["hit", "-iv"]).unwrap_err();
        assert!(err_iv.contains("CLI flag '-iv' was placed after positional search arguments"));

        // Single flag after positional pattern with path selector
        let err_i = DslParser::parse(vec!["hit", "-i", "p:flags.txt"]).unwrap_err();
        assert!(err_i.contains("CLI flag '-i' was placed after positional search arguments"));

        // Attached numeric and value flags after positional pattern
        let err_m1 = DslParser::parse(vec!["hit", "-m1"]).unwrap_err();
        assert!(err_m1.contains("CLI flag '-m1' was placed after positional search arguments"));

        let err_c2 = DslParser::parse(vec!["hit", "-C2"]).unwrap_err();
        assert!(err_c2.contains("CLI flag '-C2' was placed after positional search arguments"));

        let err_im1 = DslParser::parse(vec!["hit", "-im1"]).unwrap_err();
        assert!(err_im1.contains("CLI flag '-im1' was placed after positional search arguments"));

        let err_meq = DslParser::parse(vec!["hit", "-m=1"]).unwrap_err();
        assert!(err_meq.contains("CLI flag '-m=1' was placed after positional search arguments"));

        let err_teq = DslParser::parse(vec!["hit", "-t=rs"]).unwrap_err();
        assert!(err_teq.contains("CLI flag '-t=rs' was placed after positional search arguments"));

        // Leading '-' negative terms error with clear guidance
        let err_neg = DslParser::parse(vec!["hit", "-koseoglu"]).unwrap_err();
        assert!(err_neg.contains("Leading '-' for negative terms is deprecated"));
        assert!(err_neg.contains("Use canonical 'ns:koseoglu'"));

        let err_debug = DslParser::parse(vec!["hit", "-debug"]).unwrap_err();
        assert!(err_debug.contains("Leading '-' for negative terms is deprecated"));
        assert!(err_debug.contains("Use canonical 'ns:debug'"));

        // Explicit ns: negation is permitted
        let q_ns = DslParser::parse(vec!["hit", "ns:F"]).unwrap();
        assert!(q_ns.expr.is_some());
        let q_ns_m1 = DslParser::parse(vec!["hit", "ns:m1"]).unwrap();
        assert!(q_ns_m1.expr.is_some());
        let q_ns_kose = DslParser::parse(vec!["hit", "ns:koseoglu"]).unwrap();
        assert!(q_ns_kose.expr.is_some());
    }

    #[test]
    fn test_empty_set_pattern_and_positional_targets() {
        let external = Some(QueryExpr::Pattern(SearchPattern::EmptySet));
        let q = DslParser::parse_with_compiler(["missing.txt"], external, |term, _| {
            SearchPattern::ExactLiteral(term.to_string())
        })
        .unwrap();

        assert_eq!(q.targets, vec![PathBuf::from("missing.txt")]);
        assert_eq!(q.expr, Some(QueryExpr::Pattern(SearchPattern::EmptySet)));
    }

    #[test]
    fn test_discovery_targets_not_hijacked_as_patterns() {
        // 1. in:hosts /etc -> /etc is a target, not a content pattern
        let q1 = DslParser::parse(vec!["in:hosts", "/etc"]).unwrap();
        assert_eq!(q1.targets, vec![PathBuf::from("/etc")]);
        assert!(q1.is_discovery());
        assert_eq!(q1.expr, None);

        // 2. in:main src/ -> path target with slash resolves to target without ambient disk check
        let q2 = DslParser::parse(vec!["in:main", "src/"]).unwrap();
        assert_eq!(q2.targets, vec![PathBuf::from("src/")]);
        assert!(q2.is_discovery());
        assert_eq!(q2.expr, None);

        // 2b. in:main nonexistent/ -> also resolves to target without touching disk
        let q2b = DslParser::parse(vec!["in:main", "nonexistent/"]).unwrap();
        assert_eq!(q2b.targets, vec![PathBuf::from("nonexistent/")]);
        assert!(q2b.is_discovery());
        assert_eq!(q2b.expr, None);

        // 3. Multiple discovery targets: in:report /etc /var
        let q3 = DslParser::parse(vec!["in:report", "/etc", "/var"]).unwrap();
        assert_eq!(
            q3.targets,
            vec![PathBuf::from("/etc"), PathBuf::from("/var")]
        );
        assert!(q3.is_discovery());

        // 4. Pure bare path with slashes without any selector -> content pattern
        let q4 = DslParser::parse(vec!["Steam/"]).unwrap();
        assert_eq!(q4.targets, Vec::<PathBuf>::new());
        assert!(!q4.is_discovery());
        assert!(q4.expr.is_some());

        // 5. parse_with_compiler_and_base with pre-set type filter -> resolves target correctly
        let mut base = Query::default();
        base.type_includes.push("rs".to_string());
        let q5 = DslParser::parse_with_compiler_and_base(
            vec!["src/"],
            None,
            base,
            DslParser::compile_term_to_pattern,
        )
        .unwrap();
        assert_eq!(q5.targets, vec![PathBuf::from("src/")]);
        assert!(q5.is_discovery());

        // 6. A regular file is also a valid positional discovery root.
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("report.txt");
        std::fs::write(&file, "report\n").unwrap();
        let q6 = DslParser::parse(vec!["in:report", file.to_str().unwrap()]).unwrap();
        assert_eq!(q6.targets, vec![file]);
        assert!(q6.is_discovery());
        assert_eq!(q6.expr, None);
    }

    #[test]
    fn test_typo_prefix_diagnostics_and_quoted_escapes() {
        // Unrecognized typo prefixes return clear suggestions
        let err1 = DslParser::parse(vec!["needle", "ext:rs"]).unwrap_err();
        assert!(err1.contains("Unrecognized filter prefix 'ext:'"));
        assert!(err1.contains("Did you mean 't:rs'"));

        let err2 = DslParser::parse(vec!["needle", "ptah:src"]).unwrap_err();
        assert!(err2.contains("Unrecognized filter prefix 'ptah:'"));
        assert!(err2.contains("Did you mean 'p:src'"));

        let err3 = DslParser::parse(vec!["needle", "types:json"]).unwrap_err();
        assert!(err3.contains("Unrecognized filter prefix 'types:'"));
        assert!(err3.contains("Did you mean 't:json'"));

        let err4 = DslParser::parse(vec!["needle", "name:main.rs"]).unwrap_err();
        assert!(err4.contains("Unrecognized filter prefix 'name:'"));
        assert!(err4.contains("Did you mean 'in:main.rs'"));

        let err5 = DslParser::parse(vec!["needle", "dr:src"]).unwrap_err();
        assert!(err5.contains("Unrecognized filter prefix 'dr:'"));
        assert!(err5.contains("Did you mean 'd:src'"));

        // Standalone proximity operator diagnostics
        let err_near = DslParser::parse(vec!["near:3,foo"]).unwrap_err();
        assert!(err_near.contains("'NEAR/near:' proximity filter requires a base search pattern"));

        let err_infix_near = DslParser::parse(vec!["NEAR", "foo"]).unwrap_err();
        assert!(
            err_infix_near.contains("'NEAR/near:' proximity filter requires a base search pattern")
        );

        // Quoted literal search containing typo-like prefixes are preserved as patterns
        let q_quoted = DslParser::parse(vec!["needle", "\"ext:rs\""]).unwrap();
        assert!(q_quoted.expr.is_some());

        let q_exact = DslParser::parse(vec!["needle", "=ext:rs"]).unwrap();
        assert!(q_exact.expr.is_some());

        // Standard colon strings like URLs or Rust namespaces are not mistaken for typo prefixes
        let q_rust = DslParser::parse(vec!["std::io::Error"]).unwrap();
        assert!(q_rust.expr.is_some());

        let q_url = DslParser::parse(vec!["http://localhost:8080"]).unwrap();
        assert!(q_url.expr.is_some());
    }

    #[test]
    fn test_kind_before_pattern_selects_entry_names() {
        for kind in ["file", "dir", "link", "bin", "text"] {
            let q = DslParser::parse([format!("kind:{kind}"), "needle".to_string()]).unwrap();
            assert!(q.is_discovery(), "kind:{kind} should discover names");
            assert_eq!(q.basename_includes, vec!["needle"]);

            let q = DslParser::parse(["needle".to_string(), format!("kind:{kind}")]).unwrap();
            assert!(
                !q.is_discovery(),
                "kind:{kind} after the pattern should search contents"
            );
            assert!(q.basename_includes.is_empty());
        }

        let q_dir = DslParser::parse(vec!["kind:dir", "np:llvm", "np:unreal", "cpp"]).unwrap();
        assert!(q_dir.is_discovery());
        assert_eq!(q_dir.kind, Some(EntryKind::Dir));
        assert_eq!(q_dir.basename_includes, vec!["cpp"]);
        assert_eq!(q_dir.path_excludes, vec!["llvm", "unreal"]);

        let q_link = DslParser::parse(vec!["kind:link", "libssl"]).unwrap();
        assert!(q_link.is_discovery());
        assert_eq!(q_link.kind, Some(EntryKind::Link));
        assert_eq!(q_link.basename_includes, vec!["libssl"]);
    }

    #[test]
    fn test_content_only_terms_are_not_silently_used_as_name_filters() {
        let err = DslParser::parse(["kind:file", "alpha", "OR", "beta"]).unwrap_err();
        assert!(err.contains("Put the content pattern before kind:"));

        let err = DslParser::parse(["kind:dir", "alpha", "NOT", "beta"]).unwrap_err();
        assert!(err.contains("Put the content pattern before kind:"));

        let err = DslParser::parse(["kind:file", "alpha", "ns:debug"]).unwrap_err();
        assert!(err.contains("Put the content pattern before kind:"));

        let q = DslParser::parse(["alpha", "OR", "beta", "kind:file"]).unwrap();
        assert!(!q.is_discovery());

        let q = DslParser::parse(["kind:file", "re:alpha"]).unwrap();
        assert!(!q.is_discovery());

        let q = DslParser::parse(["kind:bin", "str:4"]).unwrap();
        assert!(!q.is_discovery());
        assert_eq!(q.binary_strings_min_len, Some(4));

        let q = DslParser::parse(["kind:file", "needle", "src"]).unwrap();
        assert!(q.is_discovery());
        assert_eq!(q.basename_includes, vec!["needle"]);
        assert_eq!(q.targets, vec![PathBuf::from("src")]);

        let q = DslParser::parse(["kind:file", "=needle.txt"]).unwrap();
        assert!(q.is_discovery());
        assert_eq!(q.basename_includes, vec!["=needle.txt"]);
        assert!(q.basename_filters[0].is_exact);
    }

    #[test]
    fn test_posix_double_dash_treats_subsequent_args_as_literals() {
        let q = DslParser::parse(vec!["--", "-i"]).unwrap();
        assert!(q.expr.is_some());
        assert_eq!(
            q.expr.unwrap(),
            QueryExpr::Pattern(SearchPattern::Literal {
                text: "-i".to_string(),
                case_sensitive: None,
            })
        );

        let q_target = DslParser::parse(vec!["--", "-v", "test_file.txt"]).unwrap();
        assert_eq!(
            q_target.expr.unwrap(),
            QueryExpr::Pattern(SearchPattern::Literal {
                text: "-v".to_string(),
                case_sensitive: None,
            })
        );
        assert_eq!(q_target.targets, vec![PathBuf::from("test_file.txt")]);
    }

    #[test]
    fn test_inline_actions_and_dry_run_dsl() {
        let q_mv = DslParser::parse(vec!["kind:dir", "in:cpp", "d:1", "mv:cpp-projects/"]).unwrap();
        assert_eq!(
            q_mv.action,
            Some(crate::ops::ActionKind::Move(PathBuf::from("cpp-projects/")))
        );
        assert!(!q_mv.dry_run);

        let q_cp_dry = DslParser::parse(vec!["in:test", "cp:backup/", "dry:"]).unwrap();
        assert_eq!(
            q_cp_dry.action,
            Some(crate::ops::ActionKind::Copy(PathBuf::from("backup/")))
        );
        assert!(q_cp_dry.dry_run);

        let err_rm = DslParser::parse(vec!["in:temp", "rm:"]).unwrap_err();
        assert!(err_rm.contains("Prefix 'rm:' has been consolidated. Use canonical 'trash:'"));

        let q_trash = DslParser::parse(vec!["in:temp", "trash:", "--dry-run"]).unwrap();
        assert_eq!(q_trash.action, Some(crate::ops::ActionKind::Trash));
        assert!(q_trash.dry_run);

        let q_rename = DslParser::parse(vec!["in:test", "rename:old/new"]).unwrap();
        assert_eq!(
            q_rename.action,
            Some(crate::ops::ActionKind::Rename {
                pattern: "old".to_string(),
                replacement: "new".to_string(),
            })
        );

        let q_chmod = DslParser::parse(vec!["in:bin", "chmod:755"]).unwrap();
        assert_eq!(
            q_chmod.action,
            Some(crate::ops::ActionKind::Chmod("755".to_string()))
        );

        // Multiple actions error
        let err_multi = DslParser::parse(vec!["in:test", "mv:a/", "cp:b/"]).unwrap_err();
        assert!(err_multi.contains("Multiple file actions specified"));

        // Typo suggestions for actions
        let err_mov = DslParser::parse(vec!["mov:dest/"]).unwrap_err();
        assert!(err_mov.contains("Did you mean 'mv:dest/'"));
    }

    #[test]
    fn test_classify_custom_configured_type_exclude() {
        let mut cfg = crate::config::Config::default();
        cfg.types
            .insert("proto".to_string(), vec!["*.proto".to_string()]);
        let tok = DslParser::classify_token_with_config("nt:proto", Some(&cfg)).unwrap();
        assert_eq!(tok, Token::TypeExclude(vec!["proto".to_string()]));

        let tok_path = DslParser::classify_token_with_config("np:proto", Some(&cfg)).unwrap();
        assert_eq!(tok_path, Token::PathExclude(vec!["proto".to_string()]));

        // Legacy polymorphic no:proto is rejected with migration error
        let err_no = DslParser::classify_token_with_config("no:proto", Some(&cfg)).unwrap_err();
        assert!(err_no.contains("Unrecognized negation 'no:proto'. Use canonical 'nt:proto'"));
    }

    #[test]
    fn test_suggest_size_prefix_typos() {
        let err_size = DslParser::parse(vec!["needle", "size:10M"]).unwrap_err();
        assert!(
            err_size
                .contains("Did you mean 'larger:10M' or 'smaller:10M' for file size filtering?")
        );

        let err_sz = DslParser::parse(vec!["needle", "sz:1K"]).unwrap_err();
        assert!(
            err_sz.contains("Did you mean 'larger:1K' or 'smaller:1K' for file size filtering?")
        );

        let err_bytes = DslParser::parse(vec!["needle", "bytes:500"]).unwrap_err();
        assert!(
            err_bytes
                .contains("Did you mean 'larger:500' or 'smaller:500' for file size filtering?")
        );
    }

    #[test]
    fn test_dsl_tilde_path_resolution_and_safety() {
        if let Some(home) = crate::config::Config::user_home_dir() {
            let home_str = home.to_string_lossy().to_string();

            // 1. Path inclusion: p:~/Data
            let q_inc = DslParser::parse(vec!["query", "p:~/Data"]).unwrap();
            assert_eq!(q_inc.path_includes, vec![format!("{home_str}/Data")]);

            // 2. Comma-separated path inclusion: p:~/Data,~/Projects
            let q_multi = DslParser::parse(vec!["query", "p:~/Data,~/Projects"]).unwrap();
            assert_eq!(
                q_multi.path_includes,
                vec![format!("{home_str}/Data"), format!("{home_str}/Projects")]
            );

            // 3. Path exclusion: np:~/cache/
            let q_exc = DslParser::parse(vec!["query", "np:~/cache/"]).unwrap();
            assert_eq!(q_exc.path_excludes, vec![format!("{home_str}/cache/")]);

            // 4. Action move and copy: mv:~/backup/
            let q_mv = DslParser::parse(vec!["in:test", "mv:~/backup"]).unwrap();
            assert_eq!(
                q_mv.action,
                Some(crate::ops::ActionKind::Move(home.join("backup")))
            );

            let q_cp = DslParser::parse(vec!["in:test", "cp:~/dest/"]).unwrap();
            assert_eq!(
                q_cp.action,
                Some(crate::ops::ActionKind::Copy(home.join("dest/")))
            );

            // 5. Positional target path: grx pattern ~/Data
            let q_pos = DslParser::parse(vec!["pattern", "~/Data"]).unwrap();
            assert_eq!(q_pos.targets, vec![home.join("Data")]);
        }

        // 6. Critical safety invariant: Search patterns containing '~' MUST NEVER be expanded
        let q_pat = DslParser::parse(vec!["~"]).unwrap();
        assert!(
            matches!(q_pat.expr, Some(QueryExpr::Pattern(SearchPattern::Literal { ref text, .. })) if text == "~")
        );
        assert!(q_pat.targets.is_empty());

        let q_pat_infix = DslParser::parse(vec!["foo~bar", "p:src/"]).unwrap();
        assert!(
            matches!(q_pat_infix.expr, Some(QueryExpr::Pattern(SearchPattern::Literal { ref text, .. })) if text == "foo~bar")
        );
        assert_eq!(q_pat_infix.path_includes, vec!["src/"]);
    }
}
