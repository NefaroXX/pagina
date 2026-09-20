//! Optional syntax-highlight hook for fenced code blocks.
//!
//! Adapter-hook pattern: the core renderers (`markdown_to_html`, `ast`,
//! `stream`) call back into a caller-supplied [`SyntaxHighlighter`] when one
//! is registered and otherwise emit the historical escaped output
//! byte-identically. The core therefore stays dependency-free; `syntect`
//! lives behind the `syntax-highlight` cargo feature (see
//! [`SyntectAdapter`]).
//!
//! # Example
//!
//! ```
//! use pagina::highlight::SyntaxHighlighter;
//!
//! struct Upper;
//! impl SyntaxHighlighter for Upper {
//!     fn write_highlighted(&self, out: &mut String, lang: &str, code: &str) {
//!         out.push_str(&format!("<!--{}-->{}", lang, code));
//!     }
//! }
//!
//! let html = pagina::markdown_to_html::convert_with_highlighter(
//!     "```rust\nlet x = 1;\n```\n",
//!     pagina::markdown_to_html::Options::default(),
//!     Some(&Upper),
//! )
//! .unwrap();
//! assert!(html.contains("<!--rust-->"));
//! ```

use crate::html_escape::escape_html;

/// Callback for highlighted code-block content.
///
/// `lang` is the fence language from [`language_from_info`] (`""` for
/// indented code or fences without info). `code` is the verbatim block text
/// with a trailing `\n` when non-empty (mirroring the default escaped path),
/// so adapters that fall back to escaped output stay byte-identical.
///
/// Implementations must push valid HTML for the code *content* only (the
/// surrounding `<pre><code …>` wrapper stays with the caller) and must
/// escape where they do not highlight. The trait is object-safe and
/// `Send + Sync` so highlighters cross threads and the WASM boundary.
pub trait SyntaxHighlighter: Send + Sync {
    /// Append highlighted HTML for `code` (fence language `lang`) to `out`.
    fn write_highlighted(&self, out: &mut String, lang: &str, code: &str);
}

/// Fence language for highlighting and `class="language-…"` output.
///
/// First whitespace-delimited word of `info` (backslash escapes and entities
/// resolved, mirroring the historical info-word rule), then cut at the first
/// `,` so rustdoc-style `"rust,ignore"` / `"rust,no_run"` map to `"rust"`.
/// Only `,` splits: `";"`-style info words keep their historical class
/// (spec example with `class="language-;"` stays byte-identical).
pub fn language_from_info(info: &str) -> String {
    crate::markdown_to_html::clean_info_word(info)
        .split(',')
        .next()
        .unwrap_or("")
        .to_string()
}

/// Join code lines with the historical trailing-newline rule.
pub fn join_code_text(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Escaped fallback shared by renderers when no highlighter is registered.
pub fn push_escaped_code(lines: &[String], out: &mut String) {
    for (k, line) in lines.iter().enumerate() {
        out.push_str(&escape_html(line));
        if k + 1 < lines.len() {
            out.push('\n');
        }
    }
    if !lines.is_empty() {
        out.push('\n');
    }
}

// ---------------------------------------------------------------------------
// syntect adapter (feature-gated)
// ---------------------------------------------------------------------------

/// `syntect`-backed [`SyntaxHighlighter`] emitting class-based spans.
///
/// Available only with the `syntax-highlight` cargo feature. Uses
/// `ClassedHTMLGenerator` with `ClassStyle::Spaced` (caller styles the
/// `source rust`, `keyword …` classes with their own CSS) and the pure-Rust
/// `fancy-regex` backend (`default-fancy`, no Onig C dependency, WASM-safe).
///
/// The adapter borrows a caller-supplied `SyntaxSet` and never embeds the
/// default dumps itself: binaries pay the ~2-4MB cost only when the caller
/// loads them (typically
/// `SyntaxSet::load_defaults_newlines()`). Load the `_newlines` variant —
/// `parse_html_for_line_which_includes_newline` requires newline-terminated
/// lines and highlighting degrades otherwise.
///
/// Unknown languages fall back to escaped output (byte-identical to the
/// default path); per-line errors also fall back line-by-line so one bad
/// line never drops the block.
#[cfg(feature = "syntax-highlight")]
pub struct SyntectAdapter<'a> {
    /// Caller-owned syntax definitions the adapter highlights against.
    syntax_set: &'a syntect::parsing::SyntaxSet,
}

#[cfg(feature = "syntax-highlight")]
impl<'a> SyntectAdapter<'a> {
    /// Borrow a caller-supplied syntax set (no dumps embedded here).
    pub fn new(syntax_set: &'a syntect::parsing::SyntaxSet) -> Self {
        SyntectAdapter { syntax_set }
    }
}

#[cfg(feature = "syntax-highlight")]
impl SyntaxHighlighter for SyntectAdapter<'_> {
    fn write_highlighted(&self, out: &mut String, lang: &str, code: &str) {
        if code.is_empty() {
            return;
        }
        let syntax = if lang.is_empty() {
            self.syntax_set.find_syntax_plain_text()
        } else {
            self.syntax_set
                .find_syntax_by_token(lang)
                .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text())
        };
        // Plain-text path: escaped output matches the default renderer.
        if syntax.name == "Plain Text" && self.syntax_set.find_syntax_by_token(lang).is_none() {
            out.push_str(&escape_html(code));
            return;
        }
        let mut gen = syntect::html::ClassedHTMLGenerator::new_with_class_style(
            syntax,
            self.syntax_set,
            syntect::html::ClassStyle::Spaced,
        );
        for line in syntect::util::LinesWithEndings::from(code) {
            if gen
                .parse_html_for_line_which_includes_newline(line)
                .is_err()
            {
                out.push_str(&escape_html(line));
            }
        }
        out.push_str(&gen.finalize());
        if !code.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_split_takes_comma_prefix() {
        assert_eq!(language_from_info("rust,ignore"), "rust");
        assert_eq!(language_from_info("rust,no_run foo"), "rust");
        assert_eq!(language_from_info("python"), "python");
        assert_eq!(language_from_info(""), "");
        // `;` does not split (spec `language-;` stays identical).
        assert_eq!(language_from_info(";"), ";");
    }
}
