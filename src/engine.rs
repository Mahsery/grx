use crate::cli::Cli;
use crate::config::{ColorChoice, Config, SearchMode};
use crate::core::{ContentReader, DirectoryWalker, EntryPredicate, Matcher};
use crate::dsl::{
    BoolExpr, DslParser, EntryKind, PatternSyntax, Query, QueryExpr, SearchPattern, SelectorExpr,
    SizePredicate, TimePredicate, assign_setting,
};
use crate::execution::{BufferSearch, SearchOptions, process_ordered};
use crate::ignore::GitignoreEngine;
use crate::printer::OutputFormatter;
use crate::reader::AdaptiveReader;
use crate::search::{BooleanEngine, HexMatcher, RegexMatcher, SimdLiteralMatcher};
use crate::walker::ParallelWalker;
use std::io::{self, IsTerminal, stdout};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

thread_local! {
    static SCRATCH: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn is_file_binary(path: &Path, probe_size: usize) -> bool {
    let probe_limit = if probe_size > 0 { probe_size } else { 1024 };
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    use std::io::Read;
    if probe_limit <= 1024 {
        let mut buf = [0u8; 1024];
        let n = match file.read(&mut buf[..probe_limit]) {
            Ok(n) => n,
            Err(_) => return false,
        };
        if n == 0 {
            return false;
        }
        crate::search::is_binary_with_probe_size(&buf[..n], n)
    } else {
        let mut buf = vec![0u8; probe_limit];
        let n = match file.read(&mut buf) {
            Ok(n) => n,
            Err(_) => return false,
        };
        if n == 0 {
            return false;
        }
        crate::search::is_binary_with_probe_size(&buf[..n], n)
    }
}

fn result_index(index: isize, len: usize) -> Option<usize> {
    if index > 0 {
        (index as usize).checked_sub(1).filter(|&index| index < len)
    } else if index < 0 {
        len.checked_sub(index.unsigned_abs())
    } else {
        None
    }
}

fn lexical_absolute(path: &Path, cwd: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn normalized_path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    #[cfg(unix)]
    {
        let raw = path.as_os_str().as_encoded_bytes();
        let raw = raw.strip_prefix(b"./").unwrap_or(raw);
        if raw == b"." {
            return std::borrow::Cow::Borrowed(b"");
        }
        let needs_normalization = (raw.len() > 1 && raw.ends_with(b"/"))
            || raw.windows(2).any(|pair| pair == b"//")
            || raw.split(|byte| *byte == b'/').any(|part| part == b".");
        if !needs_normalization {
            return std::borrow::Cow::Borrowed(raw);
        }
    }
    let normalized: PathBuf = path
        .components()
        .filter(|component| !matches!(component, std::path::Component::CurDir))
        .collect();
    #[allow(unused_mut)]
    let mut bytes = normalized.as_os_str().as_encoded_bytes().to_vec();
    #[cfg(windows)]
    for byte in &mut bytes {
        if *byte == b'\\' {
            *byte = b'/';
        }
    }

    std::borrow::Cow::Owned(bytes)
}

#[derive(Clone)]
enum CompiledPathPredicate {
    Directory {
        matcher: Arc<crate::search::CompiledExpr>,
        absolute: bool,
    },
    Pattern {
        matcher: Arc<crate::search::CompiledExpr>,
        absolute: bool,
    },
}

impl CompiledPathPredicate {
    fn new(raw: &str, case: Option<bool>, cwd: &Path, roots: &[PathBuf]) -> Result<Self, String> {
        let explicit_pattern = raw.starts_with(['=', '^'])
            || raw.starts_with("re:")
            || raw.starts_with("fz:")
            || raw.starts_with("lit:")
            || raw.contains("..");
        if raw.ends_with('/') && !explicit_pattern {
            let trimmed = raw.trim_end_matches('/');
            let raw = if trimmed.is_empty() { "/" } else { trimmed };
            let expanded = Config::expand_tilde(PathBuf::from(raw));
            let anchored = expanded.is_absolute();
            let expanded = if anchored {
                lexical_absolute(&expanded, cwd)
            } else {
                expanded
            };
            let sequence = String::from_utf8(normalized_path_bytes(&expanded).into_owned())
                .map_err(|error| error.to_string())?;
            let sensitive = case.unwrap_or_else(|| raw.chars().any(char::is_uppercase));
            let escaped = regex::escape(&sequence);
            let pattern = if sequence == "/" {
                "^/".to_owned()
            } else if anchored {
                format!("^{escaped}(?:/|$)")
            } else {
                format!("(?:^|/){escaped}(?:/|$)")
            };
            let matcher = crate::search::CompiledExpr::from_query_expr_with_case(
                &QueryExpr::Pattern(SearchPattern::Regex(pattern)),
                Some(sensitive),
            )
            .map_err(|error| error.to_string())?;
            return Ok(Self::Directory {
                matcher: Arc::new(matcher),
                absolute: anchored,
            });
        }
        let mut pattern = DslParser::parse_search_pattern(raw)?;
        let absolute = if let SearchPattern::ExactMatch {
            text,
            case_sensitive,
        } = &pattern
        {
            let operand = Config::expand_tilde(PathBuf::from(text));
            let roots = if roots.is_empty() {
                vec![cwd.to_path_buf()]
            } else {
                roots.to_vec()
            };
            let candidates = if operand.is_absolute() {
                vec![lexical_absolute(&operand, cwd)]
            } else {
                roots
                    .into_iter()
                    .map(|root| {
                        let root = lexical_absolute(&root, cwd);
                        let base = if root.is_file() {
                            root.parent().unwrap_or(cwd)
                        } else {
                            root.as_path()
                        };
                        lexical_absolute(&operand, base)
                    })
                    .collect()
            };
            let expr = candidates
                .into_iter()
                .map(|path| {
                    QueryExpr::Pattern(SearchPattern::ExactMatch {
                        text: path.to_string_lossy().into_owned(),
                        case_sensitive: *case_sensitive,
                    })
                })
                .reduce(|a, b| QueryExpr::Or(Box::new(a), Box::new(b)))
                .expect("at least one search root");
            let matcher = crate::search::CompiledExpr::from_query_expr_with_case(&expr, case)
                .map_err(|error| error.to_string())?;
            return Ok(Self::Pattern {
                matcher: Arc::new(matcher),
                absolute: true,
            });
        } else if raw.starts_with('/') || raw.starts_with("~/") {
            let expanded = Config::expand_tilde(PathBuf::from(raw));
            pattern = DslParser::parse_search_pattern(&format!("^{}", expanded.display()))?;
            true
        } else {
            false
        };
        let matcher = crate::search::CompiledExpr::from_query_expr_with_case(
            &QueryExpr::Pattern(pattern),
            case,
        )
        .map_err(|error| error.to_string())?;
        Ok(Self::Pattern {
            matcher: Arc::new(matcher),
            absolute,
        })
    }

    fn matches(&self, entry: &crate::core::DirEntry, cwd: &Path) -> bool {
        match self {
            Self::Directory { matcher, absolute } => {
                let directory = entry.is_dir.then(|| entry.full_path());
                let path = directory.as_deref().unwrap_or(&entry.parent);
                let anchored = absolute.then(|| lexical_absolute(path, cwd));
                let path = anchored.as_deref().unwrap_or(path);
                matcher.eval_on_line(&normalized_path_bytes(path))
            }
            Self::Pattern { matcher, absolute } => {
                let path = entry.full_path();
                let path = if *absolute {
                    lexical_absolute(&path, cwd)
                } else {
                    path
                };
                matcher.eval_on_line(&normalized_path_bytes(&path))
            }
        }
    }
}

#[derive(Clone)]
enum CompiledEntryPredicate {
    Glob(String),
    Path(CompiledPathPredicate),
    Type {
        patterns: Vec<String>,
        case_sensitive: Option<bool>,
        literal_question_marks: bool,
    },
    Size(SizePredicate),
    Time(TimePredicate),
}

type CompiledSelectorExpr = BoolExpr<CompiledEntryPredicate>;

fn directory_exclusions(expr: &CompiledSelectorExpr, paths: &mut Vec<CompiledPathPredicate>) {
    match expr {
        BoolExpr::And(a, b) => {
            directory_exclusions(a, paths);
            directory_exclusions(b, paths);
        }
        BoolExpr::Not(inner) => excluded_directory_alternatives(inner, paths),
        _ => {}
    }
}

fn excluded_directory_alternatives(
    expr: &CompiledSelectorExpr,
    paths: &mut Vec<CompiledPathPredicate>,
) {
    match expr {
        BoolExpr::Or(a, b) => {
            excluded_directory_alternatives(a, paths);
            excluded_directory_alternatives(b, paths);
        }
        BoolExpr::Pattern(CompiledEntryPredicate::Path(
            path @ CompiledPathPredicate::Directory { .. },
        )) => {
            paths.push(path.clone());
        }
        _ => {}
    }
}

struct SelectionIgnoreFilter {
    base: Arc<dyn crate::core::IgnoreFilter>,
    directories: Vec<CompiledPathPredicate>,
    cwd: PathBuf,
}

impl crate::core::IgnoreFilter for SelectionIgnoreFilter {
    fn filter(&self, parent: &Path, name: &[u8], is_dir: bool) -> crate::core::FilterDecision {
        if is_dir && !self.directories.is_empty() {
            let entry =
                crate::core::DirEntry::new(parent.to_path_buf(), name.to_vec(), true, false, None);
            if self
                .directories
                .iter()
                .any(|path| path.matches(&entry, &self.cwd))
            {
                return crate::core::FilterDecision::SkipDir;
            }
        }
        self.base.filter(parent, name, is_dir)
    }

    fn for_dir(&self, dir: &Path) -> Option<Arc<dyn crate::core::IgnoreFilter>> {
        self.base.for_dir(dir).map(|base| {
            Arc::new(Self {
                base,
                directories: self.directories.clone(),
                cwd: self.cwd.clone(),
            }) as Arc<dyn crate::core::IgnoreFilter>
        })
    }
}

fn compile_selector(
    expr: &SelectorExpr,
    config: &Config,
    case: Option<bool>,
    cwd: &Path,
    roots: &[PathBuf],
) -> Result<CompiledSelectorExpr, String> {
    Ok(match expr {
        BoolExpr::Pattern(predicate) => BoolExpr::Pattern(match predicate {
            crate::dsl::EntryPredicate::Path(raw) => {
                CompiledEntryPredicate::Path(CompiledPathPredicate::new(raw, case, cwd, roots)?)
            }
            crate::dsl::EntryPredicate::Glob(raw) => CompiledEntryPredicate::Glob(raw.clone()),
            crate::dsl::EntryPredicate::Type(raw) => CompiledEntryPredicate::Type {
                patterns: config
                    .types
                    .get(raw)
                    .cloned()
                    .unwrap_or_else(|| vec![raw.clone()]),
                case_sensitive: case,

                literal_question_marks: !config.types.contains_key(raw) && raw.contains('?'),
            },
            crate::dsl::EntryPredicate::Size(predicate) => CompiledEntryPredicate::Size(*predicate),
            crate::dsl::EntryPredicate::Time(predicate) => CompiledEntryPredicate::Time(*predicate),
        }),
        BoolExpr::And(a, b) => BoolExpr::And(
            Box::new(compile_selector(a, config, case, cwd, roots)?),
            Box::new(compile_selector(b, config, case, cwd, roots)?),
        ),
        BoolExpr::Or(a, b) => BoolExpr::Or(
            Box::new(compile_selector(a, config, case, cwd, roots)?),
            Box::new(compile_selector(b, config, case, cwd, roots)?),
        ),
        BoolExpr::Not(a) => BoolExpr::Not(Box::new(compile_selector(a, config, case, cwd, roots)?)),
    })
}

fn selector_matches(
    expr: &CompiledSelectorExpr,
    entry: &crate::core::DirEntry,
    cwd: &Path,
    reference: std::time::SystemTime,
    metadata: &std::cell::OnceCell<Option<std::fs::Metadata>>,
) -> bool {
    let get_metadata = || {
        metadata
            .get_or_init(|| {
                let path = entry.full_path();
                if entry.is_symlink {
                    std::fs::symlink_metadata(path).ok()
                } else {
                    std::fs::metadata(path).ok()
                }
            })
            .as_ref()
    };
    match expr {
        BoolExpr::Pattern(CompiledEntryPredicate::Path(path)) => path.matches(entry, cwd),
        BoolExpr::Pattern(CompiledEntryPredicate::Glob(pattern)) => {
            let path = entry.full_path();
            let relative = path.strip_prefix(cwd).unwrap_or(&path);
            let target = if pattern.contains('/') || pattern.contains('\\') {
                relative
                    .strip_prefix(".")
                    .unwrap_or(relative)
                    .as_os_str()
                    .as_encoded_bytes()
            } else {
                &entry.name
            };
            crate::ignore::glob_match_bytes(pattern.as_bytes(), target)
        }
        BoolExpr::Pattern(CompiledEntryPredicate::Type {
            patterns,
            case_sensitive,
            literal_question_marks,
        }) => {
            entry.is_file()
                && GitignoreEngine::matches_type_patterns(
                    &entry.name,
                    patterns,
                    *case_sensitive,
                    *literal_question_marks,
                )
        }
        BoolExpr::Pattern(CompiledEntryPredicate::Size(predicate)) => {
            entry.is_file()
                && entry
                    .size
                    .or_else(|| get_metadata().map(std::fs::Metadata::len))
                    .is_some_and(|size| predicate.matches(size))
        }
        BoolExpr::Pattern(CompiledEntryPredicate::Time(predicate)) => get_metadata()
            .and_then(|metadata| metadata.modified().ok())
            .is_some_and(|time| predicate.matches(time, reference)),
        BoolExpr::And(a, b) => {
            selector_matches(a, entry, cwd, reference, metadata)
                && selector_matches(b, entry, cwd, reference, metadata)
        }
        BoolExpr::Or(a, b) => {
            selector_matches(a, entry, cwd, reference, metadata)
                || selector_matches(b, entry, cwd, reference, metadata)
        }
        BoolExpr::Not(a) => !selector_matches(a, entry, cwd, reference, metadata),
    }
}

#[derive(Clone)]
pub struct QueryEntryPredicate {
    pub kind: Option<EntryKind>,
    pub binary_null_probe_bytes: usize,
    pub case_sensitive: Option<bool>,
    pub reference_time: std::time::SystemTime,
    pub name_matcher: Option<std::sync::Arc<crate::search::CompiledExpr>>,
    selector: Option<CompiledSelectorExpr>,
    cwd: PathBuf,
}

impl std::fmt::Debug for QueryEntryPredicate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryEntryPredicate")
            .field("kind", &self.kind)
            .field("binary_null_probe_bytes", &self.binary_null_probe_bytes)
            .field("case_sensitive", &self.case_sensitive)
            .field("reference_time", &self.reference_time)
            .field("has_name_matcher", &self.name_matcher.is_some())
            .finish()
    }
}

impl QueryEntryPredicate {
    pub fn new(
        query: &Query,
        config: &Config,
        reference_time: std::time::SystemTime,
    ) -> Result<Self, String> {
        let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
        let selector = query
            .selector_expr
            .as_ref()
            .map(|expr| compile_selector(expr, config, query.case_sensitive, &cwd, &query.targets))
            .transpose()?;
        let name_matcher = query
            .name_expr
            .as_ref()
            .map(|expr| {
                crate::search::CompiledExpr::from_query_expr_with_case(expr, query.case_sensitive)
                    .map(Arc::new)
                    .map_err(|error| error.to_string())
            })
            .transpose()?;

        Ok(Self {
            selector,
            cwd,
            kind: query.kind,
            binary_null_probe_bytes: config.search.binary_null_probe_bytes,
            case_sensitive: query.case_sensitive,
            reference_time,
            name_matcher,
        })
    }

    pub fn matches(&self, entry: &crate::core::DirEntry) -> bool {
        if let Some(ref selector) = self.selector
            && !selector_matches(
                selector,
                entry,
                &self.cwd,
                self.reference_time,
                &std::cell::OnceCell::new(),
            )
        {
            return false;
        }
        if let Some(ref matcher) = self.name_matcher
            && !matcher.eval_on_line(&entry.name)
        {
            return false;
        }

        if let Some(expected_kind) = self.kind {
            match expected_kind {
                EntryKind::File | EntryKind::Bin | EntryKind::Text => {
                    if !entry.is_file() {
                        return false;
                    }
                }
                EntryKind::Dir => {
                    if !entry.is_dir {
                        return false;
                    }
                }
                EntryKind::Link => {
                    if !entry.is_symlink {
                        return false;
                    }
                }
            }
        }

        if let Some(expected_kind) = self.kind
            && (expected_kind == EntryKind::Bin || expected_kind == EntryKind::Text)
        {
            let path = entry.full_path();
            let is_bin = is_file_binary(&path, self.binary_null_probe_bytes);
            if expected_kind == EntryKind::Bin && !is_bin {
                return false;
            }
            if expected_kind == EntryKind::Text && is_bin {
                return false;
            }
        }

        true
    }
}

impl EntryPredicate for QueryEntryPredicate {
    fn matches(&self, entry: &crate::core::DirEntry) -> io::Result<bool> {
        Ok(self.matches(entry))
    }
}

fn query_has_entry_selector(query: &Query) -> bool {
    query.has_entry_selectors()
}

pub struct Engine {
    config: Config,
    cli: Cli,
}

impl Engine {
    pub fn new(config: Config, mut cli: Cli) -> Self {
        cli.normalize();
        Self { config, cli }
    }

    pub fn run(&mut self) -> Result<i32, String> {
        self.cli.normalize();

        if self.cli.tutorial {
            crate::cli::print_tutorial_with_choice(self.cli.color);
            return Ok(0);
        }

        if self.cli.help_full || (self.cli.help && self.cli.all) {
            crate::cli::print_full_help();
            return Ok(0);
        }

        if self.cli.help {
            crate::cli::print_concise_help();
            return Ok(0);
        }

        if self.cli.dump_config {
            print!("{}", Config::generate_commented_toml());
            return Ok(0);
        }

        if self.cli.init_config {
            match Config::init_default_config(self.cli.force) {
                Ok(path) => {
                    println!("grx: initialized configuration at {}", path.display());
                    return Ok(0);
                }
                Err(err) => return Err(err.to_string()),
            }
        }

        if self.cli.config_path {
            let custom_path = self
                .cli
                .config
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(Path::new);
            match Config::locate_config_path(custom_path) {
                Some(path) => println!("{}", path.display()),
                None => println!(
                    "(built-in defaults, fallback: {})",
                    Config::config_file_path().display()
                ),
            }
            return Ok(0);
        }

        if self.cli.paths {
            let current_exe = std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            println!("grx Distro-Appropriate Storage Paths (XDG Compliant):");
            println!("  Binary:         {}", current_exe);
            println!("  Config Dir:     {}", Config::config_dir().display());
            println!("  Config File:    {}", Config::config_file_path().display());
            println!(
                "  Global Ignore:  {}",
                Config::global_ignore_path().display()
            );
            println!("  Data Dir:       {}", Config::data_dir().display());
            println!(
                "  Journal File:   {}",
                Config::journal_file_path().display()
            );
            println!("  Cache Dir:      {}", Config::cache_dir().display());
            println!("  State Dir:      {}", Config::state_dir().display());
            return Ok(0);
        }

        if let Some(ref sh) = self.cli.completions {
            let normalized = crate::completions::detect_shell(Some(sh))?;
            let script = match normalized {
                "fish" => crate::completions::generate_fish_completions(),
                "bash" => crate::completions::generate_bash_completions(),
                "zsh" => crate::completions::generate_zsh_completions(),
                other => {
                    return Err(format!(
                        "Unsupported shell '{other}'. Supported: fish, bash, zsh"
                    ));
                }
            };
            print!("{script}");
            return Ok(0);
        }

        if let Some(ref sh_opt) = self.cli.install_completions {
            let requested = if sh_opt == "auto" {
                None
            } else {
                Some(sh_opt.as_str())
            };
            match crate::completions::install_completion_script(requested) {
                Ok((path, shell)) => {
                    println!("grx: installed {shell} completions to {}", path.display());
                    return Ok(0);
                }
                Err(err) => return Err(format!("Failed to install completions: {err}")),
            }
        }

        let is_undo = self.cli.args.first().map(|s| s.as_str()) == Some("undo");
        if self.cli.clean_trash
            || (is_undo && self.cli.args.iter().any(|a| a == "--clean-trash"))
            || self.cli.args.first().map(|s| s.as_str()) == Some("clean-trash")
        {
            match crate::ops::clean_trash() {
                Ok(count) => {
                    println!(
                        "grx clean-trash: successfully purged {count} item(s) from trash cache."
                    );
                    return Ok(0);
                }
                Err(err) => {
                    eprintln!("grx clean-trash: {err}");
                    return Ok(2);
                }
            }
        }

        let is_list =
            self.cli.list || (is_undo && self.cli.args.iter().any(|a| a == "--list" || a == "-l"));

        if (is_undo || (self.cli.list && self.cli.args.is_empty()))
            && self.cli.regexp.is_empty()
            && self.cli.file.is_empty()
            && self.cli.fuzzy.is_none()
        {
            if is_undo && self.cli.args.iter().any(|a| a == "--help" || a == "-h") {
                println!(
                    "grx undo — Revert filesystem mutations recorded in the transaction WAL\n\nUSAGE:\n    grx undo               Revert latest transaction\n    grx undo <TX_ID>       Revert specific transaction\n    grx undo --dry-run     Preview reversion without touching disk\n    grx undo --list        List recent transactions\n    grx undo --clean-trash Purge trash staging cache"
                );
                return Ok(0);
            }

            if is_list {
                match crate::ops::list_transactions() {
                    Ok(txs) => {
                        if txs.is_empty() {
                            println!("No transaction records found in WAL.");
                        } else {
                            println!("WAL Transactions ({} records):", txs.len());
                            for (id, ts, summary) in txs {
                                println!("  {id:<32} (epoch {ts})  {summary}");
                            }
                        }
                        return Ok(0);
                    }
                    Err(err) => {
                        eprintln!("grx undo: {err}");
                        return Ok(2);
                    }
                }
            }

            let target_tx_id = self
                .cli
                .args
                .iter()
                .skip(1)
                .find(|a| !a.starts_with('-'))
                .cloned();

            let is_dry = self.cli.dry_run
                || (is_undo
                    && self
                        .cli
                        .args
                        .iter()
                        .any(|a| a == "--dry-run" || a == "--dry"));

            if is_dry {
                let tx_res = if let Some(ref id) = target_tx_id {
                    crate::ops::load_transaction_by_id(id)
                } else {
                    crate::ops::load_latest_transaction()
                };
                match tx_res {
                    Ok(tx) => {
                        println!(
                            "grx undo (dry-run): would revert '{}' ({} ops, id: {})",
                            tx.summary,
                            tx.ops.len(),
                            tx.id
                        );
                        return Ok(0);
                    }
                    Err(err) => {
                        eprintln!("grx undo: {err}");
                        return Ok(2);
                    }
                }
            } else {
                let tx_res = if let Some(ref id) = target_tx_id {
                    crate::ops::undo_transaction(id)
                } else {
                    crate::ops::undo_latest()
                };
                match tx_res {
                    Ok(tx) => {
                        println!(
                            "grx undo: successfully reverted '{}' ({})",
                            tx.summary, tx.id
                        );
                        return Ok(0);
                    }
                    Err(err) => {
                        eprintln!("grx undo: {err}");
                        return Ok(2);
                    }
                }
            }
        }

        if self.cli.args.is_empty()
            && self.cli.regexp.is_empty()
            && self.cli.file.is_empty()
            && self.cli.fuzzy.is_none()
            && self.cli.file_type.is_empty()
            && self.cli.file_type_not.is_empty()
            && self.cli.glob.is_empty()
        {
            crate::cli::eprint_concise_help();
            return Ok(2);
        }

        let start_time = std::time::Instant::now();
        let mode = self.cli.mode.unwrap_or(self.config.mode);

        let query = match mode {
            SearchMode::Dsl => self.build_dsl_query()?,
            SearchMode::Grep | SearchMode::GitGrep => {
                return Err(
                    "compatibility modes (grep, git-grep) are dispatched via CLI wrapper; direct in-engine execution is not supported".into(),
                );
            }
        };

        if query.is_discovery()
            && !query_has_entry_selector(&query)
            && query.sort.is_none()
            && query.tail.is_none()
            && query.action.is_none()
            && query.exec.is_empty()
            && query.exec_batch.is_empty()
            && self.cli.exec.is_empty()
            && self.cli.exec_batch.is_empty()
        {
            crate::cli::eprint_concise_help();
            return Ok(2);
        }

        if matches!(query.kind, Some(EntryKind::Dir | EntryKind::Link))
            && query.selector_expr.as_ref().is_some_and(|expr| {
                expr.any(&|predicate| matches!(predicate, crate::dsl::EntryPredicate::Type(_)))
            })
        {
            eprintln!("grx: filetype predicates apply only to regular files");
            return Ok(2);
        }
        if query.stdin && query.has_entry_selectors() {
            eprintln!(
                "grx: stdin cannot be combined with filesystem selection predicates or roots"
            );
            return Ok(2);
        }
        if !query.is_discovery() {
            if query.kind == Some(crate::dsl::EntryKind::Dir)
                || query.kind == Some(crate::dsl::EntryKind::Link)
            {
                eprintln!("grx: content search cannot be combined with kind:dir or kind:link");
                return Ok(2);
            }
            if (query.action.is_some()
                || !query.exec.is_empty()
                || !query.exec_batch.is_empty()
                || !self.cli.exec.is_empty()
                || !self.cli.exec_batch.is_empty())
                && !self.cli.files_with_matches
            {
                eprintln!(
                    "grx: file actions (mv:, cp:, rm:, trash:) cannot be run directly on content search matches without -l / --files-with-matches to explicitly confirm whole-file selection."
                );
                return Ok(2);
            }
        } else {
            if query.max_count.is_some() {
                eprintln!(
                    "grx: max: limits matching content lines per file; use head: for discovery results"
                );
                return Ok(2);
            }
            if !query.proximity_filters.is_empty() {
                eprintln!("grx: proximity matching is only supported for content search");
                return Ok(2);
            }
            if matches!(
                query.sort,
                Some(
                    crate::dsl::SortKey::Count
                        | crate::dsl::SortKey::CountDesc
                        | crate::dsl::SortKey::LineNum
                        | crate::dsl::SortKey::LineNumDesc
                )
            ) {
                eprintln!("grx: count and line-number sorting require content search");
                return Ok(2);
            }
            if query.stdin {
                eprintln!(
                    "grx: stdin target '-' is not supported in discovery mode: stdin is not a directory inventory"
                );
                return Ok(2);
            }
            if query.selector_expr.as_ref().is_some_and(|expr| {
                expr.any(&|predicate| matches!(predicate, crate::dsl::EntryPredicate::Size(_)))
            }) && (query.kind == Some(crate::dsl::EntryKind::Dir)
                || query.kind == Some(crate::dsl::EntryKind::Link))
            {
                eprintln!("grx: size predicates apply only to regular files");
                return Ok(2);
            }
            if query.context.is_some()
                || self.cli.context.is_some()
                || self.cli.before_context.is_some()
                || self.cli.after_context.is_some()
            {
                eprintln!("grx: context lines are only supported for content search");
                return Ok(2);
            }
            if self.cli.only_matching {
                eprintln!("grx: -o / --only-matching is only supported for content search");
                return Ok(2);
            }
            if self.cli.files_without_match {
                eprintln!("grx: -L / --files-without-match is only supported for content search");
                return Ok(2);
            }
            if self.cli.count || self.cli.count_matches {
                eprintln!("grx: -c / --count is only supported for content search");
                return Ok(2);
            }
            if self.cli.byte_offset {
                eprintln!("grx: -b / --byte-offset is only supported for content search");
                return Ok(2);
            }
            if self.cli.column {
                eprintln!("grx: --column is only supported for content search");
                return Ok(2);
            }
        }

        let total_matches = Arc::new(AtomicUsize::new(0));
        let total_matched_lines = Arc::new(AtomicUsize::new(0));
        let files_with_matches = Arc::new(AtomicUsize::new(0));
        let total_files_searched = Arc::new(AtomicUsize::new(0));
        let total_bytes_searched = Arc::new(AtomicU64::new(0));
        let total_skipped_binaries = Arc::new(AtomicUsize::new(0));

        let ignore_engine = self.build_ignore_engine(&query);
        let max_depth = self
            .cli
            .max_depth
            .or(query.max_depth)
            .or(self.config.search.max_depth);
        let is_dir_discovery = query.kind == Some(crate::dsl::EntryKind::Dir);
        let is_link_discovery = query.kind == Some(crate::dsl::EntryKind::Link);
        let is_file_discovery = query.kind == Some(crate::dsl::EntryKind::File)
            || query.kind == Some(crate::dsl::EntryKind::Bin)
            || query.kind == Some(crate::dsl::EntryKind::Text)
            || !query.is_discovery()
            || (query.is_discovery() && query.kind.is_none());
        let walker = ParallelWalker::new(
            self.cli.threads.unwrap_or(self.config.search.threads),
            max_depth,
            query
                .follow_symlinks
                .unwrap_or(self.config.search.follow_symlinks),
        )
        .with_buffer_size(self.config.search.walker_buffer_size_bytes)
        .with_emit_dirs(is_dir_discovery)
        .with_emit_links(is_link_discovery)
        .with_emit_files(is_file_discovery)
        .with_regular_files_only(
            query.kind == Some(crate::dsl::EntryKind::File)
                || query.kind == Some(crate::dsl::EntryKind::Bin)
                || query.kind == Some(crate::dsl::EntryKind::Text)
                || (query.is_discovery() && query.kind.is_none())
                || (!query.is_discovery() && self.config.search.regular_files_only),
        );

        let mmap_threshold = self
            .cli
            .mmap_threshold
            .unwrap_or(self.config.search.mmap_threshold_bytes);
        let max_file_size = self
            .cli
            .max_file_size
            .or(self.config.search.max_file_size_bytes);
        let reader = AdaptiveReader::new(
            mmap_threshold,
            max_file_size,
            self.config.search.buffer_size_bytes,
        );

        let before_context = self
            .cli
            .before_context
            .or(self.cli.context)
            .or(query.context)
            .unwrap_or(self.config.output.context_before);
        let after_context = self
            .cli
            .after_context
            .or(self.cli.context)
            .or(query.context)
            .unwrap_or(self.config.output.context_after);

        if !query.is_discovery()
            && matches!(
                query.sort,
                Some(
                    crate::dsl::SortKey::Len
                        | crate::dsl::SortKey::LenDesc
                        | crate::dsl::SortKey::LineNum
                        | crate::dsl::SortKey::LineNumDesc
                )
            )
            && (before_context > 0 || after_context > 0 || !query.proximity_filters.is_empty())
        {
            return Err(
                "Record-level length/line sorting cannot be combined with context or proximity"
                    .into(),
            );
        }

        let max_columns = if self.cli.no_truncate {
            None
        } else if let Some(mc) = self.cli.max_columns {
            Some(mc)
        } else if io::stdout().is_terminal() {
            Some(1000)
        } else {
            None
        };
        let unrestricted = self.cli.unrestricted;
        let allow_binary = query.include_binaries
            || self.cli.text
            || unrestricted >= 3
            || matches!(
                self.config.search.binary_handling,
                crate::config::BinaryHandling::Search | crate::config::BinaryHandling::HexDump
            );

        let color_choice = if self.cli.pretty {
            ColorChoice::Always
        } else {
            self.cli.color.unwrap_or(self.config.output.color)
        };

        let show_column = self.cli.column && !self.cli.no_column;
        let show_line_numbers = if self.cli.no_line_number || query.line_number == Some(false) {
            false
        } else if self.cli.line_number
            || query.line_number == Some(true)
            || self.cli.pretty
            || show_column
        {
            true
        } else if query.is_discovery() {
            false
        } else {
            self.config.output.line_numbers
        };
        let show_byte_offset = self.cli.byte_offset;
        let count_matches = self.cli.count_matches;

        let show_heading = if self.cli.no_filename {
            false
        } else if self.cli.heading || self.cli.pretty {
            true
        } else if self.cli.no_heading || !io::stdout().is_terminal() {
            false
        } else {
            self.config.output.heading
        };

        let raw_binary_text = self.cli.text
            || self.config.search.binary_handling == crate::config::BinaryHandling::Search;

        let query_predicate = Arc::new(QueryEntryPredicate::new(
            &query,
            &self.config,
            std::time::SystemTime::now(),
        )?);

        let mut directories = Vec::new();
        if let Some(ref selector) = query_predicate.selector {
            directory_exclusions(selector, &mut directories);
        }
        let ignore_engine: Arc<dyn crate::core::IgnoreFilter> = Arc::new(SelectionIgnoreFilter {
            base: Arc::new(ignore_engine),
            directories,
            cwd: query_predicate.cwd.clone(),
        });

        let printer = Arc::new(Mutex::new(
            OutputFormatter::new(
                Box::new(stdout()),
                color_choice,
                self.cli.hyperlinks.unwrap_or(self.config.output.hyperlinks),
                self.config.output.hyperlink_format.clone(),
                show_line_numbers,
                show_heading,
                self.cli.null_output || self.config.output.null_separator,
                self.cli.only_matching,
                self.cli.quiet,
                self.cli.files_with_matches,
                self.cli.count,
                self.cli.no_filename,
                max_columns,
                allow_binary,
            )
            .with_colors(self.config.output.colors.clone())
            .with_name_matcher(query_predicate.name_matcher.clone())
            .with_raw_binary_text(raw_binary_text)
            .with_column(show_column)
            .with_byte_offset(show_byte_offset)
            .with_count_matches(count_matches)
            .with_json(self.cli.json)
            .with_line_len(
                !query.is_discovery()
                    && !self.cli.files_with_matches
                    && !self.cli.files_without_match
                    && matches!(
                        query.sort,
                        Some(crate::dsl::SortKey::Len | crate::dsl::SortKey::LenDesc)
                    ),
            ),
        ));

        let use_stdin = !query.is_discovery()
            && (query.stdin
                || (!io::stdin().is_terminal()
                    && query.targets.is_empty()
                    && !query_has_entry_selector(&query)));

        if use_stdin
            && (query.action.is_some()
                || !query.exec.is_empty()
                || !query.exec_batch.is_empty()
                || !self.cli.exec.is_empty()
                || !self.cli.exec_batch.is_empty())
        {
            eprintln!("grx: file actions and commands require filesystem inputs, not stdin");
            return Ok(2);
        }
        let roots: Vec<PathBuf> = if query.targets.is_empty() && !use_stdin {
            vec![PathBuf::from(".")]
        } else {
            query.targets.clone()
        };
        let suppress_traversal_errors = (self.config.search.suppress_errors
            || self.cli.no_messages)
            && !self.cli.no_ignore_messages;
        let no_messages = self.cli.no_messages;
        let had_error = AtomicBool::new(false);
        for root in &roots {
            if let Err(error) = root.symlink_metadata() {
                if !no_messages {
                    eprintln!("{}: {error}", root.display());
                }
                had_error.store(true, Ordering::Relaxed);
            }
        }

        let null_output = self.cli.null_output || self.config.output.null_separator;
        let discovery_head = query.head;
        let tail_limit = query.tail;
        let mut selected_files_without_match_cnt: Option<usize> = None;

        if query.is_discovery() {
            let pred = Arc::clone(&query_predicate);
            let pr = Arc::clone(&printer);
            let tot_matches = Arc::clone(&total_matches);
            let tot_searched = Arc::clone(&total_files_searched);
            let tot_lines = Arc::clone(&total_matched_lines);

            let sort_key = query.sort;
            let effective_exec = if !self.cli.exec.is_empty() {
                &self.cli.exec
            } else {
                &query.exec
            };
            let effective_exec_batch = if !self.cli.exec_batch.is_empty() {
                &self.cli.exec_batch
            } else {
                &query.exec_batch
            };
            let is_dry = query.dry_run || self.cli.dry_run;
            #[derive(Clone)]
            struct BufferedEntry {
                path: PathBuf,
                is_dir: bool,
                is_symlink: bool,
                size: u64,
                mtime: std::time::SystemTime,
                score: i32,
            }

            let entries = Arc::new(std::sync::Mutex::new(Vec::new()));
            let entries_clone = Arc::clone(&entries);

            let on_entry =
                move |entry: crate::core::DirEntry| -> Result<(), crate::core::WalkerError> {
                    if !pred.matches(&entry) {
                        return Ok(());
                    }
                    let raw_path = entry.full_path();
                    let path = raw_path
                        .strip_prefix("./")
                        .or_else(|_| raw_path.strip_prefix(".\\"))
                        .map(Path::to_path_buf)
                        .unwrap_or(raw_path);
                    let (size, mtime) = if sort_key.is_some() {
                        let meta = if entry.is_symlink {
                            std::fs::symlink_metadata(&path).ok()
                        } else {
                            std::fs::metadata(&path).ok()
                        };
                        let s = entry
                            .size
                            .or_else(|| meta.as_ref().map(|m| m.len()))
                            .unwrap_or(0);
                        let m = meta
                            .as_ref()
                            .and_then(|m| m.modified().ok())
                            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                        (s, m)
                    } else {
                        (entry.size.unwrap_or(0), std::time::SystemTime::UNIX_EPOCH)
                    };
                    if let Ok(mut lock) = entries_clone.lock() {
                        lock.push(BufferedEntry {
                            path,
                            is_dir: entry.is_dir,
                            is_symlink: entry.is_symlink,
                            size,
                            mtime,
                            score: pred
                                .name_matcher
                                .as_ref()
                                .and_then(|matcher| matcher.score_on_line(&entry.name))
                                .unwrap_or(0),
                        });
                    }
                    Ok(())
                };

            if !roots.is_empty()
                && let Err(err) = walker.walk(&roots, Arc::clone(&ignore_engine), &on_entry)
            {
                if err.is_broken_pipe() {
                    return Ok(0);
                }
                return Err(err.to_string());
            }

            let mut list = match entries.lock() {
                Ok(mut guard) => std::mem::take(&mut *guard),
                Err(_) => return Err("Failed to acquire lock on collected entries".into()),
            };

            if let Some(sk) = sort_key {
                use crate::dsl::SortKey;
                match sk {
                    SortKey::Size => {
                        list.sort_by(|a, b| a.size.cmp(&b.size).then_with(|| a.path.cmp(&b.path)))
                    }
                    SortKey::SizeDesc => {
                        list.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)))
                    }
                    SortKey::Modified => {
                        list.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| a.path.cmp(&b.path)))
                    }
                    SortKey::ModifiedDesc => {
                        list.sort_by(|a, b| a.mtime.cmp(&b.mtime).then_with(|| a.path.cmp(&b.path)))
                    }
                    SortKey::Path => list.sort_by(|a, b| a.path.cmp(&b.path)),
                    SortKey::PathDesc => list.sort_by(|a, b| b.path.cmp(&a.path)),
                    SortKey::Len => list.sort_by(|a, b| {
                        a.path
                            .to_string_lossy()
                            .len()
                            .cmp(&b.path.to_string_lossy().len())
                            .then_with(|| a.path.cmp(&b.path))
                    }),
                    SortKey::LenDesc => list.sort_by(|a, b| {
                        b.path
                            .to_string_lossy()
                            .len()
                            .cmp(&a.path.to_string_lossy().len())
                            .then_with(|| a.path.cmp(&b.path))
                    }),
                    SortKey::LineNum
                    | SortKey::LineNumDesc
                    | SortKey::Count
                    | SortKey::CountDesc => {
                        list.sort_by(|a, b| a.path.cmp(&b.path));
                    }
                }
            } else {
                list.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
            }

            if let Some(nth_idx) = query.nth {
                let target_idx = result_index(nth_idx, list.len());

                match target_idx {
                    Some(idx) if idx < list.len() => {
                        let item = list.remove(idx);
                        list = vec![item];
                    }
                    _ => {
                        list.clear();
                    }
                }
            } else if let Some(t) = tail_limit {
                if t == 0 {
                    list.clear();
                } else if list.len() > t {
                    list = list.split_off(list.len() - t);
                }
            } else if let Some(max) = discovery_head
                && list.len() > max
            {
                list.truncate(max);
            }

            if query.pick && !list.is_empty() {
                let display_items: Vec<String> =
                    list.iter().map(|e| e.path.display().to_string()).collect();
                match crate::picker::pick_items(&display_items) {
                    Ok(selected_indices) => {
                        let mut picked = Vec::with_capacity(selected_indices.len());
                        for idx in selected_indices {
                            if idx < list.len() {
                                picked.push(list[idx].clone());
                            }
                        }
                        list = picked;
                    }
                    Err(err) if err == "Selection canceled" => {
                        return Ok(130);
                    }
                    Err(err) => {
                        return Err(err);
                    }
                }
            }

            if had_error.load(Ordering::Relaxed)
                && (query.action.is_some()
                    || !effective_exec.is_empty()
                    || !effective_exec_batch.is_empty())
            {
                return Ok(2);
            }
            if let Some(action) = &query.action {
                let paths: Vec<PathBuf> = list.iter().map(|e| e.path.clone()).collect();
                let use_color = pr.lock().map(|p| p.should_use_color()).unwrap_or(false);
                return execute_file_action(action, &paths, is_dry, self.cli.force, use_color);
            }

            if !effective_exec_batch.is_empty() {
                let paths: Vec<PathBuf> = list.iter().map(|e| e.path.clone()).collect();
                return execute_command_batch(effective_exec_batch, &paths);
            } else if !effective_exec.is_empty() {
                let paths: Vec<PathBuf> = list.iter().map(|e| e.path.clone()).collect();
                return execute_command_per_match(effective_exec, &paths);
            }

            let mut output = pr.lock().map_err(|_| "output lock poisoned".to_string())?;
            for (idx, e) in list.iter().enumerate() {
                tot_matches.fetch_add(1, Ordering::Relaxed);
                tot_searched.fetch_add(1, Ordering::Relaxed);
                tot_lines.fetch_add(1, Ordering::Relaxed);
                let col = sort_key.and_then(|sk| match sk {
                    crate::dsl::SortKey::Modified | crate::dsl::SortKey::ModifiedDesc => {
                        Some(crate::printer::format_mtime_eza(e.mtime))
                    }
                    crate::dsl::SortKey::Size | crate::dsl::SortKey::SizeDesc => {
                        Some(crate::printer::format_size_eza(e.size, e.is_dir))
                    }
                    crate::dsl::SortKey::Len | crate::dsl::SortKey::LenDesc => {
                        Some(format!("{:>5}", e.path.to_string_lossy().len()))
                    }
                    _ => None,
                });
                let line_num = if show_line_numbers {
                    Some(idx + 1)
                } else {
                    None
                };
                if let Err(err) = output.print_entry_with_column(
                    &e.path,
                    e.is_dir,
                    e.is_symlink,
                    col.as_deref(),
                    line_num,
                    null_output,
                ) {
                    if err.kind() == io::ErrorKind::BrokenPipe {
                        return Ok(0);
                    }
                    return Err(err.to_string());
                }
            }
        } else {
            let content_sort = query.sort;
            let content_head = query.head;
            let content_tail = query.tail;
            let effective_exec = if !self.cli.exec.is_empty() {
                &self.cli.exec
            } else {
                &query.exec
            };
            let effective_exec_batch = if !self.cli.exec_batch.is_empty() {
                &self.cli.exec_batch
            } else {
                &query.exec_batch
            };
            let is_dry = query.dry_run || self.cli.dry_run;
            let matcher = self.build_matcher(&query)?;
            let search = BufferSearch {
                matcher: matcher.as_ref(),
                options: SearchOptions {
                    max_count: query.max_count,
                    has_proximity: !query.proximity_filters.is_empty(),
                    before_context,
                    after_context,
                    skip_binary: self.config.search.binary_detection && !allow_binary,
                    only_binary: query.only_binaries,
                    binary_strings_min_len: query.binary_strings_min_len,
                    binary_null_probe_bytes: self.config.search.binary_null_probe_bytes,
                },
            };
            let files_without_match = self.cli.files_without_match;
            let print_empty_count = self.cli.count || self.cli.count_matches;

            #[derive(Clone)]
            struct BufferedFileResult {
                path: PathBuf,
                records: Vec<crate::core::OwnedMatchRecord>,
                matches: usize,
                _matched_lines: usize,
                file_size: u64,
                mtime: std::time::SystemTime,
                score: i32,
            }
            let mut results = Vec::new();

            let content_expression = query
                .expr
                .as_ref()
                .map(|expr| {
                    crate::search::CompiledExpr::from_query_expr_with_case(
                        expr,
                        query.case_sensitive,
                    )
                    .map_err(|error| error.to_string())
                })
                .transpose()?;
            let fuzzy_content = content_expression
                .as_ref()
                .is_some_and(|expr| expr.has_fuzzy());
            let fuzzy_names = query_predicate
                .name_matcher
                .as_ref()
                .is_some_and(|expr| expr.has_fuzzy());
            let has_effects = query.action.is_some()
                || !effective_exec.is_empty()
                || !effective_exec_batch.is_empty();
            let quiet_existence = self.cli.quiet
                && !has_effects
                && query.nth.is_none()
                && query.tail.is_none()
                && query.head.is_none()
                && !query.pick
                && !self.cli.stats
                && !self.cli.count
                && !self.cli.count_matches
                && !self.cli.json;
            let rank_fuzzy =
                !quiet_existence && content_sort.is_none() && (fuzzy_content || fuzzy_names);

            let emit_result = |f: &BufferedFileResult| -> io::Result<()> {
                let mut output = printer
                    .lock()
                    .map_err(|_| io::Error::other("output lock poisoned"))?;
                let col = query.sort.and_then(|sk| match sk {
                    crate::dsl::SortKey::Modified | crate::dsl::SortKey::ModifiedDesc => {
                        Some(crate::printer::format_mtime_eza(f.mtime))
                    }
                    crate::dsl::SortKey::Size | crate::dsl::SortKey::SizeDesc => {
                        Some(crate::printer::format_size_eza(f.file_size, false))
                    }
                    crate::dsl::SortKey::Count | crate::dsl::SortKey::CountDesc => {
                        Some(format!("{:>5}", f.matches))
                    }
                    crate::dsl::SortKey::Len | crate::dsl::SortKey::LenDesc
                        if files_without_match || self.cli.files_with_matches =>
                    {
                        Some(format!("{:>5}", f.path.to_string_lossy().len()))
                    }
                    _ => None,
                });
                if files_without_match {
                    output.print_file_without_match_with_column(
                        &f.path,
                        col.as_deref(),
                        null_output,
                    )
                } else {
                    let borrowed: Vec<crate::core::MatchRecord> = f
                        .records
                        .iter()
                        .map(|r| crate::core::MatchRecord {
                            line_number: r.line_number,
                            line_byte_offset: r.line_byte_offset,
                            line_bytes: &r.line_bytes,
                            match_spans: r.match_spans.clone(),
                            occurrence_count: r.occurrence_count,
                            context_origins: r.context_origins.clone(),
                            near_spans: r.near_spans.clone(),
                            is_context: r.is_context,
                        })
                        .collect();
                    output.print_file_matches_with_column(&f.path, &borrowed, col.as_deref())
                }
            };
            let retain_results = (content_head.is_some() && !query.proximity_filters.is_empty())
                || rank_fuzzy
                || query.tail.is_some()
                || query.nth.is_some()
                || query.pick
                || has_effects
                || content_sort.is_some_and(|sort| {
                    !matches!(
                        sort,
                        crate::dsl::SortKey::Path | crate::dsl::SortKey::PathDesc
                    )
                });
            let presence_only = (self.cli.quiet
                || ((self.cli.files_with_matches || files_without_match) && !self.cli.json))
                && !rank_fuzzy
                && !self.cli.count
                && !self.cli.count_matches
                && !self.cli.stats
                && !self.cli.json;

            let process_buffer = |path: &Path,
                                  buffer: &[u8],
                                  remaining: Option<usize>|
             -> io::Result<Option<BufferedFileResult>> {
                let max_count = match (
                    query.max_count,
                    if presence_only {
                        Some(1)
                    } else if !retain_results {
                        remaining
                    } else {
                        None
                    },
                ) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                let search = BufferSearch {
                    matcher: search.matcher,
                    options: SearchOptions {
                        max_count,
                        ..search.options
                    },
                };
                let Some(result) = search.search(buffer)? else {
                    total_skipped_binaries.fetch_add(1, Ordering::Relaxed);
                    return Ok(None);
                };
                let content_score = if fuzzy_content {
                    result
                        .records
                        .iter()
                        .filter(|record| !record.is_context)
                        .filter_map(|record| {
                            content_expression
                                .as_ref()?
                                .score_on_line(record.line_bytes)
                        })
                        .max()
                        .unwrap_or(0)
                } else {
                    0
                };
                let name_score = if fuzzy_names {
                    path.file_name()
                        .and_then(|name| {
                            query_predicate
                                .name_matcher
                                .as_ref()?
                                .score_on_line(name.as_encoded_bytes())
                        })
                        .unwrap_or(0)
                } else {
                    0
                };
                let score = content_score.saturating_add(name_score);
                total_files_searched.fetch_add(1, Ordering::Relaxed);
                total_bytes_searched.fetch_add(buffer.len() as u64, Ordering::Relaxed);
                total_matches.fetch_add(result.matches, Ordering::Relaxed);
                total_matched_lines.fetch_add(result.matched_lines, Ordering::Relaxed);
                if result.matched_lines > 0 {
                    files_with_matches.fetch_add(1, Ordering::Relaxed);
                }

                if (files_without_match && result.matched_lines != 0)
                    || (!files_without_match && result.matched_lines == 0 && !print_empty_count)
                {
                    return Ok(None);
                }
                let records = if files_without_match {
                    Vec::new()
                } else if self.cli.files_with_matches && !self.cli.json {
                    result
                        .records
                        .iter()
                        .find(|record| !record.is_context)
                        .map(|record| record.to_owned())
                        .into_iter()
                        .collect()
                } else {
                    result
                        .records
                        .iter()
                        .map(|record| record.to_owned())
                        .collect()
                };
                let mtime = std::fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                Ok(Some(BufferedFileResult {
                    path: path.to_path_buf(),
                    records,
                    matches: result.matches,
                    _matched_lines: result.matched_lines,
                    file_size: buffer.len() as u64,
                    mtime,
                    score,
                }))
            };

            let paths = Mutex::new(Vec::new());
            let on_entry = |entry: crate::core::DirEntry| -> Result<(), crate::core::WalkerError> {
                if query_predicate.matches(&entry) {
                    paths
                        .lock()
                        .map_err(|_| io::Error::other("candidate path lock poisoned"))?
                        .push(entry.full_path());
                }
                Ok(())
            };
            if !roots.is_empty() {
                walker
                    .walk(&roots, Arc::clone(&ignore_engine), &on_entry)
                    .map_err(|error| error.to_string())?;
            }
            if has_effects && had_error.load(Ordering::Relaxed) {
                return Ok(2);
            }
            let mut paths = paths
                .into_inner()
                .map_err(|_| "candidate path lock poisoned".to_owned())?;
            paths.sort();
            if matches!(content_sort, Some(crate::dsl::SortKey::PathDesc)) {
                paths.reverse();
            }
            if use_stdin {
                paths.push(PathBuf::from("(standard input)"));
            }
            let remaining = AtomicUsize::new(content_head.unwrap_or(usize::MAX));
            let early_exit = !retain_results && (content_head.is_some() || self.cli.quiet);
            let requested_threads = self.cli.threads.unwrap_or(self.config.search.threads);
            let parallelism = if requested_threads == 0 {
                std::thread::available_parallelism()
                    .map_or(1, usize::from)
                    .min((paths.len() / 8).max(1))
            } else {
                requested_threads
            };
            let workers = if early_exit || use_stdin {
                1
            } else {
                parallelism.min(paths.len()).max(1)
            };
            let window = if early_exit {
                1
            } else {
                workers.saturating_mul(2)
            };
            let mut selected_without_match = 0;
            let process_path = |path: PathBuf| -> io::Result<Option<BufferedFileResult>> {
                let limit = content_head.map(|_| remaining.load(Ordering::Relaxed));
                if !retain_results && limit == Some(0) {
                    return Ok(None);
                }
                SCRATCH.with(|cell| -> io::Result<Option<BufferedFileResult>> {
                    let mut scratch = cell.borrow_mut();
                    scratch.clear();
                    if use_stdin && path == Path::new("(standard input)") {
                        use std::io::Read;
                        io::stdin().read_to_end(&mut scratch)?;
                        return process_buffer(&path, &scratch, limit);
                    }
                    let buffer = match reader.read(&path, &mut scratch) {
                        Ok(buffer) => buffer,
                        Err(error) => {
                            if has_effects {
                                had_error.store(true, Ordering::Relaxed);
                            }
                            let benign = matches!(
                                error.kind(),
                                io::ErrorKind::PermissionDenied
                                    | io::ErrorKind::NotFound
                                    | io::ErrorKind::IsADirectory
                            );
                            if !suppress_traversal_errors || (!benign && !no_messages) {
                                if !no_messages {
                                    eprintln!("{}: {error}", path.display());
                                }
                                had_error.store(true, Ordering::Relaxed);
                            }
                            return Ok(None);
                        }
                    };
                    process_buffer(&path, &buffer, limit)
                })
            };
            let run = process_ordered(paths, workers, window, &process_path, |searched| {
                let Some(file) = searched else {
                    return Ok(
                        if !retain_results
                            && content_head.is_some()
                            && remaining.load(Ordering::Relaxed) == 0
                        {
                            std::ops::ControlFlow::Break(())
                        } else {
                            std::ops::ControlFlow::Continue(())
                        },
                    );
                };
                if retain_results {
                    results.push(file);
                } else {
                    let selected = if self.cli.files_with_matches || files_without_match {
                        1
                    } else {
                        file.records
                            .iter()
                            .filter(|record| !record.is_context)
                            .count()
                    };
                    if content_head.is_some() {
                        remaining.fetch_sub(selected, Ordering::Relaxed);
                    }
                    if files_without_match {
                        selected_without_match += 1;
                    }
                    emit_result(&file)?;
                    if (self.cli.quiet && !has_effects)
                        || (content_head.is_some() && remaining.load(Ordering::Relaxed) == 0)
                    {
                        return Ok(std::ops::ControlFlow::Break(()));
                    }
                }
                Ok(std::ops::ControlFlow::Continue(()))
            });
            if let Err(error) = run {
                if error.kind() == io::ErrorKind::BrokenPipe {
                    return Ok(0);
                }
                return Err(error.to_string());
            }
            if files_without_match && !retain_results {
                selected_files_without_match_cnt = Some(selected_without_match);
            }
            if retain_results {
                results.sort_by(|a, b| a.path.cmp(&b.path));
                if rank_fuzzy {
                    results.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
                    if before_context == 0
                        && after_context == 0
                        && query.proximity_filters.is_empty()
                        && fuzzy_content
                    {
                        for file in &mut results {
                            file.records.sort_by_cached_key(|record| {
                                let score = content_expression
                                    .as_ref()
                                    .and_then(|expr| expr.score_on_line(&record.line_bytes))
                                    .unwrap_or(0);
                                (std::cmp::Reverse(score), record.line_number)
                            });
                        }
                    }
                }
                if let Some(sk) = content_sort {
                    use crate::dsl::SortKey;
                    match sk {
                        SortKey::Size => results.sort_by(|a, b| {
                            a.file_size
                                .cmp(&b.file_size)
                                .then_with(|| a.path.cmp(&b.path))
                        }),
                        SortKey::SizeDesc => results.sort_by(|a, b| {
                            b.file_size
                                .cmp(&a.file_size)
                                .then_with(|| a.path.cmp(&b.path))
                        }),
                        SortKey::Modified => results.sort_by(|a, b| {
                            b.mtime.cmp(&a.mtime).then_with(|| a.path.cmp(&b.path))
                        }),
                        SortKey::ModifiedDesc => results.sort_by(|a, b| {
                            a.mtime.cmp(&b.mtime).then_with(|| a.path.cmp(&b.path))
                        }),
                        SortKey::Path => results.sort_by(|a, b| a.path.cmp(&b.path)),
                        SortKey::PathDesc => results.sort_by(|a, b| b.path.cmp(&a.path)),
                        SortKey::Count => results.sort_by(|a, b| {
                            b.matches.cmp(&a.matches).then_with(|| a.path.cmp(&b.path))
                        }),
                        SortKey::CountDesc => results.sort_by(|a, b| {
                            a.matches.cmp(&b.matches).then_with(|| a.path.cmp(&b.path))
                        }),
                        SortKey::Len => {
                            for f in &mut results {
                                f.records.sort_by_key(|a| a.line_bytes.len());
                            }
                        }
                        SortKey::LenDesc => {
                            for f in &mut results {
                                f.records
                                    .sort_by_key(|a| std::cmp::Reverse(a.line_bytes.len()));
                            }
                        }
                        SortKey::LineNum => {
                            for f in &mut results {
                                f.records.sort_by_key(|a| a.line_number);
                            }
                        }
                        SortKey::LineNumDesc => {
                            for f in &mut results {
                                f.records.sort_by_key(|a| std::cmp::Reverse(a.line_number));
                            }
                        }
                    }
                }

                if files_without_match || self.cli.files_with_matches {
                    if let Some(nth_idx) = query.nth {
                        let target_idx = result_index(nth_idx, results.len());
                        match target_idx {
                            Some(idx) if idx < results.len() => {
                                let item = results.remove(idx);
                                results = vec![item];
                            }
                            _ => {
                                results.clear();
                            }
                        }
                    } else if let Some(t) = content_tail {
                        if t == 0 {
                            results.clear();
                        } else if results.len() > t {
                            results = results.split_off(results.len() - t);
                        }
                    } else if let Some(h) = content_head
                        && results.len() > h
                    {
                        results.truncate(h);
                    }
                } else if query.nth.is_some() || content_head.is_some() || content_tail.is_some() {
                    let total_matching_lines: usize = results
                        .iter()
                        .map(|file| {
                            file.records
                                .iter()
                                .filter(|record| !record.is_context)
                                .count()
                        })
                        .sum();
                    let (keep_start, keep_end) = if let Some(index) = query.nth {
                        result_index(index, total_matching_lines)
                            .map(|index| (index, index + 1))
                            .unwrap_or((0, 0))
                    } else if let Some(tail) = content_tail {
                        (
                            total_matching_lines.saturating_sub(tail),
                            total_matching_lines,
                        )
                    } else {
                        (
                            0,
                            content_head
                                .unwrap_or(total_matching_lines)
                                .min(total_matching_lines),
                        )
                    };
                    let mut current_match_idx = 0;
                    results.retain_mut(|file| {
                        let selected: std::collections::BTreeSet<_> = file
                            .records
                            .iter()
                            .filter(|record| !record.is_context)
                            .filter_map(|record| {
                                let keep = (keep_start..keep_end).contains(&current_match_idx);
                                current_match_idx += 1;
                                keep.then_some(record.line_number)
                            })
                            .collect();
                        file.records
                            .retain_mut(|record| record.retain_for_primary(&selected));
                        !file.records.is_empty() || print_empty_count
                    });
                }

                if query.pick && !results.is_empty() {
                    let display_items: Vec<String> = results
                        .iter()
                        .map(|f| f.path.display().to_string())
                        .collect();
                    match crate::picker::pick_items(&display_items) {
                        Ok(selected_indices) => {
                            let mut picked = Vec::with_capacity(selected_indices.len());
                            for idx in selected_indices {
                                if idx < results.len() {
                                    picked.push(results[idx].clone());
                                }
                            }
                            results = picked;
                        }
                        Err(err) if err == "Selection canceled" => {
                            return Ok(130);
                        }
                        Err(err) => {
                            return Err(err);
                        }
                    }
                }

                {
                    if files_without_match {
                        selected_files_without_match_cnt = Some(results.len());
                    } else {
                        let mut sliced_matches = 0;
                        let mut sliced_lines = 0;
                        let mut sliced_files_with_match = 0;
                        for f in &results {
                            let file_matches: usize = if self.cli.files_with_matches {
                                f.matches
                            } else {
                                f.records
                                    .iter()
                                    .filter(|record| !record.is_context)
                                    .map(|record| record.occurrence_count)
                                    .sum()
                            };
                            let file_matched_lines: usize = if self.cli.files_with_matches {
                                f._matched_lines
                            } else {
                                f.records.iter().filter(|record| !record.is_context).count()
                            };
                            sliced_matches += file_matches;
                            sliced_lines += file_matched_lines;
                            if file_matched_lines > 0
                                || (self.cli.files_with_matches && !f.records.is_empty())
                            {
                                sliced_files_with_match += 1;
                            }
                        }
                        total_matches.store(sliced_matches, Ordering::Relaxed);
                        total_matched_lines.store(sliced_lines, Ordering::Relaxed);
                        files_with_matches.store(sliced_files_with_match, Ordering::Relaxed);
                    }
                }

                if had_error.load(Ordering::Relaxed)
                    && (query.action.is_some()
                        || !effective_exec.is_empty()
                        || !effective_exec_batch.is_empty())
                {
                    return Ok(2);
                }
                if let Some(action) = &query.action {
                    let paths: Vec<PathBuf> = results.iter().map(|f| f.path.clone()).collect();
                    let use_color = printer
                        .lock()
                        .map(|p| p.should_use_color())
                        .unwrap_or(false);
                    return execute_file_action(action, &paths, is_dry, self.cli.force, use_color);
                }

                if !effective_exec_batch.is_empty() {
                    let paths: Vec<PathBuf> = results.iter().map(|f| f.path.clone()).collect();
                    return execute_command_batch(effective_exec_batch, &paths);
                } else if !effective_exec.is_empty() {
                    let paths: Vec<PathBuf> = results.iter().map(|f| f.path.clone()).collect();
                    return execute_command_per_match(effective_exec, &paths);
                }

                for f in &results {
                    let res = emit_result(f);
                    if let Err(err) = res {
                        if err.kind() == io::ErrorKind::BrokenPipe {
                            return Ok(0);
                        }
                        return Err(err.to_string());
                    }
                }
            }
        }

        let matches_found = total_matches.load(Ordering::Relaxed);
        let matched_lines_found = total_matched_lines.load(Ordering::Relaxed);
        let files_with_match_cnt = files_with_matches.load(Ordering::Relaxed);
        let files_searched = total_files_searched.load(Ordering::Relaxed);
        let bytes_searched = total_bytes_searched.load(Ordering::Relaxed);
        let skipped_binaries_cnt = total_skipped_binaries.load(Ordering::Relaxed);
        let elapsed = start_time.elapsed();

        if self.cli.json && !self.cli.quiet {
            let mut p = printer.lock().map_err(|e| format!("Lock error: {e}"))?;
            if let Err(err) = p.print_json_summary_with_skipped(
                matches_found,
                matched_lines_found,
                files_with_match_cnt,
                files_searched,
                bytes_searched,
                skipped_binaries_cnt,
                elapsed,
            ) {
                if err.kind() == io::ErrorKind::BrokenPipe {
                    return Ok(0);
                }
                return Err(err.to_string());
            }
        } else if self.cli.stats && !self.cli.quiet {
            let mut p = printer.lock().map_err(|e| format!("Lock error: {e}"))?;
            if let Err(err) = p.print_stats_summary_with_skipped(
                matches_found,
                matched_lines_found,
                files_with_match_cnt,
                files_searched,
                bytes_searched,
                skipped_binaries_cnt,
                elapsed,
            ) {
                if err.kind() == io::ErrorKind::BrokenPipe {
                    return Ok(0);
                }
                return Err(err.to_string());
            }
        }

        let exit_code = if had_error.load(Ordering::Relaxed) {
            2
        } else if query.is_discovery() {
            if matches_found > 0 { 0 } else { 1 }
        } else if self.cli.files_without_match {
            if let Some(cnt) = selected_files_without_match_cnt {
                if cnt > 0 { 0 } else { 1 }
            } else if files_with_match_cnt < files_searched {
                0
            } else {
                1
            }
        } else if matched_lines_found > 0 {
            0
        } else {
            1
        };

        if self.cli.journal || self.config.journal.enabled {
            let journal_path = self
                .config
                .journal
                .path
                .clone()
                .unwrap_or_else(Config::journal_file_path);
            let record = crate::journal::JournalRecord::new_with_bytes(
                self.cli.args.clone(),
                matches_found,
                files_searched,
                bytes_searched,
                elapsed,
                exit_code,
            );
            let _ = crate::journal::Journal::append(&journal_path, &record);
        }

        Ok(exit_code)
    }

    fn collect_patterns(&self) -> Result<Vec<String>, String> {
        let mut patterns = self.cli.regexp.clone();
        for file_path in &self.cli.file {
            let expanded = crate::config::Config::expand_tilde(file_path);
            let content = std::fs::read_to_string(&expanded).map_err(|e| {
                format!("Failed to read pattern file '{}': {e}", file_path.display())
            })?;
            for line in content.lines() {
                patterns.push(line.to_string());
            }
        }
        Ok(patterns)
    }

    fn pattern_syntax(&self) -> PatternSyntax {
        if self.cli.fixed_strings || (self.config.search.fixed_strings && !self.cli.extended_regexp)
        {
            PatternSyntax::Fixed
        } else if self.cli.extended_regexp {
            PatternSyntax::Regex
        } else {
            PatternSyntax::Dsl
        }
    }

    fn compile_ordinary_pattern(&self, term: &str) -> SearchPattern {
        match self.pattern_syntax() {
            PatternSyntax::Dsl => DslParser::compile_term_to_pattern(term, false),
            PatternSyntax::Fixed => SearchPattern::ExactLiteral(term.to_owned()),
            PatternSyntax::Regex => SearchPattern::Regex(term.to_owned()),
        }
    }

    fn build_expr_from_patterns(&self, patterns: &[String]) -> Result<Option<QueryExpr>, String> {
        if patterns.len() > 128 {
            return Err("Expression complexity exceeds the supported limit".into());
        }
        let mut expression = None;
        for pattern in patterns {
            let pattern = DslParser::parse_search_pattern_with_compiler(pattern, &|term, _| {
                self.compile_ordinary_pattern(term)
            })?;
            let atom = QueryExpr::Pattern(pattern);
            expression = Some(match expression {
                Some(previous) => QueryExpr::Or(Box::new(previous), Box::new(atom)),
                None => atom,
            });
        }
        Ok(expression)
    }

    fn build_dsl_query(&self) -> Result<Query, String> {
        if self.cli.file_type.len() + self.cli.file_type_not.len() + self.cli.glob.len() > 128 {
            return Err("Expression complexity exceeds the supported limit".into());
        }
        let has_explicit_pattern_flag = !self.cli.regexp.is_empty() || !self.cli.file.is_empty();
        let flag_patterns = self.collect_patterns()?;
        let empty_pattern_set = has_explicit_pattern_flag && flag_patterns.is_empty();
        let external_expr = if empty_pattern_set {
            Some(QueryExpr::Pattern(SearchPattern::EmptySet))
        } else {
            self.build_expr_from_patterns(&flag_patterns)?
        };
        let mut dsl_args = self.cli.args.clone();
        if let Some(ref fuzzy) = self.cli.fuzzy {
            dsl_args.insert(0, format!("fz:{fuzzy}"));
        }
        if self.cli.reverse {
            dsl_args.insert(0, "--reverse".into());
        }
        let mut base_query = Query {
            pattern_syntax: self.pattern_syntax(),
            ..Query::default()
        };
        if self.cli.ignore_case {
            assign_setting(&mut base_query.case_sensitive, false, "case sensitivity")?;
        }
        if self.cli.case_sensitive {
            assign_setting(&mut base_query.case_sensitive, true, "case sensitivity")?;
        }
        if let Some(depth) = self.cli.max_depth {
            assign_setting(&mut base_query.max_depth, depth, "depth")?;
        }
        if let Some(value) = self.cli.max_count {
            assign_setting(&mut base_query.max_count, value, "max")?;
        }
        if let Some(value) = self.cli.head {
            assign_setting(&mut base_query.head, value, "head")?;
        }
        if let Some(value) = self.cli.tail {
            assign_setting(&mut base_query.tail, value, "tail")?;
        }
        if let Some(value) = self.cli.nth {
            assign_setting(&mut base_query.nth, value, "nth")?;
        }
        if let Some(value) = self.cli.context {
            assign_setting(&mut base_query.context, value, "context")?;
        }
        if let Some(ref value) = self.cli.sort {
            assign_setting(
                &mut base_query.sort,
                crate::dsl::parse_sort_key(value, false)?,
                "sort",
            )?;
        }
        if self.cli.hidden || self.cli.unrestricted >= 2 {
            assign_setting(&mut base_query.search_hidden, true, "hidden entries")?;
        }
        if self.cli.no_ignore || self.cli.unrestricted >= 1 {
            assign_setting(&mut base_query.respect_ignore, false, "ignore rules")?;
        }
        if self.cli.follow {
            assign_setting(&mut base_query.follow_symlinks, true, "following links")?;
        }
        if self.cli.line_number {
            assign_setting(&mut base_query.line_number, true, "line numbers")?;
        }
        if self.cli.no_line_number {
            assign_setting(&mut base_query.line_number, false, "line numbers")?;
        }
        base_query.pick = self.cli.pick;
        if self.cli.text || self.cli.unrestricted >= 3 {
            base_query.include_binaries = true;
        }
        let type_atom =
            |value: &str| BoolExpr::Pattern(crate::dsl::EntryPredicate::Type(value.to_owned()));
        if let Some(types) = self
            .cli
            .file_type
            .iter()
            .map(|value| type_atom(value))
            .reduce(|a, b| BoolExpr::Or(Box::new(a), Box::new(b)))
        {
            base_query.constrain(types);
        }
        for value in &self.cli.file_type_not {
            base_query.constrain(BoolExpr::Not(Box::new(type_atom(value))));
        }
        let glob_atom =
            |value: &str| BoolExpr::Pattern(crate::dsl::EntryPredicate::Glob(value.into()));
        if let Some(globs) = self
            .cli
            .glob
            .iter()
            .filter(|value| !value.starts_with('!'))
            .map(|value| glob_atom(value))
            .reduce(|a, b| BoolExpr::Or(Box::new(a), Box::new(b)))
        {
            base_query.constrain(globs);
        }
        for value in &self.cli.glob {
            if let Some(negative) = value.strip_prefix('!') {
                base_query.constrain(BoolExpr::Not(Box::new(glob_atom(negative))));
            }
        }

        let mut query = DslParser::parse_with_compiler_base_and_config(
            &dsl_args,
            external_expr,
            base_query,
            Some(&self.config),
            |term, _| self.compile_ordinary_pattern(term),
        )?;
        query.empty_pattern_set = empty_pattern_set;
        if self.cli.smart_case && query.case_sensitive.is_some() {
            return Err(
                "Conflicting values for case sensitivity: smart case and explicit case mode".into(),
            );
        }
        if query.case_sensitive.is_none() && !self.cli.smart_case && !self.config.search.smart_case
        {
            query.case_sensitive = Some(true);
        }
        let effective_case = query.case_sensitive;
        let mut declared_before = query.context;
        let mut declared_after = query.context;
        if let Some(value) = self.cli.before_context {
            assign_setting(&mut declared_before, value, "before context")?;
        }
        if let Some(value) = self.cli.after_context {
            assign_setting(&mut declared_after, value, "after context")?;
        }

        for expr in [&mut query.expr, &mut query.name_expr]
            .into_iter()
            .flatten()
        {
            if let Some(cs) = effective_case {
                Self::set_case_sensitive(expr, cs);
            } else if self.cli.smart_case || self.config.search.smart_case {
                Self::set_smart_case(expr);
            }
        }
        if self.cli.invert_match {
            let target = if query.is_discovery() {
                &mut query.name_expr
            } else {
                &mut query.expr
            };
            if let Some(expr) = target.take() {
                *target = Some(QueryExpr::Not(Box::new(expr)));
            }
        }

        for target in &mut query.targets {
            *target = crate::config::Config::expand_tilde(&*target);
        }

        for expr in [&query.expr, &query.name_expr].into_iter().flatten() {
            if expr.any(&|pattern| match pattern {
                SearchPattern::Literal { text, .. }
                | SearchPattern::ExactLiteral(text)
                | SearchPattern::ExactMatch { text, .. }
                | SearchPattern::Fuzzy { text, .. } => text.contains(['\n', '\r']),
                _ => false,
            }) {
                return Err("Line separators are unsupported in literal patterns".into());
            }
            if (self.cli.word_regexp || self.cli.line_regexp)
                && expr.any(&|pattern| {
                    matches!(pattern, SearchPattern::Fuzzy { .. } | SearchPattern::Hex(_))
                })
            {
                return Err(
                    "Word and line constraints are unsupported for fuzzy and hex patterns".into(),
                );
            }
        }

        for expr in [&mut query.expr, &mut query.name_expr]
            .into_iter()
            .flatten()
        {
            self.apply_word_and_line_boundaries(expr);
        }

        if query.nth == Some(0) {
            return Err("nth index cannot be 0".into());
        }
        if query.head.is_some() && query.tail.is_some() {
            return Err("Cannot specify both 'head' and 'tail' limits simultaneously.".into());
        }
        if query.nth.is_some() && (query.head.is_some() || query.tail.is_some()) {
            return Err("Cannot specify 'nth' together with 'head' or 'tail' limits.".into());
        }
        if query.pick && query.nth.is_some() {
            return Err("Cannot specify both '--pick' and '--nth'.".into());
        }

        if let Some(ref dst) = self.cli.r#move {
            assign_setting(
                &mut query.action,
                crate::ops::ActionKind::Move(Config::expand_tilde(dst)),
                "file action",
            )?;
        }
        if let Some(ref dst) = self.cli.copy {
            assign_setting(
                &mut query.action,
                crate::ops::ActionKind::Copy(Config::expand_tilde(dst)),
                "file action",
            )?;
        }
        if self.cli.trash {
            assign_setting(
                &mut query.action,
                crate::ops::ActionKind::Trash,
                "file action",
            )?;
        }
        if let Some(ref ren) = self.cli.rename {
            let (pat, rep) = if let Some((p, r)) = ren.split_once('/') {
                (p.to_string(), r.to_string())
            } else if let Some((p, r)) = ren.split_once("->") {
                (p.to_string(), r.to_string())
            } else {
                return Err(format!(
                    "Invalid --rename specification '{ren}'. Expected 'old/new' or 'old->new'."
                ));
            };
            assign_setting(
                &mut query.action,
                crate::ops::ActionKind::Rename {
                    pattern: pat,
                    replacement: rep,
                },
                "file action",
            )?;
        }
        if let Some(ref mode) = self.cli.chmod {
            assign_setting(
                &mut query.action,
                crate::ops::ActionKind::Chmod(mode.clone()),
                "file action",
            )?;
        }

        Ok(query)
    }

    fn build_ignore_engine(&self, query: &Query) -> GitignoreEngine {
        let respect_ignore = query.respect_ignore.unwrap_or(
            !self.cli.no_ignore && !self.config.search.no_ignore && self.cli.unrestricted < 1,
        );
        let search_hidden = query
            .search_hidden
            .unwrap_or(self.cli.hidden || self.config.search.hidden || self.cli.unrestricted >= 2);

        let mut engine = GitignoreEngine::new(
            self.config.default_excludes.clone(),
            Vec::new(),
            Vec::new(),
            respect_ignore,
            search_hidden,
        );
        engine.load_global_ignore_files();
        engine
    }

    fn build_matcher(&self, query: &Query) -> Result<Box<dyn Matcher>, String> {
        let expr = match &query.expr {
            Some(e) => e,
            None => {
                return Ok(Box::new(crate::search::AllLinesMatcher));
            }
        };

        if !query.proximity_filters.is_empty() {
            return Ok(Box::new(
                BooleanEngine::try_new(expr.clone())
                    .map_err(|err| format!("Invalid regex: {err}"))?
                    .with_proximity_filters_case(&query.proximity_filters, query.case_sensitive),
            ));
        }

        match expr {
            QueryExpr::Pattern(SearchPattern::EmptySet) => {
                Ok(Box::new(crate::search::NeverMatcher))
            }
            QueryExpr::Pattern(pat @ SearchPattern::Literal { text, .. }) => Ok(Box::new(
                SimdLiteralMatcher::new(text, pat.is_case_sensitive()),
            )),
            QueryExpr::Pattern(SearchPattern::ExactLiteral(text)) => {
                Ok(Box::new(SimdLiteralMatcher::new(text, true)))
            }
            QueryExpr::Pattern(SearchPattern::Regex(pattern)) => {
                let re = RegexMatcher::new(pattern, self.cli.ignore_case)
                    .map_err(|e| format!("Invalid regex '{pattern}': {e}"))?;
                Ok(Box::new(re))
            }
            QueryExpr::Pattern(SearchPattern::Hex(bytes)) => {
                Ok(Box::new(HexMatcher::new(bytes.clone())))
            }
            complex_expr => Ok(Box::new(
                BooleanEngine::try_new(complex_expr.clone())
                    .map_err(|err| format!("Invalid regex: {err}"))?,
            )),
        }
    }

    fn set_case_sensitive(expr: &mut QueryExpr, case_sensitive_val: bool) {
        match expr {
            QueryExpr::Pattern(
                SearchPattern::Literal { case_sensitive, .. }
                | SearchPattern::ExactMatch { case_sensitive, .. }
                | SearchPattern::Fuzzy { case_sensitive, .. },
            ) => {
                *case_sensitive = Some(case_sensitive_val);
            }
            QueryExpr::Pattern(SearchPattern::ExactLiteral(text)) => {
                *expr = QueryExpr::Pattern(SearchPattern::Literal {
                    text: text.clone(),
                    case_sensitive: Some(case_sensitive_val),
                });
            }
            QueryExpr::Pattern(SearchPattern::Regex(pattern)) => {
                let body = pattern
                    .strip_prefix("(?i)")
                    .or_else(|| pattern.strip_prefix("(?-i)"))
                    .map(str::to_string)
                    .or_else(|| {
                        pattern
                            .strip_prefix("(?im)")
                            .map(|body| format!("(?m){body}"))
                    })
                    .unwrap_or_else(|| pattern.clone());
                *pattern = format!(
                    "{}{}",
                    if case_sensitive_val { "(?-i)" } else { "(?i)" },
                    body
                );
            }
            QueryExpr::And(a, b) | QueryExpr::Or(a, b) => {
                Self::set_case_sensitive(a, case_sensitive_val);
                Self::set_case_sensitive(b, case_sensitive_val);
            }
            QueryExpr::Not(a) => Self::set_case_sensitive(a, case_sensitive_val),
            _ => {}
        }
    }

    fn set_smart_case(expr: &mut QueryExpr) {
        match expr {
            QueryExpr::Pattern(
                SearchPattern::Literal { case_sensitive, .. }
                | SearchPattern::ExactMatch { case_sensitive, .. }
                | SearchPattern::Fuzzy { case_sensitive, .. },
            ) => {
                *case_sensitive = None;
            }
            QueryExpr::Pattern(SearchPattern::Regex(pattern)) => {
                let sensitive = pattern.chars().any(char::is_uppercase);
                Self::set_case_sensitive(expr, sensitive);
            }
            QueryExpr::And(a, b) | QueryExpr::Or(a, b) => {
                Self::set_smart_case(a);
                Self::set_smart_case(b);
            }
            QueryExpr::Not(a) => Self::set_smart_case(a),
            _ => {}
        }
    }

    fn apply_word_and_line_boundaries(&self, expr: &mut QueryExpr) {
        if !self.cli.word_regexp && !self.cli.line_regexp {
            return;
        }
        match expr {
            QueryExpr::Pattern(pattern) => {
                let mut body = match pattern {
                    SearchPattern::Literal { text, .. } => regex::escape(text),
                    SearchPattern::ExactLiteral(text) => regex::escape(text),
                    SearchPattern::Regex(pattern) => pattern.clone(),
                    SearchPattern::ExactMatch { .. }
                    | SearchPattern::Fuzzy { .. }
                    | SearchPattern::Hex(_)
                    | SearchPattern::EmptySet => return,
                };
                if !matches!(pattern, SearchPattern::Regex(_)) {
                    body = format!(
                        "(?{}:{body})",
                        if pattern.is_case_sensitive() {
                            "-i"
                        } else {
                            "i"
                        }
                    );
                }
                if self.cli.word_regexp {
                    body = format!(r"\b{{start-half}}(?:{body})\b{{end-half}}");
                }
                if self.cli.line_regexp {
                    body = format!(r"^(?:{body})$");
                }
                *pattern = SearchPattern::Regex(body);
            }
            QueryExpr::And(a, b) | QueryExpr::Or(a, b) => {
                self.apply_word_and_line_boundaries(a);
                self.apply_word_and_line_boundaries(b);
            }
            QueryExpr::Not(a) => self.apply_word_and_line_boundaries(a),
        }
    }
}

fn execute_command_per_match(cmd_template: &[String], paths: &[PathBuf]) -> Result<i32, String> {
    if cmd_template.is_empty() {
        return Ok(0);
    }
    if paths.is_empty() {
        return Ok(1);
    }
    let mut exit_code = 0;
    for path in paths {
        let path_str = path.to_string_lossy();
        let basename = path
            .file_name()
            .map(|f| f.to_string_lossy())
            .unwrap_or_default();
        let parent = path
            .parent()
            .map(|p| p.to_string_lossy())
            .unwrap_or_else(|| std::borrow::Cow::Borrowed("."));
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        let without_ext = if let Some(p) = path.parent() {
            if p.as_os_str().is_empty() {
                stem.into_owned()
            } else {
                format!("{}/{}", p.display(), stem)
            }
        } else {
            stem.into_owned()
        };

        let mut has_placeholder = false;
        let mut final_args: Vec<String> = Vec::new();
        for arg in &cmd_template[1..] {
            let mut s = arg.clone();
            if s.contains("{}") || s.contains("{/}") || s.contains("{//}") || s.contains("{.}") {
                has_placeholder = true;
                s = s
                    .replace("{}", &path_str)
                    .replace("{/}", &basename)
                    .replace("{//}", &parent)
                    .replace("{.}", &without_ext);
            }
            final_args.push(s);
        }
        if !has_placeholder {
            final_args.push(path_str.into_owned());
        }

        let status = std::process::Command::new(&cmd_template[0])
            .args(&final_args)
            .status()
            .map_err(|e| format!("Failed to execute '{}': {e}", cmd_template[0]))?;

        if !status.success() {
            exit_code = status.code().unwrap_or(1);
        }
    }
    Ok(exit_code)
}

fn execute_command_batch(cmd_template: &[String], paths: &[PathBuf]) -> Result<i32, String> {
    if cmd_template.is_empty() {
        return Ok(0);
    }
    if paths.is_empty() {
        return Ok(1);
    }
    let mut has_placeholder = false;
    let mut final_args: Vec<String> = Vec::new();
    for arg in &cmd_template[1..] {
        if arg.contains("{}") || arg.contains("{/}") || arg.contains("{//}") || arg.contains("{.}")
        {
            has_placeholder = true;
            for path in paths {
                let path_str = path.to_string_lossy();
                let basename = path
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();
                let parent = path
                    .parent()
                    .map(|p| p.to_string_lossy())
                    .unwrap_or_else(|| std::borrow::Cow::Borrowed("."));
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_default();
                let without_ext = if let Some(p) = path.parent() {
                    if p.as_os_str().is_empty() {
                        stem.into_owned()
                    } else {
                        format!("{}/{}", p.display(), stem)
                    }
                } else {
                    stem.into_owned()
                };

                final_args.push(
                    arg.replace("{}", &path_str)
                        .replace("{/}", &basename)
                        .replace("{//}", &parent)
                        .replace("{.}", &without_ext),
                );
            }
        } else {
            final_args.push(arg.clone());
        }
    }
    if !has_placeholder {
        for path in paths {
            final_args.push(path.to_string_lossy().into_owned());
        }
    }

    let status = std::process::Command::new(&cmd_template[0])
        .args(&final_args)
        .status()
        .map_err(|e| format!("Failed to execute '{}': {e}", cmd_template[0]))?;

    Ok(status.code().unwrap_or(0))
}

fn execute_file_action(
    action: &crate::ops::ActionKind,
    paths: &[PathBuf],
    dry_run: bool,
    force: bool,
    use_color: bool,
) -> Result<i32, String> {
    if paths.is_empty() {
        eprintln!("grx: no matching items found for action");
        return Ok(1);
    }

    let plan = crate::ops::plan_action(paths, action, force)?;

    if dry_run {
        print!("{}", plan.render_preview(use_color));
        if plan.has_conflicts() {
            eprintln!(
                "\ngrx: {} conflict(s) detected. Fix conflicts or rerun with --force.",
                plan.conflicts.len()
            );
            return Ok(2);
        }
        return Ok(0);
    }

    if plan.has_conflicts() && !force {
        eprint!("{}", plan.render_preview(use_color));
        eprintln!(
            "\ngrx: cannot execute action: {} conflict(s) detected. Fix conflicts or rerun with --force.",
            plan.conflicts.len()
        );
        return Ok(2);
    }

    match crate::ops::execute_plan(&plan) {
        Ok(tx) => {
            println!("grx: {} ({}, id: {})", tx.summary, plan.ops.len(), tx.id);
            Ok(0)
        }
        Err(err) => {
            eprintln!("grx: action failed: {err}");
            Ok(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn type_selector(value: &str) -> SelectorExpr {
        BoolExpr::Pattern(crate::dsl::EntryPredicate::Type(value.into()))
    }

    #[test]
    fn path_normalization_preserves_component_semantics() {
        for (input, expected) in [
            ("", ""),
            (".", ""),
            ("./", ""),
            ("./a", "a"),
            ("a/b", "a/b"),
            ("a/./b", "a/b"),
            ("a//b/", "a/b"),
            ("././a", "a"),
            ("a/../b", "a/../b"),
            ("../a", "../a"),
            ("/a//b/", "/a/b"),
        ] {
            assert_eq!(
                normalized_path_bytes(Path::new(input)).as_ref(),
                expected.as_bytes(),
                "{input}"
            );
        }
        #[cfg(unix)]
        {
            assert_eq!(normalized_path_bytes(Path::new("//a/b")).as_ref(), b"/a/b");
            assert_eq!(normalized_path_bytes(Path::new("a\\b")).as_ref(), b"a\\b");
        }
    }

    #[test]
    fn pattern_flags_keep_raw_syntax_and_case_in_compound_queries() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("input.txt");
        std::fs::write(&file, "A..B\n(foo OR bar)\n").unwrap();
        for options in [
            vec!["-F", "-i", "a..b"],
            vec!["-F", "(foo OR bar)"],
            vec!["-E", "-i", "(a[.][.]b|other)"],
            vec!["-F", "-i", "a..b", "AND", "lit:a..b"],
            vec!["-i", "-x", "re:a[.][.]b|other"],
            vec!["-i", "re:a[.][.]b"],
        ] {
            let mut args = vec!["grx", "-q"];
            args.extend(options);
            let root_arg = format!("root:{}", file.display());
            args.push(&root_arg);
            assert_eq!(
                Engine::new(Config::new_with_defaults(), Cli::parse_from(args))
                    .run()
                    .unwrap(),
                0
            );
        }
        let engine = Engine::new(
            Config::new_with_defaults(),
            Cli::parse_from(["grx", "-E", "a..b", "root:input.txt"]),
        );
        assert_eq!(
            engine.build_dsl_query().unwrap().expr,
            Some(QueryExpr::Pattern(SearchPattern::Regex("(?i)a..b".into())))
        );
    }

    #[test]
    fn reject_invalid_regex_before_searching_any_source() {
        for args in [
            vec!["grx", "re:[", "AND", "foo"],
            vec!["grx", "re:[", "near:foo"],
        ] {
            let mut engine = Engine::new(Config::new_with_defaults(), Cli::parse_from(args));
            assert!(engine.run().unwrap_err().contains("Invalid regex"));
        }
    }

    #[test]
    fn files_without_match_rejects_all_matching_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("sample.txt"), "needle\n").unwrap();
        let root_arg = format!("root:{}", dir.path().display());
        let cli = Cli::parse_from(["grx", "--hidden", "-q", "-L", "needle", &root_arg]);
        assert_eq!(
            Engine::new(Config::new_with_defaults(), cli).run().unwrap(),
            1
        );
    }

    #[test]
    fn test_engine_proximity_matcher_selection() {
        let config = Config::default();
        let cli = Cli::parse_from(["grx", "unsafe", "near:5,safety"]);
        let engine = Engine::new(config, cli);
        let query = engine.build_dsl_query().unwrap();

        assert_eq!(query.proximity_filters.len(), 1);
        let matcher = engine.build_matcher(&query).unwrap();

        let sample = b"line 1: safety guard\nline 2: comment\nline 3: unsafe block\nline 4: done\n";
        let mut sink = crate::search::VecMatchSink::default();
        let count = matcher.find_matches(sample, &mut sink).unwrap();
        assert_eq!(count, 1);
        assert_eq!(sink.matches.len(), 3);
        assert_eq!(sink.matches[0].line_number, 1);
        assert!(!sink.matches[0].near_spans.is_empty());
        assert_eq!(sink.matches[1].line_number, 2);
        assert!(sink.matches[1].is_context);
        assert_eq!(sink.matches[2].line_number, 3);
        assert!(!sink.matches[2].match_spans.is_empty());
    }

    #[test]
    fn test_query_entry_predicate_matches() {
        let config = Config::default();
        let reference = std::time::SystemTime::now();

        let file_query = Query {
            kind: Some(EntryKind::File),
            ..Query::default()
        };
        let pred_file = QueryEntryPredicate::new(&file_query, &config, reference).unwrap();
        let file_entry =
            crate::core::DirEntry::new(PathBuf::from("."), b"foo.rs".to_vec(), false, false, None);
        let dir_entry =
            crate::core::DirEntry::new(PathBuf::from("."), b"sub".to_vec(), true, false, None);
        let link_entry =
            crate::core::DirEntry::new(PathBuf::from("."), b"sym".to_vec(), false, true, None);
        assert!(pred_file.matches(&file_entry));
        assert!(!pred_file.matches(&dir_entry));
        assert!(!pred_file.matches(&link_entry));

        let dir_query = Query {
            kind: Some(EntryKind::Dir),
            ..Query::default()
        };
        let pred_dir = QueryEntryPredicate::new(&dir_query, &config, reference).unwrap();
        assert!(!pred_dir.matches(&file_entry));
        assert!(pred_dir.matches(&dir_entry));

        let link_query = Query {
            kind: Some(EntryKind::Link),
            ..Query::default()
        };
        let pred_link = QueryEntryPredicate::new(&link_query, &config, reference).unwrap();
        assert!(!pred_link.matches(&file_entry));
        assert!(pred_link.matches(&link_entry));

        let tmp = tempfile::tempdir().unwrap();
        let bin_file = tmp.path().join("test.bin");
        std::fs::write(&bin_file, b"\x7fELF\0data\0null").unwrap();
        let txt_file = tmp.path().join("test.txt");
        std::fs::write(&txt_file, b"plain text without nulls\n").unwrap();

        let bin_entry = crate::core::DirEntry::new(
            tmp.path().to_path_buf(),
            b"test.bin".to_vec(),
            false,
            false,
            None,
        );
        let txt_entry = crate::core::DirEntry::new(
            tmp.path().to_path_buf(),
            b"test.txt".to_vec(),
            false,
            false,
            None,
        );

        let bin_query = Query {
            kind: Some(EntryKind::Bin),
            ..Query::default()
        };
        let pred_bin = QueryEntryPredicate::new(&bin_query, &config, reference).unwrap();
        assert!(pred_bin.matches(&bin_entry));
        assert!(!pred_bin.matches(&txt_entry));
        assert!(!pred_bin.matches(&dir_entry));

        let txt_query = Query {
            kind: Some(EntryKind::Text),
            ..Query::default()
        };
        let pred_txt = QueryEntryPredicate::new(&txt_query, &config, reference).unwrap();
        assert!(!pred_txt.matches(&bin_entry));
        assert!(pred_txt.matches(&txt_entry));
        assert!(!pred_txt.matches(&dir_entry));

        let in_query = Query {
            name_expr: Some(QueryExpr::And(
                Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                    text: "report".into(),
                    case_sensitive: None,
                })),
                Box::new(QueryExpr::Not(Box::new(QueryExpr::Pattern(
                    SearchPattern::Literal {
                        text: "draft".into(),
                        case_sensitive: None,
                    },
                )))),
            )),
            ..Query::default()
        };
        let pred_in = QueryEntryPredicate::new(&in_query, &config, reference).unwrap();
        let match_entry = crate::core::DirEntry::new(
            PathBuf::from("."),
            b"final_report.txt".to_vec(),
            false,
            false,
            None,
        );
        let match_dir_entry = crate::core::DirEntry::new(
            PathBuf::from("."),
            b"annual_report".to_vec(),
            true,
            false,
            None,
        );
        let fail_entry = crate::core::DirEntry::new(
            PathBuf::from("."),
            b"final_summary.txt".to_vec(),
            false,
            false,
            None,
        );
        let excluded_entry = crate::core::DirEntry::new(
            PathBuf::from("."),
            b"draft_report.txt".to_vec(),
            false,
            false,
            None,
        );
        assert!(pred_in.matches(&match_entry));
        assert!(pred_in.matches(&match_dir_entry));
        assert!(!pred_in.matches(&fail_entry));
        assert!(!pred_in.matches(&excluded_entry));

        let size_query = Query {
            selector_expr: Some(BoolExpr::And(
                Box::new(BoolExpr::Pattern(crate::dsl::EntryPredicate::Size(
                    SizePredicate::Larger(50),
                ))),
                Box::new(BoolExpr::Pattern(crate::dsl::EntryPredicate::Size(
                    SizePredicate::Smaller(200),
                ))),
            )),
            ..Query::default()
        };
        let pred_size = QueryEntryPredicate::new(&size_query, &config, reference).unwrap();
        let entry_100 = crate::core::DirEntry::new(
            PathBuf::from("."),
            b"test.bin".to_vec(),
            false,
            false,
            Some(100),
        );
        let entry_20 = crate::core::DirEntry::new(
            PathBuf::from("."),
            b"small.bin".to_vec(),
            false,
            false,
            Some(20),
        );
        let entry_500 = crate::core::DirEntry::new(
            PathBuf::from("."),
            b"large.bin".to_vec(),
            false,
            false,
            Some(500),
        );
        assert!(pred_size.matches(&entry_100));
        assert!(!pred_size.matches(&entry_20));
        assert!(!pred_size.matches(&entry_500));
    }

    #[test]
    fn test_engine_discovery_usage_validation() {
        let config = Config::new_with_defaults();

        let cli = Cli::parse_from(["grx", "-e", "pattern", "kind:dir"]);
        let mut engine = Engine::new(config.clone(), cli);
        assert!(engine.run().is_err());

        let cli = Cli::parse_from(["grx", "-e", "pattern", "kind:link"]);
        let mut engine = Engine::new(config.clone(), cli);
        assert!(engine.run().is_err());

        let cli = Cli::parse_from(["grx", "kind:file", "report", "-"]);
        let mut engine = Engine::new(config.clone(), cli);
        assert_eq!(engine.run().unwrap(), 2);

        let cli = Cli::parse_from(["grx", "kind:dir", "larger:10MiB"]);
        let mut engine = Engine::new(config.clone(), cli);
        assert!(engine.run().is_err());

        let cli = Cli::parse_from(["grx", "kind:file", "report", "ctx:2"]);
        let mut engine = Engine::new(config.clone(), cli);
        assert!(engine.run().is_err());

        let cli = Cli::parse_from(["grx"]);
        let mut engine = Engine::new(config.clone(), cli);
        assert_eq!(engine.run().unwrap(), 2);

        let cli_ht = Cli::parse_from(["grx", "kind:file", "report", "--head", "5", "--tail", "10"]);
        let mut engine_ht = Engine::new(config.clone(), cli_ht);
        assert!(engine_ht.run().is_err());

        let cli_ht_dsl = Cli::parse_from(["grx", "kind:file", "report", "head:5", "tail:10"]);
        let mut engine_ht_dsl = Engine::new(config, cli_ht_dsl);
        assert!(engine_ht_dsl.run().is_err());
    }

    #[test]
    fn test_engine_cli_case_flags_reach_predicates() {
        let config = Config::new_with_defaults();
        let reference = std::time::SystemTime::now();

        let report_lower = crate::core::DirEntry::new(
            PathBuf::from("."),
            b"report.txt".to_vec(),
            false,
            false,
            None,
        );
        let report_upper = crate::core::DirEntry::new(
            PathBuf::from("."),
            b"REPORT.txt".to_vec(),
            false,
            false,
            None,
        );

        let cli_i = Cli::parse_from(["grx", "-i", "REPORT", "kind:file"]);
        let engine_i = Engine::new(config.clone(), cli_i);
        let q_i = engine_i.build_dsl_query().unwrap();
        assert_eq!(q_i.case_sensitive, Some(false));
        let pred_i = QueryEntryPredicate::new(&q_i, &config, reference).unwrap();
        assert!(pred_i.matches(&report_lower));
        assert!(pred_i.matches(&report_upper));

        let cli_s = Cli::parse_from(["grx", "-s", "report", "kind:file"]);
        let engine_s = Engine::new(config.clone(), cli_s);
        let q_s = engine_s.build_dsl_query().unwrap();
        assert_eq!(q_s.case_sensitive, Some(true));
        let pred_s = QueryEntryPredicate::new(&q_s, &config, reference).unwrap();
        assert!(pred_s.matches(&report_lower));
        assert!(!pred_s.matches(&report_upper));

        let cli_smart_upper = Cli::parse_from(["grx", "REPORT", "kind:file"]);
        let engine_smart_upper = Engine::new(config.clone(), cli_smart_upper);
        let q_smart_upper = engine_smart_upper.build_dsl_query().unwrap();
        assert_eq!(q_smart_upper.case_sensitive, None);
        let pred_smart_upper =
            QueryEntryPredicate::new(&q_smart_upper, &config, reference).unwrap();
        assert!(!pred_smart_upper.matches(&report_lower));
        assert!(pred_smart_upper.matches(&report_upper));

        let cli_smart_lower = Cli::parse_from(["grx", "report", "kind:file"]);
        let engine_smart_lower = Engine::new(config.clone(), cli_smart_lower);
        let q_smart_lower = engine_smart_lower.build_dsl_query().unwrap();
        assert_eq!(q_smart_lower.case_sensitive, None);
        let pred_smart_lower =
            QueryEntryPredicate::new(&q_smart_lower, &config, reference).unwrap();
        assert!(pred_smart_lower.matches(&report_lower));
        assert!(pred_smart_lower.matches(&report_upper));
    }

    #[test]
    fn test_empty_pattern_file_preserves_positional_targets_and_inversion() {
        let tmp = tempfile::tempdir().unwrap();
        let pat_path = tmp.path().join("empty.pat");
        let target_path = tmp.path().join("yes.txt");
        std::fs::write(&pat_path, b"").unwrap();
        std::fs::write(&target_path, b"hit\n").unwrap();

        let config = Config::default();
        let root_arg = format!("root:{}", target_path.display());

        let cli = Cli::parse_from(["grx", "-f", pat_path.to_str().unwrap(), &root_arg]);
        let engine = Engine::new(config.clone(), cli);
        let q = engine.build_dsl_query().unwrap();
        assert_eq!(q.targets, vec![target_path.clone()]);
        assert_eq!(q.expr, Some(QueryExpr::Pattern(SearchPattern::EmptySet)));
        let matcher = engine.build_matcher(&q).unwrap();
        let mut sink = crate::search::VecMatchSink::default();
        let count = matcher.find_matches(b"hit\n", &mut sink).unwrap();
        assert_eq!(count, 0);

        let cli_v = Cli::parse_from(["grx", "-v", "-f", pat_path.to_str().unwrap(), &root_arg]);
        let engine_v = Engine::new(config, cli_v);
        let q_v = engine_v.build_dsl_query().unwrap();
        assert_eq!(q_v.targets, vec![target_path]);
        assert_eq!(
            q_v.expr,
            Some(QueryExpr::Not(Box::new(QueryExpr::Pattern(
                SearchPattern::EmptySet
            ))))
        );
        let matcher_v = engine_v.build_matcher(&q_v).unwrap();
        let mut sink_v = crate::search::VecMatchSink::default();
        let count_v = matcher_v.find_matches(b"hit\n", &mut sink_v).unwrap();
        assert_eq!(count_v, 1);
        assert_eq!(sink_v.matches.len(), 1);
        assert_eq!(sink_v.matches[0].line_bytes, b"hit");
    }

    #[test]
    fn test_query_entry_predicate_type_excludes_preserves_directories() {
        let config = Config::default();
        let reference = std::time::SystemTime::now();

        let mut query = Query::default();
        query.constrain(BoolExpr::Not(Box::new(type_selector("c"))));
        query.constrain(BoolExpr::Not(Box::new(type_selector("rs"))));

        let pred = QueryEntryPredicate::new(&query, &config, reference).unwrap();

        let file_c = crate::core::DirEntry::new(
            PathBuf::from("src"),
            b"main.c".to_vec(),
            false,
            false,
            None,
        );
        assert!(!pred.matches(&file_c));

        let dir_c =
            crate::core::DirEntry::new(PathBuf::from("src"), b"test.c".to_vec(), true, false, None);
        assert!(pred.matches(&dir_c));
        let dir_rs =
            crate::core::DirEntry::new(PathBuf::from("src"), b"rs".to_vec(), true, false, None);
        assert!(pred.matches(&dir_rs));
    }

    #[test]
    fn test_engine_undo_listing_and_dispatch() {
        let _lock = crate::ops::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let old_xdg = std::env::var("XDG_DATA_HOME").ok();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path());
        }

        let config = Config::default();
        let cli = Cli::parse_from(["grx", "undo", "--list"]);
        let mut engine = Engine::new(config.clone(), cli);
        assert_eq!(engine.run().unwrap(), 0);

        let cli_dry = Cli::parse_from(["grx", "undo", "--dry-run"]);
        let mut engine_dry = Engine::new(config.clone(), cli_dry);
        assert_eq!(engine_dry.run().unwrap(), 2);

        unsafe {
            if let Some(val) = old_xdg {
                std::env::set_var("XDG_DATA_HOME", val);
            } else {
                std::env::remove_var("XDG_DATA_HOME");
            }
        }
    }

    #[test]
    fn modal_entry_selection_combines_independent_names_and_boolean_selectors() {
        let config = Config::default();
        let query = DslParser::parse([
            "kind:file",
            "(t:rs OR t:py)",
            "memory",
            "p:codex/",
            "in:needle",
        ])
        .unwrap();
        let predicate =
            QueryEntryPredicate::new(&query, &config, std::time::SystemTime::now()).unwrap();
        let entry = |parent, name: &[u8]| {
            crate::core::DirEntry::new(PathBuf::from(parent), name.to_vec(), false, false, None)
        };
        assert!(predicate.matches(&entry("nested/codex", b"memory.rs")));
        assert!(predicate.matches(&entry("nested/codex", b"memory.py")));
        assert!(!predicate.matches(&entry("nested/codex", b"other.rs")));
        assert!(!predicate.matches(&entry("nested/codex", b"memory.txt")));
        assert!(!predicate.matches(&entry("nested/codexish", b"memory.rs")));
        assert!(query.expr.is_some());
    }

    #[test]
    fn path_predicates_share_directory_boundaries_and_case_policy() {
        let cwd = std::env::current_dir().unwrap();
        let entry = |parent, name: &[u8], dir| {
            crate::core::DirEntry::new(PathBuf::from(parent), name.to_vec(), dir, false, None)
        };
        let contains = CompiledPathPredicate::new("codex", None, &cwd, &[]).unwrap();
        let directory = CompiledPathPredicate::new("codex/", None, &cwd, &[]).unwrap();
        assert!(contains.matches(&entry("nested/.codex", b"memory", false), &cwd));
        assert!(!directory.matches(&entry("nested/.codex", b"memory", false), &cwd));
        assert!(directory.matches(&entry("nested/codex/deeper", b"memory", false), &cwd));
        assert!(directory.matches(&entry("nested", b"codex", true), &cwd));
        assert!(!directory.matches(&entry("nested", b"codex", false), &cwd));
        assert!(!directory.matches(&entry("nested/codexish", b"memory", false), &cwd));
        let insensitive = CompiledPathPredicate::new("CODEX/", Some(false), &cwd, &[]).unwrap();
        assert!(insensitive.matches(&entry("nested/codex", b"memory", false), &cwd));
        let sequence = CompiledPathPredicate::new("foo/codex/", None, &cwd, &[]).unwrap();
        assert!(sequence.matches(&entry("nested/foo/codex/deep", b"memory", false), &cwd));
        assert!(!sequence.matches(&entry("nested/notfoo/codex", b"memory", false), &cwd));
    }

    #[test]
    fn path_patterns_use_shared_matching_and_lexical_exact_paths() {
        let cwd = std::env::current_dir().unwrap();
        let entry = crate::core::DirEntry::new(
            PathBuf::from("./src"),
            b"engine.rs".to_vec(),
            false,
            false,
            None,
        );
        for raw in [
            "^src/",
            "engine.rs$",
            "src..engine",
            "re:engine[.]rs$",
            "=src/engine.rs",
            "fz:srceng",
        ] {
            assert!(
                CompiledPathPredicate::new(raw, None, &cwd, &[])
                    .unwrap()
                    .matches(&entry, &cwd),
                "{raw}"
            );
        }
        assert!(
            !CompiledPathPredicate::new("=engine.rs", None, &cwd, &[])
                .unwrap()
                .matches(&entry, &cwd)
        );
        assert!(CompiledPathPredicate::new("re:[", None, &cwd, &[]).is_err());
        assert!(CompiledPathPredicate::new("re:[/", None, &cwd, &[]).is_err());
        let absolute = format!("{}/src/", cwd.display());
        assert!(
            CompiledPathPredicate::new(&absolute, None, &cwd, &[])
                .unwrap()
                .matches(&entry, &cwd)
        );
    }

    #[test]
    fn path_exclusion_keeps_selector_boolean_structure() {
        let query = DslParser::parse(["kind:file", "np:vendor/", "OR", "p:vendor/"]).unwrap();
        let predicate =
            QueryEntryPredicate::new(&query, &Config::default(), std::time::SystemTime::now())
                .unwrap();
        let entry = crate::core::DirEntry::new(
            PathBuf::from("vendor"),
            b"foo.rs".to_vec(),
            false,
            false,
            None,
        );
        assert!(predicate.matches(&entry));
        let mut pruned = Vec::new();
        directory_exclusions(predicate.selector.as_ref().unwrap(), &mut pruned);
        assert!(pruned.is_empty(), "an OR exclusion is not mandatory");

        let mut excluded = DslParser::parse(["kind:file", "np:VENDOR/"]).unwrap();
        excluded.case_sensitive = Some(false);
        assert!(
            !QueryEntryPredicate::new(&excluded, &Config::default(), std::time::SystemTime::now())
                .unwrap()
                .matches(&entry)
        );
        let compulsory =
            QueryEntryPredicate::new(&excluded, &Config::default(), std::time::SystemTime::now())
                .unwrap();
        directory_exclusions(compulsory.selector.as_ref().unwrap(), &mut pruned);
        assert_eq!(pruned.len(), 1);
        let anchored = DslParser::parse(["kind:file", "np:vendor$"]).unwrap();
        let anchored =
            QueryEntryPredicate::new(&anchored, &Config::default(), std::time::SystemTime::now())
                .unwrap();
        pruned.clear();
        directory_exclusions(anchored.selector.as_ref().unwrap(), &mut pruned);
        assert!(
            pruned.is_empty(),
            "end-anchored exclusions are not hereditary"
        );
    }

    #[test]
    fn exact_paths_resolve_relative_to_each_search_root() {
        let query = DslParser::parse(["root:src", "kind:file", "p:=engine.rs"]).unwrap();
        let predicate =
            QueryEntryPredicate::new(&query, &Config::default(), std::time::SystemTime::now())
                .unwrap();
        let entry = crate::core::DirEntry::new(
            PathBuf::from("src"),
            b"engine.rs".to_vec(),
            false,
            false,
            None,
        );
        assert!(predicate.matches(&entry));
        let query =
            DslParser::parse(["root:src", "root:src/dsl", "kind:file", "p:=mod.rs"]).unwrap();
        let predicate =
            QueryEntryPredicate::new(&query, &Config::default(), std::time::SystemTime::now())
                .unwrap();
        let entry = crate::core::DirEntry::new(
            PathBuf::from("src/dsl"),
            b"mod.rs".to_vec(),
            false,
            false,
            None,
        );
        assert!(predicate.matches(&entry));
    }

    #[test]
    fn directory_case_matching_supports_unicode() {
        let cwd = std::env::current_dir().unwrap();
        let matcher = CompiledPathPredicate::new("École/", Some(false), &cwd, &[]).unwrap();
        let entry = crate::core::DirEntry::new(
            PathBuf::from("école"),
            b"notes".to_vec(),
            false,
            false,
            None,
        );
        assert!(matcher.matches(&entry, &cwd));
    }

    #[test]
    fn result_indices_handle_minimum_signed_index_without_overflow() {
        assert_eq!(result_index(isize::MIN, 3), None);
        assert_eq!(result_index(-1, 3), Some(2));
        assert_eq!(result_index(1, 3), Some(0));
        assert_eq!(result_index(0, 3), None);
        assert_eq!(result_index(4, 3), None);
    }

    #[test]
    #[cfg(unix)]
    fn default_discovery_emits_files_and_valid_file_links_only() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("codex")).unwrap();
        std::os::unix::fs::symlink("absent", temp.path().join("codex-broken")).unwrap();
        let root = format!("root:{}", temp.path().display());
        let query = Cli::parse_from(["grx", "-q", "--hidden", &root, "p:codex"]);
        assert_eq!(Engine::new(Config::default(), query).run().unwrap(), 1);
        std::fs::write(temp.path().join("target.txt"), "contents").unwrap();
        std::os::unix::fs::symlink("target.txt", temp.path().join("codex-link")).unwrap();
        let query = Cli::parse_from(["grx", "-q", "--hidden", &root, "p:codex", "d:0"]);
        assert_eq!(Engine::new(Config::default(), query).run().unwrap(), 0);
    }

    #[test]
    fn cli_and_dsl_scalar_declarations_must_agree() {
        for (options, label) in [
            (vec!["-i", "-s"], "case"),
            (vec!["-s", "-i"], "case"),
            (vec!["-S", "-s"], "case"),
            (vec!["-d", "1", "d:2"], "depth"),
            (vec!["-m", "1", "max:2"], "max"),
            (vec!["--head", "1", "head:2"], "head"),
            (vec!["--tail", "1", "tail:2"], "tail"),
            (vec!["--nth", "1", "nth:2"], "nth"),
            (vec!["-C", "1", "ctx:2"], "context"),
            (vec!["-B", "1", "ctx:2"], "context"),
            (vec!["--sort", "path", "sort:size"], "sort"),
            (vec!["-n", "-N"], "line"),
        ] {
            let mut args = vec!["grx"];
            args.extend(options);
            args.push("needle");
            let error = Engine::new(Config::default(), Cli::parse_from(args))
                .build_dsl_query()
                .unwrap_err();
            assert!(
                error.contains("Conflicting") && error.contains(label),
                "{error}"
            );
        }
        let cli = Cli::parse_from([
            "grx", "-i", "-d", "2", "d:2", "--head", "1", "head:1", "needle",
        ]);
        let query = Engine::new(Config::default(), cli)
            .build_dsl_query()
            .unwrap();
        assert_eq!(query.case_sensitive, Some(false));
        assert_eq!(query.max_depth, Some(2));
        assert_eq!(query.head, Some(1));
    }

    #[test]
    fn content_commands_require_explicit_file_selection() {
        let cli = Cli::parse_from(["grx", "needle", "--exec", "echo", "{}"]);
        assert_eq!(Engine::new(Config::default(), cli).run().unwrap(), 2);
    }

    #[test]
    fn mixed_proximity_windows_preserve_context_owners_during_selection() {
        let cli = Cli::parse_from([
            "grx",
            "needle",
            "near:1,SMALL",
            "near:20,LARGE",
            "no-near:100,FORBIDDEN",
        ]);
        let engine = Engine::new(Config::default(), cli);
        let query = engine.build_dsl_query().unwrap();
        let matcher = engine.build_matcher(&query).unwrap();
        let bytes =
            b"SMALL first\nneedle first\nLARGE shared\nblank\nSMALL second\nneedle second\n";
        let selected = std::collections::BTreeSet::from([2]);
        for after_context in [0, 4] {
            let search = BufferSearch {
                matcher: matcher.as_ref(),
                options: SearchOptions {
                    has_proximity: true,
                    after_context,
                    ..SearchOptions::default()
                },
            };
            let mut result = search.search(bytes).unwrap().unwrap();
            result
                .records
                .retain_mut(|record| record.retain_for_primary(&selected));
            if after_context == 0 {
                assert_eq!(
                    result
                        .records
                        .iter()
                        .map(|record| record.line_number)
                        .collect::<Vec<_>>(),
                    vec![1, 2, 3]
                );
            } else {
                let ordinary = result
                    .records
                    .iter()
                    .find(|record| record.line_number == 5)
                    .unwrap();
                assert!(ordinary.is_context);
                assert!(
                    ordinary.near_spans.is_empty(),
                    "an omitted primary's neighbor must lose its highlighting"
                );
                assert_eq!(ordinary.occurrence_count, 0);
            }
        }
    }

    #[test]
    fn explicit_pattern_flags_share_matching_operators_with_in() {
        for (pattern, accepted, rejected) in [
            ("=codex", b"codex\n".as_slice(), b"codex extra\n".as_slice()),
            ("fz:mmry", b"memory\n".as_slice(), b"unrelated\n".as_slice()),
            (
                "^codex",
                b"codex extra\n".as_slice(),
                b"extra codex\n".as_slice(),
            ),
        ] {
            let cli = Cli::parse_from(["grx", "-e", pattern]);
            let engine = Engine::new(Config::default(), cli);
            let query = engine.build_dsl_query().unwrap();
            let from_in = DslParser::parse([format!("in:{pattern}")]).unwrap();
            let in_matcher = engine.build_matcher(&from_in).unwrap();
            let in_search = BufferSearch {
                matcher: in_matcher.as_ref(),
                options: SearchOptions::default(),
            };
            assert_eq!(
                in_search.search(accepted).unwrap().unwrap().matched_lines,
                1
            );
            assert_eq!(
                in_search.search(rejected).unwrap().unwrap().matched_lines,
                0
            );
            let matcher = engine.build_matcher(&query).unwrap();
            let search = BufferSearch {
                matcher: matcher.as_ref(),
                options: SearchOptions::default(),
            };
            assert_eq!(search.search(accepted).unwrap().unwrap().matched_lines, 1);
            assert_eq!(search.search(rejected).unwrap().unwrap().matched_lines, 0);
        }
        let mut patterns = NamedTempFile::new().unwrap();
        writeln!(patterns, "=codex").unwrap();
        let cli = Cli::parse_from(["grx", "-f", patterns.path().to_str().unwrap()]);
        let query = Engine::new(Config::default(), cli)
            .build_dsl_query()
            .unwrap();
        assert!(matches!(
            query.expr,
            Some(QueryExpr::Pattern(SearchPattern::ExactMatch { .. }))
        ));
    }

    #[test]
    fn record_level_sorts_reject_context_and_proximity() {
        for sort in ["sort:len", "sort:-len", "sort:line", "sort:-line"] {
            for context in ["ctx:1", "near:1,neighbor", "no-near:1,neighbor"] {
                let cli = Cli::parse_from(["grx", "needle", sort, context]);
                let error = Engine::new(Config::default(), cli).run().unwrap_err();
                assert!(error.contains("context or proximity"), "{error}");
            }
        }
    }

    #[test]
    fn plain_absolute_path_scopes_anchor_at_the_start() {
        let cwd = std::env::current_dir().unwrap();
        let matcher = CompiledPathPredicate::new("/scope", None, &cwd, &[]).unwrap();
        let entry = |parent| {
            crate::core::DirEntry::new(PathBuf::from(parent), b"file".to_vec(), false, false, None)
        };
        assert!(matcher.matches(&entry("/scope/nested"), &cwd));
        assert!(!matcher.matches(&entry("/other/scope/nested"), &cwd));
        let explicit_regex = CompiledPathPredicate::new("re:/scope", None, &cwd, &[]).unwrap();
        assert!(explicit_regex.matches(&entry("/other/scope/nested"), &cwd));
    }
}
