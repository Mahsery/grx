use crate::config::{ColorChoice, HyperlinkChoice, SearchMode};
use clap::{CommandFactory, Parser};
use std::io::IsTerminal;
use std::path::PathBuf;

/// Fast, ergonomic grep/ripgrep search CLI with a human-friendly DSL.
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

    /// Positional arguments: search terms, filters (:ext, no:path), target directories, or boolean operators.
    #[arg(value_name = "ARGS", allow_hyphen_values = true)]
    pub args: Vec<String>,

    /// Operational mode: "dsl" (smart DSL), "grep" (POSIX grep compatibility), "git-grep".
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

    // --- POSIX Grep & Ripgrep Compatibility Flags ---
    /// A pattern to search for. Can be provided multiple times (combined with OR).
    #[arg(short = 'e', long = "regexp", action = clap::ArgAction::Append)]
    pub regexp: Vec<String>,

    /// Obtain patterns from FILE, one per line. Can be provided multiple times.
    #[arg(short = 'f', long = "file", action = clap::ArgAction::Append)]
    pub file: Vec<PathBuf>,

    /// Case-insensitive search (subject to smart-case in DSL mode).
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

    /// Print 1-indexed line numbers beside matches.
    #[arg(short = 'n', long = "line-number", overrides_with = "no_line_number")]
    pub line_number: bool,

    /// Suppress line numbers in match output.
    #[arg(short = 'N', long = "no-line-number", overrides_with = "line_number")]
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

    /// Sort results by key (size, modified, path, len, line, count).
    #[arg(long = "sort", value_name = "KEY")]
    pub sort: Option<String>,

    /// Reverse sort ordering.
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

    /// Fuzzy token permutation search (match lines containing tokens in any order).
    #[arg(short = 'Z', long = "fuzzy", value_name = "TOKENS")]
    pub fuzzy: Option<String>,

    // --- Modern Ergonomics & Engine Dials ---
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
    #[arg(long = "exec", num_args = 1.., value_name = "CMD", conflicts_with = "exec_batch")]
    pub exec: Vec<String>,

    /// Execute a command once with all search results as arguments.
    #[arg(short = 'X', long = "exec-batch", num_args = 1.., value_name = "CMD", conflicts_with = "exec")]
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

    /// Safely remove matching items by staging them into the transaction trash cache.
    #[arg(long = "trash", alias = "rm")]
    pub trash: bool,

    /// Permanently remove all staged items from the transaction trash cache.
    #[arg(long = "clean-trash")]
    pub clean_trash: bool,

    /// List available transaction history (e.g. `grx undo --list`).
    #[arg(long = "list")]
    pub list: bool,
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
    let msg = format!(
        r#"grx {} — Fast, ergonomic grep/ripgrep search CLI with a human-friendly DSL
Author: Mehmet Koseoglu <mehmet.mkoseoglu@gmail.com>

USAGE:
    grx [OPTIONS] [DSL_EXPRESSIONS] [TARGETS...]

EXAMPLES:
    grx auth p:src/                 # Search 'auth' under src/ (p: or path:)
    grx "Steam/"                    # First bare arg is always pattern (even with slashes)
    grx in:report                   # Discover files/entries whose basename contains 'report'
    grx in:report t:pdf             # Discover PDF files containing 'report'
    grx in:=report.md p:Documents/  # Discover exact 'report.md' under Documents/
    grx "annual revenue" in:report  # Search content inside files matching 'report'
    grx kind:dir in:cache           # Discover directories named 'cache'
    grx dir:src np:data             # Discover directories named 'src', excluding 'data'
    grx dir:=src tail:5             # Discover exact directory 'src', showing last 5 with colors
    grx in:test sort:size tail:10   # Sort matches by file size, showing last 10
    grx t:log larger:10MiB          # Discover log files larger than 10 MiB
    grx t:rs newer:7d               # Discover Rust files modified within 7 days
    grx auth d:0                    # Search current directory only (strictly non-recursive)
    grx auth np:tests/ t:rs         # Search 'auth' in Rust files, excluding tests/ (np: exclude path)
    grx auth ns:debug               # Search 'auth', rejecting lines containing 'debug' (ns: exclude line)
    grx get..id t:rs                # Shell-safe wildcard: 'get' followed by 'id'
    grx ^pub struct                 # Line anchor: lines starting with 'pub struct'
    grx @token p:src/               # Whole-word '@token' inside src/
    grx 'token' AND 'key'           # Both terms must appear on the same line
    grx 'error' NOT debug           # Match 'error', reject lines containing 'debug'
    grx max:10 ctx:2 pattern        # Limit to 10 matches per file with 2 context lines
    grx auth near:5,safety          # Search 'auth' only if 'safety' appears within 5 lines
    grx fz:from_ptr_err             # Fuzzy token permutation (matches any order on line)
    grx -Z 'string,from,""'         # CLI fuzzy search flag (or fz:'string,from,""')
    grx ELF kind:bin                # Mini-hexdump view of binary matches
    grx str:4 kind:bin              # Extract printable ASCII strings (>= 4 chars) from binaries
    grx yes:dots 'secret'           # Search hidden dotfiles and directories
    grx no:ignore 'TODO'            # Search ignoring .gitignore rules
    grx kind:dir in:cpp d:1 mv:dest # Move matching directories into dest/
    grx in:test t:py -X rm          # Batch remove matching files with system rm
    grx in:temp dry: trash:         # Preview safe staging into trash
    grx undo                        # Revert the latest filesystem mutation
    grx undo [TX_ID]                # Revert specific transaction by ID
    grx undo --list                 # List available undo transaction records
    grx --config                    # Open active configuration in system text editor
    grx --mode grep -i "pattern"    # Standard POSIX grep mode

SEARCH DSL CHEAT SHEET (Zero-Flag Filtering):
    p:<path>                        Target directory or file path root (e.g. p:src/, p:crates/)
    in:<pat>, in:=<name>            Filter entry basename (contains, ^start, end$, ..seq, =exact)
    ni:<pat>                        Exclude entry basename (e.g. ni:test, ni:*.bak)
    t:<type>                        Filter filetype or extension (e.g. t:rs, t:py, t:pdf)
    nt:<type>                       Exclude extension or filetype (e.g. nt:rs, nt:c,h)
    kind:file|dir|link|bin|text     Constrain entry kind in discovery/search (e.g. kind:bin)
    sort:<key>, sort:-<key>         Sort results: size, modified, path, len, line, count (- for desc)
    head:<N>, tail:<N>              Limit total results globally to first/last N
    max:<N>                         Limit maximum matching lines per file (e.g. max:10)
    larger:<size>, smaller:<size>   Filter files by size (e.g. larger:10MiB, smaller:1KiB)
    newer:<age>, older:<age>        Filter entries by age (e.g. newer:7d, older:24h)
    d:<N>                           Traversal recursion depth (d:0 = non-recursive)
    ctx:<N>                         Unified before and after context lines (e.g. ctx:3)
    np:<dir>                        Exclude directory or path (e.g. np:target/, np:dist/)
    ns:<str>                        Exclude content lines matching string (e.g. ns:debug)
    near:<N>,<pat>, near:<pat>      Proximity filter (pat must appear within N lines, default 3)
    no-near:<N>,<pat>               Inverted proximity (pat must NOT appear within N lines)
    <pat> NEAR:<N> <target>         Infix proximity operator (or NOT NEAR:<N>)
    fz:<tokens>                     Fuzzy token permutation match (any order on the same line)
    mv:<dest>, cp:<dest>            Move / copy matching items into destination directory
    trash:                          Safely remove matching items (staged in trash WAL)
    dry:                            Simulate action without touching disk
    yes:dots / no:dots              Include / exclude hidden dotfiles and directories
    yes:ignore / no:ignore          Respect / ignore .gitignore rules
    yes:case / no:case              Force case-sensitive / case-insensitive search
    yes:cache                       Include cache directories (.cache/)
    foo..bar                        Shell-safe unquoted wildcard (escapes shell expansion)
    'foo*bar', 'foo?bar'            Quoted glob wildcards converted to regex
    ^term, term$                    Indentation-aware line start (^) and line end ($) anchors
    @term                           Whole-word match boundary (\bterm\b)
    AND, OR, NOT                    Boolean pattern composition
    /regex/, re:<regex>             Explicit regular expression pattern
    "exact phrase"                  Literal phrase matching without regex escaping
    hex:48??e5                      Binary byte pattern with wildcards
    str:<N>                         Extract printable strings (>= N chars) from binaries

COMMON OPTIONS:
    -i, --ignore-case               Case-insensitive search (smart-case by default)
    -s, --case-sensitive            Force case-sensitive search
    -S, --smart-case                Smart-case: case-sensitive only if uppercase present
    -d, --max-depth <NUM>           Maximum directory recursion depth (0 = current dir only)
    -Z, --fuzzy <TOKENS>            Fuzzy token permutation search (any order on line)
    -0, --null                      Output zero byte (NUL) line terminator (for xargs -0)
    -v, --invert-match              Invert match: select non-matching lines
    -w, --word-regexp               Match whole words only
    -n, --line-number               Show line numbers (default on TTY)
    -N, --no-line-number            Suppress line numbers
        --column                    Show 1-based column numbers
    -b, --byte-offset               Show 0-based byte offsets
    -c, --count                     Count matching lines per file
        --count-matches             Count total match occurrences per file
    -l, --files-with-matches        List files containing matches
    -m, --max-count <NUM>           Stop reading after NUM matches per file
    -C, --context <NUM>             Show NUM context lines before and after
    -u, --unrestricted              -u: ignore .gitignore, -uu: +hidden, -uuu: +binary
        --no-messages               Suppress error messages for unreadable files
        --stats                     Print aggregate search and timing statistics
        --json                      Stream ripgrep-compatible JSON Lines
        --config [PATH]             Open config in $EDITOR, or use custom config path
        --edit-config               Open active configuration file in $EDITOR
        --dry-run, --dry            Simulate file actions without modifying disk
        --mmap-threshold <BYTES>    File size threshold for memory mapping (default: 64KB)
        --max-file-size <BYTES>     Skip files exceeding maximum byte size
        --rename <PATTERN>          Rename matching items via destination pattern
        --chmod <OCTAL>             Change permissions mode for matching items
        --move <DIR>, --mv <DIR>    Move matching items into destination directory
        --copy <DIR>, --cp <DIR>    Copy matching items into destination directory
        --trash, --rm               Safely stage matching items into trash cache
        --clean-trash               Purge trash staging cache permanently
        --list                      List undo transaction history (with grx undo)
        --exec <CMD...>             Execute command for each match ({{}}, {{/}}, {{//}}, {{.}})
    -X, --exec-batch <CMD...>       Execute command once with all matches as arguments
    -h, --help                      Print this concise help summary
        --help-full, --help --all   Print full, exhaustive list of all 40+ flags
        --tutorial                  Interactive TLDR tutorial for the search DSL
    -V, --version                   Print version"#,
        env!("GRX_BUILD_VERSION")
    );
    let _ = std::io::Write::write_all(out, msg.as_bytes());
    let _ = std::io::Write::write_all(out, b"\n");
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
pub fn render_tutorial(color: bool) -> String {
    let mut out = String::with_capacity(16384);

    push_box_header(&mut out, color);

    // 01. EXACT MATCHING
    push_section(
        &mut out,
        color,
        "01",
        "EXACT MATCHING: WHOLE-WORD, LITERALS & EXACT LINES",
    );
    out.push_str(
        "  Match whole words, exact lines, or literal strings without substring bleeding:\n",
    );
    push_cmd(
        &mut out,
        color,
        "grx @impl :rs",
        "Whole-word 'impl' (matches 'impl', NOT 'Simple')",
    );
    push_cmd(
        &mut out,
        color,
        "grx w:impl",
        "Alias for whole-word (@impl)",
    );
    push_cmd(
        &mut out,
        color,
        "grx -w impl",
        "POSIX -w / --word-regexp flag",
    );
    push_cmd(
        &mut out,
        color,
        "grx -@simple",
        "Whole-word negation (rejects lines with word 'simple')",
    );
    push_cmd(
        &mut out,
        color,
        "grx @impl -@simple :rs",
        "Whole-word 'impl', no 'simple', in Rust files",
    );
    push_cmd(
        &mut out,
        color,
        "grx -x \"impl Foo for Bar\"",
        "Exact whole-line match (-x / --line-regexp)",
    );
    push_cmd(
        &mut out,
        color,
        "grx \"Steam/\"",
        "Exact literal string (never treated as directory)",
    );
    push_cmd(
        &mut out,
        color,
        "grx =fn()",
        "Equal prefix: literal match without regex syntax",
    );
    push_cmd(
        &mut out,
        color,
        "grx -F \"pattern*[\"",
        "POSIX -F / --fixed-strings flag",
    );
    push_cmd(
        &mut out,
        color,
        "grx -w -F \"impl\"",
        "Combined: exact whole-word fixed-string match",
    );
    push_cmd(
        &mut out,
        color,
        "grx /conn_[a-z]+/",
        "Explicit regex enclosed in slashes",
    );
    push_cmd(
        &mut out,
        color,
        "grx re:^[A-Z][a-z]+",
        "Explicit regex prefixed with re:",
    );

    // 02. POSITIONAL DSL & PATH FILTERING
    push_section(
        &mut out,
        color,
        "02",
        "POSITIONAL DSL & ZERO-FLAG PATH FILTERING",
    );
    out.push_str(
        "  The first bare argument is ALWAYS the search pattern; filter paths directly:\n",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern src/",
        "Search 'pattern' inside src/ directory",
    );
    push_cmd(
        &mut out,
        color,
        "grx \"Steam/\"",
        "Literal search for 'Steam/' (not a path)",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern p:src/",
        "Scoped root path inclusion (p: or path:)",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern np:proj",
        "Shell-safe path exclusion: prunes any path with 'proj'",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern no:proj/",
        "Exclude directories containing 'proj/' (no glob needed)",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern no-path:target/",
        "Explicit namespace for directory exclusion",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern nt:c,h",
        "Exclude .c and .h files (or no-type:c,h)",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern ns:debug",
        "Exclude lines containing 'debug' (or no-str:debug)",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern :rs",
        "Include only Rust (*.rs) files",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern :web",
        "Include web files (*.html, *.css, *.js, *.ts)",
    );

    // 03. TRAVERSAL DEPTH LIMITS
    push_section(&mut out, color, "03", "DIRECTORY TRAVERSAL DEPTH LIMITS");
    out.push_str("  Control recursive directory descent cleanly without shell-glob headaches:\n");
    push_cmd(
        &mut out,
        color,
        "grx pattern d:0",
        "Current directory only (non-recursive)",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern d:1",
        "Descend at most 1 subdirectory level",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern depth:2 p:src/",
        "Search inside src/ up to 2 levels deep",
    );
    push_cmd(
        &mut out,
        color,
        "grx -d 1 pattern",
        "POSIX-style -d / --max-depth flag",
    );

    // 04. INLINE MATCH LIMITS & CONTEXT
    push_section(&mut out, color, "04", "INLINE MATCH LIMITS & CONTEXT");
    out.push_str("  Configure result limits and context lines directly inside the DSL query:\n");
    push_cmd(
        &mut out,
        color,
        "grx pattern top:10",
        "Limit maximum matching lines per file to 10",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern limit:5",
        "Alias for per-file top:5",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern head:10",
        "Limit total results globally across all files to 10",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern tail:5",
        "Limit total results globally to last 5 entries",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern ctx:3",
        "Show 3 lines of context before and after",
    );
    push_cmd(
        &mut out,
        color,
        "grx pattern top:5 ctx:2 :rs",
        "Compose match limits, context, and file types",
    );
    push_cmd(
        &mut out,
        color,
        "grx -m 10 -C 3 pattern",
        "POSIX-style flags (-m / --max-count, -C / --context)",
    );

    // 05. SHELL-SAFE WILDCARDS & GLOBS
    push_section(&mut out, color, "05", "SHELL-SAFE WILDCARDS & GLOBS");
    out.push_str("  Avoid shell wildcard collisions (* and ?) in Fish, Zsh, and Bash:\n");
    push_cmd(
        &mut out,
        color,
        "grx get..id :rs",
        "Unquoted double-dot wildcard (matches 'get_id', 'getUserById')",
    );
    push_cmd(
        &mut out,
        color,
        "grx fn..main :rs",
        "Matches 'fn main()', 'fn init_main(..)')",
    );
    push_cmd(
        &mut out,
        color,
        "grx 'fn *(&self)'",
        "Quoted glob (converted to regex with literals escaped)",
    );
    push_cmd(
        &mut out,
        color,
        "grx 'test_?_helper'",
        "Quoted single-char wildcard (matches 'test_1_helper')",
    );

    // 06. LINE ANCHORS & REGEX PATTERNS
    push_section(&mut out, color, "06", "LINE ANCHORS & REGEX PATTERNS");
    out.push_str("  Anchor matches to line boundaries (indentation-aware):\n");
    push_cmd(
        &mut out,
        color,
        "grx ^pub struct",
        "Lines beginning with 'pub struct' (after indentation)",
    );
    push_cmd(
        &mut out,
        color,
        "grx return..false$",
        "Lines ending with 'return false'",
    );
    push_cmd(
        &mut out,
        color,
        "grx -E \"^[A-Z][a-z]+\"",
        "Extended POSIX regex mode",
    );

    // 07. RICH BINARY & REVERSE ENGINEERING INSPECTION
    push_section(
        &mut out,
        color,
        "07",
        "RICH BINARY & REVERSE ENGINEERING INSPECTION",
    );
    out.push_str("  Inspect binaries safely with aligned hexdumps and string extractors:\n");
    push_cmd(
        &mut out,
        color,
        "grx ELF :bin",
        "Safely inspect ELF headers in binary files",
    );
    push_cmd(
        &mut out,
        color,
        "grx hex:7f454c46 :bin",
        "Search raw byte signatures in hexadecimal",
    );
    push_cmd(
        &mut out,
        color,
        "grx str:4 :bin",
        "Extract printable ASCII strings >= 4 chars with byte offsets",
    );
    push_cmd(
        &mut out,
        color,
        "grx strings:8 :bin",
        "Extract printable strings >= 8 chars",
    );

    // 08. SMART-CASE & CASE SENSITIVITY
    push_section(&mut out, color, "08", "SMART-CASE & CASE SENSITIVITY");
    out.push_str("  Intelligent case folding based on query casing:\n");
    push_cmd(
        &mut out,
        color,
        "grx connect",
        "Lowercase query: case-INSENSITIVE (matches 'Connect', etc.)",
    );
    push_cmd(
        &mut out,
        color,
        "grx Connect",
        "Mixed-case query: case-SENSITIVE (matches only 'Connect')",
    );
    push_cmd(
        &mut out,
        color,
        "grx -i pattern",
        "Force case-insensitive (or grx no:case)",
    );
    push_cmd(
        &mut out,
        color,
        "grx -s pattern",
        "Force case-sensitive (or grx yes:case)",
    );

    // 09. BOOLEAN SEARCH EXPRESSIONS
    push_section(&mut out, color, "09", "BOOLEAN SEARCH EXPRESSIONS");
    out.push_str("  Combine multiple terms on the same line with boolean logic:\n");
    push_cmd(
        &mut out,
        color,
        "grx auth AND token",
        "Line must contain BOTH 'auth' and 'token'",
    );
    push_cmd(
        &mut out,
        color,
        "grx error OR warning",
        "Line contains EITHER 'error' or 'warning'",
    );
    push_cmd(
        &mut out,
        color,
        "grx auth AND NOT expired",
        "Line contains 'auth' but NOT 'expired'",
    );

    // 10. UNARY LINE MODIFIERS (+ AND -)
    push_section(&mut out, color, "10", "UNARY LINE MODIFIERS (+ AND -)");
    out.push_str("  Filter lines with concise plus/minus prefixes:\n");
    push_cmd(
        &mut out,
        color,
        "grx auth +token",
        "Match 'auth', line must also contain 'token'",
    );
    push_cmd(
        &mut out,
        color,
        "grx auth -test",
        "Match 'auth', line must not contain 'test'",
    );
    push_cmd(
        &mut out,
        color,
        "grx auth ns:debug",
        "Explicit string negation namespace",
    );

    // 11. HIDDEN FILES & UNRESTRICTED MODES
    push_section(&mut out, color, "11", "HIDDEN FILES & UNRESTRICTED MODES");
    out.push_str("  Control traversal boundaries and ignore rules:\n");
    push_cmd(
        &mut out,
        color,
        "grx yes:dots 'config'",
        "Search inside hidden files (.env, .config/)",
    );
    push_cmd(&mut out, color, "grx -u", "Unrestricted: ignore .gitignore");
    push_cmd(
        &mut out,
        color,
        "grx -uu",
        "Unrestricted + search hidden files",
    );
    push_cmd(
        &mut out,
        color,
        "grx -uuu",
        "Unrestricted + hidden + search binary files",
    );

    // 12. LOCATION MARKERS, STATS & JSON
    push_section(&mut out, color, "12", "LOCATION MARKERS, STATS & JSON");
    out.push_str("  Precision output formatting for tooling and editors:\n");
    push_cmd(
        &mut out,
        color,
        "grx --column -b \"token\"",
        "1-based column numbers and 0-based byte offsets",
    );
    push_cmd(
        &mut out,
        color,
        "grx -c \"token\"",
        "Count of matching lines per file",
    );
    push_cmd(
        &mut out,
        color,
        "grx --count-matches \"token\"",
        "Total count of all matching token occurrences",
    );
    push_cmd(
        &mut out,
        color,
        "grx --stats \"token\"",
        "Search telemetry (duration, files, bytes scanned)",
    );
    push_cmd(
        &mut out,
        color,
        "grx --json \"token\"",
        "Stream ripgrep-compatible JSON Lines",
    );

    // 13. OPERATIONAL MODES & COMPATIBILITY
    push_section(&mut out, color, "13", "OPERATIONAL MODES & COMPATIBILITY");
    out.push_str("  Switch between search paradigms:\n");
    push_cmd(
        &mut out,
        color,
        "grx pattern p:src/ :rs",
        "Ergonomic DSL mode (default)",
    );
    push_cmd(
        &mut out,
        color,
        "grx --mode grep -i -E \"re\" path/",
        "Strict POSIX grep mode",
    );
    push_cmd(
        &mut out,
        color,
        "grx --mode git-grep pattern",
        "Git-grep compatibility mode",
    );

    // 14. CONFIGURATION & EDITOR INTEGRATION
    push_section(&mut out, color, "14", "CONFIGURATION & EDITOR INTEGRATION");
    out.push_str("  Manage preferences and editor integration:\n");
    push_cmd(
        &mut out,
        color,
        "grx --config",
        "Open active config in $VISUAL / $EDITOR",
    );
    push_cmd(
        &mut out,
        color,
        "grx --edit-config",
        "Launch system editor for ~/.config/grx/config.toml",
    );
    push_cmd(
        &mut out,
        color,
        "grx --no-messages pattern",
        "Suppress unreadable/nonexistent file warnings",
    );

    // 15. PROXIMITY SEARCH & LINE WINDOW CONSTRAINTS
    push_section(
        &mut out,
        color,
        "15",
        "PROXIMITY SEARCH & LINE WINDOW CONSTRAINTS",
    );
    out.push_str(
        "  Filter matches based on whether a secondary term appears within nearby lines:\n",
    );
    push_cmd(
        &mut out,
        color,
        "grx unsafe near:5,safety",
        "Match 'unsafe' only if 'safety' appears within 5 lines",
    );
    push_cmd(
        &mut out,
        color,
        "grx fn near:test",
        "Default 3-line proximity window (near:3,test)",
    );
    push_cmd(
        &mut out,
        color,
        "grx unsafe no-near:5,safety",
        "Inverted: match 'unsafe' with NO 'safety' within 5 lines",
    );
    push_cmd(
        &mut out,
        color,
        "grx auth NEAR:5 safety",
        "Infix boolean proximity operator",
    );
    push_cmd(
        &mut out,
        color,
        "grx auth NOT NEAR:5 test",
        "Infix NOT NEAR inverted proximity",
    );

    // 16. FUZZY TOKEN PERMUTATION SEARCH
    push_section(&mut out, color, "16", "FUZZY TOKEN PERMUTATION SEARCH");
    out.push_str(
        "  Match lines containing a set of tokens in any order without knowing exact order:\n",
    );
    push_cmd(
        &mut out,
        color,
        "grx fz:from,ptr,err",
        "Order-agnostic token search (matches any permutation)",
    );
    push_cmd(
        &mut out,
        color,
        "grx %%from_ptr_err",
        "Fast 2-key alias (auto-splits snake_case & camelCase)",
    );
    push_cmd(
        &mut out,
        color,
        "grx -Z 'string,from,\"\"'",
        "CLI flag with exact strings (or fz:'string,from,\"\"')",
    );
    push_cmd(
        &mut out,
        color,
        "grx %%'\"\",from,ptr'",
        "Shell-safe quote: outer quote prevents shell eating \"\"",
    );

    // 17. UNIFIED FILE DISCOVERY & ENTRY SELECTION
    push_section(
        &mut out,
        color,
        "17",
        "UNIFIED FILE DISCOVERY & ENTRY SELECTION",
    );
    out.push_str(
        "  Discover files, directories, and symlinks using the same selection language:\n",
    );
    push_cmd(
        &mut out,
        color,
        "grx in:report",
        "Discover entries whose basename contains 'report'",
    );
    push_cmd(
        &mut out,
        color,
        "grx in:=report.md p:Documents/",
        "Exact filename match under Documents/ directory",
    );
    push_cmd(
        &mut out,
        color,
        "grx in:^test in:rs$",
        "Basename anchors (^ starts with, $ ends with)",
    );
    push_cmd(
        &mut out,
        color,
        "grx in:annual..2026",
        "Basename wildcard (annual followed by 2026)",
    );
    push_cmd(
        &mut out,
        color,
        "grx in:report t:pdf",
        "Narrow by file type or extension (t: or type:)",
    );
    push_cmd(
        &mut out,
        color,
        "grx kind:dir in:cache",
        "Discover directories matching 'cache' (kind:dir)",
    );
    push_cmd(
        &mut out,
        color,
        "grx dir:src np:data",
        "Discover directories named 'src' excluding 'data'",
    );
    push_cmd(
        &mut out,
        color,
        "grx dir:=src tail:5",
        "Discover exact directory 'src' showing last 5 with colors",
    );
    push_cmd(
        &mut out,
        color,
        "grx in:test sort:size tail:10",
        "Sort entries by size and show last 10",
    );
    push_cmd(
        &mut out,
        color,
        "grx kind:link p:.",
        "Discover symbolic links (kind:link)",
    );
    push_cmd(
        &mut out,
        color,
        "grx t:log larger:10MiB",
        "Filter files larger than 10 MiB (larger: / smaller:)",
    );
    push_cmd(
        &mut out,
        color,
        "grx t:rs newer:7d",
        "Filter files modified within 7 days (newer: / older:)",
    );
    push_cmd(
        &mut out,
        color,
        "grx -0 in:report p:Documents/",
        "Output NUL-delimited entries for xargs -0",
    );

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

        // Handle quoted string literal
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

        // Regular whitespace-delimited token
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let token: String = chars[start..i].iter().collect();

        if token == "grx" {
            result.push_str("\x1b[1;37m");
            result.push_str(&token);
            result.push_str("\x1b[0m");
        } else if token.starts_with('-')
            && !token.starts_with("-@")
            && !token.starts_with("-w:")
            && !token.starts_with("-test")
        {
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
        } else if token.starts_with('@')
            || token.starts_with("-@")
            || token.starts_with("w:")
            || token.starts_with("-w:")
            || token.starts_with(':')
            || token.starts_with("p:")
            || token.starts_with("in:")
            || token.starts_with("np:")
            || token.starts_with("no-path:")
            || token.starts_with("no:")
            || token.starts_with("nt:")
            || token.starts_with("no-type:")
            || token.starts_with("ns:")
            || token.starts_with("no-str:")
            || token.starts_with("near:")
            || token.starts_with("no-near:")
            || token.starts_with("-near:")
            || token.starts_with("fz:")
            || token.starts_with("fuzzy:")
            || token.starts_with("%%")
            || token.starts_with("d:")
            || token.starts_with("depth:")
            || token.starts_with("top:")
            || token.starts_with("limit:")
            || token.starts_with("ctx:")
            || token.starts_with("str:")
            || token.starts_with("strings:")
            || token.starts_with("hex:")
            || token.starts_with("re:")
            || (token.starts_with('/') && token.ends_with('/') && token.len() >= 2)
            || token.starts_with("yes:")
            || token.starts_with('=')
            || token.starts_with('+')
            || (token.starts_with('-')
                && (token.starts_with("-@")
                    || token.starts_with("-w:")
                    || token.starts_with("-test")))
            || token.contains("..")
            || token.starts_with('^')
            || token.ends_with('$')
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
        assert!(plain.contains("EXACT MATCHING: WHOLE-WORD, LITERALS & EXACT LINES"));
        assert!(plain.contains("@impl"));
        assert!(plain.contains("w:impl"));
        assert!(plain.contains("-w impl"));
        assert!(plain.contains("-@simple"));
        assert!(plain.contains("-x \"impl Foo for Bar\""));
        assert!(plain.contains("\"Steam/\""));
        assert!(plain.contains("=fn()"));
        assert!(plain.contains("np:proj"));
        assert!(plain.contains("no:proj/"));
        assert!(plain.contains("d:0"));
        assert!(plain.contains("top:10"));
        assert!(plain.contains("ctx:3"));
        assert!(plain.contains("PROXIMITY SEARCH & LINE WINDOW CONSTRAINTS"));
        assert!(plain.contains("FUZZY TOKEN PERMUTATION SEARCH"));
        assert!(plain.contains("UNIFIED FILE DISCOVERY & ENTRY SELECTION"));
        assert!(plain.contains("in:report"));
        assert!(plain.contains("kind:dir"));
        assert!(plain.contains("near:5,safety"));
        assert!(plain.contains("%%from_ptr_err"));
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
        assert!(colored.contains("EXACT MATCHING: WHOLE-WORD, LITERALS & EXACT LINES"));
        assert!(colored.contains("PROXIMITY SEARCH & LINE WINDOW CONSTRAINTS"));
        assert!(colored.contains("FUZZY TOKEN PERMUTATION SEARCH"));
        assert!(colored.contains("UNIFIED FILE DISCOVERY & ENTRY SELECTION"));
        assert!(colored.contains("\x1b[1;36m")); // Cyan
        assert!(colored.contains("\x1b[1;32m")); // Green prompt
    }

    #[test]
    fn test_cli_head_tail_sort_flags() {
        let cli = Cli::try_parse_from(["grx", "--head", "5", "--sort", "size", "pattern"]).unwrap();
        assert_eq!(cli.head, Some(5));
        assert_eq!(cli.max_count, None);
        assert_eq!(cli.sort.as_deref(), Some("size"));
        assert!(!cli.reverse);

        let cli2 = Cli::try_parse_from(["grx", "--tail", "10", "--reverse"]).unwrap();
        assert_eq!(cli2.tail, Some(10));
        assert!(cli2.reverse);

        let cli3 = Cli::try_parse_from(["grx", "-m", "3", "--head", "5", "pattern"]).unwrap();
        assert_eq!(cli3.max_count, Some(3));
        assert_eq!(cli3.head, Some(5));
    }

    #[test]
    fn test_explicit_regex_tokens_highlighted_and_rendered_in_tutorial() {
        let highlighted_re = highlight_cmd("grx re:^[A-Z]+");
        assert!(highlighted_re.contains("\x1b[1;33mre:^[A-Z]+\x1b[0m"));

        let highlighted_slash = highlight_cmd("grx /pattern/");
        assert!(highlighted_slash.contains("\x1b[1;33m/pattern/\x1b[0m"));

        let tutorial = render_tutorial(false);
        assert!(tutorial.contains("/conn_[a-z]+/"));
        assert!(tutorial.contains("re:^[A-Z][a-z]+"));

        let mut help_buf = Vec::new();
        print_concise_help_to(&mut help_buf);
        let help_str = String::from_utf8_lossy(&help_buf);
        assert!(help_str.contains("/regex/, re:<regex>"));
    }

    #[test]
    fn test_cli_thresholds_actions_and_undo_flags() {
        let cli = Cli::try_parse_from([
            "grx",
            "--mmap-threshold",
            "1048576",
            "--max-file-size",
            "52428800",
            "--rename",
            "dest/{name}",
            "--chmod",
            "755",
            "--list",
            "pattern",
        ])
        .unwrap();

        assert_eq!(cli.mmap_threshold, Some(1048576));
        assert_eq!(cli.max_file_size, Some(52428800));
        assert_eq!(cli.rename.as_deref(), Some("dest/{name}"));
        assert_eq!(cli.chmod.as_deref(), Some("755"));
        assert!(cli.list);

        let cli_actions = Cli::try_parse_from([
            "grx",
            "--move",
            "dest_dir/",
            "--copy",
            "cp_dir/",
            "--trash",
            "--clean-trash",
            "pattern",
        ])
        .unwrap();
        assert_eq!(cli_actions.r#move, Some(PathBuf::from("dest_dir/")));
        assert_eq!(cli_actions.copy, Some(PathBuf::from("cp_dir/")));
        assert!(cli_actions.trash);
        assert!(cli_actions.clean_trash);

        let cli_aliases = Cli::try_parse_from([
            "grx",
            "--mv",
            "dest_dir/",
            "--cp",
            "cp_dir/",
            "--rm",
            "pattern",
        ])
        .unwrap();
        assert_eq!(cli_aliases.r#move, Some(PathBuf::from("dest_dir/")));
        assert_eq!(cli_aliases.copy, Some(PathBuf::from("cp_dir/")));
        assert!(cli_aliases.trash);

        let undo_cli = Cli::try_parse_from(["grx", "undo", "--list"]).unwrap();
        assert_eq!(undo_cli.args, vec!["undo", "--list"]);

        let list_cli = Cli::try_parse_from(["grx", "--list", "pattern"]).unwrap();
        assert!(list_cli.list);

        let undo_id_cli = Cli::try_parse_from(["grx", "undo", "tx-001"]).unwrap();
        assert_eq!(undo_id_cli.args, vec!["undo", "tx-001"]);
    }
}
