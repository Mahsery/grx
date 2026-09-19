# Bash completion script for grx
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
