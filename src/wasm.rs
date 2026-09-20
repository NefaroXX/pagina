//! WebAssembly bindings for `pagina` (behind the `wasm` cargo feature).
//!
//! Thin [`wasm-bindgen`](https://crates.io/crates/wasm-bindgen) wrappers
//! around the normal Rust API. No parser logic lives here: every function
//! delegates to [`crate::markdown_to_html`], [`crate::html_to_markdown`],
//! or [`crate::frontmatter`], so WASM output is byte-identical to the
//! native build.
//!
//! JS-facing types are kept to `string` / `bool` / `Option<string>` (plus a
//! `Result<string, JsValue>` for fallible conversions) so the `wasm`
//! feature only needs `wasm-bindgen` — no `js-sys` or `serde`.
//!
//! See the README section "WebAssembly / npm" for build commands.

use wasm_bindgen::prelude::*;

fn map_err(e: crate::error::Error) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// Convert Markdown to HTML (pure CommonMark; GFM tables always on).
///
/// Throws a JS error if the conversion fails.
#[wasm_bindgen]
pub fn markdown_to_html(input: &str) -> Result<String, JsValue> {
    crate::markdown_to_html::convert(input).map_err(map_err)
}

/// Convert Markdown to HTML with GFM extensions (task lists,
/// strikethrough, bare autolinks; tables render in both modes).
///
/// Throws a JS error if the conversion fails.
#[wasm_bindgen]
pub fn markdown_to_html_gfm(input: &str) -> Result<String, JsValue> {
    crate::markdown_to_html::convert_gfm(input).map_err(map_err)
}

/// Convert HTML to Markdown (CommonMark-compatible reverse direction).
///
/// Throws a JS error if the conversion fails.
#[wasm_bindgen]
pub fn html_to_markdown(input: &str) -> Result<String, JsValue> {
    crate::html_to_markdown::convert(input).map_err(map_err)
}

/// Convert HTML to Markdown with GFM extensions (`~~del~~`, task-list
/// checkboxes, bare anchors as plain URLs).
///
/// Throws a JS error if the conversion fails.
#[wasm_bindgen]
pub fn html_to_markdown_gfm(input: &str) -> Result<String, JsValue> {
    crate::html_to_markdown::convert_gfm(input).map_err(map_err)
}

/// True when `input` starts with a valid `---` / `+++` frontmatter block.
///
/// Same detection as [`crate::frontmatter::parse_with_frontmatter`].
#[wasm_bindgen]
pub fn frontmatter_has(input: &str) -> bool {
    crate::frontmatter::parse_with_frontmatter(input)
        .0
        .is_some()
}

/// Body of `input` with a leading frontmatter block stripped.
///
/// Returns `input` unchanged when no valid block is present.
#[wasm_bindgen]
pub fn frontmatter_body(input: &str) -> String {
    crate::frontmatter::parse_with_frontmatter(input)
        .1
        .to_string()
}

/// Verbatim source text of the leading frontmatter block (delimiters
/// included), or `None` when `input` has no valid block.
///
/// Pair with [`prepend_frontmatter`] for a byte-exact round-trip:
/// `prepend_frontmatter(frontmatter_original(md), frontmatter_body(md))`.
#[wasm_bindgen]
pub fn frontmatter_original(input: &str) -> Option<String> {
    crate::frontmatter::parse_with_frontmatter(input)
        .0
        .map(|fm| fm.original)
}

/// Value of one top-level `key` in the leading frontmatter block, or `None`
/// when the block (or the key) is absent.
///
/// With the `frontmatter` cargo feature this reads the mapping (`---`
/// blocks as YAML via `yaml-rust`, `+++` blocks as TOML converted to the
/// same shape; scalars stringified: strings verbatim, numbers/bools via
/// `to_string`); without it, it reads the dependency-free fallback maps
/// (`key: value` for `---`, `key = value` with `[table]` prefixes for
/// `+++`). Non-scalar values (lists, nested maps, null) yield `None`.
#[wasm_bindgen]
pub fn frontmatter_get(input: &str, key: &str) -> Option<String> {
    let (fm, _) = crate::frontmatter::parse_with_frontmatter(input);
    let fm = fm?;
    frontmatter_lookup(&fm, key)
}

/// Re-attach a verbatim frontmatter block (as returned by
/// [`frontmatter_original`]) ahead of `body`.
///
/// Uses the block's original CRLF/LF style, matching
/// [`crate::frontmatter::prepend_frontmatter`].
#[wasm_bindgen]
pub fn prepend_frontmatter(original: &str, body: &str) -> String {
    let newline = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut out = String::with_capacity(original.len() + newline.len() + body.len());
    out.push_str(original);
    out.push_str(newline);
    out.push_str(body);
    out
}

#[cfg(feature = "frontmatter")]
fn frontmatter_lookup(fm: &crate::frontmatter::Frontmatter, key: &str) -> Option<String> {
    use yaml_rust::Yaml;
    let Yaml::Hash(map) = &fm.data else {
        return None;
    };
    match map.get(&Yaml::String(key.to_string()))? {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Real(r) => Some(r.clone()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

#[cfg(not(feature = "frontmatter"))]
fn frontmatter_lookup(fm: &crate::frontmatter::Frontmatter, key: &str) -> Option<String> {
    fm.data.get(key).cloned()
}
