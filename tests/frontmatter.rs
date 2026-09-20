//! YAML frontmatter tests (`---` / `+++` leading blocks).
//!
//! Covers: silent stripping in every `markdown_to_html` entry point (so
//! CommonMark compliance is unaffected), verbatim `original` preservation,
//! the `parse_with_frontmatter` / `markdown_to_html_with_frontmatter` /
//! `prepend_frontmatter` round-trip API, and the documented rule that
//! `html_to_markdown` cannot carry frontmatter (round-trip is MD-side).

use pagina::frontmatter::{
    markdown_to_html_with_frontmatter, markdown_to_html_with_frontmatter_gfm,
    parse_with_frontmatter, prepend_frontmatter,
};
use pagina::{html_to_markdown, markdown_to_html};

fn convert(input: &str) -> String {
    markdown_to_html::convert(input).expect("md->html failed")
}

// ---------------------------------------------------------------------------
// Stripping: convert() must behave as if the block were absent
// ---------------------------------------------------------------------------

#[test]
fn dash_block_stripped_before_convert() {
    let fenced = "---\ntitle: Hello\n---\n# Hello\n";
    assert_eq!(convert(fenced), convert("# Hello\n"));
    assert_eq!(convert(fenced), "<h1>Hello</h1>\n");
}

#[test]
fn plus_block_stripped_before_convert() {
    let fenced = "+++\ntitle: Hello\n+++\n# Hello\n";
    assert_eq!(convert(fenced), "<h1>Hello</h1>\n");
}

#[test]
fn strip_applies_to_gfm_entry_points() {
    let fenced = "---\ntitle: x\n---\n~~hi~~\n";
    let plain = "~~hi~~\n";
    assert_eq!(
        markdown_to_html::convert_gfm(fenced).unwrap(),
        markdown_to_html::convert_gfm(plain).unwrap()
    );
    assert_eq!(
        markdown_to_html::convert_with(fenced, markdown_to_html::Options::gfm()).unwrap(),
        "<p><del>hi</del></p>\n"
    );
}

#[test]
fn spec_setext_documents_are_never_frontmatter() {
    // CommonMark spec examples 96 and 98: leading `---` with non-mapping
    // content is setext / thematic breaks, not metadata.
    assert_eq!(
        convert("---\nFoo\n---\nBar\n---\nBaz"),
        "<hr />\n<h2>Foo</h2>\n<h2>Bar</h2>\n<p>Baz</p>\n"
    );
    assert_eq!(convert("---\n---"), "<hr />\n<hr />\n");
}

#[test]
fn lone_leading_dashes_stay_a_thematic_break() {
    // No closing fence: not frontmatter, still CommonMark `<hr />`.
    assert_eq!(convert("---\n"), "<hr />\n");
    assert_eq!(convert("---\n\n# Hi\n"), "<hr />\n<h1>Hi</h1>\n");
}

#[test]
fn unclosed_block_is_not_frontmatter() {
    let input = "---\ntitle: x\n# Hi\n";
    let (fm, body) = parse_with_frontmatter(input);
    assert!(fm.is_none());
    assert_eq!(body, input);
}

#[test]
fn mismatched_closing_delimiter_is_not_frontmatter() {
    let input = "---\ntitle: x\n+++\n# Hi\n";
    let (fm, body) = parse_with_frontmatter(input);
    assert!(fm.is_none());
    assert_eq!(body, input);
}

#[test]
fn block_must_start_on_first_line() {
    let input = "\n---\ntitle: x\n---\n# Hi\n";
    let (fm, _) = parse_with_frontmatter(input);
    assert!(fm.is_none());
    // Not stripped: the `---` lines render as normal Markdown content.
    let html = convert(input);
    assert!(html.contains("<hr />"), "expected content rendering, got {html:?}");
    assert!(html.contains("<h1>Hi</h1>"));
}

#[test]
fn indented_opening_fence_is_content_not_metadata() {
    let input = "  ---\n  title: x\n  ---\n";
    let (fm, _) = parse_with_frontmatter(input);
    assert!(fm.is_none());
}

#[test]
fn empty_dash_block_is_content_not_metadata() {
    // `---\n---` is two CommonMark `<hr />`s (spec example 98): the mapping
    // guard keeps setext/hr documents untouched.
    let (fm, body) = parse_with_frontmatter("---\n---\n# Hi\n");
    assert!(fm.is_none());
    assert_eq!(body, "---\n---\n# Hi\n");
    assert_eq!(convert("---\n---"), "<hr />\n<hr />\n");
}

#[test]
fn empty_plus_block_is_still_frontmatter() {
    // `+++` has no CommonMark meaning, so no content guard applies.
    let (fm, body) = parse_with_frontmatter("+++\n+++\n# Hi\n");
    let fm = fm.expect("empty plus block is frontmatter");
    assert_eq!(fm.original, "+++\n+++");
    assert_eq!(body, "# Hi\n");
    assert_eq!(convert("+++\n+++\n# Hi\n"), "<h1>Hi</h1>\n");
}

#[test]
fn trailing_spaces_on_fences_tolerated() {
    let (fm, body) = parse_with_frontmatter("---  \ntitle: x\n---\t\n# Hi\n");
    let fm = fm.expect("fences with trailing blanks");
    assert_eq!(body, "# Hi\n");
    assert!(fm.original.starts_with("---"));
    assert!(fm.original.ends_with("---"));
}

#[test]
fn crlf_document_round_trips_verbatim() {
    let input = "---\r\ntitle: x\r\n---\r\n# Hi\r\n";
    let (fm, body) = parse_with_frontmatter(input);
    let fm = fm.expect("CRLF frontmatter");
    assert_eq!(body, "# Hi\r\n");
    assert_eq!(convert(input), "<h1>Hi</h1>\n");
    assert_eq!(prepend_frontmatter(&fm, body), input);
}

// ---------------------------------------------------------------------------
// parse_with_frontmatter: verbatim original + body split
// ---------------------------------------------------------------------------

#[test]
fn parse_preserves_original_verbatim() {
    let input = "---\ntitle: \"Hello: world\" # comment\ncount: 3\n---\n# Hello\n";
    let (fm, body) = parse_with_frontmatter(input);
    let fm = fm.expect("frontmatter present");
    assert_eq!(fm.original, "---\ntitle: \"Hello: world\" # comment\ncount: 3\n---");
    assert_eq!(body, "# Hello\n");
}

#[test]
fn parse_absent_returns_input_untouched() {
    let input = "# Just a heading\n";
    let (fm, body) = parse_with_frontmatter(input);
    assert!(fm.is_none());
    assert_eq!(body, input);
}

// ---------------------------------------------------------------------------
// markdown_to_html_with_frontmatter
// ---------------------------------------------------------------------------

#[test]
fn with_frontmatter_returns_metadata_plus_body_html() {
    let (fm, html) =
        markdown_to_html_with_frontmatter("---\ntitle: Hi\n---\n# Hi\n").unwrap();
    let fm = fm.expect("metadata kept aside");
    assert_eq!(fm.original, "---\ntitle: Hi\n---");
    assert_eq!(html, "<h1>Hi</h1>\n");
}

#[test]
fn with_frontmatter_absent_returns_none_and_full_html() {
    let (fm, html) = markdown_to_html_with_frontmatter("# Hi\n").unwrap();
    assert!(fm.is_none());
    assert_eq!(html, "<h1>Hi</h1>\n");
}

#[test]
fn with_frontmatter_gfm_renders_body_as_gfm() {
    let (fm, html) =
        markdown_to_html_with_frontmatter_gfm("+++\ntitle: x\n+++\n~~hi~~\n").unwrap();
    assert!(fm.is_some());
    assert_eq!(html, "<p><del>hi</del></p>\n");
}

// ---------------------------------------------------------------------------
// Round-trip is MD-side: html_to_markdown drops frontmatter, prepend restores
// ---------------------------------------------------------------------------

#[test]
fn html_round_trip_needs_prepend() {
    let md = "---\ntitle: Hi\n---\n# Hi\n";
    let (fm, body) = parse_with_frontmatter(md);
    let fm = fm.expect("frontmatter present");
    let html = convert(body);
    let back = html_to_markdown::convert(&html).expect("html->md failed");
    // HTML carries no frontmatter: the body-only round-trip loses it …
    assert!(!back.starts_with("---\n"));
    // … until re-attached verbatim.
    assert_eq!(prepend_frontmatter(&fm, &back), md);
}

#[test]
fn prepend_is_byte_for_byte_identity() {
    for md in [
        "---\ntitle: Hi\n---\n# Hi\n",
        "+++\ntitle: Hi\n+++\n",
        "+++\n+++\n",
        "---\ntitle: Hi\n---\n",
    ] {
        let (fm, body) = parse_with_frontmatter(md);
        let rebuilt = prepend_frontmatter(&fm.expect("frontmatter present"), body);
        assert_eq!(rebuilt, md, "round-trip failed for {md:?}");
    }
}

// ---------------------------------------------------------------------------
// Parsed data: feature vs. no-feature representation
// ---------------------------------------------------------------------------

#[cfg(feature = "frontmatter")]
#[test]
fn yaml_data_parsed_with_feature() {
    let (fm, _) = parse_with_frontmatter("---\ntitle: Hello\ndraft: true\ncount: 3\n---\n# H\n");
    let data = &fm.expect("frontmatter present").data;
    assert_eq!(data["title"].as_str(), Some("Hello"));
    assert_eq!(data["draft"].as_bool(), Some(true));
    assert_eq!(data["count"].as_i64(), Some(3));
}

#[cfg(feature = "frontmatter")]
#[test]
fn yaml_lists_parse_with_feature() {
    let (fm, _) = parse_with_frontmatter("---\ntags: [rust, docs]\n---\n# H\n");
    let data = &fm.expect("frontmatter present").data;
    let tags = data["tags"].as_vec().expect("sequence");
    assert_eq!(tags.len(), 2);
    assert_eq!(tags[0].as_str(), Some("rust"));
}

#[cfg(feature = "frontmatter")]
#[test]
fn invalid_yaml_still_detects_with_bad_value() {
    let input = "---\ntitle: [unclosed flow\n---\n# H\n";
    let (fm, body) = parse_with_frontmatter(input);
    let fm = fm.expect("detection never depends on parsing");
    assert_eq!(body, "# H\n");
    assert!(fm.original.starts_with("---\n"));
    // Conversion still strips the block and renders the body.
    assert_eq!(convert(input), "<h1>H</h1>\n");
}

#[cfg(not(feature = "frontmatter"))]
#[test]
fn subset_data_without_feature() {
    let (fm, _) = parse_with_frontmatter(
        "---\ntitle: Hello\ndraft: true\n# a comment\n  nested: skipped\n---\n# H\n",
    );
    let data = &fm.expect("frontmatter present").data;
    assert_eq!(data.get("title"), Some(&"Hello".to_string()));
    assert_eq!(data.get("draft"), Some(&"true".to_string()));
    assert!(!data.contains_key("nested"));
}

#[cfg(not(feature = "frontmatter"))]
#[test]
fn subset_strips_surrounding_quotes() {
    let (fm, _) =
        parse_with_frontmatter("---\na: \"quoted\"\nb: 'single'\n---\n# H\n");
    let data = &fm.expect("frontmatter present").data;
    assert_eq!(data.get("a"), Some(&"quoted".to_string()));
    assert_eq!(data.get("b"), Some(&"single".to_string()));
}
