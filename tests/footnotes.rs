//! GFM footnote tests (cmark-gfm footnote-extension shape, GFM-gated).
//!
//! Markers follow the caret-bracket form: `[^label]` references in text and
//! `[^label]:` block definitions. Defined references render as numbered
//! `<sup class="footnote-ref">` anchors; definitions collect into a
//! `<section class="footnotes" data-footnotes>` footer in first-reference
//! order with one `footnote-backref` backlink per reference (repeat
//! references share the number but take unique `fnref-N-K` anchors).
//! Pandoc-style inline `^[...]` notes are NOT supported and stay literal.
//! With GFM off, every marker stays literal and compliance cannot move.

use pagina::ast::{self, Block, Inline};
use pagina::visitor::{walk_document, Visitor};
use pagina::{html_to_markdown, markdown_to_html};

fn html_gfm(input: &str) -> String {
    markdown_to_html::convert_gfm(input).expect("gfm md->html failed")
}

fn html_cm(input: &str) -> String {
    markdown_to_html::convert(input).expect("commonmark md->html failed")
}

fn rev_gfm(input: &str) -> String {
    html_to_markdown::convert_gfm(input).expect("gfm html->md failed")
}

fn rev_cm(input: &str) -> String {
    html_to_markdown::convert(input).expect("commonmark html->md failed")
}

// ---------------------------------------------------------------------------
// GFM reference shapes
// ---------------------------------------------------------------------------

#[test]
fn gfm_basic_reference_and_footer_shape() {
    let html = html_gfm("See this[^note].\n\n[^note]: The note.\n");
    // Reference anchor shape.
    assert!(
        html.contains("<sup class=\"footnote-ref\"><a href=\"#fn-1\" id=\"fnref-1\""),
        "ref anchor shape:\n{}",
        html
    );
    assert!(html.contains(">1</a></sup>"), "ref shows number:\n{}", html);
    // Footer section shape.
    assert!(
        html.contains("<section class=\"footnotes\" data-footnotes>"),
        "footer section:\n{}",
        html
    );
    assert!(html.contains("<li id=\"fn-1\">"), "footer item:\n{}", html);
    assert!(
        html.contains("<p>The note."),
        "definition content:\n{}",
        html
    );
    // Backlink shape.
    assert!(
        html.contains("href=\"#fnref-1\" class=\"footnote-backref\""),
        "backlink:\n{}",
        html
    );
    assert!(html.contains("↩"), "backlink glyph:\n{}", html);
}

#[test]
fn gfm_numbers_follow_first_reference_not_definition() {
    // Defined b-first, referenced a-first: footer order is a, b.
    let html = html_gfm("First[^a] then[^b].\n\n[^b]: Bee.\n\n[^a]: Ay.\n");
    let pa = html.find("<li id=\"fn-1\">").expect("fn-1 missing");
    let pb = html.find("<li id=\"fn-2\">").expect("fn-2 missing");
    assert!(pa < pb, "reference order wins:\n{}", html);
    assert!(html.contains("<p>Bee."), "b content:\n{}", html);
    assert!(html.contains("<p>Ay."), "a content:\n{}", html);
    // References carry their numbers.
    assert!(
        html.contains("href=\"#fn-2\" id=\"fnref-2\""),
        "b ref:\n{}",
        html
    );
}

#[test]
fn gfm_repeat_reference_shares_number_with_unique_anchors() {
    let html = html_gfm("One[^x] and two[^x].\n\n[^x]: Shared.\n");
    assert!(html.contains("id=\"fnref-1\""), "first anchor:\n{}", html);
    assert!(
        html.contains("id=\"fnref-1-2\""),
        "repeat anchor:\n{}",
        html
    );
    // Both display the same number.
    assert_eq!(
        html.matches(">1</a></sup>").count(),
        2,
        "numbers:\n{}",
        html
    );
    // The footer carries one backlink per reference, each unique.
    assert!(html.contains("href=\"#fnref-1\" class=\"footnote-backref\""));
    assert!(html.contains("href=\"#fnref-1-2\" class=\"footnote-backref\""));
    assert_eq!(
        html.matches("<li id=\"fn-1\">").count(),
        1,
        "one item:\n{}",
        html
    );
}

#[test]
fn gfm_multi_block_definition() {
    let html = html_gfm("Ref[^m].\n\n[^m]: First para.\n\n    Second para.\n");
    assert!(html.contains("<p>First para.</p>"), "para one:\n{}", html);
    assert!(
        html.contains("<p>Second para."),
        "para two holds backlink:\n{}",
        html
    );
}

#[test]
fn gfm_definition_before_use_and_after_use() {
    let before = html_gfm("[^d]: Def.\n\nUse[^d].\n");
    assert!(before.contains("<li id=\"fn-1\">"), "before:\n{}", before);
    let after = html_gfm("Use[^d].\n\n[^d]: Def.\n");
    assert!(after.contains("<li id=\"fn-1\">"), "after:\n{}", after);
    assert_eq!(before, after, "position-independent");
}

#[test]
fn gfm_first_definition_wins() {
    let html = html_gfm("R[^d].\n\n[^d]: First.\n\n[^d]: Second.\n");
    assert!(html.contains("First."), "first wins:\n{}", html);
    assert!(!html.contains("Second"), "dup dropped:\n{}", html);
}

#[test]
fn gfm_unreferenced_definition_dropped_from_html() {
    let html = html_gfm("R[^a].\n\n[^a]: Used.\n\n[^b]: Orphan.\n");
    assert!(html.contains("Used."), "used kept:\n{}", html);
    assert!(!html.contains("Orphan"), "orphan dropped:\n{}", html);
    assert!(!html.contains("fn-2"), "no second item:\n{}", html);
}

#[test]
fn gfm_undefined_reference_stays_literal() {
    let html = html_gfm("See[^nope] here.\n");
    assert!(html.contains("[^nope]"), "literal:\n{}", html);
    assert!(!html.contains("footnote-ref"), "no markup:\n{}", html);
    assert!(!html.contains("data-footnotes"), "no footer:\n{}", html);
}

#[test]
fn gfm_labels_are_case_sensitive() {
    let html = html_gfm("See[^A].\n\n[^a]: Lower.\n");
    assert!(
        html.contains("[^A]"),
        "case mismatch stays literal:\n{}",
        html
    );
    assert!(!html.contains("data-footnotes"), "no footer:\n{}", html);
}

#[test]
fn gfm_no_pandoc_inline_notes() {
    // Caret notes without brackets are not footnotes here.
    let html = html_gfm("Text^[inline note] here.\n");
    assert!(html.contains("^[inline note]"), "literal:\n{}", html);
    assert!(!html.contains("data-footnotes"), "no footer:\n{}", html);
}

#[test]
fn gfm_code_spans_never_form_references() {
    let html = html_gfm("`[^a]` and [^a].\n\n[^a]: Def.\n");
    assert!(
        html.contains("<code>[^a]</code>"),
        "code literal:\n{}",
        html
    );
    assert_eq!(
        html.matches("<sup class=\"footnote-ref\">").count(),
        1,
        "one ref:\n{}",
        html
    );
}

#[test]
fn gfm_reference_inside_emphasis_and_heading() {
    let html = html_gfm("# Title[^h]\n\n[^h]: H.\n\nSome *em[^e] here*.\n\n[^e]: E.\n");
    assert!(html.contains("<h1>Title<sup"), "heading ref:\n{}", html);
    assert!(html.contains("<em>em<sup"), "emphasis ref:\n{}", html);
    assert!(html.contains("<li id=\"fn-1\">"), "h def:\n{}", html);
    assert!(html.contains("<li id=\"fn-2\">"), "e def:\n{}", html);
}

#[test]
fn commonmark_markers_stay_literal() {
    // No definition around: the marker is plain text.
    let html = html_cm("See[^a] here.\n");
    assert!(html.contains("[^a]"), "ref literal:\n{}", html);
    assert!(!html.contains("footnote"), "no footnote markup:\n{}", html);
    // Historical CommonMark nuance (unchanged): `[^a]: /url` still parses
    // as a link reference definition with label `^a`, so `[^a]` resolves
    // as a shortcut link — never as a footnote.
    let html = html_cm("See[^a] here.\n\n[^a]: /url\n");
    assert!(
        html.contains("<a href=\"/url\">^a</a>"),
        "shortcut link, not footnote:\n{}",
        html
    );
    assert!(!html.contains("footnote-ref"), "no footnote:\n{}", html);
    assert!(!html.contains("data-footnotes"), "no footer:\n{}", html);
}

// ---------------------------------------------------------------------------
// AST shapes + walker
// ---------------------------------------------------------------------------

#[test]
fn ast_footnote_node_shapes() {
    let doc = ast::parse_gfm("See[^a] twice[^a].\n\n[^a]: Body *em*.\n\n[^z]: Orphan.\n");
    // Reference inlines carry label + shared number.
    let para = match &doc.blocks[0] {
        Block::Paragraph(inlines) => inlines,
        other => panic!("expected para, got {:?}", other),
    };
    let refs: Vec<(&str, usize)> = para
        .iter()
        .filter_map(|i| match i {
            Inline::FootnoteReference(r) => Some((r.label.as_str(), r.number)),
            _ => None,
        })
        .collect();
    assert_eq!(refs, vec![("a", 1), ("a", 1)], "refs: {:?}", refs);
    // Definitions hoisted after the body in reference order, orphan last.
    let defs: Vec<(&str, usize)> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::FootnoteDefinition(d) => Some((d.label.as_str(), d.number)),
            _ => None,
        })
        .collect();
    assert_eq!(defs, vec![("a", 1), ("z", 0)], "defs: {:?}", defs);
    match &doc.blocks[1] {
        Block::FootnoteDefinition(d) => match &d.blocks[0] {
            Block::Paragraph(inlines) => {
                assert_eq!(ast::plain_text(inlines), "Body em.");
            }
            other => panic!("expected para, got {:?}", other),
        },
        other => panic!("expected footnote def, got {:?}", other),
    }
}

#[derive(Default)]
struct FootCounter {
    refs: usize,
    def_blocks: usize,
}

impl Visitor for FootCounter {
    fn visit_block(&mut self, block: &Block) {
        if matches!(block, Block::FootnoteDefinition(_)) {
            self.def_blocks += 1;
        }
    }
    fn visit_inline(&mut self, inline: &Inline) {
        if matches!(inline, Inline::FootnoteReference(_)) {
            self.refs += 1;
        }
    }
}

#[test]
fn visitor_sees_footnote_refs_and_definition_content() {
    let doc = ast::parse_gfm("A[^x].\n\n[^x]: Body.\n");
    let mut c = FootCounter::default();
    walk_document(&doc, &mut c);
    assert_eq!(c.refs, 1);
    assert_eq!(c.def_blocks, 1);
}

// ---------------------------------------------------------------------------
// Legacy / AST parity + Markdown stability
// ---------------------------------------------------------------------------

const PARITY_CORPUS: &[&str] = &[
    "See[^1].\n\n[^1]: Note.\n",
    "A[^x] and B[^x] and C[^y].\n\n[^x]: X.\n\n[^y]: Y.\n",
    "Ref[^m].\n\n[^m]: P1.\n\n    P2 with *em*.\n",
    "Before[^d] after.\n\n[^d]: Def with `code` and [link](/u).\n",
    "No defs here[^q].\n",
    "Orphan[^z] free.\n\n[^z]: Unused.\n",
    "# Head[^h]\n\n[^h]: H.\n\n> Quote[^q2]\n>\n> more\n\n[^q2]: Q.\n",
    "- Item[^l]\n- plain\n\n[^l]: L.\n",
];

#[test]
fn legacy_ast_html_parity() {
    for md in PARITY_CORPUS {
        let expected = html_gfm(md);
        let actual = ast::render_html(&ast::parse_gfm(md));
        assert_eq!(actual, expected, "HTML mismatch for {:?}", md);
    }
}

fn assert_stable(md: &str) {
    let once = ast::render_markdown(&ast::parse_gfm(md));
    let twice = ast::render_markdown(&ast::parse_gfm(&once));
    assert_eq!(twice, once, "not idempotent for {:?}", md);
    let html_before = ast::render_html(&ast::parse_gfm(md));
    let html_after = ast::render_html(&ast::parse_gfm(&once));
    assert_eq!(html_after, html_before, "HTML drift for {:?}", md);
}

#[test]
fn markdown_round_trip_stable() {
    for md in PARITY_CORPUS {
        assert_stable(md);
    }
    assert_stable("B[^b] A[^a].\n\n[^a]: Ay.\n\n[^b]: Bee.\n");
}

// ---------------------------------------------------------------------------
// Reverse direction (HTML -> Markdown)
// ---------------------------------------------------------------------------

#[test]
fn reverse_footnote_section_to_ref_and_def() {
    // Punctuation-adjacent references: the reverse converter's historical
    // whitespace normalization keeps these byte-stable round-trip.
    let html = html_gfm("See[^1], twice[^1].\n\n[^1]: The note.\n");
    let md = rev_gfm(&html);
    assert!(md.contains("[^1]"), "ref:\n{}", md);
    assert!(md.contains("[^1]: The note."), "def:\n{}", md);
    assert!(!md.contains("↩"), "no backlink text:\n{}", md);
    assert!(!md.contains("fnref"), "no anchor residue:\n{}", md);
    // Re-parsing the reversed form renders the same HTML.
    let html2 = html_gfm(&md);
    assert_eq!(html2, html, "reverse round trip");
}

#[test]
fn reverse_github_style_footer() {
    // Tolerate the GitHub `<div>` + `<hr>` footer flavor.
    let html = "<p>Hi<sup id=\"fnref-a\"><a href=\"#fn-a\">1</a></sup></p>\
        <div class=\"footnotes\" data-footnotes><hr><ol>\
        <li id=\"fn-a\"><p>Alpha <a href=\"#fnref-a\" class=\"footnote-backref\">&#8617;</a></p></li>\
        </ol></div>";
    let md = rev_gfm(html);
    assert!(md.contains("[^a]"), "ref:\n{}", md);
    assert!(md.contains("[^a]: Alpha"), "def:\n{}", md);
}

#[test]
fn reverse_off_mode_has_no_footnote_markers() {
    let html = html_gfm("See[^1].\n\n[^1]: The note.\n");
    let md = rev_cm(&html);
    assert!(!md.contains("[^"), "no markers:\n{}", md);
    assert!(!md.contains("data-footnotes"), "no residue:\n{}", md);
    // Content itself survives unwrapped.
    assert!(md.contains("See"), "text kept:\n{}", md);
    assert!(md.contains("The note."), "def text kept:\n{}", md);
}
