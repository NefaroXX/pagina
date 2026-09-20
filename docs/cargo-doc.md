# Generating project docs from Markdown with pagina (build.rs / xtask)

rustdoc already renders inline Markdown in `///` docs, and a README can be
pulled in with `#![doc = include_str!("../README.md")]`. Use pagina when you
want **its** rendering — GFM tables, task lists, footnotes, dollar math, or
syntect syntax highlighting — to control how a README or guide becomes HTML,
e.g. a companion docs page, a man-page–style reference, or a `docs/` build.

Two constraints up front (verified against this repo):

1. **A crate cannot depend on its own library from `build.rs`.** To call the
   *library* API, put the generator in an `xtask` workspace member or a
   `tools/` helper crate that depends on `pagina`.
2. The v0.2 **CLI has no `--sanitize` flag** (only `to-html`/`to-md`, `--gfm`,
   stdin/stdout `-`). Sanitization is library/WASM-only — use the library
   helper if you need it.

## Option A — xtask / helper crate (library API)

`xtask/Cargo.toml`:

```toml
[package]
name = "xtask"
edition = "2021"

[dependencies]
pagina = { version = "0.2", features = ["frontmatter", "syntax-highlight"] }
yaml-rust = "0.4"   # to name `yaml_rust::Yaml` (Frontmatter.data with `frontmatter`)
syntect = "5"       # for SyntaxSet::load_defaults_newlines
```

`xtask/src/main.rs` — convert `README.md` to HTML with the frontmatter title
as an `<h1>` and syntect-highlighted fenced code:

```rust
use pagina::frontmatter::parse_with_frontmatter;
use pagina::highlight::SyntectAdapter;
use pagina::html_escape::escape_html;
use pagina::markdown_to_html::{convert_with_highlighter, Options};
use syntect::parsing::SyntaxSet;
use yaml_rust::Yaml;

fn main() {
    let md = std::fs::read_to_string("README.md").expect("README.md");
    let (fm, body) = parse_with_frontmatter(&md);

    // `frontmatter` feature => `Frontmatter.data` is `yaml_rust::Yaml`.
    let title = fm.as_ref().and_then(|f| match &f.data {
        Yaml::Hash(map) => map
            .get(&Yaml::String("title".into()))
            .and_then(|v| match v {
                Yaml::String(s) => Some(s.clone()),
                _ => None,
            }),
        _ => None,
    });

    // Load syntaxes once; SyntectAdapter borrows the set, so keep it alive.
    let syntaxes = SyntaxSet::load_defaults_newlines();
    let hl = SyntectAdapter::new(&syntaxes);

    let body_html =
        convert_with_highlighter(body, Options::gfm(), Some(&hl)).expect("convert");
    let heading = title
        .map(|t| format!("<h1>{}</h1>\n", escape_html(&t)))
        .unwrap_or_default();

    std::fs::create_dir_all("target/generated/docs").unwrap();
    std::fs::write(
        "target/generated/docs/readme.html",
        format!("{heading}{body_html}"),
    )
    .expect("write");
}
```

Run with `cargo run -p xtask` and copy the fragment into your docs/site build.

> Without `frontmatter`, `Frontmatter.data` is a `HashMap<String, String>` and
> the title lookup becomes `fm.data.get("title")` (no `yaml_rust` needed).

## Option B — build.rs shelling out to the CLI

If the system has `pagina` installed (`cargo install pagina`), a `build.rs`
can convert a Markdown source into a fragment:

```rust
// build.rs
fn main() {
    let out = std::env::var("OUT_DIR").unwrap();
    let status = std::process::Command::new("pagina")
        .args(["to-html", "--gfm", "README.md", &format!("{out}/readme.html")])
        .status()
        .expect("run pagina");
    assert!(status.success(), "pagina to-html failed");
    println!("cargo:rerun-if-changed=README.md");
}
```

- The fragment lands in `OUT_DIR`; rustdoc itself will not know about it
  unless you `include_str!` it into a doc attribute — but the whole point of
  this pipeline is to *not* hand the Markdown back to rustdoc's renderer.
- Compiled as part of `cargo doc`, so it always regenerates from the README.

## Rustdoc-friendly notes

- **Code fences.** rustdoc-style info strings such as `rust,ignore` and
  `rust,no_run` resolve to the `rust` token: `language_from_info("rust,no_run")
  == "rust"` (verified in `src/highlight.rs` tests). Fences you already write
  for rustdoc therefore highlight correctly through `SyntectAdapter` and emit
  `class="language-rust"`.
- **Raw HTML.** Both rustdoc and pagina pass raw HTML through verbatim. If the
  Markdown is untrusted, use the `sanitize` feature's
  `convert_gfm_sanitized` instead of `convert_with_highlighter`.
- **Math.** With GFM, `$…$` → `<span class="math-inline">` and `$$…$$` →
  `<div class="math-display">`. Pair those classes with KaTeX or MathJax;
  rustdoc has no built-in math renderer for these shapes.
- **Intra-doc links are out of scope.** pagina treats `[foo](crate::foo)` as a
  plain link and does **not** resolve rustdoc paths. Keep intra-doc-link
  processing separate from pagina-generated pages (e.g. a placeholder/rewrite
  pass after conversion).
- **Frontmatter.** A leading `---`/`+++` block is stripped automatically, so a
  README with `+++` TOML frontmatter (title, extra metadata) converts cleanly;
  the `frontmatter` feature additionally parses it.

## Feature flags relevant here

| Feature | Needed for | Notes |
| --- | --- | --- |
| *(none)* | GFM conversion | `convert_gfm` / CLI `--gfm` |
| `frontmatter` | `Frontmatter.data` as `yaml_rust::Yaml` (full `---`/`+++` parse) | detection + stripping work without it |
| `syntax-highlight` | `SyntectAdapter` (syntect, class-based spans) | include `syntect = "5"` for `SyntaxSet` |
| `sanitize` | `convert_sanitized` / `convert_gfm_sanitized` for untrusted Markdown | library/WASM only |
| `wasm` | browser-side conversion (`pagina-wasm` npm) | not needed for rustdoc/xtask |

This repository already ships `doc/pagina.1` (a man page); the guides in
`docs/` follow the same "generated from Markdown" idea — a good first target
for the xtask above.