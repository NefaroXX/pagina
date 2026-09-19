//! CommonMark spec compliance runner for `pagina`'s Markdown → HTML converter.
//!
//! This integration test measures how closely [`pagina::markdown_to_html::convert`]
//! conforms to the CommonMark 0.31.2 spec (<https://spec.commonmark.org/0.31.2/>).
//!
//! The full compliance run is gated behind `#[ignore]` because it is a
//! measurement/audit tool rather than a fast unit test. Run it with:
//!
//! ```text
//! cargo test --test commonmark_compliance -- --ignored --nocapture
//! ```
//!
//! The spec file lives at `tests/fixtures/spec.txt` (fetched verbatim from
//! <https://spec.commonmark.org/0.31.2/spec.txt>, CC-BY-SA 4.0) and is embedded
//! at compile time with `include_str!`, so the runner has zero runtime/network
//! dependencies and is reproducible offline. It also keeps the crate's "zero
//! dependencies" property: everything here uses `std` only.
//!
//! # spec.txt format
//!
//! Every test case looks like this (fenced with 32 backticks):
//!
//! ```text
//! ```````````````````````````````` example
//! <markdown input>
//! .
//! <expected HTML output>
//! .
//! (optional note lines, e.g. "→ indicates a tab character.")
//! ````````````````````````````````
//! ```
//!
//! A line consisting of a single `.` separates the markdown section from the
//! expected-HTML section. Inside both sections the `→` character (U+2192)
//! represents a tab and is replaced with `\t` before conversion/comparison
//! (this mirrors the official `spec_tests.py` harness).
//!
//! # Normalization
//!
//! HTML is normalized before comparison to avoid *false* failures caused by
//! renderer formatting choices rather than content differences:
//!
//! - Line endings are normalized to `\n`.
//! - Trailing whitespace per line is trimmed and blank-line runs are collapsed
//!   outside `<pre>` blocks. Inside `<pre>` blocks lines are kept verbatim,
//!   because code indentation and blank lines are semantically meaningful.
//! - Attributes inside tags are sorted alphabetically.
//! - Void-element self-closing slashes are made consistent (`<hr />` ⇔ `<hr>`).
//! - The five core entities are normalized to one canonical spelling each
//!   (`&#38;`/`&#x26;`/`&AMP;` → `&amp;`, `&apos;`/`&#x27;` → `&#39;`, ...).
//!
//! Normalization deliberately does *not* collapse spaces inside text or code
//! spans, so genuine content differences still count as failures.

use std::collections::HashMap;

/// Embedded CommonMark 0.31.2 spec (see module docs).
const SPEC: &str = include_str!("fixtures/spec.txt");

/// A single parsed spec example.
#[derive(Debug, Clone)]
struct SpecExample {
    number: usize,
    section: String,
    markdown: String,
    html: String,
}

struct CaseResult {
    example: SpecExample,
    passed: bool,
    actual: String,
    expected: String,
    error: Option<String>,
    panicked: bool,
}

// -------------------------------------------------------------------------
// spec.txt parsing
// -------------------------------------------------------------------------

/// If `line` is an example opener (a run of backticks followed by " example"),
/// return the backtick run.
///
/// A minimum run length of 20 distinguishes spec example fences (32 backticks)
/// from 3-backtick code fences shown in prose.
fn example_fence_len(line: &str) -> Option<&str> {
    let body = line.strip_suffix(" example")?;
    if body.len() < 20 || !body.chars().all(|c| c == '`') {
        return None;
    }
    Some(body)
}

/// Check if a line is a closing fence (just backticks, same length as opener).
fn is_closing_fence(line: &str, expected_len: usize) -> bool {
    line.len() == expected_len && line.chars().all(|c| c == '`')
}

/// Parse the `spec.txt` example-fence format into individual cases.
///
/// Returns examples in document order, each tagged with its containing `##`
/// section (used to categorize failures). Section headings that appear as
/// *content* inside examples (e.g. ATX-heading test cases like `## foo`) are
/// never mistaken for headings because example lines are consumed here.
fn parse_spec(spec: &str) -> Vec<SpecExample> {
    let lines: Vec<&str> = spec.split('\n').collect();
    let mut examples: Vec<SpecExample> = Vec::new();
    let mut section = String::from("(top)");
    let mut i = 0;

    while i < lines.len() {
        if let Some(fence) = example_fence_len(lines[i]) {
            let fence_len = fence.len();
            i += 1;

            // Markdown section: everything up to a lone "." line.
            let mut md: Vec<&str> = Vec::new();
            while i < lines.len() && lines[i] != "." {
                md.push(lines[i]);
                i += 1;
            }
            // Check if we hit the dot separator (not closing fence)
            if i < lines.len() && lines[i] == "." {
                i += 1; // skip the "." separator between markdown and HTML.
            }

            // Expected-HTML section: everything up to the closing fence.
            let mut html: Vec<&str> = Vec::new();
            while i < lines.len() && !is_closing_fence(lines[i], fence_len) {
                html.push(lines[i]);
                i += 1;
            }
            if i < lines.len() {
                i += 1; // skip the closing fence
            }

            examples.push(SpecExample {
                number: examples.len() + 1,
                section: section.clone(),
                markdown: md.join("\n").replace('→', "\t"),
                html: html.join("\n").replace('→', "\t"),
            });
        } else if let Some(rest) = lines[i].strip_prefix("## ") {
            if !rest.trim().is_empty() {
                section = rest.trim().to_string();
            }
            i += 1;
        } else {
            i += 1;
        }
    }

    examples
}

// -------------------------------------------------------------------------
// HTML normalization
// -------------------------------------------------------------------------

/// Normalize HTML output for comparison. See module docs for details.
fn normalize_html(html: &str) -> String {
    let lf = html.replace("\r\n", "\n").replace('\r', "\n");
    let whitespace = normalize_whitespace(&lf);
    let tags = normalize_tags(&whitespace);
    normalize_entities(&tags)
}

/// Line-ending + whitespace normalization.
///
/// - Trailing whitespace per line is trimmed.
/// - Blank-line runs are collapsed (leading/trailing blank lines dropped) but
///   only *outside* `<pre>` blocks; inside `<pre>` lines are preserved.
fn normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_pre = false;

    for line in s.split('\n') {
        update_pre_state(&mut in_pre, line);
        if in_pre {
            out.push_str(line);
            out.push('\n');
        } else {
            let trimmed = line.trim_end();
            if !trimmed.is_empty() {
                out.push_str(trimmed);
                out.push('\n');
            }
        }
    }

    // Drop the trailing newline added after the last real line.
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Track whether we are inside a `<pre>` block. The state is updated *before*
/// the current line is emitted, which matches how the spec renders code
/// (`<pre><code>...` opener and `</code></pre>` closer on their own lines).
fn update_pre_state(in_pre: &mut bool, line: &str) {
    let lower = line.to_ascii_lowercase();
    if *in_pre {
        if lower.contains("</pre") {
            *in_pre = false;
        }
    } else if lower.contains("<pre") {
        *in_pre = true;
    }
}

/// Void elements per HTML5. Their self-closing form is normalized away
/// (`<hr />` → `<hr>`) for consistency between renderers.
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
    "source", "track", "wbr",
];

fn is_void_element(name: &str) -> bool {
    VOID_ELEMENTS.contains(&name)
}

/// Tag normalization: sort attributes alphabetically and make void-element
/// self-closing slashes consistent. Comments, DOCTYPEs, processing
/// instructions and closing tags pass through untouched.
fn normalize_tags(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;

    while i < n {
        if chars[i] != '<' {
            out.push(chars[i]);
            i += 1;
            continue;
        }

        let next = if i + 1 < n { chars[i + 1] } else { '\0' };
        if next == '!' || next == '?' || next == '/' {
            // Comment (`<!-- ... -->`), declaration, processing instruction or
            // closing tag: find the end and copy the whole construct verbatim.
            let is_comment = next == '!'
                && chars.get(i + 2) == Some(&'-')
                && chars.get(i + 3) == Some(&'-');
            let end = if is_comment {
                find_comment_end(&chars, i).unwrap_or(n)
            } else {
                find_tag_end(&chars, i).unwrap_or(n)
            };
            if end >= n {
                // No well-formed end: emit as literal text and keep scanning.
                out.push(chars[i]);
                i += 1;
                continue;
            }
            out.extend(&chars[i..=end]);
            i = end + 1;
        } else {
            // Opening tag: rebuild with normalized attributes.
            match find_tag_end(&chars, i) {
                Some(end) => {
                    out.push_str(&rebuild_opening_tag(&chars, i, end));
                    i = end + 1;
                }
                None => {
                    out.push(chars[i]);
                    i += 1;
                }
            }
        }
    }

    out
}

/// Find the index of the `>` that closes a tag opened at `start`, respecting
/// single- and double-quoted attribute values. Returns `None` if unmatched.
fn find_tag_end(chars: &[char], start: usize) -> Option<usize> {
    let mut j = start;
    let mut quote: Option<char> = None;
    while j < chars.len() {
        let c = chars[j];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else {
            match c {
                '"' | '\'' => quote = Some(c),
                '>' => return Some(j),
                _ => {}
            }
        }
        j += 1;
    }
    None
}

/// Find the end (`-->`) of an HTML comment starting at `start`.
fn find_comment_end(chars: &[char], start: usize) -> Option<usize> {
    let mut j = start;
    while j + 2 < chars.len() {
        if chars[j] == '-' && chars[j + 1] == '-' && chars[j + 2] == '>' {
            return Some(j + 2);
        }
        j += 1;
    }
    None
}

/// Parse the attributes of an opening tag (`name="value"`, `name='value'`,
/// `name=value` or bare `name`). Attribute names are lowercased for canonical
/// comparison.
fn parse_attributes(s: &str) -> Vec<(String, Option<String>)> {
    let mut attrs: Vec<(String, Option<String>)> = Vec::new();
    let mut rest = s;

    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let name_end = rest
            .find(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or(rest.len());
        let name = rest[..name_end].trim().to_ascii_lowercase();
        rest = &rest[name_end..];

        rest = rest.trim_start();
        let value = if let Some(after_eq) = rest.strip_prefix('=') {
            let after_eq = after_eq.trim_start();
            if let Some(inner) = after_eq.strip_prefix('"') {
                let close = inner.find('"').unwrap_or(inner.len());
                let v = inner[..close].to_string();
                rest = if close < inner.len() { &inner[close + 1..] } else { "" };
                Some(v)
            } else if let Some(inner) = after_eq.strip_prefix('\'') {
                let close = inner.find('\'').unwrap_or(inner.len());
                let v = inner[..close].to_string();
                rest = if close < inner.len() { &inner[close + 1..] } else { "" };
                Some(v)
            } else {
                let v_end = after_eq.find(char::is_whitespace).unwrap_or(after_eq.len());
                let v = after_eq[..v_end].to_string();
                rest = &after_eq[v_end..];
                Some(v)
            }
        } else {
            None
        };

        attrs.push((name, value));
    }

    attrs
}

/// Rebuild an opening tag (`chars[start..=end]`, where `end` is the closing
/// `>`) with sorted attributes and canonical void-element rendering.
fn rebuild_opening_tag(chars: &[char], start: usize, end: usize) -> String {
    let content: String = chars[start + 1..end].iter().collect();
    let content = content.trim();

    // Split off the element name.
    let name_end = content
        .find(|c: char| c.is_whitespace() || c == '/')
        .unwrap_or(content.len());
    let name = &content[..name_end];
    let lower_name = name.to_ascii_lowercase();
    let void = is_void_element(&lower_name);

    // Everything after the name: attributes plus an optional trailing '/'.
    let mut attrs_str = content[name_end..].trim();
    let was_self_closing = attrs_str.ends_with('/');
    if was_self_closing {
        attrs_str = &attrs_str[..attrs_str.len() - 1];
    }

    let mut attrs = parse_attributes(attrs_str);
    attrs.sort_by(|a, b| a.0.cmp(&b.0));

    let mut tag = String::new();
    tag.push('<');
    tag.push_str(&lower_name);
    for (a_name, a_value) in attrs {
        tag.push(' ');
        tag.push_str(&a_name);
        if let Some(v) = a_value {
            tag.push_str("=\"");
            tag.push_str(&v);
            tag.push('"');
        }
    }
    if void {
        tag.push('>');
    } else if was_self_closing {
        tag.push_str("/>");
    } else {
        tag.push('>');
    }
    tag
}

/// The five core entities, canonicalized. Numeric/hex/uppercase spellings of
/// these entities are unified; every other entity passes through unchanged.
fn entity_canonical(token: &str) -> Option<&'static str> {
    if token.len() < 3 || !token.starts_with('&') || !token.ends_with(';') {
        return None;
    }
    let body = &token[1..token.len() - 1];
    if body.is_empty()
        || !body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '#' | 'x'))
    {
        return None;
    }
    Some(match body.to_ascii_lowercase().as_str() {
        "amp" | "#38" | "#x26" => "&amp;",
        "lt" | "#60" | "#x3c" => "&lt;",
        "gt" | "#62" | "#x3e" => "&gt;",
        "quot" | "#34" | "#x22" => "&quot;",
        "apos" | "#39" | "#x27" => "&#39;",
        _ => return None,
    })
}

/// Normalize equivalent entity spellings to a single canonical form.
fn normalize_entities(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;

    while i < n {
        if chars[i] == '&' {
            // Scan a bounded run up to the closing ';' (entities are short).
            let mut j = i + 1;
            while j < n && chars[j] != ';' && j - i <= 12 {
                j += 1;
            }
            if j < n && chars[j] == ';' {
                let token: String = chars[i..=j].iter().collect();
                if let Some(canonical) = entity_canonical(&token) {
                    out.push_str(canonical);
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

// -------------------------------------------------------------------------
// Compliance run + report
// -------------------------------------------------------------------------

/// Silence the panic hook while measuring so a panicking example doesn't spam
/// the report; the previous hook is restored by the caller afterwards.
fn silence_panic_hook() -> Box<dyn Fn(&std::panic::PanicHookInfo) + Sync + Send + 'static> {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    previous
}

/// Run every spec example through the converter and produce a report string.
fn run_compliance() -> String {
    let examples = parse_spec(SPEC);
    let hook = silence_panic_hook();

    let mut results: Vec<CaseResult> = Vec::with_capacity(examples.len());
    for ex in &examples {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pagina::markdown_to_html::convert(&ex.markdown)
        }));

        let (actual, error, panicked) = match outcome {
            Ok(Ok(html)) => (html, None, false),
            Ok(Err(e)) => (String::new(), Some(e.to_string()), false),
            Err(_) => (String::new(), None, true),
        };

        let expected = normalize_html(&ex.html);
        let actual_norm = normalize_html(&actual);
        let passed = !panicked && error.is_none() && actual_norm == expected;

        results.push(CaseResult {
            example: ex.clone(),
            passed,
            actual: actual_norm,
            expected,
            error,
            panicked,
        });
    }

    std::panic::set_hook(hook);
    build_report(&results)
}

fn build_report(results: &[CaseResult]) -> String {
    let total = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = total - passed;
    let panics = results.iter().filter(|r| r.panicked).count();
    let errors = results.iter().filter(|r| r.error.is_some()).count();
    let pct = if total > 0 {
        passed as f64 * 100.0 / total as f64
    } else {
        0.0
    };

    // Failure categories by spec section.
    let mut by_section: HashMap<&str, usize> = HashMap::new();
    for r in results.iter().filter(|r| !r.passed) {
        *by_section.entry(r.example.section.as_str()).or_insert(0) += 1;
    }
    let mut categories: Vec<(&str, usize)> = by_section.into_iter().collect();
    categories.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

    let failures: Vec<&CaseResult> = results.iter().filter(|r| !r.passed).collect();

    let bar = "=".repeat(72);
    let thin = "-".repeat(72);
    let mut report = String::new();

    report.push_str(&format!("{bar}\n"));
    report.push_str("CommonMark 0.31.2 compliance report — pagina::markdown_to_html\n");
    report.push_str(&format!("{bar}\n"));
    report.push_str(&format!("Running {} CommonMark spec tests...\n", total));
    report.push_str(&format!("Passed: {} / {} ({:.1}%)\n", passed, total, pct));
    report.push_str(&format!("Failed: {}\n", failed));
    if panics > 0 {
        report.push_str(&format!("Panics: {}\n", panics));
    }
    if errors > 0 {
        report.push_str(&format!("Errors (convert returned Err): {}\n", errors));
    }

    report.push_str(&format!("{thin}\n"));
    report.push_str("Top failure categories (by spec section):\n");
    if categories.is_empty() {
        report.push_str("  (none — 100% compliance)\n");
    } else {
        for (section, count) in categories.iter().take(15) {
            report.push_str(&format!("  - {}: {}\n", section, count));
        }
        if categories.len() > 15 {
            report.push_str(&format!(
                "  - ... and {} more categories\n",
                categories.len() - 15
            ));
        }
    }

    report.push_str(&format!("{thin}\n"));
    report.push_str(&format!(
        "First {} failures with diff:\n",
        failures.len().min(10)
    ));
    for r in failures.iter().take(10) {
        report.push_str(&format_failure(r));
    }

    report.push_str(&format!("{thin}\n"));
    let failed_numbers: Vec<usize> = failures.iter().map(|r| r.example.number).collect();
    report.push_str(&format!(
        "Failed example numbers: {}\n",
        compress_ranges(&failed_numbers)
    ));

    report.push_str(&format!("{bar}\n"));
    report
}

/// Render a per-example failure card (input, expected vs actual, diff).
fn format_failure(r: &CaseResult) -> String {
    let mut s = String::new();
    let ex = &r.example;
    s.push_str(&format!("  [example {}] ({})\n", ex.number, ex.section));
    s.push_str(&format!("    Markdown: {}\n", preview(&ex.markdown, 200)));
    if let Some(err) = &r.error {
        s.push_str(&format!("    convert() error: {}\n", err));
        return s;
    }
    if r.panicked {
        s.push_str("    convert() panicked\n");
        return s;
    }
    s.push_str(&format!("    Expected: {}\n", preview(&r.expected, 200)));
    s.push_str(&format!("    Actual:   {}\n", preview(&r.actual, 200)));
    s.push_str("    Diff:\n");
    for line in diff(&r.expected, &r.actual).lines().take(60) {
        s.push_str(&format!("      {}\n", line));
    }
    s
}

/// A single-line preview of a (possibly multiline) string for failure cards.
/// Control characters such as tabs are shown as `\t` so they are visible.
fn preview(s: &str, max_chars: usize) -> String {
    let escaping: String = s.chars().flat_map(|c| c.escape_debug()).collect();
    if escaping.chars().count() <= max_chars {
        return escaping;
    }
    let truncated: String = escaping.chars().take(max_chars).collect();
    format!("{}…", truncated)
}

/// Bounded line-based diff (LCS), rendered in a compact +/- format.
fn diff(expected: &str, actual: &str) -> String {
    const MAX_LINES: usize = 250;
    let a: Vec<&str> = expected.lines().collect();
    let b: Vec<&str> = actual.lines().collect();
    if a.len() > MAX_LINES || b.len() > MAX_LINES {
        return format!("(diff suppressed: {} vs {} lines)", a.len(), b.len());
    }

    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    let mut out = String::new();
    let (mut i, mut j) = (0, 0);
    while i < m && j < n {
        if a[i] == b[j] {
            out.push_str(&format!("  {}\n", a[i]));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            out.push_str(&format!("- {}\n", a[i]));
            i += 1;
        } else {
            out.push_str(&format!("+ {}\n", b[j]));
            j += 1;
        }
    }
    while i < m {
        out.push_str(&format!("- {}\n", a[i]));
        i += 1;
    }
    while j < n {
        out.push_str(&format!("+ {}\n", b[j]));
        j += 1;
    }
    out.trim_end().to_string()
}

/// Compress a list of numbers into ranges, e.g. `1,2,3,5,6` → `1-3,5-6`.
fn compress_ranges(nums: &[usize]) -> String {
    if nums.is_empty() {
        return "(none)".to_string();
    }
    let mut sorted: Vec<usize> = nums.to_vec();
    sorted.sort_unstable();
    sorted.dedup();

    let mut parts: Vec<String> = Vec::new();
    let mut start = sorted[0];
    let mut prev = sorted[0];
    for &n in sorted.iter().skip(1) {
        if n == prev + 1 {
            prev = n;
        } else {
            parts.push(range_str(start, prev));
            start = n;
            prev = n;
        }
    }
    parts.push(range_str(start, prev));
    parts.join(",")
}

fn range_str(start: usize, end: usize) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{}-{}", start, end)
    }
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

/// The full compliance sweep. `#[ignore]`d by default because it is a
/// measurement/audit tool; run with:
/// `cargo test --test commonmark_compliance -- --ignored --nocapture`
#[test]
#[ignore]
fn commonmark_compliance_run() {
    let report = run_compliance();
    println!("\n{}", report);
    let parsed = parse_spec(SPEC);
    assert!(
        parsed.len() >= 600,
        "expected at least 600 spec examples, parsed {}",
        parsed.len()
    );
}

#[test]
fn spec_file_parses_all_examples() {
    let examples = parse_spec(SPEC);
    assert!((600..=700).contains(&examples.len()), "unexpected example count: {}", examples.len());

    // Example 1 lives in the "Tabs" section and uses a tab (→) in its input.
    assert_eq!(examples[0].section, "Tabs");
    assert!(examples[0].markdown.contains('\t'), "tab replacement missing");
    assert_eq!(examples[0].html, "<pre><code>foo\tbaz\t\tbim\n</code></pre>");

    // All examples must carry a non-empty section tag.
    for ex in examples.iter().take(50) {
        assert!(!ex.section.is_empty());
    }
}

#[test]
fn normalize_whitespace_collapses_but_preserves_pre() {
    assert_eq!(normalize_whitespace("a  \n\n\nb\r\n"), "a\nb");
    assert_eq!(
        normalize_whitespace("<pre><code>a  \n\nb\n</code></pre>\n\n"),
        "<pre><code>a  \n\nb\n</code></pre>"
    );
}

#[test]
fn normalize_tags_sorts_attrs_and_fixes_void_slashes() {
    assert_eq!(normalize_tags("<hr />"), "<hr>");
    assert_eq!(normalize_tags("<br/>"), "<br>");
    assert_eq!(normalize_tags("<img src=\"x\" alt=\"y\" />"), "<img alt=\"y\" src=\"x\">");
    assert_eq!(
        normalize_tags("<a title=\"a\" href=\"b\">x</a>"),
        "<a href=\"b\" title=\"a\">x</a>"
    );
    // Comments must pass through untouched.
    assert_eq!(normalize_tags("<!-- <a b=\"c\"> <hr /> -->"), "<!-- <a b=\"c\"> <hr /> -->");
}

#[test]
fn normalize_entities_unifies_spellings() {
    assert_eq!(normalize_entities("&#38; &#x26; &AMP;"), "&amp; &amp; &amp;");
    assert_eq!(normalize_entities("&apos; &#x27; &#39;"), "&#39; &#39; &#39;");
    assert_eq!(normalize_entities("&#x3C; &#60;"), "&lt; &lt;");
    // Unrelated entities pass through unchanged.
    assert_eq!(normalize_entities("&nbsp; &copy;"), "&nbsp; &copy;");
}

#[test]
fn compress_ranges_merges_runs() {
    assert_eq!(compress_ranges(&[]), "(none)");
    assert_eq!(compress_ranges(&[1]), "1");
    assert_eq!(compress_ranges(&[1, 2, 3, 5, 6, 9]), "1-3,5-6,9");
    assert_eq!(compress_ranges(&[6, 5, 3, 2, 1]), "1-3,5-6");
}

#[test]
fn diff_marks_added_and_removed_lines() {
    let d = diff("a\nb\nc", "a\nx\nc");
    assert!(d.contains("- b"));
    assert!(d.contains("+ x"));
}