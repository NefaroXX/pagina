//! GFM definition-list tests (PHP Markdown Extra shape, GFM-gated).
//!
//! An unindented term paragraph followed by colon-prefixed descriptions
//! (up to 3-space indent) renders as `<dl>` with `<dt>` terms and `<dd>`
//! descriptions. Consecutive terms share the following descriptions; a
//! single-paragraph description in a tight list unwraps `<p>` (mirroring
//! tight list items), while blank-separated bodies render loose. With GFM
//! off, every marker stays literal paragraph text.

use pagina::ast::{self, Block};
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
// Shapes: tight lists
// ---------------------------------------------------------------------------

#[test]
fn gfm_basic_term_and_description() {
    let html = html_gfm("Term\n: Definition\n");
    assert_eq!(
        html, "<dl>\n<dt>Term</dt>\n<dd>Definition</dd>\n</dl>\n",
        "tight dl:\n{}",
        html
    );
}

#[test]
fn gfm_multi_term_shared_definition() {
    let html = html_gfm("Apple\nOrange\n: Fruit\n");
    assert_eq!(
        html, "<dl>\n<dt>Apple</dt>\n<dt>Orange</dt>\n<dd>Fruit</dd>\n</dl>\n",
        "shared:\n{}",
        html
    );
}

#[test]
fn gfm_multi_line_term_paragraph_promotes_each_line() {
    // Each line of the term paragraph becomes its own `<dt>`.
    let html = html_gfm("First\nSecond\n: Shared\n");
    assert!(html.contains("<dt>First</dt>"), "dt1:\n{}", html);
    assert!(html.contains("<dt>Second</dt>"), "dt2:\n{}", html);
    assert!(html.contains("<dd>Shared</dd>"), "dd:\n{}", html);
}

#[test]
fn gfm_multiple_descriptions_for_one_term() {
    let html = html_gfm("Term\n: One\n: Two\n");
    assert_eq!(
        html, "<dl>\n<dt>Term</dt>\n<dd>One</dd>\n<dd>Two</dd>\n</dl>\n",
        "two dd:\n{}",
        html
    );
}

#[test]
fn gfm_multiple_items_share_one_list() {
    let html = html_gfm("A\n: Ay\nB\n: Bee\n");
    assert_eq!(
        html, "<dl>\n<dt>A</dt>\n<dd>Ay</dd>\n<dt>B</dt>\n<dd>Bee</dd>\n</dl>\n",
        "items:\n{}",
        html
    );
}

#[test]
fn gfm_term_inline_markup_parses() {
    let html = html_gfm("*Em* term\n: `code` desc\n");
    assert!(html.contains("<dt><em>Em</em> term</dt>"), "dt:\n{}", html);
    assert!(
        html.contains("<dd><code>code</code> desc</dd>"),
        "dd:\n{}",
        html
    );
}

#[test]
fn gfm_description_up_to_three_space_indent() {
    let html = html_gfm("Term\n   : Indented\n");
    assert!(html.contains("<dd>Indented</dd>"), "3-space:\n{}", html);
}

#[test]
fn gfm_definition_list_inside_blockquote() {
    let html = html_gfm("> Term\n> : Quoted def\n");
    assert!(html.contains("<blockquote>"), "quote:\n{}", html);
    assert!(html.contains("<dt>Term</dt>"), "dt:\n{}", html);
    assert!(html.contains("<dd>Quoted def</dd>"), "dd:\n{}", html);
}

// ---------------------------------------------------------------------------
// Loose lists
// ---------------------------------------------------------------------------

#[test]
fn gfm_loose_description_keeps_paragraphs() {
    let html = html_gfm("Term\n: First.\n\n    Second.\n");
    assert!(
        html.contains("<dd>\n<p>First.</p>"),
        "loose open:\n{}",
        html
    );
    assert!(html.contains("<p>Second.</p>"), "loose body:\n{}", html);
    assert!(!html.contains("<dd>First."), "not tight:\n{}", html);
}

#[test]
fn gfm_blank_between_items_loosens() {
    let html = html_gfm("A\n: Ay\n\nB\n: Bee\n");
    assert!(html.contains("<dd>\n<p>Ay</p>"), "loose:\n{}", html);
    assert_eq!(html.matches("<dl>").count(), 1, "one list:\n{}", html);
}

// ---------------------------------------------------------------------------
// Non-lists stay literal
// ---------------------------------------------------------------------------

#[test]
fn gfm_orphan_colon_line_is_paragraph() {
    let html = html_gfm(": orphan\n");
    assert!(!html.contains("<dl>"), "no list:\n{}", html);
    assert!(html.contains(": orphan"), "literal:\n{}", html);
}

#[test]
fn gfm_double_colon_is_not_a_marker() {
    let html = html_gfm("Term\n:: Not a def\n");
    assert!(!html.contains("<dl>"), "no list:\n{}", html);
}

#[test]
fn gfm_four_space_description_is_not_a_marker() {
    // Four spaces is code/continuation territory, not a marker.
    let html = html_gfm("Term\n    : Codeish\n");
    assert!(!html.contains("<dd>"), "no dd:\n{}", html);
}

#[test]
fn gfm_indented_term_stays_paragraph() {
    // Terms must be unindented; an indented term never promotes.
    let html = html_gfm("  Term\n  : Def\n");
    assert!(!html.contains("<dl>"), "no list:\n{}", html);
    assert!(html.contains(": Def"), "literal:\n{}", html);
}

#[test]
fn commonmark_deflist_markers_stay_literal() {
    let html = html_cm("Term\n: Definition\n");
    assert!(!html.contains("<dl>"), "no list:\n{}", html);
    assert!(html.contains(": Definition"), "literal:\n{}", html);
}

// ---------------------------------------------------------------------------
// AST shapes
// ---------------------------------------------------------------------------

#[test]
fn ast_deflist_node_shapes() {
    let doc = ast::parse_gfm("A\nB\n: One\n: Two\n");
    match &doc.blocks[0] {
        Block::DefinitionList(dl) => {
            assert!(dl.tight);
            assert_eq!(dl.items.len(), 1);
            assert_eq!(dl.items[0].terms.len(), 2);
            assert_eq!(dl.items[0].descriptions.len(), 2);
            assert_eq!(ast::plain_text(&dl.items[0].terms[0].content), "A");
            match &dl.items[0].descriptions[0].blocks[0] {
                Block::Paragraph(inlines) => {
                    assert_eq!(ast::plain_text(inlines), "One");
                }
                other => panic!("expected para, got {:?}", other),
            }
        }
        other => panic!("expected dl, got {:?}", other),
    }
    // Off-mode parses no definition lists.
    let cm = ast::parse("A\nB\n: One\n", pagina::Options::default());
    assert!(
        !cm.blocks
            .iter()
            .any(|b| matches!(b, Block::DefinitionList(_))),
        "off-mode: {:?}",
        cm.blocks
    );
}

// ---------------------------------------------------------------------------
// Legacy / AST parity + Markdown stability
// ---------------------------------------------------------------------------

const PARITY_CORPUS: &[&str] = &[
    "Term\n: Definition\n",
    "Apple\nOrange\n: Fruit\n",
    "Term\n: One\n: Two\n",
    "A\n: Ay\nB\n: Bee\n",
    "Term\n: First.\n\n    Second.\n",
    "*Em* term\n: `code` desc\n",
    "Term\n: Def with [link](/u)\n",
    "> Term\n> : Quoted def\n",
    "- item\n\n  Term\n  : Nested?\n",
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
    assert_eq!(
        twice, once,
        "not idempotent for {:?} (once: {:?})",
        md, once
    );
    let html_before = ast::render_html(&ast::parse_gfm(md));
    let html_after = ast::render_html(&ast::parse_gfm(&once));
    assert_eq!(html_after, html_before, "HTML drift for {:?}", md);
}

#[test]
fn markdown_round_trip_stable() {
    for md in PARITY_CORPUS {
        assert_stable(md);
    }
}

// ---------------------------------------------------------------------------
// Reverse direction (HTML -> Markdown)
// ---------------------------------------------------------------------------

#[test]
fn reverse_dl_to_term_colon_form() {
    let html = html_gfm("Term\n: Definition\n");
    let md = rev_gfm(&html);
    assert_eq!(md, "Term\n: Definition\n");
    // Re-parsing the reversed form renders the same list.
    assert_eq!(html_gfm(&md), html);
}

#[test]
fn reverse_multi_term_and_loose_dd() {
    let md = rev_gfm(
        "<dl><dt>A</dt><dt>B</dt><dd>Shared</dd>\
         <dt>C</dt><dd><p>P1</p><p>P2</p></dd></dl>",
    );
    assert!(!md.contains("<dt>"), "no tags:\n{}", md);
    assert!(md.contains("A\nB\n: Shared"), "shared:\n{}", md);
    assert!(md.contains(": P1"), "loose head:\n{}", md);
    assert!(md.contains("    P2"), "indented tail:\n{}", md);
    // The reversed loose form re-parses to an equivalent list.
    let html = html_gfm(&md);
    assert!(html.contains("<dl>"), "list:\n{}", html);
    assert!(html.contains("<dd>\n<p>P1</p>"), "loose:\n{}", html);
}

#[test]
fn reverse_off_mode_has_no_definition_markers() {
    let md = rev_cm("<dl><dt>Term</dt><dd>Definition</dd></dl>");
    assert!(!md.contains("\n: "), "no markers:\n{}", md);
    assert!(!md.contains("<dl>"), "no tags:\n{}", md);
    assert!(md.contains("Term"), "term kept:\n{}", md);
    assert!(md.contains("Definition"), "desc kept:\n{}", md);
}
