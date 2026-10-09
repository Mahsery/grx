
complete -c grx -f

function __grx_complete_dsl
    set -l token (commandline -ct)

    if string match -rq '^root:(?<val>.*)' -- $token
        for p in $val*
            test -e "$p" && printf "root:%s\tStarting path\n" "$p"
        end
        return 0
    else if string match -rq '^np:(?<val>.*)' -- $token
        for d in $val*/
            test -d "$d" && printf "np:%s\tExclude directory\n" "$d"
        end
        return 0
    else if string match -rq '^sort:(?<val>.*)' -- $token
        set -l keys size -size bytes largest smallest modified -modified time age date newest recent oldest path name len -len length path-len line-len linelen shortest longest line line-num linenum count -count
        for k in $keys
            string match -q "$val*" -- $k && printf "sort:%s\tSort by %s\n" "$k" "$k"
        end
        return 0
    else if string match -rq '^t:(?<val>.*)' -- $token
        for t in rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css web code data doc
            string match -q "$val*" -- $t && printf "t:%s\tFilter by type %s\n" "$t" "$t"
        end
        return 0
    else if string match -rq '^nt:(?<val>.*)' -- $token
        for t in rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css web code data doc
            string match -q "$val*" -- $t && printf "nt:%s\tExclude type %s\n" "$t" "$t"
        end
        return 0
    else if string match -rq '^(?<prefix>kind|k):(?<val>.*)' -- $token
        for k in file dir link bin text
            string match -q "$val*" -- $k && printf "%s:%s\tSelect filesystem kind\n" "$prefix" "$k"
        end
        return 0
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

complete -c grx -a "(__grx_complete_dsl)"

# BEGIN DSL REGISTRY
complete -c grx -a "root:" -d "Exact traversal root (repeat for multiple roots)"
complete -c grx -a "p:" -d "Include paths; ? separates alternatives, trailing / selects directories"
complete -c grx -a "np:" -d "Exclude paths; ? separates alternatives"
complete -c grx -a "kind:" -d "Discover file, dir, link, bin, or text entries"
complete -c grx -a "k:" -d "Alias for kind: (file, dir, link, bin, text)"
complete -c grx -a "t:" -d "Filter filetypes without changing mode; ?-separated alternatives"
complete -c grx -a "nt:" -d "Exclude filetypes; ? separates alternatives"
complete -c grx -a "in:" -d "Search contents of selected files"
complete -c grx -a "ns:" -d "Exclude content lines; ? separates alternatives"
complete -c grx -a "lit:" -d "Literal substring without DSL interpretation"
complete -c grx -a "fz:" -d "Scored ordered-subsequence fuzzy matching"
complete -c grx -a "re:" -d "Explicit regular expression"
complete -c grx -a "larger:" -d "File size lower bound"
complete -c grx -a "smaller:" -d "File size upper bound"
complete -c grx -a "newer:" -d "Modification age upper bound"
complete -c grx -a "older:" -d "Modification age lower bound"
complete -c grx -a "d:" -d "Traversal depth from each root"
complete -c grx -a "max:" -d "Maximum matching lines per file"
complete -c grx -a "head:" -d "First result records after sorting"
complete -c grx -a "tail:" -d "Last result records after sorting"
complete -c grx -a "nth:" -d "Select a one-based result index"
complete -c grx -a "pick:" -d "Interactive result selection"
complete -c grx -a "sort:" -d "Result sorting key"
complete -c grx -a "ctx:" -d "Content context lines"
complete -c grx -a "near:" -d "Content proximity constraint"
complete -c grx -a "no-near:" -d "Negated content proximity constraint"
complete -c grx -a "str:" -d "Binary printable-string extraction"
complete -c grx -a "hex:" -d "Hex byte content pattern"
complete -c grx -a "mv:" -d "Move selected entries"
complete -c grx -a "cp:" -d "Copy selected entries"
complete -c grx -a "trash:" -d "Trash selected entries"
complete -c grx -a "rename:" -d "Rename selected entries"
complete -c grx -a "chmod:" -d "Change selected entry permissions"
complete -c grx -a "dry:" -d "Preview actions"
# END DSL REGISTRY

complete -c grx -l mode -x -a "dsl grep git-grep" -d "Operational search mode"
complete -c grx -l color -x -a "auto always never" -d "When to use colors"
complete -c grx -l hyperlinks -x -a "auto always never" -d "When to emit OSC 8 hyperlinks"
complete -c grx -s d -l max-depth -x -d "Maximum directory recursion depth"
complete -c grx -l sort -x -a "size bytes largest smallest modified time age date newest recent oldest path name len length path-len line-len linelen shortest longest line line-num linenum count" -d "Sort results by key"
complete -c grx -l reverse -d "Reverse sort ordering"
complete -c grx -l sort-reverse -d "Reverse sort ordering"
complete -c grx -l head -x -d "Limit results to first NUM entries"
complete -c grx -l tail -x -d "Limit results to last NUM entries"
complete -c grx -l nth -x -d "Pick the N-th result (1-based; negative from end)"
complete -c grx -l pick -d "Interactively pick result(s) before emitting or executing"

complete -c grx -s t -l type -x -a "rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css web code data doc" -d "Filter by file type"
complete -c grx -s T -l type-not -x -a "rs rust c cpp c++ py python go golang js javascript ts typescript toml json yaml yml md markdown sh shell bash fish zsh java kotlin kt zig lua sql html css web code data doc" -d "Exclude files matching file type"

complete -c grx -l config -r -F -d "Path to custom configuration file"
complete -c grx -s g -l glob -x -d "Include or exclude files matching glob"
complete -c grx -s m -l max-count -x -d "Stop reading after NUM matches"
complete -c grx -s M -l max-columns -x -d "Truncate lines longer than NUM characters"
complete -c grx -s C -l context -x -d "Print NUM lines of context"
complete -c grx -s B -l before-context -x -d "Print NUM lines before matches"
complete -c grx -s A -l after-context -x -d "Print NUM lines after matches"
complete -c grx -s j -l threads -x -d "Worker thread count (0 = auto)"
complete -c grx -s Z -l fuzzy -x -d "Scored fzf-style fuzzy matching"

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
