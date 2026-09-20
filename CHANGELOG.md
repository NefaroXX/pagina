# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Docs

- README: badge row (CommonMark 652/652, crates.io, docs.rs, CI, npm
  `pagina-wasm`) and usage examples for GFM mode, frontmatter, sanitize, and
  the WASM entry point.

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