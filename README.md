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

## WebAssembly / npm

The `wasm` cargo feature exposes the converters to JavaScript via
`wasm-bindgen` (`src/wasm.rs`). The default build stays dependency-free;
only `--features wasm` pulls in `wasm-bindgen`.

Exported functions: `markdown_to_html`, `markdown_to_html_gfm`,
`html_to_markdown`, `html_to_markdown_gfm` (each throws on error), plus
frontmatter helpers `frontmatter_has`, `frontmatter_body`,
`frontmatter_original`, `frontmatter_get`, and `prepend_frontmatter`.

```bash
# Check the bindings compile (host target, no wasm toolchain needed)
cargo check --features wasm

# Size-optimized WASM build (needs the wasm32 target + wasm-pack)
rustup target add wasm32-unknown-unknown
wasm-pack build --target bundler --features wasm
# or: cargo build --profile wasm --target wasm32-unknown-unknown --features wasm
```

`Cargo.toml` ships a `[profile.wasm]` size profile (`opt-level = "z"`,
LTO, single codegen unit, stripped) and
`[package.metadata.wasm-pack.profile.release]` (`wasm-opt -Oz`) tuning
for the npm artifact.

## License

MIT
