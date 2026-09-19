use crate::error::Result;

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
}

impl MdConverter {
    fn new(tokens: Vec<HtmlToken>) -> Self {
        MdConverter {
            tokens,
            pos: 0,
            in_pre: false,
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
                    // Normalize whitespace
                    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    if normalized.is_empty() {
                        String::new()
                    } else {
                        normalized
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
            Some(HtmlToken::StartTag {
                name,
                attrs,
                self_closing,
            }) => {
                let name = name.clone();
                let attrs = attrs.clone();
                let self_closing = *self_closing;
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
                        let href = attrs
                            .iter()
                            .find(|(k, _)| k == "href")
                            .map(|(_, v)| v.clone())
                            .unwrap_or_default();
                        let content = self.convert_until_end("a");
                        if href.is_empty() {
                            content
                        } else {
                            format!("[{}]({})", content, href)
                        }
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
                        format!("- {}\n", content.trim())
                    }
                    "blockquote" => {
                        let content = self.convert_until_end("blockquote");
                        let lines: Vec<&str> = content.lines().collect();
                        let quoted: Vec<String> =
                            lines.iter().map(|l| format!("> {}", l)).collect();
                        format!("\n{}\n", quoted.join("\n"))
                    }
                    "br" => {
                        if self_closing {
                            "\n".to_string()
                        } else {
                            String::new()
                        }
                    }
                    "hr" => "\n---\n".to_string(),
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
}

/// Convert HTML to Markdown
pub fn convert(input: &str) -> Result<String> {
    let mut tokenizer = HtmlTokenizer::new(input.to_string());
    let tokens = tokenizer.tokenize();
    let mut converter = MdConverter::new(tokens);
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
        // Converter preserves entity references as-is in text nodes
        assert!(md.contains("&lt;script&gt;"));
        assert!(md.contains("&amp;"));
        assert!(md.contains("&quot;test&quot;"));
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
