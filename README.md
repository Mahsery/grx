# grx

A file search and discovery CLI built around a compact inline query syntax.

> **Note**: This tool was built primarily for my own personal workflow and daily use. It does not claim superiority over established tools like `ripgrep` or `fd`. If you already have muscle memory and scripts around `rg` + `fd`, you should probably stick with them. `grx` exists because I wanted a single tool that combines file finding, content search, and safe reversible file operations with short inline filters.

---

## Quick Examples

### Search File Contents
```bash
# Search for 'auth' in Rust files under src/
grx auth p:src/ t:rs

# Case-insensitive search, excluding test files
grx token -i ni:test

# Proximity search: find lines where 'unsafe' appears within 5 lines of 'pointer'
grx unsafe near:5,pointer
```

### Find Files / Directories by Name
```bash
# Find all PDF reports modified in the last 7 days
grx in:report t:pdf newer:7d

# Find all directories named 'cache'
grx kind:dir cache

# Find regular files named like 'report' (use =report.md for an exact name)
grx kind:file report
```

### File Operations (with built-in undo)
```bash
# Preview moving matching files
grx in:test t:txt dry: mv:backup/

# Move matching files
grx in:test t:txt mv:backup/

# Undo the last action
grx undo
```

---

## Common Query Syntax

Instead of chaining multiple command-line flags, `grx` accepts short filter tokens inline:

| Filter | Example | What it does |
| :--- | :--- | :--- |
| `p:<path>` | `p:src/` | Search root / starting directory |
| `t:<type>` | `t:rs`, `t:py`, `t:md` | Include file type or extension |
| `in:<name>` | `in:report` | Match entry filename / basename |
| `ni:<name>` | `ni:test` | Exclude entry filename / basename |
| `np:<dir>` | `np:target/` | Exclude directory path segment |
| `newer:<age>` | `newer:24h`, `newer:7d` | Modified within duration |
| `older:<age>` | `older:30d` | Modified before duration |
| `larger:<size>` | `larger:10MB` | File size threshold |
| `kind:<type>` | `kind:file`, `kind:bin` | Select file, directory, link, binary, or text entries |
| `near:<N>,<pat>` | `near:5,token` | Pattern must appear within N lines |
| `mv:<dir>`, `cp:<dir>` | `mv:dest/`, `cp:backup/` | Move or copy discovered files |
| `trash:` | `trash:` | Stage matching files into undoable trash |
| `dry:` | `dry:` | Preview planned file operations without modifying disk |

Standard flags (`-i`, `-w`, `-F`, `-C <N>`, `-l`, `-c`, `-j <threads>`, etc.) work as expected alongside the inline tokens.

---

## How It Works

- **Parsing**: `grx report` searches file contents. `grx kind:file report` finds file names containing `report`; `grx report kind:file` searches contents only in regular files. This order also distinguishes name discovery from content search for `kind:bin` and `kind:text`. `kind:dir` and `kind:link` support name discovery; placing them after a content pattern is an error. Use `-e` before `kind:` to request content search explicitly, and `in:` to filter basenames in either mode.
- **Match Display**: Colored discovery results highlight the matching part of each filename with a contrasting background. Content matches keep their own highlight color. Configure them separately with `output.colors.name-match-highlight` and `output.colors.match-highlight`.
- **Directory Traversal**: Uses a work-stealing thread pool (`crossbeam-deque`) respecting `.gitignore` rules. On Linux, it queries directory entries directly using the `SYS_getdents64` syscall to reduce libc overhead.
- **Search Core**: Files under 64 KB are read via buffered streaming; larger files use memory mapping (`memmap2`). Fast byte scanning (`memchr`) rejects non-matching lines before invoking regular expressions.
- **Undo Log**: Mutating operations (`mv:`, `cp:`, `trash:`) write an action record to `~/.local/share/grx/` before touching files, allowing `grx undo` to restore or move items back.

---

## Benchmark

Tested against `ripgrep` on a warm repository tree (Linux x86_64, `python3 scripts/bench_comparison.py`):

| Scenario | `grx` | `ripgrep` | Notes |
| :--- | :--- | :--- | :--- |
| **Repository Search** (`auth t:rs`) | **2.9 ms** | 4.7 ms | Competitive on smaller, warm codebases |
| **Exclusion Query** (`auth np:target/`) | 6.0 ms | **4.5 ms** | Similar performance |
| **50 MB Corpus (1,000 files)** | 14.5 ms | **7.1 ms** | `ripgrep` is faster on large bulk scans |

*These are historical local measurements, not a benchmark of version 0.2.0. `ripgrep` was faster on the bulk scan in that run.*

---

## Installation

```bash
cargo install --path .
```

### Shell Completions

```bash
grx --install-completions
# or generate manually:
grx --completions fish > ~/.config/fish/completions/grx.fish
```

---

## License

Dual-licensed under either:
- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
