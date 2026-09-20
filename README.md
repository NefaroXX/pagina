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

# GFM mode (task lists, strikethrough, bare autolinks, footnotes, deflists, math)
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

`convert_gfm` / `html_to_markdown_gfm` enable task lists, strikethrough,
bare autolinks, footnotes, definition lists and math; pipe tables render
in both modes.

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

### AST & visitor

`pagina::ast` exposes an owned, lossless-leaning [`Document`] tree
(`Block`/`Inline` node types, all `String`s — no arenas, no lifetimes, so it
is `Send + Sync`). `parse`/`parse_gfm` build it; `render_html`/`render_markdown`
convert it back (all four are also re-exported at the crate root as
`pagina::parse`, `pagina::parse_gfm`, `pagina::render_html`,
`pagina::render_markdown`). The renderers are additive: for matched options,
`render_html(&parse(md))` is verified byte-identical to
`markdown_to_html::convert(md)`.

```rust
use pagina::{parse_gfm, render_html, render_markdown};

let doc = parse_gfm("# Hi\n\nA note[^1].\n\n[^1]: The note.\n");
let html = render_html(&doc);    // footnotes render as a <section class="footnotes" data-footnotes> footer
let md = render_markdown(&doc);  // normalized Markdown that re-parses to the same tree
```

Walk the tree without rendering via `pagina::visitor`: implement the
`Visitor` trait (both callbacks default to no-ops) and drive it with
`walk_document` / `walk_block` / `walk_inline` / `walk_inlines` in pre-order:

```rust
use pagina::ast::{parse, Block};
use pagina::visitor::{walk_document, Visitor};

#[derive(Default)]
struct Headings { entries: Vec<(u8, String)> }

impl Visitor for Headings {
    fn visit_block(&mut self, block: &Block) {
        if let Block::Heading(h) = block {
            self.entries
                .push((h.level, pagina::ast::plain_text(&h.content)));
        }
    }
}

let doc = parse("# A\n\n## B\n", pagina::Options::default());
let mut toc = Headings::default();
walk_document(&doc, &mut toc);
assert_eq!(toc.entries.len(), 2);
```

### Streaming events

`pagina::stream` is a pulldown-style pull API over the same AST:
`parse_stream(input, options)` returns a [`Parser`]
(`Iterator` + `ExactSizeIterator`, with `Parser::new` taking an owned
[`Document`]); `collect_events` / `events_from_document` hand you the events
as a `Vec`. Every open is an [`Event::Start`] carrying an owned [`Tag`]
payload, matched by a payload-free [`Event::End`] with a [`TagEnd`];
`render_events_to_html` turns an event slice back into HTML (byte-identical
to `ast::render_html` for its own output, and footnote definitions hoist
into the same `<section class="footnotes">` footer). Frontmatter is
metadata and emits no events.

```rust
use pagina::stream::{parse_stream, render_events_to_html, Event, Tag, TagEnd};
use pagina::Options;

let events: Vec<Event> = parse_stream("~~done~~", Options::gfm()).collect();
assert_eq!(render_events_to_html(&events), "<p><del>done</del></p>\n");

let mut parser = parse_stream("# Hi\n", Options::default());
assert_eq!(parser.len(), 3); // Start(Heading) … Text … End(Heading)
while let Some(event) = parser.next() {
    match event {
        Event::Start(Tag::Heading { level }) => println!("<h{level}>"),
        Event::End(TagEnd::Heading) => println!("</h>"),
        _ => {}
    }
}
```

`render_events_to_html_with_highlighter` renders with a syntax highlighter
(see below).

### Footnotes & definition lists

Both are GFM extensions: enable them with `pagina to-html --gfm` (or the
library GFM entry points `convert_gfm`, `parse_gfm`, `Options::gfm()`).
Pure CommonMark mode leaves the syntax literal.

- **Footnotes** — an inline `[^label]` reference plus a `[^label]:`
  definition. Definitions are hoisted out of flow and rendered as a
  `<section class="footnotes" data-footnotes>` footer with `id="fn-N"`
  anchors and `class="footnote-backref"` backlinks, matching the
  [GitHub Flavored Markdown] shape. `pagina to-md --gfm` folds the footer
  back into `[^label]:` definitions.
- **Definition lists** — PHP Markdown Extra-style: a term line followed by a
  `: description` marker renders `<dl>`/`<dt>`/`<dd>`:

  ```markdown
  Apple
  : Pomaceous fruit of the genus *Malus*.

  Orange
  : Citrus fruit.
  ```

  Tight lists unwrap single-paragraph `<dd>`s; multi-block descriptions and
  blank gaps render loose. `pagina to-md --gfm` reverses `<dl>` back to
  `Term\n: description`.

[GitHub Flavored Markdown]: https://github.github.com/gfm/

### Syntax highlighting

Fenced code blocks highlight through the `pagina::highlight::SyntaxHighlighter`
trait: implement `write_highlighted(&self, out, lang, code)` (HTML for the
code *content* only; the `<pre><code class="language-…">` wrapper stays with
the renderer) and pass it to a renderer:

- `markdown_to_html::convert_with_highlighter(input, options, Some(&hl))`
- `ast::render_html_with_highlighter(&doc, Some(&hl))`
- `stream::render_events_to_html_with_highlighter(&events, Some(&hl))`

`language_from_info` extracts the fence language, and implementations that do
not highlight are expected to fall back to escaping (`code` carries the same
trailing `\n` as the default escaped path, so fallbacks stay byte-identical).

```rust
use pagina::highlight::SyntaxHighlighter;

struct Upper;
impl SyntaxHighlighter for Upper {
    fn write_highlighted(&self, out: &mut String, lang: &str, code: &str) {
        out.push_str(&format!("<!--{}-->{}", lang, code));
    }
}

let html = pagina::markdown_to_html::convert_with_highlighter(
    "```rust\nlet x = 1;\n```\n",
    pagina::markdown_to_html::Options::default(),
    Some(&Upper),
)
.unwrap();
assert!(html.contains("<!--rust-->"));
```

The `syntax-highlight` cargo feature (default off) adds a `syntect`-backed
adapter, `SyntectAdapter::new(&SyntaxSet)`, emitting class-based spans you
style with your own CSS. It is off by default so the default build stays
dependency-free and code blocks render HTML-escaped.

### Math (GFM)

Dollar math is a GFM extension (`to-html --gfm`, `convert_gfm`, `parse_gfm`):

- Inline `$…$` renders `<span class="math-inline">…content…</span>`.
- Display `$$…$$` renders `<div class="math-display">…content…</div>`.

Content is passed through verbatim (HTML-escaped, no inline parsing inside),
so pair the `math-inline`/`math-display` classes with a client-side renderer
such as KaTeX or MathJax to typeset the formulas. `pagina to-md --gfm` maps
the span/div shapes back to `$…$` / `$$…$$`. Dollar heuristics keep currency
literal: `$5` and `$5 and $10` stay plain text.

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
