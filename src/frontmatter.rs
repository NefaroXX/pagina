//! Optional YAML frontmatter support (`---` / `+++` leading blocks).
//!
//! A document may start with a metadata block whose first line is exactly
//! `---` (or `+++`) and whose closing line repeats the same delimiter:
//!
//! ```markdown
//! ---
//! title: Hello
//! tags: [rust, docs]
//! ---
//! # Hello
//! ```
//!
//! The block is metadata, not content: [`crate::markdown_to_html`] silently
//! strips it before conversion (with or without the `frontmatter` cargo
//! feature), so a fenced document renders exactly like its body alone and
//! CommonMark compliance is unaffected.
//!
//! Because `---` is live CommonMark syntax (thematic break / setext
//! underline), a `---` block only counts as frontmatter when it contains a
//! top-level `key:` mapping line — setext documents such as
//! `---\nFoo\n---` and `<hr />` runs such as `---\n---` (spec examples 96
//! and 98) are left untouched. `+++` has no CommonMark meaning and needs
//! no such guard.
//!
//! # Cargo feature
//!
//! The `frontmatter` feature (off by default, so the default build stays
//! dependency-free) parses the block with `yaml-rust` (pure Rust) and
//! exposes it as [`Frontmatter::data`]. Without the feature, detection,
//! stripping, verbatim preservation, and the round-trip helpers below still
//! work; only the parsed representation is reduced to a tiny subset
//! (top-level `key: value` pairs — see the limitation note on
//! [`Frontmatter::data`]).
//!
//! # Round-trip (Markdown side only)
//!
//! [`html_to_markdown`](crate::html_to_markdown) cannot carry frontmatter:
//! HTML has no frontmatter concept, so converting Markdown (with
//! frontmatter) to HTML and back yields body-only Markdown. Keep the
//! [`Frontmatter`] returned by [`parse_with_frontmatter`] (or by
//! [`markdown_to_html_with_frontmatter`]) aside and re-attach it with
//! [`prepend_frontmatter`]:
//!
//! ```
//! use pagina::frontmatter::{parse_with_frontmatter, prepend_frontmatter};
//! use pagina::{html_to_markdown, markdown_to_html};
//!
//! let md = "---\ntitle: Hi\n---\n# Hi\n";
//! let (fm, body) = parse_with_frontmatter(md);
//! let fm = fm.expect("frontmatter present");
//! let html = markdown_to_html::convert(body).unwrap();
//! assert_eq!(html, "<h1>Hi</h1>\n");
//! let back = html_to_markdown::convert(&html).unwrap();
//! let round_tripped = prepend_frontmatter(&fm, &back);
//! assert_eq!(round_tripped, md);
//! ```

#[cfg(not(feature = "frontmatter"))]
use std::collections::HashMap;

use crate::error::Result;

#[cfg(feature = "frontmatter")]
use yaml_rust::YamlLoader;

// ---------------------------------------------------------------------------
// Frontmatter struct
// ---------------------------------------------------------------------------

/// A leading `---` / `+++` metadata block: parsed data plus the exact
/// source text of the block.
///
/// `original` is preserved verbatim (opening delimiter line through the
/// closing delimiter line, without the trailing line break), so
/// [`prepend_frontmatter`] reproduces the input byte-for-byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frontmatter {
    /// Exact source bytes of the block (delimiters included, verbatim).
    pub original: String,
    /// Parsed metadata.
    ///
    /// With the `frontmatter` cargo feature this is the first YAML document
    /// parsed by `yaml-rust` (`Yaml::Null` for an empty block,
    /// `Yaml::BadValue` when the block is not valid YAML — detection still
    /// succeeds and `original` is still preserved verbatim).
    ///
    /// Without the feature this is a tiny YAML-subset fallback limited to
    /// top-level `key: value` pairs: blank lines, `#` comments, and
    /// indented (nested) lines are skipped; surrounding single or double
    /// quotes are stripped from values. Lists, nested maps, and multiline
    /// scalars are NOT parsed — enable the `frontmatter` feature for those.
    #[cfg(feature = "frontmatter")]
    pub data: yaml_rust::Yaml,
    /// Parsed metadata (no-feature fallback; see the feature-enabled
    /// [`Frontmatter::data`] docs for the full-fidelity alternative).
    #[cfg(not(feature = "frontmatter"))]
    pub data: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Detection (dependency-free, always available)
// ---------------------------------------------------------------------------

/// Byte-level split of a leading frontmatter block.
///
/// Returns `(original_verbatim, inner_yaml, body)` on success.
/// Detection rules:
/// - The first line (after an optional UTF-8 BOM) must be exactly `---` or
///   `+++`, allowing trailing spaces/tabs. No leading whitespace: an
///   indented fence is content, not metadata.
/// - The closing line must repeat the *same* delimiter under the same
///   rules. Without a closing line there is no frontmatter (a lone leading
///   `---` stays a thematic break / setext underline per CommonMark).
/// - A `---` block additionally requires a top-level `key:` mapping line
///   inside (see `inner_has_mapping`); `+++` blocks need no content guard.
/// - `original` spans from byte 0 through the end of the closing delimiter
///   text (BOM included when present). `body` is everything after the
///   closing line's line break, verbatim.
fn split_raw(input: &str) -> Option<(&str, &str, &str)> {
    let stripped = input.strip_prefix('\u{FEFF}').unwrap_or(input);
    let base = input.len() - stripped.len();

    // First line (content excludes `\n`, then an optional `\r`).
    let first_end = stripped.find('\n').map(|i| i + 1).unwrap_or(stripped.len());
    let first = &stripped[..first_end];
    let content = first.strip_suffix('\n').unwrap_or(first);
    let content = content.strip_suffix('\r').unwrap_or(content);
    // `trim_end_matches` strips trailing blanks only: any leading whitespace
    // survives and fails the comparison, so indented fences are rejected.
    let delimiter = content.trim_end_matches([' ', '\t']);
    if delimiter != "---" && delimiter != "+++" {
        return None;
    }

    // Scan for the closing fence (same delimiter, same strictness).
    let mut pos = first_end;
    while pos <= stripped.len() {
        let line_end = stripped[pos..]
            .find('\n')
            .map(|i| pos + i + 1)
            .unwrap_or(stripped.len());
        let line = &stripped[pos..line_end];
        let line_content = line.strip_suffix('\n').unwrap_or(line);
        let line_content = line_content.strip_suffix('\r').unwrap_or(line_content);
        // Same strictness as the opening fence: exact delimiter with only
        // trailing blanks tolerated (leading blanks fail the comparison).
        if line_content.trim_end_matches([' ', '\t']) == delimiter {
            let original = &input[..base + pos + delimiter.len()];
            let inner = &stripped[first_end..pos];
            let body = &stripped[line_end..];
            // `line_end` already sits past the closing fence's line break
            // when one is present; when the fence is the final line without
            // a trailing newline, `body` is already empty.
            //
            // `---` guard: the fence is live CommonMark syntax, so the
            // block only counts as metadata when it holds a YAML mapping
            // (keeps setext/hr documents like `---\nFoo\n---` untouched).
            // `+++` has no CommonMark meaning: no guard needed.
            if delimiter == "---" && !inner_has_mapping(inner) {
                return None;
            }
            return Some((original, inner, body));
        }
        if line_end == stripped.len() {
            // Reached EOF (last line unterminated) without a closing fence.
            // Check the final line itself once: handled above, so fall out.
            break;
        }
        pos = line_end;
    }
    None
}

/// True when `inner` contains at least one top-level `key:` mapping line.
///
/// A `---` fence is live CommonMark syntax (thematic break / setext
/// underline), so a bare `---` block is only metadata when its content
/// looks like a YAML mapping. Without this guard, spec documents such as
/// `---\nFoo\n---\n…` (setext headings) or `---\n---` (two `<hr />`s)
/// would be misread as frontmatter. `+++` needs no guard: it has no
/// CommonMark meaning and cannot collide with the spec suite.
fn inner_has_mapping(inner: &str) -> bool {
    for line in inner.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        // Only column-0 lines count: indented lines are nested content or
        // code, and `#` lines are comments, not mappings.
        if line.starts_with([' ', '\t', '#']) || line.is_empty() {
            continue;
        }
        let Some(colon) = line.find(':') else {
            continue;
        };
        let key = line[..colon].trim();
        if !key.is_empty() && !key.contains([' ', '\t']) {
            return true;
        }
    }
    false
}

/// Strip a leading frontmatter block, returning the body.
///
/// Always available (no cargo feature required). Returns `input` unchanged
/// when no valid block is present.
pub(crate) fn strip_frontmatter_body(input: &str) -> &str {
    match split_raw(input) {
        Some((_, _, body)) => body,
        None => input,
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse the inner YAML text of a frontmatter block.
#[cfg(feature = "frontmatter")]
fn parse_data(inner: &str) -> yaml_rust::Yaml {
    match YamlLoader::load_from_str(inner) {
        Ok(mut docs) => docs.pop().unwrap_or(yaml_rust::Yaml::Null),
        Err(_) => yaml_rust::Yaml::BadValue,
    }
}

/// Tiny YAML-subset fallback (no-feature builds): top-level `key: value`
/// pairs only. See [`Frontmatter::data`] for the limitation note.
#[cfg(not(feature = "frontmatter"))]
fn parse_data(inner: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in inner.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() || line.starts_with([' ', '\t', '#']) {
            continue;
        }
        let Some(colon) = line.find(':') else {
            continue;
        };
        let key = line[..colon].trim();
        if key.is_empty() || key.contains([' ', '\t']) {
            continue;
        }
        let mut value = line[colon + 1..].trim().to_string();
        if value.len() >= 2 {
            let bytes = value.as_bytes();
            let (first, last) = (bytes[0], bytes[value.len() - 1]);
            if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
                value = value[1..value.len() - 1].to_string();
            }
        }
        map.insert(key.to_string(), value);
    }
    map
}

/// Split Markdown into an optional [`Frontmatter`] and the body.
///
/// Infallible by design: an unparseable block still yields `Some` (with
/// best-effort `data`) because `original` preservation never depends on
/// parsing. Returns `(None, input)` when no valid block is present.
pub fn parse_with_frontmatter(input: &str) -> (Option<Frontmatter>, &str) {
    match split_raw(input) {
        None => (None, input),
        Some((original, inner, body)) => {
            let data = parse_data(inner);
            (
                Some(Frontmatter {
                    original: original.to_string(),
                    data,
                }),
                body,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Convert + re-attach helpers
// ---------------------------------------------------------------------------

/// Convert Markdown to HTML, keeping a leading frontmatter block aside.
///
/// Returns the parsed [`Frontmatter`] (when present) plus the HTML of the
/// body. Equivalent to [`parse_with_frontmatter`] followed by
/// [`crate::markdown_to_html::convert_with`] on the body.
pub fn markdown_to_html_with_frontmatter_with(
    input: &str,
    options: crate::markdown_to_html::Options,
) -> Result<(Option<Frontmatter>, String)> {
    let (frontmatter, body) = parse_with_frontmatter(input);
    let html = crate::markdown_to_html::convert_with(body, options)?;
    Ok((frontmatter, html))
}

/// Convert Markdown to HTML, keeping a leading frontmatter block aside.
///
/// Pure CommonMark (equivalent to [`crate::markdown_to_html::convert`] on
/// the body).
///
/// # Examples
///
/// ```
/// let (fm, html) =
///     pagina::frontmatter::markdown_to_html_with_frontmatter("---\ntitle: Hi\n---\n# Hi\n")
///         .unwrap();
/// assert!(fm.is_some());
/// assert_eq!(html, "<h1>Hi</h1>\n");
/// ```
pub fn markdown_to_html_with_frontmatter(input: &str) -> Result<(Option<Frontmatter>, String)> {
    markdown_to_html_with_frontmatter_with(input, crate::markdown_to_html::Options::default())
}

/// Convert Markdown to HTML with GFM extensions, keeping a leading
/// frontmatter block aside (equivalent to
/// [`crate::markdown_to_html::convert_gfm`] on the body).
pub fn markdown_to_html_with_frontmatter_gfm(input: &str) -> Result<(Option<Frontmatter>, String)> {
    markdown_to_html_with_frontmatter_with(input, crate::markdown_to_html::Options::gfm())
}

/// Re-attach frontmatter ahead of a body (typically body-only Markdown
/// produced by [`crate::html_to_markdown`] during an HTML round-trip).
///
/// Uses the verbatim [`Frontmatter::original`], so
/// `prepend_frontmatter(parse(md).0, parse(md).1) == md` byte-for-byte
/// (modulo the line break joining the fence to the body, which matches the
/// original CRLF/LF style).
///
/// # Examples
///
/// ```
/// use pagina::frontmatter::{parse_with_frontmatter, prepend_frontmatter};
///
/// let md = "---\ntitle: Hi\n---\n# Hi\n";
/// let (fm, body) = parse_with_frontmatter(md);
/// let rebuilt = prepend_frontmatter(&fm.expect("frontmatter present"), body);
/// assert_eq!(rebuilt, md);
/// ```
pub fn prepend_frontmatter(frontmatter: &Frontmatter, body: &str) -> String {
    let newline = if frontmatter.original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut out = String::with_capacity(frontmatter.original.len() + newline.len() + body.len());
    out.push_str(&frontmatter.original);
    out.push_str(newline);
    out.push_str(body);
    out
}
