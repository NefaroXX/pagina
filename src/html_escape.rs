/// Escape HTML special characters in a string.
pub fn escape_html(input: &str) -> String {
    let mut output = String::with_capacity(input.len() + 16);
    for ch in input.chars() {
        match ch {
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '&' => output.push_str("&amp;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            c => output.push(c),
        }
    }
    output
}

/// Unescape HTML entities in a string.
/// Handles named entities (< > & " ' &apos;) and numeric entities (&#nnn; &#xhhh;).
pub fn unescape_html(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut i = 0;
    let chars: Vec<char> = input.chars().collect();

    while i < chars.len() {
        if chars[i] == '&' {
            // Look for entity ending with ';'
            let mut j = i + 1;
            while j < chars.len() && chars[j] != ';' && (j - i) < 10 {
                j += 1;
            }

            if j < chars.len() && chars[j] == ';' {
                let entity: String = chars[i..=j].iter().collect();

                let replacement = match entity.as_str() {
                    "&lt;" => Some('<'),
                    "&gt;" => Some('>'),
                    "&amp;" => Some('&'),
                    "&quot;" => Some('"'),
                    "&#39;" => Some('\''),
                    "&apos;" => Some('\''),
                    _ if entity.starts_with("&#x") || entity.starts_with("&#X") => {
                        // Hexadecimal numeric entity
                        let hex_str = &entity[3..entity.len() - 1];
                        u32::from_str_radix(hex_str, 16)
                            .ok()
                            .and_then(char::from_u32)
                    }
                    _ if entity.starts_with("&#") => {
                        // Decimal numeric entity
                        let dec_str = &entity[2..entity.len() - 1];
                        dec_str.parse::<u32>().ok().and_then(char::from_u32)
                    }
                    _ => None,
                };

                if let Some(ch) = replacement {
                    output.push(ch);
                    i = j + 1;
                    continue;
                }
            }
        }
        output.push(chars[i]);
        i += 1;
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
        assert_eq!(escape_html("a & b"), "a &amp; b");
        assert_eq!(escape_html("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(escape_html("'single'"), "&#39;single&#39;");
        assert_eq!(escape_html("normal"), "normal");
    }

    #[test]
    fn test_unescape_html() {
        assert_eq!(unescape_html("&lt;script&gt;"), "<script>");
        assert_eq!(unescape_html("a &amp; b"), "a & b");
        assert_eq!(unescape_html("&quot;quoted&quot;"), "\"quoted\"");
        assert_eq!(unescape_html("&#39;single&#39;"), "'single'");
        assert_eq!(unescape_html("&apos;apos'"), "'apos'");
        assert_eq!(unescape_html("&#65;"), "A");
        assert_eq!(unescape_html("&#x41;"), "A");
        assert_eq!(unescape_html("normal"), "normal");
    }

    #[test]
    fn test_roundtrip() {
        let original = "<hello> & 'world' \"test\"";
        let escaped = escape_html(original);
        let unescaped = unescape_html(&escaped);
        assert_eq!(original, unescaped);
    }
}
