# md2html

Bidirectional Markdown ↔ HTML converter. Zero dependencies.

## Features

- **Markdown → HTML**: headings, bold/italic, code spans and fences, links, ordered/unordered lists, blockquotes, horizontal rules, HTML entity escaping
- **HTML → Markdown**: nested inline elements, blockquotes, pre/code blocks, ordered/unordered lists, links, self-closing tags, HTML entities, comments, doctypes
- **Zero dependencies**: pure Rust standard library implementation
- **Fast**: opt-level 3 + LTO in release builds

## Installation

```bash
cargo install md2html
```

## Usage

```bash
# Markdown to HTML
md2html to-html input.md output.html

# HTML to Markdown
md2html to-md input.html output.md

# Help
md2html --help

# Version
md2html --version
```

## Examples

```bash
# Convert a README to HTML for web preview
md2html to-html README.md README.html

# Extract Markdown from a saved web page
md2html to-md page.html page.md
```

## License

MIT
