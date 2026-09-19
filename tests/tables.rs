//! GFM pipe-table tests for `pagina`'s Markdown <-> HTML converters.
//!
//! These cover the GitHub Flavored Markdown table extension (GFM spec
//! examples 198-205): delimiter/alignment parsing, cell escaping, row
//! padding/truncation, count-mismatch fallback, and the HTML -> Markdown
//! direction (alignment via `align`/`style`, `|` escaping, thead handling).

use pagina::html_to_markdown;
use pagina::markdown_to_html;

fn m2h(md: &str) -> String {
    markdown_to_html::convert(md).expect("markdown conversion failed")
}

fn h2m(html: &str) -> String {
    html_to_markdown::convert(html).expect("html conversion failed")
}

// ---------------------------------------------------------------------------
// Markdown -> HTML
// ---------------------------------------------------------------------------

#[test]
fn basic_table_matches_gfm_example() {
    // GFM spec example 198.
    let md = "| foo | bar |\n| --- | --- |\n| baz | bim |\n";
    let expected = concat!(
        "<table>\n",
        "<thead>\n",
        "<tr>\n",
        "<th>foo</th>\n",
        "<th>bar</th>\n",
        "</tr>\n",
        "</thead>\n",
        "<tbody>\n",
        "<tr>\n",
        "<td>baz</td>\n",
        "<td>bim</td>\n",
        "</tr>\n",
        "</tbody>\n",
        "</table>\n",
    );
    assert_eq!(m2h(md), expected);
}

#[test]
fn alignments_apply_to_headers_and_body() {
    // GFM spec example 199: alignment markers with/without pipes.
    let md = "| abc | def | ghi |\n| :-- | :-: | --: |\n| bar | baz | bim |\n";
    let expected = concat!(
        "<table>\n",
        "<thead>\n",
        "<tr>\n",
        "<th align=\"left\">abc</th>\n",
        "<th align=\"center\">def</th>\n",
        "<th align=\"right\">ghi</th>\n",
        "</tr>\n",
        "</thead>\n",
        "<tbody>\n",
        "<tr>\n",
        "<td align=\"left\">bar</td>\n",
        "<td align=\"center\">baz</td>\n",
        "<td align=\"right\">bim</td>\n",
        "</tr>\n",
        "</tbody>\n",
        "</table>\n",
    );
    assert_eq!(m2h(md), expected);
}

#[test]
fn delimiter_without_outer_pipes() {
    // GFM spec example 199 variant: `:-: | -----------:` (no outer pipes).
    let md = "| abc | def |\n:-: | -----------:\n| bar | baz |\n";
    let html = m2h(md);
    assert!(html.contains("<th align=\"center\">abc</th>"));
    assert!(html.contains("<th align=\"right\">def</th>"));
    assert!(html.contains("<td align=\"center\">bar</td>"));
    assert!(html.contains("<td align=\"right\">baz</td>"));
}

#[test]
fn pipe_less_header_still_makes_table() {
    // cmark-gfm: a header without any `|` is a valid one-column table.
    let md = "abc\n| --- |\n| def |\n";
    let html = m2h(md);
    assert!(html.contains("<th>abc</th>"));
    assert!(html.contains("<td>def</td>"));
    assert!(!html.contains("<p>"));
}

#[test]
fn cells_support_inline_formatting() {
    let md = "| **bold** | *em* | `code` |\n| --- | --- | --- |\n| [x](u) | ~~no~~ | x |\n";
    let html = m2h(md);
    assert!(html.contains("<th><strong>bold</strong></th>"));
    assert!(html.contains("<th><em>em</em></th>"));
    assert!(html.contains("<th><code>code</code></th>"));
    assert!(html.contains("<td><a href=\"u\">x</a></td>"));
}

#[test]
fn escaped_pipes_in_cells() {
    // GFM spec example 200 (shape).
    let md = "| f\\|oo  |\n| ------ |\n| b `\\|` az |\n| **\\|** |\n";
    let html = m2h(md);
    assert!(html.contains("<th>f|oo</th>"));
    assert!(html.contains("<td>b <code>|</code> az</td>"));
    assert!(html.contains("<td><strong>|</strong></td>"));
}

#[test]
fn trailing_pipe_does_not_add_empty_cell() {
    let md = "| f\\|oo  |\n| ------ |\n| b `\\|` az |\n| **\\|** |\n";
    let html = m2h(md);
    // Trailing pipes are consumed; no stray empty cells appear.
    assert_eq!(
        html.matches("<td>").count() + html.matches("<td align=").count(),
        2
    );
}

#[test]
fn body_rows_pad_and_truncate() {
    // GFM spec examples 202/204 (shape): rows shorter are padded, longer
    // are truncated to the header width.
    let md = "| abc | def |\n| --- | --- |\n| bar |\n| bar | baz | boo |\n";
    let html = m2h(md);
    assert!(html.contains("<tr>\n<td>bar</td>\n<td></td>\n</tr>"));
    assert!(html.contains("<tr>\n<td>bar</td>\n<td>baz</td>\n</tr>"));
    assert!(!html.contains("boo"));
}

#[test]
fn count_mismatch_is_a_paragraph() {
    // GFM spec example 203 (shape): 2 header cells vs 1 delimiter cell.
    let md = "| abc | def |\n| --- |\n";
    let html = m2h(md);
    assert!(html.starts_with("<p>| abc | def |\n| --- |</p>\n"));
    assert!(!html.contains("<table>"));
}

#[test]
fn no_tbody_without_body_rows() {
    // GFM spec example 205 (shape).
    let md = "| abc | def |\n| --- | --- |\n";
    let html = m2h(md);
    assert_eq!(
        html,
        concat!(
            "<table>\n",
            "<thead>\n",
            "<tr>\n",
            "<th>abc</th>\n",
            "<th>def</th>\n",
            "</tr>\n",
            "</thead>\n",
            "</table>\n",
        )
    );
}

#[test]
fn leading_paragraph_lines_flush_before_table() {
    let md = "foo\nbar | baz\n| --- | --- |\n| x | y |\n";
    let html = m2h(md);
    assert!(html.starts_with("<p>foo</p>\n<table>\n"));
    assert!(html.contains("<th>bar</th>"));
    assert!(html.contains("<th>baz</th>"));
    assert!(html.contains("<td>x</td>"));
}

#[test]
fn blank_line_ends_table() {
    let md = "| a |\n| - |\n| b |\n\npara\n";
    let html = m2h(md);
    assert!(html.contains("</table>\n<p>para</p>\n"));
}

#[test]
fn blockquote_sibling_ends_table() {
    // GFM spec example 201 (shape): `> x` after the delimiter starts a quote.
    let md = "| abc |\n| --- |\n> x\n";
    let html = m2h(md);
    assert!(html.contains("</table>\n<blockquote>\n<p>x</p>\n"));
}

#[test]
fn setext_is_not_a_body_row_terminator() {
    // `===` under a delimiter is a table row, not a setext underline.
    let md = "| a |\n| - |\n| === |\n";
    let html = m2h(md);
    assert!(html.contains("<td>===</td>"));
    assert!(!html.contains("<h1>"));
}

#[test]
fn thematic_break_ends_table() {
    let md = "| a |\n| - |\n---\n";
    let html = m2h(md);
    assert!(html.contains("</table>\n<hr />\n"));
}

#[test]
fn list_marker_ends_table() {
    let md = "| a |\n| - |\n- item\n";
    let html = m2h(md);
    assert!(html.contains("</table>\n<ul>"));
}

#[test]
fn nos_table_inside_fenced_or_indented_code() {
    let fenced = "```\n| a |\n| - |\n```\n";
    assert!(!m2h(fenced).contains("<table>"));

    let indented = "    | a |\n    | - |\n";
    let html = m2h(indented);
    assert!(!html.contains("<table>"));
    assert!(html.contains("<pre><code>"));
}

#[test]
fn html_block_start_ends_table() {
    // `<b>` (HTML block start) after the delimiter ends the table, matching
    // the block parser's own HTML-block rule (empirically verified against
    // cmark-gfm).
    let md = "| a |\n| - |\n<b>\n";
    let html = m2h(md);
    assert!(html.contains("</table>\n<b>\n"));
    assert!(!html.contains("<td>"));
}

#[test]
fn delimiter_with_four_space_indent_is_not_a_table() {
    let md = "| a |\n    | - |\n";
    let html = m2h(md);
    assert!(!html.contains("<table>"));
}

#[test]
fn round_trip_markdown_to_html_to_markdown() {
    let md = "| a | b |\n| :-: | --: |\n| x | y |\n";
    let html = m2h(md);
    let back = h2m(&html);
    let html2 = m2h(&back);
    assert_eq!(html, html2);
}

// ---------------------------------------------------------------------------
// HTML -> Markdown
// ---------------------------------------------------------------------------

#[test]
fn html_to_markdown_basic_table() {
    let html = "<table><thead><tr><th>a</th><th>b</th></tr></thead><tbody><tr><td>x</td><td>y</td></tr></tbody></table>";
    assert_eq!(h2m(html), "| a | b |\n| --- | --- |\n| x | y |\n");
}

#[test]
fn html_to_markdown_alignment_attr() {
    let html = "<table><tr><th align=\"left\">a</th><th align=\"center\">b</th><th align=\"right\">c</th></tr><tr><td>x</td><td>y</td><td>z</td></tr></table>";
    assert_eq!(
        h2m(html),
        "| a | b | c |\n| :-- | :-: | --: |\n| x | y | z |\n"
    );
}

#[test]
fn html_to_markdown_alignment_style() {
    let html = "<table><tr><th style=\"text-align: right;\">a</th><th>b</th></tr><tr><td>x</td><td>y</td></tr></table>";
    assert_eq!(h2m(html), "| a | b |\n| --: | --- |\n| x | y |\n");
}

#[test]
fn html_to_markdown_escapes_pipes() {
    let html = "<table><tr><th>a|b</th><th>c</th></tr><tr><td>x</td><td>y</td></tr></table>";
    assert_eq!(h2m(html), "| a\\|b | c |\n| --- | --- |\n| x | y |\n");
    // Escaped Markdown round-trips back to the same cell content.
    let md = h2m(html);
    let html2 = m2h(&md);
    assert!(html2.contains("<th>a|b</th>"));
}

#[test]
fn html_to_markdown_thead_headers() {
    let html = "<table><thead><tr><th>h1</th><th>h2</th></tr></thead><tbody><tr><td>x</td><td>y</td></tr></tbody></table>";
    assert_eq!(h2m(html), "| h1 | h2 |\n| --- | --- |\n| x | y |\n");
}

#[test]
fn html_to_markdown_no_thead_uses_first_th_row() {
    let html = "<table><tr><th>h</th><td>n</td></tr><tr><td>x</td><td>y</td></tr></table>";
    assert_eq!(h2m(html), "| h | n |\n| --- | --- |\n| x | y |\n");
}

#[test]
fn html_to_markdown_rows_pad_and_truncate() {
    let html = "<table><tr><th>a</th><th>b</th><th>c</th></tr><tr><td>x</td></tr><tr><td>1</td><td>2</td><td>3</td><td>4</td></tr></table>";
    assert_eq!(
        h2m(html),
        "| a | b | c |\n| --- | --- | --- |\n| x |  |  |\n| 1 | 2 | 3 |\n"
    );
}

#[test]
fn html_to_markdown_inline_cells() {
    let html =
        "<table><tr><th><strong>a</strong></th></tr><tr><td><code>x</code></td></tr></table>";
    assert_eq!(h2m(html), "| **a** |\n| --- |\n| `x` |\n");
}
