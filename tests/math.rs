//! Dollar math: Pandoc heuristics, display form, reverse, off-mode literal.

use pagina::ast::Inline;

fn gfm(md: &str) -> String {
    pagina::markdown_to_html::convert_gfm(md).unwrap()
}

#[test]
fn inline_math_renders_span() {
    let html = gfm("Einstein: $E=mc^2$.\n");
    assert!(
        html.contains("<span class=\"math-inline\">E=mc^2</span>"),
        "got {html:?}"
    );
}

#[test]
fn currency_traps_stay_literal() {
    for md in ["$5\n", "It costs $5 and $10.\n", "a $5 b\n"] {
        let html = gfm(md);
        assert!(
            !html.contains("math-inline") && !html.contains("math-display"),
            "currency must stay literal for {md:?}, got {html:?}"
        );
    }
}

#[test]
fn inline_heuristics_spaces_and_digit() {
    // Space after opener / before closer stays literal.
    for md in ["$ a$\n", "$a $\n"] {
        let html = gfm(md);
        assert!(
            !html.contains("math-inline"),
            "space heuristic failed for {md:?}, got {html:?}"
        );
    }
    // Digit after closer stays literal.
    let html = gfm("$a$5\n");
    assert!(
        !html.contains("math-inline"),
        "digit-after-close must stay literal, got {html:?}"
    );
}

#[test]
fn display_math_renders_div() {
    let html = gfm("$$x^2$$\n");
    assert!(
        html.contains("<div class=\"math-display\">x^2</div>"),
        "got {html:?}"
    );
}

#[test]
fn off_mode_stays_literal() {
    // Pure CommonMark: dollars never parse without GFM.
    let html = pagina::markdown_to_html::convert("$E=mc^2$ and $$x^2$$\n").unwrap();
    assert!(!html.contains("math-inline") && !html.contains("math-display"));
    assert!(html.contains("$E=mc^2$"));
}

#[test]
fn ast_nodes_and_markdown_round_trip() {
    let doc = pagina::ast::parse("A $x$ plus $$y$$.\n", pagina::Options::gfm());
    let mut found_inline = false;
    let mut found_display = false;
    for block in &doc.blocks {
        if let pagina::ast::Block::Paragraph(inlines) = block {
            for inline in inlines {
                match inline {
                    Inline::MathInline(c) if c == "x" => found_inline = true,
                    Inline::MathDisplay(c) if c == "y" => found_display = true,
                    _ => {}
                }
            }
        }
    }
    assert!(
        found_inline && found_display,
        "AST must hold both math nodes"
    );

    let html = pagina::ast::render_html(&doc);
    assert!(html.contains("<span class=\"math-inline\">x</span>"));
    assert!(html.contains("<div class=\"math-display\">y</div>"));

    let md = pagina::ast::render_markdown(&doc);
    assert!(md.contains("$x$"), "got {md:?}");
    assert!(md.contains("$$y$$"), "got {md:?}");
}

#[test]
fn stream_events_carry_math() {
    let events = pagina::stream::collect_events("A $x$.\n", pagina::Options::gfm());
    assert!(
        events
            .iter()
            .any(|e| matches!(e, pagina::stream::Event::MathInline(c) if c == "x")),
        "stream must carry MathInline"
    );
    let html = pagina::stream::render_events_to_html(&events);
    assert!(html.contains("<span class=\"math-inline\">x</span>"));
}

#[test]
fn reverse_spans_back_to_dollars() {
    let md =
        pagina::html_to_markdown::convert_gfm("<p>A <span class=\"math-inline\">x</span>.</p>")
            .unwrap();
    assert!(md.contains("$x$"), "got {md:?}");

    let md =
        pagina::html_to_markdown::convert_gfm("<div class=\"math-display\">x^2</div>").unwrap();
    assert!(md.contains("$$x^2$$"), "got {md:?}");
}

#[test]
fn reverse_off_mode_unwraps() {
    let md =
        pagina::html_to_markdown::convert("<p><span class=\"math-inline\">x</span></p>").unwrap();
    assert!(!md.contains('$'), "got {md:?}");
    assert!(md.contains('x'));
}
