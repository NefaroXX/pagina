//! Dollar-math scanning with Pandoc heuristics, shared by the legacy
//! inline pipeline and the public AST.
//!
//! GFM-gated extension (documented choice): `Options::gfm` enables math and
//! `convert()` stays pure CommonMark either way. Inline `$…$` needs a
//! non-space after the opener and a non-space before the closer, and the
//! closer must not be followed by a digit (so `$5` / `$5 and $10` currency
//! stays literal). Display `$$…$$` is the lenient block form: surrounding
//! whitespace/newlines are allowed and the trimmed content must be
//! non-empty, so `$$\nx^2\n$$` still parses. Content is verbatim (no
//! emphasis/link parsing inside; linkify skips math nodes).
//!
//! Output is span passthrough: inline renders
//! `<span class="math-inline">…</span>` (HTML-escaped content) and display
//! renders `<div class="math-display">…</div>`. The reverse converter maps
//! those shapes back to `$…$` / `$$…$$`.

use crate::html_escape::escape_html;

/// One math-aware text segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathSegment {
    /// Plain text (no math).
    Text(String),
    /// `$…$` content (verbatim, delimiters stripped).
    Inline(String),
    /// `$$…$$` content (verbatim, delimiters stripped).
    Display(String),
}

/// True for math-boundary whitespace (space, tab, newline, CR).
fn is_ws(c: char) -> bool {
    c == ' ' || c == '\t' || c == '\n' || c == '\r'
}

/// Split `s` at `$…$` / `$$…$$` spans. Unclosed delimiters stay literal.
pub fn split_math_text(s: &str) -> Vec<MathSegment> {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out: Vec<MathSegment> = Vec::new();
    let mut buf = String::new();
    let mut i = 0usize;
    while i < n {
        if chars[i] != '$' {
            buf.push(chars[i]);
            i += 1;
            continue;
        }
        // Display `$$` opener takes precedence over inline `$`.
        if i + 1 < n && chars[i + 1] == '$' {
            // `$$$` runs stay literal one `$` at a time (no triple-math).
            if i + 2 < n && chars[i + 2] == '$' {
                buf.push('$');
                i += 1;
                continue;
            }
            match find_display_close(&chars, i) {
                Some(j) => {
                    if !buf.is_empty() {
                        out.push(MathSegment::Text(std::mem::take(&mut buf)));
                    }
                    let content: String = chars[i + 2..j].iter().collect();
                    out.push(MathSegment::Display(content));
                    i = j + 2;
                }
                None => {
                    buf.push_str("$$");
                    i += 2;
                }
            }
            continue;
        }
        // Single `$`: opener needs a non-space after (and not end).
        if i + 1 >= n || is_ws(chars[i + 1]) {
            buf.push('$');
            i += 1;
            continue;
        }
        match find_inline_close(&chars, i) {
            Some(j) => {
                if !buf.is_empty() {
                    out.push(MathSegment::Text(std::mem::take(&mut buf)));
                }
                let content: String = chars[i + 1..j].iter().collect();
                out.push(MathSegment::Inline(content));
                i = j + 1;
            }
            None => {
                buf.push('$');
                i += 1;
            }
        }
    }
    if !buf.is_empty() {
        out.push(MathSegment::Text(buf));
    }
    if out.is_empty() {
        out.push(MathSegment::Text(String::new()));
    }
    out
}

/// Find the closing `$$` for the opener at `open` (points at first `$`).
/// Lenient block form: surrounding whitespace allowed, trimmed content must
/// be non-empty, closer must not be followed by a digit or `$`.
fn find_display_close(chars: &[char], open: usize) -> Option<usize> {
    let n = chars.len();
    let mut j = open + 2;
    while j + 1 < n {
        if chars[j] == '$' && chars[j + 1] == '$' {
            // Skip `$$$` overlaps.
            if j + 2 < n && chars[j + 2] == '$' {
                j += 1;
                continue;
            }
            if j > open + 2 {
                let k = j + 2;
                if k < n && (chars[k].is_ascii_digit() || chars[k] == '$') {
                    j += 2;
                    continue;
                }
                let content: String = chars[open + 2..j].iter().collect();
                if !content.trim().is_empty() {
                    return Some(j);
                }
            }
            j += 2;
            continue;
        }
        j += 1;
    }
    None
}

/// Find the closing single `$` for the opener at `open`. Closer needs a
/// non-space before, must not be followed by a digit, and must not touch
/// another `$` (so `$$` pairs never half-match as inline math).
fn find_inline_close(chars: &[char], open: usize) -> Option<usize> {
    let n = chars.len();
    let mut j = open + 1;
    while j < n {
        if chars[j] == '$' {
            let prev_dollar = j > 0 && chars[j - 1] == '$';
            let next_dollar = j + 1 < n && chars[j + 1] == '$';
            if prev_dollar || next_dollar {
                if next_dollar {
                    j += 2;
                } else {
                    j += 1;
                }
                continue;
            }
            if j > open + 1 && !is_ws(chars[j - 1]) {
                let k = j + 1;
                if k >= n || !chars[k].is_ascii_digit() {
                    return Some(j);
                }
            }
        }
        j += 1;
    }
    None
}

/// Inline math span HTML (content escaped, verbatim otherwise).
pub fn render_inline_math(content: &str) -> String {
    format!(
        "<span class=\"math-inline\">{}</span>",
        escape_html(content)
    )
}

/// Display math block HTML (content escaped, verbatim otherwise).
pub fn render_display_math(content: &str) -> String {
    format!("<div class=\"math-display\">{}</div>", escape_html(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn currency_stays_literal() {
        assert_eq!(
            split_math_text("$5"),
            vec![MathSegment::Text("$5".to_string())]
        );
        assert_eq!(
            split_math_text("$5 and $10"),
            vec![MathSegment::Text("$5 and $10".to_string())]
        );
    }

    #[test]
    fn inline_parses_with_heuristics() {
        assert_eq!(
            split_math_text("$a$"),
            vec![MathSegment::Inline("a".to_string())]
        );
        // Space after opener / before closer stays literal.
        assert!(split_math_text("$ a$")
            .iter()
            .all(|s| matches!(s, MathSegment::Text(_))));
        assert!(split_math_text("$a $")
            .iter()
            .all(|s| matches!(s, MathSegment::Text(_))));
        // Digit after closer stays literal.
        assert!(split_math_text("$a$5")
            .iter()
            .all(|s| matches!(s, MathSegment::Text(_))));
    }

    #[test]
    fn display_block_form_allows_newlines() {
        assert_eq!(
            split_math_text("$$x^2$$"),
            vec![MathSegment::Display("x^2".to_string())]
        );
        assert_eq!(
            split_math_text("$$\nx^2\n$$"),
            vec![MathSegment::Display("\nx^2\n".to_string())]
        );
    }
}
