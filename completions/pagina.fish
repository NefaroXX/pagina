# Fish completion for pagina.
# Install: copy to ~/.config/fish/completions/pagina.fish
# Covers: to-html, to-md, --gfm, --color, --help, --version.

# Subcommands (also accepted: help, version).
complete -c pagina -f -n __fish_use_subcommand -a to-html -d 'Convert Markdown to HTML'
complete -c pagina -f -n __fish_use_subcommand -a to-md -d 'Convert HTML to Markdown'
complete -c pagina -f -n __fish_use_subcommand -a help -d 'Print help information'
complete -c pagina -f -n __fish_use_subcommand -a version -d 'Print version information'

# Flags for the top level and both subcommands.
complete -c pagina -s h -l help -d 'Print help information'
complete -c pagina -s v -l version -d 'Print version information'
complete -c pagina -l gfm -d 'Enable GFM extensions'
complete -c pagina -l color -d 'Colorize help and errors' -x -a 'auto always never'

# File arguments for the conversion subcommands.
complete -c pagina -f -n '__fish_seen_subcommand_from to-html; and not __fish_seen_subcommand_from -- --gfm --color' -a '(__fish_complete_path)'
complete -c pagina -f -n '__fish_seen_subcommand_from to-md; and not __fish_seen_subcommand_from -- --gfm --color' -a '(__fish_complete_path)'
