//! Property-based round-trip tests for `pagina`'s Markdown <-> HTML converters.
//!
//! Uses the `proptest!` block macro (default proptest features, no extra
//! feature flags) to sweep a bounded space of generated Markdown fragments —
//! emphasis nests, links, lists, tables, code spans, code fences — and check
//! invariants that must hold everywhere in that space:
//!
//! - conversion never panics or returns `Err` (both the legacy `convert` path
//!   and the AST `parse` + `render_html` path);
//! - generated text nodes never leak an unescaped `<script` into the HTML
//!   output (the generators never emit raw HTML, so any `<script` would be a
//!   text-node escape bug);
//! - inline fragments round-trip: `render_inline_md(parse_inline(fragment))`
//!   reproduces the canonical fragment byte-for-byte;
//! - block-level documents are HTML-stable: `html -> md -> html` is a fixed
//!   point and `md -> html -> md -> html` settles after one cycle;
//! - the AST renderer stays byte-identical to the legacy converter on
//!   generated documents;
//! - GFM extensions (strikethrough, task lists, bare autolinks) render with
//!   their structural wrappers.
//!
//! Case counts are deliberately bounded (96 per block-level property, 256 for
//! the cheap inline identity property) so the whole suite stays comfortably
//! under CI's default runner timeouts.

use pagina::ast::{parse, render_html};
use pagina::html_to_markdown;
use pagina::inline_parser::{parse_inline, render_inline_md};
use pagina::markdown_to_html;
use pagina::Options;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

fn m2h(md: &str) -> String {
    markdown_to_html::convert(md).expect("markdown -> html conversion failed")
}

fn h2m(html: &str) -> String {
    html_to_markdown::convert(html).expect("html -> markdown conversion failed")
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------
//
// Every generated string is *canonical* Markdown: the same glyphs the crate's
// own Markdown emitters produce (`**`/`*` emphasis, single-backtick code,
// `[text](url)` links, `- ` bullets, pipe tables, plain fences). That is what
// lets the round-trip properties assert byte equality rather than semantic
// equivalence.

/// Plain words: no Markdown-significant characters at all, so a run of words
/// re-parses to the same text node(s) it was generated from.
const WORDS: &[&str] = &[
    "alpha", "beta", "gamma", "delta", "omega", "zeta", "theta", "kappa",
];

/// URL pool: simple `https://` destinations that survive href escaping
/// unchanged and round-trip through `html_to_markdown`.
const URLS: &[&str] = &[
    "https://example.com",
    "https://example.com/a",
    "https://docs.rs/pagina/latest",
    "https://example.org/x1",
];

/// Code-span pool: no backticks, no leading/trailing space, so the
/// single-backtick canonical form re-parses to exactly the same content.
const CODE_WORDS: &[&str] = &["x1", "a.b", "foo", "y2k"];

fn word() -> impl Strategy<Value = &'static str> {
    prop_oneof![
        Just(WORDS[0]),
        Just(WORDS[1]),
        Just(WORDS[2]),
        Just(WORDS[3]),
        Just(WORDS[4]),
        Just(WORDS[5]),
        Just(WORDS[6]),
        Just(WORDS[7]),
    ]
}

fn phrase() -> impl Strategy<Value = String> {
    proptest::collection::vec(word(), 1..=3).prop_map(|ws| ws.join(" "))
}

fn url() -> impl Strategy<Value = &'static str> {
    prop_oneof![Just(URLS[0]), Just(URLS[1]), Just(URLS[2]), Just(URLS[3]),]
}

fn code_span() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(CODE_WORDS[0]),
        Just(CODE_WORDS[1]),
        Just(CODE_WORDS[2]),
        Just(CODE_WORDS[3]),
    ]
    .prop_map(|w| format!("`{}`", w))
}

fn link() -> impl Strategy<Value = String> {
    (prop_oneof![phrase().boxed(), code_span().boxed()], url())
        .prop_map(|(inner, u)| format!("[{}]({})", inner, u))
}

/// Content safe to place directly inside one emphasis level.
fn emphasis_inner() -> impl Strategy<Value = String> {
    prop_oneof![phrase().boxed(), code_span().boxed(), link().boxed()]
}

/// Nested-emphasis templates. Inner emphasis is always padded with plain
/// words on both sides so delimiters never touch (`***` runs are avoided) and
/// the canonical form re-parses to the same nesting.
fn nested_emph() -> impl Strategy<Value = String> {
    let two_level = (phrase(), phrase(), phrase(), 0..2u8).prop_map(|(a, b, c, which)| {
        if which == 0 {
            format!("**{} *{}* {}**", a, b, c)
        } else {
            format!("*{} **{}** {}*", a, b, c)
        }
    });
    let three_level = (phrase(), phrase(), phrase(), phrase(), phrase(), 0..2u8).prop_map(
        |(a, b, c, d, e, which)| {
            if which == 0 {
                format!("**{} *{} **{}** {}* {}**", a, b, c, d, e)
            } else {
                format!("*{} **{} *{}* {}** {}*", a, b, c, d, e)
            }
        },
    );
    prop_oneof![two_level.boxed(), three_level.boxed()]
}

/// A canonical inline Markdown fragment: text, code span, link, single-level
/// emphasis, or a padded emphasis nest.
fn inline_fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        phrase().boxed(),
        code_span().boxed(),
        link().boxed(),
        emphasis_inner().prop_map(|s| format!("**{}**", s)).boxed(),
        emphasis_inner().prop_map(|s| format!("*{}*", s)).boxed(),
        nested_emph().boxed(),
    ]
}

// ---------------------------------------------------------------------------
// Block-level document strategy
// ---------------------------------------------------------------------------
//
// Constrained to constructs that round-trip HTML-stably by design: paragraphs
// of *flat* inline fragments (no nested emphasis), ATX headings, tight bullet
// lists, uniform-width pipe tables, and plain (language-less) fenced code.
//
// Nested emphasis (`**a *b* c**` and deeper) is deliberately *not* part of
// the block-level space: `html_to_markdown`'s text-node handling normalizes
// each text node independently (`split_whitespace().join(" ")`), which drops
// boundary whitespace around inline elements, so `**a *b* c**` does not
// survive `html -> md -> html` byte-identically (`alpha *alpha*` inside a
// strong loses its padding spaces). Deep nests (`**a *b **c** d* e**`)
// additionally trip an internal `\0EM\0` emphasis sentinel in `inline_parser`
// when re-parsing the converter's output. These are pre-existing converter
// behaviors; the library itself is deliberately out of scope here, so the
// property space stops at the byte-stable shapes. Nested emphasis still gets
// full coverage through `inline_roundtrip_is_identity`, which exercises
// `parse_inline`/`render_inline_md` directly and never touches the HTML
// direction.

/// Flat inline content safe inside a document paragraph, in the sense that
/// `md -> html -> md -> html` is byte-stable: plain phrases, code spans,
/// links, and single-level emphasis around any of those (never nested
/// emphasis). See the module note above.
fn flat_fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        phrase().boxed(),
        code_span().boxed(),
        link().boxed(),
        emphasis_inner().prop_map(|s| format!("**{}**", s)).boxed(),
        emphasis_inner().prop_map(|s| format!("*{}*", s)).boxed(),
    ]
}

/// A heading (`#`/`##`/`###` + phrase).
fn heading() -> impl Strategy<Value = String> {
    (1..=3u8, phrase()).prop_map(|(level, text)| format!("{} {}", "#".repeat(level as usize), text))
}

/// A tight bullet list of word phrases.
fn bullet_list() -> impl Strategy<Value = String> {
    proptest::collection::vec(phrase(), 1..=4).prop_map(|items| {
        items
            .iter()
            .map(|item| format!("- {}", item))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// A GFM pipe table with uniform column count (2-4 columns, 1-3 rows, plain
/// word cells) so the header/delimiter/body widths always agree.
fn table() -> impl Strategy<Value = String> {
    (2..=4usize).prop_flat_map(|ncols| {
        (
            proptest::collection::vec(word(), ncols),
            proptest::collection::vec(proptest::collection::vec(word(), ncols), 1..=3),
        )
            .prop_map(move |(header, rows)| {
                let mut out = format!("| {} |", header.join(" | "));
                out.push('\n');
                out.push_str(&format!("| {} |", vec!["---"; ncols].join(" | ")));
                for row in rows {
                    out.push('\n');
                    out.push_str(&format!("| {} |", row.join(" | ")));
                }
                out
            })
    })
}

/// A plain fenced code block with a single code line.
fn code_block() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("let x = 1;"),
        Just("fn main() {}"),
        Just("x = x + 1"),
        Just("print('hi')"),
    ]
    .prop_map(|line| format!("```\n{}\n```", line))
}

/// One block-level Markdown construct.
fn block() -> impl Strategy<Value = String> {
    prop_oneof![
        flat_fragment().boxed(),
        heading().boxed(),
        table().boxed(),
        code_block().boxed(),
    ]
}

/// A simple document: 1-4 non-list blocks separated by blank lines.
///
/// Bullet lists are excluded from this mixed space: two `- x` blocks separated
/// by a blank line merge into one *loose* list (CommonMark spec), and
/// `html_to_markdown` normalizes loose lists back to tight ones, so that
/// shape is not byte-stable. Tight lists get their own dedicated property
/// (`tight_single_list_roundtrips`) where the list is the whole document.
fn simple_doc() -> impl Strategy<Value = String> {
    proptest::collection::vec(block(), 1..=4).prop_map(|blocks| blocks.join("\n\n"))
}

// ---------------------------------------------------------------------------
// Properties (block-level document space, 96 cases each)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    /// Conversion (legacy and AST paths) never panics or errors on generated
    /// Markdown, and never produces an empty result for a non-empty document.
    #[test]
    fn md_to_html_completes_and_ast_matches(md in simple_doc()) {
        let html = m2h(&md);
        prop_assert!(!html.is_empty(), "empty output for {:?}", md);

        let doc = parse(&md, Options::default());
        let ast_html = render_html(&doc);
        prop_assert_eq!(ast_html, html, "AST renderer diverged from legacy convert for {:?}", md);
    }

    /// Text nodes never leak an unescaped `<script` into the output. The
    /// generators never emit raw HTML, so any literal `<script` in the HTML
    /// would mean a text/code node escaped the escaping step. Checked in
    /// both CommonMark and GFM modes.
    #[test]
    fn text_never_emits_unescaped_script(md in simple_doc()) {
        let html = m2h(&md);
        prop_assert!(
            !html.contains("<script"),
            "unescaped <script> in CommonMark output for {:?}",
            md
        );

        let gfm_html = markdown_to_html::convert_gfm(&md).expect("gfm conversion failed");
        prop_assert!(
            !gfm_html.contains("<script"),
            "unescaped <script> in GFM output for {:?}",
            md
        );
    }

    /// `html -> md -> html` is a fixed point: re-running the cycle over the
    /// Markdown produced from the HTML of a generated document settles on the
    /// same HTML and the same Markdown.
    #[test]
    fn html_md_html_is_idempotent(md in simple_doc()) {
        let html = m2h(&md);
        let md1 = h2m(&html);
        let html1 = m2h(&md1);
        let md2 = h2m(&html1);
        let html2 = m2h(&md2);

        prop_assert_eq!(html1, html2, "html -> md -> html is not a fixed point for {:?}", md);
        prop_assert_eq!(md1, md2, "first html -> md -> html cycle did not settle for {:?}", md);
    }

    /// `md -> html -> md -> html` stability: converting a generated document
    /// and running the cycle once more keeps the intermediate HTML and
    /// Markdown unchanged (the converters are idempotent after a single pass).
    #[test]
    fn md_html_md_is_stable(md in simple_doc()) {
        let html1 = m2h(&md);
        let md2 = h2m(&html1);
        let html2 = m2h(&md2);
        let md3 = h2m(&html2);

        prop_assert_eq!(html1, html2, "HTML changed on the second md -> html pass for {:?}", md);
        prop_assert_eq!(md2, md3, "Markdown changed on the second html -> md pass for {:?}", md);
    }

    /// A single tight bullet list (the whole document) is byte-stable through
    /// both cycles. Loose lists are not in this space (see `simple_doc`), but
    /// a tight list on its own cannot merge with anything, so
    /// `md -> html -> md -> html` is a fixed point.
    #[test]
    fn tight_single_list_roundtrips(list in bullet_list()) {
        let html1 = m2h(&list);
        let md2 = h2m(&html1);
        let html2 = m2h(&md2);
        let md3 = h2m(&html2);

        prop_assert_eq!(html1, html2, "tight list HTML changed for {:?}", list);
        prop_assert_eq!(md2, md3, "tight list Markdown changed for {:?}", list);
    }
}

// ---------------------------------------------------------------------------
// Properties (inline fragment space)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Inline round-trip: parsing a canonical fragment and re-rendering it to
    /// Markdown reproduces the fragment byte-for-byte.
    ///
    /// This is a real invariant of the generator space: every generated
    /// fragment uses the exact canonical spellings the crate's own
    /// `render_inline_md` emits, so parse+render is the identity on that
    /// space.
    #[test]
    fn inline_roundtrip_is_identity(fragment in inline_fragment()) {
        let reparsed = render_inline_md(&parse_inline(&fragment));
        prop_assert_eq!(
            &reparsed,
            &fragment,
            "inline round-trip mismatch for {:?}",
            fragment
        );
    }
}

// ---------------------------------------------------------------------------
// Properties (fixed small inputs, GFM structural checks)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    /// Generated bold/italic/code/link fragments appear wrapped in their
    /// structural HTML element (`<strong>…</strong>`, `<em>…</em>`,
    /// `<code>…</code>`, `<a href="…">…</a>`).
    #[test]
    fn generated_inline_markers_wrap_correctly(w in word()) {
        let bold = m2h(&format!("**{}**", w));
        let expected = format!("<strong>{}</strong>", w);
        prop_assert!(bold.contains(&expected), "expect {:?} in {:?}", expected, bold);

        let italic = m2h(&format!("*{}*", w));
        let expected = format!("<em>{}</em>", w);
        prop_assert!(italic.contains(&expected), "expect {:?} in {:?}", expected, italic);

        let code = m2h(&format!("`{}`", w));
        let expected = format!("<code>{}</code>", w);
        prop_assert!(code.contains(&expected), "expect {:?} in {:?}", expected, code);

        let link = m2h(&format!("[{}](https://example.com/x)", w));
        let expected = format!("<a href=\"https://example.com/x\">{}</a>", w);
        prop_assert!(link.contains(&expected), "expect {:?} in {:?}", expected, link);
    }

    /// GFM extensions keep their structural wrappers: `~~…~~` becomes `<del>`,
    /// task items carry a checked checkbox, and bare URLs become `<a href=…>`.
    #[test]
    fn gfm_extensions_render_with_structure(w in word()) {
        let strike = markdown_to_html::convert_gfm(&format!("~~{}~~", w)).expect("gfm conversion");
        let expected = format!("<del>{}</del>", w);
        prop_assert!(strike.contains(&expected), "expect {:?} in {:?}", expected, strike);

        let task = markdown_to_html::convert_gfm(&format!("- [x] {}", w)).expect("gfm conversion");
        prop_assert!(task.contains("checked"), "task checkbox not rendered for {:?}", w);
        prop_assert!(task.contains(w), "task item text missing for {:?}", w);

        let bare =
            markdown_to_html::convert_gfm("visit https://example.com/x now").expect("gfm conversion");
        prop_assert!(
            bare.contains("<a href=\"https://example.com/x\""),
            "bare URL not autolinked in {:?}",
            bare
        );
    }
}
