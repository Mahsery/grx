use crate::config::{ColorChoice, HyperlinkChoice, SearchMode};
use clap::{CommandFactory, Parser};
use std::io::IsTerminal;
use std::path::PathBuf;

/// Search file contents and filesystem entries.
#[derive(Parser, Debug, Clone)]
#[command(name = "grx", version = env!("GRX_BUILD_VERSION"), author, about, disable_help_flag = true)]
pub struct Cli {
    /// Print concise help information.
    #[arg(short = 'h', long = "help")]
    pub help: bool,

    /// Print exhaustive help information listing all options and flags.
    #[arg(long = "help-full", alias = "help-all")]
    pub help_full: bool,

    /// Extended modifier flag (e.g. --help --all).
    #[arg(long = "all")]
    pub all: bool,

    /// Print an interactive TLDR tutorial explaining the search DSL with examples.
    #[arg(long = "tutorial")]
    pub tutorial: bool,

    /// Modal DSL expressions, filesystem selectors, and Boolean operators.
    #[arg(value_name = "ARGS")]
    pub args: Vec<String>,

    /// Search mode: dsl, grep, or git-grep.
    #[arg(long, value_enum)]
    pub mode: Option<SearchMode>,

    /// Path to custom grx.toml configuration file, or open configuration in system editor if passed without arguments.
    #[arg(long, value_name = "PATH", num_args = 0..=1, default_missing_value = "")]
    pub config: Option<String>,

    /// Open the active configuration file in the system text editor ($VISUAL, $EDITOR, or xdg-open).
    #[arg(long)]
    pub edit_config: bool,

    /// Print the default, fully commented-out grx.toml configuration template to stdout and exit.
    #[arg(long)]
    pub dump_config: bool,

    /// Initialize default configuration file in distro-appropriate XDG config directory.
    #[arg(long)]
    pub init_config: bool,

    /// Overwrite existing file when initializing configuration.
    #[arg(long)]
    pub force: bool,

    /// Print the path of the active configuration file and exit.
    #[arg(long)]
    pub config_path: bool,

    /// Print all distro-appropriate storage paths (config, data, cache, state, binary) and exit.
    #[arg(long)]
    pub paths: bool,

    /// Enable append-only execution telemetry journaling for this search run.
    #[arg(long)]
    pub journal: bool,

    /// Output shell completion script to stdout (supported: fish, bash, zsh).
    #[arg(long, value_name = "SHELL")]
    pub completions: Option<String>,

    /// Install completion script to the distro-appropriate shell completion directory (auto-detects shell if omitted).
    #[arg(long, value_name = "SHELL", num_args = 0..=1, default_missing_value = "auto")]
    pub install_completions: Option<String>,

    /// A pattern to search for. Can be provided multiple times (combined with OR).
    #[arg(short = 'e', long = "regexp", action = clap::ArgAction::Append)]
    pub regexp: Vec<String>,

    /// Obtain patterns from FILE, one per line. Can be provided multiple times.
    #[arg(short = 'f', long = "file", action = clap::ArgAction::Append)]
    pub file: Vec<PathBuf>,

    /// Force case-insensitive matching across names, paths, and contents.
    #[arg(short = 'i', long = "ignore-case")]
    pub ignore_case: bool,

    /// Force case-sensitive search.
    #[arg(short = 's', long = "case-sensitive")]
    pub case_sensitive: bool,

    /// Search case-insensitively if pattern is all lowercase, case-sensitively otherwise.
    #[arg(short = 'S', long = "smart-case")]
    pub smart_case: bool,

    /// Invert match: select non-matching lines.
    #[arg(short = 'v', long = "invert-match")]
    pub invert_match: bool,

    /// Match only whole words.
    #[arg(short = 'w', long = "word-regexp")]
    pub word_regexp: bool,

    /// Match only whole lines.
    #[arg(short = 'x', long = "line-regexp")]
    pub line_regexp: bool,

    /// Print 1-indexed line numbers beside matches or discovery paths.
    #[arg(short = 'n', long = "line-number")]
    pub line_number: bool,

    /// Suppress line numbers in match output.
    #[arg(short = 'N', long = "no-line-number")]
    pub no_line_number: bool,

    /// Show 1-based column number for matches.
    #[arg(long = "column", overrides_with = "no_column")]
    pub column: bool,

    /// Suppress column numbers.
    #[arg(long = "no-column", overrides_with = "column")]
    pub no_column: bool,

    /// Print 0-based byte offset of matching lines or matched parts.
    #[arg(short = 'b', long = "byte-offset")]
    pub byte_offset: bool,

    /// Print file path heading above matching lines (default on TTY).
    #[arg(long = "heading", overrides_with = "no_heading")]
    pub heading: bool,

    /// Suppress file path headings, printing path prefix before each match line.
    #[arg(long = "no-heading", overrides_with = "heading")]
    pub no_heading: bool,

    /// Pretty output alias: enables --color always --heading --line-number.
    #[arg(short = 'p', long = "pretty")]
    pub pretty: bool,

    /// Reduce ignore filtering (-u: ignore .gitignore; -uu: search hidden files; -uuu: search binary files).
    #[arg(short = 'u', long = "unrestricted", action = clap::ArgAction::Count)]
    pub unrestricted: u8,

    /// Print the file name for each match.
    #[arg(short = 'H', long = "with-filename")]
    pub with_filename: bool,

    /// Suppress file names in match output.
    #[arg(short = 'I', long = "no-filename")]
    pub no_filename: bool,

    /// Only print a count of matching lines per file.
    #[arg(short = 'c', long = "count")]
    pub count: bool,

    /// Print the total count of individual matches per file (distinct from -c which counts lines).
    #[arg(long = "count-matches")]
    pub count_matches: bool,

    /// Print aggregate traversal, matching, and timing statistics.
    #[arg(long = "stats")]
    pub stats: bool,

    /// Output search results as a stream of JSON records.
    #[arg(long = "json")]
    pub json: bool,

    /// Only print file names of files containing matches.
    #[arg(short = 'l', long = "files-with-matches")]
    pub files_with_matches: bool,

    /// Only print file names of files containing NO matches.
    #[arg(short = 'L', long = "files-without-match")]
    pub files_without_match: bool,

    /// Stop reading a file after NUM matching lines.
    #[arg(short = 'm', long = "max-count", value_name = "NUM")]
    pub max_count: Option<usize>,

    /// Limit results globally to first NUM matches or discovery entries.
    #[arg(long = "head", value_name = "NUM")]
    pub head: Option<usize>,

    /// Show only the last NUM matches or discovery results.
    #[arg(long = "tail", value_name = "NUM")]
    pub tail: Option<usize>,

    /// Pick the N-th result (1-based; negative numbers index from the end).
    #[arg(long = "nth", value_name = "INDEX", allow_negative_numbers = true, conflicts_with_all = ["head", "tail", "pick"])]
    pub nth: Option<isize>,

    /// Interactively select result(s) before emitting or executing.
    #[arg(long = "pick", conflicts_with = "nth")]
    pub pick: bool,

    /// Sort results by key (size, modified, path, len, line, count; default: path).
    #[arg(long = "sort", value_name = "KEY", num_args = 0..=1, default_missing_value = "path")]
    pub sort: Option<String>,

    /// Reverse sort ordering (alias: --sort-reverse).
    #[arg(long = "reverse", visible_alias = "sort-reverse")]
    pub reverse: bool,

    /// Show only the non-empty matched parts of matching lines.
    #[arg(short = 'o', long = "only-matching")]
    pub only_matching: bool,

    /// Quiet mode: suppress all normal output, exit with 0 if match found.
    #[arg(short = 'q', long = "quiet", alias = "silent")]
    pub quiet: bool,

    /// Suppress error messages about nonexistent or unreadable files.
    #[arg(long = "no-messages")]
    pub no_messages: bool,

    /// Explicitly show ignore and traversal error messages (ripgrep compatibility).
    #[arg(long = "no-ignore-messages")]
    pub no_ignore_messages: bool,

    /// Treat search pattern as fixed literal strings (no regex).
    #[arg(short = 'F', long = "fixed-strings")]
    pub fixed_strings: bool,

    /// Treat search pattern as extended regular expressions.
    #[arg(short = 'E', long = "extended-regexp")]
    pub extended_regexp: bool,

    /// Print NUM lines of context before and after matches.
    #[arg(short = 'C', long = "context", value_name = "NUM")]
    pub context: Option<usize>,

    /// Print NUM lines of context before matches.
    #[arg(short = 'B', long = "before-context", value_name = "NUM")]
    pub before_context: Option<usize>,

    /// Print NUM lines of context after matches.
    #[arg(short = 'A', long = "after-context", value_name = "NUM")]
    pub after_context: Option<usize>,

    /// Recursively search directories.
    #[arg(short = 'r', short_alias = 'R', long = "recursive")]
    pub recursive: bool,

    /// Maximum directory recursion depth (0 = current directory only).
    #[arg(short = 'd', long = "max-depth", value_name = "NUM")]
    pub max_depth: Option<usize>,

    /// Output a zero byte (NUL) instead of standard separator for xargs -0.
    #[arg(short = '0', long = "null")]
    pub null_output: bool,

    /// Scored fzf-style ordered-subsequence matching in the resolved query mode.
    #[arg(short = 'Z', long = "fuzzy", value_name = "TOKENS")]
    pub fuzzy: Option<String>,

    /// Filter by file type or extension (e.g. rust, toml, c, web).
    #[arg(short = 't', long = "type", value_name = "TYPE")]
    pub file_type: Vec<String>,

    /// Exclude files matching file type or extension.
    #[arg(short = 'T', long = "type-not", value_name = "TYPE")]
    pub file_type_not: Vec<String>,

    /// Include or exclude (with !) files and directories matching glob.
    #[arg(short = 'g', long = "glob", value_name = "GLOB")]
    pub glob: Vec<String>,

    /// Search hidden files and directories.
    #[arg(long = "hidden")]
    pub hidden: bool,

    /// Do not respect .gitignore and .ignore files.
    #[arg(long = "no-ignore")]
    pub no_ignore: bool,

    /// Follow directory symlinks during traversal.
    #[arg(long = "follow")]
    pub follow: bool,

    /// Search inside binary files as text.
    #[arg(short = 'a', long = "text", alias = "binary")]
    pub text: bool,

    /// Number of worker threads (0 = auto-detect CPU cores).
    #[arg(short = 'j', long = "threads", value_name = "NUM")]
    pub threads: Option<usize>,

    /// When to use colors: "auto", "always", or "never".
    #[arg(long = "color", value_enum)]
    pub color: Option<ColorChoice>,

    /// When to emit OSC 8 terminal hyperlinks: "auto", "always", or "never".
    #[arg(long = "hyperlinks", value_enum)]
    pub hyperlinks: Option<HyperlinkChoice>,

    /// Truncate lines longer than NUM characters in output (default: 1000 in TTY, 0/unlimited if redirected).
    #[arg(short = 'M', long = "max-columns", value_name = "NUM")]
    pub max_columns: Option<usize>,

    /// Do not truncate long lines in output.
    #[arg(long = "no-truncate")]
    pub no_truncate: bool,

    /// Execute a command for each search result (placeholders: {}, {/}, {//}, {.}).
    #[arg(long = "exec", num_args = 1.., allow_hyphen_values = true, value_name = "CMD", conflicts_with = "exec_batch")]
    pub exec: Vec<String>,

    /// Execute a command once with all search results as arguments.
    #[arg(short = 'X', long = "exec-batch", num_args = 1.., allow_hyphen_values = true, value_name = "CMD", conflicts_with = "exec")]
    pub exec_batch: Vec<String>,

    /// Simulate file actions without modifying the filesystem.
    #[arg(long = "dry-run", alias = "dry")]
    pub dry_run: bool,

    /// File size threshold in bytes above which memory-mapping (mmap) is used.
    #[arg(long = "mmap-threshold", value_name = "BYTES")]
    pub mmap_threshold: Option<u64>,

    /// Maximum file size to inspect in bytes (files exceeding threshold are skipped).
    #[arg(long = "max-file-size", value_name = "BYTES")]
    pub max_file_size: Option<u64>,

    /// Rename matching files/directories using a destination pattern.
    #[arg(long = "rename", value_name = "PATTERN")]
    pub rename: Option<String>,

    /// Change permissions mode (in octal, e.g. 755 or 644) for matching items.
    #[arg(long = "chmod", value_name = "OCTAL")]
    pub chmod: Option<String>,

    /// Move matching files/directories into a destination directory.
    #[arg(long = "move", alias = "mv", value_name = "DIR")]
    pub r#move: Option<PathBuf>,

    /// Copy matching files/directories into a destination directory.
    #[arg(long = "copy", alias = "cp", value_name = "DIR")]
    pub copy: Option<PathBuf>,

    /// Move matching items into the transaction trash directory.
    #[arg(long = "trash", alias = "rm")]
    pub trash: bool,

    /// Permanently remove all staged items from the transaction trash cache.
    #[arg(long = "clean-trash")]
    pub clean_trash: bool,

    /// List available transaction history (e.g. `grx undo --list`).
    #[arg(long = "list")]
    pub list: bool,
}

impl Cli {
    /// Normalize CLI arguments (e.g. restore misplaced sort values into positional args).
    pub fn normalize(&mut self) {
        if let Some(ref s) = self.sort
            && !crate::dsl::is_valid_sort_key(s)
        {
            let positional = self.sort.take().unwrap();
            self.sort = Some("path".to_string());
            self.args.insert(0, positional);
        }
    }

    /// Parse command-line arguments preserving POSIX "--" in positional arguments.
    pub fn parse_preserving_double_dash<I, T>(raw_args: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let raw: Vec<std::ffi::OsString> = raw_args.into_iter().map(Into::into).collect();
        let mut cli = Self::parse_from(&raw);
        if let Some(dash_pos) = raw
            .iter()
            .take_while(|arg| *arg != "--exec" && *arg != "--exec-batch" && *arg != "-X")
            .position(|a| a == "--")
        {
            let count_after_dash = raw.len().saturating_sub(dash_pos + 1);
            let insert_idx = cli.args.len().saturating_sub(count_after_dash);
            cli.args.insert(insert_idx, "--".to_string());
        }
        cli.normalize();
        cli
    }
}

/// Print concise, high-signal help summary to stdout.
pub fn print_concise_help() {
    let mut out = std::io::stdout().lock();
    print_concise_help_to(&mut out);
}

/// Print concise, high-signal help summary to stderr for usage errors.
pub fn eprint_concise_help() {
    let mut out = std::io::stderr().lock();
    print_concise_help_to(&mut out);
}

fn print_concise_help_to(out: &mut dyn std::io::Write) {
    let _ = writeln!(
        out,
        "grx {} — modal search and filesystem discovery",
        env!("GRX_BUILD_VERSION")
    );
    let _ = writeln!(
        out,
        r#"
USAGE:
    grx [OPTIONS] [DSL_EXPRESSIONS]

MODES:
    Bare text searches contents. kind: (alias k:) makes bare text match names.
    t: and nt: filter filetypes without changing the expression target.
    in: explicitly searches contents of the selected files.
    Selectors without a match expression list eligible paths.

EXAMPLES:
    grx codex                              # Search file contents
    grx kind:dir codex                      # Find directory names
    grx kind:file =memory.md                # Find an exact filename
    grx t:rs p:codex/ memory                # Search Rust contents under codex directories
    grx t:rs p:codex/ in:memory             # Search contents of those Rust files
    grx kind:file memory in:TODO            # Name and content constraints
    grx t:rs OR t:py memory                 # Either type, contents matching memory
    grx kind:file fz:mmry                   # Scored fuzzy filename matching
    grx root:~/Projects t:rs                # Exact traversal root
    grx kind:file temp dry: trash:          # Preview an action
    grx t:rs -n --sort --nth 3               # Pick the third sorted path

MATCHING:
    text           Literal substring
    =text          Complete name or line equality
    fz:text        fzf-style scored ordered-subsequence match
    ^text, text$   Start/end anchors
    foo..bar       Shell-safe wildcard
    re:REGEX       Explicit regex
    lit:TEXT       Literal text, bypassing DSL operators
    AND OR NOT     Boolean expressions; NOT > AND > OR
    '(foo OR bar)' Group one expression domain

FILTER ALTERNATIVES:
    p:a?b / t:a?b  Include either path/type
    np:a?b / nt:a?b Exclude either path/type
    ns:a?b         Exclude lines containing either term
    'p:a\?b'       Literal question mark; commas remain literal
    ? is literal in bare content and in: expressions

SELECTORS AND MODIFIERS:"#
    );
    for syntax in crate::dsl::DSL_SYNTAX {
        let _ = writeln!(
            out,
            "    {:<18} {}",
            format!("{}:", syntax.prefix),
            syntax.description
        );
    }
    let _ = writeln!(
        out,
        r#"
COMMON OPTIONS:
    -e, --regexp PATTERN    Explicit content expression
    -i / -s / -S           Ignore case / case sensitive / smart case
    -u / -uu / -uuu        Ignore rules / also hidden / also binary
    -l / -L / -c           Matching files / nonmatching files / per-file counts
    -n / -N / -0           Number records / no numbers / NUL termination
    --json                 JSON Lines output
    --sort [KEY]           Sort; default path (fuzzy queries default relevance)
    --nth INDEX / --pick   Pick one record / interactively choose records
    --exec CMD...          Command boundary: remaining arguments belong to CMD
    --dry-run              Preview filesystem actions
    --help-full            All CLI options
    --tutorial             Worked examples
    --mode grep            Explicit native grep compatibility

No @ operator or guessed positional roots. Use =, lit:, and root:."#
    );
}

/// Print full, exhaustive help generated by clap.
pub fn print_full_help() {
    let mut cmd = Cli::command();
    let mut out = std::io::stdout().lock();
    let _ = cmd.write_help(&mut out);
    let _ = std::io::Write::write_all(&mut out, b"\n");
}

/// Print an interactive TLDR tutorial for grx and its search DSL.
pub fn print_tutorial() {
    print_tutorial_with_choice(None);
}

/// Print tutorial respecting explicit color choice or detecting TTY and NO_COLOR.
pub fn print_tutorial_with_choice(choice: Option<ColorChoice>) {
    let color = match choice {
        Some(ColorChoice::Always) => true,
        Some(ColorChoice::Never) => false,
        Some(ColorChoice::Auto) | None => {
            std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
        }
    };
    let content = render_tutorial(color);
    let mut out = std::io::stdout().lock();
    let _ = std::io::Write::write_all(&mut out, content.as_bytes());
}

/// Render the complete grx tutorial.
/// When `color` is true, ANSI styling and syntax highlighting are applied.
/// When `color` is false, clean plain text without any ANSI codes is returned.
struct TutorialSection {
    num: &'static str,
    title: &'static str,
    desc: &'static str,
    cmds: &'static [(&'static str, &'static str)],
}

const TUTORIAL_SECTIONS: &[TutorialSection] = &[
    TutorialSection {
        num: "01",
        title: "MODAL NAME AND CONTENT MATCHING",
        desc: "  Resolve the complete query before matching. kind: establishes name matching; t: and nt: filter files; in: targets contents.\n",
        cmds: &[
            ("grx codex", "Search contents for codex"),
            (
                "grx kind:dir codex",
                "Find directory names containing codex",
            ),
            ("grx kind:file =memory.md", "Find a complete filename"),
            (
                "grx k:file main",
                "Short alias for kind:file; match filenames",
            ),
            ("grx t:rs memory", "Search Rust contents for memory"),
            (
                "grx kind:file t:rs memory",
                "Find Rust filenames containing memory",
            ),
            ("grx t:rs in:memory", "Search contents inside Rust files"),
            (
                "grx kind:file report in:TODO",
                "Select names first, then search contents",
            ),
        ],
    },
    TutorialSection {
        num: "02",
        title: "PATH SELECTION AND TRAVERSAL ROOTS",
        desc: "  p: matches paths, never guessed roots. A trailing slash requests exact directory components.\n",
        cmds: &[
            (
                "grx kind:file memory p:codex",
                "Paths contain codex; names contain memory",
            ),
            (
                "grx t:rs p:codex/",
                "Rust files beneath a directory named codex",
            ),
            (
                "grx root:~/Projects t:rs",
                "Start traversal at an exact location",
            ),
            (
                "grx t:rs np:target/ d:2",
                "Exclude target directories; bound traversal depth",
            ),
        ],
    },
    TutorialSection {
        num: "03",
        title: "EXACT, FUZZY, REGEX AND LITERAL MATCHING",
        desc: "  One matching language serves names and contents. Exact matching never trims whitespace.\n",
        cmds: &[
            ("grx kind:file =memory.md", "Exact complete name"),
            ("grx in:=codex", "Exact complete content line"),
            ("grx kind:file fz:mmry", "Scored fzf-style fuzzy names"),
            ("grx t:rs in:fz:mmry", "Scored fuzzy content lines"),
            ("grx kind:file memory..summary", "Ordered wildcard segments"),
            ("grx re:^[A-Z][a-z]+", "Explicit regular expression"),
            ("grx lit:kind:file", "Search literal reserved syntax"),
        ],
    },
    TutorialSection {
        num: "04",
        title: "BOOLEAN EXPRESSIONS",
        desc: "  Selector, name and content expressions have separate scopes, combined with AND.\n",
        cmds: &[
            (
                "grx t:rs OR t:py codex",
                "Either type, with the same content constraint",
            ),
            ("grx t:rs?py codex", "Equivalent type alternatives"),
            (
                "grx font ns:# np:bak?zellij",
                "Exclude comment lines and either path",
            ),
            (
                "grx kind:file '(codex OR memory)' NOT backup",
                "Grouped names with exclusion",
            ),
            ("grx t:rs in:error OR in:warning", "Either content term"),
            (
                "grx kind:file report in:error AND in:panic",
                "Both content terms on the same line",
            ),
        ],
    },
    TutorialSection {
        num: "05",
        title: "CONTENT CONTEXT, PROXIMITY AND BINARY DATA",
        desc: "  Content-only modifiers are rejected in discovery. Filetypes do not enable format decoding.\n",
        cmds: &[
            (
                "grx error near:5,panic ctx:2",
                "Neighbouring content and context",
            ),
            (
                "grx t:rs in:error max:10 head:20",
                "Per-file limit plus global record limit",
            ),
            ("grx kind:bin in:hex:7f454c46", "Raw binary signature"),
            ("grx kind:bin str:4", "Extract printable strings"),
        ],
    },
    TutorialSection {
        num: "06",
        title: "METADATA, VISIBILITY AND RESULT SELECTION",
        desc: "  Explicit filters do not silently override ignores. Sorting precedes global selection.\n",
        cmds: &[
            ("grx t:rs larger:1KiB newer:7d", "File metadata selection"),
            (
                "grx -uuu kind:file p:codex",
                "Include ignored, hidden and binary files",
            ),
            (
                "grx t:rs -n --sort",
                "Number discovery paths in stable order",
            ),
            ("grx t:rs --nth 3", "Pick the third path"),
            ("grx t:rs --pick", "Interactively pick matching files"),
            ("grx t:rs sort:size tail:3", "Last three after size sorting"),
        ],
    },
    TutorialSection {
        num: "07",
        title: "ACTIONS AND NATIVE COMPATIBILITY",
        desc: "  Actions consume selected paths; content actions require explicit whole-file selection.\n",
        cmds: &[
            ("grx kind:file temp dry: trash:", "Preview staged removal"),
            ("grx t:rs cp:backup/", "Copy selected files"),
            (
                "grx -l t:rs in:TODO dry: mv:review/",
                "Preview moving files selected by contents",
            ),
            ("grx undo", "Undo the latest logged filesystem action"),
            (
                "grx --mode grep -i pattern file",
                "Explicit native grep syntax",
            ),
        ],
    },
];

pub fn render_tutorial(color: bool) -> String {
    let mut out = String::with_capacity(16384);

    push_box_header(&mut out, color);

    for sec in TUTORIAL_SECTIONS {
        push_section(&mut out, color, sec.num, sec.title);
        out.push_str(sec.desc);
        for &(cmd, desc) in sec.cmds {
            push_cmd(&mut out, color, cmd, desc);
        }
    }

    push_box_footer(&mut out, color);

    out
}

fn push_box_header(out: &mut String, color: bool) {
    let b_color = if color { "\x1b[38;5;240m" } else { "" };
    let t_color = if color { "\x1b[1;36m" } else { "" };
    let s_color = if color { "\x1b[38;5;248m" } else { "" };
    let r_color = if color { "\x1b[0m" } else { "" };

    out.push_str(b_color);
    out.push_str(
        "╭────────────────────────────────────────────────────────────────────────────╮\n",
    );
    out.push('│');
    out.push_str(r_color);
    out.push_str("                  ");
    out.push_str(t_color);
    out.push_str("grx Search Engine & DSL Tutorial (TLDR)");
    out.push_str(r_color);
    out.push_str("                   ");
    out.push_str(b_color);
    out.push_str("│\n│");
    out.push_str(r_color);
    out.push_str("             ");
    out.push_str(s_color);
    out.push_str("Fast, ergonomic code search with zero-flag syntax");
    out.push_str(r_color);
    out.push_str("              ");
    out.push_str(b_color);
    out.push_str("│\n");
    out.push_str(
        "╰────────────────────────────────────────────────────────────────────────────╯\n",
    );
    out.push_str(r_color);
}

fn push_section(out: &mut String, color: bool, num: &str, title: &str) {
    let b_color = if color { "\x1b[38;5;240m" } else { "" };
    let n_color = if color { "\x1b[1;33m" } else { "" };
    let t_color = if color { "\x1b[1;36m" } else { "" };
    let r_color = if color { "\x1b[0m" } else { "" };

    out.push('\n');
    out.push_str(b_color);
    out.push_str("─── ");
    out.push_str(n_color);
    out.push_str(num);
    out.push_str(". ");
    out.push_str(t_color);
    out.push_str(title);
    out.push(' ');
    out.push_str(b_color);

    let current_len = 4 + num.chars().count() + 2 + title.chars().count() + 1;
    let dashes = if current_len < 78 {
        78 - current_len
    } else {
        3
    };
    for _ in 0..dashes {
        out.push('─');
    }
    out.push_str(r_color);
    out.push('\n');
}

fn push_cmd(out: &mut String, color: bool, cmd: &str, comment: &str) {
    let p_color = if color { "\x1b[1;32m" } else { "" };
    let r_color = if color { "\x1b[0m" } else { "" };
    let c_color = if color { "\x1b[38;5;243m" } else { "" };

    out.push_str("  ");
    out.push_str(p_color);
    out.push_str("$ ");
    out.push_str(r_color);

    let colored_cmd = if color {
        highlight_cmd(cmd)
    } else {
        cmd.to_string()
    };
    out.push_str(&colored_cmd);

    let len = 4 + cmd.chars().count();
    let pad = if len < 40 { 40 - len } else { 1 };
    for _ in 0..pad {
        out.push(' ');
    }

    out.push_str(c_color);
    out.push_str("# ");
    out.push_str(comment);
    out.push_str(r_color);
    out.push('\n');
}

fn highlight_cmd(cmd: &str) -> String {
    let mut result = String::with_capacity(cmd.len() + 64);
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            result.push(chars[i]);
            i += 1;
            continue;
        }

        if chars[i] == '"' || chars[i] == '\'' {
            let quote = chars[i];
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != quote {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if i < chars.len() {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            result.push_str("\x1b[1;36m");
            result.push_str(&token);
            result.push_str("\x1b[0m");
            continue;
        }

        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let token: String = chars[start..i].iter().collect();

        if token == "grx" {
            result.push_str("\x1b[1;37m");
            result.push_str(&token);
            result.push_str("\x1b[0m");
        } else if token.starts_with('-') {
            result.push_str("\x1b[1;35m");
            result.push_str(&token);
            result.push_str("\x1b[0m");
        } else if token == "AND"
            || token == "OR"
            || token == "NOT"
            || token == "NEAR"
            || token.starts_with("NEAR:")
        {
            result.push_str("\x1b[1;31m");
            result.push_str(&token);
            result.push_str("\x1b[0m");
        } else if token.ends_with('$')
            || token.contains("..")
            || crate::dsl::DSL_SYNTAX.iter().any(|entry| {
                token
                    .strip_prefix(entry.prefix)
                    .is_some_and(|rest| rest.starts_with(':'))
            })
            || token.starts_with('=')
            || token.starts_with('^')
        {
            result.push_str("\x1b[1;33m");
            result.push_str(&token);
            result.push_str("\x1b[0m");
        } else {
            result.push_str("\x1b[37m");
            result.push_str(&token);
            result.push_str("\x1b[0m");
        }
    }
    result
}

fn push_box_footer(out: &mut String, color: bool) {
    let b_color = if color { "\x1b[38;5;240m" } else { "" };
    let t_color = if color { "\x1b[1;37m" } else { "" };
    let c_color = if color { "\x1b[1;36m" } else { "" };
    let r_color = if color { "\x1b[0m" } else { "" };

    out.push('\n');
    out.push_str(b_color);
    out.push_str(
        "╭────────────────────────────────────────────────────────────────────────────╮\n",
    );
    out.push('│');
    out.push_str(r_color);
    out.push_str("  ");
    out.push_str(t_color);
    out.push_str("Exhaustive CLI flags: ");
    out.push_str(c_color);
    out.push_str("grx --help-full");
    out.push_str(r_color);
    out.push_str("                                     ");
    out.push_str(b_color);
    out.push_str("│\n│");
    out.push_str(r_color);
    out.push_str("  ");
    out.push_str(t_color);
    out.push_str("Config file locations: ");
    out.push_str(r_color);
    out.push_str("~/.config/grx/config.toml, ./grx.toml, ~/.grx.toml  ");
    out.push_str(b_color);
    out.push_str("│\n");
    out.push_str(
        "╰────────────────────────────────────────────────────────────────────────────╯\n",
    );
    out.push_str(r_color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_tutorial_plain_contains_no_ansi() {
        let plain = render_tutorial(false);
        assert!(
            !plain.contains("\x1b["),
            "Plain tutorial must contain zero ANSI escapes"
        );
        assert!(plain.contains("grx Search Engine & DSL Tutorial"));
        assert!(plain.contains("MODAL NAME AND CONTENT MATCHING"));
        for example in [
            "grx codex",
            "grx kind:dir codex",
            "grx kind:file =memory.md",
            "grx t:rs in:memory",
            "grx root:~/Projects t:rs",
            "grx t:rs OR t:py codex",
            "grx kind:file fz:mmry",
            "grx t:rs --nth 3",
        ] {
            assert!(plain.contains(example), "missing example: {example}");
        }
        assert!(!plain.contains("@impl"));
        assert!(!plain.contains("token permutation"));
    }

    #[test]
    fn unsupported_double_tilde_is_not_highlighted_as_dsl_syntax() {
        let highlighted = highlight_cmd("grx ~~needle");

        assert!(highlighted.contains("\x1b[37m~~needle\x1b[0m"));
        assert!(!highlighted.contains("\x1b[1;33m~~needle\x1b[0m"));
    }

    #[test]
    fn test_render_tutorial_colored_contains_ansi_and_boxes() {
        let colored = render_tutorial(true);
        assert!(
            colored.contains("\x1b["),
            "Colored tutorial must contain ANSI escape codes"
        );
        assert!(colored.contains(
            "╭────────────────────────────────────────────────────────────────────────────╮"
        ));
        assert!(colored.contains(
            "╰────────────────────────────────────────────────────────────────────────────╯"
        ));
        assert!(colored.contains("MODAL NAME AND CONTENT MATCHING"));
        assert!(colored.contains("CONTENT CONTEXT, PROXIMITY AND BINARY DATA"));
        assert!(colored.contains("EXACT, FUZZY, REGEX AND LITERAL MATCHING"));
        assert!(colored.contains("PATH SELECTION AND TRAVERSAL ROOTS"));
        assert!(colored.contains("\x1b[1;36m"));
        assert!(colored.contains("\x1b[1;32m"));
    }

    #[test]
    fn test_explicit_regex_tokens_highlighted_and_rendered_in_tutorial() {
        let highlighted_re = highlight_cmd("grx re:^[A-Z]+");
        assert!(highlighted_re.contains("\x1b[1;33mre:^[A-Z]+\x1b[0m"));

        let tutorial = render_tutorial(false);
        assert!(tutorial.contains("re:^[A-Z][a-z]+"));

        let mut help_buf = Vec::new();
        print_concise_help_to(&mut help_buf);
        let help_str = String::from_utf8_lossy(&help_buf);
        assert!(help_str.contains("re:REGEX"));
        assert!(!help_str.contains("/regex/"));
    }
}
