//! Pull streaming API tests: event shapes, `TagEnd` payload-freedom, and
//! stream-render parity (`render_events_to_html` equals `convert`).
//!
//! - Event sequences cover heading/list/table/footnote/task/strikethrough.
//! - `TagEnd` is asserted payload-free (`size_of == 1`, `Copy`, exhaustive
//!   match) so closers stay one byte wide.
//! - `stream -> html` must equal `convert`/`convert_gfm` on the corpus.

use pagina::ast::Alignment;
use pagina::stream::{
    collect_events, parse_stream, render_events_to_html, Event, Parser, Tag, TagEnd,
};
use pagina::{markdown_to_html, Options};

fn events(input: &str) -> Vec<Event> {
    collect_events(input, Options::default())
}

fn events_gfm(input: &str) -> Vec<Event> {
    collect_events(input, Options::gfm())
}

// ---------------------------------------------------------------------------
// Event sequences
// ---------------------------------------------------------------------------

#[test]
fn heading_event_sequence() {
    assert_eq!(
        events("# Hi *em*\n"),
        vec![
            Event::Start(Tag::Heading { level: 1 }),
            Event::Text("Hi ".to_string()),
            Event::Start(Tag::Emphasis),
            Event::Text("em".to_string()),
            Event::End(TagEnd::Emphasis),
            Event::End(TagEnd::Heading),
        ]
    );
}

#[test]
fn task_list_event_sequence() {
    assert_eq!(
        events_gfm("- [x] done\n"),
        vec![
            Event::Start(Tag::List {
                ordered: false,
                start: 0,
                tight: true,
            }),
            Event::Start(Tag::Item),
            Event::TaskListMarker(true),
            Event::Start(Tag::Paragraph),
            Event::Text("done".to_string()),
            Event::End(TagEnd::Paragraph),
            Event::End(TagEnd::Item),
            Event::End(TagEnd::List),
        ]
    );
}

#[test]
fn table_event_sequence() {
    let found = events("| a | b |\n| --- | :-: |\n| 1 | 2 |\n");
    assert!(matches!(
        found.first(),
        Some(Event::Start(Tag::Table { alignments }))
            if *alignments == vec![Alignment::None, Alignment::Center]
    ));
    assert!(found.contains(&Event::Start(Tag::TableHead)));
    assert!(found.contains(&Event::End(TagEnd::TableHead)));
    assert!(found.contains(&Event::Start(Tag::TableRow)));
    assert!(found.contains(&Event::End(TagEnd::TableRow)));
    assert_eq!(
        found
            .iter()
            .filter(|e| matches!(e, Event::Start(Tag::TableCell)))
            .count(),
        4
    );
    assert!(matches!(found.last(), Some(Event::End(TagEnd::Table))));
}

#[test]
fn footnote_event_sequence() {
    let found = events_gfm("Hello[^a]\n\n[^a]: note\n");
    assert!(found.contains(&Event::FootnoteReference {
        label: "a".to_string(),
        number: 1,
    }));
    assert!(found.contains(&Event::Start(Tag::FootnoteDefinition {
        label: "a".to_string(),
        number: 1,
    })));
    assert!(found.contains(&Event::End(TagEnd::FootnoteDefinition)));
}

#[test]
fn strikethrough_event_sequence() {
    assert_eq!(
        events_gfm("~~hi~~\n"),
        vec![
            Event::Start(Tag::Paragraph),
            Event::Start(Tag::Strikethrough),
            Event::Text("hi".to_string()),
            Event::End(TagEnd::Strikethrough),
            Event::End(TagEnd::Paragraph),
        ]
    );
}

#[test]
fn parser_is_a_pull_iterator() {
    let mut parser = parse_stream("- a\n- b\n", Options::default());
    assert_eq!(parser.len(), 12);
    assert!(!parser.is_empty());
    let first = parser.next();
    assert!(matches!(first, Some(Event::Start(Tag::List { .. }))));
    let rest: Vec<Event> = parser.collect();
    assert_eq!(rest.len(), 11);
    assert!(matches!(rest.last(), Some(Event::End(TagEnd::List))));
}

#[test]
fn parser_matches_collected_events() {
    let via_iter: Vec<Event> = parse_stream("# T\n", Options::default()).collect();
    assert_eq!(via_iter, events("# T\n"));
}

// ---------------------------------------------------------------------------
// TagEnd payload-freedom
// ---------------------------------------------------------------------------

fn assert_copy<T: Copy>() {}

#[test]
fn tag_end_is_payload_free() {
    assert_copy::<TagEnd>();
    assert_eq!(std::mem::size_of::<TagEnd>(), 1);
    // Exhaustive match: fails to compile if a variant gains a payload.
    fn name(end: TagEnd) -> &'static str {
        match end {
            TagEnd::Paragraph => "paragraph",
            TagEnd::Heading => "heading",
            TagEnd::BlockQuote => "blockquote",
            TagEnd::CodeBlock => "codeblock",
            TagEnd::HtmlBlock => "htmlblock",
            TagEnd::List => "list",
            TagEnd::Item => "item",
            TagEnd::Table => "table",
            TagEnd::TableHead => "tablehead",
            TagEnd::TableRow => "tablerow",
            TagEnd::TableCell => "tablecell",
            TagEnd::FootnoteDefinition => "footnote",
            TagEnd::DefinitionList => "deflist",
            TagEnd::DefinitionListItem => "defitem",
            TagEnd::DefinitionTerm => "defterm",
            TagEnd::DefinitionDescription => "defdesc",
            TagEnd::Emphasis => "em",
            TagEnd::Strong => "strong",
            TagEnd::Strikethrough => "del",
            TagEnd::Link => "link",
            TagEnd::Image => "image",
        }
    }
    assert_eq!(name(TagEnd::TableCell), "tablecell");
    assert_eq!(name(TagEnd::Image), "image");
}

#[test]
fn events_are_thread_safe() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Event>();
    assert_send_sync::<Tag>();
    assert_send_sync::<TagEnd>();
    assert_send_sync::<Parser>();
}

// ---------------------------------------------------------------------------
// Stream-render parity: stream -> html equals convert
// ---------------------------------------------------------------------------

const STREAM_PARITY_CORPUS: &[&str] = &[
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
    "- empty\n-\n",
    "```rust\nlet x = 1;\n```\n",
    "    indented\n",
    "***\n",
    "- - -\n",
    "[link](/url \"title\") and ![img](/i.png)\n",
    "[foo]: /url \"title\"\n\n[foo] and [t][foo] and [foo][]\n",
    "<http://example.com> and <a@b.com>\n",
    "<div>\nfoo\n</div>\n",
    "| a | b |\n| --- | :-: |\n| 1 | 2 |\n",
    "| a |\n| --- |\n",
    "para one\n\npara two\n",
    "a  \nb\n",
    "a\nb\n",
    "---\ntitle: Hi\n---\n# Hi\n",
    "# H1\n\n> quote with [link](/u)\n\n- item *em*\n\n```\ncode\n```\n",
];

const STREAM_PARITY_GFM_CORPUS: &[&str] = &[
    "- [ ] todo\n- [x] done\n- plain\n",
    "- [ ] a\n\n- [x] b\n",
    "- [x] **done** with `code`\n",
    "~~strike~~ and **bold**\n",
    "Visit http://example.com and www.example.com/x and a@b.com\n",
    "Hello[^a] and again[^a]\n\n[^a]: the note\n",
    "Single[^u]\n\n[^u]: alone\n",
    "Unreferenced[^z] stays literal\n",
    "Term\n: description\n",
    "Term\n: para one\n\n  para two\n",
    "# GFM *doc* with ~~strike~~\n\n- [ ] item\n",
    "> - [x] quoted task\n",
    "| *em* | `code` |\n| --- | --- |\n| [l](/u) | ![i](/i.png) |\n",
    "- item with [^fn] footnote\n\n[^fn]: fn body\n",
];

#[test]
fn stream_html_matches_convert() {
    for md in STREAM_PARITY_CORPUS {
        let expected = markdown_to_html::convert(md).expect("convert failed");
        let actual = render_events_to_html(&events(md));
        assert_eq!(actual, expected, "HTML mismatch for {:?}", md);
    }
}

#[test]
fn stream_html_matches_convert_gfm() {
    for md in STREAM_PARITY_GFM_CORPUS {
        let expected = markdown_to_html::convert_gfm(md).expect("convert_gfm failed");
        let actual = render_events_to_html(&events_gfm(md));
        assert_eq!(actual, expected, "GFM HTML mismatch for {:?}", md);
    }
}

#[test]
fn frontmatter_produces_no_events() {
    // Frontmatter is skipped by design (metadata, never HTML); the stream
    // equals the body-only stream.
    assert_eq!(events("---\ntitle: Hi\n---\n# Hi\n"), events("# Hi\n"));
}
