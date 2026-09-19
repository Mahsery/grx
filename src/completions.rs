use std::fs;
use std::io;
use std::path::PathBuf;

/// Generate fully functional completions for fish shell.
pub fn generate_fish_completions() -> String {
    r#"# Fish completion script for grx (High-performance search CLI & DSL)
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
"#
    .to_string()
}

/// Generate bash completions script.
pub fn generate_bash_completions() -> String {
    r#"# Bash completion script for grx
# Generated automatically by grx --completions bash

_grx_completions() {
    local cur prev words cword
    _init_completion || return

    local options="
        --help -h -V --version --help-full --help-all --all --tutorial --mode --config --dump-config --init-config --force
        --config-path --paths --journal --completions --install-completions
        -e --regexp -f --file
        -i --ignore-case -s --case-sensitive -S --smart-case -v --invert-match -w --word-regexp
        -x --line-regexp -n --line-number -N --no-line-number --column --no-column -b --byte-offset
        --heading --no-heading -p --pretty -u --unrestricted
        -H --with-filename -I --no-filename -d --max-depth
        -c --count --count-matches --stats --json -l --files-with-matches -L --files-without-match -m --max-count
        -M --max-columns --no-truncate
        -o --only-matching -q --quiet --silent -F --fixed-strings -E --extended-regexp
        -C --context -B --before-context -A --after-context -r -R --recursive
        -0 --null -Z --fuzzy -t --type -T --type-not -g --glob --hidden --no-ignore
        --follow -a --text --binary -j --threads --color --hyperlinks
        --no-messages --no-ignore-messages --edit-config
        --sort --reverse --sort-reverse --head --tail
        --exec -X --exec-batch --dry-run --dry --mmap-threshold --max-file-size
        --move --mv --copy --cp --trash --rm --clean-trash --rename --chmod --list
    "

    local dsl_tokens="
        link: symlink: dir: directory: file: bin:
        p: path: np: no-path: nt: no-type: ns: no-str:
        t: type: kind:file kind:dir kind:link kind:bin kind:text
        sort: sortr:
        head:10 tail:10 top:10 limit:10 ctx:3
        d:0 d:1 d:2
        larger:10MiB smaller:1KiB newer:7d older:30d
        yes:dots no:dots yes:bin no:bin yes:case no:case yes:cache
        only:rust only:bin
        AND OR NOT
        str:4 strings:8 near:3, no-near:3, NEAR:3 fz: hex: re:
        mv: cp: rm: trash: dry: rename: chmod: undo
    "

    case "$prev" in
        --mode)
            COMPREPLY=( $(compgen -W "dsl grep git-grep" -- "$cur") )
            return 0
            ;;
        --color|--hyperlinks)
            COMPREPLY=( $(compgen -W "auto always never" -- "$cur") )
            return 0
            ;;
        --sort)
            COMPREPLY=( $(compgen -W "time modified date size bytes name path len count matches" -- "$cur") )
            return 0
            ;;
        -t|--type|-T|--type-not)
            COMPREPLY=( $(compgen -W "rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css web code data doc" -- "$cur") )
            return 0
            ;;
        -d|--max-depth|-m|--max-count|-C|--context|-B|--before-context|-A|--after-context|-Z|--fuzzy|--head|--tail|-M|--max-columns|-j|--threads|--exec|-X|--exec-batch|--mmap-threshold|--max-file-size|--rename|--chmod)
            return 0
            ;;
        --completions|--install-completions)
            COMPREPLY=( $(compgen -W "fish bash zsh" -- "$cur") )
            return 0
            ;;
        --config|-f|--file|--move|--mv|--copy|--cp)
            _filedir
            return 0
            ;;
    esac

    if [[ "$cur" == -* ]]; then
        COMPREPLY=( $(compgen -W "$options" -- "$cur") )
        return 0
    elif [[ "$cur" == :* ]]; then
        local types=":rs :c :cpp :py :go :toml :json :yaml :md :web :code :data :doc"
        COMPREPLY=( $(compgen -W "$types" -- "$cur") )
        return 0
    elif [[ "$cur" == no:* ]]; then
        local prefix="${cur#no:}"
        local matches=()
        for d in "$prefix"*/; do
            [[ -d "$d" ]] && matches+=("no:$d")
        done
        for ext in c cpp h hpp rs py go toml json yaml md js ts html css txt sh zig lua java; do
            [[ "no:$ext" == "$cur"* ]] && matches+=("no:$ext")
        done
        COMPREPLY=( "${matches[@]}" )
        return 0
    elif [[ "$cur" == sort:* || "$cur" == sortr:* ]]; then
        local prefix="${cur%%:*}"
        local keys="size -size largest smallest modified -modified newest oldest path len -len shortest longest line-num count -count"
        COMPREPLY=( $(compgen -W "$keys" -P "${prefix}:" -- "${cur#*:}") )
        return 0
    elif [[ "$cur" == head:* || "$cur" == tail:* || "$cur" == top:* || "$cur" == limit:* ]]; then
        return 0
    elif [[ "$cur" == dir:* || "$cur" == directory:* ]]; then
        local prefix="${cur%%:*}"
        local val="${cur#*:}"
        local matches=()
        for d in "$val"*/; do
            [[ -d "$d" ]] && matches+=("${prefix}:$d")
        done
        COMPREPLY=( "${matches[@]}" )
        return 0
    elif [[ "$cur" == file:* ]]; then
        local prefix="${cur%%:*}"
        local val="${cur#*:}"
        local matches=()
        for f in "$val"*; do
            [[ -f "$f" ]] && matches+=("${prefix}:$f")
        done
        COMPREPLY=( "${matches[@]}" )
        return 0
    elif [[ "$cur" == link:* || "$cur" == symlink:* ]]; then
        local prefix="${cur%%:*}"
        local val="${cur#*:}"
        local matches=()
        for f in "$val"*; do
            [[ -L "$f" || -e "$f" ]] && matches+=("${prefix}:$f")
        done
        COMPREPLY=( "${matches[@]}" )
        return 0
    elif [[ "$cur" == p:* || "$cur" == path:* ]]; then
        local prefix="${cur%%:*}"
        local val="${cur#*:}"
        local matches=()
        for p in "$val"*; do
            [[ -e "$p" ]] && matches+=("${prefix}:$p")
        done
        COMPREPLY=( "${matches[@]}" )
        return 0
    elif [[ "$cur" == mv:* || "$cur" == cp:* ]]; then
        local prefix="${cur%%:*}"
        local val="${cur#*:}"
        local matches=()
        for d in "$val"*/; do
            [[ -d "$d" ]] && matches+=("${prefix}:$d")
        done
        COMPREPLY=( "${matches[@]}" )
        return 0
    elif [[ "$cur" == np:* || "$cur" == no-path:* ]]; then
        local prefix="${cur%%:*}"
        local val="${cur#*:}"
        local matches=()
        for d in "$val"*/; do
            [[ -d "$d" ]] && matches+=("${prefix}:$d")
        done
        COMPREPLY=( "${matches[@]}" )
        return 0
    elif [[ "$cur" == t:* || "$cur" == type:* || "$cur" == nt:* || "$cur" == no-type:* ]]; then
        local prefix="${cur#*:}"
        local types="rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css web code data doc"
        COMPREPLY=( $(compgen -W "$types" -P "${cur%%:*}:" -- "$prefix") )
        return 0
    elif [[ "$cur" == kind:* ]]; then
        COMPREPLY=( $(compgen -W "file dir link bin text" -P "kind:" -- "${cur#kind:}") )
        return 0
    elif [[ "$cur" == yes:* ]]; then
        COMPREPLY=( $(compgen -W "dots bin case cache" -P "yes:" -- "${cur#yes:}") )
        return 0
    fi

    # Suggest matching DSL tokens alongside file paths
    local dsl_matches
    dsl_matches=$(compgen -W "$dsl_tokens" -- "$cur")
    local file_matches
    file_matches=$(compgen -f -- "$cur")
    COMPREPLY=( $dsl_matches $file_matches )
}

complete -F _grx_completions grx
"#
    .to_string()
}

/// Generate zsh completions script.
pub fn generate_zsh_completions() -> String {
    r#"#compdef grx

_grx() {
    local curcontext="$curcontext" state line
    typeset -A opt_args

    local -a common_args
    common_args=(
        '(-e --regexp)'{-e,--regexp}'[A pattern to search for]:pattern:'
        '(-f --file)'{-f,--file}'[Obtain patterns from file]:file:_files'
        '(-i --ignore-case)'{-i,--ignore-case}'[Case-insensitive search]'
        '(-s --case-sensitive)'{-s,--case-sensitive}'[Force case-sensitive search]'
        '(-S --smart-case)'{-S,--smart-case}'[Smart-case matching]'
        '(-v --invert-match)'{-v,--invert-match}'[Invert match: select non-matching lines]'
        '(-w --word-regexp)'{-w,--word-regexp}'[Match only whole words]'
        '(-x --line-regexp)'{-x,--line-regexp}'[Match only whole lines]'
        '(-n --line-number)'{-n,--line-number}'[Print 1-indexed line numbers]'
        '(-N --no-line-number)'{-N,--no-line-number}'[Suppress line numbers]'
        '--column[Show 1-based column number for matches]'
        '--no-column[Suppress column numbers in match output]'
        '(-b --byte-offset)'{-b,--byte-offset}'[Print 0-based byte offset of matching lines or parts]'
        '--heading[Print file path heading above matching lines]'
        '--no-heading[Suppress file path headings]'
        '(-p --pretty)'{-p,--pretty}'[Pretty output: alias for --color always --heading --line-number]'
        '*-u[Reduce ignore filtering]'
        '*--unrestricted[Reduce ignore filtering]'
        '(-H --with-filename)'{-H,--with-filename}'[Print filename for each match]'
        '(-I --no-filename)'{-I,--no-filename}'[Suppress file names in match output]'
        '(-d --max-depth)'{-d,--max-depth}'[Maximum directory recursion depth]:depth:'
        '(-h --help)'{-h,--help}'[Print concise help summary]'
        '--help-full[Print full, exhaustive list of all options]'
        '--help-all[Print full, exhaustive list of all options]'
        '--all[Modifier flag (e.g. --help --all)]'
        '--tutorial[Print interactive DSL tutorial]'
        '(-c --count)'{-c,--count}'[Only print count of matching lines per file]'
        '--count-matches[Print total count of individual matches per file]'
        '--stats[Print aggregate traversal, matching, and timing statistics]'
        '--json[Output search results as a stream of JSON records]'
        '(-l --files-with-matches)'{-l,--files-with-matches}'[Print names of files with matches]'
        '(-L --files-without-match)'{-L,--files-without-match}'[Print names of files without matches]'
        '(-m --max-count)'{-m,--max-count}'[Stop reading after NUM matches]:count:'
        '(-M --max-columns)'{-M,--max-columns}'[Truncate lines longer than NUM characters]:columns:'
        '--no-truncate[Do not truncate long lines in output]'
        '(-o --only-matching)'{-o,--only-matching}'[Show only matched parts of lines]'
        '(-q --quiet --silent)'{-q,--quiet,--silent}'[Quiet mode: suppress output, exit 0 if match]'
        '(-F --fixed-strings)'{-F,--fixed-strings}'[Treat pattern as fixed literal string]'
        '(-E --extended-regexp)'{-E,--extended-regexp}'[Treat pattern as extended regular expression]'
        '(-C --context)'{-C,--context}'[Print NUM lines of context]:lines:'
        '(-B --before-context)'{-B,--before-context}'[Print NUM lines before matches]:lines:'
        '(-A --after-context)'{-A,--after-context}'[Print NUM lines after matches]:lines:'
        '(-r -R --recursive)'{-r,-R,--recursive}'[Recursively search directories]'
        '(-0 --null)'{-0,--null}'[Output zero byte (NUL) line terminator]'
        '(-Z --fuzzy)'{-Z,--fuzzy}'[Fuzzy token permutation search]:tokens:'
        '(-t --type)'{-t,--type}'[Filter by file type]:type:(rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css web code data doc)' \
        '(-T --type-not)'{-T,--type-not}'[Exclude files matching file type]:type:(rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css web code data doc)' \
        '(-g --glob)'{-g,--glob}'[Include or exclude files matching glob]:glob:'
        '--hidden[Search hidden files and directories]'
        '--no-ignore[Do not respect .gitignore and .ignore]'
        '--follow[Follow directory symlinks]'
        '(-a --text --binary)'{-a,--text,--binary}'[Search inside binary files as text]'
        '(-j --threads)'{-j,--threads}'[Worker thread count]:threads:'
        '--color[When to use colors]:color:(auto always never)'
        '--hyperlinks[When to emit OSC 8 hyperlinks]:hyperlinks:(auto always never)'
        '--mode[Operational search mode]:mode:(dsl grep git-grep)'
        '--config[Path to custom configuration file]:file:_files'
        '--edit-config[Open configuration file in default editor]'
        '--no-messages[Suppress error messages about nonexistent or unreadable files]'
        '--no-ignore-messages[Print error messages about unreadable files during traversal]'
        '--dump-config[Print default configuration template]'
        '--init-config[Initialize configuration in XDG config directory]'
        '--force[Overwrite existing configuration file]'
        '--config-path[Print path of active configuration file]'
        '--paths[Print all distro-appropriate storage paths]'
        '--journal[Enable telemetry journaling]'
        '--completions[Generate shell completions]:shell:(fish bash zsh)'
        '--install-completions[Install completion script]:shell:(fish bash zsh)'
        '--sort[Sort search results by criteria]:key:(size bytes largest smallest modified time age date newest recent oldest path name len length path-len line-len linelen shortest longest line line-num linenum count)'
        '--reverse[Reverse sort ordering]'
        '--sort-reverse[Reverse sort ordering]'
        '--head[Limit results to first NUM entries]:count:'
        '--tail[Limit results to last NUM entries]:count:'
        '--exec[Execute command for each match]:command:'
        '(-X --exec-batch)'{-X,--exec-batch}'[Execute command once with all matches as arguments]:command:'
        '(--dry-run --dry)'{--dry-run,--dry}'[Simulate file actions without modifying filesystem]'
        '--mmap-threshold[File size threshold in bytes for memory mapping]:bytes:'
        '--max-file-size[Maximum file size to inspect in bytes]:bytes:'
        '(--move --mv)'{--move,--mv}'[Move matching items into destination directory]:directory:_files -/'
        '(--copy --cp)'{--copy,--cp}'[Copy matching items into destination directory]:directory:_files -/'
        '(--trash --rm)'{--trash,--rm}'[Safely stage matching items into trash cache]'
        '--clean-trash[Purge trash staging cache permanently]'
        '--rename[Rename matching files/directories using destination pattern]:pattern:'
        '--chmod[Change permissions mode in octal]:mode:'
        '--list[List undo transaction history]'
        '--help[Print help information]'
        '(-V --version)'{-V,--version}'[Print version]'
        '*::args:->args'
    )

    _arguments -s -S $common_args && return 0

    case "$state" in
        args)
            local -a dsl_tokens
            dsl_tokens=(
                'link\:[select symbolic link by basename]'
                'symlink\:[select symbolic link by basename]'
                'dir\:[select directory by basename]'
                'directory\:[select directory by basename]'
                'file\:[select regular file by basename]'
                'bin\:[select binary files (discovery or content)]'
                'p\:[root starting path or directory]'
                'path\:[root starting path or directory]'
                'in\:[match entry basename containing pattern]'
                'ni\:[exclude entry basename]'
                't\:[filter by file type or extension]'
                'type\:[filter by file type or extension]'
                'kind\:file[select regular files]'
                'kind\:dir[select directories]'
                'kind\:link[select symbolic links]'
                'kind\:bin[select binary files]'
                'kind\:text[select plain text files]'
                'sort\:size[sort by size ascending]'
                'sort\:modified[sort by modification time]'
                'sort\:path[sort alphabetically by path]'
                'sort\:len[sort by length]'
                'sort\:count[sort by match count]'
                'sortr\:size[sort by size descending]'
                'sortr\:modified[sort by modification time oldest first]'
                'sortr\:path[sort reverse alphabetically]'
                'sortr\:len[sort by length longest first]'
                'sortr\:count[sort by match count lowest first]'
                'head\:10[limit to first 10 results]'
                'tail\:10[limit to last 10 results]'
                'top\:10[limit matches per file]'
                'limit\:10[limit matches per file]'
                'larger\:[filter files larger than threshold]'
                'smaller\:[filter files smaller than threshold]'
                'newer\:[filter entries modified within age]'
                'older\:[filter entries modified before age]'
                'str\:4[extract printable strings from binaries]'
                'strings\:8[extract printable strings from binaries]'
                'near\:3,[proximity filter pattern within N lines]'
                'no-near\:3,[inverted proximity filter]'
                'ctx\:3[show 3 lines of context]'
                'd\:0[current directory only]'
                'd\:1[descend at most 1 level]'
                'np\:[exclude directory path]'
                'nt\:[exclude file type]'
                'ns\:[exclude matching lines]'
                'yes\:dots[search hidden files]'
                'no\:dots[ignore hidden files]'
                'yes\:bin[search binary files]'
                'no\:bin[ignore binary files]'
                'yes\:case[force case-sensitive search]'
                'no\:case[force case-insensitive search]'
                'yes\:cache[search cache directories]'
                'AND[boolean match both]'
                'OR[boolean match either]'
                'NOT[boolean invert]'
                'fz\:[fuzzy token permutation search]'
                'hex\:[hex byte signature matching]'
                're\:[explicit regular expression pattern]'
                'mv\:[move matching items to destination directory]'
                'cp\:[copy matching items to destination directory]'
                'rm\:[safely remove/trash matching items]'
                'trash\:[safely remove/trash matching items]'
                'dry\:[simulate action without touching disk]'
                'rename\:[rename matching items via destination pattern]'
                'chmod\:[change permissions mode for matching items]'
                'NEAR\:3[infix proximity operator]'
                'undo[revert the latest filesystem mutation]'
            )
            _describe -t dsl 'DSL tokens' dsl_tokens
            _files
            ;;
    esac
}

_grx "$@"
"#
    .to_string()
}

/// Detect the user's active shell or normalize provided name.
pub fn detect_shell(requested: Option<&str>) -> Result<&'static str, String> {
    if let Some(sh) = requested {
        let clean = sh.trim().to_ascii_lowercase();
        if clean.contains("fish") {
            return Ok("fish");
        } else if clean.contains("zsh") {
            return Ok("zsh");
        } else if clean.contains("bash") {
            return Ok("bash");
        } else {
            return Err(format!(
                "Unsupported shell '{sh}'. Supported shells: fish, bash, zsh"
            ));
        }
    }

    if let Ok(shell_env) = std::env::var("SHELL") {
        let clean = shell_env.to_ascii_lowercase();
        if clean.contains("fish") {
            return Ok("fish");
        } else if clean.contains("zsh") {
            return Ok("zsh");
        } else if clean.contains("bash") {
            return Ok("bash");
        }
    }

    // Default to fish if active on system, otherwise bash
    Ok("fish")
}

/// Resolve the distro-appropriate installation path for a shell's completion script.
pub fn resolve_completion_path(shell: &str) -> Result<PathBuf, io::Error> {
    match shell {
        "fish" => {
            let config_home = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
                if !xdg.trim().is_empty() {
                    PathBuf::from(xdg.trim())
                } else {
                    fallback_home_config()?
                }
            } else {
                fallback_home_config()?
            };
            Ok(config_home.join("fish/completions/grx.fish"))
        }
        "bash" => {
            let data_home = if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
                if !xdg.trim().is_empty() {
                    PathBuf::from(xdg.trim())
                } else {
                    fallback_home_data()?
                }
            } else {
                fallback_home_data()?
            };
            Ok(data_home.join("bash-completion/completions/grx"))
        }
        "zsh" => {
            if let Ok(zdotdir) = std::env::var("ZDOTDIR")
                && !zdotdir.trim().is_empty()
            {
                return Ok(PathBuf::from(zdotdir.trim()).join(".zfunc/_grx"));
            }
            if let Ok(home) = std::env::var("HOME")
                && !home.trim().is_empty()
            {
                return Ok(PathBuf::from(home.trim()).join(".zfunc/_grx"));
            }
            #[cfg(windows)]
            {
                if let Ok(userprofile) = std::env::var("USERPROFILE")
                    && !userprofile.trim().is_empty()
                {
                    return Ok(PathBuf::from(userprofile.trim()).join(".zfunc/_grx"));
                }
            }
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Could not locate HOME, USERPROFILE, or ZDOTDIR for zsh completions",
            ))
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Unsupported shell '{other}'. Supported shells: fish, bash, zsh"),
        )),
    }
}

fn fallback_home_config() -> Result<PathBuf, io::Error> {
    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return Ok(PathBuf::from(home.trim()).join(".config"));
    }
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA")
            && !appdata.trim().is_empty()
        {
            return Ok(PathBuf::from(appdata.trim()));
        }
        if let Ok(userprofile) = std::env::var("USERPROFILE")
            && !userprofile.trim().is_empty()
        {
            return Ok(PathBuf::from(userprofile.trim()).join(".config"));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "HOME environment variable not set",
    ))
}

fn fallback_home_data() -> Result<PathBuf, io::Error> {
    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return Ok(PathBuf::from(home.trim()).join(".local/share"));
    }
    #[cfg(windows)]
    {
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA")
            && !localappdata.trim().is_empty()
        {
            return Ok(PathBuf::from(localappdata.trim()));
        }
        if let Ok(userprofile) = std::env::var("USERPROFILE")
            && !userprofile.trim().is_empty()
        {
            return Ok(PathBuf::from(userprofile.trim()).join(".local/share"));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "HOME environment variable not set",
    ))
}

/// Install completion script to the distro-appropriate location.
pub fn install_completion_script(
    requested_shell: Option<&str>,
) -> Result<(PathBuf, &'static str), io::Error> {
    let shell = detect_shell(requested_shell)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let target_path = resolve_completion_path(shell)?;

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let script = match shell {
        "fish" => generate_fish_completions(),
        "bash" => generate_bash_completions(),
        "zsh" => generate_zsh_completions(),
        _ => unreachable!(),
    };

    fs::write(&target_path, script)?;
    Ok((target_path, shell))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_fish_completions() {
        let script = generate_fish_completions();
        assert!(script.contains("complete -c grx -f"));
        assert!(script.contains("__grx_complete_dsl"));
        assert!(script.contains("link:"));
        assert!(script.contains("symlink:"));
        assert!(script.contains("dir:"));
        assert!(script.contains("directory:"));
        assert!(script.contains("file:"));
        assert!(script.contains("bin:"));
        assert!(script.contains(":rs"));
        assert!(script.contains("in:report"));
        assert!(script.contains("kind:file"));
        assert!(script.contains("kind:bin"));
        assert!(script.contains("kind:text"));
        assert!(!script.contains("+dot"));
        assert!(!script.contains("+bin"));
        assert!(script.contains("larger:10MiB"));
        assert!(script.contains("__fish_complete_path"));
        assert!(script.contains("-l mode"));
        assert!(script.contains("sort:size"));
        assert!(script.contains("sort:modified"));
        assert!(script.contains("head:10"));
        assert!(script.contains("tail:10"));
        assert!(!script.contains("line:10"));
        assert!(script.contains("-l sort"));
        assert!(script.contains("-l reverse"));
        assert!(script.contains("re:regex"));
        assert!(script.contains("hex:7f454c"));
        assert!(script.contains("mv:dest/"));
        assert!(script.contains("cp:dest/"));
        assert!(script.contains("rm:"));
        assert!(script.contains("trash:"));
        assert!(script.contains("dry:"));
        assert!(script.contains("rename:pattern"));
        assert!(script.contains("chmod:755"));
        assert!(script.contains("NEAR:3"));
        assert!(script.contains("undo"));
        assert!(script.contains("-l dry-run"));
        assert!(script.contains("-l mmap-threshold"));
        assert!(script.contains("-l max-file-size"));
        assert!(script.contains("-l rename"));
        assert!(script.contains("-l chmod"));
        assert!(script.contains("-l move"));
        assert!(script.contains("-l copy"));
        assert!(script.contains("-l trash"));
        assert!(script.contains("-l clean-trash"));
        assert!(script.contains("-l list"));
    }

    #[test]
    fn test_generate_bash_completions() {
        let script = generate_bash_completions();
        assert!(script.contains("_grx_completions"));
        assert!(script.contains("complete -F _grx_completions grx"));
        assert!(script.contains("link:*"));
        assert!(script.contains("symlink:*"));
        assert!(script.contains("dir:*"));
        assert!(script.contains("file:*"));
        assert!(script.contains("p:*"));
        assert!(script.contains("kind:bin"));
        assert!(script.contains("kind:text"));
        assert!(script.contains("bin:"));
        assert!(!script.contains("+dot"));
        assert!(!script.contains("+bin"));
        assert!(script.contains("--sort"));
        assert!(script.contains("--head"));
        assert!(script.contains("--tail"));
        assert!(script.contains("sort:*"));
        assert!(script.contains("head:*"));
        assert!(script.contains("hex:"));
        assert!(script.contains("re:"));
        assert!(script.contains("mv:"));
        assert!(script.contains("cp:"));
        assert!(script.contains("rm:"));
        assert!(script.contains("trash:"));
        assert!(script.contains("dry:"));
        assert!(script.contains("rename:"));
        assert!(script.contains("chmod:"));
        assert!(script.contains("NEAR:3"));
        assert!(script.contains("undo"));
        assert!(script.contains("--dry-run"));
        assert!(script.contains("--mmap-threshold"));
        assert!(script.contains("--max-file-size"));
        assert!(script.contains("--move"));
        assert!(script.contains("--copy"));
        assert!(script.contains("--trash"));
        assert!(script.contains("--clean-trash"));
        assert!(script.contains("--rename"));
        assert!(script.contains("--chmod"));
        assert!(script.contains("--list"));
    }

    #[test]
    fn test_generate_zsh_completions() {
        let script = generate_zsh_completions();
        assert!(script.contains("#compdef grx"));
        assert!(script.contains("_arguments"));
        assert!(script.contains("link\\:"));
        assert!(script.contains("symlink\\:"));
        assert!(script.contains("dir\\:"));
        assert!(script.contains("file\\:"));
        assert!(script.contains("bin\\:"));
        assert!(script.contains("kind\\:bin"));
        assert!(script.contains("kind\\:text"));
        assert!(script.contains("--sort"));
        assert!(script.contains("--head"));
        assert!(script.contains("--tail"));
        assert!(script.contains("(-C --context)"));
        assert!(script.contains("(-B --before-context)"));
        assert!(script.contains("(-A --after-context)"));
        assert!(script.contains("sortr\\:"));
        assert!(script.contains("larger\\:"));
        assert!(script.contains("fz\\:"));
        assert!(script.contains("hex\\:"));
        assert!(script.contains("re\\:"));
        assert!(script.contains("mv\\:"));
        assert!(script.contains("cp\\:"));
        assert!(script.contains("rm\\:"));
        assert!(script.contains("trash\\:"));
        assert!(script.contains("dry\\:"));
        assert!(script.contains("rename\\:"));
        assert!(script.contains("chmod\\:"));
        assert!(script.contains("NEAR\\:3"));
        assert!(script.contains("undo"));
        assert!(script.contains("--dry-run"));
        assert!(script.contains("--mmap-threshold"));
        assert!(script.contains("--max-file-size"));
        assert!(script.contains("--move"));
        assert!(script.contains("--copy"));
        assert!(script.contains("--trash"));
        assert!(script.contains("--clean-trash"));
        assert!(script.contains("--rename"));
        assert!(script.contains("--chmod"));
        assert!(script.contains("--list"));
    }

    #[test]
    fn test_detect_shell() {
        assert_eq!(detect_shell(Some("fish")), Ok("fish"));
        assert_eq!(detect_shell(Some("/bin/zsh")), Ok("zsh"));
        assert_eq!(detect_shell(Some("bash")), Ok("bash"));
        assert!(detect_shell(Some("invalid_shell")).is_err());
    }

    #[test]
    fn test_resolve_completion_path_fish() {
        let path = resolve_completion_path("fish").unwrap();
        assert!(path.ends_with("fish/completions/grx.fish"));
    }
}
