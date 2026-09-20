# Bash completion for pagina.
# Install: copy to /usr/share/bash-completion/completions/pagina
#        or `source completions/pagina.bash`.

_pagina() {
    local cur prev sub
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]:-}"

    # First positional: subcommands.
    if [[ "$COMP_CWORD" -eq 1 ]]; then
        COMPREPLY=($(compgen -W "to-html to-md help version --help -h --version -v --color" -- "$cur"))
        return 0
    fi

    sub="${COMP_WORDS[1]}"

    # `--color` takes auto/always/never (both `--color X` and `--color=X`).
    if [[ "$prev" == "--color" ]]; then
        COMPREPLY=($(compgen -W "auto always never" -- "$cur"))
        return 0
    fi
    if [[ "$cur" == --color=* ]]; then
        local prefix="--color="
        local val="${cur#--color=}"
        COMPREPLY=($(compgen -W "auto always never" -- "$val" | sed "s|^|${prefix}|"))
        return 0
    fi

    case "$sub" in
        to-html|to-md)
            # Flags plus file arguments.
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "--gfm --color --help -h --version -v" -- "$cur"))
                return 0
            fi
            # Complete file paths for <input> and <output>.
            COMPREPLY=($(compgen -f -- "$cur"))
            return 0
            ;;
        *)
            COMPREPLY=($(compgen -W "--gfm --color --help -h --version -v" -- "$cur"))
            return 0
            ;;
    esac
}

complete -F _pagina pagina
