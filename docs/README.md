# pagina integration guides

pagina renders Markdown **fragments** to HTML **fragments** (no page
template, no wrapper). These guides show how to plug that output into the
common static-doc pipelines without fighting their renderers.

## Guides

- [**mdBook**](mdbook.md) — preprocessor that rewrites each chapter body with
  pagina's HTML, or a pre-render step for external sources. Covers the
  preprocessing protocol, `book.toml` wiring, and why `{{#include}}` is not
  the right tool for embedding raw HTML.
- [**Zola**](zola.md) — frontmatter compatibility (`+++` TOML fences), a
  pre-build CLI step feeding a `load_data`/`safe` shortcode, and a library
  helper for sanitizing user content (the CLI has no sanitize flag).
- [**cargo-doc / rustdoc**](cargo-doc.md) — generating README/guide HTML with
  pagina from an xtask (library API, syntect highlighting) or a `build.rs`
  shelling out to the CLI, plus what to keep rustdoc-specific away from
  pagina-generated pages.

## Facts that apply everywhere (pagina v0.2)

- Output is an HTML fragment: no `<html>`/`<head>`, no template, no TOC.
- GFM is a runtime flag (`--gfm` / `Options::gfm()`), not a cargo feature.
  Pipe tables always render.
- A leading `---` (YAML) / `+++` (TOML) frontmatter block is detected and
  silently stripped in every mode, feature or not.
- Features: `frontmatter`, `sanitize`, `syntax-highlight`, `wasm`
  (default = none). `sanitize` is library/WASM-only — the v0.2 CLI has no
  `--sanitize` flag.
- Raw HTML passes through verbatim per CommonMark — treat output as
  unsanitized unless you run a sanitizer.

## Related

- [`doc/pagina.1`](../doc/pagina.1) — man page for the `pagina` CLI.