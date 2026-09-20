# pagina

Bidirectional Markdown ↔ HTML converter. Zero dependencies.

*"Pagina" — Latin for "page."*

**CommonMark 0.31.2: 652/652 (100%)** — Markdown → HTML conforms to the CommonMark
0.31.2 spec, verified via `tests/commonmark_compliance.rs` + `tests/fixtures/spec.txt`.

[![CommonMark 0.31.2: 652/652 (100%)](https://img.shields.io/badge/CommonMark%200.31.2-652%2F652%20(100%25)-brightgreen)](https://github.com/NefaroXX/pagina/blob/main/tests/commonmark_compliance.rs)
[![crates.io](https://img.shields.io/crates/v/pagina.svg)](https://crates.io/crates/pagina)
[![docs.rs](https://docs.rs/pagina/badge.svg)](https://docs.rs/pagina)
[![CI](https://github.com/NefaroXX/pagina/actions/workflows/ci.yml/badge.svg)](https://github.com/NefaroXX/pagina/actions/workflows/ci.yml)
[![npm pagina-wasm](https://img.shields.io/npm/v/pagina-wasm.svg)](https://www.npmjs.com/package/pagina-wasm)

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

# GFM mode (task lists, strikethrough, bare autolinks)
pagina to-html --gfm notes.md notes.html
pagina to-md --gfm page.html page.md

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

## Library API

The converters are plain Rust functions under `pagina::markdown_to_html` and
`pagina::html_to_markdown` — the CLI is a thin wrapper on top.

```toml
[dependencies]
pagina = "0.1"
```

### GFM mode

`convert_gfm` / `html_to_markdown_gfm` enable task lists, strikethrough and
bare autolinks; pipe tables render in both modes.

```rust
use pagina::markdown_to_html::convert_gfm;

let html = convert_gfm("~~done~~").unwrap();
assert!(html.contains("<del>done</del>"));
```

### Frontmatter

A leading metadata block is detected and stripped before conversion (with or
without the `frontmatter` feature). `---` blocks parse as YAML, `+++` blocks
as the built-in TOML subset. Keep the `Frontmatter` aside and re-attach it
after an HTML round-trip:

```rust
use pagina::frontmatter::{parse_with_frontmatter, prepend_frontmatter};
use pagina::{html_to_markdown, markdown_to_html};

let md = "---\ntitle: Hi\n---\n# Hi\n";
let (fm, body) = parse_with_frontmatter(md);
let fm = fm.expect("frontmatter present");
let html = markdown_to_html::convert(body).unwrap();
let back = html_to_markdown::convert(&html).unwrap();
let round_tripped = prepend_frontmatter(&fm, &back);
assert_eq!(round_tripped, md);
```

### Sanitized HTML

The `sanitize` feature adds a zero-dependency denylist post-processor:
`convert_sanitized`, `convert_gfm_sanitized`, `convert_with_sanitized` (also
re-exported as `markdown_to_html_sanitized` at the crate root). It strips
`script`/`style`/`iframe`/`object`/`embed`/`form`/`base`/`link`/`meta`, drops
`on*` and `style` attributes, and blocks `javascript:`, `vbscript:` and
`data:text/html` URLs in `href`/`src`.

```toml
[dependencies]
pagina = { version = "0.1", features = ["sanitize"] }
```

```rust
use pagina::markdown_to_html::convert_sanitized;

let html = convert_sanitized("<script>alert(1)</script># Hi").unwrap();
assert_eq!(html, "<h1>Hi</h1>\n");
```

The default `convert` path stays byte-identical with or without the feature —
sanitization only happens through the `*_sanitized` entry points.

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

Import the functions from the `pagina-wasm` npm package:

```js
import { markdown_to_html, markdown_to_html_gfm, html_to_markdown } from "pagina-wasm";

const html = markdown_to_html("# Hello");      // "<h1>Hello</h1>\n"
const gfm = markdown_to_html_gfm("~~done~~");  // GFM extensions on
const md = html_to_markdown(html);             // reverse direction
```

Exported functions: `markdown_to_html`, `markdown_to_html_gfm`,
`html_to_markdown`, `html_to_markdown_gfm` (each throws on error), plus
frontmatter helpers `frontmatter_has`, `frontmatter_body`,
`frontmatter_original`, `frontmatter_get`, and `prepend_frontmatter`.
With `--features wasm,sanitize` you also get `markdown_to_html_sanitized` and
`markdown_to_html_gfm_sanitized`.

```bash
# Check the bindings compile (host target, no wasm toolchain needed)
cargo check --features wasm

# Size-optimized WASM build (needs the wasm32 target + wasm-pack)
rustup target add wasm32-unknown-unknown
RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc wasm-pack build --target bundler --features wasm
# or: cargo build --profile wasm --target wasm32-unknown-unknown --features wasm

# Rename before publish: wasm-pack names the npm package after the
# crate ("pagina", taken on npm). pkg/ is git-ignored, so re-apply
# after every fresh build:
#   edit pkg/package.json -> "name": "pagina-wasm"
#   cd pkg && npm publish
# Live as pagina-wasm@0.1.0: https://www.npmjs.com/package/pagina-wasm
```

`Cargo.toml` ships a `[profile.wasm]` size profile (`opt-level = "z"`,
LTO, single codegen unit, stripped) and
`[package.metadata.wasm-pack.profile.release]` (`wasm-opt -Oz`) tuning
for the npm artifact.

## License

MIT
