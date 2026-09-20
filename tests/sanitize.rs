//! Sanitization tests for `pagina`'s opt-in `sanitize` feature.
//!
//! The default [`pagina::markdown_to_html::convert`] path passes raw HTML
//! through verbatim per CommonMark (100% spec compliance); that contract
//! is locked here unconditionally. All sanitizer assertions are gated on
//! `#[cfg(feature = "sanitize")]` so this file compiles and passes both
//! with the default build and with `--features sanitize`.

use pagina::markdown_to_html;

// ---------------------------------------------------------------------------
// Default path: raw HTML passthrough (always compiled, no feature needed).
// ---------------------------------------------------------------------------

/// Locks the CommonMark verbatim-passthrough contract: without the
/// sanitized entry points, `convert()` must NOT strip or neutralize
/// anything, even known-XSS payloads. Sanitization is opt-in only.
#[test]
fn default_convert_still_passes_raw_html_through() {
    let html = markdown_to_html::convert("<script>alert(1)</script>").unwrap();
    assert!(
        html.contains("<script>alert(1)</script>"),
        "default convert() must pass <script> through verbatim, got: {html}"
    );

    let html = markdown_to_html::convert("<p onclick=\"alert(1)\">x</p>").unwrap();
    assert!(
        html.contains("onclick"),
        "default convert() must keep onclick attributes, got: {html}"
    );

    let html = markdown_to_html::convert("[x](javascript:alert(1))").unwrap();
    assert!(
        html.contains("javascript:alert(1)"),
        "default convert() must keep javascript: hrefs, got: {html}"
    );
}

// ---------------------------------------------------------------------------
// Sanitized path (only compiled with `--features sanitize`).
// ---------------------------------------------------------------------------

#[cfg(feature = "sanitize")]
#[test]
fn sanitize_strips_script_elements() {
    let html = markdown_to_html::convert_sanitized("<script>alert(1)</script>").unwrap();
    assert!(!html.contains("<script"), "script tag survived: {html}");
    assert!(!html.contains("alert(1)"), "script body survived: {html}");

    // Case-insensitive, and the content goes with the element.
    let html = markdown_to_html::convert_sanitized("<SCRIPT>evil()</SCRIPT>after").unwrap();
    assert!(!html.to_ascii_lowercase().contains("script"), "{html}");
    assert!(!html.contains("evil()"), "{html}");
    assert!(html.contains("after"), "{html}");

    // Inline raw HTML inside a paragraph is stripped the same way.
    let html = markdown_to_html::convert_sanitized("a <script>x</script> b").unwrap();
    assert!(!html.contains("script"), "{html}");
}

#[cfg(feature = "sanitize")]
#[test]
fn sanitize_strips_dangerous_elements() {
    for tag in [
        "style", "iframe", "object", "embed", "form", "base", "link", "meta",
    ] {
        let md = format!("<{tag} src=\"x\">fallback</{tag}>");
        let html = markdown_to_html::convert_sanitized(&md).unwrap();
        assert!(
            !html.to_ascii_lowercase().contains(&format!("<{tag}")),
            "<{tag}> survived: {html}"
        );
    }
    // Style bodies are code, not text: they must not leak as content.
    let html = markdown_to_html::convert_sanitized("<style>body{color:red}</style>").unwrap();
    assert!(!html.contains("color"), "{html}");
}

#[cfg(feature = "sanitize")]
#[test]
fn sanitize_drops_event_handlers_and_style_attrs() {
    let html =
        markdown_to_html::convert_sanitized("<p onclick=\"alert(1)\" ONMOUSEOVER='x'>hi</p>")
            .unwrap();
    assert!(!html.to_ascii_lowercase().contains("onclick"), "{html}");
    assert!(!html.to_ascii_lowercase().contains("onmouseover"), "{html}");
    assert!(html.contains("hi"), "{html}");

    let html =
        markdown_to_html::convert_sanitized("<p style=\"color:red\" title=\"ok\">hi</p>").unwrap();
    assert!(!html.to_ascii_lowercase().contains("style="), "{html}");
    assert!(html.contains("title=\"ok\""), "{html}");
    assert!(html.contains("hi"), "{html}");
}

#[cfg(feature = "sanitize")]
#[test]
fn sanitize_neutralizes_dangerous_url_schemes() {
    // javascript: is dropped but the link text survives.
    let html = markdown_to_html::convert_sanitized("[x](javascript:alert(1))").unwrap();
    assert!(!html.to_ascii_lowercase().contains("javascript:"), "{html}");
    assert!(!html.contains("alert(1)"), "{html}");
    assert!(html.contains(">x</a>"), "{html}");

    // Case-insensitive …
    let html = markdown_to_html::convert_sanitized("[x](JaVaScRiPt:alert(1))").unwrap();
    assert!(!html.to_ascii_lowercase().contains("javascript:"), "{html}");

    // … entity-decoded first (`&#58;` is `:`) …
    let html = markdown_to_html::convert_sanitized("[x](javascript&#58;alert(1))").unwrap();
    assert!(!html.to_ascii_lowercase().contains("javascript:"), "{html}");
    assert!(!html.contains("alert(1)"), "{html}");

    // … and caught in raw-HTML attributes too.
    let html =
        markdown_to_html::convert_sanitized("<a href=\"javascript:alert(1)\">x</a>").unwrap();
    assert!(!html.to_ascii_lowercase().contains("javascript:"), "{html}");
    assert!(html.contains(">x</a>"), "{html}");

    // vbscript: and data:text/html are blocked as well.
    let html = markdown_to_html::convert_sanitized("[x](vbscript:msgbox(1))").unwrap();
    assert!(!html.to_ascii_lowercase().contains("vbscript:"), "{html}");
    let html = markdown_to_html::convert_sanitized("[x](data:text/html,<h1>hi</h1>)").unwrap();
    assert!(
        !html.to_ascii_lowercase().contains("data:text/html"),
        "{html}"
    );

    // Safe schemes are untouched.
    let html = markdown_to_html::convert_sanitized("[x](https://example.com/a?b=c#d)").unwrap();
    assert!(html.contains("https://example.com/a?b=c#d"), "{html}");
}

#[cfg(feature = "sanitize")]
#[test]
fn sanitize_preserves_safe_markup() {
    let html = markdown_to_html::convert_sanitized(
        "# T\n\n**bold** *em* `code` [ok](https://example.com)\n\n- a\n- b\n",
    )
    .unwrap();
    for needle in [
        "<h1>T</h1>",
        "<strong>bold</strong>",
        "<em>em</em>",
        "<code>code</code>",
        "<a href=\"https://example.com\">ok</a>",
        "<ul>",
        "<li>a</li>",
    ] {
        assert!(html.contains(needle), "lost safe markup {needle}: {html}");
    }

    // GFM tables survive the sanitizer.
    let html =
        markdown_to_html::convert_gfm_sanitized("| a | b |\n|---|---|\n| 1 | 2 |\n").unwrap();
    assert!(html.contains("<table>"), "{html}");
    assert!(html.contains("<td>1</td>"), "{html}");

    // GFM entry point sanitizes too.
    let html = markdown_to_html::convert_gfm_sanitized("<script>x</script>").unwrap();
    assert!(!html.contains("script"), "{html}");
}

#[cfg(feature = "sanitize")]
#[test]
fn sanitize_html_unit_edge_cases() {
    use pagina::sanitize_html;

    // Unquoted + single-quoted handler values are still dropped.
    assert!(!sanitize_html("<p onclick=alert(1)>x</p>").contains("onclick"));
    assert!(!sanitize_html("<p onload='x'>y</p>").contains("onload"));

    // Unquoted dangerous href is dropped.
    let out = sanitize_html("<a href=javascript:alert(1)>x</a>");
    assert!(!out.to_ascii_lowercase().contains("javascript:"), "{out}");

    // data:image (non-HTML) is not in the block list and survives.
    let out = sanitize_html("<img src=\"data:image/png;base64,AAA\">");
    assert!(out.contains("data:image/png"), "{out}");

    // Comments carry no content and are removed; neighbors survive.
    let out = sanitize_html("a<!-- secret -->b");
    assert_eq!(out, "ab");

    // Escaped code spans are text, not tags: left intact.
    let out = sanitize_html("<p><code>&lt;script&gt;</code></p>");
    assert!(out.contains("&lt;script&gt;"), "{out}");
}
