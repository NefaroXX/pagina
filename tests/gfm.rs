//! GFM opt-in extension tests (task lists, strikethrough, bare autolinks).
//!
//! Default [`pagina::markdown_to_html::convert`] is pure CommonMark 0.31.2:
//! `~~`, `[ ]` prefixes and bare `http(s)://` / `www.` / emails stay
//! literal (notably a bare `a@b.com` stays plain per spec example 612).
//! [`pagina::markdown_to_html::convert_gfm`] (and `convert_with` with
//! `Options::gfm()`) enables the three extensions. Pipe tables are the one
//! always-on GFM exception and render in both modes (spec-neutral: the
//! CommonMark suite has no pipe-table tests).

use pagina::html_to_markdown;
use pagina::markdown_to_html;

fn md(html_mode: bool, input: &str) -> String {
    if html_mode {
        markdown_to_html::convert_gfm(input).expect("gfm md->html failed")
    } else {
        markdown_to_html::convert(input).expect("commonmark md->html failed")
    }
}

fn rev(gfm: bool, input: &str) -> String {
    if gfm {
        html_to_markdown::convert_gfm(input).expect("gfm html->md failed")
    } else {
        html_to_markdown::convert(input).expect("commonmark html->md failed")
    }
}

// ---------------------------------------------------------------------------
// Task lists: Markdown -> HTML
// ---------------------------------------------------------------------------

#[test]
fn gfm_task_list_unchecked_and_checked() {
    let html = md(true, "- [ ] todo\n- [x] done\n- [X] upper\n- plain\n");
    assert!(html.contains("<li><input type=\"checkbox\" disabled=\"\" /> todo</li>"));
    assert!(html.contains("<li><input type=\"checkbox\" checked=\"\" disabled=\"\" /> done</li>"));
    assert!(html.contains("<li><input type=\"checkbox\" checked=\"\" disabled=\"\" /> upper</li>"));
    assert!(html.contains("<li>plain</li>"));
}

#[test]
fn commonmark_task_brackets_stay_literal() {
    let html = md(false, "- [ ] todo\n- [x] done\n");
    assert!(html.contains("<li>[ ] todo</li>"));
    assert!(html.contains("<li>[x] done</li>"));
    assert!(!html.contains("checkbox"));
}

#[test]
fn gfm_task_requires_space_after_bracket() {
    // No whitespace after `]` (or a non-space marker) is not a task.
    let html = md(true, "- [ ]todo\n- [y] x\n");
    assert!(html.contains("<li>[ ]todo</li>"));
    assert!(html.contains("<li>[y] x</li>"));
    assert!(!html.contains("checkbox"));
}

#[test]
fn gfm_task_loose_list_checkbox_inside_paragraph() {
    let html = md(true, "- [ ] a\n\n- [x] b\n");
    assert!(html.contains("<p><input type=\"checkbox\" disabled=\"\" /> a</p>"));
    assert!(html.contains("<p><input type=\"checkbox\" checked=\"\" disabled=\"\" /> b</p>"));
}

// ---------------------------------------------------------------------------
// Strikethrough: Markdown -> HTML
// ---------------------------------------------------------------------------

#[test]
fn gfm_strikethrough_renders_del() {
    let html = md(true, "~~strike~~");
    assert_eq!(html, "<p><del>strike</del></p>\n");
}

#[test]
fn commonmark_double_tilde_stays_literal() {
    let html = md(false, "~~strike~~");
    assert_eq!(html, "<p>~~strike~~</p>\n");
}

#[test]
fn gfm_single_tilde_stays_literal() {
    let html = md(true, "~x~ and a ~ b");
    assert!(html.contains("~x~"), "lone tildes must not form <del>");
    assert!(!html.contains("<del>"));
}

#[test]
fn gfm_strikethrough_nests_emphasis_but_not_code() {
    let html = md(true, "~~**bold**~~ and `~~code~~`");
    assert!(html.contains("<del><strong>bold</strong></del>"));
    assert!(html.contains("<code>~~code~~</code>"));
}

// ---------------------------------------------------------------------------
// Bare autolinks: Markdown -> HTML
// ---------------------------------------------------------------------------

#[test]
fn gfm_bare_http_link_and_trailing_punct_trim() {
    let html = md(true, "Visit http://example.com.");
    assert!(html.contains("<a href=\"http://example.com\">http://example.com</a>."));
}

#[test]
fn gfm_bare_www_gets_http_href() {
    let html = md(true, "see www.example.com/x");
    assert!(html.contains("<a href=\"http://www.example.com/x\">www.example.com/x</a>"));
}

#[test]
fn gfm_bare_email_becomes_mailto_link() {
    let html = md(true, "mail a@b.com ok");
    assert!(html.contains("<a href=\"mailto:a@b.com\">a@b.com</a>"));
}

#[test]
fn commonmark_bare_url_and_email_stay_plain() {
    // Spec example 612 boundary: without GFM a bare email is plain text.
    let html = md(false, "Visit http://example.com and a@b.com");
    assert!(
        !html.contains("<a href"),
        "CommonMark mode must not linkify"
    );
    assert!(html.contains("http://example.com"));
    assert!(html.contains("a@b.com"));
}

#[test]
fn gfm_no_link_inside_code_or_explicit_links() {
    let html = md(true, "`http://example.com` and [t](http://example.com)");
    assert!(html.contains("<code>http://example.com</code>"));
    assert_eq!(html.matches("<a href=\"http://example.com\">").count(), 1);
}

// ---------------------------------------------------------------------------
// HTML -> Markdown (GFM reverse)
// ---------------------------------------------------------------------------

#[test]
fn gfm_checkbox_to_bracket_prefix() {
    let md = rev(
        true,
        "<ul><li><input type=\"checkbox\" checked=\"\" disabled=\"\" /> done</li>\
         <li><input type=\"checkbox\" disabled=\"\" /> todo</li>\
         <li>plain</li></ul>",
    );
    assert!(md.contains("- [x] done"));
    assert!(md.contains("- [ ] todo"));
    assert!(md.contains("- plain"));
}

#[test]
fn commonmark_checkbox_input_vanishes() {
    let md = rev(
        false,
        "<ul><li><input type=\"checkbox\" checked=\"\" /> done</li></ul>",
    );
    assert!(!md.contains("[x]"));
    assert!(md.contains("- done"));
}

#[test]
fn gfm_del_s_strike_to_double_tilde() {
    assert_eq!(rev(true, "<p><del>hi</del></p>"), "~~hi~~\n");
    assert_eq!(rev(true, "<p><s>hi</s></p>"), "~~hi~~\n");
    assert_eq!(rev(true, "<p><strike>hi</strike></p>"), "~~hi~~\n");
}

#[test]
fn commonmark_del_unwraps_to_plain() {
    assert_eq!(rev(false, "<p><del>hi</del></p>"), "hi\n");
}

#[test]
fn gfm_bare_anchors_collapse_to_text() {
    assert_eq!(
        rev(
            true,
            "<p><a href=\"http://example.com\">http://example.com</a></p>"
        ),
        "http://example.com\n"
    );
    assert_eq!(
        rev(true, "<p><a href=\"mailto:a@b.com\">a@b.com</a></p>"),
        "a@b.com\n"
    );
    assert_eq!(
        rev(
            true,
            "<p><a href=\"http://www.example.com\">www.example.com</a></p>"
        ),
        "www.example.com\n"
    );
}

#[test]
fn gfm_non_bare_anchor_keeps_link_form() {
    assert_eq!(
        rev(true, "<p><a href=\"http://example.com\">click</a></p>"),
        "[click](http://example.com)\n"
    );
}

#[test]
fn commonmark_anchors_always_keep_link_form() {
    assert_eq!(
        rev(
            false,
            "<p><a href=\"http://example.com\">http://example.com</a></p>"
        ),
        "[http://example.com](http://example.com)\n"
    );
}

// ---------------------------------------------------------------------------
// Options / entry-point shape + tables exception
// ---------------------------------------------------------------------------

#[test]
fn options_entry_points_agree() {
    let via_opts =
        markdown_to_html::convert_with("~~x~~", markdown_to_html::Options::gfm()).unwrap();
    assert_eq!(via_opts, md(true, "~~x~~"));
    let via_opts =
        html_to_markdown::convert_with("<del>x</del>", html_to_markdown::Options::gfm()).unwrap();
    assert_eq!(via_opts, rev(true, "<del>x</del>"));
    assert_eq!(markdown_to_html::Options::default().gfm, false);
    assert_eq!(html_to_markdown::Options::default().gfm, false);
}

#[test]
fn tables_render_with_and_without_gfm() {
    // Documented always-on exception: tables are spec-neutral GFM that
    // render even in pure-CommonMark mode.
    let table = "| a |\n| - |\n| b |\n";
    assert!(md(false, table).contains("<table>"));
    assert!(md(true, table).contains("<table>"));
}
