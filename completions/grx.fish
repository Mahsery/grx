# Fish completion script for grx (High-performance search CLI & DSL)
# Generated automatically by grx --completions fish

# Disable default indiscriminate file completion
complete -c grx -f

# Dynamic completion function for DSL selectors, filters, and prefixes
function __grx_complete_dsl
    set -l token (commandline -ct)

    # 1. Symlink selectors: link:, symlink:
    if string match -rq '^link:(?<val>.*)' -- $token
        for f in $val*
            test -L "$f" && printf "link:%s\tSymbolic link\n" "$f"
        end
        return 0
    else if string match -rq '^symlink:(?<val>.*)' -- $token
        for f in $val*
            test -L "$f" && printf "symlink:%s\tSymbolic link\n" "$f"
        end
        return 0
    # 2. Directory selectors: dir:, directory:
    else if string match -rq '^dir:(?<val>.*)' -- $token
        for d in $val*/
            if test -d "$d"
                set -l clean (string replace -r '^\./' '' "$d" | string replace -r '/$' '')
                not string match -rq '^\.\.?$' "$clean" && printf "dir:%s\tDirectory\n" "$clean"
            end
        end
        return 0
    else if string match -rq '^directory:(?<val>.*)' -- $token
        for d in $val*/
            if test -d "$d"
                set -l clean (string replace -r '^\./' '' "$d" | string replace -r '/$' '')
                not string match -rq '^\.\.?$' "$clean" && printf "directory:%s\tDirectory\n" "$clean"
            end
        end
        return 0
    # 3. File selector: file:
    else if string match -rq '^file:(?<val>.*)' -- $token
        for f in $val*
            test -f "$f" && printf "file:%s\tRegular file\n" "$f"
        end
        return 0
    # 4. Path selectors: p:, path:
    else if string match -rq '^p:(?<val>.*)' -- $token
        for p in $val*
            test -e "$p" && printf "p:%s\tStarting path\n" "$p"
        end
        return 0
    else if string match -rq '^path:(?<val>.*)' -- $token
        for p in $val*
            test -e "$p" && printf "path:%s\tStarting path\n" "$p"
        end
        return 0
    # 5. Exclude path: np:, no-path:
    else if string match -rq '^np:(?<val>.*)' -- $token
        for d in $val*/
            test -d "$d" && printf "np:%s\tExclude directory\n" "$d"
        end
        return 0
    else if string match -rq '^no-path:(?<val>.*)' -- $token
        for d in $val*/
            test -d "$d" && printf "no-path:%s\tExclude directory\n" "$d"
        end
        return 0
    # 6. Exclude directory or extension: no:
    else if string match -rq '^no:(?<val>.*)' -- $token
        for d in $val*/
            test -d "$d" && printf "no:%s\tExclude directory\n" "$d"
        end
        for ext in rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css txt web code data doc
            string match -q "$val*" -- $ext && printf "no:%s\tExclude file extension\n" "$ext"
        end
        return 0
    # 7. Sort keys: sort:, sortr:
    else if string match -rq '^sort:(?<val>.*)' -- $token
        set -l keys size bytes largest smallest modified time age date newest recent oldest path name len length path-len line-len linelen shortest longest line line-num linenum count -size -modified -len -count
        for k in $keys
            string match -q "$val*" -- $k && printf "sort:%s\tSort by %s\n" "$k" "$k"
        end
        return 0
    else if string match -rq '^sortr:(?<val>.*)' -- $token
        set -l keys size bytes largest smallest modified time age date newest recent oldest path name len length path-len line-len linelen shortest longest line line-num linenum count
        for k in $keys
            string match -q "$val*" -- $k && printf "sortr:%s\tReverse sort by %s\n" "$k" "$k"
        end
        return 0
    # 8. Type filters: t:, type:, nt:, no-type:
    else if string match -rq '^t:(?<val>.*)' -- $token
        for t in rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css web code data doc
            string match -q "$val*" -- $t && printf "t:%s\tFilter by type %s\n" "$t" "$t"
        end
        return 0
    else if string match -rq '^type:(?<val>.*)' -- $token
        for t in rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css web code data doc
            string match -q "$val*" -- $t && printf "type:%s\tFilter by type %s\n" "$t" "$t"
        end
        return 0
    else if string match -rq '^nt:(?<val>.*)' -- $token
        for t in rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css web code data doc
            string match -q "$val*" -- $t && printf "nt:%s\tExclude type %s\n" "$t" "$t"
        end
        return 0
    else if string match -rq '^no-type:(?<val>.*)' -- $token
        for t in rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css web code data doc
            string match -q "$val*" -- $t && printf "no-type:%s\tExclude type %s\n" "$t" "$t"
        end
        return 0
    # 9. Kind filter: kind:
    else if string match -rq '^kind:(?<val>.*)' -- $token
        for k in file dir link bin text
            string match -q "$val*" -- $k && printf "kind:%s\tSelect filesystem kind\n" "$k"
        end
        return 0
    # 10. Type shortcuts: :ext
    else if string match -rq '^:(?<val>.*)' -- $token
        for t in rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css web code data doc
            string match -q "$val*" -- $t && printf ":%s\tFilter filetype\n" "$t"
        end
        return 0
    # 11. Booleans and yes/no options
    else if string match -rq '^yes:(?<val>.*)' -- $token
        for y in dots bin case cache
            string match -q "$val*" -- $y && printf "yes:%s\tEnable option %s\n" "$y" "$y"
        end
        return 0
    # 12. Actions: mv:, cp:
    else if string match -rq '^mv:(?<val>.*)' -- $token
        for d in $val*/
            test -d "$d" && printf "mv:%s\tMove matching items to directory\n" "$d"
        end
        return 0
    else if string match -rq '^cp:(?<val>.*)' -- $token
        for d in $val*/
            test -d "$d" && printf "cp:%s\tCopy matching items to directory\n" "$d"
        end
        return 0
    end
end

# Positional arguments: files and directories (only when not completing a prefixed DSL selector)
function __grx_complete_paths
    set -l token (commandline -ct)
    if not string match -rq '^[^/:]+:' -- $token
        __fish_complete_path $token
    end
end

complete -c grx -a "(__grx_complete_paths)" -d "Target path"

# Dynamic DSL tokens
complete -c grx -a "(__grx_complete_dsl)"

# DSL static tokens & prefixes
complete -c grx -a "link:" -d "DSL: select symbolic link by basename"
complete -c grx -a "symlink:" -d "DSL: select symbolic link by basename"
complete -c grx -a "dir:" -d "DSL: select directory by basename"
complete -c grx -a "directory:" -d "DSL: select directory by basename"
complete -c grx -a "file:" -d "DSL: select regular file by basename"
complete -c grx -a "bin:" -d "DSL: select binary files (discovery or content)"
complete -c grx -a "p:" -d "DSL: root starting path or directory (p:src/)"
complete -c grx -a "path:" -d "DSL: root starting path or directory (path:src/)"
complete -c grx -a "in:report" -d "DSL: match entry basename (contains pattern)"
complete -c grx -a "in:=report.md" -d "DSL: match exact entry basename"
complete -c grx -a "in:^report" -d "DSL: match entry basename starting with pattern"
complete -c grx -a "in:report\\\$" -d "DSL: match entry basename ending with pattern"
complete -c grx -a "ni:report" -d "DSL: exclude entry basename (not-in:)"
complete -c grx -a "not-in:report" -d "DSL: exclude entry basename"
complete -c grx -a "t:rs" -d "DSL: filter by file type or extension (type:)"
complete -c grx -a "type:rs" -d "DSL: filter by file type or extension"
complete -c grx -a "kind:file" -d "DSL: select regular files (default)"
complete -c grx -a "kind:dir" -d "DSL: select directories"
complete -c grx -a "kind:link" -d "DSL: select symbolic links"
complete -c grx -a "kind:bin" -d "DSL: select binary files"
complete -c grx -a "kind:text" -d "DSL: select plain text files"
complete -c grx -a "larger:10MiB" -d "DSL: filter files larger than size threshold"
complete -c grx -a "smaller:1KiB" -d "DSL: filter files smaller than size threshold"
complete -c grx -a "newer:7d" -d "DSL: filter entries modified within age"
complete -c grx -a "older:30d" -d "DSL: filter entries modified before age"
complete -c grx -a "np:target/" -d "DSL: exclude directory path (no-path:)"
complete -c grx -a "no-path:target/" -d "DSL: exclude directory path"
complete -c grx -a "nt:rs" -d "DSL: exclude file type (no-type:)"
complete -c grx -a "no-type:rs" -d "DSL: exclude file type"
complete -c grx -a "ns:foo" -d "DSL: exclude lines matching string (no-str:)"
complete -c grx -a "no-str:foo" -d "DSL: exclude lines matching string"
complete -c grx -a "str:4" -d "DSL: extract printable strings (>= 4 chars) from binaries"
complete -c grx -a "strings:8" -d "DSL: extract printable strings (>= 8 chars) from binaries"
complete -c grx -a "near:3,pat" -d "DSL: proximity filter (pat must appear within N lines)"
complete -c grx -a "no-near:3,pat" -d "DSL: inverted proximity (pat must NOT appear within N lines)"
complete -c grx -a "fz:from,ptr,err" -d "DSL: fuzzy token permutation search"
complete -c grx -a "%%from_ptr_err" -d "DSL: fuzzy token permutation shortcut"
complete -c grx -a "re:regex" -d "DSL: explicit regular expression pattern"
complete -c grx -a "hex:7f454c" -d "DSL: hex byte signature search"
complete -c grx -a "sort:size" -d "DSL: sort by size ascending"
complete -c grx -a "sort:-size" -d "DSL: sort by size descending"
complete -c grx -a "sort:modified" -d "DSL: sort by modification time (newest first)"
complete -c grx -a "sort:-modified" -d "DSL: sort by modification time (oldest first)"
complete -c grx -a "sort:newest" -d "DSL: sort by newest modification time"
complete -c grx -a "sort:oldest" -d "DSL: sort by oldest modification time"
complete -c grx -a "sort:path" -d "DSL: sort alphabetically by path"
complete -c grx -a "sort:len" -d "DSL: sort by length (shortest first)"
complete -c grx -a "sort:-len" -d "DSL: sort by length (longest first)"
complete -c grx -a "sort:shortest" -d "DSL: sort by shortest length"
complete -c grx -a "sort:longest" -d "DSL: sort by longest length"
complete -c grx -a "sort:largest" -d "DSL: sort by largest size"
complete -c grx -a "sort:smallest" -d "DSL: sort by smallest size"
complete -c grx -a "sort:count" -d "DSL: sort by match count (highest first)"
complete -c grx -a "sort:-count" -d "DSL: sort by match count (lowest first)"
complete -c grx -a "sortr:size" -d "DSL: sort by size in reverse"
complete -c grx -a "sortr:modified" -d "DSL: sort by modification time in reverse"
complete -c grx -a "sortr:len" -d "DSL: sort by length in reverse"
complete -c grx -a "sortr:count" -d "DSL: sort by match count in reverse"
complete -c grx -a "head:10" -d "DSL: limit to first 10 results"
complete -c grx -a "tail:10" -d "DSL: limit to last 10 results"
complete -c grx -a "top:10" -d "DSL: limit maximum matching lines per file"
complete -c grx -a "limit:10" -d "DSL: limit maximum matching lines per file"
complete -c grx -a "d:0" -d "DSL: current directory only (non-recursive)"
complete -c grx -a "d:1" -d "DSL: descend at most 1 directory level"
complete -c grx -a "ctx:3" -d "DSL: show 3 lines of context"
complete -c grx -a "AND" -d "Boolean: match both expressions"
complete -c grx -a "OR" -d "Boolean: match either expression"
complete -c grx -a "NOT" -d "Boolean: invert subsequent expression"
complete -c grx -a "yes:dots" -d "Enable searching hidden files (.config, .bashrc)"
complete -c grx -a "no:dots" -d "Disable searching hidden files"
complete -c grx -a "yes:bin" -d "Enable searching binary files"
complete -c grx -a "no:bin" -d "Disable searching binary files"
complete -c grx -a "yes:case" -d "Force case-sensitive search"
complete -c grx -a "no:case" -d "Force case-insensitive search"
complete -c grx -a "yes:cache" -d "Include ephemeral cache directories (.cache/)"
complete -c grx -a "only:rust" -d "Exclusively search Rust files"
complete -c grx -a "only:bin" -d "Exclusively search binary files"
complete -c grx -a "mv:dest/" -d "DSL: move matching items to destination directory"
complete -c grx -a "cp:dest/" -d "DSL: copy matching items to destination directory"
complete -c grx -a "rm:" -d "DSL: safely remove/trash matching items"
complete -c grx -a "trash:" -d "DSL: safely remove/trash matching items"
complete -c grx -a "dry:" -d "DSL: simulate action without touching disk"
complete -c grx -a "rename:pattern" -d "DSL: rename matching items via pattern"
complete -c grx -a "chmod:755" -d "DSL: change permissions mode for matching items"
complete -c grx -a "NEAR:3" -d "DSL: infix proximity operator"
complete -c grx -a "undo" -d "Revert the latest filesystem mutation"

# Enums & Options
complete -c grx -l mode -x -a "dsl grep git-grep" -d "Operational search mode"
complete -c grx -l color -x -a "auto always never" -d "When to use colors"
complete -c grx -l hyperlinks -x -a "auto always never" -d "When to emit OSC 8 hyperlinks"
complete -c grx -s d -l max-depth -x -d "Maximum directory recursion depth"
complete -c grx -l sort -x -a "size bytes largest smallest modified time age date newest recent oldest path name len length path-len line-len linelen shortest longest line line-num linenum count" -d "Sort results by key"
complete -c grx -l reverse -d "Reverse sort ordering"
complete -c grx -l sort-reverse -d "Reverse sort ordering"
complete -c grx -l head -x -d "Limit results to first NUM entries"
complete -c grx -l tail -x -d "Limit results to last NUM entries"

# Type filter flags
complete -c grx -s t -l type -x -a "rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css web code data doc" -d "Filter by file type"
complete -c grx -s T -l type-not -x -a "rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css web code data doc" -d "Exclude files matching file type"

# Flags with arguments
complete -c grx -l config -r -F -d "Path to custom configuration file"
complete -c grx -s g -l glob -x -d "Include or exclude files matching glob"
complete -c grx -s m -l max-count -x -d "Stop reading after NUM matches"
complete -c grx -s M -l max-columns -x -d "Truncate lines longer than NUM characters"
complete -c grx -s C -l context -x -d "Print NUM lines of context"
complete -c grx -s B -l before-context -x -d "Print NUM lines before matches"
complete -c grx -s A -l after-context -x -d "Print NUM lines after matches"
complete -c grx -s j -l threads -x -d "Worker thread count (0 = auto)"
complete -c grx -s Z -l fuzzy -x -d "Fuzzy token permutation search (match lines containing tokens in any order)"

# Standard flags
complete -c grx -s e -l regexp -x -d "A pattern to search for (multiple combined with OR)"
complete -c grx -s f -l file -r -F -d "Obtain patterns from FILE, one per line"
complete -c grx -s i -l ignore-case -d "Case-insensitive search"
complete -c grx -s s -l case-sensitive -d "Force case-sensitive search"
complete -c grx -s S -l smart-case -d "Search case-insensitively if lowercase, case-sensitively otherwise"
complete -c grx -s v -l invert-match -d "Invert match: select non-matching lines"
complete -c grx -s w -l word-regexp -d "Match only whole words"
complete -c grx -s x -l line-regexp -d "Match only whole lines"
complete -c grx -s n -l line-number -d "Print 1-indexed line numbers"
complete -c grx -s N -l no-line-number -d "Suppress line numbers in match output"
complete -c grx -l column -d "Show 1-based column number for matches"
complete -c grx -l no-column -d "Suppress column numbers in match output"
complete -c grx -s b -l byte-offset -d "Print 0-based byte offset of matching lines or parts"
complete -c grx -l heading -d "Print file path heading above matching lines"
complete -c grx -l no-heading -d "Suppress file path headings"
complete -c grx -s p -l pretty -d "Pretty output: alias for --color always --heading --line-number"
complete -c grx -s u -l unrestricted -d "Reduce ignore filtering (-u: ignore gitignore, -uu: hidden, -uuu: binary)"
complete -c grx -s H -l with-filename -d "Print filename for each match"
complete -c grx -s I -l no-filename -d "Suppress file names in match output"
complete -c grx -s c -l count -d "Only print count of matching lines per file"
complete -c grx -l count-matches -d "Print total count of individual matches per file"
complete -c grx -l stats -d "Print aggregate traversal, matching, and timing statistics"
complete -c grx -l json -d "Output search results as a stream of JSON records"
complete -c grx -s l -l files-with-matches -d "Print names of files with matches"
complete -c grx -s L -l files-without-match -d "Print names of files without matches"
complete -c grx -s o -l only-matching -d "Show only matched parts of lines"
complete -c grx -s q -l quiet -d "Quiet mode: suppress output, exit 0 if match"
complete -c grx -l silent -d "Alias for --quiet"
complete -c grx -s F -l fixed-strings -d "Treat pattern as fixed literal string"
complete -c grx -s E -l extended-regexp -d "Treat pattern as extended regular expression"
complete -c grx -s r -s R -l recursive -d "Recursively search directories"
complete -c grx -s 0 -l null -d "Output zero byte (NUL) line terminator"
complete -c grx -l hidden -d "Search hidden files and directories"
complete -c grx -l no-ignore -d "Do not respect .gitignore and .ignore"
complete -c grx -l no-truncate -d "Do not truncate long lines in output"
complete -c grx -l follow -d "Follow directory symlinks"
complete -c grx -s a -l text -d "Search inside binary files as text"
complete -c grx -l binary -d "Alias for --text"
complete -c grx -l no-messages -d "Suppress error messages about nonexistent or unreadable files"
complete -c grx -l no-ignore-messages -d "Print error messages about unreadable files during traversal"
complete -c grx -l edit-config -d "Open configuration file in default editor"
complete -c grx -l exec -x -d "Execute command for each match ({}, {/}, {//}, {.})"
complete -c grx -s X -l exec-batch -x -d "Execute command once with all matches as arguments"
complete -c grx -l dry-run -d "Simulate file actions without modifying filesystem"
complete -c grx -l dry -d "Alias for --dry-run"
complete -c grx -l mmap-threshold -x -d "File size threshold for memory mapping (bytes)"
complete -c grx -l max-file-size -x -d "Maximum file size to inspect (bytes)"
complete -c grx -l move -x -a "(__fish_complete_directories)" -d "Move matching items into destination directory"
complete -c grx -l mv -x -a "(__fish_complete_directories)" -d "Alias for --move"
complete -c grx -l copy -x -a "(__fish_complete_directories)" -d "Copy matching items into destination directory"
complete -c grx -l cp -x -a "(__fish_complete_directories)" -d "Alias for --copy"
complete -c grx -l trash -d "Safely stage matching items into trash cache"
complete -c grx -l rm -d "Alias for --trash"
complete -c grx -l clean-trash -d "Purge trash staging cache permanently"
complete -c grx -l rename -x -d "Rename matching items using destination pattern"
complete -c grx -l chmod -x -d "Change permissions mode in octal (e.g. 755 or 644)"
complete -c grx -l list -d "List undo transaction history (with grx undo)"

# Management & utility flags
complete -c grx -l dump-config -d "Print default commented-out configuration template"
complete -c grx -l init-config -d "Initialize configuration in XDG config directory"
complete -c grx -l force -d "Overwrite existing configuration file"
complete -c grx -l config-path -d "Print path of active configuration file"
complete -c grx -l paths -d "Print all distro-appropriate storage paths"
complete -c grx -l journal -d "Enable append-only execution telemetry journaling"
complete -c grx -l completions -x -a "fish bash zsh" -d "Generate shell completion script"
complete -c grx -l install-completions -x -a "fish bash zsh" -d "Install completion script for active shell"
complete -c grx -s h -l help -d "Print concise help summary"
complete -c grx -l help-full -d "Print exhaustive help information"
complete -c grx -l help-all -d "Print exhaustive help information"
complete -c grx -l all -d "Modifier flag (e.g. --help --all)"
complete -c grx -l tutorial -d "Print interactive TLDR search DSL tutorial"
complete -c grx -s V -l version -d "Print version"
