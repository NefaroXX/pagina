pub mod cli;
pub mod error;
pub mod html_escape;
pub mod html_to_markdown;
pub mod inline_parser;
pub mod markdown_to_html;

/// Convenience re-exports for the public API.
pub use html_escape::{escape_html, unescape_html};
pub use html_to_markdown::convert as html_to_markdown;
pub use markdown_to_html::convert as markdown_to_html;
