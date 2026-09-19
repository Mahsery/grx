use crate::cli::Cli;
use crate::config::{ColorChoice, Config, SearchMode};
use crate::core::{ContentReader, DirectoryWalker, EntryPredicate, Matcher};
use crate::dsl::{
    BasenameFilter, DslParser, EntryKind, Query, QueryExpr, SearchPattern, SizePredicate,
    TimePredicate,
};
use crate::execution::{BufferSearch, SearchOptions};
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

/// Helper to probe if a candidate file contains binary content based on null byte heuristics.
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

/// Predicate evaluator that determines whether a candidate filesystem entry
/// satisfies kind, filename, filetype, size, and modification age constraints.
#[derive(Debug, Clone)]
pub struct QueryEntryPredicate {
    pub kind: Option<EntryKind>,
    pub binary_null_probe_bytes: usize,
    pub case_sensitive: Option<bool>,
    pub basename_filters: Vec<BasenameFilter>,
    pub basename_exclude_filters: Vec<BasenameFilter>,
    pub type_includes: Vec<String>,
    pub type_excludes: Vec<String>,
    pub size_predicates: Vec<SizePredicate>,
    pub time_predicates: Vec<TimePredicate>,
    pub reference_time: std::time::SystemTime,
}

impl QueryEntryPredicate {
    pub fn new(query: &Query, config: &Config, reference_time: std::time::SystemTime) -> Self {
        let mut type_includes = Vec::new();
        for inc in &query.type_includes {
            if let Some(patterns) = config.types.get(inc) {
                type_includes.extend(patterns.clone());
            } else {
                type_includes.push(inc.clone());
            }
        }

        let mut type_excludes = Vec::new();
        for exc in &query.type_excludes {
            if let Some(patterns) = config.types.get(exc) {
                type_excludes.extend(patterns.clone());
            } else {
                type_excludes.push(exc.clone());
            }
        }

        Self {
            kind: query.kind,
            binary_null_probe_bytes: config.search.binary_null_probe_bytes,
            case_sensitive: query.case_sensitive,
            basename_filters: query.basename_filters.clone(),
            basename_exclude_filters: query.basename_exclude_filters.clone(),
            type_includes,
            type_excludes,
            size_predicates: query.size_predicates.clone(),
            time_predicates: query.time_predicates.clone(),
            reference_time,
        }
    }

    pub fn matches(&self, entry: &crate::core::DirEntry) -> bool {
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

        for filter in &self.basename_filters {
            if !filter.matches(&entry.name, self.case_sensitive) {
                return false;
            }
        }

        for filter in &self.basename_exclude_filters {
            if filter.matches(&entry.name, self.case_sensitive) {
                return false;
            }
        }

        if !self.type_includes.is_empty() {
            if entry.is_dir && self.kind != Some(EntryKind::Dir) {
                return false;
            }
            if !GitignoreEngine::matches_ext_list_bytes(&entry.name, &self.type_includes) {
                return false;
            }
        }

        if !self.type_excludes.is_empty()
            && !entry.is_dir
            && GitignoreEngine::matches_ext_list_bytes(&entry.name, &self.type_excludes)
        {
            return false;
        }

        if !self.size_predicates.is_empty() {
            if !entry.is_file() {
                return false;
            }
            let size = match entry.size {
                Some(s) => s,
                None => {
                    let path = entry.full_path();
                    match std::fs::metadata(&path) {
                        Ok(meta) => meta.len(),
                        Err(_) => return false,
                    }
                }
            };
            for pred in &self.size_predicates {
                if !pred.matches(size) {
                    return false;
                }
            }
        }

        if !self.time_predicates.is_empty() {
            let path = entry.full_path();
            let mtime = if entry.is_symlink {
                match std::fs::symlink_metadata(&path).and_then(|m| m.modified()) {
                    Ok(t) => t,
                    Err(_) => return false,
                }
            } else {
                match std::fs::metadata(&path).and_then(|m| m.modified()) {
                    Ok(t) => t,
                    Err(_) => return false,
                }
            };
            for pred in &self.time_predicates {
                if !pred.matches(mtime, self.reference_time) {
                    return false;
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

/// The central search coordinator executing traversal, reading, matching, and formatting.
pub struct Engine {
    config: Config,
    cli: Cli,
}

impl Engine {
    pub fn new(config: Config, cli: Cli) -> Self {
        Self { config, cli }
    }

    /// Execute the complete search workflow and return the standard POSIX exit code:
    /// 0 = matches found, 1 = no matches found, 2 = error.
    pub fn run(&mut self) -> Result<i32, String> {
        // Handle --tutorial
        if self.cli.tutorial {
            crate::cli::print_tutorial_with_choice(self.cli.color);
            return Ok(0);
        }

        // Handle --help-full, --help-all, or --help --all
        if self.cli.help_full || (self.cli.help && self.cli.all) {
            crate::cli::print_full_help();
            return Ok(0);
        }

        // Handle concise -h / --help
        if self.cli.help {
            crate::cli::print_concise_help();
            return Ok(0);
        }

        // Handle --dump-config immediately
        if self.cli.dump_config {
            print!("{}", Config::generate_commented_toml());
            return Ok(0);
        }

        // Handle --init-config
        if self.cli.init_config {
            match Config::init_default_config(self.cli.force) {
                Ok(path) => {
                    println!("grx: initialized configuration at {}", path.display());
                    return Ok(0);
                }
                Err(err) => return Err(err.to_string()),
            }
        }

        // Handle --config-path
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

        // Handle --paths
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

        // Handle --completions <SHELL>
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

        // Handle --install-completions [SHELL]
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

        // Handle `grx clean-trash` or `grx --clean-trash` or `grx undo --clean-trash`
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

        // Handle `grx undo` and transaction history listing
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

        // Handle bare invocation without arguments or filters
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

        // Parse query according to operational mode
        let query = match mode {
            SearchMode::Dsl => self.build_dsl_query()?,
            SearchMode::Grep | SearchMode::GitGrep => {
                return Err(
                    "compatibility modes (grep, git-grep) are dispatched via CLI wrapper; direct in-engine execution is not supported".into(),
                );
            }
        };

        // If discovery query has neither content pattern nor any entry selector or modifier (e.g. only modifiers like `grx ctx:2`), show help and exit 2
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

        // Validate discovery vs content search constraints
        if !query.is_discovery() {
            if query.kind == Some(crate::dsl::EntryKind::Dir)
                || query.kind == Some(crate::dsl::EntryKind::Link)
            {
                eprintln!("grx: content search cannot be combined with kind:dir or kind:link");
                return Ok(2);
            }
            if query.action.is_some() && !self.cli.files_with_matches {
                eprintln!(
                    "grx: file actions (mv:, cp:, rm:, trash:) cannot be run directly on content search matches without -l / --files-with-matches to explicitly confirm whole-file selection."
                );
                return Ok(2);
            }
        } else {
            // Discovery mode validations
            if query.targets.iter().any(|t| t == Path::new("-")) {
                eprintln!(
                    "grx: stdin target '-' is not supported in discovery mode: stdin is not a directory inventory"
                );
                return Ok(2);
            }
            if !query.size_predicates.is_empty()
                && (query.kind == Some(crate::dsl::EntryKind::Dir)
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
            if self.cli.line_number {
                eprintln!("grx: -n / --line-number is only supported for content search");
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

        // Build ignore and traversal engine
        let ignore_engine = self.build_ignore_engine(&query);
        let max_depth = self
            .cli
            .max_depth
            .or(query.max_depth)
            .or(self.config.search.max_depth);
        let is_dir_discovery = (query.kind == Some(crate::dsl::EntryKind::Dir)
            || (query.is_discovery() && query.kind.is_none()))
            && query.type_includes.is_empty();
        let is_link_discovery = query.kind == Some(crate::dsl::EntryKind::Link)
            || (query.is_discovery() && query.kind.is_none());
        let is_file_discovery = query.kind == Some(crate::dsl::EntryKind::File)
            || query.kind == Some(crate::dsl::EntryKind::Bin)
            || query.kind == Some(crate::dsl::EntryKind::Text)
            || !query.is_discovery()
            || (query.is_discovery() && query.kind.is_none());
        let walker = ParallelWalker::new(
            self.cli.threads.unwrap_or(self.config.search.threads),
            max_depth,
            self.cli.follow
                || query.follow_symlinks.unwrap_or(false)
                || self.config.search.follow_symlinks,
        )
        .with_buffer_size(self.config.search.walker_buffer_size_bytes)
        .with_emit_dirs(is_dir_discovery)
        .with_emit_links(is_link_discovery)
        .with_emit_files(is_file_discovery)
        .with_regular_files_only(
            query.kind == Some(crate::dsl::EntryKind::File)
                || query.kind == Some(crate::dsl::EntryKind::Bin)
                || query.kind == Some(crate::dsl::EntryKind::Text)
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
        let show_line_numbers = if self.cli.no_line_number {
            false
        } else if self.cli.line_number || self.cli.pretty || show_column {
            true
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

        // Build output printer
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
            && (query.targets.iter().any(|target| target == Path::new("-"))
                || (!io::stdin().is_terminal()
                    && query.targets.is_empty()
                    && !query_has_entry_selector(&query)));

        let roots: Vec<PathBuf> = if query.targets.is_empty() && !use_stdin {
            vec![PathBuf::from(".")]
        } else {
            query
                .targets
                .iter()
                .filter(|path| *path != Path::new("-"))
                .cloned()
                .collect()
        };
        let suppress_traversal_errors = (self.config.search.suppress_errors
            || self.cli.no_messages)
            && !self.cli.no_ignore_messages;
        let no_messages = self.cli.no_messages;
        let had_error = AtomicBool::new(false);
        for root in &roots {
            if root.symlink_metadata().is_err() && !no_messages {
                eprintln!("{}: No such file or directory (os error 2)", root.display());
                had_error.store(true, Ordering::Relaxed);
            }
        }

        let query_predicate = Arc::new(QueryEntryPredicate::new(
            &query,
            &self.config,
            std::time::SystemTime::now(),
        ));
        let null_output = self.cli.null_output || self.config.output.null_separator;
        let discovery_head = query.head.or(query.max_count);
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
            let has_exec = !effective_exec.is_empty() || !effective_exec_batch.is_empty();
            let has_action = query.action.is_some();
            let is_dry = query.dry_run || self.cli.dry_run;
            let needs_buffering =
                sort_key.is_some() || tail_limit.is_some() || has_exec || has_action;

            if needs_buffering {
                #[derive(Clone)]
                struct BufferedEntry {
                    path: PathBuf,
                    is_dir: bool,
                    is_symlink: bool,
                    size: u64,
                    mtime: std::time::SystemTime,
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
                            });
                        }
                        Ok(())
                    };

                if !roots.is_empty()
                    && let Err(err) = walker.walk(&roots, Arc::new(ignore_engine), &on_entry)
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
                        SortKey::Size => list
                            .sort_by(|a, b| a.size.cmp(&b.size).then_with(|| a.path.cmp(&b.path))),
                        SortKey::SizeDesc => list
                            .sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path))),
                        SortKey::Modified => list.sort_by(|a, b| {
                            b.mtime.cmp(&a.mtime).then_with(|| a.path.cmp(&b.path))
                        }),
                        SortKey::ModifiedDesc => list.sort_by(|a, b| {
                            a.mtime.cmp(&b.mtime).then_with(|| a.path.cmp(&b.path))
                        }),
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
                }

                if let Some(t) = tail_limit {
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
                for e in &list {
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
                    if let Err(err) = output.print_entry_with_column(
                        &e.path,
                        e.is_dir,
                        e.is_symlink,
                        col.as_deref(),
                        null_output,
                    ) {
                        if err.kind() == io::ErrorKind::BrokenPipe {
                            return Ok(0);
                        }
                        return Err(err.to_string());
                    }
                }
            } else {
                let detached = pr
                    .lock()
                    .map_err(|_| "output lock poisoned".to_string())?
                    .clone_detached();
                let on_entry =
                    move |entry: crate::core::DirEntry| -> Result<(), crate::core::WalkerError> {
                        if !pred.matches(&entry) {
                            return Ok(());
                        }
                        if let Some(max) = discovery_head {
                            let mut current = tot_matches.load(Ordering::Relaxed);
                            loop {
                                if current >= max {
                                    return Ok(());
                                }
                                match tot_matches.compare_exchange_weak(
                                    current,
                                    current + 1,
                                    Ordering::Relaxed,
                                    Ordering::Relaxed,
                                ) {
                                    Ok(_) => break,
                                    Err(actual) => current = actual,
                                }
                            }
                        } else {
                            tot_matches.fetch_add(1, Ordering::Relaxed);
                        }
                        tot_searched.fetch_add(1, Ordering::Relaxed);
                        tot_lines.fetch_add(1, Ordering::Relaxed);

                        let raw_path = entry.full_path();
                        let path = raw_path
                            .strip_prefix("./")
                            .or_else(|_| raw_path.strip_prefix(".\\"))
                            .map(Path::to_path_buf)
                            .unwrap_or(raw_path);
                        let mut buf = Vec::new();
                        detached
                            .format_entry_path_styled(
                                &path,
                                entry.is_dir,
                                entry.is_symlink,
                                null_output,
                                &mut buf,
                            )
                            .map_err(crate::core::WalkerError::Io)?;
                        let mut output = pr
                            .lock()
                            .map_err(|_| io::Error::other("output lock poisoned"))?;
                        output.write_raw_bytes(&buf).map_err(Into::into)
                    };

                if !roots.is_empty()
                    && let Err(err) = walker.walk(&roots, Arc::new(ignore_engine), &on_entry)
                {
                    if err.is_broken_pipe() {
                        return Ok(0);
                    }
                    return Err(err.to_string());
                }
            }
        } else {
            // Content search mode
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
            let has_exec = !effective_exec.is_empty() || !effective_exec_batch.is_empty();
            let has_action = query.action.is_some();
            let is_dry = query.dry_run || self.cli.dry_run;
            let needs_content_buffering = content_sort.is_some()
                || content_head.is_some()
                || content_tail.is_some()
                || has_exec
                || has_action;

            let matcher = self.build_matcher(&query)?;
            let search = BufferSearch {
                matcher: matcher.as_ref(),
                options: SearchOptions {
                    max_count: query.max_count,
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
            }
            let buffered_results = Arc::new(std::sync::Mutex::new(Vec::new()));
            let buffered_clone = Arc::clone(&buffered_results);

            let detached_printer = printer
                .lock()
                .map_err(|_| "output lock poisoned".to_string())?
                .clone_detached();

            let process_buffer = |path: &Path, buffer: &[u8]| -> io::Result<()> {
                let Some(result) = search.search(buffer)? else {
                    total_skipped_binaries.fetch_add(1, Ordering::Relaxed);
                    return Ok(());
                };
                total_files_searched.fetch_add(1, Ordering::Relaxed);
                total_bytes_searched.fetch_add(buffer.len() as u64, Ordering::Relaxed);
                total_matches.fetch_add(result.matches, Ordering::Relaxed);
                total_matched_lines.fetch_add(result.matched_lines, Ordering::Relaxed);
                if result.matched_lines > 0 {
                    files_with_matches.fetch_add(1, Ordering::Relaxed);
                }

                if needs_content_buffering {
                    if files_without_match {
                        if result.matched_lines == 0 {
                            let mtime = std::fs::metadata(path)
                                .and_then(|m| m.modified())
                                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            if let Ok(mut guard) = buffered_clone.lock() {
                                guard.push(BufferedFileResult {
                                    path: path.to_path_buf(),
                                    records: Vec::new(),
                                    matches: 0,
                                    _matched_lines: 0,
                                    file_size: buffer.len() as u64,
                                    mtime,
                                });
                            }
                        }
                    } else if result.matched_lines > 0 || print_empty_count {
                        let owned_records = result.records.iter().map(|r| r.to_owned()).collect();
                        let mtime = std::fs::metadata(path)
                            .and_then(|m| m.modified())
                            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                        if let Ok(mut guard) = buffered_clone.lock() {
                            guard.push(BufferedFileResult {
                                path: path.to_path_buf(),
                                records: owned_records,
                                matches: result.matches,
                                _matched_lines: result.matched_lines,
                                file_size: buffer.len() as u64,
                                mtime,
                            });
                        }
                    }
                } else {
                    // Keep the no-match path allocation-free. Formatting a
                    // selected result will grow this buffer on first write.
                    let mut buf = Vec::new();
                    if files_without_match {
                        if result.matched_lines == 0 {
                            detached_printer.format_file_without_match_with_column(
                                path,
                                None,
                                null_output,
                                &mut buf,
                            )?;
                        }
                    } else if result.matched_lines > 0 || print_empty_count {
                        detached_printer.format_file_matches_with_column(
                            path,
                            &result.records,
                            None,
                            &mut buf,
                        )?;
                    }
                    if !buf.is_empty() {
                        let mut output = printer
                            .lock()
                            .map_err(|_| io::Error::other("output lock poisoned"))?;
                        output.write_raw_bytes(&buf)?;
                    }
                }
                Ok(())
            };

            if use_stdin {
                use std::io::Read;
                let mut bytes = Vec::new();
                io::stdin()
                    .read_to_end(&mut bytes)
                    .map_err(|err| err.to_string())?;
                if let Err(err) = process_buffer(Path::new("(standard input)"), &bytes) {
                    if err.kind() == io::ErrorKind::BrokenPipe {
                        return Ok(0);
                    }
                    return Err(err.to_string());
                }
            }

            let pred = Arc::clone(&query_predicate);
            let on_entry = |entry: crate::core::DirEntry| -> Result<(), crate::core::WalkerError> {
                if !pred.matches(&entry) {
                    return Ok(());
                }
                let path = entry.full_path();
                SCRATCH.with(|cell| {
                    let mut scratch = cell.borrow_mut();
                    scratch.clear();
                    let buffer = match reader.read(&path, &mut scratch) {
                        Ok(buffer) => buffer,
                        Err(err) => {
                            let benign = matches!(
                                err.kind(),
                                io::ErrorKind::PermissionDenied
                                    | io::ErrorKind::NotFound
                                    | io::ErrorKind::IsADirectory
                            );
                            if !suppress_traversal_errors || (!benign && !no_messages) {
                                eprintln!("{}: {err}", path.display());
                                had_error.store(true, Ordering::Relaxed);
                            }
                            return Ok(());
                        }
                    };
                    process_buffer(&path, &buffer).map_err(Into::into)
                })
            };

            if !roots.is_empty()
                && let Err(err) = walker.walk(&roots, Arc::new(ignore_engine), &on_entry)
            {
                if err.is_broken_pipe() {
                    return Ok(0);
                }
                return Err(err.to_string());
            }

            if needs_content_buffering {
                let mut results = match buffered_results.lock() {
                    Ok(mut guard) => std::mem::take(&mut *guard),
                    Err(_) => return Err("Failed to acquire lock on content search results".into()),
                };

                if let Some(sk) = content_sort {
                    if (before_context > 0 || after_context > 0)
                        && matches!(sk, crate::dsl::SortKey::Len | crate::dsl::SortKey::LenDesc)
                    {
                        return Err(
                            "Cannot combine sort:len with context lines (-A, -B, -C, ctx:)"
                                .to_string(),
                        );
                    }
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
                    if let Some(t) = content_tail {
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
                } else if content_head.is_some() || content_tail.is_some() {
                    let total_matching_lines: usize = results
                        .iter()
                        .map(|f| f.records.iter().filter(|r| !r.is_context).count())
                        .sum();

                    let (keep_start, keep_end) = if let Some(t) = content_tail {
                        if t == 0 {
                            (0, 0)
                        } else {
                            (total_matching_lines.saturating_sub(t), total_matching_lines)
                        }
                    } else if let Some(h) = content_head {
                        (0, h.min(total_matching_lines))
                    } else {
                        (0, total_matching_lines)
                    };

                    let mut current_match_idx = 0;
                    let mut filtered = Vec::new();
                    for mut f in results {
                        let mut kept_match_lines = std::collections::BTreeSet::new();
                        for r in &f.records {
                            if !r.is_context {
                                if current_match_idx >= keep_start && current_match_idx < keep_end {
                                    kept_match_lines.insert(r.line_number);
                                }
                                current_match_idx += 1;
                            }
                        }

                        if kept_match_lines.is_empty() {
                            f.records.clear();
                        } else {
                            f.records.retain_mut(|r| {
                                if kept_match_lines.contains(&r.line_number) {
                                    r.is_context = false;
                                    true
                                } else {
                                    let in_context = kept_match_lines.iter().any(|&m_line| {
                                        (r.line_number < m_line
                                            && m_line - r.line_number <= before_context)
                                            || (r.line_number > m_line
                                                && r.line_number - m_line <= after_context)
                                    });
                                    if in_context {
                                        r.is_context = true;
                                        r.match_spans.clear();
                                        true
                                    } else {
                                        false
                                    }
                                }
                            });
                        }

                        if !f.records.is_empty() || print_empty_count {
                            filtered.push(f);
                        }
                    }
                    results = filtered;
                }

                if needs_content_buffering {
                    if files_without_match {
                        selected_files_without_match_cnt = Some(results.len());
                    } else {
                        let mut sliced_matches = 0;
                        let mut sliced_lines = 0;
                        let mut sliced_files_with_match = 0;
                        for f in &results {
                            let file_matches: usize = f
                                .records
                                .iter()
                                .filter(|r| !r.is_context)
                                .map(|r| r.match_spans.len().max(1))
                                .sum();
                            let file_matched_lines: usize =
                                f.records.iter().filter(|r| !r.is_context).count();
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

                let mut output = printer
                    .lock()
                    .map_err(|_| "output lock poisoned".to_string())?;
                for f in &results {
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
                    let res = if files_without_match {
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
                                is_context: r.is_context,
                            })
                            .collect();
                        output.print_file_matches_with_column(&f.path, &borrowed, col.as_deref())
                    };
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
            2 // POSIX 2 = error encountered (takes precedence over match status)
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
            0 // POSIX 0 = match found
        } else {
            1 // POSIX 1 = no matches found
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

    /// Collect patterns passed explicitly via `-e/--regexp` and `-f/--file`.
    fn collect_patterns(&self) -> Result<Vec<String>, String> {
        let mut patterns = self.cli.regexp.clone();
        for file_path in &self.cli.file {
            let content = std::fs::read_to_string(file_path).map_err(|e| {
                format!("Failed to read pattern file '{}': {e}", file_path.display())
            })?;
            for line in content.lines() {
                patterns.push(line.to_string());
            }
        }
        Ok(patterns)
    }

    /// Build a QueryExpr from a list of patterns, combining multiple patterns with OR.
    fn build_expr_from_patterns(&self, patterns: &[String]) -> Option<QueryExpr> {
        if patterns.is_empty() {
            return None;
        }
        let case_sensitive = if self.cli.ignore_case {
            Some(false)
        } else if self.cli.case_sensitive {
            Some(true)
        } else {
            None
        };
        let mut expr: Option<QueryExpr> = None;
        let fixed_strings = self.cli.fixed_strings || self.config.search.fixed_strings;

        for p in patterns {
            let pattern = if fixed_strings {
                SearchPattern::ExactLiteral(p.clone())
            } else if self.cli.extended_regexp {
                SearchPattern::Regex(p.clone())
            } else {
                SearchPattern::Literal {
                    text: p.clone(),
                    case_sensitive,
                }
            };
            let atom = QueryExpr::Pattern(pattern);
            expr = match expr {
                Some(prev) => Some(QueryExpr::Or(Box::new(prev), Box::new(atom))),
                None => Some(atom),
            };
        }
        expr
    }

    /// Build query using the ergonomic smart DSL.
    fn build_dsl_query(&self) -> Result<Query, String> {
        let has_explicit_pattern_flag = !self.cli.regexp.is_empty() || !self.cli.file.is_empty();
        let flag_patterns = self.collect_patterns()?;
        let empty_pattern_set = has_explicit_pattern_flag && flag_patterns.is_empty();
        let external_expr = if let Some(ref fz_arg) = self.cli.fuzzy {
            let tokens = DslParser::split_fuzzy_tokens(fz_arg, fz_arg)?;
            Some(DslParser::compile_fuzzy_to_expr(&tokens))
        } else if empty_pattern_set {
            Some(QueryExpr::Pattern(SearchPattern::EmptySet))
        } else {
            self.build_expr_from_patterns(&flag_patterns)
        };
        let fixed_strings = self.cli.fixed_strings || self.config.search.fixed_strings;

        let mut base_query = Query::default();
        if self.cli.text || self.cli.unrestricted >= 3 {
            base_query.include_binaries = true;
        }
        for t in &self.cli.file_type {
            base_query.type_includes.push(t.clone());
        }
        for t in &self.cli.file_type_not {
            base_query.type_excludes.push(t.clone());
        }
        for g in &self.cli.glob {
            if let Some(neg) = g.strip_prefix('!') {
                base_query.path_excludes.push(neg.to_string());
            } else {
                base_query.type_includes.push(g.clone());
            }
        }

        let mut query = DslParser::parse_with_compiler_base_and_config(
            &self.cli.args,
            external_expr,
            base_query,
            Some(&self.config),
            |term, whole_word| {
                if !fixed_strings && !self.cli.extended_regexp {
                    return DslParser::compile_term_to_pattern(term, whole_word);
                }
                if whole_word {
                    let body = if fixed_strings {
                        regex::escape(term)
                    } else {
                        term.to_string()
                    };
                    SearchPattern::Regex(format!(r"\b(?:{body})\b"))
                } else if fixed_strings {
                    SearchPattern::ExactLiteral(term.to_string())
                } else {
                    SearchPattern::Regex(term.to_string())
                }
            },
        )?;
        query.empty_pattern_set = empty_pattern_set;
        if let Some(ref fz_arg) = self.cli.fuzzy {
            let tokens = DslParser::split_fuzzy_tokens(fz_arg, fz_arg)?;
            query.fuzzy = Some(crate::dsl::FuzzyQuery {
                tokens,
                original: fz_arg.clone(),
            });
        }

        // Resolve effective case sensitivity: explicit CLI flags take precedence over DSL query modifiers.
        let effective_case = if self.cli.ignore_case {
            Some(false)
        } else if self.cli.case_sensitive {
            Some(true)
        } else {
            query.case_sensitive
        };
        query.case_sensitive = effective_case;

        if let Some(cs) = query.case_sensitive {
            if let Some(ref mut expr) = query.expr {
                Self::set_case_sensitive(expr, cs);
            }
        } else if (self.cli.smart_case || self.config.search.smart_case)
            && let Some(ref mut expr) = query.expr
        {
            Self::set_smart_case(expr);
        }
        if self.cli.invert_match
            && let Some(expr) = query.expr.take()
        {
            query.expr = Some(QueryExpr::Not(Box::new(expr)));
        }

        for inc in &query.path_includes {
            query.targets.push(PathBuf::from(inc));
        }

        if let Some(ref mut expr) = query.expr {
            self.apply_word_and_line_boundaries(expr);
        }

        if let Some(m) = self.cli.max_count {
            query.max_count = Some(m);
        }
        if let Some(h) = self.cli.head {
            query.head = Some(h);
        }
        if let Some(t) = self.cli.tail {
            query.tail = Some(t);
        }
        if (query.head.is_some() || query.max_count.is_some()) && query.tail.is_some() {
            return Err(
                "Cannot specify both 'head' (or max count) and 'tail' limits simultaneously."
                    .into(),
            );
        }
        if let Some(ref s) = self.cli.sort {
            let key = crate::dsl::parse_sort_key(s, self.cli.reverse)?;
            query.sort = Some(key);
        } else if self.cli.reverse
            && let Some(existing) = query.sort
        {
            query.sort = Some(existing.reversed());
        }

        let mut cli_action: Option<crate::ops::ActionKind> = None;
        let mut action_count = 0;

        if let Some(ref dst) = self.cli.r#move {
            action_count += 1;
            cli_action = Some(crate::ops::ActionKind::Move(dst.clone()));
        }
        if let Some(ref dst) = self.cli.copy {
            action_count += 1;
            cli_action = Some(crate::ops::ActionKind::Copy(dst.clone()));
        }
        if self.cli.trash {
            action_count += 1;
            cli_action = Some(crate::ops::ActionKind::Trash);
        }
        if let Some(ref ren) = self.cli.rename {
            action_count += 1;
            let (pat, rep) = if let Some((p, r)) = ren.split_once('/') {
                (p.to_string(), r.to_string())
            } else if let Some((p, r)) = ren.split_once("->") {
                (p.to_string(), r.to_string())
            } else {
                return Err(format!(
                    "Invalid --rename specification '{ren}'. Expected 'old/new' or 'old->new'."
                ));
            };
            cli_action = Some(crate::ops::ActionKind::Rename {
                pattern: pat,
                replacement: rep,
            });
        }
        if let Some(ref mode) = self.cli.chmod {
            action_count += 1;
            cli_action = Some(crate::ops::ActionKind::Chmod(mode.clone()));
        }

        if action_count > 1 || (action_count > 0 && query.action.is_some()) {
            return Err(
                "Multiple file actions specified. Only one action may be used at a time.".into(),
            );
        }

        if let Some(act) = cli_action {
            query.action = Some(act);
        }

        Ok(query)
    }

    /// Construct the compiled ignore filter from config and query filters.
    fn build_ignore_engine(&self, query: &Query) -> GitignoreEngine {
        let mut default_excludes = self.config.default_excludes.clone();
        if query.search_cache {
            default_excludes.retain(|p| p != ".cache/" && p != "Cache/" && p != "CachedData/");
        }

        // If the user explicitly requested targets, paths, or entry basenames,
        // override matching default excludes so explicitly requested directories are searched.
        if !query.basename_filters.is_empty()
            || !query.targets.is_empty()
            || !query.path_includes.is_empty()
        {
            default_excludes.retain(|excl| {
                let clean = excl.trim_end_matches('/').trim_end_matches('\\');
                let matches_basename = query
                    .basename_filters
                    .iter()
                    .any(|f| f.matches(clean.as_bytes(), query.case_sensitive));
                let matches_target = query.targets.iter().any(|t| {
                    let s = t.to_string_lossy();
                    let trimmed = s.trim_end_matches('/').trim_end_matches('\\');
                    trimmed == clean
                        || trimmed.ends_with(&format!("/{clean}"))
                        || trimmed.ends_with(&format!("\\{clean}"))
                });
                let matches_path_inc = query.path_includes.iter().any(|p| {
                    let trimmed = p.trim_end_matches('/').trim_end_matches('\\');
                    trimmed == clean
                        || trimmed.ends_with(&format!("/{clean}"))
                        || trimmed.ends_with(&format!("\\{clean}"))
                });
                !matches_basename && !matches_target && !matches_path_inc
            });
        }

        // Expand file type aliases (e.g. :web -> *.html, *.css, *.js)
        let mut type_includes = Vec::new();
        for inc in &query.type_includes {
            if let Some(patterns) = self.config.types.get(inc) {
                type_includes.extend(patterns.clone());
            } else {
                type_includes.push(inc.clone());
            }
        }

        let mut type_excludes = Vec::new();
        for exc in &query.type_excludes {
            if let Some(patterns) = self.config.types.get(exc) {
                type_excludes.extend(patterns.clone());
            } else {
                type_excludes.push(exc.clone());
            }
        }

        let respect_ignore =
            !self.cli.no_ignore && !self.config.search.no_ignore && self.cli.unrestricted < 1;
        let search_hidden = query
            .search_hidden
            .unwrap_or(self.cli.hidden || self.config.search.hidden || self.cli.unrestricted >= 2);

        let mut engine = GitignoreEngine::new(
            default_excludes,
            type_includes,
            type_excludes,
            respect_ignore,
            search_hidden,
        )
        .with_path_excludes(query.path_excludes.clone());
        engine.load_global_ignore_files();
        engine
    }

    /// Build the runtime matcher engine from the query AST.
    fn build_matcher(&self, query: &Query) -> Result<Box<dyn Matcher>, String> {
        let expr = match &query.expr {
            Some(e) => e,
            None => {
                // If no pattern specified, match every non-empty line
                return Ok(Box::new(crate::search::AllLinesMatcher));
            }
        };

        // Proximity constraints require BooleanEngine evaluation
        if !query.proximity_filters.is_empty() {
            return Ok(Box::new(
                BooleanEngine::try_new(expr.clone())
                    .map_err(|err| format!("Invalid regex: {err}"))?
                    .with_proximity_filters(&query.proximity_filters),
            ));
        }

        // If it's a simple atomic pattern, optimize with dedicated matcher
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
            QueryExpr::Pattern(SearchPattern::Literal { case_sensitive, .. }) => {
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
            QueryExpr::Pattern(SearchPattern::Literal { case_sensitive, .. }) => {
                *case_sensitive = None;
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
                    SearchPattern::Hex(_) | SearchPattern::EmptySet => return,
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
                    body = format!(r"\b(?:{body})\b");
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

    #[test]
    fn pattern_flags_keep_raw_syntax_and_case_in_compound_queries() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("input.txt");
        std::fs::write(&file, "A..B\n").unwrap();
        for options in [
            vec!["-F", "-i", "a..b"],
            vec!["-F", "-i", "a..b", "AND", "=a..b"],
            vec!["-i", "-x", "re:a[.][.]b|other"],
            vec!["-i", "yes:case", "re:a[.][.]b"],
        ] {
            let mut args = vec!["grx", "-q"];
            args.extend(options);
            args.push(file.to_str().unwrap());
            assert_eq!(
                Engine::new(Config::new_with_defaults(), Cli::parse_from(args))
                    .run()
                    .unwrap(),
                0
            );
        }
        let engine = Engine::new(
            Config::new_with_defaults(),
            Cli::parse_from(["grx", "-E", "a..b", "input.txt"]),
        );
        assert_eq!(
            engine.build_dsl_query().unwrap().expr,
            Some(QueryExpr::Pattern(SearchPattern::Regex("a..b".into())))
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
        let cli = Cli::parse_from(["grx", "-q", "-L", "needle", dir.path().to_str().unwrap()]);
        assert_eq!(
            Engine::new(Config::new_with_defaults(), cli).run().unwrap(),
            1
        );
    }

    #[test]
    fn test_multiple_regexp_flags_combine_with_or() {
        let cli = Cli::parse_from(["grx", "-e", "alpha", "-e", "beta", "src/"]);
        let config = Config::new_with_defaults();
        let engine = Engine::new(config, cli);
        let query = engine.build_dsl_query().unwrap();

        assert_eq!(query.targets, vec![PathBuf::from("src/")]);
        let expected = QueryExpr::Or(
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "alpha".to_string(),
                case_sensitive: None,
            })),
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "beta".to_string(),
                case_sensitive: None,
            })),
        );
        assert_eq!(query.expr, Some(expected));
    }

    #[test]
    fn test_pattern_file_flag() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "foo").unwrap();
        writeln!(file, "bar").unwrap();

        let cli = Cli::parse_from(["grx", "-f", file.path().to_str().unwrap(), "tests/"]);
        let config = Config::new_with_defaults();
        let engine = Engine::new(config, cli);
        let query = engine.build_dsl_query().unwrap();

        assert_eq!(query.targets, vec![PathBuf::from("tests/")]);
        let expected = QueryExpr::Or(
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "foo".to_string(),
                case_sensitive: None,
            })),
            Box::new(QueryExpr::Pattern(SearchPattern::Literal {
                text: "bar".to_string(),
                case_sensitive: None,
            })),
        );
        assert_eq!(query.expr, Some(expected));
    }

    #[test]
    fn test_unrestricted_flag_levels() {
        let cli1 = Cli::parse_from(["grx", "-u", "pat"]);
        assert_eq!(cli1.unrestricted, 1);

        let cli2 = Cli::parse_from(["grx", "-uu", "pat"]);
        assert_eq!(cli2.unrestricted, 2);

        let cli3 = Cli::parse_from(["grx", "-uuu", "pat"]);
        assert_eq!(cli3.unrestricted, 3);
    }

    #[test]
    fn test_smart_case_flag_override() {
        let cli = Cli::parse_from(["grx", "-S", "term"]);
        let config = Config::new_with_defaults();
        let engine = Engine::new(config, cli);
        let query = engine.build_dsl_query().unwrap();
        assert_eq!(
            query.expr,
            Some(QueryExpr::Pattern(SearchPattern::Literal {
                text: "term".to_string(),
                case_sensitive: None,
            }))
        );
    }

    #[test]
    fn test_column_and_byte_offset_cli_flags() {
        let cli = Cli::parse_from(["grx", "--column", "-b", "--count-matches", "pattern"]);
        assert!(cli.column);
        assert!(cli.byte_offset);
        assert!(cli.count_matches);
    }

    #[test]
    fn test_stats_and_json_cli_flags() {
        let cli = Cli::parse_from(["grx", "--stats", "--json", "pattern"]);
        assert!(cli.stats);
        assert!(cli.json);
    }

    #[test]
    fn test_help_and_tutorial_cli_flags() {
        let cli1 = Cli::parse_from(["grx", "-h"]);
        assert!(cli1.help);

        let cli2 = Cli::parse_from(["grx", "--help"]);
        assert!(cli2.help);

        let cli3 = Cli::parse_from(["grx", "--help-full"]);
        assert!(cli3.help_full);

        let cli4 = Cli::parse_from(["grx", "--help-all"]);
        assert!(cli4.help_full);

        let cli5 = Cli::parse_from(["grx", "--tutorial"]);
        assert!(cli5.tutorial);
    }

    #[test]
    fn test_dsl_max_depth_and_limits() {
        let config = Config::default();
        let cli = Cli::parse_from(["grx", "pattern", "d:0", "top:10", "ctx:2", "p:src/"]);
        let engine = Engine::new(config, cli);
        let query = engine.build_dsl_query().unwrap();

        assert_eq!(query.max_depth, Some(0));
        assert_eq!(query.max_count, Some(10));
        assert_eq!(query.context, Some(2));
        assert_eq!(query.path_includes, vec!["src/"]);
    }

    #[test]
    fn test_cli_max_depth_flag() {
        let cli1 = Cli::parse_from(["grx", "-d", "1", "pattern"]);
        assert_eq!(cli1.max_depth, Some(1));

        let cli2 = Cli::parse_from(["grx", "--max-depth", "3", "pattern"]);
        assert_eq!(cli2.max_depth, Some(3));
    }

    #[test]
    fn test_engine_fuzzy_cli_flag() {
        let config = Config::default();
        let cli = Cli::parse_from(["grx", "-Z", "from,ptr,err", "src/"]);
        let engine = Engine::new(config, cli);
        let query = engine.build_dsl_query().unwrap();

        assert!(query.fuzzy.is_some());
        let fz = query.fuzzy.unwrap();
        assert_eq!(fz.tokens, vec!["from", "ptr", "err"]);
        assert_eq!(query.targets, vec![PathBuf::from("src/")]);
        assert!(query.expr.is_some());
    }

    #[test]
    fn test_engine_proximity_matcher_selection() {
        let config = Config::default();
        let cli = Cli::parse_from(["grx", "unsafe", "near:5,safety"]);
        let engine = Engine::new(config, cli);
        let query = engine.build_dsl_query().unwrap();

        assert_eq!(query.proximity_filters.len(), 1);
        let matcher = engine.build_matcher(&query);
        assert!(matcher.is_ok());
    }

    #[test]
    fn test_query_entry_predicate_matches() {
        let config = Config::default();
        let reference = std::time::SystemTime::now();

        // 1. Kind matches
        let file_query = Query {
            kind: Some(EntryKind::File),
            ..Query::default()
        };
        let pred_file = QueryEntryPredicate::new(&file_query, &config, reference);
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
        let pred_dir = QueryEntryPredicate::new(&dir_query, &config, reference);
        assert!(!pred_dir.matches(&file_entry));
        assert!(pred_dir.matches(&dir_entry));

        let link_query = Query {
            kind: Some(EntryKind::Link),
            ..Query::default()
        };
        let pred_link = QueryEntryPredicate::new(&link_query, &config, reference);
        assert!(!pred_link.matches(&file_entry));
        assert!(pred_link.matches(&link_entry));

        // Bin and Text matches
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
        let pred_bin = QueryEntryPredicate::new(&bin_query, &config, reference);
        assert!(pred_bin.matches(&bin_entry));
        assert!(!pred_bin.matches(&txt_entry));
        assert!(!pred_bin.matches(&dir_entry));

        let txt_query = Query {
            kind: Some(EntryKind::Text),
            ..Query::default()
        };
        let pred_txt = QueryEntryPredicate::new(&txt_query, &config, reference);
        assert!(!pred_txt.matches(&bin_entry));
        assert!(pred_txt.matches(&txt_entry));
        assert!(!pred_txt.matches(&dir_entry));

        // 2. Basename filter matches (Option 1: matches both files and directories)
        let in_query = Query {
            basename_filters: vec![BasenameFilter::parse("report")],
            basename_exclude_filters: vec![BasenameFilter::parse("draft")],
            ..Query::default()
        };
        let pred_in = QueryEntryPredicate::new(&in_query, &config, reference);
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
        assert!(pred_in.matches(&match_dir_entry)); // Option 1 unified discovery
        assert!(!pred_in.matches(&fail_entry));
        assert!(!pred_in.matches(&excluded_entry));

        // 3. Size matches
        let size_query = Query {
            size_predicates: vec![SizePredicate::Larger(50), SizePredicate::Smaller(200)],
            ..Query::default()
        };
        let pred_size = QueryEntryPredicate::new(&size_query, &config, reference);
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

        // 1. Explicit content search + kind:dir must return exit code 2
        let cli = Cli::parse_from(["grx", "-e", "pattern", "kind:dir"]);
        let mut engine = Engine::new(config.clone(), cli);
        assert_eq!(engine.run().unwrap(), 2);

        // 2. Explicit content search + kind:link must return exit code 2
        let cli = Cli::parse_from(["grx", "-e", "pattern", "kind:link"]);
        let mut engine = Engine::new(config.clone(), cli);
        assert_eq!(engine.run().unwrap(), 2);

        // 3. Discovery mode + stdin target '-' must return exit code 2
        let cli = Cli::parse_from(["grx", "in:report", "-"]);
        let mut engine = Engine::new(config.clone(), cli);
        assert_eq!(engine.run().unwrap(), 2);

        // 4. Discovery mode + size predicate on kind:dir must return exit code 2
        let cli = Cli::parse_from(["grx", "kind:dir", "larger:10MiB"]);
        let mut engine = Engine::new(config.clone(), cli);
        assert_eq!(engine.run().unwrap(), 2);

        // 5. Discovery mode + ctx:2 must return exit code 2
        let cli = Cli::parse_from(["grx", "in:report", "ctx:2"]);
        let mut engine = Engine::new(config.clone(), cli);
        assert_eq!(engine.run().unwrap(), 2);

        // 6. Bare invocation with zero arguments must return exit code 2
        let cli = Cli::parse_from(["grx"]);
        let mut engine = Engine::new(config.clone(), cli);
        assert_eq!(engine.run().unwrap(), 2);

        // 7. Simultaneous head and tail flags must return error (exit code 2)
        let cli_ht = Cli::parse_from(["grx", "in:report", "--head", "5", "--tail", "10"]);
        let mut engine_ht = Engine::new(config.clone(), cli_ht);
        assert!(engine_ht.run().is_err());

        // 8. Simultaneous head and tail DSL selectors must return error
        let cli_ht_dsl = Cli::parse_from(["grx", "in:report", "head:5", "tail:10"]);
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

        // 1. -i flag forces case-insensitive matching even with uppercase filter
        let cli_i = Cli::parse_from(["grx", "-i", "in:REPORT", "kind:file"]);
        let engine_i = Engine::new(config.clone(), cli_i);
        let q_i = engine_i.build_dsl_query().unwrap();
        assert_eq!(q_i.case_sensitive, Some(false));
        let pred_i = QueryEntryPredicate::new(&q_i, &config, reference);
        assert!(pred_i.matches(&report_lower));
        assert!(pred_i.matches(&report_upper));

        // 2. -s flag forces case-sensitive matching even with lowercase filter
        let cli_s = Cli::parse_from(["grx", "-s", "in:report", "kind:file"]);
        let engine_s = Engine::new(config.clone(), cli_s);
        let q_s = engine_s.build_dsl_query().unwrap();
        assert_eq!(q_s.case_sensitive, Some(true));
        let pred_s = QueryEntryPredicate::new(&q_s, &config, reference);
        assert!(pred_s.matches(&report_lower));
        assert!(!pred_s.matches(&report_upper));

        // 3. Default smart-case: uppercase filter is case-sensitive
        let cli_smart_upper = Cli::parse_from(["grx", "in:REPORT", "kind:file"]);
        let engine_smart_upper = Engine::new(config.clone(), cli_smart_upper);
        let q_smart_upper = engine_smart_upper.build_dsl_query().unwrap();
        assert_eq!(q_smart_upper.case_sensitive, None);
        let pred_smart_upper = QueryEntryPredicate::new(&q_smart_upper, &config, reference);
        assert!(!pred_smart_upper.matches(&report_lower));
        assert!(pred_smart_upper.matches(&report_upper));

        // 4. Default smart-case: lowercase filter matches both
        let cli_smart_lower = Cli::parse_from(["grx", "in:report", "kind:file"]);
        let engine_smart_lower = Engine::new(config.clone(), cli_smart_lower);
        let q_smart_lower = engine_smart_lower.build_dsl_query().unwrap();
        assert_eq!(q_smart_lower.case_sensitive, None);
        let pred_smart_lower = QueryEntryPredicate::new(&q_smart_lower, &config, reference);
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

        // 1. Without inversion: positional target preserved, empty pattern set
        let cli = Cli::parse_from([
            "grx",
            "-f",
            pat_path.to_str().unwrap(),
            target_path.to_str().unwrap(),
        ]);
        let engine = Engine::new(config.clone(), cli);
        let q = engine.build_dsl_query().unwrap();
        assert_eq!(q.targets, vec![target_path.clone()]);
        assert_eq!(q.expr, Some(QueryExpr::Pattern(SearchPattern::EmptySet)));
        let matcher = engine.build_matcher(&q).unwrap();
        let mut sink = crate::search::VecMatchSink::default();
        let count = matcher.find_matches(b"hit\n", &mut sink).unwrap();
        assert_eq!(count, 0);

        // 2. With inversion (-v): positional target preserved, complement selects all lines
        let cli_v = Cli::parse_from([
            "grx",
            "-v",
            "-f",
            pat_path.to_str().unwrap(),
            target_path.to_str().unwrap(),
        ]);
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
    fn test_engine_cli_type_and_glob_flags_preserve_positional_targets() {
        let config = Config::default();

        // 1. grx -t rs src/
        let cli_t = Cli::parse_from(["grx", "-t", "rs", "src/"]);
        let engine_t = Engine::new(config.clone(), cli_t);
        let q_t = engine_t.build_dsl_query().unwrap();
        assert_eq!(q_t.targets, vec![PathBuf::from("src/")]);
        assert_eq!(q_t.type_includes, vec!["rs".to_string()]);
        assert!(q_t.is_discovery());

        // 2. grx -g "*.rs" src/
        let cli_g = Cli::parse_from(["grx", "-g", "*.rs", "src/"]);
        let engine_g = Engine::new(config.clone(), cli_g);
        let q_g = engine_g.build_dsl_query().unwrap();
        assert_eq!(q_g.targets, vec![PathBuf::from("src/")]);
        assert_eq!(q_g.type_includes, vec!["*.rs".to_string()]);
        assert!(q_g.is_discovery());
    }

    #[test]
    fn test_query_entry_predicate_type_excludes_preserves_directories() {
        let config = Config::default();
        let reference = std::time::SystemTime::now();

        let mut query = Query::default();
        query.type_excludes.push("c".to_string());
        query.type_excludes.push("rs".to_string());

        let pred = QueryEntryPredicate::new(&query, &config, reference);

        // Regular file named "main.c" should be excluded
        let file_c = crate::core::DirEntry::new(
            PathBuf::from("src"),
            b"main.c".to_vec(),
            false,
            false,
            None,
        );
        assert!(!pred.matches(&file_c));

        // Directory named "test.c" or "rs" should NOT be excluded
        let dir_c =
            crate::core::DirEntry::new(PathBuf::from("src"), b"test.c".to_vec(), true, false, None);
        assert!(pred.matches(&dir_c));
        let dir_rs =
            crate::core::DirEntry::new(PathBuf::from("src"), b"rs".to_vec(), true, false, None);
        assert!(pred.matches(&dir_rs));
    }

    #[test]
    fn test_engine_text_flag_sets_include_binaries_and_raw_binary_text() {
        let config = Config::default();
        let cli = Cli::parse_from(["grx", "-a", "needle", "file.bin"]);
        let engine = Engine::new(config, cli);
        let q = engine.build_dsl_query().unwrap();
        assert!(q.include_binaries);
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

        // Test listing empty WAL
        let config = Config::default();
        let cli = Cli::parse_from(["grx", "undo", "--list"]);
        let mut engine = Engine::new(config.clone(), cli);
        assert_eq!(engine.run().unwrap(), 0);

        // Test dry-run with no transactions
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
    fn test_engine_action_flags_and_validation() {
        let config = Config::default();
        let cli_ren = Cli::parse_from(["grx", "--rename", "old/new", "pattern"]);
        let engine_ren = Engine::new(config.clone(), cli_ren);
        let q_ren = engine_ren.build_dsl_query().unwrap();
        assert_eq!(
            q_ren.action,
            Some(crate::ops::ActionKind::Rename {
                pattern: "old".to_string(),
                replacement: "new".to_string(),
            })
        );

        let cli_chmod = Cli::parse_from(["grx", "--chmod", "755", "pattern"]);
        let engine_chmod = Engine::new(config.clone(), cli_chmod);
        let q_chmod = engine_chmod.build_dsl_query().unwrap();
        assert_eq!(
            q_chmod.action,
            Some(crate::ops::ActionKind::Chmod("755".to_string()))
        );

        let cli_move = Cli::parse_from(["grx", "--move", "dest_dir/", "pattern"]);
        let engine_move = Engine::new(config.clone(), cli_move);
        let q_move = engine_move.build_dsl_query().unwrap();
        assert_eq!(
            q_move.action,
            Some(crate::ops::ActionKind::Move(PathBuf::from("dest_dir/")))
        );

        let cli_copy = Cli::parse_from(["grx", "--cp", "copy_dir/", "pattern"]);
        let engine_copy = Engine::new(config.clone(), cli_copy);
        let q_copy = engine_copy.build_dsl_query().unwrap();
        assert_eq!(
            q_copy.action,
            Some(crate::ops::ActionKind::Copy(PathBuf::from("copy_dir/")))
        );

        let cli_trash = Cli::parse_from(["grx", "--trash", "pattern"]);
        let engine_trash = Engine::new(config.clone(), cli_trash);
        let q_trash = engine_trash.build_dsl_query().unwrap();
        assert_eq!(q_trash.action, Some(crate::ops::ActionKind::Trash));

        // Multiple actions conflict validation
        let cli_conflict = Cli::parse_from(["grx", "--move", "a/", "--trash", "pattern"]);
        let engine_conflict = Engine::new(config, cli_conflict);
        assert!(engine_conflict.build_dsl_query().is_err());
    }
}
