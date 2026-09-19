# pagina

Bidirectional Markdown ↔ HTML converter. Zero dependencies.

*"Pagina" — Latin for "page."*

## Features

- **Markdown → HTML**: headings, bold/italic (`**`/`__`, `*`/`_`), code spans and fences, links, ordered/unordered lists, blockquotes, horizontal rules, HTML entity escaping
- **HTML → Markdown**: nested inline elements, blockquotes, pre/code blocks, ordered/unordered lists, links, `<br>`, `<img>`, HTML entity unescaping, comments, doctypes
- **Zero dependencies**: pure Rust standard library implementation
- **Fast**: opt-level 3 + LTO in release builds
- **Stdin/stdout**: pipe-friendly with `-` argument

## Installation

```bash
cargo install pagina
```

## Usage

```bash
# Markdown to HTML
pagina to-html input.md output.html

# HTML to Markdown
pagina to-md input.html output.md

# Pipe from stdin
cat file.md | pagina to-html - output.html

# Output to stdout
pagina to-html file.md -

# Help
pagina --help

# Version
pagina --version
```

## Examples

```bash
# Convert a README to HTML for web preview
pagina to-html README.md README.html

# Extract Markdown from a saved web page
pagina to-md page.html page.md

# Use in a pipeline
curl -s https://example.com | pagina to-md - page.md
```

## License

MIT
