#compdef pagina
# Zsh completion for pagina.
# Install: copy to a directory on your $fpath, e.g. ~/.zsh/completions/_pagina.

_pagina() {
    local -a subcommands
    subcommands=(
        'to-html:Convert Markdown to HTML'
        'to-md:Convert HTML to Markdown'
        'help:Print help information'
        'version:Print version information'
    )

    local -a color_values
    color_values=(
        'auto:Colorize only when writing to a TTY'
        'always:Always colorize help and errors'
        'never:Never colorize output'
    )

    if (( CURRENT == 2 )); then
        _describe -t subcommands 'pagina subcommand' subcommands
        _arguments \
            '(-h --help)'{-h,--help}'[Print help information]' \
            '(-v --version)'{-v,--version}'[Print version information]' \
            '--color=[Colorize help and errors]:when:->color'
        return
    fi

    case "$words[2]" in
        to-html|to-md)
            _arguments \
                '--gfm[Enable GFM extensions]' \
                '--color=[Colorize help and errors]:when:->color' \
                '(-h --help)'{-h,--help}'[Print help information]' \
                '(-v --version)'{-v,--version}'[Print version information]' \
                '1:input file:_files' \
                '2:output file:_files'
            ;;
    esac

    case "$state" in
        color) _describe -t color-values 'color mode' color_values ;;
    esac
}

_pagina "$@"
