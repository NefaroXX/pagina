use crate::error::Result;
use crate::html_escape::unescape_html;

/// HTML void elements that never have children or closing tags.
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr",
];

fn is_void_element(name: &str) -> bool {
    VOID_ELEMENTS.contains(&name)
}

#[derive(Debug, Clone, PartialEq)]
enum HtmlToken {
    StartTag {
        name: String,
        attrs: Vec<(String, String)>,
        self_closing: bool,
    },
    EndTag(String),
    Text(String),
    Comment(String),
    Doctype(String),
}

struct HtmlTokenizer {
    input: String,
    pos: usize,
}

impl HtmlTokenizer {
    fn new(input: String) -> Self {
        HtmlTokenizer { input, pos: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn advance(&mut self, n: usize) {
        self.pos += n;
    }

    fn consume_while<F>(&mut self, mut pred: F) -> String
    where
        F: FnMut(char) -> bool,
    {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if pred(ch) {
                let len = ch.len_utf8();
                self.advance(len);
            } else {
                break;
            }
        }
        self.input[start..self.pos].to_string()
    }

    fn next_token(&mut self) -> Option<HtmlToken> {
        if self.pos >= self.input.len() {
            return None;
        }

        let ch = self.peek()?;

        if ch == '<' {
            // Check for comment, doctype, or tag
            if self.input[self.pos..].starts_with("<!--") {
                return self.parse_comment();
            }
            if self.input[self.pos..].starts_with("<!") {
                return self.parse_doctype();
            }
            return self.parse_tag();
        }

        // Text content
        let text = self.consume_while(|c| c != '<');
        if !text.is_empty() {
            return Some(HtmlToken::Text(text));
        }

        None
    }

    fn parse_comment(&mut self) -> Option<HtmlToken> {
        self.advance(4); // <!--
        let start = self.pos;
        while self.pos + 3 <= self.input.len() {
            if &self.input[self.pos..self.pos + 3] == "-->" {
                let content = self.input[start..self.pos].to_string();
                self.advance(3);
                return Some(HtmlToken::Comment(content));
            }
            let len = self.peek()?.len_utf8();
            self.advance(len);
        }
        // Unterminated comment - treat as text
        let content = self.input[start..].to_string();
        self.pos = self.input.len();
        Some(HtmlToken::Comment(content))
    }

    fn parse_doctype(&mut self) -> Option<HtmlToken> {
        self.advance(2); // <!
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch == '>' {
                let content = self.input[start..self.pos].to_string();
                self.advance(1);
                return Some(HtmlToken::Doctype(content));
            }
            let len = ch.len_utf8();
            self.advance(len);
        }
        let content = self.input[start..].to_string();
        self.pos = self.input.len();
        Some(HtmlToken::Doctype(content))
    }

    fn parse_tag(&mut self) -> Option<HtmlToken> {
        self.advance(1); // <

        let is_end_tag = self.peek() == Some('/');
        if is_end_tag {
            self.advance(1); // /
            let name = self.consume_while(|c| c.is_ascii_alphanumeric() || c == '-' || c == ':');
            self.consume_while(|c| c != '>');
            if self.peek() == Some('>') {
                self.advance(1);
            }
            return Some(HtmlToken::EndTag(name.to_lowercase()));
        }

        // Start tag
        let name = self.consume_while(|c| c.is_ascii_alphanumeric() || c == '-' || c == ':');
        let name = name.to_lowercase();

        // Parse attributes
        let mut attrs = Vec::new();
        loop {
            self.consume_while(|c| c.is_whitespace());
            if self.peek() == Some('>') || self.peek() == Some('/') {
                break;
            }

            let attr_name = self
                .consume_while(|c| c.is_ascii_alphanumeric() || c == '-' || c == ':' || c == '_');
            if attr_name.is_empty() {
                break;
            }

            self.consume_while(|c| c.is_whitespace());
            let mut attr_value = String::new();
            if self.peek() == Some('=') {
                self.advance(1); // =
                self.consume_while(|c| c.is_whitespace());
                let quote = self.peek();
                if quote == Some('"') || quote == Some('\'') {
                    self.advance(1);
                    let start = self.pos;
                    while let Some(ch) = self.peek() {
                        if Some(ch) == quote {
                            break;
                        }
                        let len = ch.len_utf8();
                        self.advance(len);
                    }
                    attr_value = self.input[start..self.pos].to_string();
                    if self.peek() == quote {
                        self.advance(1);
                    }
                } else {
                    // Unquoted attribute value
                    attr_value = self.consume_while(|c| !c.is_whitespace() && c != '>' && c != '/');
                }
            }
            attrs.push((attr_name, attr_value));
        }

        let self_closing = self.peek() == Some('/');
        if self_closing {
            self.advance(1);
        }

        if self.peek() == Some('>') {
            self.advance(1);
        }

        Some(HtmlToken::StartTag {
            name,
            attrs,
            self_closing,
        })
    }

    fn tokenize(&mut self) -> Vec<HtmlToken> {
        let mut tokens = Vec::new();
        while let Some(token) = self.next_token() {
            tokens.push(token);
        }
        tokens
    }
}

struct MdConverter {
    tokens: Vec<HtmlToken>,
    pos: usize,
    in_pre: bool,
    gfm: bool,
    /// Inside a footnote `<li>`: backref anchors are swallowed.
    in_footnote_li: bool,
}

/// Per-column alignment of an HTML table, derived from `align`/`style`
/// attributes on `<th>`/`<td>` cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Alignment {
    None,
    Left,
    Center,
    Right,
}

/// One collected `<tr>` row during HTML table conversion.
struct TableRow {
    cells: Vec<(String, Alignment)>,
    any_th: bool,
    in_thead: bool,
}

/// Column alignment from a cell's attributes: the `align` attribute wins,
/// then `style="text-align: …"`.
fn alignment_from_attrs(attrs: &[(String, String)]) -> Alignment {
    if let Some((_, v)) = attrs.iter().find(|(k, _)| k == "align") {
        match v.to_ascii_lowercase().as_str() {
            "left" => return Alignment::Left,
            "center" | "middle" => return Alignment::Center,
            "right" => return Alignment::Right,
            _ => {}
        }
    }
    if let Some((_, v)) = attrs.iter().find(|(k, _)| k == "style") {
        let lower = v.to_ascii_lowercase();
        if let Some(colon) = lower.find("text-align:") {
            let value = lower[colon + "text-align:".len()..]
                .split(';')
                .next()
                .unwrap_or("")
                .trim();
            match value {
                "left" => return Alignment::Left,
                "center" | "middle" => return Alignment::Center,
                "right" => return Alignment::Right,
                _ => {}
            }
        }
    }
    Alignment::None
}

/// Escape a table cell for the pipe-table format: `|` becomes `\|`,
/// newlines become spaces, surrounding whitespace is trimmed.
fn escape_cell(cell: &str) -> String {
    cell.trim().replace('|', "\\|").replace('\n', " ")
}

/// Delimiter marker for a column alignment (cmark table delimiter codes:
/// none `---`, left `:--`, center `:-:`, right `--:`).
fn delimiter_marker(a: Alignment) -> &'static str {
    match a {
        Alignment::None => "---",
        Alignment::Left => ":--",
        Alignment::Center => ":-:",
        Alignment::Right => "--:",
    }
}

impl MdConverter {
    #[allow(dead_code)]
    fn new(tokens: Vec<HtmlToken>) -> Self {
        MdConverter {
            tokens,
            pos: 0,
            in_pre: false,
            gfm: false,
            in_footnote_li: false,
        }
    }

    fn with_gfm(tokens: Vec<HtmlToken>, gfm: bool) -> Self {
        MdConverter {
            tokens,
            pos: 0,
            in_pre: false,
            gfm,
            in_footnote_li: false,
        }
    }

    fn peek(&self) -> Option<&HtmlToken> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn convert(&mut self) -> String {
        let mut output = String::new();
        while self.pos < self.tokens.len() {
            output.push_str(&self.convert_node());
        }
        output
    }

    fn convert_node(&mut self) -> String {
        match self.peek() {
            Some(HtmlToken::Text(text)) => {
                let text = text.clone();
                self.advance();
                if self.in_pre {
                    text
                } else {
                    // Normalize whitespace, then unescape HTML entities
                    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    if normalized.is_empty() {
                        String::new()
                    } else {
                        unescape_html(&normalized)
                    }
                }
            }
            Some(HtmlToken::Comment(_)) => {
                self.advance();
                String::new() // Skip comments
            }
            Some(HtmlToken::Doctype(_)) => {
                self.advance();
                String::new()
            }
            Some(HtmlToken::StartTag { name, attrs, .. }) => {
                let name = name.clone();
                let attrs = attrs.clone();
                self.advance();

                match name.as_str() {
                    "p" => {
                        let content = self.convert_until_end("p");
                        format!("\n{}\n\n", content.trim())
                    }
                    "h1" => {
                        let content = self.convert_until_end("h1");
                        format!("\n# {}\n\n", content.trim())
                    }
                    "h2" => {
                        let content = self.convert_until_end("h2");
                        format!("\n## {}\n\n", content.trim())
                    }
                    "h3" => {
                        let content = self.convert_until_end("h3");
                        format!("\n### {}\n\n", content.trim())
                    }
                    "h4" => {
                        let content = self.convert_until_end("h4");
                        format!("\n#### {}\n\n", content.trim())
                    }
                    "h5" => {
                        let content = self.convert_until_end("h5");
                        format!("\n##### {}\n\n", content.trim())
                    }
                    "h6" => {
                        let content = self.convert_until_end("h6");
                        format!("\n###### {}\n\n", content.trim())
                    }
                    "strong" | "b" => {
                        let content = self.convert_until_end(&name);
                        format!("**{}**", content)
                    }
                    "em" | "i" => {
                        let content = self.convert_until_end(&name);
                        format!("*{}*", content)
                    }
                    "del" | "s" | "strike" => {
                        let content = self.convert_until_end(&name);
                        if self.gfm {
                            format!("~~{}~~", content)
                        } else {
                            content
                        }
                    }
                    "input" => {
                        // GFM task checkbox: `<input type="checkbox">`
                        // becomes a sentinel consumed by the `<li>` arm.
                        // Outside lists (or with GFM off) it vanishes.
                        if self.gfm && is_checkbox(attrs.as_slice()) {
                            if has_checked(attrs.as_slice()) {
                                "\0CHECKED\0".to_string()
                            } else {
                                "\0UNCHECKED\0".to_string()
                            }
                        } else {
                            String::new()
                        }
                    }
                    "code" => {
                        if self.in_pre {
                            self.convert_until_end("code")
                        } else {
                            let content = self.convert_until_end("code");
                            format!("`{}`", content)
                        }
                    }
                    "pre" => {
                        let was_in_pre = self.in_pre;
                        self.in_pre = true;
                        let content = self.convert_until_end("pre");
                        self.in_pre = was_in_pre;
                        format!("\n```\n{}\n```\n", content.trim_end())
                    }
                    "a" => {
                        // Footnote backlinks vanish inside footnote bodies:
                        // they are anchor chrome, not content.
                        if self.gfm && self.in_footnote_li && is_backref_anchor(&attrs) {
                            self.convert_until_end("a");
                            return String::new();
                        }
                        let href = attrs
                            .iter()
                            .find(|(k, _)| k == "href")
                            .map(|(_, v)| v.clone())
                            .unwrap_or_default();
                        let content = self.convert_until_end("a");
                        if href.is_empty() || (self.gfm && is_bare_anchor(&content, &href)) {
                            content
                        } else {
                            format!("[{}]({})", content, href)
                        }
                    }
                    "sup" => {
                        // GFM footnote reference: `<sup><a
                        // href="#fn-N">N</a></sup>` becomes `[^N]`.
                        // Anything else unwraps to its content.
                        if self.gfm {
                            match self.scan_footnote_sup() {
                                Some(label) => format!("[^{}]", label),
                                None => self.convert_until_end("sup"),
                            }
                        } else {
                            self.convert_until_end("sup")
                        }
                    }
                    "span" => {
                        // GFM dollar math passthrough back to `$…$`.
                        // Non-math spans unwrap to their content.
                        if has_math_class(&attrs, "math-inline") {
                            let content = self.convert_until_end("span");
                            if self.gfm {
                                format!("${}$", content.trim())
                            } else {
                                content
                            }
                        } else if has_math_class(&attrs, "math-display") {
                            let content = self.convert_until_end("span");
                            if self.gfm {
                                format!("$${}$$", content.trim())
                            } else {
                                content
                            }
                        } else {
                            self.convert_until_end(&name)
                        }
                    }
                    "section" | "div" => {
                        // GFM dollar math display back to `$$…$$`.
                        if has_math_class(&attrs, "math-display") {
                            let content = self.convert_until_end(&name);
                            if self.gfm {
                                format!("\n$${}$$\n", content.trim())
                            } else {
                                format!("\n{}\n", content.trim())
                            }
                        // GFM footnotes footer back to `[^N]: …` definitions.
                        // Anything else unwraps to its content.
                        } else if self.gfm && has_footnotes_class(&attrs) {
                            let content = self.convert_footnote_section(&name);
                            format!("\n{}\n", content.trim())
                        } else {
                            self.convert_until_end(&name)
                        }
                    }
                    "dl" => {
                        // GFM definition list back to term / `: desc` form.
                        if self.gfm {
                            let content = self.convert_deflist();
                            format!("\n{}\n", content.trim())
                        } else {
                            self.convert_until_end(&name)
                        }
                    }
                    "dt" | "dd" => {
                        // Stray term/description outside any `<dl>` (which
                        // consumes its own): keep content readable without
                        // inventing markers, in both modes.
                        let content = self.convert_until_end(&name);
                        format!("\n{}\n", content.trim())
                    }
                    "ul" => {
                        let content = self.convert_until_end("ul");
                        format!("\n{}\n", content.trim())
                    }
                    "ol" => {
                        let start = attrs
                            .iter()
                            .find(|(k, _)| k == "start")
                            .and_then(|(_, v)| v.parse::<u32>().ok())
                            .unwrap_or(1);
                        let content = self.convert_ordered_list(start);
                        format!("\n{}\n", content.trim())
                    }
                    "li" => {
                        let content = self.convert_until_end("li");
                        let trimmed = content.trim_start();
                        if self.gfm {
                            if let Some(rest) = trimmed.strip_prefix("\0CHECKED\0") {
                                return format!("- [x] {}\n", rest.trim_start());
                            }
                            if let Some(rest) = trimmed.strip_prefix("\0UNCHECKED\0") {
                                return format!("- [ ] {}\n", rest.trim_start());
                            }
                        }
                        format!("- {}\n", content.trim())
                    }
                    "blockquote" => {
                        let content = self.convert_until_end("blockquote");
                        let lines: Vec<&str> = content.lines().collect();
                        let quoted: Vec<String> =
                            lines.iter().map(|l| format!("> {}", l)).collect();
                        format!("\n{}\n", quoted.join("\n"))
                    }
                    "br" => "\n".to_string(),
                    "hr" => "\n---\n".to_string(),
                    "table" => {
                        let content = self.convert_table();
                        format!("\n{}\n", content.trim())
                    }
                    "img" => {
                        let src = attrs
                            .iter()
                            .find(|(k, _)| k == "src")
                            .map(|(_, v)| v.clone())
                            .unwrap_or_default();
                        let alt = attrs
                            .iter()
                            .find(|(k, _)| k == "alt")
                            .map(|(_, v)| v.clone())
                            .unwrap_or_default();
                        if src.is_empty() {
                            String::new()
                        } else {
                            format!("![{}]({})", alt, src)
                        }
                    }
                    name if is_void_element(name) => {
                        // Other void elements: skip silently
                        String::new()
                    }
                    _ => {
                        // Unknown tag - skip but process children
                        self.convert_until_end(&name)
                    }
                }
            }
            Some(HtmlToken::EndTag(_)) => {
                self.advance();
                String::new()
            }
            None => String::new(),
        }
    }

    fn convert_until_end(&mut self, tag_name: &str) -> String {
        let mut content = String::new();
        while self.pos < self.tokens.len() {
            match self.peek() {
                Some(HtmlToken::EndTag(name)) if name == tag_name => {
                    self.advance();
                    break;
                }
                _ => {
                    content.push_str(&self.convert_node());
                }
            }
        }
        content
    }

    fn convert_ordered_list(&mut self, mut start: u32) -> String {
        let mut content = String::new();
        while self.pos < self.tokens.len() {
            match self.peek() {
                Some(HtmlToken::EndTag(name)) if name == "ol" => {
                    self.advance();
                    break;
                }
                Some(HtmlToken::StartTag { name, .. }) if name == "li" => {
                    self.advance();
                    let item_content = self.convert_until_end("li");
                    let trimmed = item_content.trim_start();
                    if self.gfm {
                        if let Some(rest) = trimmed.strip_prefix("\0CHECKED\0") {
                            content.push_str(&format!("{}. [x] {}\n", start, rest.trim_start()));
                            start += 1;
                            continue;
                        }
                        if let Some(rest) = trimmed.strip_prefix("\0UNCHECKED\0") {
                            content.push_str(&format!("{}. [ ] {}\n", start, rest.trim_start()));
                            start += 1;
                            continue;
                        }
                    }
                    content.push_str(&format!("{}. {}\n", start, item_content.trim()));
                    start += 1;
                }
                _ => {
                    self.convert_node(); // Skip other tags
                }
            }
        }
        content
    }

    /// Convert a `<table>` element into GFM pipe-table Markdown, consuming
    /// through its `</table>` end tag. Consumes the closing tag itself.
    fn convert_table(&mut self) -> String {
        let rows = self.collect_table_rows();
        if rows.is_empty() {
            return String::new();
        }
        // Header row: the first row inside <thead>, else the first row that
        // contains any <th>, else the first row (graceful fallback).
        let header_idx = rows
            .iter()
            .position(|r| r.in_thead)
            .or_else(|| rows.iter().position(|r| r.any_th))
            .unwrap_or(0);
        let header_row = &rows[header_idx];
        let ncols = header_row.cells.len();
        if ncols == 0 {
            return String::new();
        }
        // Per-column alignment: the header cell's alignment, falling back to
        // the first body (non-header) row's cell.
        let mut aligns: Vec<Alignment> = Vec::with_capacity(ncols);
        for k in 0..ncols {
            let from_header = header_row
                .cells
                .get(k)
                .map(|(_, a)| *a)
                .unwrap_or(Alignment::None);
            let fallback = rows
                .iter()
                .skip(header_idx + 1)
                .find_map(|r| r.cells.get(k).map(|(_, a)| *a))
                .unwrap_or(Alignment::None);
            aligns.push(if from_header != Alignment::None {
                from_header
            } else {
                fallback
            });
        }
        let mut out = String::new();
        for (k, (cell, _)) in header_row.cells.iter().take(ncols).enumerate() {
            out.push_str(if k == 0 { "| " } else { " | " });
            out.push_str(cell);
        }
        out.push_str(" |\n");
        out.push('|');
        for a in &aligns {
            out.push_str(&format!(" {} |", delimiter_marker(*a)));
        }
        out.push('\n');
        for (idx, row) in rows.iter().enumerate() {
            if idx == header_idx {
                continue;
            }
            out.push('|');
            for k in 0..ncols {
                let cell = row.cells.get(k).map(|(c, _)| c.as_str()).unwrap_or("");
                out.push_str(&format!(" {} |", cell));
            }
            out.push('\n');
        }
        out
    }

    /// Collect every `<tr>` row (with cell content and alignment) until the
    /// `</table>` end tag, which is consumed. Rows inside `<thead>` are
    /// flagged; `<tbody>`/`<colgroup>`/`<caption>` are skipped structurally.
    fn collect_table_rows(&mut self) -> Vec<TableRow> {
        let mut rows: Vec<TableRow> = Vec::new();
        let mut thead_depth = 0usize;
        while self.pos < self.tokens.len() {
            match self.peek() {
                Some(HtmlToken::EndTag(name)) if name == "table" => {
                    self.advance();
                    break;
                }
                Some(HtmlToken::StartTag { name, .. }) if name == "thead" => {
                    thead_depth += 1;
                    self.advance();
                }
                Some(HtmlToken::EndTag(name)) if name == "thead" => {
                    thead_depth = thead_depth.saturating_sub(1);
                    self.advance();
                }
                Some(HtmlToken::StartTag { name, .. }) if name == "tr" => {
                    self.advance();
                    rows.push(self.collect_table_row(thead_depth > 0));
                }
                _ => {
                    self.advance();
                }
            }
        }
        rows
    }

    /// Scan a `<sup>…</sup>` body (start tag already consumed) for a
    /// footnote anchor (`<a href="#fn-N">`). On a hit the whole element is
    /// consumed and the footnote label returned; otherwise nothing is
    /// consumed and the caller falls back to generic children conversion.
    fn scan_footnote_sup(&mut self) -> Option<String> {
        let mut k = self.pos;
        let mut depth = 0usize;
        let mut found: Option<String> = None;
        while k < self.tokens.len() {
            match &self.tokens[k] {
                HtmlToken::StartTag { name, attrs, .. } => {
                    if name == "a" {
                        if let Some((_, href)) = attrs.iter().find(|(a, _)| a == "href") {
                            if let Some(label) = href.strip_prefix("#fn-") {
                                if !label.is_empty() {
                                    found = Some(label.to_string());
                                }
                            }
                        }
                    }
                    if name == "sup" {
                        depth += 1;
                    }
                    k += 1;
                }
                HtmlToken::EndTag(name) if name == "sup" => {
                    if depth == 0 {
                        break;
                    }
                    depth = depth.saturating_sub(1);
                    k += 1;
                }
                _ => {
                    k += 1;
                }
            }
            if found.is_some() {
                // Skip to the matching `</sup>` so the whole element is
                // consumed exactly once.
                while k < self.tokens.len() {
                    match &self.tokens[k] {
                        HtmlToken::EndTag(name) if name == "sup" => {
                            if depth == 0 {
                                k += 1;
                                break;
                            }
                            depth = depth.saturating_sub(1);
                            k += 1;
                        }
                        HtmlToken::StartTag { name, .. } if name == "sup" => {
                            depth += 1;
                            k += 1;
                        }
                        _ => k += 1,
                    }
                }
                self.pos = k;
                return found;
            }
        }
        None
    }

    /// Convert a footnotes footer (`<section>`/`<div class="footnotes">`,
    /// start tag already consumed) into `[^N]: …` definitions, consuming
    /// through the matching end tag. Backref anchors are swallowed;
    /// `<hr>` separators are skipped.
    fn convert_footnote_section(&mut self, tag_name: &str) -> String {
        let mut defs: Vec<String> = Vec::new();
        while self.pos < self.tokens.len() {
            match self.peek() {
                Some(HtmlToken::EndTag(name)) if name == tag_name => {
                    self.advance();
                    break;
                }
                Some(HtmlToken::StartTag { name, attrs, .. }) if name == "li" => {
                    let attrs = attrs.clone();
                    self.advance();
                    let label = attrs
                        .iter()
                        .find(|(k, _)| k == "id")
                        .map(|(_, v)| v.strip_prefix("fn-").unwrap_or(v).to_string())
                        .unwrap_or_default();
                    let was = self.in_footnote_li;
                    self.in_footnote_li = true;
                    let content = self.convert_until_end("li");
                    self.in_footnote_li = was;
                    if !label.is_empty() {
                        defs.push(format_reverse_def(&label, content.trim()));
                    }
                }
                Some(HtmlToken::StartTag { name, .. }) if name == "hr" => {
                    self.advance();
                }
                _ => {
                    self.advance();
                }
            }
        }
        defs.join("\n\n")
    }

    /// Convert a `<dl>` element (start tag already consumed) into term /
    /// `: description` form, consuming through `</dl>`. Multi-paragraph
    /// descriptions keep later paragraphs 4-space indented so they re-parse
    /// into the same description.
    fn convert_deflist(&mut self) -> String {
        let mut items: Vec<(Vec<String>, Vec<String>)> = Vec::new();
        let mut terms: Vec<String> = Vec::new();
        let mut descs: Vec<String> = Vec::new();
        while self.pos < self.tokens.len() {
            match self.peek() {
                Some(HtmlToken::EndTag(name)) if name == "dl" => {
                    self.advance();
                    break;
                }
                Some(HtmlToken::StartTag { name, .. }) if name == "dt" => {
                    self.advance();
                    let content = self.convert_until_end("dt");
                    let term = content.split_whitespace().collect::<Vec<_>>().join(" ");
                    // A term after descriptions starts a new item sharing
                    // nothing with the previous one.
                    if !descs.is_empty() {
                        items.push((std::mem::take(&mut terms), std::mem::take(&mut descs)));
                    }
                    if !term.is_empty() {
                        terms.push(term);
                    }
                }
                Some(HtmlToken::StartTag { name, .. }) if name == "dd" => {
                    self.advance();
                    let content = self.convert_until_end("dd");
                    descs.push(format_reverse_desc(content.trim()));
                }
                _ => {
                    self.advance();
                }
            }
        }
        if !terms.is_empty() || !descs.is_empty() {
            items.push((terms, descs));
        }
        let mut out = String::new();
        for (terms, descs) in &items {
            for term in terms {
                out.push_str(term);
                out.push('\n');
            }
            for desc in descs {
                out.push_str(desc);
                out.push('\n');
            }
        }
        out.trim_end().to_string()
    }

    /// Collect the cells of one `<tr>` row, consuming through `</tr>`.
    fn collect_table_row(&mut self, in_thead: bool) -> TableRow {
        let mut cells: Vec<(String, Alignment)> = Vec::new();
        let mut any_th = false;
        while self.pos < self.tokens.len() {
            match self.peek() {
                Some(HtmlToken::EndTag(name)) if name == "tr" => {
                    self.advance();
                    break;
                }
                Some(HtmlToken::StartTag { name, attrs, .. }) if name == "th" || name == "td" => {
                    let name = name.clone();
                    let attrs = attrs.clone();
                    let is_th = name == "th";
                    let alignment = alignment_from_attrs(&attrs);
                    if is_th {
                        any_th = true;
                    }
                    self.advance();
                    let content = self.convert_until_end(&name);
                    cells.push((escape_cell(&content), alignment));
                }
                _ => {
                    // Skip stray whitespace/text between cells.
                    self.convert_node();
                }
            }
        }
        TableRow {
            cells,
            any_th,
            in_thead,
        }
    }
}

/// True when a tag's `class` attribute carries a math passthrough token
/// (`class="… math-inline …"`, the forward converter's shape).
fn has_math_class(attrs: &[(String, String)], token: &str) -> bool {
    attrs
        .iter()
        .find(|(k, _)| k == "class")
        .map(|(_, v)| v.split_whitespace().any(|c| c == token))
        .unwrap_or(false)
}

/// True when a `<section>`/`<div>` tag's attributes mark a footnotes footer
/// (`class="… footnotes …"`, the forward converter's shape).
fn has_footnotes_class(attrs: &[(String, String)]) -> bool {
    attrs
        .iter()
        .find(|(k, _)| k == "class")
        .map(|(_, v)| v.split_whitespace().any(|c| c == "footnotes"))
        .unwrap_or(false)
}

/// True when an anchor's attributes mark a footnote backlink (swallowed
/// inside footnote bodies on the reverse path).
fn is_backref_anchor(attrs: &[(String, String)]) -> bool {
    if attrs.iter().any(|(k, _)| k == "data-footnote-backref") {
        return true;
    }
    attrs
        .iter()
        .find(|(k, _)| k == "class")
        .map(|(_, v)| v.split_whitespace().any(|c| c == "footnote-backref"))
        .unwrap_or(false)
}

/// Format one reversed footnote definition: `[^label]: first paragraph`,
/// later paragraphs blank-separated and 4-space indented (mirrors the AST
/// Markdown emitter so output re-parses into the same definition).
fn format_reverse_def(label: &str, content: &str) -> String {
    let paras: Vec<&str> = content.split("\n\n").collect();
    let first = paras.first().copied().unwrap_or("").replace('\n', " ");
    let mut out = format!("[^{}]: {}", label, first.trim());
    for para in paras.iter().skip(1) {
        out.push_str("\n\n");
        for line in para.lines() {
            if line.trim().is_empty() {
                out.push('\n');
            } else {
                out.push_str("    ");
                out.push_str(line.trim());
                out.push('\n');
            }
        }
        out.truncate(out.trim_end().len());
    }
    out
}

/// Format one reversed description: `: first paragraph`, later paragraphs
/// blank-separated and 4-space indented.
fn format_reverse_desc(content: &str) -> String {
    let paras: Vec<&str> = content.split("\n\n").collect();
    let first = paras.first().copied().unwrap_or("").replace('\n', " ");
    let mut out = format!(": {}", first.trim());
    for para in paras.iter().skip(1) {
        out.push_str("\n\n");
        for line in para.lines() {
            if line.trim().is_empty() {
                out.push('\n');
            } else {
                out.push_str("    ");
                out.push_str(line.trim());
                out.push('\n');
            }
        }
        out.truncate(out.trim_end().len());
    }
    out
}

/// True when an `<input>` tag's attributes describe a checkbox.
fn is_checkbox(attrs: &[(String, String)]) -> bool {
    attrs
        .iter()
        .find(|(k, _)| k == "type")
        .map(|(_, v)| v.eq_ignore_ascii_case("checkbox"))
        .unwrap_or(false)
}

/// True when a checkbox `<input>` carries a `checked` attribute (any value,
/// including bare `checked` which parses to an empty string).
fn has_checked(attrs: &[(String, String)]) -> bool {
    attrs.iter().any(|(k, _)| k == "checked")
}

/// True when an anchor's visible text is already its bare form, so the GFM
/// reverse direction emits plain text instead of `[text](href)`:
/// `href == text`, `mailto:x` with text `x`, or `http://www.x` with text
/// `www.x` (the forward direction's `www.` href expansion).
fn is_bare_anchor(content: &str, href: &str) -> bool {
    if content == href {
        return true;
    }
    if let Some(addr) = href.strip_prefix("mailto:") {
        if addr == content {
            return true;
        }
    }
    if content.starts_with("www.") && href == format!("http://{}", content) {
        return true;
    }
    false
}

/// Conversion options for HTML → Markdown.
///
/// `gfm: false` (the default) keeps CommonMark-compatible output: `<del>` /
/// `<s>` unwrap to plain text, checkbox inputs vanish, every link keeps
/// its `[text](href)` form, footnote sections flatten to plain content,
/// `<dl>`/`<dt>`/`<dd>` unwrap to plain content and math passthrough spans
/// unwrap to plain content. `gfm: true` renders `<del>`/`<s>`/`<strike>`
/// as `~~`, checkbox inputs as `[ ]`/`[x]` prefixes, already-bare anchors
/// as plain URLs/emails, footnote reference `<sup>` elements as `[^N]` with
/// their `<section class="footnotes">` footer back as `[^N]: …`
/// definitions, `<dl>` elements as term / `: description` definition lists,
/// and `<span class="math-inline">` / `<div class="math-display">` back as
/// `$…$` / `$$…$$` dollar math.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    /// Enable GFM extensions in the reverse direction.
    pub gfm: bool,
}

impl Options {
    /// Options with GFM extensions enabled.
    pub fn gfm() -> Self {
        Options { gfm: true }
    }
}

/// Convert an HTML string to Markdown.
///
/// CommonMark-compatible reverse direction (see [`Options`] for the GFM
/// boundary).
///
/// Supports headings, paragraphs, bold/italic, code spans and blocks, links,
/// ordered and unordered lists, blockquotes, horizontal rules, GFM pipe
/// tables, `<br>`, `<img>`, GFM footnotes and definition lists (see
/// [`Options`]), and HTML entity unescaping.
///
/// # Examples
///
/// ```
/// let md = pagina::html_to_markdown::convert("<h1>Hello</h1>").unwrap();
/// assert_eq!(md, "# Hello\n");
/// ```
pub fn convert(input: &str) -> Result<String> {
    convert_with(input, Options::default())
}

/// Convert HTML to Markdown with explicit [`Options`].
pub fn convert_with(input: &str, options: Options) -> Result<String> {
    let mut tokenizer = HtmlTokenizer::new(input.to_string());
    let tokens = tokenizer.tokenize();
    let mut converter = MdConverter::with_gfm(tokens, options.gfm);
    let markdown = converter.convert();

    // Clean up excessive blank lines
    let lines: Vec<&str> = markdown.lines().collect();
    let mut cleaned = Vec::new();
    let mut prev_blank = false;

    for line in lines {
        let is_blank = line.trim().is_empty();
        if is_blank && prev_blank {
            continue;
        }
        cleaned.push(line);
        prev_blank = is_blank;
    }

    Ok(cleaned.join("\n").trim().to_string() + "\n")
}

/// Convert HTML to Markdown with GFM extensions enabled (see [`Options`]).
///
/// # Examples
///
/// ```
/// let md = pagina::html_to_markdown::convert_gfm("<del>hi</del>").unwrap();
/// assert_eq!(md, "~~hi~~\n");
/// ```
pub fn convert_gfm(input: &str) -> Result<String> {
    convert_with(input, Options::gfm())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heading() {
        let md = convert("<h1>Heading</h1>").unwrap();
        assert_eq!(md, "# Heading\n");
    }

    #[test]
    fn test_paragraph() {
        let md = convert("<p>Hello world</p>").unwrap();
        assert_eq!(md, "Hello world\n");
    }

    #[test]
    fn test_bold_italic() {
        let md = convert("<p><strong>bold</strong> <em>italic</em></p>").unwrap();
        assert!(md.contains("**bold**"));
        assert!(md.contains("*italic*"));
    }

    #[test]
    fn test_code() {
        let md = convert("<p><code>code</code></p>").unwrap();
        assert_eq!(md, "`code`\n");
    }

    #[test]
    fn test_pre_code() {
        let md = convert("<pre><code>code block\nline 2</code></pre>").unwrap();
        assert!(md.contains("```"));
        assert!(md.contains("code block"));
    }

    #[test]
    fn test_link() {
        let md = convert("<p><a href=\"url\">link</a></p>").unwrap();
        assert_eq!(md, "[link](url)\n");
    }

    #[test]
    fn test_unordered_list() {
        let md = convert("<ul><li>item 1</li><li>item 2</li></ul>").unwrap();
        assert!(md.contains("- item 1"));
        assert!(md.contains("- item 2"));
    }

    #[test]
    fn test_ordered_list() {
        let md = convert("<ol><li>item 1</li><li>item 2</li></ol>").unwrap();
        assert!(md.contains("1. item 1"));
        assert!(md.contains("2. item 2"));
    }

    #[test]
    fn test_ordered_list_start() {
        let md = convert("<ol start=\"5\"><li>item 1</li><li>item 2</li></ol>").unwrap();
        assert!(md.contains("5. item 1"));
        assert!(md.contains("6. item 2"));
    }

    #[test]
    fn test_blockquote() {
        let md = convert("<blockquote>quote line 1\nquote line 2</blockquote>").unwrap();
        // Newlines within a single text node are collapsed by whitespace normalization
        assert!(md.contains("> quote line 1 quote line 2"));
    }

    #[test]
    fn test_html_entities() {
        let md = convert("<p>&lt;script&gt; &amp; &quot;test&quot;</p>").unwrap();
        // Entities are properly unescaped to their literal characters
        assert!(md.contains("<script>"));
        assert!(md.contains("&"));
        assert!(md.contains("\"test\""));
    }

    #[test]
    fn test_self_closing() {
        let md = convert("<p>line 1<br/>line 2</p>").unwrap();
        assert!(md.contains("line 1"));
        assert!(md.contains("line 2"));
    }

    #[test]
    fn test_nested_inline() {
        let md = convert("<p><strong>bold <em>italic</em> bold</strong></p>").unwrap();
        // Whitespace between inline elements is normalized by the tokenizer
        assert!(md.contains("**bold*italic*bold**"));
    }
}
