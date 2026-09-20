//! Pull streaming API: pulldown-style [`Event`] iterator over the AST.
//!
//! The stream walks the parsed [`Document`](crate::ast::Document) (see
//! [`Parser`]); it is **not** a separate tokenizer. Rationale: the crate's
//! block pipeline plus inline parser already own every syntax edge case, so
//! a second tokenizer would duplicate that logic and risk drift. Walking the
//! AST reuses the single source of truth, which keeps the legacy
//! `convert()` bytes untouched (this module only reads the AST) and lets
//! [`render_events_to_html`] stay byte-identical to
//! [`crate::ast::render_html`] by construction.
//!
//! [`TagEnd`] is split from [`Tag`] from day one and carries no payloads
//! (compact, `Copy`), so `End` events stay one byte wide and consumers can
//! match on closers without binding data they do not need. Opening data
//! (heading level, link destination, table alignments, ...) lives on
//! [`Tag`]; leaf content lives on dedicated [`Event`] variants. All payloads
//! are owned `String`s — no lifetimes on events — so streams cross threads
//! (`Send + Sync`) and cross the WASM boundary cleanly.
//!
//! Frontmatter is **skipped**: it is metadata, never renders into HTML
//! (matching [`crate::ast::render_html`]), so it produces no events. Stream
//! consumers that need metadata read `Document::frontmatter` directly.
//!
//! # Example
//!
//! ```
//! use pagina::stream::{parse_stream, render_events_to_html};
//! use pagina::Options;
//!
//! let events: Vec<_> = parse_stream("# Hi\n", Options::default()).collect();
//! let html = render_events_to_html(&events);
//! assert_eq!(html, "<h1>Hi</h1>\n");
//! ```

use crate::ast::{Alignment, Block, Document, Inline};
use crate::html_escape::{clean_url, escape_href, escape_html};
use crate::inline_parser::footnote_ref_html;
use crate::markdown_to_html::{append_alignment, task_checkbox, Alignment as HtmlAlignment};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// An opening tag carrying whatever payload its HTML needs (heading level,
/// link destination, table alignments, ...). Payload-free closers live on
/// [`TagEnd`].
#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    /// `<p>`.
    Paragraph,
    /// `<hN>`.
    Heading {
        /// Heading level (1-6).
        level: u8,
    },
    /// `<blockquote>`.
    BlockQuote,
    /// `<pre><code>`; `info` is the fenced info string (empty for indented).
    CodeBlock {
        /// Fenced info string as written (empty for indented code).
        info: String,
    },
    /// Verbatim HTML block; inner [`Event::Html`] lines carry the content.
    HtmlBlock,
    /// `<ul>` / `<ol>`. Bullet characters and ordered delimiters are
    /// Markdown-retention details with no HTML effect, so only the
    /// HTML-relevant shape is kept.
    List {
        /// Ordered (`<ol>`) vs bulleted (`<ul>`).
        ordered: bool,
        /// Start number for ordered lists.
        start: u32,
        /// Tight (paragraphs unwrap) vs loose (paragraphs keep `<p>`).
        tight: bool,
    },
    /// `<li>`. A leading [`Event::TaskListMarker`] carries GFM task state.
    Item,
    /// `<table>`; alignments drive per-cell `align` attributes.
    Table {
        /// Per-column alignment from the delimiter row.
        alignments: Vec<Alignment>,
    },
    /// `<thead><tr>`.
    TableHead,
    /// `<tr>` (body row; opens `<tbody>` on the first row).
    TableRow,
    /// `<th>` (in head) / `<td>` (in body).
    TableCell,
    /// Footnote definition. Skipped in flow by [`render_events_to_html`]
    /// and hoisted into the `<section class="footnotes">` footer, mirroring
    /// [`crate::ast::render_html`].
    FootnoteDefinition {
        /// Definition label as written (without `[^` / `]`).
        label: String,
        /// 1-based first-reference order (0 = unreferenced, dropped from HTML).
        number: usize,
    },
    /// `<dl>`; `tight` unwraps single-paragraph descriptions.
    DefinitionList {
        /// Tight (single-paragraph descriptions unwrap `<p>`) vs loose.
        tight: bool,
    },
    /// One definition-list entry (terms sharing descriptions).
    DefinitionListItem,
    /// `<dt>`.
    DefinitionTerm,
    /// `<dd>`.
    DefinitionDescription,
    /// `<em>`.
    Emphasis,
    /// `<strong>`.
    Strong,
    /// `<del>` (GFM `~~…~~`).
    Strikethrough,
    /// `<a>`; children are the link text events.
    Link {
        /// Destination (entity-decoded; percent-encoded at render time).
        url: String,
        /// Optional title.
        title: Option<String>,
    },
    /// `<img />`; `alt` rides in the payload and no child events are
    /// emitted (the closer is a pure delimiter). Autolinks are lowered to
    /// `Link` + `Text`, which renders byte-identically through the link path.
    Image {
        /// Flattened alt text.
        alt: String,
        /// Destination (entity-decoded; percent-encoded at render time).
        url: String,
        /// Optional title.
        title: Option<String>,
    },
}

/// A closing tag. Deliberately payload-free (compact, `Copy`): every closer
/// is one byte, and consumers match end events without binding data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagEnd {
    /// `</p>`.
    Paragraph,
    /// `</hN>`.
    Heading,
    /// `</blockquote>`.
    BlockQuote,
    /// `</code></pre>`.
    CodeBlock,
    /// End of an HTML block (emits nothing itself).
    HtmlBlock,
    /// `</ul>` / `</ol>`.
    List,
    /// `</li>`.
    Item,
    /// `</table>` (plus `</tbody>` when rows were emitted).
    Table,
    /// `</tr></thead>`.
    TableHead,
    /// `</tr>`.
    TableRow,
    /// `</th>` / `</td>`.
    TableCell,
    /// End of a footnote definition (emits nothing in flow).
    FootnoteDefinition,
    /// `</dl>`.
    DefinitionList,
    /// End of a definition-list entry (emits nothing itself).
    DefinitionListItem,
    /// `</dt>`.
    DefinitionTerm,
    /// `</dd>`.
    DefinitionDescription,
    /// `</em>`.
    Emphasis,
    /// `</strong>`.
    Strong,
    /// `</del>`.
    Strikethrough,
    /// `</a>`.
    Link,
    /// End of an image (emits nothing; the opener wrote the whole tag).
    Image,
}

/// A pulldown-style stream event. All payloads are owned (`String`, no
/// lifetimes) so events are `Send + Sync` and WASM-clean.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// An opening tag (payloads live here).
    Start(Tag),
    /// A closing tag (never carries data; see [`TagEnd`]).
    End(TagEnd),
    /// Literal text (entities decoded, escapes resolved).
    Text(String),
    /// Code span content (backticks stripped).
    Code(String),
    /// GFM-gated dollar math, inline `$…$` (verbatim content).
    MathInline(String),
    /// GFM-gated dollar math, display `$$…$$` (verbatim content).
    MathDisplay(String),
    /// One verbatim HTML block line (inside `HtmlBlock`).
    Html(String),
    /// Inline raw HTML, passed through verbatim.
    InlineHtml(String),
    /// GFM footnote reference (`[^label]`).
    FootnoteReference {
        /// Reference label as written (without `[^` / `]`).
        label: String,
        /// 1-based first-reference order (matches the footer entry).
        number: usize,
    },
    /// Plain line ending inside a paragraph.
    SoftBreak,
    /// Two-space / backslash line ending.
    HardBreak,
    /// Thematic break (`<hr />`).
    Rule,
    /// GFM task-list checkbox; always the first event inside an `Item`.
    TaskListMarker(bool),
}

// ---------------------------------------------------------------------------
// Parser: owned-Document pull iterator
// ---------------------------------------------------------------------------

/// Pull iterator of [`Event`]s over an owned [`Document`].
///
/// Eager-walk design (the simple sound choice): construction walks the
/// borrowed document once into an owned `Vec<Event>`, so iteration needs no
/// lifetimes, no self-referential borrow of the document, and no runtime
/// borrow machinery — the parser itself owns everything it yields and is
/// therefore `Send + Sync`.
#[derive(Debug, Clone)]
pub struct Parser {
    /// Pre-walked events; iteration is a plain drain.
    events: std::vec::IntoIter<Event>,
}

impl Parser {
    /// Walk an owned document into a pull stream.
    pub fn new(doc: Document) -> Self {
        Parser {
            events: document_events(&doc).into_iter(),
        }
    }

    /// Remaining event count.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// True when no events remain.
    pub fn is_empty(&self) -> bool {
        self.events.len() == 0
    }
}

impl Iterator for Parser {
    type Item = Event;

    fn next(&mut self) -> Option<Event> {
        self.events.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.events.size_hint()
    }
}

impl ExactSizeIterator for Parser {}

/// Parse Markdown into a pull [`Parser`] with explicit [`crate::Options`].
pub fn parse_stream(input: &str, options: crate::Options) -> Parser {
    Parser::new(crate::ast::parse(input, options))
}

/// Collect the event stream for `input` into a `Vec`.
pub fn collect_events(input: &str, options: crate::Options) -> Vec<Event> {
    parse_stream(input, options).collect()
}

/// Walk a borrowed document into owned events (no lifetimes escape: every
/// payload is cloned into the output).
pub fn events_from_document(doc: &Document) -> Vec<Event> {
    document_events(doc)
}

fn document_events(doc: &Document) -> Vec<Event> {
    let mut events = Vec::new();
    push_blocks(&doc.blocks, &mut events);
    events
}

fn push_blocks(blocks: &[Block], events: &mut Vec<Event>) {
    for block in blocks {
        push_block(block, events);
    }
}

fn push_block(block: &Block, events: &mut Vec<Event>) {
    match block {
        Block::Paragraph(inlines) => {
            events.push(Event::Start(Tag::Paragraph));
            push_inlines(inlines, events);
            events.push(Event::End(TagEnd::Paragraph));
        }
        Block::Heading(heading) => {
            events.push(Event::Start(Tag::Heading {
                level: heading.level,
            }));
            push_inlines(&heading.content, events);
            events.push(Event::End(TagEnd::Heading));
        }
        Block::ThematicBreak(_) => events.push(Event::Rule),
        Block::CodeBlock(code) => {
            events.push(Event::Start(Tag::CodeBlock {
                info: code.info.clone(),
            }));
            if !code.lines.is_empty() {
                let mut text = code.lines.join("\n");
                text.push('\n');
                events.push(Event::Text(text));
            }
            events.push(Event::End(TagEnd::CodeBlock));
        }
        Block::HtmlBlock(lines) => {
            events.push(Event::Start(Tag::HtmlBlock));
            for line in lines {
                events.push(Event::Html(line.clone()));
            }
            events.push(Event::End(TagEnd::HtmlBlock));
        }
        Block::BlockQuote(children) => {
            events.push(Event::Start(Tag::BlockQuote));
            push_blocks(children, events);
            events.push(Event::End(TagEnd::BlockQuote));
        }
        Block::List(list) => {
            events.push(Event::Start(Tag::List {
                ordered: list.ordered,
                start: list.start,
                tight: list.tight,
            }));
            for item in &list.items {
                events.push(Event::Start(Tag::Item));
                if let Some(checked) = item.task {
                    events.push(Event::TaskListMarker(checked));
                }
                push_blocks(&item.blocks, events);
                events.push(Event::End(TagEnd::Item));
            }
            events.push(Event::End(TagEnd::List));
        }
        Block::Table(table) => {
            events.push(Event::Start(Tag::Table {
                alignments: table.alignments.clone(),
            }));
            events.push(Event::Start(Tag::TableHead));
            for cell in &table.header {
                events.push(Event::Start(Tag::TableCell));
                push_inlines(&cell.content, events);
                events.push(Event::End(TagEnd::TableCell));
            }
            events.push(Event::End(TagEnd::TableHead));
            for row in &table.rows {
                events.push(Event::Start(Tag::TableRow));
                for cell in row {
                    events.push(Event::Start(Tag::TableCell));
                    push_inlines(&cell.content, events);
                    events.push(Event::End(TagEnd::TableCell));
                }
                events.push(Event::End(TagEnd::TableRow));
            }
            events.push(Event::End(TagEnd::Table));
        }
        Block::FootnoteDefinition(def) => {
            events.push(Event::Start(Tag::FootnoteDefinition {
                label: def.label.clone(),
                number: def.number,
            }));
            push_blocks(&def.blocks, events);
            events.push(Event::End(TagEnd::FootnoteDefinition));
        }
        Block::DefinitionList(list) => {
            events.push(Event::Start(Tag::DefinitionList { tight: list.tight }));
            for item in &list.items {
                events.push(Event::Start(Tag::DefinitionListItem));
                for term in &item.terms {
                    events.push(Event::Start(Tag::DefinitionTerm));
                    push_inlines(&term.content, events);
                    events.push(Event::End(TagEnd::DefinitionTerm));
                }
                for desc in &item.descriptions {
                    events.push(Event::Start(Tag::DefinitionDescription));
                    push_blocks(&desc.blocks, events);
                    events.push(Event::End(TagEnd::DefinitionDescription));
                }
                events.push(Event::End(TagEnd::DefinitionListItem));
            }
            events.push(Event::End(TagEnd::DefinitionList));
        }
    }
}

fn push_inlines(inlines: &[Inline], events: &mut Vec<Event>) {
    for inline in inlines {
        push_inline(inline, events);
    }
}

fn push_inline(inline: &Inline, events: &mut Vec<Event>) {
    match inline {
        Inline::Text(s) => events.push(Event::Text(s.clone())),
        Inline::Emphasis { content, .. } => {
            events.push(Event::Start(Tag::Emphasis));
            push_inlines(content, events);
            events.push(Event::End(TagEnd::Emphasis));
        }
        Inline::Strong { content, .. } => {
            events.push(Event::Start(Tag::Strong));
            push_inlines(content, events);
            events.push(Event::End(TagEnd::Strong));
        }
        Inline::Strikethrough(content) => {
            events.push(Event::Start(Tag::Strikethrough));
            push_inlines(content, events);
            events.push(Event::End(TagEnd::Strikethrough));
        }
        Inline::Code(s) => events.push(Event::Code(s.clone())),
        Inline::MathInline(s) => events.push(Event::MathInline(s.clone())),
        Inline::MathDisplay(s) => events.push(Event::MathDisplay(s.clone())),
        Inline::Link(link) => {
            events.push(Event::Start(Tag::Link {
                url: link.url.clone(),
                title: link.title.clone(),
            }));
            push_inlines(&link.text, events);
            events.push(Event::End(TagEnd::Link));
        }
        Inline::Image(img) => {
            // Alt rides in the opener; no children (see `Tag::Image`).
            events.push(Event::Start(Tag::Image {
                alt: img.alt.clone(),
                url: img.url.clone(),
                title: img.title.clone(),
            }));
            events.push(Event::End(TagEnd::Image));
        }
        Inline::Autolink(a) => {
            // Lowered to Link + Text: renders byte-identically via the link path.
            events.push(Event::Start(Tag::Link {
                url: a.url.clone(),
                title: None,
            }));
            events.push(Event::Text(a.text.clone()));
            events.push(Event::End(TagEnd::Link));
        }
        Inline::FootnoteReference(r) => events.push(Event::FootnoteReference {
            label: r.label.clone(),
            number: r.number,
        }),
        Inline::RawHtml(s) => events.push(Event::InlineHtml(s.clone())),
        Inline::HardBreak => events.push(Event::HardBreak),
        Inline::SoftBreak => events.push(Event::SoftBreak),
    }
}

// ---------------------------------------------------------------------------
// HTML rendering from the event stream
// ---------------------------------------------------------------------------

/// Open-list stack frame (only the HTML-relevant shape is needed: tight
/// decides whether item paragraphs unwrap; ordered/start already rendered
/// into the opening tag).
struct ListFrame {
    tight: bool,
}

/// Open-table frame: alignments plus the per-row column cursor.
struct TableFrame {
    alignments: Vec<Alignment>,
    col: usize,
    in_head: bool,
    had_rows: bool,
}

/// A footnote definition hoisted out of flow: label/number plus its inner
/// event range (indices into the source slice).
struct HoistedDef {
    label: String,
    number: usize,
    inner: (usize, usize),
}

/// Renderer state threaded through the recursive event walk.
struct RenderCtx<'a> {
    lists: Vec<ListFrame>,
    tables: Vec<TableFrame>,
    dl_tight: Vec<bool>,
    tight_item_depth: usize,
    seen: HashMap<String, usize>,
    totals: HashMap<String, usize>,
    defs: Vec<HoistedDef>,
    hl: Option<&'a dyn crate::highlight::SyntaxHighlighter>,
}

/// Render an event slice to HTML.
///
/// Byte-identical to [`crate::ast::render_html`] on streams produced by
/// [`Parser`] (verified in `tests/stream.rs`): list tight/loose branches,
/// table alignment attributes, code info classes, and footnote hoisting all
/// mirror the AST renderer. Footnote definitions never render in flow; they
/// are collected during the walk and emitted as the
/// `<section class="footnotes" data-footnotes>` footer.
pub fn render_events_to_html(events: &[Event]) -> String {
    render_events_to_html_with_highlighter(events, None)
}

/// Render an event slice to HTML with an optional highlighter (see
/// [`crate::ast::render_html_with_highlighter`]). With `None` the output is
/// byte-identical to [`render_events_to_html`].
pub fn render_events_to_html_with_highlighter(
    events: &[Event],
    highlighter: Option<&dyn crate::highlight::SyntaxHighlighter>,
) -> String {
    let mut totals: HashMap<String, usize> = HashMap::new();
    for event in events {
        if let Event::FootnoteReference { label, .. } = event {
            *totals.entry(label.clone()).or_insert(0) += 1;
        }
    }
    let mut ctx = RenderCtx {
        lists: Vec::new(),
        tables: Vec::new(),
        dl_tight: Vec::new(),
        tight_item_depth: 0,
        seen: HashMap::new(),
        totals,
        defs: Vec::new(),
        hl: highlighter,
    };
    let mut out = String::new();
    let mut pos = 0;
    render_blocks(events, &mut pos, &mut ctx, &mut out);
    render_footer(events, &mut ctx, &mut out);
    out
}

/// Render blocks until the slice ends or a dangling `End` closes an outer
/// range (the closer is consumed).
fn render_blocks(events: &[Event], pos: &mut usize, ctx: &mut RenderCtx, out: &mut String) {
    while *pos < events.len() {
        match &events[*pos] {
            Event::Start(_) => render_block_at(events, pos, ctx, out),
            Event::End(_) => {
                *pos += 1;
                return;
            }
            // Stray inline-level events at block level cannot occur from
            // `Parser`; render them harmlessly instead of panicking.
            _ => render_inline_leaf(events, pos, ctx, out),
        }
    }
}

/// Render the single block opened at `events[*pos]`; leaves `pos` just past
/// its matching `End`.
fn render_block_at(events: &[Event], pos: &mut usize, ctx: &mut RenderCtx, out: &mut String) {
    let Some(Event::Start(tag)) = events.get(*pos) else {
        return;
    };
    let tag = tag.clone();
    match tag {
        Tag::Paragraph => {
            *pos += 1;
            if ctx.tight_item_depth > 0 {
                render_inlines_until(events, pos, ctx, out, TagEnd::Paragraph);
            } else {
                out.push_str("<p>");
                render_inlines_until(events, pos, ctx, out, TagEnd::Paragraph);
                out.push_str("</p>\n");
            }
        }
        Tag::Heading { level } => {
            *pos += 1;
            out.push_str(&format!("<h{level}>"));
            render_inlines_until(events, pos, ctx, out, TagEnd::Heading);
            out.push_str(&format!("</h{level}>\n"));
        }
        Tag::BlockQuote => {
            *pos += 1;
            out.push_str("<blockquote>\n");
            render_blocks(events, pos, ctx, out);
            out.push_str("</blockquote>\n");
        }
        Tag::CodeBlock { info } => {
            *pos += 1;
            out.push_str("<pre><code");
            let lang = crate::highlight::language_from_info(&info);
            if !lang.is_empty() {
                out.push_str(&format!(" class=\"language-{}\"", escape_href(&lang)));
            }
            out.push('>');
            if let Some(Event::Text(text)) = events.get(*pos) {
                match ctx.hl {
                    Some(h) => h.write_highlighted(out, &lang, text),
                    None => out.push_str(&escape_html(text)),
                }
                *pos += 1;
            } else if ctx.hl.is_some() {
                // Empty code block: highlighter sees empty content (no-op).
                if let Some(h) = ctx.hl {
                    h.write_highlighted(out, &lang, "");
                }
            }
            // Consume the closer.
            *pos += 1;
            out.push_str("</code></pre>\n");
        }
        Tag::HtmlBlock => {
            *pos += 1;
            while let Some(event) = events.get(*pos) {
                match event {
                    Event::End(TagEnd::HtmlBlock) => {
                        *pos += 1;
                        break;
                    }
                    Event::Html(line) => {
                        let line = line.clone();
                        out.push_str(&line);
                        out.push('\n');
                        *pos += 1;
                    }
                    _ => {
                        *pos += 1;
                    }
                }
            }
        }
        Tag::List {
            ordered,
            start,
            tight,
        } => {
            *pos += 1;
            if ordered {
                if start != 1 {
                    out.push_str(&format!("<ol start=\"{start}\">\n"));
                } else {
                    out.push_str("<ol>\n");
                }
            } else {
                out.push_str("<ul>\n");
            }
            ctx.lists.push(ListFrame { tight });
            render_blocks(events, pos, ctx, out);
            ctx.lists.pop();
            out.push_str(if ordered { "</ol>\n" } else { "</ul>\n" });
        }
        Tag::Item => render_item(events, pos, ctx, out),
        Tag::Table { alignments } => {
            *pos += 1;
            out.push_str("<table>\n");
            ctx.tables.push(TableFrame {
                alignments,
                col: 0,
                in_head: false,
                had_rows: false,
            });
            render_blocks(events, pos, ctx, out);
            let frame = ctx.tables.pop();
            if frame.map(|f| f.had_rows).unwrap_or(false) {
                out.push_str("</tbody>\n");
            }
            out.push_str("</table>\n");
        }
        Tag::TableHead => {
            *pos += 1;
            out.push_str("<thead>\n<tr>\n");
            if let Some(frame) = ctx.tables.last_mut() {
                frame.in_head = true;
                frame.col = 0;
            }
            render_blocks(events, pos, ctx, out);
            if let Some(frame) = ctx.tables.last_mut() {
                frame.in_head = false;
            }
            out.push_str("</tr>\n</thead>\n");
        }
        Tag::TableRow => {
            *pos += 1;
            let first = ctx.tables.last().map(|f| !f.had_rows).unwrap_or(true);
            if first {
                out.push_str("<tbody>\n");
            }
            if let Some(frame) = ctx.tables.last_mut() {
                frame.had_rows = true;
                frame.in_head = false;
                frame.col = 0;
            }
            out.push_str("<tr>\n");
            render_blocks(events, pos, ctx, out);
            out.push_str("</tr>\n");
        }
        Tag::TableCell => {
            *pos += 1;
            let (in_head, alignment) = match ctx.tables.last() {
                Some(frame) => (
                    frame.in_head,
                    frame
                        .alignments
                        .get(frame.col)
                        .copied()
                        .unwrap_or(Alignment::None),
                ),
                None => (false, Alignment::None),
            };
            if in_head {
                out.push_str("<th");
            } else {
                out.push_str("<td");
            }
            append_alignment(out, convert_alignment(alignment));
            out.push('>');
            render_inlines_until(events, pos, ctx, out, TagEnd::TableCell);
            if let Some(frame) = ctx.tables.last_mut() {
                frame.col += 1;
            }
            out.push_str(if in_head { "</th>\n" } else { "</td>\n" });
        }
        Tag::FootnoteDefinition { label, number } => {
            let end = find_end(events, *pos);
            ctx.defs.push(HoistedDef {
                label,
                number,
                inner: (*pos + 1, end),
            });
            *pos = end + 1;
        }
        Tag::DefinitionList { tight } => {
            *pos += 1;
            out.push_str("<dl>\n");
            ctx.dl_tight.push(tight);
            render_blocks(events, pos, ctx, out);
            ctx.dl_tight.pop();
            out.push_str("</dl>\n");
        }
        Tag::DefinitionListItem => {
            *pos += 1;
            render_blocks(events, pos, ctx, out);
        }
        Tag::DefinitionTerm => {
            *pos += 1;
            out.push_str("<dt>");
            render_inlines_until(events, pos, ctx, out, TagEnd::DefinitionTerm);
            out.push_str("</dt>\n");
        }
        Tag::DefinitionDescription => render_description(events, pos, ctx, out),
        // Inline-level openers at block level cannot occur from `Parser`;
        // route them through the inline path instead of dropping content.
        Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } | Tag::Image { .. } => {
            render_inline_at(events, pos, ctx, out);
        }
    }
}

/// Index of the `End` matching the `Start` at `start` (nesting-aware).
fn find_end(events: &[Event], start: usize) -> usize {
    let mut depth = 0usize;
    for (i, event) in events.iter().enumerate().skip(start) {
        match event {
            Event::Start(_) => depth += 1,
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    events.len()
}

/// Render one list item, mirroring `ast::render_list_html` branch for
/// branch (tight unwrapping, loose empty-item collapsing, checkbox
/// placement). `pos` opens at `Start(Item)`; leaves it past `End(Item)`.
fn render_item(events: &[Event], pos: &mut usize, ctx: &mut RenderCtx, out: &mut String) {
    let end = find_end(events, *pos);
    let mut cur = *pos + 1;
    let mut task: Option<bool> = None;
    if let Some(Event::TaskListMarker(checked)) = events.get(cur) {
        task = Some(*checked);
        cur += 1;
    }
    // Split the item body into top-level (block_start, block_end) pairs.
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    let mut m = cur;
    while m < end {
        match events.get(m) {
            Some(Event::Start(_)) => {
                let f = find_end(events, m);
                blocks.push((m, f));
                m = f + 1;
            }
            _ => m += 1,
        }
    }
    let tight = ctx.lists.last().map(|l| l.tight).unwrap_or(false);
    let checkbox = task.map(task_checkbox).unwrap_or("");
    if tight {
        out.push_str("<li>");
        ctx.tight_item_depth += 1;
        let mut first_inline = true;
        for (k, (lo, hi)) in blocks.iter().enumerate() {
            if matches!(events.get(*lo), Some(Event::Start(Tag::Paragraph))) {
                if k > 0 && !out.ends_with('\n') {
                    out.push('\n');
                }
                if first_inline {
                    out.push_str(checkbox);
                    first_inline = false;
                }
                render_inline_range(events, lo + 1, *hi, ctx, out);
            } else {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                render_block_range(events, *lo, *hi, ctx, out);
            }
        }
        ctx.tight_item_depth -= 1;
        if blocks.is_empty() {
            out.push_str(checkbox.trim_end());
        }
        out.push_str("</li>\n");
    } else if blocks.is_empty() || is_single_empty_paragraph(events, &blocks) {
        if task.is_some() {
            out.push_str("<li>\n");
            out.push_str(&format!("<p>{}</p>\n", checkbox.trim_end()));
            out.push_str("</li>\n");
        } else {
            out.push_str("<li></li>\n");
        }
    } else {
        out.push_str("<li>\n");
        let mut rendered_first = false;
        for (lo, hi) in &blocks {
            let is_first_para =
                !rendered_first && matches!(events.get(*lo), Some(Event::Start(Tag::Paragraph)));
            if is_first_para {
                rendered_first = true;
                out.push_str("<p>");
                out.push_str(checkbox);
                render_inline_range(events, lo + 1, *hi, ctx, out);
                out.push_str("</p>\n");
            } else {
                render_block_range(events, *lo, *hi, ctx, out);
            }
        }
        out.push_str("</li>\n");
    }
    *pos = end + 1;
}

/// True for the loose empty-item shape: a single paragraph with no inline
/// events between its tags (mirrors the AST `v.is_empty()` check).
fn is_single_empty_paragraph(events: &[Event], blocks: &[(usize, usize)]) -> bool {
    match blocks {
        [(lo, hi)] => {
            matches!(events.get(*lo), Some(Event::Start(Tag::Paragraph)))
                && hi.saturating_sub(*lo) <= 1
        }
        _ => false,
    }
}

/// Render the block pair `(lo, hi)` (`lo` opens at `Start`, `hi` is the
/// matching `End`) via a sub-cursor.
fn render_block_range(
    events: &[Event],
    lo: usize,
    hi: usize,
    ctx: &mut RenderCtx,
    out: &mut String,
) {
    let mut q = lo;
    render_block_at(events, &mut q, ctx, out);
    debug_assert_eq!(q, hi + 1);
}

/// Render inline events in `events[lo..hi]` (no closers inside).
fn render_inline_range(
    events: &[Event],
    lo: usize,
    hi: usize,
    ctx: &mut RenderCtx,
    out: &mut String,
) {
    let mut q = lo;
    while q < hi {
        match &events[q] {
            Event::Start(_) => render_inline_at(events, &mut q, ctx, out),
            Event::End(_) => q += 1,
            _ => render_inline_leaf(events, &mut q, ctx, out),
        }
    }
}

/// Render inline content until the matching `stop` closer (consumed).
fn render_inlines_until(
    events: &[Event],
    pos: &mut usize,
    ctx: &mut RenderCtx,
    out: &mut String,
    stop: TagEnd,
) {
    while *pos < events.len() {
        match &events[*pos] {
            Event::Start(_) => render_inline_at(events, pos, ctx, out),
            Event::End(t) => {
                *pos += 1;
                if *t == stop {
                    return;
                }
                // A foreign closer cannot occur from `Parser`; stop anyway
                // to keep the cursor in sync rather than over-consuming.
                return;
            }
            _ => render_inline_leaf(events, pos, ctx, out),
        }
    }
}

/// Render the inline opener at `pos` (leaves `pos` past its closer).
fn render_inline_at(events: &[Event], pos: &mut usize, ctx: &mut RenderCtx, out: &mut String) {
    let Some(Event::Start(tag)) = events.get(*pos) else {
        return;
    };
    let tag = tag.clone();
    match tag {
        Tag::Emphasis => {
            *pos += 1;
            out.push_str("<em>");
            render_inlines_until(events, pos, ctx, out, TagEnd::Emphasis);
            out.push_str("</em>");
        }
        Tag::Strong => {
            *pos += 1;
            out.push_str("<strong>");
            render_inlines_until(events, pos, ctx, out, TagEnd::Strong);
            out.push_str("</strong>");
        }
        Tag::Strikethrough => {
            *pos += 1;
            out.push_str("<del>");
            render_inlines_until(events, pos, ctx, out, TagEnd::Strikethrough);
            out.push_str("</del>");
        }
        Tag::Link { url, title } => {
            *pos += 1;
            push_link_open(&url, &title, out);
            render_inlines_until(events, pos, ctx, out, TagEnd::Link);
            out.push_str("</a>");
        }
        Tag::Image { alt, url, title } => {
            push_image(&alt, &url, &title, out);
            *pos += 1;
            // Consume through the image closer (no children were emitted).
            while *pos < events.len() {
                *pos += 1;
                if matches!(events.get(*pos - 1), Some(Event::End(TagEnd::Image))) {
                    break;
                }
            }
        }
        // Block-level openers cannot occur inside inline content from
        // `Parser`; skip the opener and keep the cursor moving.
        _ => {
            *pos += 1;
        }
    }
}

/// Render one inline leaf event; advances `pos` by exactly one.
fn render_inline_leaf(events: &[Event], pos: &mut usize, ctx: &mut RenderCtx, out: &mut String) {
    match events.get(*pos) {
        Some(Event::Text(s)) => {
            let s = s.clone();
            out.push_str(&escape_html(&s));
        }
        Some(Event::Code(s)) => {
            let s = s.clone();
            out.push_str("<code>");
            out.push_str(&escape_html(&s));
            out.push_str("</code>");
        }
        Some(Event::MathInline(s)) => {
            let s = s.clone();
            out.push_str(&crate::math::render_inline_math(&s));
        }
        Some(Event::MathDisplay(s)) => {
            let s = s.clone();
            out.push_str(&crate::math::render_display_math(&s));
        }
        Some(Event::InlineHtml(s)) => {
            let s = s.clone();
            out.push_str(&s);
        }
        Some(Event::Html(s)) => {
            let s = s.clone();
            out.push_str(&s);
            out.push('\n');
        }
        Some(Event::FootnoteReference { label, number }) => {
            let label = label.clone();
            let number = *number;
            let occurrence = ctx.seen.entry(label.clone()).or_insert(0);
            *occurrence += 1;
            out.push_str(&footnote_ref_html(&label, number, *occurrence));
        }
        Some(Event::SoftBreak) => out.push('\n'),
        Some(Event::HardBreak) => out.push_str("<br />\n"),
        Some(Event::Rule) => out.push_str("<hr />\n"),
        Some(Event::TaskListMarker(_)) => {}
        Some(Event::Start(_)) | Some(Event::End(_)) | None => {}
    }
    *pos += 1;
}

/// Render a definition description, mirroring `ast::render_deflist_html`:
/// tight single-paragraph descriptions unwrap `<p>`, empty ones collapse.
fn render_description(events: &[Event], pos: &mut usize, ctx: &mut RenderCtx, out: &mut String) {
    let end = find_end(events, *pos);
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    let mut m = *pos + 1;
    while m < end {
        match events.get(m) {
            Some(Event::Start(_)) => {
                let f = find_end(events, m);
                blocks.push((m, f));
                m = f + 1;
            }
            _ => m += 1,
        }
    }
    let tight = ctx.dl_tight.last().copied().unwrap_or(false);
    let single_para = matches!(blocks.as_slice(), [(lo, _)]
        if matches!(events.get(*lo), Some(Event::Start(Tag::Paragraph))));
    if tight && single_para {
        let (lo, hi) = blocks[0];
        out.push_str("<dd>");
        render_inline_range(events, lo + 1, hi, ctx, out);
        out.push_str("</dd>\n");
    } else if blocks.is_empty() {
        out.push_str("<dd></dd>\n");
    } else {
        out.push_str("<dd>\n");
        for (lo, hi) in &blocks {
            render_block_range(events, *lo, *hi, ctx, out);
        }
        out.push_str("</dd>\n");
    }
    *pos = end + 1;
}

/// Render the hoisted footnote footer from the definitions collected during
/// the walk, mirroring `ast::render_footnote_footer` (reference-order
/// filtering, backref anchors, trailing-paragraph splicing).
fn render_footer(events: &[Event], ctx: &mut RenderCtx, out: &mut String) {
    let kept: Vec<(String, usize, (usize, usize))> = ctx
        .defs
        .iter()
        .filter(|def| ctx.totals.get(&def.label).copied().unwrap_or(0) > 0)
        .map(|def| (def.label.clone(), def.number, def.inner))
        .collect();
    if kept.is_empty() {
        return;
    }
    out.push_str("<section class=\"footnotes\" data-footnotes>\n<ol>\n");
    for (label, number, (lo, hi)) in &kept {
        out.push_str(&format!("<li id=\"fn-{number}\">\n"));
        let total = ctx.totals.get(label).copied().unwrap_or(1).max(1);
        let mut backs = String::new();
        for k in 1..=total {
            let href = if k == 1 {
                format!("#fnref-{number}")
            } else {
                format!("#fnref-{number}-{k}")
            };
            let text = if k == 1 {
                "↩".to_string()
            } else {
                format!("↩<sup>{k}</sup>")
            };
            backs.push_str(&format!(
                " <a href=\"{href}\" class=\"footnote-backref\" data-footnote-backref aria-label=\"Back to reference {number}\">{text}</a>"
            ));
        }
        let mut rendered = String::new();
        let mut q = *lo;
        while q < *hi {
            match events.get(q) {
                Some(Event::Start(_)) => render_block_at(events, &mut q, ctx, &mut rendered),
                Some(Event::End(_)) => q += 1,
                Some(_) => render_inline_leaf(events, &mut q, ctx, &mut rendered),
                None => break,
            }
        }
        // A trailing top-level paragraph absorbs the backrefs (same check
        // as the AST renderer: last block is a paragraph ending `</p>\n`).
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        let mut m = *lo;
        while m < *hi {
            match events.get(m) {
                Some(Event::Start(_)) => {
                    let f = find_end(events, m);
                    pairs.push((m, f));
                    m = f + 1;
                }
                _ => m += 1,
            }
        }
        let ends_para = matches!(pairs.last(), Some((lo, _))
            if matches!(events.get(*lo), Some(Event::Start(Tag::Paragraph))))
            && rendered.ends_with("</p>\n");
        if ends_para {
            rendered.truncate(rendered.len() - "</p>\n".len());
            rendered.push_str(&backs);
            rendered.push_str("</p>\n");
        } else {
            if !rendered.is_empty() && !rendered.ends_with('\n') {
                rendered.push('\n');
            }
            rendered.push_str(&format!("<p>{}</p>\n", backs.trim_start()));
        }
        out.push_str(&rendered);
        out.push_str("</li>\n");
    }
    out.push_str("</ol>\n</section>\n");
}

fn push_link_open(url: &str, title: &Option<String>, out: &mut String) {
    match title {
        Some(t) if !t.is_empty() => {
            out.push_str(&format!(
                "<a href=\"{}\" title=\"{}\">",
                escape_href(&clean_url(url)),
                escape_href(t)
            ));
        }
        _ => {
            out.push_str(&format!("<a href=\"{}\">", escape_href(&clean_url(url))));
        }
    }
}

fn push_image(alt: &str, url: &str, title: &Option<String>, out: &mut String) {
    match title {
        Some(t) if !t.is_empty() => {
            out.push_str(&format!(
                "<img src=\"{}\" alt=\"{}\" title=\"{}\" />",
                escape_href(&clean_url(url)),
                escape_html(alt),
                escape_href(t)
            ));
        }
        _ => {
            out.push_str(&format!(
                "<img src=\"{}\" alt=\"{}\" />",
                escape_href(&clean_url(url)),
                escape_html(alt)
            ));
        }
    }
}

fn convert_alignment(a: Alignment) -> HtmlAlignment {
    match a {
        Alignment::None => HtmlAlignment::None,
        Alignment::Left => HtmlAlignment::Left,
        Alignment::Center => HtmlAlignment::Center,
        Alignment::Right => HtmlAlignment::Right,
    }
}
