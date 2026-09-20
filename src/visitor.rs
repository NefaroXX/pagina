//! Generic walker over the public AST ([`crate::ast`]).
//!
//! The walker is decoupled from rendering: implement [`Visitor`] to collect
//! information (table of contents, link inventories, lint findings) without
//! producing output. Traversal is pre-order over references — the visitor
//! borrows the document, never owns it.
//!
//! # Example
//!
//! ```
//! use pagina::ast::{Block, parse};
//! use pagina::visitor::{Visitor, walk_document};
//!
//! #[derive(Default)]
//! struct Headings {
//!     entries: Vec<(u8, String)>,
//! }
//!
//! impl Visitor for Headings {
//!     fn visit_block(&mut self, block: &Block) {
//!         if let Block::Heading(h) = block {
//!             self.entries
//!                 .push((h.level, pagina::ast::plain_text(&h.content)));
//!         }
//!     }
//! }
//!
//! let doc = parse("# A\n\n## B\n", pagina::Options::default());
//! let mut toc = Headings::default();
//! walk_document(&doc, &mut toc);
//! assert_eq!(toc.entries.len(), 2);
//! ```

use crate::ast::{Block, Document, Inline};

/// Visitor callbacks invoked once per node, pre-order (parent before
/// children). Both methods default to no-ops so visitors override only
/// what they need.
pub trait Visitor {
    /// Called for every [`Block`] in the document, parent before children.
    fn visit_block(&mut self, _block: &Block) {}

    /// Called for every [`Inline`] in the document, parent before children.
    fn visit_inline(&mut self, _inline: &Inline) {}
}

/// Walk a whole [`Document`]: every top-level block via [`walk_block`]
/// (footnote definitions included, so their content is visible to visitors).
/// (Frontmatter and reference definitions carry no inline content and need
/// no traversal; read them off [`Document`] directly.)
pub fn walk_document<V: Visitor>(doc: &Document, visitor: &mut V) {
    for block in &doc.blocks {
        walk_block(block, visitor);
    }
}

/// Walk one [`Block`]: calls [`Visitor::visit_block`], then recurses into
/// child blocks and inlines.
pub fn walk_block<V: Visitor>(block: &Block, visitor: &mut V) {
    visitor.visit_block(block);
    match block {
        Block::Paragraph(inlines) => walk_inlines(inlines, visitor),
        Block::Heading(heading) => walk_inlines(&heading.content, visitor),
        Block::ThematicBreak(_) | Block::CodeBlock(_) | Block::HtmlBlock(_) => {}
        Block::BlockQuote(children) => {
            for child in children {
                walk_block(child, visitor);
            }
        }
        Block::List(list) => {
            for item in &list.items {
                for child in &item.blocks {
                    walk_block(child, visitor);
                }
            }
        }
        Block::Table(table) => {
            for cell in &table.header {
                walk_inlines(&cell.content, visitor);
            }
            for row in &table.rows {
                for cell in row {
                    walk_inlines(&cell.content, visitor);
                }
            }
        }
        Block::FootnoteDefinition(def) => {
            for child in &def.blocks {
                walk_block(child, visitor);
            }
        }
        Block::DefinitionList(list) => {
            for item in &list.items {
                for term in &item.terms {
                    walk_inlines(&term.content, visitor);
                }
                for desc in &item.descriptions {
                    for child in &desc.blocks {
                        walk_block(child, visitor);
                    }
                }
            }
        }
    }
}

/// Walk one [`Inline`]: calls [`Visitor::visit_inline`], then recurses
/// into emphasis/link children.
pub fn walk_inline<V: Visitor>(inline: &Inline, visitor: &mut V) {
    visitor.visit_inline(inline);
    match inline {
        Inline::Emphasis { content, .. }
        | Inline::Strong { content, .. }
        | Inline::Strikethrough(content) => walk_inlines(content, visitor),
        Inline::Link(link) => walk_inlines(&link.text, visitor),
        Inline::Text(_)
        | Inline::Code(_)
        | Inline::Image(_)
        | Inline::Autolink(_)
        | Inline::FootnoteReference(_)
        | Inline::RawHtml(_)
        | Inline::HardBreak
        | Inline::SoftBreak => {}
    }
}

/// Walk a slice of [`Inline`] nodes via [`walk_inline`].
pub fn walk_inlines<V: Visitor>(inlines: &[Inline], visitor: &mut V) {
    for inline in inlines {
        walk_inline(inline, visitor);
    }
}
