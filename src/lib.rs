pub mod ast;
pub mod cli;
pub mod error;
pub mod frontmatter;
pub mod highlight;
pub mod html_escape;
pub mod html_to_markdown;
pub mod inline_parser;
pub mod markdown_to_html;
pub mod math;
#[cfg(feature = "sanitize")]
pub mod sanitize;
pub mod stream;
pub mod visitor;
#[cfg(feature = "wasm")]
pub mod wasm;

/// Conversion options shared by both directions.
///
/// Pure CommonMark is the default (`gfm: false`); set `gfm: true` for the
/// opt-in GFM extensions (task lists, strikethrough, bare autolinks,
/// footnotes, definition lists, dollar math).
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
pub use ast::{parse, parse_gfm, render_html, render_markdown};
pub use frontmatter::{
    markdown_to_html_with_frontmatter, markdown_to_html_with_frontmatter_gfm,
    markdown_to_html_with_frontmatter_with, parse_with_frontmatter, prepend_frontmatter,
    Frontmatter,
};
pub use html_escape::{escape_html, unescape_html};
pub use html_to_markdown::convert as html_to_markdown;
pub use html_to_markdown::convert_gfm as html_to_markdown_gfm;
pub use html_to_markdown::convert_with as html_to_markdown_with;
pub use markdown_to_html::convert as markdown_to_html;
pub use markdown_to_html::convert_gfm as markdown_to_html_gfm;
pub use markdown_to_html::convert_with as markdown_to_html_with;
pub use markdown_to_html::convert_with_highlighter as markdown_to_html_with_highlighter;
#[cfg(feature = "sanitize")]
pub use markdown_to_html::{
    convert_gfm_sanitized as markdown_to_html_gfm_sanitized,
    convert_sanitized as markdown_to_html_sanitized,
    convert_with_sanitized as markdown_to_html_with_sanitized,
};
#[cfg(feature = "sanitize")]
pub use sanitize::sanitize_html;
pub use stream::{
    collect_events, events_from_document, parse_stream, render_events_to_html,
    render_events_to_html_with_highlighter, Event, Parser, Tag, TagEnd,
};
