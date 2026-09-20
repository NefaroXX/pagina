# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-09-20

### Added

- Public AST (`pagina::ast`, re-exported `parse` / `parse_gfm` /
  `render_html` / `render_markdown` at the crate root): an owned
  `Document` tree that renders back to HTML or Markdown; for matched
  options `render_html(&parse(md))` is byte-identical to the legacy
  `convert(md)` path.
- Generic visitor walker (`pagina::visitor`): the `Visitor` trait plus
  pre-order `walk_document` / `walk_block` / `walk_inline` / `walk_inlines`
  traversal over the AST.
- Streaming pull API (`pagina::stream`): `parse_stream`, `collect_events`,
  `events_from_document`, and the `Parser` iterator (`Iterator` +
  `ExactSizeIterator`) yielding owned `Event` / `Tag` / `TagEnd`;
  `render_events_to_html` / `render_events_to_html_with_highlighter` render
  event slices back to HTML (byte-identical to `ast::render_html`).
- GFM footnotes (`[^label]` references plus `[^label]:` definitions rendered
  as a `<section class="footnotes" data-footnotes>` footer with backrefs)
  and PHP Markdown Extra-style definition lists (`Term` + `: description`
  → `<dl>`/`<dt>`/`<dd>`); `html_to_markdown_gfm` reverses both back to
  source form.
- Syntax highlighting hook: the `highlight::SyntaxHighlighter` trait with
  `convert_with_highlighter` / `render_html_with_highlighter` /
  `render_events_to_html_with_highlighter`, plus the `syntax-highlight`
  cargo feature (default off) adding a `syntect`-backed `SyntectAdapter`
  emitting class-based spans.
- GFM dollar-math passthrough: `$…$` → `<span class="math-inline">`,
  `$$…$$` → `<div class="math-display">` (content HTML-escaped, verbatim);
  `html_to_markdown_gfm` maps the shapes back to `$…$` / `$$…$$`.

### Docs

- README: badge row (CommonMark 652/652, crates.io, docs.rs, CI, npm
  `pagina-wasm`) and usage examples for GFM mode, frontmatter, sanitize, and
  the WASM entry point.
- README: P2 Library API subsections for the AST + visitor, streaming
  events, footnotes & definition lists, syntax highlighting, and math, with
  matching CHANGELOG entries.

## [0.1.1] - 2026-09-20

Initial release — a bidirectional Markdown ↔ HTML converter in pure Rust with
zero required dependencies. Markdown → HTML conforms to the CommonMark 0.31.2
spec (652/652 examples), while opt-in ecosystem features (GFM, frontmatter,
WASM, sanitize) ship behind cargo feature flags so the default build stays
dependency-free and byte-stable.

### Added

- Bidirectional conversion: Markdown → HTML and HTML → Markdown.
- **CommonMark 0.31.2 compliance: 652/652 (100%)** for Markdown → HTML,
  verified by `tests/commonmark_compliance.rs` against
  `tests/fixtures/spec.txt`.
- GFM pipe tables, always on (spec-neutral, render in both modes).
- GFM mode (`--gfm` CLI flag; `convert_gfm` / `html_to_markdown_gfm` library
  entry points): task lists, strikethrough, bare autolinks.
- Frontmatter `---` (YAML) / `+++` (TOML) detection, stripping, and
  round-trip via `parse_with_frontmatter` / `prepend_frontmatter`; the
  optional `frontmatter` feature adds full-fidelity `yaml-rust` parsing.
- WASM bindings (optional `wasm` feature, `wasm-bindgen`), published to the
  `pagina-wasm` npm package.
- HTML sanitization (optional `sanitize` feature): zero-dependency denylist
  (`convert_sanitized`, `convert_gfm_sanitized`, `convert_with_sanitized`,
  and WASM variants).
- CLI: stdin/stdout piping via `-`, `--help`, `--version`.
- Release profile tuned for speed (opt-level 3 + LTO) and a size-optimized
  WASM build profile.

### Safety

- Per CommonMark, raw HTML and `javascript:`/`data:` URLs are preserved
  verbatim; consumers must sanitize before rendering. The optional `sanitize`
  feature provides a built-in first layer.