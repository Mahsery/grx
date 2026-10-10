# grx semantic bug report

Investigated on 2026-10-10 against commit `40f2331` (`init`), grx 0.1.0.

## Method and environment

Direct invocations of `target/release/grx`, using piped bytes, existing repository files, Linux procfs, and configurations supplied through Bash process substitution. Expectations came from query meaning and CLI help; implementation inspection followed observed failures. No regression tests or fixes were written. File actions were exercised exclusively through `--dry-run`.

The release executable was rebuilt with `cargo build --offline --release --locked`. Its SHA-256 was `dc076a943f9f40e0d3a0aa77a072cf290cae7085d8cc2b7db67e957a30b97c89`.

- Platform: Linux x86-64, kernel `7.3.0-rc6-1-cachyos-rc`.
- Compiler: `rustc 1.101.0-nightly (8d1a76430 2026-10-06)`.
- Investigation started: `20:36:45 UTC`.
- Direct investigation and report review finished: `20:56:48 UTC` (20 minutes, 3 seconds; repository commit checks followed).
- Commands below run from the repository root in Bash. Set `GRX=target/release/grx` first.
- Exit codes refer to grx unless a timeout is explicitly indicated. `0` means selected results, `1` means no selected results, `2` means an error; the crash exited `134`.
- No standalone fixture directories or custom test harnesses were created. Configuration process substitutions are pipes, not saved files.
- `tk new` failed with `UNIQUE constraint failed: tasks.id (1555)`. The user explicitly authorized proceeding without task tracking for this investigation.

## Findings

There are 20 findings, grouped by behavior rather than by the number of failing invocations. High priority denotes crashes, silent wrong selection, corrupted match output, or misleading operation plans. Medium priority denotes narrower inconsistencies, ignored settings, and invalid structured output.

| ID | Priority | Finding |
| --- | --- | --- |
| B01 | High | Whole-word matching reverses the expected boundary behavior for punctuation |
| B02 | High | Glob exclusions do not exclude wildcard matches |
| B03 | Medium | Positive globs cannot select paths or directories and mishandle `?` |
| B04 | High | Combining `-t` and `-g` silently broadens selection |
| B05 | High | Hex context turns unrelated bytes into the reported match |
| B06 | Medium | A newline-containing literal changes meaning when combined with a true predicate |
| B07 | Medium | Case-insensitive literal matching is asymmetric for Unicode |
| B08 | Medium | Proximity filters ignore explicit case settings |
| B09 | Medium | Smart-case ignores explicit regex patterns |
| B10 | High | `-a` plus NUL-containing lines bypasses `-o` and position output |
| B11 | Medium | JSON text changes source bytes while retaining original byte offsets |
| B12 | High | Dry-run approves multiple sources with the same destination |
| B13 | Medium | Dry-run approves impossible destinations |
| B14 | High | Directory read failures silently truncate discovery with a success status |
| B15 | Medium | Changing mmap threshold loses readable procfs contents |
| B16 | High | Valid long Boolean expressions abort with stack overflow |
| B17 | High | Fuzzy span reconstruction has a severe performance cliff, including quiet mode |
| B18 | Medium | Compatibility dispatch leaks frontend options to the backend |
| B19 | Medium | Configuration selection and explicit overrides can be silently ignored |
| B20 | Medium | Maximum output columns are budgeted as bytes rather than characters |

### B01 — Whole-word matching is wrong for punctuation

```bash
printf '.\na.a\n . \n' | "$GRX" --color never -w lit:.
printf 'C++\n C++ \nxC++y\n' | "$GRX" --color never -w lit:C++
```

Expected: the standalone punctuation matches; punctuation embedded between word characters does not. For `C++`, the first two lines qualify and `xC++y` does not.

Actual: the first command prints only `(standard input):2:a.a` and exits `0`. The `C++` command prints nothing and exits `1`. Direct `rg -n -w -F` invocations on the identical inputs select lines 1 and 3 for `.`, and lines 1 and 2 for `C++`, consistent with checking the characters outside the match.

Cause: `src/engine.rs:2474` wraps patterns in `\b(?:pattern)\b`. Those boundaries require a transition between word and non-word characters; they do not express “neither neighboring character is a word character” when a pattern itself begins or ends with punctuation.

Related omission: `-w fz:needle` and `-x fz:needle` both admit `xneedley`. `apply_word_and_line_boundaries` returns immediately for fuzzy and hex patterns (`src/engine.rs:2459`). These combinations should either implement the requested boundary constraint or reject it explicitly.

### B02 — Negative glob patterns are treated as literal path text

```bash
"$GRX" --color never kind:file -g '!*.rs' root:src/dsl head:3 sort:path
"$GRX" --color never kind:file -g '!Cargo*' root:Cargo.toml
"$GRX" --color never kind:file -g '*.rs' -g '!src/dsl/*' root:src/dsl head:3 sort:path
```

Expected: all three searches have no selected files and exit `1`.

Actual: the first and third print `src/dsl/ast.rs`, `src/dsl/mod.rs`, and `src/dsl/parser.rs`; the second prints `Cargo.toml`. All exit `0`. Excluding the literal `!Cargo` does remove `Cargo.toml`, demonstrating that negation exists but wildcard interpretation is missing.

Cause: negative `-g` values become `EntryPredicate::Path`, using ordinary DSL pattern interpretation (`src/engine.rs:2199`); `*` is then literal text rather than a glob wildcard. This can select files that a user explicitly excluded, including when selection feeds file actions.

### B03 — Positive globs operate on basenames/types rather than paths

```bash
"$GRX" --color never kind:file -g 'src/*.rs' root:. head:3 sort:path
"$GRX" --color never kind:dir -g 'src*' root:. head:3
"$GRX" --color never kind:file -g 'Cargo.?oml' root:Cargo.toml
```

Expected: Rust paths under `src` are eligible; a glob can filter directory discovery; `Cargo.?oml` matches `Cargo.toml`.

Actual: the first and third return nothing, exit `1`; the directory query exits `2` with `Directories and links cannot have filetype, size, or content constraints`. The control `-g '*.rs' root:src` returns Rust files.

Cause: positive globs are represented as type predicates (`src/engine.rs:2185`). Type matching sees only an entry name (`src/ignore.rs:441`), and unknown type strings containing `?` deliberately treat that character literally (`src/engine.rs:342`). CLI help describes `-g` as filtering files and directories by glob.

### B04 — Combining `-t` and `-g` silently broadens selection

```bash
"$GRX" --color never kind:file -t rs -g '*.toml' root:src/dsl head:3 sort:path
"$GRX" --color never version -t rs -g '*.toml' root:Cargo.toml
"$GRX" --color never version -t rs root:Cargo.toml
"$GRX" --color never version t:rs -g '*.toml' root:Cargo.toml
```

Expected: the explicit positive `*.toml` glob should not admit Rust files from a Rust-only directory. The equivalent `-t rs` and `t:rs` type filters should agree when combined with the same glob.

Actual: the first command returns `src/dsl/ast.rs`, `src/dsl/mod.rs`, and `src/dsl/parser.rs`, exit `0`, although none matches the glob. The second prints five matching TOML lines and exits `0`. The last two print nothing and exit `1`.

Cause: CLI type filters and positive globs are chained into a single OR expression (`src/engine.rs:2180`), while DSL type filters are subsequently ANDed with it. The choice of equivalent type-filter syntax changes the selected set.

### B05 — Hex matching plus context reports the wrong content

```bash
printf 'a\nb\nc\n' | "$GRX" --color never hex:62 ctx:1
printf 'a\nb\nc\n' | "$GRX" --color never --json hex:62 ctx:1
```

Expected: byte `0x62` is `b`, on line 2 at byte offset 2. Context can add `a` and `c`, but must preserve `b` as the match.

Actual plain output, exit `0`:

```text
(standard input)-2-b
(standard input):3:c
```

The JSON match record identifies `c`, at absolute offset 4, as the matched byte. The actual `b` becomes context. Without `ctx:1`, the match text is correctly `b`, but its `line_number` is incorrectly 3.

Cause: `HexMatcher` assigns `offset + 1` as a line number and supplies the matched byte slice as the record (`src/search/matcher.rs:459`). Context expansion interprets that field as an actual line index and substitutes another line (`src/search/context.rs:62`).

Related count error: `printf 'bbb\n' | "$GRX" --color never -c hex:62` prints `3`; `-c lit:b` prints `1`. The CLI promises matching-line counts, but each hex occurrence is treated as a separate line. A hex mode with byte-oriented records needs explicit handling for line-oriented options.

### B06 — Adding a true predicate removes a newline-containing literal match

```bash
printf 'alpha\nbeta\n' | "$GRX" --color never -o $'lit:alpha\nbeta'
printf 'alpha\nbeta\n' | "$GRX" --color never -o $'lit:alpha\nbeta' AND lit:
```

Expected: ANDing a query with the empty literal, which matches every line, must not change whether the query succeeds. If cross-line literals are unsupported, reject them consistently.

Actual: the first command exits `0` and prints only `(standard input):1:alpha`, truncating the supplied literal before its newline. The second prints nothing and exits `1`.

Cause: a single literal searches the entire buffer, then clamps its span to one line (`src/search/matcher.rs:207`). A Boolean expression evaluates each line separately (`src/search/boolean.rs:290`). Carriage-return literals against CRLF data show the same difference between the two execution paths.

### B07 — Unicode case-insensitive matching is asymmetric

```bash
printf 'k\nK\n' | "$GRX" --color never -i lit:k
printf 'k\nK\n' | "$GRX" --color never -i lit:K
printf 'k\nK\n' | "$GRX" --color never -i -w lit:k
```

Expected: the same case-equivalent characters should match in either direction. Adding a word constraint must not introduce a new case-equivalence relation.

Actual: the first command selects only `k`; the second and third select both `k` and `K`. All exit `0`. The same asymmetry occurs with `s`/`ſ`.

Cause: ASCII needles use ASCII folding; non-ASCII needles use a Unicode case-insensitive regex (`src/search/matcher.rs:77`). Applying `-w` also converts the literal into a regex, changing the folding behavior.

### B08 — Proximity filters ignore `-i`, `-s`, and configured case behavior

```bash
printf 'needle\nupper\n' | "$GRX" --color never -i needle near:1,UPPER
printf 'needle\nUPPER\n' | "$GRX" --color never -s needle near:1,upper
printf 'needle\nupper\n' | "$GRX" --color never -i needle no-near:1,UPPER
```

Expected: the first query finds the neighbor under `-i`; the second rejects the differently cased neighbor under `-s`; the third rejects the primary match because the forbidden neighbor exists under `-i`.

Actual: the first exits `1` without output; the second prints both lines and exits `0`; the third prints `needle` and exits `0`.

Cause: proximity matchers unconditionally use smart-case (`src/search/boolean.rs:235`). Explicit and configured case settings applied to the main expression never reach the proximity terms.

### B09 — `-S` does not apply smart-case to explicit regexes

```bash
printf 'NEEDLE\nneedle\n' | "$GRX" --color never -S re:^needle
printf 'NEEDLE\nneedle\n' | "$GRX" --color never -S '^needle'
printf 'NEEDLE\nneedle\n' | "$GRX" --color never -i re:^needle
```

Expected: the lowercase pattern under explicitly requested smart-case selects both lines.

Actual: the first selects only line 2. The equivalent DSL anchor and the explicit ignore-case control select both lines. All exit `0`.

Cause: `set_smart_case` ignores the regex variant (`src/engine.rs:2437`). This affects `-E` regex patterns as well.

### B10 — NUL-containing text bypasses extraction and positions

```bash
printf 'xx\000needleZZ\n' | "$GRX" --color never -a -o -I -N needle | od -An -tx1
printf 'xxneedleZZ\n' | "$GRX" --color never -a -o -I -N needle | od -An -tx1
printf 'xx\000needleZZ\n' | "$GRX" --color never -a -n --column -b needle
```

Expected: both `-o` commands output only `needle\n`; `-a` requests text treatment, so unrelated bytes should not bypass extraction. Explicit position flags should retain their fields.

Actual: the first outputs the entire `xx\0needleZZ\n` line, including the NUL. The second outputs only `needle\n`. The third outputs `(standard input): xx\0needleZZ` without line, column, or byte position. grx exits `0` in all cases.

Cause: the formatted-output branches detect a NUL and write the whole line before processing `only_matching` or position fields (`src/printer/content.rs:484`, `src/printer/content.rs:694`). Even raw text mode takes that branch.

### B11 — JSON payloads do not preserve the bytes addressed by their offsets

```bash
printf '\377needle\n' | "$GRX" --color never --json -a needle
printf 'needle' | "$GRX" --color never --json needle
printf 'needle\r\nneedle\r\n' | "$GRX" --color never --json needle
```

Expected: consumers must be able to recover source bytes and use `submatches.start/end` to obtain the reported match. Line payloads should preserve whether a terminator existed and which terminator it was.

Actual:

- Invalid byte `0xff` becomes UTF-8 replacement character `�`; no base64 byte representation is supplied. The submatch still uses original byte range `[1,7)`. Slicing the emitted UTF-8 payload at those offsets gives `b'\xbf\xbdneed'`, while `match.text` says `needle`.
- Unterminated input `needle` is emitted as `lines.text = "needle\n"`, despite `bytes_searched = 6`.
- CRLF input is emitted with LF-only payloads. Match offsets are 0 and 8, but each emitted line encodes to 7 bytes.

Cause: the printer applies `String::from_utf8_lossy` and unconditionally appends `\n` (`src/printer/content.rs:311`, `src/printer/content.rs:341`), while offsets refer to the original buffer. Filename JSON already supports base64; content JSON does not.

Additional accounting inconsistency: `printf 'one\nneedle\nthree\n' | "$GRX" --color never --json needle ctx:1` reports 16 `bytes_searched` in the end record and 17 in the summary. For stdin, per-file bytes are inferred from the last emitted record, rather than the inspected input size (`src/printer/content.rs:349`).

### B12 — Dry-run does not detect destination collisions within a batch

```bash
"$GRX" --color never kind:file =mod.rs root:src/dsl root:src/printer --copy .cargo --dry-run
"$GRX" --color never kind:file =mod.rs root:src/dsl root:src/printer --move .cargo --dry-run
```

Expected: two distinct sources cannot safely occupy the same `.cargo/mod.rs` destination without an explicit collision policy. The planner should identify the conflict before execution.

Actual: both commands exit `0` and claim `0 conflicts`, with these two operations:

```text
src/dsl/mod.rs     -> .cargo/mod.rs
src/printer/mod.rs -> .cargo/mod.rs
```

No `--force` was supplied. The destination did not exist. Planning checks only the current filesystem (`src/ops.rs:286`, `src/ops.rs:338`) and never checks uniqueness among destinations in the plan. Inspection of `execute_plan` shows that a later operation backs up an already-created destination and proceeds, regardless of whether `--force` was supplied (`src/ops.rs:508`, `src/ops.rs:552`). Thus a collision introduced by the same batch is handled as an overwrite.

Only the misleading plans were executed during this investigation; actual movement/copying and undo were not exercised.

### B13 — Dry-run approves impossible targets

```bash
"$GRX" --color never kind:file =Cargo.toml root:Cargo.toml --copy README.md --dry-run
"$GRX" --color never kind:file =Cargo.toml root:Cargo.toml --move README.md --dry-run
"$GRX" --color never kind:file =Cargo.toml root:Cargo.toml rename:Cargo.toml/ --dry-run
```

Expected: reject a regular file used as a destination directory, and reject a replacement that yields an empty filename.

Actual: every command exits `0` and claims `0 conflicts`. Copy/move plan `README.md/Cargo.toml`, even though `README.md` is a regular file. Rename prints a blank destination after the arrow.

Cause: copy/move validate destination existence only at the final joined path; they do not validate that the destination parent is a directory (`src/ops.rs:286`, `src/ops.rs:338`). Rename accepts an empty replacement and builds an empty relative destination (`src/ops.rs:388`).

Related plan inconsistency: copying or moving `Cargo.toml` into `.` with `--force --dry-run` approves `Cargo.toml -> Cargo.toml`. Same-path checks compare the source with the destination directory rather than the final destination file. Execution would attempt to back up that same source and then use its now-missing path; this was inspected in source, not executed.

### B14 — Linux directory errors produce silently incomplete results

```bash
"$GRX" --color never --config <(printf '[search]\nwalker-buffer-size-bytes = 32\n') kind:file root:src d:0 --no-ignore-messages sort:path
"$GRX" --color never kind:file root:src d:0 --no-ignore-messages sort:path
"$GRX" --color never --config <(printf '[search]\nwalker-buffer-size-bytes = 1\n') kind:file root:src/dsl --no-ignore-messages
```

Expected: changing a traversal buffer cannot silently change the inventory. An insufficient buffer must be grown or reported as an error, rather than interpreted as completion.

Actual: the 32-byte configuration returns only 8 of the 15 top-level Rust files and exits `0`. Missing files include `src/engine.rs`, `src/config.rs`, and `src/completions.rs`. A 64-byte buffer returns the full inventory. With a 1-byte buffer, an existing nonempty directory returns no entries, exit `1`, and no diagnostic even with `--no-ignore-messages`.

Cause: `getdents64` requires enough space for the next directory entry. Its non-EINTR errors are returned as `Ok(())` (`src/walker/linux.rs:98`). Directory open failures are likewise returned as success (`src/walker/linux.rs:55`).

Second direct reproduction on this machine: `kind:file root:/proc/1/fd --no-ignore-messages` exits `1` silently although the directory cannot be read by this user. The comparable unreadable file `/proc/1/mem` with the same diagnostic flag correctly exits `2` and reports permission denial. The procfs permission reproduction depends on user privileges; the small-buffer case is self-contained in the checkout.

### B15 — Mmap threshold changes whether procfs contents exist

```bash
"$GRX" --color never Linux root:/proc/version
"$GRX" --color never Linux root:/proc/version --mmap-threshold 0
"$GRX" --color never Linux root:/proc/version --mmap-threshold 1
```

Expected: reader strategy changes should not change the searchable bytes of an unchanged, readable regular file.

Actual: the default command prints the kernel version and exits `0`; thresholds 0 and 1 print nothing and exit `1`.

Cause: procfs reports a metadata size of zero despite readable contents. Once a read exceeds the initial threshold, `AdaptiveReader` trusts that metadata and returns an empty buffer (`src/reader.rs:62`), losing the probe bytes it already read. The existing procfs reader test covers only the default 64 KiB threshold.

### B16 — Long valid Boolean expressions abort the process

```bash
ulimit -c 0
args=()
for ((i=0; i<4000; i++)); do args+=(NOT); done
printf 'alpha\nbeta\n' | "$GRX" --color never "${args[@]}" alpha
```

Expected: an even number of negations is equivalent to `alpha`. The command should return line 1, or reject excessive expression complexity with exit `2`.

Actual: exit `134`, with:

```text
thread 'main' (...) has overflowed its stack
fatal runtime error: stack overflow, aborting
```

The same query with 2,000 negations succeeds. 6,000 and 10,000 negations also abort. A 10,000-operator OR chain aborts, while a 2,000-operator OR chain returns the expected no-match status.

Source exposes unbounded recursive negation parsing (`src/dsl/parser.rs:1199`) and recursive AST traversal/compilation. The observations do not identify the exact stack-overflow frame; there is no backtrace from this abort. A complexity limit or iterative processing must cover subsequent AST operations as well as parsing.

### B17 — Fuzzy matching becomes impractically slow on a few kilobytes

```bash
TIMEFORMAT='elapsed=%R user=%U'
pattern=$(printf '%4000s' '' | tr ' ' a)
time printf '%s\n' "$pattern" | timeout 30 "$GRX" --color never -q "fz:$pattern"
time printf '%s\n' "$pattern" | timeout 30 "$GRX" --color never -q "lit:$pattern"
```

Expected: an exact repetitive subsequence on a single 4 KB line should complete without this level of computation. Quiet mode only needs existence.

Actual fuzzy command times out with status `124`, using about 29.9 CPU seconds. The literal control exits `0` in 0.004 seconds. Measurements at smaller sizes, each pattern matching an equally sized all-`a` line:

| Pattern/input length | Invocation | Elapsed seconds | CPU seconds | Result |
| --- | --- | --- | --- | --- |
| 500 | `-o fz:...` | 0.169 | 0.167 | exit 0 |
| 1,000 | `-o fz:...` | 1.306 | 1.286 | exit 0 |
| 2,000 | `-o fz:...` | 9.814 | 9.761 | exit 0 |
| 1,000 | `-q fz:...` | 1.255 | 1.243 | exit 0 |
| 2,000 | `-q fz:...` | 9.495 | 9.450 | exit 0 |
| 4,000 | `-q fz:...` | 30.003 | 29.869 | timeout |

Cause: reconstructing selected fuzzy character spans calls the dynamic-programming search again for each successively shorter prefix (`src/search/fuzzy.rs:75`). On these inputs that implies cubic work. Quiet searches still build those spans through `BufferSearch`, then compute a fuzzy score again (`src/engine.rs:1535`). Times are observations on this machine, not a general throughput benchmark.

The same 4,000-character identity query under `--count-matches` also timed out after 30.002 seconds (29.765 CPU seconds), without producing its count. A 3,000-character nonmatching query returned `1` in 0.020 seconds; the expensive path is successful-match reconstruction, not simply argument size or process startup.

### B18 — Compatibility mode breaks when frontend options are not first

```bash
printf 'Needle\nother\n' | "$GRX" --mode grep -i needle
printf 'Needle\nother\n' | "$GRX" -i needle --mode grep
printf 'Needle\nother\n' | "$GRX" --config <(printf 'mode="grep"\n') -i needle
```

Expected: relocating a global `--mode` option should preserve the requested mode. A configuration flag used to select that mode should be consumed by grx.

Actual: the first command prints `Needle`, exit `0`. The second exits `2` because GNU grep receives and rejects `--mode`. The third exits `2` because GNU grep receives and rejects `--config`. The equals form `--mode=grep` has the same positional failure.

Cause: the early dispatch strips mode options only when they lead the arguments (`src/compat.rs:20`). Dispatch after parsing forwards the original argument vector without removing frontend-only options (`src/main.rs:64`, `src/compat.rs:40`).

### B19 — Selected configurations and explicit syntax overrides can be ignored

Two independently reproducible configuration failures:

**Explicit `-E` cannot override a configured fixed-string default.**

```bash
printf 'abc\n[a-z]+\n' | "$GRX" --color never --config <(printf '[search]\nfixed-strings = true\n') -E '[a-z]+'
```

Expected: the explicit extended-regex option selects regex interpretation and matches `abc`.

Actual: only the literal `[a-z]+` line is selected, exit `0`. The same invocation with `fixed-strings = false` selects both lines. The `pattern_syntax` decision checks the configured fixed-string default before the explicit `-E` flag (`src/engine.rs:2075`).

**An invalid environment-selected config is silently replaced by defaults.**

```bash
GRX_CONFIG=README.md "$GRX" --config-path
GRX_CONFIG=README.md "$GRX" --color never kind:file =Cargo.toml root:Cargo.toml
"$GRX" --config README.md --color never kind:file =Cargo.toml root:Cargo.toml
```

Expected: a selected configuration that cannot be parsed should produce a configuration error, consistently with explicit `--config`.

Actual: `--config-path` reports `README.md`; the environment-configured search succeeds, prints `Cargo.toml`, and exits `0` without mentioning the invalid TOML. The explicit-config control reports a TOML parse error and exits `2`. A nonexistent `GRX_CONFIG` path is also silently ignored.

Cause: configuration discovery locates the environment-specified file, but `load_from_paths` silently discards parsing errors (`src/config.rs:359`); `load_explicit` propagates them. The reported active config can therefore be a file whose contents were never used.

### B20 — Maximum columns counts UTF-8 bytes rather than characters

```bash
printf 'ééé\n' | "$GRX" --color never -M 2 lit:
printf 'eee\n' | "$GRX" --color never -M 2 lit:
```

Expected: the help promises truncation after NUM characters. Both inputs have three characters and should retain two at a limit of 2.

Actual: the Unicode input retains only one `é`, followed by `... [omitted 4 bytes]`; the ASCII input retains two `e` characters. Both exit `0`. `--no-truncate` correctly restores the entire Unicode line.

Cause: `format_line_content_with_kinds` compares the UTF-8 byte length with the column limit and selects a byte window (`src/printer/content.rs:43`). Rounding that window to character boundaries avoids invalid UTF-8 but does not implement the documented character budget. These Latin characters each occupy one ordinary display column, so wide-character width is not responsible.

## Controls, scope, and follow-up

Basic AND/OR/NOT, ordinary grouped-expression precedence, double negation, contradictory selectors, duplicate/overlapping roots, type aliases, explicit type exclusions, zero result limits, negative/out-of-range `nth`, context retained around selected ordinary matches, malformed regex/hex rejection, and numeric size overflow rejection behaved as expected in the exercised cases.

Repository checks passed: formatting, offline Clippy for all targets, and the existing offline Cargo test suite (178 library unit tests, 121 integration tests, 10 golden compatibility tests, plus binary and doc test targets). These were housekeeping checks after the direct investigation, not the source of its semantic expectations.

For concurrency controls, sorted content output was identical over 20,276 repeated invocations with 8 workers. A second three-minute probe ran 61,192 invocations, alternating 1, 2, 8, and 32 workers, with sorted file selection and a five-file limit; every output had the same SHA-256. These checks establish consistency for those particular queries and this corpus, not general race freedom.

Priority for fixes: incorrect glob selection, directory error propagation, hex/context corruption, and action-plan collisions; then whole-word/binary extraction, crash limits, and fuzzy reconstruction; then remaining case/configuration/JSON inconsistencies. Each reproducer can become a black-box integration regression using independent expected bytes or selected filenames. Passing the current suite does not invalidate these observed CLI failures.

Actual mutating actions, crash recovery/undo, adversarial filenames requiring new fixtures, and Windows behavior remain untested. Self-copy execution consequences were inspected in source only. The report does not claim observed data loss.
