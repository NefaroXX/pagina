//! Syntax-highlight hook: trait mock, default-off identity, info split.

use pagina::highlight::{language_from_info, SyntaxHighlighter};
use pagina::markdown_to_html::{convert_with, Options};

struct Mock {
    seen: std::sync::Mutex<Vec<(String, String)>>,
}

impl Mock {
    fn new() -> Self {
        Mock {
            seen: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl SyntaxHighlighter for Mock {
    fn write_highlighted(&self, out: &mut String, lang: &str, code: &str) {
        self.seen
            .lock()
            .unwrap()
            .push((lang.to_string(), code.to_string()));
        out.push_str(&format!("<hl lang=\"{lang}\">{code}</hl>"));
    }
}

#[test]
fn info_split_takes_comma_prefix() {
    assert_eq!(language_from_info("rust,ignore"), "rust");
    assert_eq!(language_from_info("rust,no_run extra"), "rust");
    assert_eq!(language_from_info("python"), "python");
    assert_eq!(language_from_info(""), "");
    assert_eq!(language_from_info(";"), ";");
}

#[test]
fn default_off_is_byte_identical() {
    let inputs = [
        "```rust\nlet x = 1;\n```\n",
        "```rust,ignore\nlet x = 1;\n```\n",
        "```\nplain & <code>\n```\n",
        "    indented & <code>\n",
    ];
    for md in inputs {
        let a = convert_with(md, Options::default()).unwrap();
        let b = pagina::markdown_to_html::convert_with_highlighter(md, Options::default(), None)
            .unwrap();
        assert_eq!(a, b, "highlighter=None must match convert_with for {md:?}");

        let doc = pagina::ast::parse(md, pagina::Options::default());
        assert_eq!(
            pagina::ast::render_html(&doc),
            pagina::ast::render_html_with_highlighter(&doc, None),
            "AST None must match render_html for {md:?}"
        );

        let events = pagina::stream::collect_events(md, pagina::Options::default());
        assert_eq!(
            pagina::stream::render_events_to_html(&events),
            pagina::stream::render_events_to_html_with_highlighter(&events, None),
            "stream None must match for {md:?}"
        );
    }
}

#[test]
fn mock_highlighter_receives_split_lang_and_code() {
    let mock = Mock::new();
    let html = pagina::markdown_to_html::convert_with_highlighter(
        "```rust,ignore\nlet x = 1;\n```\n",
        Options::default(),
        Some(&mock),
    )
    .unwrap();
    // Wrapper unchanged, content from the hook.
    assert!(html.contains("<pre><code class=\"language-rust\">"));
    assert!(html.contains("<hl lang=\"rust\">"));
    assert!(!html.contains("rust,ignore"));
    let seen = mock.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, "rust");
    assert_eq!(seen[0].1, "let x = 1;\n");
}

#[test]
fn mock_highlighter_ast_and_stream_paths() {
    let mock = Mock::new();
    let doc = pagina::ast::parse("```py\nx = 1\n```\n", pagina::Options::default());
    let html = pagina::ast::render_html_with_highlighter(&doc, Some(&mock));
    assert!(html.contains("class=\"language-py\""));
    assert!(html.contains("<hl lang=\"py\">"));

    let mock2 = Mock::new();
    let events = pagina::stream::collect_events("```py\nx = 1\n```\n", pagina::Options::default());
    let html2 = pagina::stream::render_events_to_html_with_highlighter(&events, Some(&mock2));
    assert!(html2.contains("class=\"language-py\""));
    assert!(html2.contains("<hl lang=\"py\">"));
}

#[test]
#[cfg(feature = "syntax-highlight")]
fn syntect_adapter_class_spans_and_fallback() {
    use pagina::highlight::SyntectAdapter;

    let ss = syntect::parsing::SyntaxSet::load_defaults_newlines();
    let adapter = SyntectAdapter::new(&ss);
    // Known language: class-based spans.
    let html = pagina::markdown_to_html::convert_with_highlighter(
        "```rust\nfn main() {}\n```\n",
        Options::default(),
        Some(&adapter),
    )
    .unwrap();
    assert!(html.contains("class=\"language-rust\""));
    assert!(html.contains("<span class=\""));
    // Unknown language: escaped fallback, wrapper intact.
    let html = pagina::markdown_to_html::convert_with_highlighter(
        "```nosuchlang_xyz\n<a>&\n```\n",
        Options::default(),
        Some(&adapter),
    )
    .unwrap();
    assert!(html.contains("class=\"language-nosuchlang_xyz\""));
    assert!(html.contains("&lt;a&gt;&amp;"));
}
