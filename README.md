# pagina

Bidirectional Markdown ↔ HTML converter. Zero dependencies.

*"Pagina" — Latin for "page."*

**CommonMark 0.31.2: 652/652 (100%)** — Markdown → HTML conforms to the CommonMark
0.31.2 spec, verified via `tests/commonmark_compliance.rs` + `tests/fixtures/spec.txt`.

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

## Security

Per CommonMark, `pagina to-html` preserves raw HTML blocks and inline HTML
verbatim, and link URLs are passed through unmodified — `javascript:` and
`data:` URLs are **not** stripped. Do not render the output directly in a
browser or email client without sanitizing it first (e.g. with
[`ammonia`](https://crates.io/crates/ammonia)).

The CommonMark compliance suite is gated behind `#[ignore]` (it is an audit
tool, not a fast unit test). Re-run it with:

```bash
cargo test --test commonmark_compliance -- --ignored --nocapture
```

## License

MIT
