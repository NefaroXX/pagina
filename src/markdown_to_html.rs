use crate::error::Result;
use crate::html_escape::escape_html;
use crate::inline_parser::{parse_inline, render_inline_html};

#[derive(Debug, Clone, PartialEq)]
enum BlockState {
    Paragraph,
    Heading(u8),
    UnorderedList,
    OrderedList(u32),
    Blockquote,
    CodeFence(String), // fence string like "```" or "~~~"
    None,
}

struct BlockContext {
    state: BlockState,
    in_paragraph: bool,
    pending_lines: Vec<String>,
}

impl BlockContext {
    fn new() -> Self {
        BlockContext {
            state: BlockState::None,
            in_paragraph: false,
            pending_lines: Vec::new(),
        }
    }

    fn close_block(&mut self, output: &mut String) {
        match &self.state {
            BlockState::Paragraph => {
                if !self.pending_lines.is_empty() {
                    let content = self.pending_lines.join("\n");
                    let elements = parse_inline(&content);
                    output.push_str("<p>");
                    output.push_str(&render_inline_html(&elements));
                    output.push_str("</p>\n");
                }
            }
            BlockState::Heading(level) => {
                if !self.pending_lines.is_empty() {
                    let content = self.pending_lines.join("\n");
                    let elements = parse_inline(&content);
                    output.push_str(&format!("<h{}>", level));
                    output.push_str(&render_inline_html(&elements));
                    output.push_str(&format!("</h{}>\n", level));
                }
            }
            BlockState::UnorderedList => {
                output.push_str("</ul>\n");
            }
            BlockState::OrderedList(_) => {
                output.push_str("</ol>\n");
            }
            BlockState::Blockquote => {
                output.push_str("</blockquote>\n");
            }
            BlockState::CodeFence(_) => {
                output.push_str("</code></pre>\n");
            }
            BlockState::None => {}
        }
        self.state = BlockState::None;
        self.pending_lines.clear();
        self.in_paragraph = false;
    }

    fn close_to_state(&mut self, target: &BlockState, output: &mut String) {
        // Close current block if different from target
        if std::mem::discriminant(&self.state) != std::mem::discriminant(target) {
            self.close_block(output);
        }
    }
}

/// Convert a Markdown string to HTML.
///
/// Supports ATX headings (`#`-`######`), bold (`**`/`__`), italic (`*`/`_`),
/// code spans, fenced code blocks, links, ordered and unordered lists,
/// blockquotes, horizontal rules (`---`/`***`/`___`), and HTML escaping.
///
/// # Examples
///
/// ```
/// let html = md2html::markdown_to_html::convert("# Hello").unwrap();
/// assert_eq!(html, "<h1>Hello</h1>\n");
/// ```
pub fn convert(input: &str) -> Result<String> {
    let mut output = String::new();
    let mut ctx = BlockContext::new();
    let lines: Vec<&str> = input.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();

        // Check for blank line
        if trimmed.is_empty() {
            // Close any open block on blank lines (not just paragraphs)
            if !matches!(ctx.state, BlockState::None) {
                ctx.close_block(&mut output);
            }
            i += 1;
            continue;
        }

        // Check for code fence
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let fence = if trimmed.starts_with("```") {
                "```"
            } else {
                "~~~"
            };
            if matches!(ctx.state, BlockState::CodeFence(ref f) if f == fence) {
                // Closing fence
                ctx.close_block(&mut output);
            } else {
                // Opening fence
                ctx.close_block(&mut output);
                ctx.state = BlockState::CodeFence(fence.to_string());
                output.push_str("<pre><code>");
                // Skip the fence line, don't include it in output
            }
            i += 1;
            continue;
        }

        // If we're in a code fence, output raw content
        if matches!(ctx.state, BlockState::CodeFence(_)) {
            output.push_str(&escape_html(line));
            output.push('\n');
            i += 1;
            continue;
        }

        // Check for heading
        if trimmed.starts_with('#') {
            let mut level = 0;
            for ch in trimmed.chars() {
                if ch == '#' && level < 6 {
                    level += 1;
                } else {
                    break;
                }
            }
            if level > 0
                && (trimmed.len() == level as usize
                    || trimmed.chars().nth(level as usize) == Some(' '))
            {
                ctx.close_block(&mut output);
                ctx.state = BlockState::Heading(level);
                let content = trimmed[level as usize..].trim_start();
                ctx.pending_lines.push(content.to_string());
                ctx.close_block(&mut output); // Headings are single-line
                i += 1;
                continue;
            }
        }

        // Check for blockquote
        if let Some(stripped) = trimmed.strip_prefix('>') {
            if !matches!(ctx.state, BlockState::Blockquote) {
                ctx.close_to_state(&BlockState::Blockquote, &mut output);
                output.push_str("<blockquote>\n");
                ctx.state = BlockState::Blockquote;
            }
            let content = stripped.trim_start();
            let elements = parse_inline(content);
            output.push_str(&render_inline_html(&elements));
            output.push('\n');
            i += 1;
            continue;
        }

        // Check for unordered list
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            ctx.close_to_state(&BlockState::UnorderedList, &mut output);
            if !matches!(ctx.state, BlockState::UnorderedList) {
                ctx.state = BlockState::UnorderedList;
                output.push_str("<ul>\n");
            }
            let content = trimmed[2..].trim_start();
            let elements = parse_inline(content);
            output.push_str("<li>");
            output.push_str(&render_inline_html(&elements));
            output.push_str("</li>\n");
            i += 1;
            continue;
        }

        // Check for ordered list
        if let Some(dot_pos) = trimmed.find('.') {
            let num_str = &trimmed[..dot_pos];
            if num_str.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(start_num) = num_str.parse::<u32>() {
                    if dot_pos + 1 < trimmed.len() && trimmed.chars().nth(dot_pos + 1) == Some(' ')
                    {
                        ctx.close_to_state(&BlockState::OrderedList(start_num), &mut output);
                        if !matches!(ctx.state, BlockState::OrderedList(_)) {
                            ctx.state = BlockState::OrderedList(start_num);
                            if start_num != 1 {
                                output.push_str(&format!("<ol start=\"{}\">\n", start_num));
                            } else {
                                output.push_str("<ol>\n");
                            }
                        }
                        let content = trimmed[dot_pos + 2..].trim_start();
                        let elements = parse_inline(content);
                        output.push_str("<li>");
                        output.push_str(&render_inline_html(&elements));
                        output.push_str("</li>\n");
                        i += 1;
                        continue;
                    }
                }
            }
        }

        // Check for horizontal rule (---, ***, ___)
        if (trimmed.starts_with("---") || trimmed.starts_with("***") || trimmed.starts_with("___"))
            && trimmed
                .chars()
                .all(|c| c == trimmed.chars().next().unwrap())
            && trimmed.len() >= 3
        {
            ctx.close_block(&mut output);
            output.push_str("<hr>\n");
            i += 1;
            continue;
        }

        // Regular paragraph text
        ctx.close_to_state(&BlockState::Paragraph, &mut output);
        if !matches!(ctx.state, BlockState::Paragraph) {
            ctx.state = BlockState::Paragraph;
            ctx.in_paragraph = true;
        }
        ctx.pending_lines.push(line.to_string());
        i += 1;
    }

    // Close any remaining open block
    ctx.close_block(&mut output);

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heading() {
        let html = convert("# Heading 1").unwrap();
        assert_eq!(html, "<h1>Heading 1</h1>\n");

        let html = convert("## Heading 2").unwrap();
        assert_eq!(html, "<h2>Heading 2</h2>\n");
    }

    #[test]
    fn test_paragraph() {
        let html = convert("Hello world").unwrap();
        assert_eq!(html, "<p>Hello world</p>\n");
    }

    #[test]
    fn test_bold_italic() {
        let html = convert("**bold** *italic*").unwrap();
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<em>italic</em>"));
    }

    #[test]
    fn test_code_span() {
        let html = convert("`code`").unwrap();
        assert_eq!(html, "<p><code>code</code></p>\n");
    }

    #[test]
    fn test_link() {
        let html = convert("[link](url)").unwrap();
        assert!(html.contains("<a href=\"url\">link</a>"));
    }

    #[test]
    fn test_unordered_list() {
        let html = convert("- item 1\n- item 2").unwrap();
        assert_eq!(html, "<ul>\n<li>item 1</li>\n<li>item 2</li>\n</ul>\n");
    }

    #[test]
    fn test_ordered_list() {
        let html = convert("1. item 1\n2. item 2").unwrap();
        assert_eq!(html, "<ol>\n<li>item 1</li>\n<li>item 2</li>\n</ol>\n");
    }

    #[test]
    fn test_blockquote() {
        let html = convert("> quote").unwrap();
        assert_eq!(html, "<blockquote>\nquote\n</blockquote>\n");
    }

    #[test]
    fn test_code_fence() {
        let html = convert("```\ncode\n```").unwrap();
        assert_eq!(html, "<pre><code>code\n</code></pre>\n");
    }

    #[test]
    fn test_html_escaping() {
        let html = convert("<script>alert(1)</script>").unwrap();
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn test_multiple_paragraphs() {
        let html = convert("para 1\n\npara 2").unwrap();
        assert_eq!(html, "<p>para 1</p>\n<p>para 2</p>\n");
    }
}
