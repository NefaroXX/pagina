//! Public AST plus generic parse/render entry points.
//!
//! [`Document`] is an owned tree (`Vec`/`Box`, no arenas, no lifetimes on
//! nodes — all `String`s, hence `Send + Sync`) covering the syntax this
//! crate parses today: paragraphs, ATX + setext headings, blockquotes,
//! tight/loose lists with task markers, indented + fenced code, thematic
//! breaks, GFM pipe tables with alignments, verbatim HTML blocks, a
//! frontmatter attachment point, and the collected link reference
//! definitions.
//!
//! Lossless-leaning, not lossless: delimiter characters (`*` vs `_`),
//! link reference styles (inline / reference / collapsed / shortcut),
//! heading marker kinds (ATX closing hashes, setext `=` vs `-`), list
//! markers, fence characters, and autolink angle-vs-bare forms are retained
//! where cheap. Code-span backtick runs and emphasis edge whitespace are
//! normalized on Markdown re-emission.
//!
//! Route note (alongside, not rewire): [`parse`] builds this AST through
//! the same internal block pipeline as
//! [`crate::markdown_to_html::convert`], but `convert()` itself keeps its
//! legacy render path untouched, and [`crate::html_to_markdown::convert`]
//! keeps its token-direct path. New behavior therefore cannot shift legacy
//! bytes; `render_html`/`render_markdown` are additive entry points.
//! `render_html(&parse(md))` is verified byte-identical to `convert(md)`
//! across the test corpus in `tests/ast.rs`.

use crate::frontmatter::Frontmatter;
use crate::html_escape::{clean_url, escape_href, escape_html};
use crate::inline_parser::RefDefs;
use crate::markdown_to_html::{
    append_alignment, clean_info_word, parse_document_blocks, strip_task_prefix, task_checkbox,
};

// ---------------------------------------------------------------------------
// Document
// ---------------------------------------------------------------------------

/// A parsed Markdown document: block tree plus metadata.
///
/// `references` maps normalized labels to `(destination, title)` and
/// applies document-wide (definitions may follow use). `frontmatter` holds
/// a leading `---`/`+++` block when present (see
/// [`crate::frontmatter::parse_with_frontmatter`]); it never renders into
/// HTML and is re-emitted verbatim by [`render_markdown`].
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Top-level blocks in document order.
    pub blocks: Vec<Block>,
    /// Collected link reference definitions (normalized label ->
    /// (destination, optional title)).
    pub references: RefDefs,
    /// Leading metadata fence, when the source had one.
    pub frontmatter: Option<Frontmatter>,
}

// ---------------------------------------------------------------------------
// Blocks
// ---------------------------------------------------------------------------

/// A block-level node. Owned tree: nesting goes through `Vec` (and `Box`
/// for the large table variant), so no lifetimes appear anywhere.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// One or more source lines forming a paragraph.
    Paragraph(Vec<Inline>),
    /// ATX (`#`) or setext (`===` / `---`) heading.
    Heading(Heading),
    /// Thematic break (`***`, `---`, `___` and spaced variants).
    ThematicBreak(ThematicBreak),
    /// Indented or fenced code block (verbatim lines).
    CodeBlock(CodeBlock),
    /// Raw HTML block lines, passed through verbatim.
    HtmlBlock(Vec<String>),
    /// `>` quote with nested blocks.
    BlockQuote(Vec<Block>),
    /// Ordered or bulleted list.
    List(List),
    /// GFM pipe table (large variant: boxed).
    Table(Box<Table>),
}

/// A heading: level 1-6, inline content, and how it was written.
#[derive(Debug, Clone, PartialEq)]
pub struct Heading {
    /// Heading level (1-6).
    pub level: u8,
    /// Inline content.
    pub content: Vec<Inline>,
    /// ATX vs setext marker retention.
    pub kind: HeadingKind,
}

/// How a heading was written in source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadingKind {
    /// `#` heading; `closing` notes a stripped `###` run.
    Atx {
        /// Whether a closing hash run was stripped.
        closing: bool,
    },
    /// Setext underline; `marker` is `'='` (level 1) or `'-'` (level 2).
    Setext {
        /// Underline character (`'='` or `'-'`).
        marker: char,
    },
}

/// A thematic break; `marker` is the repeated character (`-`, `_`, `*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThematicBreak {
    /// The repeated marker character.
    pub marker: char,
}

/// A code block: verbatim lines plus kind/info retention.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeBlock {
    /// Indented vs fenced (with fence character/length).
    pub kind: CodeBlockKind,
    /// Fenced info string as written (empty for indented code).
    pub info: String,
    /// Verbatim content lines (opening indent/fence excluded).
    pub lines: Vec<String>,
}

/// Indented code vs fenced code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeBlockKind {
    /// Four-space (or tab-stop) indented code.
    Indented,
    /// Fenced code with the opening fence retained.
    Fenced {
        /// Fence character (`` ` `` or `~`).
        fence_char: char,
        /// Opening fence run length (3+).
        fence_len: usize,
    },
}

/// A list: marker retention plus tight/loose and per-item task state.
#[derive(Debug, Clone, PartialEq)]
pub struct List {
    /// Ordered (`1.`) vs bulleted (`-`).
    pub ordered: bool,
    /// Bullet character for unordered lists (`-`, `+`, `*`).
    pub bullet: char,
    /// Delimiter for ordered lists (`.` or `)`).
    pub delimiter: char,
    /// Start number for ordered lists.
    pub start: u32,
    /// Tight (paragraphs unwrap) vs loose (paragraphs keep `<p>`).
    pub tight: bool,
    /// Items in order.
    pub items: Vec<ListItem>,
}

/// One list item. `task` is `Some(checked)` only when parsed with GFM
/// options and the item's first paragraph carries a `[ ]`/`[x]` prefix
/// (the prefix itself is stripped from `blocks`, mirroring HTML output).
#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    /// GFM task state (`Some(true)` = `[x]`/`[X]`, `Some(false)` = `[ ]`).
    pub task: Option<bool>,
    /// Item content blocks.
    pub blocks: Vec<Block>,
}

/// A GFM pipe table.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// Header cells.
    pub header: Vec<TableCell>,
    /// Per-column alignment from the delimiter row.
    pub alignments: Vec<Alignment>,
    /// Body rows (padded/truncated to the header width by the parser).
    pub rows: Vec<Vec<TableCell>>,
}

/// One table cell: inline content.
#[derive(Debug, Clone, PartialEq)]
pub struct TableCell {
    /// Inline content of the cell.
    pub content: Vec<Inline>,
}

/// Column alignment from the delimiter row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    /// No alignment markers (`---`).
    None,
    /// `:--`.
    Left,
    /// `:-:`.
    Center,
    /// `--:`.
    Right,
}

// ---------------------------------------------------------------------------
// Inlines
// ---------------------------------------------------------------------------

/// An inline-level node. Large variants (links, images) are boxed.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    /// Literal text (entities decoded, escapes resolved).
    Text(String),
    /// `*…*` or `_…_`; `delimiter` retains which character formed it.
    Emphasis {
        /// Delimiter character (`'*'` or `'_'`).
        delimiter: char,
        /// Inner inlines.
        content: Vec<Inline>,
    },
    /// `**…**` or `__…__`; `delimiter` retains `*` vs `_`.
    Strong {
        /// Delimiter character (`'*'` for `**`, `'_'` for `__`).
        delimiter: char,
        /// Inner inlines.
        content: Vec<Inline>,
    },
    /// GFM `~~…~~` (only parsed with GFM options).
    Strikethrough(Vec<Inline>),
    /// Code span content (backticks stripped, line endings spaced).
    Code(String),
    /// A link (large variant: boxed).
    Link(Box<Link>),
    /// An image (large variant: boxed; `alt` is flattened text).
    Image(Box<Image>),
    /// `<http://…>` / `<a@b.c>` autolinks and GFM bare URLs/emails.
    Autolink(Autolink),
    /// Inline raw HTML, passed through verbatim.
    RawHtml(String),
    /// Two-space / backslash line ending.
    HardBreak,
    /// Plain line ending inside a paragraph.
    SoftBreak,
}

/// A link with its written style retained.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    /// Link text inlines.
    pub text: Vec<Inline>,
    /// Destination (entity-decoded; percent-encoded at HTML render time).
    pub url: String,
    /// Optional title.
    pub title: Option<String>,
    /// How the link was written.
    pub style: LinkStyle,
}

/// How a link/image was written in source.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkStyle {
    /// `[text](url "title")`.
    Inline,
    /// `[text][label]` (raw label retained verbatim).
    Reference(String),
    /// `[text][]`.
    Collapsed,
    /// `[text]`.
    Shortcut,
}

/// An image with its written style retained.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    /// Flattened alt text.
    pub alt: String,
    /// Destination (entity-decoded; percent-encoded at HTML render time).
    pub url: String,
    /// Optional title.
    pub title: Option<String>,
    /// How the image was written.
    pub style: LinkStyle,
}

/// An autolink: `<uri>` / `<email>` (`bare: false`) or a GFM bare
/// URL/email (`bare: true`). `url` is the href form (`mailto:`-prefixed
/// for emails); `text` is the visible form.
#[derive(Debug, Clone, PartialEq)]
pub struct Autolink {
    /// Href form of the target.
    pub url: String,
    /// Visible text.
    pub text: String,
    /// Whether this was a bare URL/email (no angle brackets).
    pub bare: bool,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse Markdown into a [`Document`] with explicit [`crate::Options`].
///
/// `gfm: false` is pure CommonMark (task brackets stay literal, `~~`
/// stays literal, bare URLs stay plain); `gfm: true` additionally parses
/// task-list items, `~~` strikethrough, and bare autolinks. Pipe tables
/// parse in both modes (spec-neutral always-on exception, as in
/// [`crate::markdown_to_html::convert`]).
pub fn parse(input: &str, options: crate::Options) -> Document {
    let (frontmatter, body) = crate::frontmatter::parse_with_frontmatter(input);
    let (blocks, refs) = parse_document_blocks(body);
    let gfm = options.gfm;
    let blocks = blocks
        .iter()
        .map(|b| convert_block(b, &refs, gfm))
        .collect();
    Document {
        blocks,
        references: refs,
        frontmatter,
    }
}

/// Parse Markdown into a [`Document`] with GFM extensions enabled (task
/// lists, strikethrough, bare autolinks; tables parse in both modes).
pub fn parse_gfm(input: &str) -> Document {
    parse(input, crate::Options::gfm())
}

/// Flatten inlines to plain text (links/images/autolinks to their visible
/// text, code to its content, breaks to `\n`). Used for TOC extraction,
/// lint messages, and autolink display text.
pub fn plain_text(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(t) => s.push_str(t),
            Inline::Emphasis { content, .. }
            | Inline::Strong { content, .. }
            | Inline::Strikethrough(content) => s.push_str(&plain_text(content)),
            Inline::Code(c) => s.push_str(c),
            Inline::Link(link) => s.push_str(&plain_text(&link.text)),
            Inline::Image(img) => s.push_str(&img.alt),
            Inline::Autolink(a) => s.push_str(&a.text),
            Inline::RawHtml(h) => s.push_str(h),
            Inline::HardBreak | Inline::SoftBreak => s.push('\n'),
        }
    }
    s
}

fn convert_alignment(a: crate::markdown_to_html::Alignment) -> Alignment {
    match a {
        crate::markdown_to_html::Alignment::None => Alignment::None,
        crate::markdown_to_html::Alignment::Left => Alignment::Left,
        crate::markdown_to_html::Alignment::Center => Alignment::Center,
        crate::markdown_to_html::Alignment::Right => Alignment::Right,
    }
}

fn parse_inlines(s: &str, refs: &RefDefs, gfm: bool) -> Vec<Inline> {
    crate::inline_parser::parse_inline_ast(s, refs, gfm)
}

fn convert_block(block: &crate::markdown_to_html::Block, refs: &RefDefs, gfm: bool) -> Block {
    use crate::markdown_to_html::Block as IB;
    match block {
        IB::Paragraph(lines) => Block::Paragraph(parse_inlines(&lines.join("\n"), refs, gfm)),
        IB::Heading {
            level,
            content,
            kind,
        } => {
            let kind = match kind {
                crate::markdown_to_html::HeadingKind::Atx { closing } => {
                    HeadingKind::Atx { closing: *closing }
                }
                crate::markdown_to_html::HeadingKind::Setext(m) => {
                    HeadingKind::Setext { marker: *m }
                }
            };
            Block::Heading(Heading {
                level: *level,
                content: parse_inlines(content, refs, gfm),
                kind,
            })
        }
        IB::ThematicBreak(m) => Block::ThematicBreak(ThematicBreak { marker: *m }),
        IB::IndentedCode(lines) => Block::CodeBlock(CodeBlock {
            kind: CodeBlockKind::Indented,
            info: String::new(),
            lines: lines.clone(),
        }),
        IB::FencedCode {
            info,
            lines,
            fence_char,
            fence_len,
        } => Block::CodeBlock(CodeBlock {
            kind: CodeBlockKind::Fenced {
                fence_char: *fence_char,
                fence_len: *fence_len,
            },
            info: info.clone(),
            lines: lines.clone(),
        }),
        IB::HtmlBlock(lines) => Block::HtmlBlock(lines.clone()),
        IB::BlockQuote(children) => Block::BlockQuote(
            children
                .iter()
                .map(|b| convert_block(b, refs, gfm))
                .collect(),
        ),
        IB::List {
            ordered,
            bullet,
            delimiter,
            start,
            tight,
            items,
        } => Block::List(List {
            ordered: *ordered,
            bullet: *bullet,
            delimiter: *delimiter,
            start: *start,
            tight: *tight,
            items: items
                .iter()
                .map(|item| convert_list_item(item, refs, gfm))
                .collect(),
        }),
        IB::Table {
            header,
            alignments,
            rows,
        } => Block::Table(Box::new(Table {
            header: header
                .iter()
                .map(|c| TableCell {
                    content: parse_inlines(c, refs, gfm),
                })
                .collect(),
            alignments: alignments.iter().map(|a| convert_alignment(*a)).collect(),
            rows: rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|c| TableCell {
                            content: parse_inlines(c, refs, gfm),
                        })
                        .collect()
                })
                .collect(),
        })),
    }
}

/// Convert one internal list item, splitting a GFM task prefix exactly as
/// the HTML renderer does (same helper, same joined string, same empty-rest
/// rule), so `render_html` stays byte-identical to `convert`.
fn convert_list_item(
    item: &[crate::markdown_to_html::Block],
    refs: &RefDefs,
    gfm: bool,
) -> ListItem {
    use crate::markdown_to_html::Block as IB;
    let mut task: Option<bool> = None;
    let mut blocks: Vec<Block> = item.iter().map(|b| convert_block(b, refs, gfm)).collect();
    if gfm {
        if let Some(IB::Paragraph(lines)) = item.first() {
            if let Some((checked, rest)) = strip_task_prefix(&lines.join("\n")) {
                task = Some(checked);
                let rest_inlines = parse_inlines(&rest, refs, gfm);
                blocks[0] = Block::Paragraph(rest_inlines);
            }
        }
    }
    ListItem { task, blocks }
}

// ---------------------------------------------------------------------------
// HTML rendering from the AST
// ---------------------------------------------------------------------------

/// Render a [`Document`] to HTML.
///
/// Inline content was resolved at [`parse`] time (reference map complete,
/// GFM gating applied), so rendering needs no options. Output matches
/// [`crate::markdown_to_html::convert`]/`convert_gfm` byte-for-byte for
/// documents parsed with the corresponding options (verified in
/// `tests/ast.rs`); frontmatter never renders.
pub fn render_html(doc: &Document) -> String {
    let mut out = String::new();
    for block in &doc.blocks {
        render_block_html(block, &mut out);
    }
    out
}

fn render_inlines_html(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(s) => out.push_str(&escape_html(s)),
            Inline::Emphasis { content, .. } => {
                out.push_str("<em>");
                out.push_str(&render_inlines_html(content));
                out.push_str("</em>");
            }
            Inline::Strong { content, .. } => {
                out.push_str("<strong>");
                out.push_str(&render_inlines_html(content));
                out.push_str("</strong>");
            }
            Inline::Strikethrough(content) => {
                out.push_str("<del>");
                out.push_str(&render_inlines_html(content));
                out.push_str("</del>");
            }
            Inline::Code(s) => {
                out.push_str("<code>");
                out.push_str(&escape_html(s));
                out.push_str("</code>");
            }
            Inline::Link(link) => render_link_html(&link.text, &link.url, &link.title, &mut out),
            Inline::Image(img) => render_image_html(img, &mut out),
            Inline::Autolink(a) => {
                render_link_html(&[Inline::Text(a.text.clone())], &a.url, &None, &mut out)
            }
            Inline::RawHtml(s) => out.push_str(s),
            Inline::HardBreak => out.push_str("<br />\n"),
            Inline::SoftBreak => out.push('\n'),
        }
    }
    out
}

fn render_link_html(text: &[Inline], url: &str, title: &Option<String>, out: &mut String) {
    let inner = render_inlines_html(text);
    match title {
        Some(t) if !t.is_empty() => {
            out.push_str(&format!(
                "<a href=\"{}\" title=\"{}\">{}</a>",
                escape_href(&clean_url(url)),
                escape_href(t),
                inner
            ));
        }
        _ => {
            out.push_str(&format!(
                "<a href=\"{}\">{}</a>",
                escape_href(&clean_url(url)),
                inner
            ));
        }
    }
}

fn render_image_html(img: &Image, out: &mut String) {
    match &img.title {
        Some(t) if !t.is_empty() => {
            out.push_str(&format!(
                "<img src=\"{}\" alt=\"{}\" title=\"{}\" />",
                escape_href(&clean_url(&img.url)),
                escape_html(&img.alt),
                escape_href(t)
            ));
        }
        _ => {
            out.push_str(&format!(
                "<img src=\"{}\" alt=\"{}\" />",
                escape_href(&clean_url(&img.url)),
                escape_html(&img.alt)
            ));
        }
    }
}

fn render_block_html(block: &Block, out: &mut String) {
    match block {
        Block::Paragraph(inlines) => {
            out.push_str("<p>");
            out.push_str(&render_inlines_html(inlines));
            out.push_str("</p>\n");
        }
        Block::Heading(heading) => {
            out.push_str(&format!("<h{}>", heading.level));
            out.push_str(&render_inlines_html(&heading.content));
            out.push_str(&format!("</h{}>\n", heading.level));
        }
        Block::ThematicBreak(_) => {
            out.push_str("<hr />\n");
        }
        Block::CodeBlock(code) => render_code_html(code, out),
        Block::HtmlBlock(lines) => {
            for line in lines {
                out.push_str(line);
                out.push('\n');
            }
        }
        Block::BlockQuote(children) => {
            out.push_str("<blockquote>\n");
            for child in children {
                render_block_html(child, out);
            }
            out.push_str("</blockquote>\n");
        }
        Block::List(list) => render_list_html(list, out),
        Block::Table(table) => render_table_html(table, out),
    }
}

fn render_code_html(code: &CodeBlock, out: &mut String) {
    match code.kind {
        CodeBlockKind::Indented => {
            out.push_str("<pre><code>");
            push_code_lines(&code.lines, out);
            out.push_str("</code></pre>\n");
        }
        CodeBlockKind::Fenced { .. } => {
            out.push_str("<pre><code");
            let lang = clean_info_word(&code.info);
            if !lang.is_empty() {
                out.push_str(&format!(" class=\"language-{}\"", escape_href(&lang)));
            }
            out.push('>');
            push_code_lines(&code.lines, out);
            out.push_str("</code></pre>\n");
        }
    }
}

fn push_code_lines(lines: &[String], out: &mut String) {
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

/// List rendering, mirroring the legacy renderer line-for-line except that
/// the task split happened at [`parse`] time (`item.task` + pre-stripped
/// first paragraph) instead of at render time.
fn render_list_html(list: &List, out: &mut String) {
    if list.ordered {
        if list.start != 1 {
            out.push_str(&format!("<ol start=\"{}\">\n", list.start));
        } else {
            out.push_str("<ol>\n");
        }
    } else {
        out.push_str("<ul>\n");
    }
    for item in &list.items {
        let checkbox = item.task.map(task_checkbox).unwrap_or("");
        if list.tight {
            out.push_str("<li>");
            let mut first_inline = true;
            for (k, block) in item.blocks.iter().enumerate() {
                match block {
                    Block::Paragraph(inlines) => {
                        let inline = render_inlines_html(inlines);
                        if k > 0 && !out.ends_with('\n') {
                            out.push('\n');
                        }
                        if first_inline {
                            out.push_str(checkbox);
                            first_inline = false;
                        }
                        out.push_str(&inline);
                    }
                    _ => {
                        if !out.ends_with('\n') {
                            out.push('\n');
                        }
                        render_block_html(block, out);
                    }
                }
            }
            if item.blocks.is_empty() {
                out.push_str(checkbox.trim_end());
            }
            out.push_str("</li>\n");
        } else if item.blocks.is_empty()
            || (item.blocks.len() == 1
                && matches!(item.blocks.first(), Some(Block::Paragraph(v)) if v.is_empty()))
        {
            if item.task.is_some() {
                out.push_str("<li>\n");
                out.push_str(&format!("<p>{}</p>\n", checkbox.trim_end()));
                out.push_str("</li>\n");
            } else {
                out.push_str("<li></li>\n");
            }
        } else {
            out.push_str("<li>\n");
            let mut rendered_first = false;
            for block in item.blocks.iter() {
                match block {
                    Block::Paragraph(inlines) if !rendered_first => {
                        rendered_first = true;
                        out.push_str("<p>");
                        out.push_str(checkbox);
                        out.push_str(&render_inlines_html(inlines));
                        out.push_str("</p>\n");
                    }
                    _ => render_block_html(block, out),
                }
            }
            out.push_str("</li>\n");
        }
    }
    if list.ordered {
        out.push_str("</ol>\n");
    } else {
        out.push_str("</ul>\n");
    }
}

fn render_table_html(table: &Table, out: &mut String) {
    out.push_str("<table>\n<thead>\n<tr>\n");
    for (k, cell) in table.header.iter().enumerate() {
        out.push_str("<th");
        append_alignment(
            out,
            convert_alignment_back(table.alignments.get(k).copied().unwrap_or(Alignment::None)),
        );
        out.push('>');
        out.push_str(&render_inlines_html(&cell.content));
        out.push_str("</th>\n");
    }
    out.push_str("</tr>\n</thead>\n");
    if !table.rows.is_empty() {
        out.push_str("<tbody>\n");
        for row in &table.rows {
            out.push_str("<tr>\n");
            for (k, cell) in row.iter().enumerate() {
                out.push_str("<td");
                append_alignment(
                    out,
                    convert_alignment_back(
                        table.alignments.get(k).copied().unwrap_or(Alignment::None),
                    ),
                );
                out.push('>');
                out.push_str(&render_inlines_html(&cell.content));
                out.push_str("</td>\n");
            }
            out.push_str("</tr>\n");
        }
        out.push_str("</tbody>\n");
    }
    out.push_str("</table>\n");
}

fn convert_alignment_back(a: Alignment) -> crate::markdown_to_html::Alignment {
    match a {
        Alignment::None => crate::markdown_to_html::Alignment::None,
        Alignment::Left => crate::markdown_to_html::Alignment::Left,
        Alignment::Center => crate::markdown_to_html::Alignment::Center,
        Alignment::Right => crate::markdown_to_html::Alignment::Right,
    }
}

// ---------------------------------------------------------------------------
// Markdown rendering from the AST
// ---------------------------------------------------------------------------

/// Render a [`Document`] back to Markdown.
///
/// Normalizing (not verbatim): headings become ATX, thematic breaks collapse
/// to a 3-char run, emphasis uses retained delimiters, links use retained
/// styles, tables re-emit pipe form, code fences widen only when content
/// demands it. Frontmatter re-emits verbatim ahead of the body (re-attach
/// via [`crate::frontmatter::prepend_frontmatter`] gives the same bytes).
/// Collected reference definitions re-emit (sorted by label) after the
/// body so link styles keep resolving. Output is stable: rendering the
/// re-parsed output yields the same text.
///
/// Known rounding limits (shared with the legacy Markdown emitter):
/// literal text that happens to read as markup (e.g. from backslash
/// escapes), and titles carrying `"` characters, may not re-parse to the
/// same nodes.
pub fn render_markdown(doc: &Document) -> String {
    let mut out = String::new();
    if let Some(fm) = &doc.frontmatter {
        out.push_str(&fm.original);
        out.push_str(if fm.original.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        });
    }
    out.push_str(&render_blocks_markdown(&doc.blocks));
    if !doc.references.is_empty() {
        if !doc.blocks.is_empty() {
            out.push('\n');
        }
        let mut labels: Vec<&String> = doc.references.keys().collect();
        labels.sort();
        for label in labels {
            let (dest, title) = &doc.references[label];
            out.push_str(&format_reference_definition(label, dest, title));
            out.push('\n');
        }
    }
    out
}

/// Format one reference definition so it re-parses: `[label]: dest
/// ["title"]`, wrapping spaceless-unsafe destinations in `<>`.
fn format_reference_definition(label: &str, dest: &str, title: &Option<String>) -> String {
    let needs_brackets = dest.contains([' ', '\t', '(', ')']);
    let mut out = String::from("[");
    out.push_str(label);
    out.push_str("]: ");
    if needs_brackets {
        out.push('<');
        out.push_str(dest);
        out.push('>');
    } else {
        out.push_str(dest);
    }
    if let Some(t) = title {
        if !t.is_empty() {
            out.push_str(&format!(" \"{}\"", t));
        }
    }
    out
}

fn render_blocks_markdown(blocks: &[Block]) -> String {
    let parts: Vec<String> = blocks.iter().map(render_block_markdown).collect();
    if parts.is_empty() {
        String::new()
    } else {
        parts.join("\n\n") + "\n"
    }
}

fn render_block_markdown(block: &Block) -> String {
    match block {
        Block::Paragraph(inlines) => render_inlines_markdown(inlines),
        Block::Heading(heading) => {
            format!(
                "{} {}",
                "#".repeat(heading.level as usize),
                render_inlines_markdown(&heading.content)
            )
        }
        Block::ThematicBreak(tb) => tb.marker.to_string().repeat(3),
        Block::CodeBlock(code) => render_code_markdown(code),
        Block::HtmlBlock(lines) => lines.join("\n"),
        Block::BlockQuote(children) => render_quote_markdown(children),
        Block::List(list) => render_list_markdown(list),
        Block::Table(table) => render_table_markdown(table),
    }
}

fn render_inlines_markdown(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(s) => out.push_str(s),
            Inline::Emphasis { delimiter, content } => {
                out.push(*delimiter);
                out.push_str(&render_inlines_markdown(content));
                out.push(*delimiter);
            }
            Inline::Strong { delimiter, content } => {
                out.push(*delimiter);
                out.push(*delimiter);
                out.push_str(&render_inlines_markdown(content));
                out.push(*delimiter);
                out.push(*delimiter);
            }
            Inline::Strikethrough(content) => {
                out.push_str("~~");
                out.push_str(&render_inlines_markdown(content));
                out.push_str("~~");
            }
            Inline::Code(s) => out.push_str(&fence_code_span(s)),
            Inline::Link(link) => {
                let inner = render_inlines_markdown(&link.text);
                match &link.style {
                    LinkStyle::Inline => {
                        out.push_str(&format_link_inline(&inner, &link.url, &link.title));
                    }
                    LinkStyle::Reference(label) => {
                        out.push_str(&format!("[{}][{}]", inner, label));
                    }
                    LinkStyle::Collapsed => {
                        out.push_str(&format!("[{}][]", inner));
                    }
                    LinkStyle::Shortcut => {
                        out.push_str(&format!("[{}]", inner));
                    }
                }
            }
            Inline::Image(img) => {
                out.push_str(&format!("![{}]", img.alt));
                match &img.style {
                    LinkStyle::Inline => {
                        out.push_str(&format!(
                            "({}{})",
                            img.url,
                            match &img.title {
                                Some(t) if !t.is_empty() => format!(" \"{}\"", t),
                                _ => String::new(),
                            }
                        ));
                    }
                    LinkStyle::Reference(label) => {
                        out.push_str(&format!("[{}]", label));
                    }
                    LinkStyle::Collapsed => out.push_str("[]"),
                    LinkStyle::Shortcut => {}
                }
            }
            Inline::Autolink(a) => {
                if a.bare {
                    out.push_str(&a.text);
                } else {
                    out.push('<');
                    out.push_str(&a.text);
                    out.push('>');
                }
            }
            Inline::RawHtml(s) => out.push_str(s),
            Inline::HardBreak => out.push_str("  \n"),
            Inline::SoftBreak => out.push('\n'),
        }
    }
    out
}

fn format_link_inline(inner: &str, url: &str, title: &Option<String>) -> String {
    match title {
        Some(t) if !t.is_empty() => format!("[{}]({} \"{}\")", inner, url, t),
        _ => format!("[{}]({})", inner, url),
    }
}

/// Wrap a code span in backticks, widening the fence past any backtick run
/// in the content (CommonMark code-span rule) so output re-parses.
fn fence_code_span(content: &str) -> String {
    let mut longest = 0usize;
    let mut run = 0usize;
    for c in content.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let fence = "`".repeat(longest + 1);
    if content.starts_with('`') || content.ends_with('`') {
        format!("{} {} {}", fence, content, fence)
    } else {
        format!("{}{}{}", fence, content, fence)
    }
}

fn render_code_markdown(code: &CodeBlock) -> String {
    match code.kind {
        CodeBlockKind::Indented => code
            .lines
            .iter()
            .map(|line| {
                if line.trim().is_empty() {
                    String::new()
                } else {
                    format!("    {}", line)
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        CodeBlockKind::Fenced {
            fence_char,
            fence_len,
        } => {
            let mut len = fence_len.max(3);
            if fence_char == '`' {
                let mut run = 0usize;
                for line in &code.lines {
                    for c in line.chars() {
                        if c == '`' {
                            run += 1;
                            len = len.max(run + 1);
                        } else {
                            run = 0;
                        }
                    }
                }
            }
            let fence: String = std::iter::repeat_n(fence_char, len).collect();
            let mut out = fence.clone();
            out.push_str(&code.info);
            for line in &code.lines {
                out.push('\n');
                out.push_str(line);
            }
            out.push('\n');
            out.push_str(&fence);
            out
        }
    }
}

fn render_quote_markdown(children: &[Block]) -> String {
    let inner = render_blocks_markdown(children);
    if inner.is_empty() {
        return ">".to_string();
    }
    inner
        .lines()
        .map(|line| {
            if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {}", line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_list_markdown(list: &List) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(list.items.len());
    let mut n = list.start;
    for item in &list.items {
        parts.push(render_list_item_markdown(list, n, item));
        if list.ordered {
            n += 1;
        }
    }
    let sep = if list.tight { "\n" } else { "\n\n" };
    parts.join(sep)
}

fn render_list_item_markdown(list: &List, n: u32, item: &ListItem) -> String {
    let marker = if list.ordered {
        let delim = if list.delimiter == '\0' {
            '.'
        } else {
            list.delimiter
        };
        format!("{}{}", n, delim)
    } else if list.bullet == '\0' {
        "-".to_string()
    } else {
        list.bullet.to_string()
    };
    let task = match item.task {
        Some(true) => "[x] ",
        Some(false) => "[ ] ",
        None => "",
    };
    if item.blocks.is_empty() {
        return marker;
    }
    let pad = " ".repeat(marker.len() + 1);
    let mut lines: Vec<String> = Vec::new();
    let mut first = true;
    for block in &item.blocks {
        let is_para = matches!(block, Block::Paragraph(_));
        if first && is_para {
            first = false;
            let text = match block {
                Block::Paragraph(inlines) => render_inlines_markdown(inlines),
                _ => String::new(),
            };
            if text.is_empty() {
                lines.push(format!("{} {}", marker, task.trim_end()));
            } else {
                // Soft-wrapped continuation lines ride under the item pad
                // so re-parsing keeps them in the item.
                let padded = text.replace('\n', &format!("\n{}", pad));
                lines.push(format!("{} {}{}", marker, task, padded));
            }
            continue;
        }
        first = false;
        let rendered = render_block_markdown(block);
        if lines.is_empty() {
            // Item starts with a non-paragraph block: marker on its own
            // line, content indented beneath it.
            lines.push(marker.clone());
        } else if list.tight {
            // Tight items keep blocks adjacent (no blank line).
        } else {
            lines.push(String::new());
        }
        for line in rendered.lines() {
            if line.is_empty() {
                lines.push(String::new());
            } else {
                lines.push(format!("{}{}", pad, line));
            }
        }
        if rendered.is_empty() {
            lines.push(pad.clone());
        }
    }
    lines.join("\n")
}

/// Escape a table cell for pipe-table form, mirroring the reverse
/// converter: `|` becomes `\|`, newlines become spaces.
fn escape_cell_markdown(cell: &str) -> String {
    cell.replace('|', "\\|").replace('\n', " ")
}

fn delimiter_marker_markdown(a: Alignment) -> &'static str {
    match a {
        Alignment::None => "---",
        Alignment::Left => ":--",
        Alignment::Center => ":-:",
        Alignment::Right => "--:",
    }
}

fn render_table_markdown(table: &Table) -> String {
    let ncols = table.header.len();
    if ncols == 0 {
        return String::new();
    }
    let cells: Vec<String> = table
        .header
        .iter()
        .map(|c| escape_cell_markdown(&render_inlines_markdown(&c.content)))
        .collect();
    let mut out = String::from("| ");
    out.push_str(&cells.join(" | "));
    out.push_str(" |\n|");
    for k in 0..ncols {
        let a = table.alignments.get(k).copied().unwrap_or(Alignment::None);
        out.push_str(&format!(" {} |", delimiter_marker_markdown(a)));
    }
    for row in &table.rows {
        out.push('\n');
        out.push('|');
        for k in 0..ncols {
            let cell = row
                .get(k)
                .map(|c| escape_cell_markdown(&render_inlines_markdown(&c.content)))
                .unwrap_or_default();
            out.push_str(&format!(" {} |", cell));
        }
    }
    out
}
