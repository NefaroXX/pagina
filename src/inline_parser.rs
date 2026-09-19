use crate::html_escape::{clean_url, decode_entity_token, escape_href, escape_html, unescape_html};
use std::collections::HashMap;

/// Reference definitions collected by the block layer.
/// Key = normalized label (lowercased, whitespace-collapsed).
/// Value = (destination, optional title).
pub type RefDefs = HashMap<String, (String, Option<String>)>;

/// Normalize a link reference label per CommonMark: strip, collapse
/// internal whitespace (including newlines) to single spaces, lowercase
/// with Unicode case folding (`ß`/`ẞ` match `SS`, ex 540).
pub fn normalize_label(label: &str) -> String {
    label
        .to_lowercase()
        .replace('ß', "ss")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, PartialEq)]
pub enum InlineElement {
    Text(String),
    Bold(Vec<InlineElement>),
    Italic(Vec<InlineElement>),
    Code(String),
    Link {
        text: Vec<InlineElement>,
        url: String,
        title: Option<String>,
    },
    Image {
        alt: String,
        url: String,
        title: Option<String>,
    },
    RawHtml(String),
    HardBreak,
    SoftBreak,
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
            InlineElement::Link { text, url, title } => {
                let inner: String = text.iter().map(|c| c.to_html()).collect();
                match title {
                    Some(t) if !t.is_empty() => format!(
                        "<a href=\"{}\" title=\"{}\">{}</a>",
                        escape_href(&clean_url(url)),
                        escape_href(t),
                        inner
                    ),
                    _ => format!("<a href=\"{}\">{}</a>", escape_href(&clean_url(url)), inner),
                }
            }
            InlineElement::Image { alt, url, title } => match title {
                Some(t) if !t.is_empty() => format!(
                    "<img src=\"{}\" alt=\"{}\" title=\"{}\" />",
                    escape_href(&clean_url(url)),
                    escape_html(alt),
                    escape_href(t)
                ),
                _ => format!(
                    "<img src=\"{}\" alt=\"{}\" />",
                    escape_href(&clean_url(url)),
                    escape_html(alt)
                ),
            },
            InlineElement::RawHtml(s) => s.clone(),
            InlineElement::HardBreak => "<br />\n".to_string(),
            InlineElement::SoftBreak => "\n".to_string(),
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
            InlineElement::Link { text, url, title } => {
                let inner: String = text.iter().map(|c| c.to_markdown()).collect();
                match title {
                    Some(t) if !t.is_empty() => format!("[{}]({} \"{}\")", inner, url, t),
                    _ => format!("[{}]({})", inner, url),
                }
            }
            InlineElement::Image { alt, url, .. } => format!("![{}]({})", alt, url),
            InlineElement::RawHtml(s) => s.clone(),
            InlineElement::HardBreak => "  \n".to_string(),
            InlineElement::SoftBreak => "\n".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tokenizer + delimiter-stack emphasis (CommonMark appendix algorithm)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Tok {
    Text(String),
    Delim { ch: char, len: usize, can_open: bool, can_close: bool },
    OpenBracket { image: bool },
    Code(String),
    RawHtml(String),
    HardBreak,
    SoftBreak,
    Link { children: Vec<Tok>, url: String, title: Option<String> },
    Image { children: Vec<Tok>, url: String, title: Option<String> },
}

fn is_ws(c: char) -> bool {
    c == '\n' || c.is_whitespace()
}

fn is_punct(c: char) -> bool {
    // Unicode-aware approximation: not alphanumeric, not whitespace.
    !(c.is_alphanumeric() || is_ws(c))
}

fn is_ascii_punct(c: char) -> bool {
    c.is_ascii_punctuation()
}

fn scan_entity(chars: &[char], i: usize) -> Option<(String, usize)> {
    if chars[i] != '&' {
        return None;
    }
    let mut j = i + 1;
    while j < chars.len() && chars[j] != ';' && j - i <= 33 {
        j += 1;
    }
    if j < chars.len() && chars[j] == ';' {
        let token: String = chars[i..=j].iter().collect();
        if let Some(rep) = decode_entity_token(&token) {
            return Some((rep, j + 1));
        }
    }
    None
}

fn scan_backtick_code(chars: &[char], i: usize) -> Option<(String, usize)> {
    if chars[i] != '`' {
        return None;
    }
    let mut open_len = 0;
    while i + open_len < chars.len() && chars[i + open_len] == '`' {
        open_len += 1;
    }
    // Find closing run of exactly open_len backticks.
    let mut j = i + open_len;
    loop {
        while j < chars.len() && chars[j] != '`' {
            j += 1;
        }
        if j >= chars.len() {
            return None;
        }
        let mut close_len = 0;
        while j + close_len < chars.len() && chars[j + close_len] == '`' {
            close_len += 1;
        }
        if close_len == open_len {
            let raw: String = chars[i + open_len..j].iter().collect();
            // Line endings -> spaces; strip one leading+trailing space if
            // content is not all spaces.
            let mut content = raw.replace("\r\n", " ").replace(['\r', '\n'], " ");
            if content.starts_with(' ')
                && content.ends_with(' ')
                && content.chars().any(|c| c != ' ')
            {
                content = content[1..content.len() - 1].to_string();
            }
            return Some((content, j + close_len));
        }
        j += close_len.max(1);
    }
}

/// Try to parse `<...>` at position i: autolink (URI/email), or raw HTML.
/// Returns (Tok, next_index).
fn scan_angle(chars: &[char], i: usize) -> Option<(Tok, usize)> {
    if chars[i] != '<' {
        return None;
    }
    let mut j = i + 1;
    while j < chars.len() && chars[j] != '>' && chars[j] != '\n' && chars[j] != '\r' {
        j += 1;
    }
    if j >= chars.len() || chars[j] != '>' {
        // Might still be raw HTML spanning? Inline tags cannot contain
        // newlines in the `<...>` scan for autolinks, but tags can span?
        // Per spec, tags are single-line-ish; give up and try tag parse below.
    } else {
        let inner: String = chars[i + 1..j].iter().collect();
        if is_autolink_uri(&inner) {
            let url = inner.clone();
            let tok = Tok::Link {
                children: vec![Tok::Text(url.clone())],
                url,
                title: None,
            };
            return Some((tok, j + 1));
        }
        if is_autolink_email(&inner) {
            let disp = inner.clone();
            let tok = Tok::Link {
                children: vec![Tok::Text(disp)],
                url: format!("mailto:{}", inner),
                title: None,
            };
            return Some((tok, j + 1));
        }
    }
    // Raw HTML: comment, pi, declaration, cdata, open/close tag.
    if let Some(end) = scan_raw_tag(chars, i) {
        let raw: String = chars[i..end].iter().collect();
        return Some((Tok::RawHtml(raw), end));
    }
    None
}

fn is_autolink_uri(s: &str) -> bool {
    let colon = match s.find(':') {
        Some(p) => p,
        None => return false,
    };
    let scheme = &s[..colon];
    if scheme.len() < 2 || scheme.len() > 32 {
        return false;
    }
    let mut it = scheme.chars();
    match it.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    if !scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-') {
        return false;
    }
    let rest = &s[colon + 1..];
    if rest.is_empty() {
        return false;
    }
    !rest.chars().any(|c| c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '<' || c == '>')
}

fn is_autolink_email(s: &str) -> bool {
    // CommonMark email autolink regex (simplified, case-insensitive).
    let at = match s.rfind('@') {
        Some(p) => p,
        None => return false,
    };
    let local = &s[..at];
    let domain = &s[at + 1..];
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    if !local.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || ".!#$%&'*+/=?^_`{|}~-".contains(c)
    }) {
        return false;
    }
    if !domain.contains('.') {
        return false;
    }
    if !domain
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' || c == '+')
    {
        return false;
    }
    let last_label = domain.rsplit('.').next().unwrap_or("");
    if last_label.len() < 2 || !last_label.chars().all(|c| c.is_ascii_alphabetic()) {
        // Allow single-char? Spec requires TLD-ish; keep 2+ alpha check but
        // fall back: allow anything with a dot and alnum labels.
        if !last_label.chars().all(|c| c.is_ascii_alphanumeric()) || last_label.is_empty() {
            return false;
        }
    }
    if s.chars().any(|c| c == ' ' || c == '\t' || c == '<' || c == '>') {
        return false;
    }
    true
}

/// Skip tag whitespace: spaces/tabs plus at most one line ending.
/// Returns (new position, whether any whitespace was consumed).
fn skip_tag_ws(b: &[u8], mut k: usize) -> (usize, bool) {
    let start = k;
    while k < b.len() && (b[k] == b' ' || b[k] == b'\t') {
        k += 1;
    }
    // At most one line ending.
    if k < b.len() && b[k] == b'\r' {
        k += 1;
        if k < b.len() && b[k] == b'\n' {
            k += 1;
        }
    } else if k < b.len() && b[k] == b'\n' {
        k += 1;
    }
    while k < b.len() && (b[k] == b' ' || b[k] == b'\t') {
        k += 1;
    }
    (k, k > start)
}

/// Strict CommonMark open/closing-tag scanner.
/// `s` starts at `<`. Returns the byte length through the closing `>`.
/// Grammar (from the spec): tag name = ASCII letter + letters/digits/`-`;
/// attributes are whitespace-separated with XML-style names; `/` must be
/// immediately followed by `>`; closing tags take no attributes.
pub fn tag_end_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.is_empty() || b[0] != b'<' {
        return None;
    }
    let mut k = 1usize;
    let closing = if k < b.len() && b[k] == b'/' {
        k += 1;
        true
    } else {
        false
    };
    // Tag name.
    if k >= b.len() || !b[k].is_ascii_alphabetic() {
        return None;
    }
    k += 1;
    while k < b.len() && (b[k].is_ascii_alphanumeric() || b[k] == b'-') {
        k += 1;
    }
    if closing {
        let (nk, _) = skip_tag_ws(b, k);
        if nk < b.len() && b[nk] == b'>' {
            return Some(nk + 1);
        }
        return None;
    }
    // Attributes.
    loop {
        let (nk, had_ws) = skip_tag_ws(b, k);
        if nk >= b.len() {
            return None;
        }
        if b[nk] == b'>' {
            return Some(nk + 1);
        }
        if b[nk] == b'/' {
            if nk + 1 < b.len() && b[nk + 1] == b'>' {
                return Some(nk + 2);
            }
            return None;
        }
        // Attributes must be whitespace-separated (ex 622).
        if !had_ws {
            return None;
        }
        k = nk;
        // Attribute name per spec: letter/underscore/colon + alnum/_/dot/colon/dash.
        if !(b[k].is_ascii_alphabetic() || b[k] == b'_' || b[k] == b':') {
            return None;
        }
        k += 1;
        while k < b.len()
            && (b[k].is_ascii_alphanumeric() || matches!(b[k], b'_' | b'.' | b':' | b'-'))
        {
            k += 1;
        }
        // Optional value specification.
        let (nk2, _) = skip_tag_ws(b, k);
        if nk2 < b.len() && b[nk2] == b'=' {
            k = nk2 + 1;
            let (nk3, _) = skip_tag_ws(b, k);
            k = nk3;
            if k >= b.len() {
                return None;
            }
            if b[k] == b'"' || b[k] == b'\'' {
                let q = b[k];
                k += 1;
                while k < b.len() && b[k] != q {
                    k += 1;
                }
                if k >= b.len() {
                    return None;
                }
                k += 1;
            } else {
                let vs = k;
                while k < b.len()
                    && !matches!(
                        b[k],
                        b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'\'' | b'=' | b'<' | b'>' | b'`'
                    )
                {
                    k += 1;
                }
                if k == vs {
                    return None;
                }
            }
        }
        // Else bare attribute; loop continues (whitespace re-skipped there).
    }
}

fn scan_raw_tag(chars: &[char], i: usize) -> Option<usize> {
    // chars[i] == '<'
    let s: String = chars[i..].iter().collect();
    let rest = &s[1..];
    // Comment
    if rest.starts_with("!--") {
        if let Some(p) = s.find("-->") {
            return Some(i + p + 3);
        }
        return None;
    }
    // Processing instruction
    if rest.starts_with('?') {
        if let Some(p) = s.find("?>") {
            return Some(i + p + 2);
        }
        return None;
    }
    // Declaration / CDATA
    if rest.starts_with('!') {
        if rest.starts_with("![CDATA[") {
            if let Some(p) = s.find("]]>") {
                return Some(i + p + 3);
            }
            return None;
        }
        // <!A ...> declaration: '!' + ASCII letter
        let after = rest[1..].chars().next()?;
        if after.is_ascii_alphabetic() {
            // find closing '>'
            let bytes = s.as_bytes();
            let mut k = 1usize;
            while k < bytes.len() {
                if bytes[k] == b'>' {
                    return Some(i + k + 1);
                }
                k += 1;
            }
            return None;
        }
        return None;
    }
    // Open or closing tag: strict CommonMark tag grammar.
    if let Some(len) = tag_end_len(&s) {
        return Some(i + len);
    }
    None
}

/// Parse the tail of an inline link starting at `(`.
/// Returns (destination, title, next_index).
fn parse_link_tail(chars: &[char], i: usize) -> Option<(String, Option<String>, usize)> {
    // chars[i] == '('
    let mut j = i + 1;
    // skip spaces/tabs/newlines (up to... allow)
    while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t' || chars[j] == '\n') {
        j += 1;
    }
    if j < chars.len() && chars[j] == ')' {
        return Some((String::new(), None, j + 1));
    }
    // Destination
    let mut dest = String::new();
    if j < chars.len() && chars[j] == '<' {
        j += 1;
        while j < chars.len() && chars[j] != '>' {
            if chars[j] == '\n' || chars[j] == '\r' || chars[j] == '<' {
                return None;
            }
            if chars[j] == '\\' && j + 1 < chars.len() && is_ascii_punct(chars[j + 1]) {
                dest.push(chars[j + 1]);
                j += 2;
            } else {
                dest.push(chars[j]);
                j += 1;
            }
        }
        if j >= chars.len() || chars[j] != '>' {
            return None;
        }
        j += 1;
    } else {
        let mut depth = 0usize;
        let mut any = false;
        while j < chars.len() {
            let c = chars[j];
            if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
                break;
            }
            if c == '(' {
                depth += 1;
                dest.push(c);
                any = true;
                j += 1;
            } else if c == ')' {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                dest.push(c);
                any = true;
                j += 1;
            } else if c == '\\' && j + 1 < chars.len() && is_ascii_punct(chars[j + 1]) {
                dest.push(chars[j + 1]);
                any = true;
                j += 2;
            } else if c.is_control() {
                return None;
            } else {
                dest.push(c);
                any = true;
                j += 1;
            }
        }
        if !any {
            return None;
        }
    }
    // skip spaces
    let mut k = j;
    while k < chars.len() && (chars[k] == ' ' || chars[k] == '\t' || chars[k] == '\n') {
        k += 1;
    }
    if k < chars.len() && chars[k] == ')' {
        return Some((dest, None, k + 1));
    }
    // Title must be separated by whitespace
    if k == j {
        return None;
    }
    let (title, after) = parse_link_title(chars, k)?;
    k = after;
    while k < chars.len() && (chars[k] == ' ' || chars[k] == '\t' || chars[k] == '\n') {
        k += 1;
    }
    if k < chars.len() && chars[k] == ')' {
        return Some((dest, Some(title), k + 1));
    }
    None
}

fn parse_link_title(chars: &[char], i: usize) -> Option<(String, usize)> {
    if i >= chars.len() {
        return None;
    }
    let q = chars[i];
    let closing = match q {
        '"' => '"',
        '\'' => '\'',
        '(' => ')',
        _ => return None,
    };
    let mut j = i + 1;
    let mut title = String::new();
    while j < chars.len() {
        let c = chars[j];
        if c == closing {
            if closing == ')' && title.contains('(') {
                // paren titles cannot contain unescaped '(' ... keep simple: allow
            }
            return Some((title, j + 1));
        }
        if c == '\n' && closing == '(' {
            return None;
        }
        if c == '\\' && j + 1 < chars.len() && is_ascii_punct(chars[j + 1]) {
            title.push(chars[j + 1]);
            j += 2;
        } else {
            title.push(c);
            j += 1;
        }
    }
    None
}

/// Parse `[label]` at position i (chars[i] == '['). Returns (label, next).
fn parse_bracket_label(chars: &[char], i: usize) -> Option<(String, usize)> {
    if chars[i] != '[' {
        return None;
    }
    let mut j = i + 1;
    let mut label = String::new();
    // Backslash escapes are kept literal (ex 549: `[ref\[]` is a valid
    // label); an unescaped `[` forbids the label.
    while j < chars.len() && chars[j] != ']' {
        if chars[j] == '\\' && j + 1 < chars.len() {
            label.push(chars[j]);
            label.push(chars[j + 1]);
            j += 2;
            continue;
        }
        if chars[j] == '[' {
            return None;
        }
        label.push(chars[j]);
        if label.len() > 999 {
            return None;
        }
        j += 1;
    }
    if j >= chars.len() || chars[j] != ']' {
        return None;
    }
    Some((label, j + 1))
}

struct Bracket {
    tok_idx: usize,
    image: bool,
    active: bool,
    /// Char index just after the opening `[` (raw label starts here).
    content_start: usize,
}

fn tokenize(input: &str, refs: &RefDefs) -> Vec<Tok> {
    let chars: Vec<char> = input.chars().collect();
    let mut toks: Vec<Tok> = Vec::new();
    let mut brackets: Vec<Bracket> = Vec::new();
    let mut i = 0usize;
    let mut text_buf = String::new();

    let flush_text = |toks: &mut Vec<Tok>, buf: &mut String| {
        if !buf.is_empty() {
            toks.push(Tok::Text(std::mem::take(buf)));
        }
    };

    // previous char for flanking (None at start); track last emitted char.
    let mut prev_char: Option<char> = None;

    while i < chars.len() {
        let c = chars[i];
        // Newline -> soft/hard break
        if c == '\n' || c == '\r' {
            // Hard break if line ends with 2+ spaces; trailing whitespace is
            // stripped from the text (it is not rendered).
            let trailing_spaces = text_buf.chars().rev().take_while(|x| *x == ' ' || *x == '\t').count();
            let trimmed_len = text_buf.len() - text_buf.chars().rev().take_while(|x| *x == ' ' || *x == '\t').map(|x| x.len_utf8()).sum::<usize>();
            text_buf.truncate(trimmed_len);
            flush_text(&mut toks, &mut text_buf);
            if trailing_spaces >= 2 {
                toks.push(Tok::HardBreak);
            } else {
                toks.push(Tok::SoftBreak);
            }
            // skip \r\n as one
            if c == '\r' && i + 1 < chars.len() && chars[i + 1] == '\n' {
                i += 1;
            }
            i += 1;
            prev_char = Some('\n');
            continue;
        }
        // Code span
        if c == '`' {
            if let Some((content, next)) = scan_backtick_code(&chars, i) {
                flush_text(&mut toks, &mut text_buf);
                toks.push(Tok::Code(content));
                // update prev_char to last char of code? Use '`'-ish: set to 'x'
                prev_char = Some('x');
                i = next;
                continue;
            }
            // No matching closer: the whole run is literal text (CommonMark:
            // a failed opener does not re-open at a later backtick).
            let mut len = 0;
            while i + len < chars.len() && chars[i + len] == '`' {
                len += 1;
            }
            for _ in 0..len {
                text_buf.push('`');
            }
            prev_char = Some('`');
            i += len;
            continue;
        }
        // Backslash escape
        if c == '\\' {
            if i + 1 < chars.len() {
                let n = chars[i + 1];
                if n == '\n' {
                    flush_text(&mut toks, &mut text_buf);
                    toks.push(Tok::HardBreak);
                    i += 2;
                    prev_char = Some('\n');
                    continue;
                }
                if n == '\r' {
                    flush_text(&mut toks, &mut text_buf);
                    toks.push(Tok::HardBreak);
                    i += 2;
                    if i < chars.len() && chars[i] == '\n' {
                        i += 1;
                    }
                    prev_char = Some('\n');
                    continue;
                }
                if is_ascii_punct(n) {
                    text_buf.push(n);
                    prev_char = Some(n);
                    i += 2;
                    continue;
                }
            }
            text_buf.push('\\');
            prev_char = Some('\\');
            i += 1;
            continue;
        }
        // Entity
        if c == '&' {
            if let Some((rep, next)) = scan_entity(&chars, i) {
                text_buf.push_str(&rep);
                prev_char = rep.chars().last();
                i = next;
                continue;
            }
            text_buf.push('&');
            prev_char = Some('&');
            i += 1;
            continue;
        }
        // Angle: autolink / raw html
        if c == '<' {
            if let Some((tok, next)) = scan_angle(&chars, i) {
                flush_text(&mut toks, &mut text_buf);
                match &tok {
                    Tok::RawHtml(_) => {}
                    _ => {}
                }
                prev_char = Some('x');
                toks.push(tok);
                i = next;
                continue;
            }
            text_buf.push('<');
            prev_char = Some('<');
            i += 1;
            continue;
        }
        // Image opener "!["
        if c == '!' && i + 1 < chars.len() && chars[i + 1] == '[' {
            flush_text(&mut toks, &mut text_buf);
            toks.push(Tok::OpenBracket { image: true });
            brackets.push(Bracket { tok_idx: toks.len() - 1, image: true, active: true, content_start: i + 2 });
            prev_char = Some('[');
            i += 2;
            continue;
        }
        // Bracket open
        if c == '[' {
            flush_text(&mut toks, &mut text_buf);
            toks.push(Tok::OpenBracket { image: false });
            brackets.push(Bracket { tok_idx: toks.len() - 1, image: false, active: true, content_start: i + 1 });
            prev_char = Some('[');
            i += 1;
            continue;
        }
        // Bracket close -> try link
        if c == ']' {
            // Flush pending text so link children include it.
            flush_text(&mut toks, &mut text_buf);
            // find nearest active opener
            let mut opener_pos: Option<usize> = None;
            for (bi, b) in brackets.iter().enumerate().rev() {
                if b.active {
                    opener_pos = Some(bi);
                    break;
                }
            }
            if let Some(bi) = opener_pos {
                let is_image = brackets[bi].image;
                let open_tok = brackets[bi].tok_idx;
                let children: Vec<Tok> = toks[open_tok + 1..].to_vec();
                // Raw source slice between the brackets: reference labels
                // match on raw text (escapes intact, ex 194/545/549/550).
                let raw_label: String = chars[brackets[bi].content_start..i].iter().collect();
                let after = i + 1;
                let mut formed: Option<(String, Option<String>, usize, bool)> = None;
                // Inline link tail
                if after < chars.len() && chars[after] == '(' {
                    // No space allowed between ] and ( (after already immediate)
                    if let Some((dest, title, next)) = parse_link_tail(&chars, after) {
                        // Empty dest with no title ok. Also links may not form if
                        // inner is... (images ok). Note: link text with code? fine.
                        formed = Some((dest, title, next, false));
                    }
                    if formed.is_none() {
                        // Not a valid inline tail: fall back to a shortcut
                        // reference link (raw label), leaving the tail
                        // literal (ex 568).
                        let key = normalize_label(&raw_label);
                        if let Some((d, t)) = refs.get(&key) {
                            formed = Some((d.clone(), t.clone(), after, true));
                        }
                    }
                } else if after < chars.len() && chars[after] == '[' {
                    if let Some((label, next)) = parse_bracket_label(&chars, after) {
                        if label.is_empty() {
                            // collapsed: label = raw link text
                            let key = normalize_label(&raw_label);
                            if let Some((d, t)) = refs.get(&key) {
                                formed = Some((d.clone(), t.clone(), next, true));
                            }
                        } else {
                            let key = normalize_label(&label);
                            if let Some((d, t)) = refs.get(&key) {
                                formed = Some((d.clone(), t.clone(), next, true));
                            }
                        }
                    }
                } else {
                    // shortcut reference (raw label, escapes intact)
                    let key = normalize_label(&raw_label);
                    if let Some((d, t)) = refs.get(&key) {
                        formed = Some((d.clone(), t.clone(), after, true));
                    }
                }
                if let Some((dest, title, next, _is_ref)) = formed {
                    // Link parts get entity decoding (backslashes were
                    // handled during tail parsing); percent-encoding happens
                    // at render time via clean_url.
                    let dest = unescape_html(&dest);
                    let title = title.map(|t| unescape_html(&t));
                    // Images: children must not contain unescaped brackets? skip check.
                    toks.truncate(open_tok);
                    if is_image {
                        toks.push(Tok::Image { children, url: dest, title });
                    } else {
                        toks.push(Tok::Link { children, url: dest, title });
                        // A formed link deactivates earlier `[` openers so
                        // outer links cannot contain it (ex 518) — except
                        // inside an unclosed `![` image description, which
                        // resolves on its own (ex 520). Only brackets before
                        // the nearest unclosed image opener die (or all of
                        // them when no image is open).
                        let kill_until = match brackets[..bi]
                            .iter()
                            .rposition(|b| b.active && b.image)
                        {
                            Some(p) => p,
                            None => bi,
                        };
                        for b in brackets.iter_mut().take(kill_until) {
                            if !b.image {
                                b.active = false;
                            }
                        }
                    }
                    // remove opener + any above from stack
                    brackets.truncate(bi);
                    // fix tok_idx? indices below bi unchanged; above removed.
                    prev_char = Some('x');
                    i = next;
                    continue;
                } else {
                    // No link formed: the opener is deactivated (a later `]`
                    // must not reuse it, ex 513) and the `]` is literal text.
                    brackets[bi].active = false;
                    toks.push(Tok::Text("]".to_string()));
                    prev_char = Some(']');
                    i += 1;
                    continue;
                }
            }
            text_buf.push(']');
            prev_char = Some(']');
            i += 1;
            continue;
        }
        // Emphasis runs
        if c == '*' || c == '_' {
            let mut len = 0;
            while i + len < chars.len() && chars[i + len] == c {
                len += 1;
            }
            let after = if i + len < chars.len() { Some(chars[i + len]) } else { None };
            let before = prev_char;
            let before_ws = before.map(is_ws).unwrap_or(true);
            let after_ws = after.map(is_ws).unwrap_or(true);
            let before_punct = before.map(is_punct).unwrap_or(false);
            let after_punct = after.map(is_punct).unwrap_or(false);
            let left = !after_ws && (!after_punct || before_ws || before_punct);
            let right = !before_ws && (!before_punct || after_ws || after_punct);
            let (can_open, can_close) = if c == '*' {
                (left, right)
            } else {
                (left && (!right || before_punct), right && (!left || after_punct))
            };
            flush_text(&mut toks, &mut text_buf);
            toks.push(Tok::Delim { ch: c, len, can_open, can_close });
            prev_char = Some(c);
            i += len;
            continue;
        }
        // Regular char
        text_buf.push(c);
        prev_char = Some(c);
        i += 1;
    }
    flush_text(&mut toks, &mut text_buf);
    process_emphasis(&mut toks);
    toks
}

/// Nesting depth of a token position: emphasis open markers minus close
/// markers before it. An opener and closer at different depths would cross
/// an established emphasis boundary (ex 469), so they must not match.
fn marker_depth(toks: &[Tok], idx: usize) -> i32 {
    let mut d = 0i32;
    for t in &toks[..idx.min(toks.len())] {
        if let Tok::Code(m) = t {
            if m == "\0EM\0" || m == "\0STRONG\0" {
                d += 1;
            } else if m == "\0EM\0\x01" || m == "\0STRONG\0\x01" {
                d -= 1;
            }
        }
    }
    d
}

/// CommonMark delimiter-stack emphasis resolution (in place).
fn process_emphasis(toks: &mut Vec<Tok>) {
    // Recurse into link/image children first (their interiors support emphasis).
    for tok in toks.iter_mut() {
        match tok {
            Tok::Link { children, .. } | Tok::Image { children, .. } => {
                process_emphasis(children);
            }
            _ => {}
        }
    }
    // Collect indices of delim tokens.
    let mut delims: Vec<usize> = Vec::new();
    for (idx, t) in toks.iter().enumerate() {
        if matches!(t, Tok::Delim { .. }) {
            delims.push(idx);
        }
    }

    // Note: no openers_bottom barrier. A closer with no preceding opener
    // (e.g. the first run in `foo*bar*`) must remain eligible as an opener
    // for later closers; the odd-match rule below is applied pairwise, so a
    // full backward scan is always correct (inputs here are small).
    let mut ci = 0;
    while ci < delims.len() {
        let c_idx = delims[ci];
        let (ch, c_len, c_open, c_close) = match &toks[c_idx] {
            Tok::Delim { ch, len, can_open, can_close } => (*ch, *len, *can_open, *can_close),
            _ => {
                ci += 1;
                continue;
            }
        };
        if !c_close {
            ci += 1;
            continue;
        }
        // Find opener scanning backwards (no lower barrier).
        let mut oi_opt: Option<usize> = None; // position in delims vec
        let mut oi = ci;
        while oi > 0 {
            oi -= 1;
            let o_idx = delims[oi];
            let (o_ch, o_len, o_open, o_close) = match &toks[o_idx] {
                Tok::Delim { ch, len, can_open, can_close } => (*ch, *len, *can_open, *can_close),
                _ => continue,
            };
            if o_ch != ch || !o_open {
                continue;
            }
            // Odd-match rule.
            if (c_open || o_close) && ((o_len + c_len) % 3 == 0) && (o_len % 3 != 0 || c_len % 3 != 0) {
                continue;
            }
            // Boundary rule: opener and closer must sit at the same marker
            // depth, else the match would cross established emphasis.
            if marker_depth(toks, o_idx) != marker_depth(toks, c_idx) {
                continue;
            }
            oi_opt = Some(oi);
            break;
        }
        if let Some(oi_found) = oi_opt {
            let o_idx = delims[oi_found];
            let o_len = match &toks[o_idx] {
                Tok::Delim { len, .. } => *len,
                _ => 0,
            };
            let use_len = if o_len >= 2 && c_len >= 2 { 2 } else { 1 };
            // Extract inner tokens between opener and closer.
            let inner: Vec<Tok> = toks[o_idx + 1..c_idx].to_vec();
            // Build node.
            let node = if use_len == 2 { Tok::Link { children: inner, url: String::new(), title: None } } else { Tok::Link { children: inner, url: String::new(), title: None } };
            // We use Link-with-empty-url as temporary strong/em marker? No —
            // instead directly splice Em/Strong via Text markers. Simplest:
            // replace range with marker tokens and convert later.
            // To keep types simple, convert now into final Tok sequence:
            // [left-over opener][Em/Strong node as nested Toks][left-over closer]
            let _ = node;
            // Remove used delims from the token stream.
            // New token layout for range o_idx..=c_idx:
            let mut replacement: Vec<Tok> = Vec::new();
            if o_len > use_len {
                let (och, o_open, o_close) = match &toks[o_idx] {
                    Tok::Delim { ch, can_open, can_close, .. } => (*ch, *can_open, *can_close),
                    _ => (ch, true, false),
                };
                replacement.push(Tok::Delim { ch: och, len: o_len - use_len, can_open: o_open, can_close: o_close });
            }
            // emphasis node encoded as Link with special url? Avoid hack:
            // push placeholder then fix below by converting inner to elements.
            // Instead, store as nested Delim-resolved structure: we introduce
            // Tok variants via Text tags — simplest correct approach: build
            // final InlineElements later; here splice raw inner with markers.
            //
            // Implementation: replace the whole range with:
            //   [EmOpen marker][inner][EmClose marker]
            // encoded as Tok::RawHtml-like sentinels? Use Code with \0 prefix.
            let marker = if use_len == 2 { "\0STRONG\0" } else { "\0EM\0" };
            replacement.push(Tok::Code(marker.to_string()));
            let inner_now: Vec<Tok> = toks[o_idx + 1..c_idx].to_vec();
            replacement.extend(inner_now);
            replacement.push(Tok::Code(format!("{}\x01", marker)));
            if c_len > use_len {
                let (cch, co_open, co_close) = match &toks[c_idx] {
                    Tok::Delim { ch, can_open, can_close, .. } => (*ch, *can_open, *can_close),
                    _ => (ch, false, true),
                };
                replacement.push(Tok::Delim { ch: cch, len: c_len - use_len, can_open: co_open, can_close: co_close });
            }
            toks.splice(o_idx..=c_idx, replacement);
            // Rebuild delims index list (positions shifted) and restart the
            // scan from the front: lengths always shrink, so this terminates,
            // and earlier closers may now match leftovers (reference behavior).
            delims.clear();
            for (idx, t) in toks.iter().enumerate() {
                if matches!(t, Tok::Delim { .. }) {
                    delims.push(idx);
                }
            }
            // Restart scan just after the new close marker to allow nesting
            // like `*a **b** c*`: find current position of close marker.
            ci = 0;
            // Advance ci to first delim after opener region to avoid infinite loop
            // on zero-progress (lengths always shrink, so restart is safe).
            continue;
        } else {
            // No opener: the run stays literal. (No barrier is recorded;
            // see the note above.)
            ci += 1;
        }
    }
}

fn toks_to_elements(toks: &[Tok]) -> Vec<InlineElement> {
    let mut out: Vec<InlineElement> = Vec::new();
    let mut i = 0usize;
    while i < toks.len() {
        match &toks[i] {
            Tok::Text(s) => {
                // Strip trailing spaces before a break? Already handled.
                // Merge with previous text.
                if let Some(InlineElement::Text(prev)) = out.last_mut() {
                    prev.push_str(s);
                } else {
                    out.push(InlineElement::Text(s.clone()));
                }
                i += 1;
            }
            Tok::Delim { ch, len, .. } => {
                let s: String = std::iter::repeat(*ch).take(*len).collect();
                if let Some(InlineElement::Text(prev)) = out.last_mut() {
                    prev.push_str(&s);
                } else {
                    out.push(InlineElement::Text(s));
                }
                i += 1;
            }
            Tok::OpenBracket { image, .. } => {
                let s = if *image { "![" } else { "[" };
                if let Some(InlineElement::Text(prev)) = out.last_mut() {
                    prev.push_str(s);
                } else {
                    out.push(InlineElement::Text(s.to_string()));
                }
                i += 1;
            }
            Tok::Code(s) if s == "\0EM\0" => {
                // find matching close
                let mut j = i + 1;
                let mut depth = 1usize;
                while j < toks.len() {
                    if let Tok::Code(m) = &toks[j] {
                        if m == "\0EM\0" {
                            depth += 1;
                        } else if m == "\0EM\0\x01" {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                    }
                    j += 1;
                }
                if j < toks.len() {
                    let inner = toks_to_elements(&toks[i + 1..j]);
                    out.push(InlineElement::Italic(inner));
                    i = j + 1;
                } else {
                    if let Some(InlineElement::Text(prev)) = out.last_mut() {
                        prev.push('*');
                    } else {
                        out.push(InlineElement::Text("*".to_string()));
                    }
                    i += 1;
                }
            }
            Tok::Code(s) if s == "\0STRONG\0" => {
                let mut j = i + 1;
                let mut depth = 1usize;
                while j < toks.len() {
                    if let Tok::Code(m) = &toks[j] {
                        if m == "\0STRONG\0" {
                            depth += 1;
                        } else if m == "\0STRONG\0\x01" {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                    }
                    j += 1;
                }
                if j < toks.len() {
                    let inner = toks_to_elements(&toks[i + 1..j]);
                    out.push(InlineElement::Bold(inner));
                    i = j + 1;
                } else {
                    if let Some(InlineElement::Text(prev)) = out.last_mut() {
                        prev.push_str("**");
                    } else {
                        out.push(InlineElement::Text("**".to_string()));
                    }
                    i += 1;
                }
            }
            Tok::Code(s) => {
                out.push(InlineElement::Code(s.clone()));
                i += 1;
            }
            Tok::RawHtml(s) => {
                out.push(InlineElement::RawHtml(s.clone()));
                i += 1;
            }
            Tok::HardBreak => {
                out.push(InlineElement::HardBreak);
                i += 1;
            }
            Tok::SoftBreak => {
                out.push(InlineElement::SoftBreak);
                i += 1;
            }
            Tok::Link { children, url, title } => {
                let inner = toks_to_elements(children);
                out.push(InlineElement::Link { text: inner, url: url.clone(), title: title.clone() });
                i += 1;
            }
            Tok::Image { children, url, title } => {
                let inner = toks_to_elements(children);
                // alt text: render inner as plain text (code -> content, em -> inner)
                let alt = elements_to_plain(&inner);
                out.push(InlineElement::Image { alt, url: url.clone(), title: title.clone() });
                i += 1;
            }
        }
    }
    out
}

fn elements_to_plain(elems: &[InlineElement]) -> String {
    let mut s = String::new();
    for e in elems {
        match e {
            InlineElement::Text(t) => s.push_str(t),
            InlineElement::Bold(c) | InlineElement::Italic(c) => s.push_str(&elements_to_plain(c)),
            InlineElement::Code(c) => s.push_str(c),
            InlineElement::Link { text, url, .. } => {
                // Image descriptions preserve nested-link markup but
                // flatten plain links (ex 520 vs 575).
                if contains_link(text) {
                    s.push('[');
                    s.push_str(&elements_to_plain(text));
                    s.push_str("](");
                    s.push_str(url);
                    s.push(')');
                } else {
                    s.push_str(&elements_to_plain(text));
                }
            }
            InlineElement::Image { alt, .. } => s.push_str(alt),
            InlineElement::RawHtml(h) => s.push_str(h),
            InlineElement::HardBreak | InlineElement::SoftBreak => s.push('\n'),
        }
    }
    s
}

/// True when inline elements contain a nested link or image.
fn contains_link(elems: &[InlineElement]) -> bool {
    elems.iter().any(|e| match e {
        InlineElement::Link { .. } | InlineElement::Image { .. } => true,
        InlineElement::Bold(c) | InlineElement::Italic(c) => contains_link(c),
        _ => false,
    })
}

/// Parse inline Markdown elements from a string (no reference definitions).
/// Handles emphasis/strong, code spans, links, autolinks, entities, breaks.
pub fn parse_inline(input: &str) -> Vec<InlineElement> {
    let refs = HashMap::new();
    parse_inline_with_refs(input, &refs)
}

/// Parse inline Markdown with link reference definitions available.
pub fn parse_inline_with_refs(input: &str, refs: &RefDefs) -> Vec<InlineElement> {
    // Trailing spaces/tabs at the very end are stripped (no hard break).
    let input = input.trim_end_matches([' ', '\t']);
    let toks = tokenize(input, refs);
    toks_to_elements(&toks)
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

    #[test]
    fn test_intraword_underscore() {
        // Intraword _ must not emphasize.
        let html = render_inline_html(&parse_inline("foo_bar_baz"));
        assert_eq!(html, "foo_bar_baz");
    }

    #[test]
    fn test_rule_of_three() {
        // `***foo**` -> `<em><strong>foo</strong></em>`? Actually `***foo**`
        // is `<p>*<strong>foo</strong></p>`.
        let html = render_inline_html(&parse_inline("***foo**"));
        assert!(html.contains("<strong>foo</strong>"));
    }

    #[test]
    fn test_autolink() {
        let html = render_inline_html(&parse_inline("<http://example.com>"));
        assert!(html.contains("<a href=\"http://example.com\">"));
    }

    #[test]
    fn test_hard_break() {
        let html = render_inline_html(&parse_inline("foo  \nbar"));
        assert!(html.contains("<br />"));
    }
}
