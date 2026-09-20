//! Criterion benchmarks for `pagina`'s bidirectional Markdown <-> HTML
//! converters.
//!
//! Covers Markdown -> HTML and HTML -> Markdown on small/medium/large inputs,
//! plus one GFM-heavy input through both directions. The medium and large
//! inputs are built from the embedded CommonMark spec corpus
//! (`tests/fixtures/spec.txt`, the same fixture the compliance runner uses),
//! so the benchmark inputs are real-world-ish Markdown rather than synthetic
//! strings.
//!
//! Run with:
//!
//! ```text
//! cargo bench --bench convert
//! ```
//!
//! Smoke / list mode:
//!
//! ```text
//! cargo bench --bench convert -- --list
//! cargo bench --bench convert -- --quick
//! ```

use criterion::{criterion_group, criterion_main, Criterion};
use pagina::html_to_markdown;
use pagina::markdown_to_html;
use std::hint::black_box;

/// Embedded CommonMark 0.31.2 spec (see module docs).
const SPEC: &str = include_str!("../tests/fixtures/spec.txt");

/// Small hand-written Markdown fixture: inline emphasis, links, code, a list.
const SMALL: &str = "# Heading\n\nSome **bold** and *italic* text with a [link](https://example.com)\nand `inline code`.\n\n- item one\n- item two\n";

/// GFM-heavy Markdown fixture: task lists, strikethrough, a pipe table,
/// footnotes, a definition list, dollar math, and bare autolinks.
const GFM_HEAVY: &str = r#"# GFM Heavy

A paragraph with **bold**, *italic*, ~~strikethrough~~, `code`, and a [link](https://example.com).

- [x] done task
- [ ] open task

| col a | col b |
| :-- | --: |
| x | y |

A reference[^ref].

[^ref]: Footnote *body* here.

term
: definition one
: definition two

Inline math $x^2$ and display math:

$$
\int_0^1 x\,dx
$$

Bare autolinks: https://example.com/path and an email a@b.com.
"#;

/// A spec-derived corpus: medium and large Markdown documents.
struct Corpus {
    medium: String,
    large: String,
}

/// Extract the Markdown side of every spec example (```` … example` blocks).
fn spec_markdown_examples() -> Vec<String> {
    let lines: Vec<&str> = SPEC.split('\n').collect();
    let mut examples: Vec<String> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let fence_len = lines[i]
            .strip_suffix(" example")
            .filter(|body| body.len() >= 20 && body.chars().all(|c| c == '`'))
            .map(|body| body.len());
        if let Some(len) = fence_len {
            i += 1;
            let mut md: Vec<&str> = Vec::new();
            while i < lines.len() && lines[i] != "." {
                md.push(lines[i]);
                i += 1;
            }
            if i < lines.len() && lines[i] == "." {
                i += 1;
            }
            // Skip the expected HTML up to the closing fence.
            while i < lines.len() && !(lines[i].len() == len && lines[i].chars().all(|c| c == '`'))
            {
                i += 1;
            }
            // Consume the closing fence; a malformed spec file terminates the
            // scan rather than indexing past the end.
            if i < lines.len() {
                i += 1;
            } else {
                break;
            }
            examples.push(md.join("\n").replace('→', "\t"));
        } else {
            // Section headers (`## …`) and example markers alike are skipped.
            i += 1;
        }
    }
    examples
}

/// Build the medium (first 64 examples) and large (all examples) corpora.
fn corpus() -> Corpus {
    let examples = spec_markdown_examples();
    Corpus {
        medium: examples
            .iter()
            .take(64)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n\n"),
        large: examples.join("\n\n"),
    }
}

fn bench_markdown_to_html(c: &mut Criterion) {
    let corpus = corpus();
    let mut group = c.benchmark_group("markdown_to_html");
    group.bench_function("small", |b| {
        b.iter(|| markdown_to_html::convert(black_box(SMALL)))
    });
    group.bench_function("medium", |b| {
        b.iter(|| markdown_to_html::convert(black_box(&corpus.medium)))
    });
    group.bench_function("large", |b| {
        b.iter(|| markdown_to_html::convert(black_box(&corpus.large)))
    });
    group.bench_function("gfm_heavy", |b| {
        b.iter(|| markdown_to_html::convert_gfm(black_box(GFM_HEAVY)))
    });
    group.finish();
}

fn bench_html_to_markdown(c: &mut Criterion) {
    let corpus = corpus();
    let medium_html = markdown_to_html::convert(&corpus.medium).expect("fixture conversion");
    let large_html = markdown_to_html::convert(&corpus.large).expect("fixture conversion");
    let gfm_html = markdown_to_html::convert_gfm(GFM_HEAVY).expect("fixture conversion");

    let mut group = c.benchmark_group("html_to_markdown");
    group.bench_function("medium", |b| {
        b.iter(|| html_to_markdown::convert(black_box(&medium_html)))
    });
    group.bench_function("large", |b| {
        b.iter(|| html_to_markdown::convert(black_box(&large_html)))
    });
    group.bench_function("gfm_heavy", |b| {
        b.iter(|| html_to_markdown::convert_gfm(black_box(&gfm_html)))
    });
    group.finish();
}

criterion_group!(benches, bench_markdown_to_html, bench_html_to_markdown);
criterion_main!(benches);
