//! Public AST tests: parse shapes, visitor counts, and round-trips.
//!
//! - Parse shapes cover every block/inline kind in `pagina::ast`.
//! - Visitor tests show TOC extraction and counting without rendering.
//! - `md -> ast -> html` must equal `convert`/`convert_gfm` on the corpus.
//! - `md -> ast -> md` must be stable (idempotent, HTML-equivalent).

use pagina::ast::{self, Alignment, Block, CodeBlockKind, HeadingKind, Inline, LinkStyle};
use pagina::visitor::{walk_document, Visitor};
use pagina::{html_to_markdown, markdown_to_html, Options};

fn doc(input: &str) -> ast::Document {
    ast::parse(input, Options::default())
}

fn doc_gfm(input: &str) -> ast::Document {
    ast::parse_gfm(input)
}

// ---------------------------------------------------------------------------
// Parse shapes: blocks
// ---------------------------------------------------------------------------

#[test]
fn parse_atx_heading_shape() {
    let d = doc("# Hello *world*\n");
    assert_eq!(d.blocks.len(), 1);
    match &d.blocks[0] {
        Block::Heading(h) => {
            assert_eq!(h.level, 1);
            assert_eq!(h.kind, HeadingKind::Atx { closing: false });
            assert_eq!(ast::plain_text(&h.content), "Hello world");
        }
        other => panic!("expected heading, got {:?}", other),
    }
}

#[test]
fn parse_atx_closing_hashes_retained() {
    let d = doc("## Hi ##\n");
    match &d.blocks[0] {
        Block::Heading(h) => {
            assert_eq!(h.level, 2);
            assert_eq!(h.kind, HeadingKind::Atx { closing: true });
            assert_eq!(ast::plain_text(&h.content), "Hi");
        }
        other => panic!("expected heading, got {:?}", other),
    }
}

#[test]
fn parse_setext_heading_shape() {
    let d = doc("Title\n===\n\nSub\n---\n");
    assert_eq!(d.blocks.len(), 2);
    match &d.blocks[0] {
        Block::Heading(h) => {
            assert_eq!(h.level, 1);
            assert_eq!(h.kind, HeadingKind::Setext { marker: '=' });
        }
        other => panic!("expected h1, got {:?}", other),
    }
    match &d.blocks[1] {
        Block::Heading(h) => {
            assert_eq!(h.level, 2);
            assert_eq!(h.kind, HeadingKind::Setext { marker: '-' });
        }
        other => panic!("expected h2, got {:?}", other),
    }
}

#[test]
fn parse_blockquote_nesting() {
    let d = doc("> quote\n> > nested\n");
    match &d.blocks[0] {
        Block::BlockQuote(children) => {
            assert_eq!(children.len(), 2);
            assert!(matches!(children[0], Block::Paragraph(_)));
            assert!(matches!(children[1], Block::BlockQuote(_)));
        }
        other => panic!("expected quote, got {:?}", other),
    }
}

#[test]
fn parse_tight_and_loose_lists() {
    let tight = doc("- a\n- b\n");
    match &tight.blocks[0] {
        Block::List(list) => {
            assert!(!list.ordered);
            assert!(list.tight);
            assert_eq!(list.bullet, '-');
            assert_eq!(list.items.len(), 2);
            assert!(list.items[0].task.is_none());
        }
        other => panic!("expected list, got {:?}", other),
    }
    let loose = doc("- a\n\n- b\n");
    match &loose.blocks[0] {
        Block::List(list) => assert!(!list.tight),
        other => panic!("expected list, got {:?}", other),
    }
}

#[test]
fn parse_ordered_list_markers() {
    let d = doc("3) a\n4) b\n");
    match &d.blocks[0] {
        Block::List(list) => {
            assert!(list.ordered);
            assert_eq!(list.start, 3);
            assert_eq!(list.delimiter, ')');
        }
        other => panic!("expected ol, got {:?}", other),
    }
}

#[test]
fn parse_task_markers_gfm_gated() {
    let gfm = doc_gfm("- [ ] todo\n- [x] done\n- plain\n");
    match &gfm.blocks[0] {
        Block::List(list) => {
            assert_eq!(list.items[0].task, Some(false));
            assert_eq!(list.items[1].task, Some(true));
            assert_eq!(list.items[2].task, None);
            // Prefix stripped from content.
            assert_eq!(
                ast::plain_text(match &list.items[0].blocks[0] {
                    Block::Paragraph(inlines) => inlines,
                    other => panic!("expected para, got {:?}", other),
                }),
                "todo"
            );
        }
        other => panic!("expected list, got {:?}", other),
    }
    // CommonMark mode: brackets stay literal, no task state.
    let cm = doc("- [ ] todo\n");
    match &cm.blocks[0] {
        Block::List(list) => {
            assert_eq!(list.items[0].task, None);
            match &list.items[0].blocks[0] {
                Block::Paragraph(inlines) => {
                    assert_eq!(ast::plain_text(inlines), "[ ] todo");
                }
                other => panic!("expected para, got {:?}", other),
            }
        }
        other => panic!("expected list, got {:?}", other),
    }
}

#[test]
fn parse_code_blocks() {
    let d = doc("```rust\nlet x = 1;\n```\n");
    match &d.blocks[0] {
        Block::CodeBlock(code) => {
            assert_eq!(
                code.kind,
                CodeBlockKind::Fenced {
                    fence_char: '`',
                    fence_len: 3
                }
            );
            assert_eq!(code.info, "rust");
            assert_eq!(code.lines, vec!["let x = 1;".to_string()]);
        }
        other => panic!("expected code, got {:?}", other),
    }
    let indented = doc("    code\n");
    match &indented.blocks[0] {
        Block::CodeBlock(code) => {
            assert_eq!(code.kind, CodeBlockKind::Indented);
            assert_eq!(code.lines, vec!["code".to_string()]);
        }
        other => panic!("expected code, got {:?}", other),
    }
}

#[test]
fn parse_thematic_break_marker() {
    let d = doc("***\n");
    assert!(matches!(
        d.blocks[0],
        Block::ThematicBreak(ast::ThematicBreak { marker: '*' })
    ));
}

#[test]
fn parse_table_alignments() {
    let d = doc("| a | b | c | d |\n| --- | :-- | :-: | --: |\n| 1 | 2 | 3 | 4 |\n");
    match &d.blocks[0] {
        Block::Table(table) => {
            assert_eq!(
                table.alignments,
                vec![
                    Alignment::None,
                    Alignment::Left,
                    Alignment::Center,
                    Alignment::Right
                ]
            );
            assert_eq!(table.header.len(), 4);
            assert_eq!(table.rows.len(), 1);
            assert_eq!(ast::plain_text(&table.rows[0][2].content), "3");
        }
        other => panic!("expected table, got {:?}", other),
    }
}

#[test]
fn parse_html_block_verbatim() {
    let d = doc("<div>\nfoo\n</div>\n");
    match &d.blocks[0] {
        Block::HtmlBlock(lines) => assert_eq!(lines, &vec!["<div>", "foo", "</div>"]),
        other => panic!("expected html block, got {:?}", other),
    }
}

#[test]
fn parse_reference_definitions_store() {
    let d = doc("[foo]: /url \"title\"\n\n[foo]\n");
    assert_eq!(d.references.len(), 1);
    assert_eq!(
        d.references["foo"],
        ("/url".to_string(), Some("title".to_string()))
    );
    // The definition itself produces no block.
    assert_eq!(d.blocks.len(), 1);
}

#[test]
fn parse_frontmatter_attachment() {
    let d = doc("---\ntitle: Hi\n---\n# Hi\n");
    let fm = d.frontmatter.expect("frontmatter attached");
    assert!(fm.original.contains("title: Hi"));
    assert_eq!(d.blocks.len(), 1);
    assert!(matches!(d.blocks[0], Block::Heading(_)));
    // No fence: no attachment.
    assert!(doc("# Hi\n").frontmatter.is_none());
}

// ---------------------------------------------------------------------------
// Parse shapes: inlines
// ---------------------------------------------------------------------------

fn only_para_inlines(d: &ast::Document) -> &Vec<Inline> {
    match &d.blocks[0] {
        Block::Paragraph(inlines) => inlines,
        other => panic!("expected paragraph, got {:?}", other),
    }
}

#[test]
fn parse_emphasis_delimiters_retained() {
    let star_doc = doc("*hi* and **bye**\n");
    let star = only_para_inlines(&star_doc);
    assert!(matches!(star[0], Inline::Emphasis { delimiter: '*', .. }));
    assert!(matches!(star[2], Inline::Strong { delimiter: '*', .. }));
    let under_doc = doc("_hi_ and __bye__\n");
    let under = only_para_inlines(&under_doc);
    assert!(matches!(under[0], Inline::Emphasis { delimiter: '_', .. }));
    assert!(matches!(under[2], Inline::Strong { delimiter: '_', .. }));
}

#[test]
fn parse_strikethrough_gfm_gated() {
    let gfm_doc = doc_gfm("~~hi~~\n");
    let gfm = only_para_inlines(&gfm_doc);
    assert!(matches!(gfm[0], Inline::Strikethrough(_)));
    let cm_doc = doc("~~hi~~\n");
    let cm = only_para_inlines(&cm_doc);
    assert!(matches!(cm[0], Inline::Text(_)));
    assert_eq!(ast::plain_text(cm), "~~hi~~");
}

#[test]
fn parse_link_styles_retained() {
    let d = doc("[a](/u) [b][L] [c][] [d]\n\n[L]: /l\n[c]: /c\n[d]: /d\n");
    let inlines = only_para_inlines(&d);
    let styles: Vec<&LinkStyle> = inlines
        .iter()
        .filter_map(|i| match i {
            Inline::Link(l) => Some(&l.style),
            _ => None,
        })
        .collect();
    assert_eq!(
        styles,
        vec![
            &LinkStyle::Inline,
            &LinkStyle::Reference("L".to_string()),
            &LinkStyle::Collapsed,
            &LinkStyle::Shortcut,
        ]
    );
}

#[test]
fn parse_autolinks_angle_and_bare() {
    let d = doc("<http://example.com>\n");
    match &only_para_inlines(&d)[0] {
        Inline::Autolink(a) => {
            assert!(!a.bare);
            assert_eq!(a.url, "http://example.com");
            assert_eq!(a.text, "http://example.com");
        }
        other => panic!("expected autolink, got {:?}", other),
    }
    let gfm_doc = doc_gfm("see http://example.com/x\n");
    let found = only_para_inlines(&gfm_doc)
        .iter()
        .find(|i| matches!(i, Inline::Autolink(_)));
    match found {
        Some(Inline::Autolink(a)) => {
            assert!(a.bare);
            assert_eq!(a.url, "http://example.com/x");
        }
        other => panic!("expected bare autolink, got {:?}", other),
    }
    // CommonMark mode keeps bare URLs plain.
    let cm_doc = doc("see http://example.com/x\n");
    let cm = only_para_inlines(&cm_doc);
    assert!(!cm.iter().any(|i| matches!(i, Inline::Autolink(_))));
}

#[test]
fn parse_images_code_rawhtml_breaks() {
    let d = doc("![alt](/img.png \"t\") `code` <b>x</b>\n");
    let inlines = only_para_inlines(&d);
    match &inlines[0] {
        Inline::Image(img) => {
            assert_eq!(img.alt, "alt");
            assert_eq!(img.url, "/img.png");
            assert_eq!(img.title.as_deref(), Some("t"));
            assert_eq!(img.style, LinkStyle::Inline);
        }
        other => panic!("expected image, got {:?}", other),
    }
    assert!(matches!(inlines[2], Inline::Code(_)));
    assert!(matches!(inlines[4], Inline::RawHtml(_)));

    let hard_doc = doc("a  \nb\n");
    let hard = only_para_inlines(&hard_doc);
    assert!(hard.iter().any(|i| matches!(i, Inline::HardBreak)));
    let soft_doc = doc("a\nb\n");
    let soft = only_para_inlines(&soft_doc);
    assert!(soft.iter().any(|i| matches!(i, Inline::SoftBreak)));
}

// ---------------------------------------------------------------------------
// Visitor
// ---------------------------------------------------------------------------

#[derive(Default)]
struct HeadingCollector {
    entries: Vec<(u8, String)>,
}

impl Visitor for HeadingCollector {
    fn visit_block(&mut self, block: &Block) {
        if let Block::Heading(h) = block {
            self.entries.push((h.level, ast::plain_text(&h.content)));
        }
    }
}

#[test]
fn visitor_extracts_toc() {
    let d = doc("# A\n\ntext\n\n## B *em*\n\n### C\n");
    let mut toc = HeadingCollector::default();
    walk_document(&d, &mut toc);
    assert_eq!(
        toc.entries,
        vec![
            (1, "A".to_string()),
            (2, "B em".to_string()),
            (3, "C".to_string()),
        ]
    );
}

#[derive(Default)]
struct Counter {
    blocks: usize,
    inlines: usize,
    links: usize,
}

impl Visitor for Counter {
    fn visit_block(&mut self, _block: &Block) {
        self.blocks += 1;
    }
    fn visit_inline(&mut self, inline: &Inline) {
        self.inlines += 1;
        if matches!(inline, Inline::Link(_)) {
            self.links += 1;
        }
    }
}

#[test]
fn visitor_counts_blocks_and_inlines() {
    // 2 blocks (heading + para); heading holds 1 Text, para holds Text,
    // Emphasis(+1 Text child), Text, Link(+1 Text child) = 7 inline visits.
    let d = doc("# T\n\nHi *em* x [l](/u)\n");
    let mut c = Counter::default();
    walk_document(&d, &mut c);
    assert_eq!(c.blocks, 2);
    assert_eq!(c.links, 1);
    assert_eq!(c.inlines, 7);
}

#[test]
fn visitor_sees_nested_and_table_cells() {
    let d = doc("> - [a](/u)\n\n| h |\n| - |\n| b |\n");
    let mut c = Counter::default();
    walk_document(&d, &mut c);
    // quote, list, item para, table = 4 blocks; link + texts visited.
    assert_eq!(c.blocks, 4);
    assert_eq!(c.links, 1);
    assert!(c.inlines >= 4);
}

// ---------------------------------------------------------------------------
// Round-trip: md -> ast -> html equals convert
// ---------------------------------------------------------------------------

const HTML_PARITY_CORPUS: &[&str] = &[
    "# Hello\n",
    "## Closed ##\n",
    "Foo\n===\n",
    "Bar\n---\n",
    "Hello *world* **bold** _em_ __strong__\n",
    "***foo** and `code` and <b>raw</b>\n",
    "> quote\n> > nested\n",
    "- a\n- b\n- c\n",
    "1. one\n2. two\n",
    "3) three\n4) four\n",
    "- a\n\n- b\n",
    "- parent\n  - child\n",
    "```rust\nlet x = 1;\n```\n",
    "    indented\n",
    "***\n",
    "- - -\n",
    "[link](/url \"title\") and ![img](/i.png)\n",
    "[foo]: /url \"title\"\n\n[foo] and [t][foo] and [foo][]\n",
    "<http://example.com> and <a@b.com>\n",
    "<div>\nfoo\n</div>\n",
    "| a | b |\n| --- | :-: |\n| 1 | 2 |\n",
    "para one\n\npara two\n",
    "a  \nb\n",
    "a\nb\n",
    "---\ntitle: Hi\n---\n# Hi\n",
    "# H1\n\n> quote with [link](/u)\n\n- item *em*\n\n```\ncode\n```\n",
];

const HTML_PARITY_GFM_CORPUS: &[&str] = &[
    "- [ ] todo\n- [x] done\n- plain\n",
    "- [ ] a\n\n- [x] b\n",
    "~~strike~~ and **bold**\n",
    "Visit http://example.com and www.example.com/x and a@b.com\n",
    "- [x] **done** with `code`\n",
    "# GFM *doc* with ~~strike~~\n\n- [ ] item\n",
];

#[test]
fn ast_html_matches_convert() {
    for md in HTML_PARITY_CORPUS {
        let expected = markdown_to_html::convert(md).expect("convert failed");
        let actual = ast::render_html(&doc(md));
        assert_eq!(actual, expected, "HTML mismatch for {:?}", md);
    }
}

#[test]
fn ast_html_matches_convert_gfm() {
    for md in HTML_PARITY_GFM_CORPUS {
        let expected = markdown_to_html::convert_gfm(md).expect("convert_gfm failed");
        let actual = ast::render_html(&doc_gfm(md));
        assert_eq!(actual, expected, "GFM HTML mismatch for {:?}", md);
    }
}

// ---------------------------------------------------------------------------
// Round-trip: md -> ast -> md stability
// ---------------------------------------------------------------------------

const MD_STABILITY_CORPUS: &[&str] = &[
    "# Hello\n",
    "## Closed ##\n",
    "Title\n===\n",
    "Hello *world* **bold** _em_ __strong__\n",
    "> quote\n> > nested\n",
    "- a\n- b\n",
    "1. one\n2. two\n",
    "3) three\n",
    "- a\n\n- b\n",
    "- parent\n  - child\n",
    "```rust\nlet x = 1;\n```\n",
    "~~~info\ntilde\n~~~\n",
    "    indented\n",
    "***\n",
    "[link](/url \"title\") and ![img](/i.png)\n",
    "[foo]: /url \"title\"\n\n[foo] and [t][foo] and [foo][]\n",
    "<http://example.com>\n",
    "<div>\nfoo\n</div>\n",
    "| a | b |\n| --- | :-: |\n| 1 | 2 |\n",
    "para one\n\npara two\n",
    "a  \nb\n",
    "---\ntitle: Hi\n---\n# Hi\n",
    "# H1\n\n> quote with [link](/u)\n\n- item *em*\n\n```\ncode\n```\n",
];

const MD_STABILITY_GFM_CORPUS: &[&str] = &[
    "- [ ] todo\n- [x] done\n",
    "- [ ] a\n\n- [x] b\n",
    "~~strike~~\n",
    "Visit http://example.com and a@b.com\n",
];

fn assert_stable(md: &str, gfm: bool) {
    let once = if gfm {
        ast::render_markdown(&doc_gfm(md))
    } else {
        ast::render_markdown(&doc(md))
    };
    // Idempotent: re-rendering the re-parsed output changes nothing.
    let twice = if gfm {
        ast::render_markdown(&doc_gfm(&once))
    } else {
        ast::render_markdown(&doc(&once))
    };
    assert_eq!(twice, once, "not idempotent for {:?}", md);
    // HTML-equivalent: the normalized form renders the same HTML.
    let html_before = if gfm {
        ast::render_html(&doc_gfm(md))
    } else {
        ast::render_html(&doc(md))
    };
    let html_after = if gfm {
        ast::render_html(&doc_gfm(&once))
    } else {
        ast::render_html(&doc(&once))
    };
    assert_eq!(html_after, html_before, "HTML drift for {:?}", md);
}

#[test]
fn ast_markdown_round_trip_stable() {
    for md in MD_STABILITY_CORPUS {
        assert_stable(md, false);
    }
}

#[test]
fn ast_markdown_round_trip_stable_gfm() {
    for md in MD_STABILITY_GFM_CORPUS {
        assert_stable(md, true);
    }
}

// ---------------------------------------------------------------------------
// Reverse-direction sanity: AST markdown feeds the HTML reverse converter
// ---------------------------------------------------------------------------

#[test]
fn ast_markdown_parses_back_to_same_html() {
    let html = markdown_to_html::convert("# T\n\n- a\n- b\n").unwrap();
    let md = html_to_markdown::convert(&html).unwrap();
    let html2 = markdown_to_html::convert(&md).unwrap();
    assert_eq!(html2, html);
}
