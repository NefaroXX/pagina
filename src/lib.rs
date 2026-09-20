pub mod cli;
pub mod error;
pub mod html_escape;
pub mod html_to_markdown;
pub mod inline_parser;
pub mod markdown_to_html;

/// Conversion options shared by both directions.
///
/// Pure CommonMark is the default (`gfm: false`); set `gfm: true` for the
/// opt-in GFM extensions (task lists, strikethrough, bare autolinks).
/// Pipe tables are the one always-on GFM exception: they render in both
/// modes because the CommonMark spec has no pipe-table tests, so
/// compliance is unaffected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    /// Enable GFM extensions.
    pub gfm: bool,
}

impl Options {
    /// Options with GFM extensions enabled.
    pub fn gfm() -> Self {
        Options { gfm: true }
    }
}

/// Convenience re-exports for the public API.
pub use html_escape::{escape_html, unescape_html};
pub use html_to_markdown::convert as html_to_markdown;
pub use html_to_markdown::convert_gfm as html_to_markdown_gfm;
pub use html_to_markdown::convert_with as html_to_markdown_with;
pub use markdown_to_html::convert as markdown_to_html;
pub use markdown_to_html::convert_gfm as markdown_to_html_gfm;
pub use markdown_to_html::convert_with as markdown_to_html_with;
