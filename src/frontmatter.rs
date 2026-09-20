//! Optional YAML/TOML frontmatter support (`---` / `+++` leading blocks).
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
//! Delimiter rule: `---` blocks parse as YAML, `+++` blocks parse as TOML
//! (a small dependency-free subset — see [`Frontmatter::data`]). Either way
//! the block is metadata, not content: [`crate::markdown_to_html`] silently
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
//! dependency-free) parses `---` blocks with `yaml-rust` (pure Rust) and
//! `+++` blocks with the built-in TOML-subset parser, exposing both as
//! [`Frontmatter::data`]. Without the feature, detection, stripping,
//! verbatim preservation, and the round-trip helpers below still work; only
//! the parsed representation is reduced to a tiny subset (top-level
//! `key: value` pairs for `---`, `key = value` pairs with `[table]`
//! prefixes for `+++` — see the limitation note on
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
    /// parsed by `yaml-rust` for `---` blocks, or the TOML-subset value for
    /// `+++` blocks converted to the same `Yaml` shape (tables become
    /// `Yaml::Hash`, arrays become `Yaml::Array`; strings, integers, floats
    /// (`Yaml::Real`), and booleans map directly; TOML datetimes are kept
    /// as strings). Empty blocks yield `Yaml::Null`; unparseable blocks
    /// yield `Yaml::BadValue` — detection still succeeds and `original` is
    /// still preserved verbatim either way.
    ///
    /// The TOML subset covers single-line values (quoted strings with the
    /// standard escapes, integers including hex/octal/binary, floats,
    /// booleans, datetimes-as-strings, arrays, inline tables), `[table]`
    /// headers (including dotted `[a.b]`), and dotted keys. NOT supported
    /// (the whole block becomes `BadValue`): multiline strings/arrays
    /// spanning lines, `[[array-of-tables]]` headers, and conflicting
    /// table/value shapes. No new dependencies: the subset parser is
    /// built in.
    ///
    /// Without the feature this is a tiny subset fallback: for `---`
    /// blocks, top-level `key: value` pairs (blank lines, `#` comments, and
    /// indented/nested lines skipped; surrounding quotes stripped; lists,
    /// nested maps, and multiline scalars NOT parsed); for `+++` blocks,
    /// `key = value` pairs flattened with `[table]`/dotted-key prefixes
    /// (`[server]` plus `host = "x"` becomes `"server.host"`; strings
    /// unquoted, other scalars/arrays/inline tables kept verbatim;
    /// `[[array]]` headers treated like `[array]`). Enable the
    /// `frontmatter` feature for full-fidelity parsing.
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

/// Which syntax a fence uses: `---` blocks are YAML, `+++` blocks are TOML.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fence {
    Yaml,
    Toml,
}

/// Byte-level split of a leading frontmatter block.
///
/// Returns `(fence, original_verbatim, inner, body)` on success.
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
fn split_raw(input: &str) -> Option<(Fence, &str, &str, &str)> {
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
            let fence = if delimiter == "+++" {
                Fence::Toml
            } else {
                Fence::Yaml
            };
            return Some((fence, original, inner, body));
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
        Some((_, _, _, body)) => body,
        None => input,
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse the inner text of a frontmatter block (`---` as YAML, `+++` as
/// the TOML subset below).
#[cfg(feature = "frontmatter")]
fn parse_data(fence: Fence, inner: &str) -> yaml_rust::Yaml {
    match fence {
        Fence::Yaml => match YamlLoader::load_from_str(inner) {
            Ok(mut docs) => docs.pop().unwrap_or(yaml_rust::Yaml::Null),
            Err(_) => yaml_rust::Yaml::BadValue,
        },
        Fence::Toml => parse_toml_to_yaml(inner),
    }
}

/// Tiny subset fallback (no-feature builds): `---` blocks parse as
/// top-level `key: value` pairs, `+++` blocks as flattened
/// `key = value` pairs. See [`Frontmatter::data`] for the limitation note.
#[cfg(not(feature = "frontmatter"))]
fn parse_data(fence: Fence, inner: &str) -> HashMap<String, String> {
    match fence {
        Fence::Yaml => parse_yaml_subset(inner),
        Fence::Toml => parse_toml_subset(inner),
    }
}

/// Tiny YAML-subset fallback (no-feature builds): top-level `key: value`
/// pairs only. See [`Frontmatter::data`] for the limitation note.
#[cfg(not(feature = "frontmatter"))]
fn parse_yaml_subset(inner: &str) -> HashMap<String, String> {
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

// ---------------------------------------------------------------------------
// TOML subset for `+++` blocks (dependency-free, shared by both `data`
// variants). Covers single-line values, `[table]` headers (including
// dotted `[a.b]`), and dotted keys. Whole-block `BadValue` (feature) or
// per-line skip (fallback) on anything outside the subset — see
// [`Frontmatter::data`].
// ---------------------------------------------------------------------------

/// Strip a trailing `#` comment from a TOML line/value slice.
///
/// The first `#` outside single/double-quoted strings starts a comment.
/// Handles `\"` escapes in basic strings; literal strings have no escapes.
fn strip_toml_comment(s: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_double => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return s[..i].trim_end(),
            _ => {}
        }
    }
    s
}

/// Byte index of the first `=` outside quoted strings, or `None`.
fn find_toml_eq(line: &str) -> Option<usize> {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_double => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '=' if !in_single && !in_double => return Some(i),
            _ => {}
        }
    }
    None
}

/// Split `s` on `sep` chars outside quotes and `[]`/`{}` nesting,
/// preserving every part verbatim. Returns `None` on unbalanced
/// quotes, escapes, or brackets.
fn split_top_level(s: &str, sep: char) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    let mut buf = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut depth = 0i32;
    for c in s.chars() {
        if escaped {
            buf.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_double => {
                buf.push(c);
                escaped = true;
            }
            '\'' if !in_double => {
                buf.push(c);
                in_single = !in_single;
            }
            '"' if !in_single => {
                buf.push(c);
                in_double = !in_double;
            }
            '[' | '{' if !in_single && !in_double => {
                buf.push(c);
                depth += 1;
            }
            ']' | '}' if !in_single && !in_double => {
                buf.push(c);
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            c if c == sep && !in_single && !in_double && depth == 0 => {
                parts.push(std::mem::take(&mut buf));
            }
            _ => buf.push(c),
        }
    }
    if in_single || in_double || escaped || depth != 0 {
        return None;
    }
    parts.push(buf);
    Some(parts)
}

/// Unquote one TOML string slice (basic `"..."` with escapes, literal
/// `'...'`, or single-line triple-quoted). Returns `None` when the slice
/// is not a well-formed single-line TOML string.
fn unquote_toml_string(s: &str) -> Option<String> {
    let t = s.trim();
    if t.len() >= 6 && t.starts_with("\"\"\"") && t.ends_with("\"\"\"") {
        let inner = &t[3..t.len() - 3];
        if inner.contains('\n') {
            return None;
        }
        return unescape_basic(inner);
    }
    if t.len() >= 6 && t.starts_with("'''") && t.ends_with("'''") {
        let inner = &t[3..t.len() - 3];
        if inner.contains('\n') {
            return None;
        }
        return Some(inner.to_string());
    }
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        return unescape_basic(&t[1..t.len() - 1]);
    }
    if t.len() >= 2 && t.starts_with('\'') && t.ends_with('\'') {
        let inner = &t[1..t.len() - 1];
        if inner.contains('\n') {
            return None;
        }
        return Some(inner.to_string());
    }
    None
}

/// Resolve the standard TOML basic-string escapes. Returns `None` on an
/// unknown escape or a truncated sequence.
fn unescape_basic(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'b' => out.push('\u{08}'),
            't' => out.push('\t'),
            'n' => out.push('\n'),
            'f' => out.push('\u{0C}'),
            'r' => out.push('\r'),
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            'u' => out.push(decode_hex_escape(&mut chars, 4)?),
            'U' => out.push(decode_hex_escape(&mut chars, 8)?),
            _ => return None,
        }
    }
    Some(out)
}

/// Decode a `\uXXXX` / `\UXXXXXXXX` escape tail.
fn decode_hex_escape(chars: &mut std::str::Chars<'_>, n: usize) -> Option<char> {
    let mut value = 0u32;
    for _ in 0..n {
        value = value * 16 + chars.next()?.to_digit(16)?;
    }
    char::from_u32(value)
}

/// Validate one dotted-path segment: quoted segments unquote, bare
/// segments must be non-empty and free of whitespace and structural
/// characters.
fn finish_key_segment(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    if t.starts_with('"') || t.starts_with('\'') {
        return unquote_toml_string(t);
    }
    if t.bytes().any(|b| {
        b.is_ascii_whitespace()
            || matches!(
                b,
                b'"' | b'\'' | b'#' | b'=' | b',' | b'[' | b']' | b'{' | b'}'
            )
    }) {
        return None;
    }
    Some(t.to_string())
}

/// Split a (possibly dotted) TOML key or table path into segments.
/// Dots inside quoted segments stay literal (`a."b.c"` -> `["a", "b.c"]`).
fn split_toml_path(s: &str) -> Option<Vec<String>> {
    split_top_level(s, '.')?
        .iter()
        .map(|part| finish_key_segment(part))
        .collect()
}

/// Parse a TOML integer (decimal plus `0x`/`0o`/`0b`, signs, and `_`
/// separators) into `i64`.
#[cfg(feature = "frontmatter")]
fn parse_toml_int(s: &str) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    let cleaned: String = s.chars().filter(|c| *c != '_').collect();
    let (neg, rest) = match cleaned.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, cleaned.strip_prefix('+').unwrap_or(&cleaned)),
    };
    let (radix, digits) =
        if let Some(h) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
            (16, h)
        } else if let Some(o) = rest.strip_prefix("0o").or_else(|| rest.strip_prefix("0O")) {
            (8, o)
        } else if let Some(b) = rest.strip_prefix("0b").or_else(|| rest.strip_prefix("0B")) {
            (2, b)
        } else {
            (10, rest)
        };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let magnitude = i64::from_str_radix(digits, radix).ok()?;
    if neg {
        magnitude.checked_neg()
    } else {
        Some(magnitude)
    }
}

/// Parse a TOML float, returning the `Yaml::Real` payload (canonical
/// lowercase, `_` separators removed). Plain integers never match: the
/// caller tries [`parse_toml_int`] first and a float marker (`.`, `e`,
/// `inf`, `nan`) is required here.
#[cfg(feature = "frontmatter")]
fn parse_toml_float(s: &str) -> Option<String> {
    let cleaned: String = s.chars().filter(|c| *c != '_').collect();
    let low = cleaned.to_ascii_lowercase();
    if ["inf", "+inf", "-inf", "nan", "+nan", "-nan"].contains(&low.as_str()) {
        return Some(low);
    }
    if !low.bytes().any(|b| b == b'.' || b == b'e') {
        return None;
    }
    low.parse::<f64>().ok()?;
    Some(low)
}

/// Loose TOML datetime probe (offset/date/time forms): starts with a digit
/// and uses only datetime alphabet characters with a `-` or `:` present.
/// Datetimes are kept as strings (no date type exists downstream).
#[cfg(feature = "frontmatter")]
fn is_toml_datetime(s: &str) -> bool {
    if s.is_empty() || !s.as_bytes()[0].is_ascii_digit() {
        return false;
    }
    let alphabet = s.bytes().all(|b| {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b':' | b'+' | b'.' | b'T' | b't' | b'Z' | b'z' | b' '
            )
    });
    alphabet && (s.contains('-') || s.contains(':'))
}

/// Parse one TOML value into its `Yaml` shape (feature builds).
/// Returns `None` for anything outside the subset (caller maps that to
/// `Yaml::BadValue` for the whole block).
#[cfg(feature = "frontmatter")]
fn parse_toml_value(s: &str) -> Option<yaml_rust::Yaml> {
    use yaml_rust::Yaml;
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    match t.as_bytes()[0] {
        b'"' | b'\'' => unquote_toml_string(t).map(Yaml::String),
        b'[' => {
            if !t.ends_with(']') {
                return None;
            }
            let inner = &t[1..t.len() - 1];
            if inner.trim().is_empty() {
                return Some(Yaml::Array(Vec::new()));
            }
            split_top_level(inner, ',')?
                .iter()
                .map(|item| parse_toml_value(item))
                .collect::<Option<Vec<Yaml>>>()
                .map(Yaml::Array)
        }
        b'{' => {
            if !t.ends_with('}') {
                return None;
            }
            let inner = &t[1..t.len() - 1];
            let mut table = Yaml::Hash(Default::default());
            if inner.trim().is_empty() {
                return Some(table);
            }
            for item in split_top_level(inner, ',')? {
                let eq = find_toml_eq(&item)?;
                let keys = split_toml_path(item[..eq].trim())?;
                if keys.is_empty() {
                    return None;
                }
                let value = parse_toml_value(strip_toml_comment(&item[eq + 1..]).trim())?;
                yaml_insert_path(&mut table, &keys, value)?;
            }
            Some(table)
        }
        _ => {
            if t == "true" {
                return Some(Yaml::Boolean(true));
            }
            if t == "false" {
                return Some(Yaml::Boolean(false));
            }
            if let Some(i) = parse_toml_int(t) {
                return Some(Yaml::Integer(i));
            }
            if let Some(f) = parse_toml_float(t) {
                return Some(Yaml::Real(f));
            }
            if is_toml_datetime(t) {
                return Some(Yaml::String(t.to_string()));
            }
            None
        }
    }
}

/// Insert `value` at `path` inside a `Yaml` tree, creating intermediate
/// hashes. Returns `None` when an intermediate segment already holds a
/// non-hash value (a conflicting table/value shape).
#[cfg(feature = "frontmatter")]
fn yaml_insert_path(
    root: &mut yaml_rust::Yaml,
    path: &[String],
    value: yaml_rust::Yaml,
) -> Option<()> {
    use yaml_rust::Yaml;
    if path.is_empty() {
        return None;
    }
    let mut current = root;
    for seg in &path[..path.len() - 1] {
        let Yaml::Hash(map) = current else {
            return None;
        };
        current = map
            .entry(Yaml::String(seg.clone()))
            .or_insert(Yaml::Hash(Default::default()));
        if !matches!(current, Yaml::Hash(_)) {
            return None;
        }
    }
    let Yaml::Hash(map) = current else {
        return None;
    };
    map.insert(Yaml::String(path[path.len() - 1].clone()), value);
    Some(())
}

/// Ensure the nested `[table]` path exists as hashes. Returns `false`
/// when a segment already holds a non-hash value.
#[cfg(feature = "frontmatter")]
fn yaml_ensure_table(root: &mut yaml_rust::Yaml, path: &[String]) -> bool {
    use yaml_rust::Yaml;
    let mut current = root;
    for seg in path {
        let Yaml::Hash(map) = current else {
            return false;
        };
        current = map
            .entry(Yaml::String(seg.clone()))
            .or_insert(Yaml::Hash(Default::default()));
        if !matches!(current, Yaml::Hash(_)) {
            return false;
        }
    }
    true
}

/// Parse a `+++` block as the TOML subset into a `Yaml` value (feature
/// builds). Empty blocks yield `Yaml::Null`; any line outside the subset
/// yields `Yaml::BadValue` for the whole block (detection and verbatim
/// `original` are unaffected).
#[cfg(feature = "frontmatter")]
fn parse_toml_to_yaml(inner: &str) -> yaml_rust::Yaml {
    use yaml_rust::Yaml;
    let mut root = Yaml::Hash(Default::default());
    let mut table: Vec<String> = Vec::new();
    let mut touched = false;
    for raw_line in inner.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            // `[[array-of-tables]]` is outside the subset.
            let header = strip_toml_comment(line).trim();
            if header.starts_with("[[") || !header.ends_with(']') {
                return Yaml::BadValue;
            }
            let path = match split_toml_path(header[1..header.len() - 1].trim()) {
                Some(p) if !p.is_empty() => p,
                _ => return Yaml::BadValue,
            };
            if !yaml_ensure_table(&mut root, &path) {
                return Yaml::BadValue;
            }
            table = path;
            touched = true;
            continue;
        }
        let Some(eq) = find_toml_eq(line) else {
            return Yaml::BadValue;
        };
        let keys = match split_toml_path(line[..eq].trim()) {
            Some(k) if !k.is_empty() => k,
            _ => return Yaml::BadValue,
        };
        let value = match parse_toml_value(strip_toml_comment(&line[eq + 1..]).trim()) {
            Some(v) => v,
            None => return Yaml::BadValue,
        };
        let mut full = table.clone();
        full.extend(keys);
        if yaml_insert_path(&mut root, &full, value).is_none() {
            return Yaml::BadValue;
        }
        touched = true;
    }
    if touched {
        root
    } else {
        Yaml::Null
    }
}

/// Parse a `+++` block into the flat no-feature fallback map (best effort:
/// unsupported lines are skipped). Table and dotted-key prefixes flatten
/// with `.` (`[server]` + `host = "x"` becomes `"server.host"`); strings
/// unquote, other scalars/arrays/inline tables stay verbatim.
/// `[[array]]` headers are treated like `[array]` (documented limitation).
#[cfg(not(feature = "frontmatter"))]
fn parse_toml_subset(inner: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut table: Vec<String> = Vec::new();
    for raw_line in inner.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            let mut header = strip_toml_comment(line).trim();
            header = header.strip_prefix('[').unwrap_or(header);
            header = header.strip_prefix('[').unwrap_or(header);
            header = header.strip_suffix(']').unwrap_or(header);
            header = header.strip_suffix(']').unwrap_or(header);
            let Some(path) = split_toml_path(header.trim()) else {
                continue;
            };
            if path.is_empty() {
                continue;
            }
            table = path;
            continue;
        }
        let Some(eq) = find_toml_eq(line) else {
            continue;
        };
        let Some(keys) = split_toml_path(line[..eq].trim()) else {
            continue;
        };
        if keys.is_empty() {
            continue;
        }
        let Some(value) = stringify_toml_value(strip_toml_comment(&line[eq + 1..]).trim()) else {
            continue;
        };
        let mut full = table.clone();
        full.extend(keys);
        map.insert(full.join("."), value);
    }
    map
}

/// Stringify one TOML value for the no-feature fallback: quoted strings
/// unquote, everything else (numbers, bools, datetimes, arrays, inline
/// tables) stays verbatim. Returns `None` for empty or unbalanced values.
#[cfg(not(feature = "frontmatter"))]
fn stringify_toml_value(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    match t.as_bytes()[0] {
        b'"' | b'\'' => unquote_toml_string(t),
        b'[' => {
            if !t.ends_with(']') {
                return None;
            }
            let inner = &t[1..t.len() - 1];
            if !inner.trim().is_empty() && split_top_level(inner, ',').is_none() {
                return None;
            }
            Some(t.to_string())
        }
        b'{' => {
            if !t.ends_with('}') {
                return None;
            }
            let inner = &t[1..t.len() - 1];
            if !inner.trim().is_empty() && split_top_level(inner, ',').is_none() {
                return None;
            }
            Some(t.to_string())
        }
        _ => Some(t.to_string()),
    }
}

/// Split Markdown into an optional [`Frontmatter`] and the body.
///
/// Infallible by design: an unparseable block still yields `Some` (with
/// best-effort `data`) because `original` preservation never depends on
/// parsing. Returns `(None, input)` when no valid block is present.
pub fn parse_with_frontmatter(input: &str) -> (Option<Frontmatter>, &str) {
    match split_raw(input) {
        None => (None, input),
        Some((fence, original, inner, body)) => {
            let data = parse_data(fence, inner);
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
