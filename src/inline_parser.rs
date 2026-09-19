use crate::html_escape::escape_html;

#[derive(Debug, Clone, PartialEq)]
pub enum InlineElement {
    Text(String),
    Bold(Vec<InlineElement>),
    Italic(Vec<InlineElement>),
    Code(String),
    Link {
        text: Vec<InlineElement>,
        url: String,
    },
}

impl InlineElement {
    fn to_html(&self) -> String {
        match self {
            InlineElement::Text(s) => escape_html(s),
            InlineElement::Bold(children) => {
                let inner: String = children.iter().map(|c| c.to_html()).collect();
                format!("<strong>{}</strong>", inner)
            }
            InlineElement::Italic(children) => {
                let inner: String = children.iter().map(|c| c.to_html()).collect();
                format!("<em>{}</em>", inner)
            }
            InlineElement::Code(s) => format!("<code>{}</code>", escape_html(s)),
            InlineElement::Link { text, url } => {
                let inner: String = text.iter().map(|c| c.to_html()).collect();
                format!("<a href=\"{}\">{}</a>", escape_html(url), inner)
            }
        }
    }

    fn to_markdown(&self) -> String {
        match self {
            InlineElement::Text(s) => s.clone(),
            InlineElement::Bold(children) => {
                let inner: String = children.iter().map(|c| c.to_markdown()).collect();
                format!("**{}**", inner)
            }
            InlineElement::Italic(children) => {
                let inner: String = children.iter().map(|c| c.to_markdown()).collect();
                format!("*{}*", inner)
            }
            InlineElement::Code(s) => format!("`{}`", s),
            InlineElement::Link { text, url } => {
                let inner: String = text.iter().map(|c| c.to_markdown()).collect();
                format!("[{}]({})", inner, url)
            }
        }
    }
}

/// Parse inline Markdown elements from a string.
/// Handles: **bold**, *italic*, `code`, [text](url)
pub fn parse_inline(input: &str) -> Vec<InlineElement> {
    let mut result = Vec::new();
    let mut i = 0;
    let chars: Vec<char> = input.chars().collect();

    while i < chars.len() {
        // Check for code spans (backticks)
        if chars[i] == '`' {
            let mut j = i + 1;
            let mut backtick_count = 1;
            while j < chars.len() && chars[j] == '`' && backtick_count < 3 {
                backtick_count += 1;
                j += 1;
            }

            // Find closing backticks
            let mut k = j;
            let mut found = false;
            while k + backtick_count <= chars.len() {
                let mut match_count = 0;
                while match_count < backtick_count
                    && k + match_count < chars.len()
                    && chars[k + match_count] == '`'
                {
                    match_count += 1;
                }
                if match_count == backtick_count {
                    found = true;
                    break;
                }
                k += 1;
            }

            if found {
                let code_content: String = chars[j..k].iter().collect();
                result.push(InlineElement::Code(code_content));
                i = k + backtick_count;
                continue;
            }
        }

        // Check for links [text](url)
        if chars[i] == '[' {
            let mut bracket_depth = 1;
            let mut j = i + 1;
            let mut link_text_end = None;

            while j < chars.len() && bracket_depth > 0 {
                if chars[j] == '[' {
                    bracket_depth += 1;
                } else if chars[j] == ']' {
                    bracket_depth -= 1;
                    if bracket_depth == 0 {
                        link_text_end = Some(j);
                        break;
                    }
                }
                j += 1;
            }

            if let Some(text_end) = link_text_end {
                // Check for (url) immediately after
                if text_end + 1 < chars.len() && chars[text_end + 1] == '(' {
                    let mut k = text_end + 2;
                    let mut paren_depth = 1;
                    let mut url_end = None;

                    while k < chars.len() && paren_depth > 0 {
                        if chars[k] == '(' {
                            paren_depth += 1;
                        } else if chars[k] == ')' {
                            paren_depth -= 1;
                            if paren_depth == 0 {
                                url_end = Some(k);
                                break;
                            }
                        }
                        k += 1;
                    }

                    if let Some(url_end_pos) = url_end {
                        let link_text: String = chars[i + 1..text_end].iter().collect();
                        let url: String = chars[text_end + 2..url_end_pos].iter().collect();

                        let parsed_text = parse_inline(&link_text);
                        result.push(InlineElement::Link {
                            text: parsed_text,
                            url,
                        });
                        i = url_end_pos + 1;
                        continue;
                    }
                }
            }
        }

        // Check for bold (**)
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            let mut j = i + 2;
            let mut found = false;

            while j + 1 < chars.len() {
                if chars[j] == '*' && chars[j + 1] == '*' {
                    found = true;
                    break;
                }
                j += 1;
            }

            if found {
                let inner_text: String = chars[i + 2..j].iter().collect();
                let parsed_inner = parse_inline(&inner_text);
                result.push(InlineElement::Bold(parsed_inner));
                i = j + 2;
                continue;
            }
        }

        // Check for italic (*) - but not **
        if chars[i] == '*' && (i + 1 >= chars.len() || chars[i + 1] != '*') {
            let mut j = i + 1;
            let mut found = false;

            while j < chars.len() {
                if chars[j] == '*' && (j + 1 >= chars.len() || chars[j + 1] != '*') {
                    found = true;
                    break;
                }
                j += 1;
            }

            if found {
                let inner_text: String = chars[i + 1..j].iter().collect();
                let parsed_inner = parse_inline(&inner_text);
                result.push(InlineElement::Italic(parsed_inner));
                i = j + 1;
                continue;
            }
        }

        // Regular text - accumulate until next special character
        let start = i;
        while i < chars.len() {
            let ch = chars[i];
            let is_special = matches!(ch, '*' | '`' | '[');
            if is_special {
                break;
            }
            i += 1;
        }

        if i > start {
            let text: String = chars[start..i].iter().collect();
            result.push(InlineElement::Text(text));
        } else if i < chars.len() {
            // Single special char that didn't form a pattern - treat as literal
            result.push(InlineElement::Text(chars[i].to_string()));
            i += 1;
        }
    }

    // Merge adjacent text nodes
    let mut merged = Vec::new();
    for elem in result {
        if let InlineElement::Text(ref s) = elem {
            if let Some(InlineElement::Text(ref mut last_s)) = merged.last_mut() {
                last_s.push_str(s);
            } else {
                merged.push(elem);
            }
        } else {
            merged.push(elem);
        }
    }

    merged
}

/// Render inline elements to HTML
pub fn render_inline_html(elements: &[InlineElement]) -> String {
    elements.iter().map(|e| e.to_html()).collect()
}

/// Render inline elements to Markdown
pub fn render_inline_md(elements: &[InlineElement]) -> String {
    elements.iter().map(|e| e.to_markdown()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bold() {
        let result = parse_inline("**bold**");
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], InlineElement::Bold(_)));
    }

    #[test]
    fn test_parse_italic() {
        let result = parse_inline("*italic*");
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], InlineElement::Italic(_)));
    }

    #[test]
    fn test_parse_code() {
        let result = parse_inline("`code`");
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], InlineElement::Code(_)));
    }

    #[test]
    fn test_parse_link() {
        let result = parse_inline("[link](url)");
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], InlineElement::Link { .. }));
    }

    #[test]
    fn test_nested() {
        let result = parse_inline("**bold *italic* bold**");
        assert_eq!(result.len(), 1);
        if let InlineElement::Bold(children) = &result[0] {
            assert_eq!(children.len(), 3);
            assert!(matches!(children[0], InlineElement::Text(_)));
            assert!(matches!(children[1], InlineElement::Italic(_)));
            assert!(matches!(children[2], InlineElement::Text(_)));
        }
    }

    #[test]
    fn test_render_html() {
        let elements = parse_inline("**bold** and *italic*");
        let html = render_inline_html(&elements);
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<em>italic</em>"));
    }

    #[test]
    fn test_render_md() {
        let elements = parse_inline("**bold** and *italic*");
        let md = render_inline_md(&elements);
        assert_eq!(md, "**bold** and *italic*");
    }

    #[test]
    fn test_link_roundtrip() {
        let original = "[link](https://example.com)";
        let parsed = parse_inline(original);
        let rendered = render_inline_md(&parsed);
        assert_eq!(original, rendered);
    }
}
