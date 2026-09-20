//! Opt-in HTML sanitization for `pagina` (behind the `sanitize` cargo feature).
//!
//! The default Markdown → HTML path passes raw HTML through verbatim per
//! CommonMark, so its output must be treated as unsanitized input. This
//! module provides a minimal **denylist** post-processor with **zero
//! dependencies** (std only) for consumers that render the HTML in an
//! active context (browser, email client) and want a cheap first layer.
//!
//! What it does (all case-insensitive on tag/attribute names):
//!
//! - Strips `script`, `style`, `iframe`, `object`, `embed`, `form`, `base`,
//!   `link` and `meta` elements. Non-void elements are removed **with**
//!   their content up to the matching close tag (nesting-aware); void
//!   elements (`base`, `link`, `meta`, `embed`) drop the tag itself.
//! - Drops `on*` event-handler attributes and `style` attributes.
//! - Blocks `javascript:`, `vbscript:` and `data:text/html` URL schemes in
//!   `href`/`src`. Values are entity-decoded first (via
//!   [`crate::html_escape::unescape_html`]), then ASCII control/whitespace
//!   is stripped and the remainder lowercased before the prefix check, so
//!   `JaVaScRiPt:`, `javascript&#58;` and tab-injected schemes are caught.
//!   A blocked URL drops the attribute; the element itself is kept.
//! - Drops HTML comments (`<!-- … -->`), processing instructions and
//!   declarations (they carry no fragment content and are a classic
//!   conditional-comment hiding spot).
//!
//! Everything else passes through unchanged, so safe markup (`a[href]`,
//! `strong`, `em`, `code`, tables, lists, …) is preserved.
//!
//! This is a deliberately small denylist, not a full HTML policy engine:
//! it closes the XSS vectors listed above and nothing more. Consumers
//! with stricter needs should layer a dedicated sanitizer on top.

use crate::html_escape::{escape_href, unescape_html};

/// Elements removed by the sanitizer (lowercase, ASCII).
const STRIP_ELEMENTS: &[&str] = &[
    "script", "style", "iframe", "object", "embed", "form", "base", "link", "meta",
];

/// Stripped elements that are void in HTML5: only the tag itself is dropped.
const VOID_STRIP_ELEMENTS: &[&str] = &["embed", "base", "link", "meta"];

fn is_stripped(name: &str) -> bool {
    STRIP_ELEMENTS.contains(&name)
}

fn is_void_stripped(name: &str) -> bool {
    VOID_STRIP_ELEMENTS.contains(&name)
}

/// Sanitize an HTML fragment: see the module docs for the exact policy.
///
/// Safe markup passes through byte-identical (including attribute order
/// and quoting style, except that kept attribute values are re-escaped
/// to their canonical `&amp;`/`&quot;`/`&lt;`/`&gt;` form, which is
/// idempotent for renderer output).
pub fn sanitize_html(html: &str) -> String {
    let bytes = html.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0usize;

    while i < len {
        if bytes[i] != b'<' {
            let next = html[i..].find('<').map_or(len, |off| i + off);
            out.push_str(&html[i..next]);
            i = next;
            continue;
        }
        let rest = &html[i..];
        // HTML comment: drop it (unclosed comment drops the remainder,
        // matching how browsers treat the tail as comment text).
        if rest.starts_with("<!--") {
            match rest.find("-->") {
                Some(off) => i += off + 3,
                None => break,
            }
            continue;
        }
        // Processing instruction `<? … ?>`: drop it.
        if rest.starts_with("<?") {
            match rest.find("?>") {
                Some(off) => i += off + 2,
                None => break,
            }
            continue;
        }
        // Declaration / doctype / CDATA `<! … >`: drop the whole construct.
        if rest.starts_with("<!") {
            if rest.starts_with("<![CDATA[") {
                match rest.find("]]>") {
                    Some(off) => i += off + 3,
                    None => break,
                }
            } else {
                match find_decl_end(rest) {
                    Some(off) => i += off,
                    None => {
                        out.push_str(rest);
                        break;
                    }
                }
            }
            continue;
        }
        // Try a real tag; otherwise emit `<` literally.
        let Some(tag_end) = find_tag_end(rest) else {
            out.push('<');
            i += 1;
            continue;
        };
        let inner = &rest[1..tag_end];
        let Some((closing, name, attrs_str, self_closing)) = split_tag(inner) else {
            out.push('<');
            i += 1;
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if is_stripped(&lower) {
            if closing || is_void_stripped(&lower) || self_closing {
                // Closing tag, void element, or self-closed non-void tag:
                // drop the tag alone.
                i += tag_end + 1;
                continue;
            }
            // Non-void opening tag: drop through the matching close tag.
            i += skip_stripped_element(rest, &lower);
            continue;
        }
        if closing {
            out.push_str("</");
            out.push_str(&lower);
            out.push('>');
        } else {
            out.push('<');
            out.push_str(&lower);
            out.push_str(&sanitize_attrs(attrs_str));
            if self_closing {
                out.push_str(" />");
            } else {
                out.push('>');
            }
        }
        i += tag_end + 1;
    }

    out
}

/// Find the `>` that closes a tag opened at the start of `s` (which must
/// begin with `<`), respecting single- and double-quoted attribute values.
/// Returns the byte offset of `>` relative to `s`.
fn find_tag_end(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut k = 1usize;
    let mut quote: Option<u8> = None;
    while k < b.len() {
        let c = b[k];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == b'"' || c == b'\'' {
            quote = Some(c);
        } else if c == b'>' {
            return Some(k);
        }
        k += 1;
    }
    None
}

/// End offset (relative, one past `>`) of a `<! … >` declaration.
/// Declarations carry no quoted attributes in practice; a plain scan is
/// sufficient and bounded by the input length.
fn find_decl_end(s: &str) -> Option<usize> {
    s.find('>').map(|off| off + 1)
}

/// Split tag inner text (between `<`/`</` and `>`) into
/// (is_closing, name, attrs_text, self_closing). Returns `None` when the
/// text is not a well-formed tag (e.g. `< 3` or an empty name).
fn split_tag(inner: &str) -> Option<(bool, &str, &str, bool)> {
    let mut text = inner.trim();
    let closing = text.starts_with('/');
    if closing {
        text = text[1..].trim_start();
    }
    // Tag name: ASCII alphanumerics (custom elements may add `-`; accept
    // it so `<my-widget>` round-trips instead of degrading to text).
    let mut name_end = 0usize;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || c == '-' {
            name_end += c.len_utf8();
        } else {
            break;
        }
    }
    if name_end == 0 {
        return None;
    }
    let name = &text[..name_end];
    let mut attrs = &text[name_end..];
    // A `/` immediately before `>` marks a self-closing tag.
    let mut self_closing = false;
    let trimmed_end = attrs.trim_end();
    if trimmed_end.ends_with('/') {
        self_closing = true;
        attrs = trimmed_end
            .strip_suffix('/')
            .unwrap_or(trimmed_end)
            .trim_end();
    } else {
        attrs = attrs.trim();
        if attrs.is_empty() {
            attrs = "";
        }
    }
    // Closing tags must not carry attributes; treat `< /div >`-style
    // spacing leniently but reject `< /div foo>` as malformed.
    if closing {
        let body = attrs.trim();
        if !body.is_empty() && body != "/" {
            return None;
        }
        return Some((true, name, "", false));
    }
    Some((false, name, attrs, self_closing))
}

/// Skip a stripped non-void element starting at `rest` (which begins with
/// its opening `<tag …>`). Returns the byte offset to resume scanning at:
/// one past the matching close tag (nesting-aware, case-insensitive), or
/// just past the open tag when no close tag exists.
fn skip_stripped_element(rest: &str, name: &str) -> usize {
    let Some(open_end) = find_tag_end(rest) else {
        return rest.len();
    };
    let mut depth = 1usize;
    let mut k = open_end + 1;
    let bytes = rest.as_bytes();
    while k < bytes.len() {
        let tail = &rest[k..];
        let lt = match tail.find('<') {
            Some(off) => off,
            None => return rest.len(),
        };
        k += lt;
        let frag = &rest[k..];
        // Skip comments/PIs/declarations inside the stripped region.
        if frag.starts_with("<!--") {
            match frag.find("-->") {
                Some(off) => k += off + 3,
                None => return rest.len(),
            }
            continue;
        }
        if frag.starts_with("<?") {
            match frag.find("?>") {
                Some(off) => k += off + 2,
                None => return rest.len(),
            }
            continue;
        }
        if frag.starts_with("<!") {
            if frag.starts_with("<![CDATA[") {
                match frag.find("]]>") {
                    Some(off) => k += off + 3,
                    None => return rest.len(),
                }
            } else {
                match find_decl_end(frag) {
                    Some(off) => k += off,
                    None => return rest.len(),
                }
            }
            continue;
        }
        let Some(end) = find_tag_end(frag) else {
            return rest.len();
        };
        if let Some((closing, tname, _, _)) = split_tag(&frag[1..end]) {
            if tname.eq_ignore_ascii_case(name) {
                if closing {
                    depth -= 1;
                    if depth == 0 {
                        return k + end + 1;
                    }
                } else {
                    depth += 1;
                }
            }
        }
        k += end + 1;
    }
    rest.len()
}

/// Sanitize an attribute string, returning the rebuilt ` name="value"…`
/// suffix (leading space included per attribute, empty when none survive).
fn sanitize_attrs(attrs_str: &str) -> String {
    let mut out = String::with_capacity(attrs_str.len());
    for (name, value) in split_attrs(attrs_str) {
        let lower = name.to_ascii_lowercase();
        if lower == "style" || lower.starts_with("on") {
            continue;
        }
        if lower == "href" || lower == "src" {
            match value {
                Some(v) => {
                    if is_blocked_url(&v) {
                        continue;
                    }
                    out.push(' ');
                    out.push_str(&lower);
                    out.push_str("=\"");
                    out.push_str(&escape_href(&unescape_html(&v)));
                    out.push('"');
                }
                None => {
                    out.push(' ');
                    out.push_str(&lower);
                }
            }
            continue;
        }
        out.push(' ');
        out.push_str(&lower);
        if let Some(v) = value {
            out.push_str("=\"");
            out.push_str(&escape_href(&unescape_html(&v)));
            out.push('"');
        }
    }
    out
}

/// Split an attribute string into `(name, Option<value>)` pairs.
/// Values may be double-quoted, single-quoted, or unquoted; a missing
/// `=` yields a boolean attribute (`None`). Malformed runs are skipped.
fn split_attrs(s: &str) -> Vec<(&str, Option<String>)> {
    let b = s.as_bytes();
    let mut attrs: Vec<(&str, Option<String>)> = Vec::new();
    let mut k = 0usize;
    while k < b.len() {
        // Skip whitespace and stray `/` separators.
        while k < b.len() && (b[k].is_ascii_whitespace() || b[k] == b'/') {
            k += 1;
        }
        if k >= b.len() {
            break;
        }
        // Attribute name: up to whitespace, `=`, `/`, `>` or a quote.
        let start = k;
        while k < b.len()
            && !b[k].is_ascii_whitespace()
            && b[k] != b'='
            && b[k] != b'/'
            && b[k] != b'>'
            && b[k] != b'"'
            && b[k] != b'\''
        {
            k += 1;
        }
        if k == start {
            k += 1;
            continue;
        }
        let name = &s[start..k];
        // Skip whitespace between the name and `=`.
        while k < b.len() && b[k].is_ascii_whitespace() {
            k += 1;
        }
        if k >= b.len() || b[k] != b'=' {
            attrs.push((name, None));
            continue;
        }
        k += 1; // consume `=`
        while k < b.len() && b[k].is_ascii_whitespace() {
            k += 1;
        }
        if k >= b.len() {
            attrs.push((name, Some(String::new())));
            break;
        }
        if b[k] == b'"' || b[k] == b'\'' {
            let q = b[k];
            k += 1;
            let vstart = k;
            while k < b.len() && b[k] != q {
                k += 1;
            }
            attrs.push((name, Some(s[vstart..k].to_string())));
            if k < b.len() {
                k += 1; // consume closing quote
            }
        } else {
            let vstart = k;
            while k < b.len() && !b[k].is_ascii_whitespace() && b[k] != b'>' {
                k += 1;
            }
            // A trailing `/` belongs to a self-closing marker, not the value.
            let mut vend = k;
            if vend > vstart && b[vend - 1] == b'/' {
                vend -= 1;
            }
            attrs.push((name, Some(s[vstart..vend].to_string())));
        }
    }
    attrs
}

/// True when a URL value uses a blocked scheme.
///
/// The value is entity-decoded first, then every ASCII control/whitespace
/// byte is removed and the remainder lowercased before the prefix check,
/// so case, entity and tab/newline smuggling variants are all caught.
fn is_blocked_url(value: &str) -> bool {
    let decoded = unescape_html(value);
    let mut compact = String::with_capacity(decoded.len());
    for c in decoded.chars() {
        if c <= ' ' {
            // Drops NUL, C0 controls and ASCII space: leading padding as
            // well as injected tab/newline separators inside the scheme.
            continue;
        }
        compact.push(c.to_ascii_lowercase());
    }
    compact.starts_with("javascript:")
        || compact.starts_with("vbscript:")
        || compact.starts_with("data:text/html")
}
