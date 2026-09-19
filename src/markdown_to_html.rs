use crate::error::Result;
use crate::html_escape::{escape_html, escape_href, unescape_html};
use crate::inline_parser::{
    normalize_label, parse_inline_with_refs, render_inline_html, tag_end_len, RefDefs,
};

// NOTE on raw HTML: per CommonMark, HTML blocks (types 1-7) and inline raw
// HTML are passed through verbatim, NOT escaped. Normal-text escaping still
// applies everywhere else. This means XSS sanitization is the consumer's
// responsibility — treat this crate's HTML output as unsanitized input.

// ---------------------------------------------------------------------------
// Block AST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Block {
    Paragraph(Vec<String>),
    Heading(u8, String),
    ThematicBreak,
    IndentedCode(Vec<String>),
    FencedCode { info: String, lines: Vec<String> },
    HtmlBlock(Vec<String>),
    BlockQuote(Vec<Block>),
    List {
        ordered: bool,
        start: u32,
        tight: bool,
        items: Vec<Vec<Block>>,
    },
}

// ---------------------------------------------------------------------------
// Line helpers (tab-aware, CommonMark tab stops of 4)
// ---------------------------------------------------------------------------

/// Scan leading whitespace from `phase` (absolute column mod 4 at `s`
/// start): returns (byte index of first non-whitespace, columns advanced
/// past the whitespace). Tabs advance to multiples of 4.
fn ws_end_col(s: &str, phase: usize) -> (usize, usize) {
    let mut c = 0usize;
    let mut byte = 0usize;
    for ch in s.chars() {
        match ch {
            ' ' => {
                c += 1;
                byte += 1;
            }
            '\t' => {
                c += 4 - ((phase + c) % 4);
                byte += 1;
            }
            _ => break,
        }
    }
    (byte, c)
}

/// Relative indent of a line: columns of leading whitespace.
fn indent_rel(line: &PLine) -> usize {
    ws_end_col(&line.s, line.phase).1
}

/// Strip up to `n` columns of leading whitespace. A fully-consumed tab
/// vanishes; a partially-consumed tab leaves spaces (its remainder). The
/// phase always advances by the consumed amount; text is taken from the
/// advanced offset, so later tab stops stay exact. Never strips
/// non-whitespace.
fn strip_cols(line: &PLine, n: usize) -> PLine {
    let mut c = 0usize;
    let mut byte = 0usize;
    let mut extra = String::new();
    for ch in line.s.chars() {
        if c >= n {
            break;
        }
        match ch {
            ' ' => {
                c += 1;
                byte += 1;
            }
            '\t' => {
                let span = 4 - ((line.phase + c) % 4);
                if c + span <= n {
                    c += span;
                    byte += 1;
                } else {
                    // Partial tab: remainder becomes spaces, tab consumed.
                    for _ in 0..(c + span - n) {
                        extra.push(' ');
                    }
                    c = n;
                    byte += ch.len_utf8();
                    break;
                }
            }
            _ => break,
        }
    }
    let mut out = extra;
    out.push_str(&line.s[byte..]);
    PLine {
        s: out,
        phase: (line.phase + c) % 4,
        lazy: line.lazy,
    }
}

fn is_blank(s: &str) -> bool {
    s.chars().all(|c| c == ' ' || c == '\t' || c == '\r')
}

/// Strip up to 3 columns of indent; None when the relative indent
/// exceeds 3.
fn after_small_indent(line: &PLine) -> Option<PLine> {
    if indent_rel(line) > 3 {
        return None;
    }
    Some(strip_cols(line, 3))
}

// ---------------------------------------------------------------------------
// Leaf-block detectors
// ---------------------------------------------------------------------------

fn is_thematic_break(line: &PLine) -> bool {
    let t = match after_small_indent(line) {
        Some(t) => t.s,
        None => return false,
    };
    let mut ch0: Option<char> = None;
    let mut count = 0usize;
    for c in t.chars() {
        if c == ' ' || c == '\t' {
            continue;
        }
        match ch0 {
            None => {
                if c != '-' && c != '_' && c != '*' {
                    return false;
                }
                ch0 = Some(c);
                count = 1;
            }
            Some(m) => {
                if c != m {
                    return false;
                }
                count += 1;
            }
        }
    }
    count >= 3
}

/// ATX heading: 1-6 `#`, then space/tab/EOL. Returns (level, content).
fn parse_atx(line: &PLine) -> Option<(u8, String)> {
    let t = after_small_indent(line)?.s;
    let mut level = 0u8;
    let mut it = t.char_indices();
    for (_, c) in it.by_ref() {
        if c == '#' && level < 6 {
            level += 1;
        } else {
            break;
        }
    }
    if level == 0 {
        return None;
    }
    let rest = &t[level as usize..];
    let mut rc = rest.chars();
    match rc.next() {
        None => return Some((level, String::new())),
        Some(c) if c == ' ' || c == '\t' => {}
        _ => return None,
    }
    // Strip one optional leading space already consumed; strip the rest later.
    let mut content = rest.trim().to_string();
    // Closing hash sequence: spaces + #'s + spaces only at end.
    if content.ends_with('#') {
        let stripped_end = content.trim_end_matches('#');
        let hashes = &content[stripped_end.len()..];
        let before = stripped_end;
        if !hashes.is_empty() && (before.is_empty() || before.ends_with(' ') || before.ends_with('\t')) {
            content = before.trim_end().to_string();
        }
    }
    Some((level, content))
}

/// Setext underline: all `=` -> 1, all `-` -> 2.
/// Leading/trailing whitespace allowed; interior spaces are NOT (`= =` and
/// `--- -` are not valid underlines).
fn parse_setext(line: &PLine) -> Option<u8> {
    let t = after_small_indent(line)?.s;
    let t = t.trim();
    if t.is_empty() {
        return None;
    }
    let c = t.chars().next().unwrap();
    if c != '=' && c != '-' {
        return None;
    }
    if !t.chars().all(|x| x == c) {
        return None;
    }
    Some(if c == '=' { 1 } else { 2 })
}

/// Fenced code open: up to 3 indent, 3+ backticks or tildes.
/// Returns (fence char, fence length, info string).
fn parse_fence_open(line: &PLine) -> Option<(char, usize, String)> {
    let t = after_small_indent(line)?.s;
    let mut it = t.chars();
    let fc = it.next()?;
    if fc != '`' && fc != '~' {
        return None;
    }
    let mut len = 0usize;
    for c in t.chars() {
        if c == fc {
            len += 1;
        } else {
            break;
        }
    }
    if len < 3 {
        return None;
    }
    let info = t[len..].trim().to_string();
    // Backtick-fence info strings may not contain backticks.
    if fc == '`' && info.contains('`') {
        return None;
    }
    Some((fc, len, info))
}

/// Closing fence: up to 3 indent, 3+ same char, only whitespace after.
fn parse_fence_close(line: &PLine, fc: char, need: usize) -> bool {
    let t = match after_small_indent(line) {
        Some(t) => t.s,
        None => return false,
    };
    let mut len = 0usize;
    for c in t.chars() {
        if c == fc {
            len += 1;
        } else {
            break;
        }
    }
    if len < need {
        return false;
    }
    t[len..].chars().all(|c| c == ' ' || c == '\t')
}

/// Blockquote marker: up to 3 indent, `>`, optional single space/tab.
/// Returns the remaining content with an exact column: a tab after `>` is
/// only partially consumed (offset stays at the tab, column +1).
fn parse_blockquote_marker(line: &PLine) -> Option<PLine> {
    let t = after_small_indent(line)?;
    match t.s.chars().next() {
        Some('>') => {}
        _ => return None,
    }
    let rest = t.s[1..].to_string();
    let cphase = (t.phase + 1) % 4;
    match rest.chars().next() {
        None => Some(PLine { s: String::new(), phase: cphase, lazy: false }),
        Some(' ') => Some(PLine {
            s: rest[1..].to_string(),
            phase: (cphase + 1) % 4,
            lazy: false,
        }),
        Some('\t') => Some(PLine { s: rest, phase: (cphase + 1) % 4, lazy: false }),
        Some(_) => Some(PLine { s: rest, phase: cphase, lazy: false }),
    }
}

// ---------------------------------------------------------------------------
// HTML blocks (CommonMark types 1-7)
// ---------------------------------------------------------------------------

const BLOCK_TAGS: &[&str] = &[
    "address", "article", "aside", "base", "basefont", "blockquote", "body", "caption",
    "center", "col", "colgroup", "dd", "details", "dialog", "dir", "div", "dl", "dt",
    "fieldset", "figcaption", "figure", "footer", "form", "frame", "frameset", "h1", "h2",
    "h3", "h4", "h5", "h6", "head", "header", "hr", "html", "iframe", "legend", "li",
    "link", "main", "menu", "menuitem", "nav", "noframes", "ol", "optgroup", "option",
    "p", "param", "search", "section", "summary", "table", "tbody", "td", "tfoot", "th",
    "thead", "title", "tr", "track", "ul",
];

fn tag_name_at(s: &str) -> Option<(bool, String)> {
    // s starts right after '<'. Returns (is_closing, lowercase name).
    let b = s.as_bytes();
    let mut k = 0usize;
    let closing = if !b.is_empty() && b[0] == b'/' {
        k = 1;
        true
    } else {
        false
    };
    let start = k;
    while k < b.len() && (b[k].is_ascii_alphanumeric() || b[k] == b'-') {
        k += 1;
    }
    if k == start {
        return None;
    }
    Some((closing, s[start..k].to_ascii_lowercase()))
}

/// Detect an HTML block start. Returns (type, end matcher description).
/// `in_paragraph` gates type 7 (which cannot interrupt a paragraph).
fn html_block_start(line: &PLine, in_paragraph: bool) -> Option<u8> {
    let t = after_small_indent(line)?.s;
    let b = t.as_bytes();
    if b.is_empty() || b[0] != b'<' {
        return None;
    }
    let rest = &t[1..];
    // Type 2: comment
    if rest.starts_with("!--") {
        return Some(2);
    }
    // Type 3: processing instruction
    if rest.starts_with('?') {
        return Some(3);
    }
    // Type 4: declaration <!A...>
    if rest.starts_with('!') {
        let nxt = rest[1..].chars().next()?;
        if nxt.is_ascii_alphabetic() {
            return Some(4);
        }
        // Type 5: CDATA
        if rest.starts_with("![CDATA[") {
            return Some(5);
        }
        return None;
    }
    // Type 1: pre/script/style/textarea OPENING tag (closing tags do not
    // start a block; they fall through to type 7 / paragraph rules).
    if !t[1..].starts_with('/') {
        let low = t.to_ascii_lowercase();
        let after_lt = &low[1..];
        for name in ["pre", "script", "style", "textarea"] {
            if after_lt.starts_with(name) {
                let rem = &after_lt[name.len()..];
                match rem.chars().next() {
                    Some(c) if c == ' ' || c == '\t' || c == '\n' || c == '>' || c == '/' => {
                        return Some(1)
                    }
                    None => return Some(1),
                    _ => {}
                }
            }
        }
    }
    // Parse tag name for types 6/7.
    let (closing, name) = tag_name_at(rest)?;
    let _ = closing;
    // Type 6: block-level tag (open or close) followed by space, >, />, EOL.
    if BLOCK_TAGS.contains(&name.as_str()) {
        let rem = &rest[name.len() + (if rest.starts_with('/') { 1 } else { 0 })..];
        match rem.chars().next() {
            Some(c) if c == ' ' || c == '\t' || c == '\n' || c == '>' || c == '/' => {
                return Some(6)
            }
            None => return Some(6),
            _ => {}
        }
    }
    // Type 7: any complete open/closing tag alone on the line.
    if !in_paragraph && is_complete_tag_line(&t) {
        return Some(7);
    }
    None
}

/// Type 7 requires the line to be a complete tag (plus trailing spaces),
/// using the strict CommonMark tag grammar.
fn is_complete_tag_line(t: &str) -> bool {
    match tag_end_len(t) {
        Some(len) => t[len..].chars().all(|c| c == ' ' || c == '\t'),
        None => false,
    }
}

fn html_block_end(line: &str, htype: u8) -> bool {
    let low = line.to_ascii_lowercase();
    match htype {
        1 => {
            low.contains("</pre")
                || low.contains("</script")
                || low.contains("</style")
                || low.contains("</textarea")
        }
        2 => line.contains("-->"),
        3 => line.contains("?>"),
        4 => line.contains('>'),
        5 => line.contains("]]>"),
        _ => false, // types 6/7 end at blank line (handled by caller)
    }
}

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

struct ListMarker {
    ordered: bool,
    bullet: char,
    number: u32,
    delim: char,
    content_indent: usize,
    empty: bool,
}

fn parse_list_marker(line: &PLine) -> Option<ListMarker> {
    if indent_rel(line) > 3 {
        return None;
    }
    let stripped = strip_cols(line, 3);
    let t = stripped.s;
    let b = t.as_bytes();
    if b.is_empty() {
        return None;
    }
    // Marker offset from line start, and tab phase there.
    let mrel = ws_end_col(&line.s, line.phase).1;
    let mphase = (line.phase + mrel) % 4;
    // Bullet
    if b[0] == b'-' || b[0] == b'+' || b[0] == b'*' {
        let bullet = b[0] as char;
        if b.len() == 1 {
            return Some(ListMarker {
                ordered: false,
                bullet,
                number: 0,
                delim: '\0',
                // Empty item: content starts one column past the marker.
                content_indent: mrel + 2,
                empty: true,
            });
        }
        if b[1] == b' ' || b[1] == b'\t' {
            let rest = &t[1..];
            if rest.trim().is_empty() {
                // Only whitespace after the marker: empty item.
                return Some(ListMarker {
                    ordered: false,
                    bullet,
                    number: 0,
                    delim: '\0',
                    content_indent: mrel + 2,
                    empty: true,
                });
            }
            let ci = mrel + marker_content_indent(mphase, 1, rest);
            return Some(ListMarker {
                ordered: false,
                bullet,
                number: 0,
                delim: '\0',
                content_indent: ci,
                empty: false,
            });
        }
        return None;
    }
    // Ordered: 1-9 digits + '.' or ')'
    let mut k = 0usize;
    while k < b.len() && b[k].is_ascii_digit() {
        k += 1;
    }
    if k == 0 || k > 9 {
        return None;
    }
    if k + 1 > b.len() {
        return None;
    }
    let d = b[k] as char;
    if d != '.' && d != ')' {
        return None;
    }
    // Spec: start number must not be... any number allowed to start a list,
    // but a list starting with 1... an ordered item starting with number != 1
    // cannot interrupt a paragraph. Caller handles that gate.
    let num: u32 = t[..k].parse().ok()?;
    if k + 1 == b.len() {
        return Some(ListMarker {
            ordered: true,
            bullet: '\0',
            number: num,
            delim: d,
            content_indent: mrel + k + 1 + 1,
            empty: true,
        });
    }
    if b[k + 1] == b' ' || b[k + 1] == b'\t' {
        let rest = &t[k + 1..];
        if rest.trim().is_empty() {
            // Only whitespace after the marker: empty item.
            return Some(ListMarker {
                ordered: true,
                bullet: '\0',
                number: num,
                delim: d,
                content_indent: mrel + k + 1 + 1,
                empty: true,
            });
        }
        let ci = mrel + marker_content_indent(mphase, k + 1, rest);
        return Some(ListMarker {
            ordered: true,
            bullet: '\0',
            number: num,
            delim: d,
            content_indent: ci,
            empty: false,
        });
    }
    None
}

/// Content indent after a marker of visual width `mw`, given the whitespace
/// run `ws` that follows it, relative to the marker start. `mphase` is the
/// tab phase at the marker. Implements the 1-4 spaces / 5+ spaces / tab rule.
fn marker_content_indent(mphase: usize, mw: usize, ws: &str) -> usize {
    let mut spaces = 0usize;
    let mut tab_stop: Option<usize> = None;
    for c in ws.chars() {
        if c == ' ' {
            spaces += 1;
        } else if c == '\t' {
            // Tab advances to next multiple of 4 from marker end.
            let rel = mw + spaces;
            tab_stop = Some(rel + 4 - ((mphase + rel) % 4));
            break;
        } else {
            break;
        }
    }
    if let Some(ts) = tab_stop {
        // Content starts right after the tab, which ends at the tab stop.
        return ts;
    }
    if spaces == 0 {
        return mw + 1;
    }
    if spaces <= 4 {
        return mw + spaces;
    }
    mw + 1
}

// ---------------------------------------------------------------------------
// Reference definitions
// ---------------------------------------------------------------------------

/// Try to parse a link reference definition starting at `lines[i]`.
/// Returns (normalized key, destination, title, lines consumed).
fn try_refdef(lines: &[PLine], i: usize) -> Option<(String, String, Option<String>, usize)> {
    // Up to 3 columns indent (a tab-indented line is code, not a refdef).
    if indent_rel(&lines[i]) > 3 {
        return None;
    }
    let t = strip_cols(&lines[i], 3).s;
    let cs: Vec<char> = t.chars().collect();
    if cs.is_empty() || cs[0] != '[' {
        return None;
    }
    // Label may span lines (no blank lines inside); gather a small window.
    let mut lwin = t.clone();
    let mut lextra = 0usize;
    while lextra < 3
        && i + 1 + lextra < lines.len()
        && !is_blank(&lines[i + 1 + lextra].s)
    {
        lwin.push('\n');
        lwin.push_str(&lines[i + 1 + lextra].s);
        lextra += 1;
    }
    let lc: Vec<char> = lwin.chars().collect();
    if lc.is_empty() || lc[0] != '[' {
        return None;
    }
    // Scan label; backslash escapes are kept literal for label matching
    // (ex 545/549: `[foo\!]` must not match `[foo!]`, `[ref\[]` matches).
    // An unescaped `[` inside is forbidden; `]` closes.
    let mut label = String::new();
    let mut k = 1usize;
    let mut closed = false;
    while k < lc.len() {
        let c = lc[k];
        if c == '\\' && k + 1 < lc.len() {
            label.push(c);
            label.push(lc[k + 1]);
            k += 2;
        } else if c == ']' {
            closed = true;
            k += 1;
            break;
        } else if c == '[' {
            return None;
        } else {
            label.push(c);
            if label.len() > 999 {
                return None;
            }
            k += 1;
        }
    }
    if !closed {
        return None;
    }
    if k >= lc.len() || lc[k] != ':' {
        return None;
    }
    k += 1;
    // Remainder window: everything after ':' plus further non-blank lines
    // (titles may span lines, but never a blank line).
    let label_newlines = lc[..k].iter().filter(|c| **c == '\n').count();
    let mut window: String = lc[k..].iter().collect();
    let mut extra = lextra;
    while i + 1 + extra < lines.len()
        && !is_blank(&lines[i + 1 + extra].s)
        && extra < 12
    {
        window.push('\n');
        window.push_str(&lines[i + 1 + extra].s);
        extra += 1;
    }
    let wc: Vec<char> = window.chars().collect();
    let mut p = 0usize;
    // skip spaces/tabs; at most one newline before dest... allow blank-ish?
    while p < wc.len() && (wc[p] == ' ' || wc[p] == '\t') {
        p += 1;
    }
    if p < wc.len() && wc[p] == '\n' {
        p += 1;
        while p < wc.len() && (wc[p] == ' ' || wc[p] == '\t') {
            p += 1;
        }
        // A second blank line before dest is not allowed.
        if p < wc.len() && wc[p] == '\n' {
            return None;
        }
    }
    // Destination
    let mut dest = String::new();
    if p < wc.len() && wc[p] == '<' {
        p += 1;
        while p < wc.len() && wc[p] != '>' {
            if wc[p] == '\n' || wc[p] == '\r' || wc[p] == '<' {
                return None;
            }
            if wc[p] == '\\' && p + 1 < wc.len() && wc[p + 1].is_ascii_punctuation() {
                dest.push(wc[p + 1]);
                p += 2;
            } else {
                dest.push(wc[p]);
                p += 1;
            }
        }
        if p >= wc.len() || wc[p] != '>' {
            return None;
        }
        p += 1;
    } else {
        let mut depth = 0usize;
        let mut any = false;
        while p < wc.len() {
            let c = wc[p];
            if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
                break;
            }
            if c == '(' {
                depth += 1;
                dest.push(c);
                any = true;
                p += 1;
            } else if c == ')' {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                dest.push(c);
                any = true;
                p += 1;
            } else if c == '\\' && p + 1 < wc.len() && wc[p + 1].is_ascii_punctuation() {
                dest.push(wc[p + 1]);
                any = true;
                p += 2;
            } else if c.is_control() {
                return None;
            } else {
                dest.push(c);
                any = true;
                p += 1;
            }
        }
        if !any {
            return None;
        }
    }
    // After dest: spaces/tabs, then optionally a title, then EOL. A title
    // that fails to parse (or has trailing garbage) falls back to a
    // destination-only definition, which must end the line itself.
    let mut p2 = p;
    while p2 < wc.len() && (wc[p2] == ' ' || wc[p2] == '\t') {
        p2 += 1;
    }
    let mut title: Option<String> = None;
    let mut end = p2;
    let mut ok = false;
    if p2 < wc.len() && wc[p2] != '\n' {
        // Title on the same line (must be whitespace-separated from dest).
        if p2 > p {
            if let Some((tt, np)) = parse_ref_title(&wc, p2) {
                let mut r = np;
                while r < wc.len() && (wc[r] == ' ' || wc[r] == '\t') {
                    r += 1;
                }
                if r >= wc.len() || wc[r] == '\n' {
                    title = Some(tt);
                    end = r;
                    ok = true;
                }
            }
        }
        if !ok {
            // Destination-only fallback: nothing but spaces may follow dest.
            let mut r = p;
            while r < wc.len() && (wc[r] == ' ' || wc[r] == '\t') {
                r += 1;
            }
            if r >= wc.len() || wc[r] == '\n' {
                end = r;
                ok = true;
            }
        }
    } else if p2 < wc.len() && wc[p2] == '\n' {
        // Maybe title on the next line; else destination-only.
        let mut q = p2 + 1;
        while q < wc.len() && (wc[q] == ' ' || wc[q] == '\t') {
            q += 1;
        }
        let mut titled = false;
        if q < wc.len() && (wc[q] == '"' || wc[q] == '\'' || wc[q] == '(') {
            if let Some((tt, np)) = parse_ref_title(&wc, q) {
                let mut r = np;
                while r < wc.len() && (wc[r] == ' ' || wc[r] == '\t') {
                    r += 1;
                }
                if r >= wc.len() || wc[r] == '\n' {
                    title = Some(tt);
                    end = r;
                    titled = true;
                }
            }
        }
        if !titled {
            end = p2;
        }
        ok = true;
    } else {
        // Window ended right after dest: destination-only.
        end = p2;
        ok = true;
    }
    if !ok {
        return None;
    }
    let newlines = label_newlines
        + wc[..end.min(wc.len())].iter().filter(|c| **c == '\n').count();
    let consumed = 1 + newlines;
    if consumed > extra + 1 || i + consumed > lines.len() {
        return None;
    }
    let key = normalize_label(&label);
    if key.is_empty() || label.chars().count() > 999 {
        return None;
    }
    // Reference destinations/titles get entity decoding (backslashes were
    // handled during parsing); percent-encoding happens at render time.
    let dest = unescape_html(&dest);
    let title = title.map(|t| unescape_html(&t));
    Some((key, dest, title, consumed))
}

fn parse_ref_title(wc: &[char], i: usize) -> Option<(String, usize)> {
    if i >= wc.len() {
        return None;
    }
    let q = wc[i];
    let closing = match q {
        '"' => '"',
        '\'' => '\'',
        '(' => ')',
        _ => return None,
    };
    let mut j = i + 1;
    let mut title = String::new();
    while j < wc.len() {
        let c = wc[j];
        if c == closing {
            return Some((title, j + 1));
        }
        if c == '\n' {
            if closing == '(' {
                return None;
            }
            // Titles with " may span lines? Spec allows multiline titles.
            title.push('\n');
            j += 1;
            continue;
        }
        if c == '\\' && j + 1 < wc.len() && wc[j + 1].is_ascii_punctuation() {
            title.push(wc[j + 1]);
            j += 2;
        } else {
            title.push(c);
            j += 1;
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Block parser (container-block model with lazy continuation)
// ---------------------------------------------------------------------------

/// A source line plus whether it entered the current container as a lazy
/// continuation. A setext underline that is a lazy continuation line does
/// not form a heading (CommonMark example 93).
#[derive(Debug, Clone)]
struct PLine {
    s: String,
    /// Tab phase (absolute column mod 4) at the start of `s`. Container
    /// stripping advances the phase without resetting it, so tab stops
    /// stay exact (tabs examples 5-7), while indentation itself is always
    /// measured relative to the current frame (spec: content indentation
    /// is relative to the last containing block marker, ex 259-260).
    phase: usize,
    lazy: bool,
}

impl PLine {
    fn fresh(s: String) -> Self {
        PLine { s, phase: 0, lazy: false }
    }
}

/// Strip all leading blockquote markers from a line, preserving the phase.
/// Used to classify nested content.
fn strip_all_bq_markers(line: &PLine) -> PLine {
    let mut cur = PLine { s: line.s.clone(), phase: line.phase, lazy: false };
    loop {
        match parse_blockquote_marker(&cur) {
            Some(rest) => {
                if rest.s.len() == cur.s.len() {
                    break;
                }
                cur = rest;
            }
            None => break,
        }
    }
    cur
}

/// Content after a list marker (marker + 1-4 spaces/tab consumed).
/// Returns None for an empty item. Phase-exact: a separator tab is only
/// partially consumed (offset stays, phase +1).
fn list_content_after(line: &PLine, m: &ListMarker) -> Option<PLine> {
    let stripped = strip_cols(line, 3);
    let t = stripped.s;
    let p0 = stripped.phase;
    let (rest, mw) = if m.ordered {
        let mut k = 0usize;
        let b = t.as_bytes();
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
        }
        k += 1; // delim
        if k > t.len() {
            return None;
        }
        (t[k..].to_string(), k)
    } else if t.len() > 1 {
        (t[1..].to_string(), 1)
    } else {
        return None;
    };
    if rest.is_empty() {
        return None;
    }
    match rest.chars().next().unwrap() {
        ' ' => {
            let ws: String = rest.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            if ws.contains('\t') {
                let tp = rest.find('\t').unwrap();
                // Content starts after the tab; phase = tab-stop end.
                let beforelen = ws[..tp].len();
                let endphase = (p0 + mw + beforelen + 4 - ((p0 + mw + beforelen) % 4)) % 4;
                Some(PLine { s: rest[tp + 1..].to_string(), phase: endphase, lazy: false })
            } else if ws.len() >= 5 {
                Some(PLine { s: rest[1..].to_string(), phase: (p0 + mw + 1) % 4, lazy: false })
            } else {
                Some(PLine {
                    s: rest[ws.len()..].to_string(),
                    phase: (p0 + mw + ws.len()) % 4,
                    lazy: false,
                })
            }
        }
        '\t' => Some(PLine { s: rest, phase: (p0 + mw + 1) % 4, lazy: false }),
        _ => None,
    }
    .filter(|c| !c.s.trim().is_empty())
}

/// True when a container line ends an open paragraph for lazy-continuation
/// purposes, looking through nested quotes/list markers (ex 250, 292).
fn inner_para_open(line: &PLine) -> bool {
    let mut cur = strip_all_bq_markers(line);
    loop {
        if let Some(m) = parse_list_marker(&cur) {
            match list_content_after(&cur, &m) {
                Some(content) => {
                    cur = strip_all_bq_markers(&content);
                    continue;
                }
                None => return false,
            }
        }
        return line_is_para_text(&cur);
    }
}
fn line_is_para_text(line: &PLine) -> bool {
    if is_blank(&line.s) {
        return false;
    }
    if parse_fence_open(line).is_some()
        || parse_atx(line).is_some()
        || is_thematic_break(line)
        || html_block_start(line, true).is_some()
        || parse_list_marker(line).is_some()
        || parse_blockquote_marker(line).is_some()
        || indent_rel(line) >= 4
    {
        return false;
    }
    true
}
/// True when a line (non-blank) begins a block that cannot be a lazy
/// continuation of a paragraph inside a quote/list container.
fn is_interrupting_block_start(line: &PLine, in_para: bool) -> bool {
    if parse_fence_open(line).is_some() {
        return true;
    }
    if parse_atx(line).is_some() {
        return true;
    }
    if parse_blockquote_marker(line).is_some() {
        return true;
    }
    if is_thematic_break(line) {
        return true;
    }
    if html_block_start(line, in_para).is_some() {
        return true;
    }
    if parse_list_marker(line).is_some() {
        return true;
    }
    false
}

fn parse_blocks(lines: &[PLine], refs: &mut RefDefs) -> Vec<Block> {
    parse_blocks_spanned(lines, refs).0
}

/// Parse blocks, also returning each top-level block's consumed line span
/// `[start, end)` in `lines` coordinates (used for tight/loose detection).
fn parse_blocks_spanned(
    lines: &[PLine],
    refs: &mut RefDefs,
) -> (Vec<Block>, Vec<(usize, usize)>) {
    let mut blocks: Vec<Block> = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut para: Vec<PLine> = Vec::new();
    let mut i = 0usize;

    let flush_para = |blocks: &mut Vec<Block>,
                          spans: &mut Vec<(usize, usize)>,
                          para: &mut Vec<PLine>,
                          end: usize| {
        if !para.is_empty() {
            let start = end - para.len();
            let texts: Vec<String> = para.iter().map(|l| l.s.clone()).collect();
            blocks.push(Block::Paragraph(texts));
            spans.push((start, end));
            para.clear();
        }
    };

    while i < lines.len() {
        // Blank line ends paragraph.
        if is_blank(&lines[i].s) {
            flush_para(&mut blocks, &mut spans, &mut para, i);
            i += 1;
            continue;
        }

        // Lazy continuation lines always join the paragraph: the parent
        // container already vetted them, so they never start new blocks
        // (even if dedenting makes them look like one, e.g. `    - e`).
        // Leading whitespace is stripped entirely.
        if lines[i].lazy {
            let stripped = strip_cols(&lines[i], lines[i].s.len() * 4 + 4);
            para.push(PLine { s: stripped.s, phase: stripped.phase, lazy: true });
            i += 1;
            continue;
        }

        // Fenced code (content dedents by the opening fence's indent).
        if let Some((fc, flen, info)) = parse_fence_open(&lines[i]) {
            let bs = i;
            flush_para(&mut blocks, &mut spans, &mut para, i);
            let fence_indent = indent_rel(&lines[i]).min(3);
            i += 1;
            let mut content: Vec<String> = Vec::new();
            while i < lines.len() && !parse_fence_close(&lines[i], fc, flen) {
                content.push(strip_cols(&lines[i], fence_indent).s);
                i += 1;
            }
            if i < lines.len() {
                i += 1; // consume closing fence
            }
            blocks.push(Block::FencedCode { info, lines: content });
            spans.push((bs, i));
            continue;
        }

        // HTML block.
        if let Some(htype) = html_block_start(&lines[i], !para.is_empty()) {
            let bs = i;
            flush_para(&mut blocks, &mut spans, &mut para, i);
            let mut content: Vec<String> = Vec::new();
            content.push(lines[i].s.clone());
            i += 1;
            if htype <= 5 {
                if !html_block_end(&lines[i - 1].s, htype) {
                    while i < lines.len() && !html_block_end(&lines[i].s, htype) {
                        content.push(lines[i].s.clone());
                        i += 1;
                    }
                    if i < lines.len() {
                        content.push(lines[i].s.clone());
                        i += 1;
                    }
                }
            } else {
                while i < lines.len() && !is_blank(&lines[i].s) {
                    content.push(lines[i].s.clone());
                    i += 1;
                }
            }
            blocks.push(Block::HtmlBlock(content));
            spans.push((bs, i));
            continue;
        }

        // ATX heading.
        if let Some((level, content)) = parse_atx(&lines[i]) {
            let bs = i;
            flush_para(&mut blocks, &mut spans, &mut para, i);
            blocks.push(Block::Heading(level, content));
            spans.push((bs, i + 1));
            i += 1;
            continue;
        }

        // Setext underline (needs an open paragraph; the underline itself
        // must not be a lazy continuation line).
        if !para.is_empty() && !lines[i].lazy {
            if let Some(level) = parse_setext(&lines[i]) {
                let ps = i - para.len();
                let text = para.iter().map(|l| l.s.clone()).collect::<Vec<_>>().join("\n");
                para.clear();
                blocks.push(Block::Heading(level, text));
                spans.push((ps, i + 1));
                i += 1;
                continue;
            }
        }

        // Thematic break.
        if is_thematic_break(&lines[i]) {
            flush_para(&mut blocks, &mut spans, &mut para, i);
            blocks.push(Block::ThematicBreak);
            spans.push((i, i + 1));
            i += 1;
            continue;
        }

        // Indented code (cannot interrupt a paragraph). Blank lines keep
        // their stripped whitespace (spec example 112).
        if para.is_empty() && indent_rel(&lines[i]) >= 4 {
            let bs = i;
            let mut content: Vec<String> = Vec::new();
            while i < lines.len()
                && (is_blank(&lines[i].s) || indent_rel(&lines[i]) >= 4)
            {
                content.push(strip_cols(&lines[i], 4).s);
                i += 1;
            }
            // Drop trailing blank lines (they belong to the following block).
            while content.last().map(|s| is_blank(s)).unwrap_or(false) {
                content.pop();
                i -= 1;
            }
            blocks.push(Block::IndentedCode(content));
            spans.push((bs, i));
            continue;
        }

        // Blockquote (gather with lazy continuation).
        if parse_blockquote_marker(&lines[i]).is_some() {
            let bs = i;
            flush_para(&mut blocks, &mut spans, &mut para, i);
            let mut inner: Vec<PLine> = Vec::new();
            while i < lines.len() {
                if let Some(stripped) = parse_blockquote_marker(&lines[i]) {
                    inner.push(stripped);
                    i += 1;
                } else if is_blank(&lines[i].s) {
                    // A blank line without a `>` marker ends the quote (a
                    // following `>` line starts a new quote; ex 242). The
                    // blank itself is left for the outer loop to skip.
                    break;
                } else if !inner.is_empty()
                    && inner
                        .last()
                        .map(|l| inner_para_open(l))
                        .unwrap_or(false)
                    && !is_interrupting_block_start(&lines[i], true)
                {
                    // Lazy continuation line (only into an open paragraph).
                    inner.push(PLine {
                        s: lines[i].s.clone(),
                        phase: lines[i].phase,
                        lazy: true,
                    });
                    i += 1;
                } else {
                    break;
                }
            }
            // Strip trailing blanks from container.
            while inner.last().map(|l| l.s.is_empty()).unwrap_or(false) {
                inner.pop();
            }
            let children = parse_blocks(&inner, refs);
            blocks.push(Block::BlockQuote(children));
            spans.push((bs, i));
            continue;
        }

        // List (empty items and non-1 ordered starts cannot interrupt).
        if let Some(m) = parse_list_marker(&lines[i]) {
            // Ordered items starting at != 1 cannot interrupt a paragraph.
            // Empty items cannot interrupt either (ex 285).
            if (m.ordered && m.number != 1 && !para.is_empty()) || (m.empty && !para.is_empty()) {
                // Fall through to paragraph continuation.
            } else {
                flush_para(&mut blocks, &mut spans, &mut para, i);
                let (list_block, next_i) = parse_list(lines, i, refs);
                blocks.push(list_block);
                spans.push((i, next_i));
                i = next_i;
                continue;
            }
        }

        // Reference definition (cannot interrupt a paragraph).
        if para.is_empty() {
            if let Some((key, dest, title, consumed)) = try_refdef(lines, i) {
                refs.entry(key).or_insert((dest, title));
                i += consumed;
                continue;
            }
        }

        // Paragraph continuation: leading whitespace is stripped entirely
        // (lazy continuation lines join the open paragraph). The source
        // line's lazy flag is preserved for the setext-underline rule.
        {
            let stripped = strip_cols(&lines[i], lines[i].s.len() * 4 + 4);
            para.push(PLine {
                s: stripped.s,
                phase: stripped.phase,
                lazy: lines[i].lazy,
            });
        }
        i += 1;
    }
    flush_para(&mut blocks, &mut spans, &mut para, i);

    (blocks, spans)
}

/// Parse a full list starting at `start`. Returns (block, next index).
/// All indentation is frame-relative (spec ex 259-260); tab phases ride
/// along in each line.
fn parse_list(
    lines: &[PLine],
    start: usize,
    refs: &mut RefDefs,
) -> (Block, usize) {
    let first = parse_list_marker(&lines[start]).expect("list marker");
    let ordered = first.ordered;
    let delim = first.delim;
    let bullet = first.bullet;
    let list_start = first.number;

    // Raw item bodies (dedented content lines, each with its content indent).
    let mut items_raw: Vec<Vec<PLine>> = Vec::new();
    let mut blank_between = false;
    let mut i = start;
    let n = lines.len();

    // First-line content of item with marker m (phase-exact).
    let first_line_content = |line: &PLine, m: &ListMarker| -> PLine {
        match list_content_after(line, m) {
            Some(c) => c,
            None => PLine { s: String::new(), phase: 0, lazy: false },
        }
    };
    // Strip an indented content line down by `ci` columns.
    let strip_to = |line: &PLine, ci: usize| -> PLine {
        strip_cols(line, ci)
    };

    let mut ci = first.content_indent;
    let mut cur: Vec<PLine> = vec![first_line_content(&lines[i], &first)];
    i += 1;

    // Whether current item has seen content (for blank-start rule).
    loop {
        if i >= n {
            break;
        }
        let line = &lines[i];
        if is_blank(&line.s) {
            // Look ahead: skip blanks, decide.
            let mut j = i;
            while j < n && is_blank(&lines[j].s) {
                j += 1;
            }
            if j >= n {
                // Trailing blanks: end list, don't consume.
                break;
            }
            // Next non-blank: same-type item marker (a thematic break such as
            // `* * *` ends the list instead of starting an item)?
            let same_item = match parse_list_marker(&lines[j]) {
                Some(m2) => {
                    !is_thematic_break(&lines[j])
                        && m2.ordered == ordered
                        && (if ordered {
                            m2.delim == delim
                        } else {
                            m2.bullet == bullet
                        })
                        && indent_rel(&lines[j]) < ci
                }
                None => false,
            };
            if same_item {
                blank_between = true;
                cur.push(PLine::fresh(String::new()));
                // push skipped blanks as one blank
                items_raw.push(std::mem::take(&mut cur));
                let m2 = parse_list_marker(&lines[j]).unwrap();
                ci = m2.content_indent;
                cur = vec![first_line_content(&lines[j], &m2)];
                i = j + 1;
                continue;
            }
            // Otherwise list ends; leave blanks unconsumed — unless the
            // current item never saw content AND the next line cannot start
            // a new item (then the list ends here, ex 280; else the empty
            // item continues, ex 315).
            if cur.iter().all(|l| l.s.is_empty()) {
                break;
            }
            // Check if following content belongs to current item (indented).
            if indent_rel(&lines[j]) >= ci {
                cur.push(PLine::fresh(String::new()));
                i += 1;
                continue;
            }
            // Otherwise list ends; leave blanks unconsumed.
            break;
        }
        // Same-type new item? (A thematic break line ends the list instead.)
        if !is_thematic_break(line) {
            if let Some(m2) = parse_list_marker(line) {
            let same = m2.ordered == ordered
                && (if ordered { m2.delim == delim } else { m2.bullet == bullet })
                && indent_rel(line) < ci;
            if same {
                // Exception: empty item directly after... always new item.
                items_raw.push(std::mem::take(&mut cur));
                ci = m2.content_indent;
                cur = vec![first_line_content(line, &m2)];
                i += 1;
                continue;
            }
            // Different-type marker: could be sublist content if indented.
            if indent_rel(line) >= ci {
                cur.push(strip_to(line, ci));
                i += 1;
                continue;
            }
            break;
            }
        }
        // Indented content line.
        if indent_rel(line) >= ci {
            cur.push(strip_to(line, ci));
            i += 1;
            continue;
        }
        // Lazy continuation: paragraph text that doesn't start a block,
        // joining an open paragraph only (looks through nesting).
        let last_is_para = cur
            .iter()
            .rfind(|l| !l.s.is_empty())
            .map(|l| inner_para_open(l))
            .unwrap_or(false);
        if last_is_para && !is_interrupting_block_start(line, true) {
            let mut lazy = strip_cols(line, 3);
            lazy.lazy = true;
            cur.push(lazy);
            i += 1;
            continue;
        }
        break;
    }
    items_raw.push(cur);

    // Two-or-more blank lines inside an item ends... (handled: blank runs that
    // aren't followed by item/content end the list.)

    // Tight vs loose: a list is loose if items are separated by blank lines
    // (blank_between) or an item directly contains two blocks separated by a
    // blank line. Blanks *inside* a child block (fenced code, nested list,
    // quote) do not count: a blank is direct only if no single child span
    // covers it. Spans are in trimmed-item coordinates.
    let mut loose = blank_between;
    let mut items_blocks: Vec<Vec<Block>> = Vec::new();
    for raw in &items_raw {
        // Trim leading/trailing blanks for parsing.
        let mut r = raw.clone();
        while r.first().map(|l| l.s.is_empty()).unwrap_or(false) {
            r.remove(0);
        }
        while r.last().map(|l| l.s.is_empty()).unwrap_or(false) {
            r.pop();
        }
        let (blocks, spans) = parse_blocks_spanned(&r, refs);
        for (idx, l) in r.iter().enumerate() {
            if !l.s.is_empty() {
                continue;
            }
            let covered = spans.iter().any(|(s, e)| *s <= idx && idx < *e);
            if !covered {
                loose = true;
                break;
            }
        }
        items_blocks.push(blocks);
    }

    (
        Block::List {
            ordered,
            start: list_start,
            tight: !loose,
            items: items_blocks,
        },
        i,
    )
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_blocks(blocks: &[Block], refs: &RefDefs, out: &mut String) {
    for b in blocks {
        render_block(b, refs, out);
    }
}

fn render_inline_text(s: &str, refs: &RefDefs) -> String {
    render_inline_html(&parse_inline_with_refs(s, refs))
}

/// First word of a fenced-code info string, with backslash escapes and
/// entities resolved (e.g. `foo\+bar` -> `foo+bar`).
fn clean_info_word(info: &str) -> String {
    let word = info.split_whitespace().next().unwrap_or("");
    let mut out = String::with_capacity(word.len());
    let mut it = word.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.peek() {
                Some(n) if n.is_ascii_punctuation() => {
                    out.push(*n);
                    it.next();
                }
                _ => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    unescape_html(&out)
}

fn render_block(b: &Block, refs: &RefDefs, out: &mut String) {
    match b {
        Block::Paragraph(lines) => {
            let text = lines.join("\n");
            out.push_str("<p>");
            out.push_str(&render_inline_text(&text, refs));
            out.push_str("</p>\n");
        }
        Block::Heading(level, content) => {
            out.push_str(&format!("<h{}>", level));
            out.push_str(&render_inline_text(content, refs));
            out.push_str(&format!("</h{}>\n", level));
        }
        Block::ThematicBreak => {
            out.push_str("<hr />\n");
        }
        Block::IndentedCode(lines) => {
            out.push_str("<pre><code>");
            for (k, l) in lines.iter().enumerate() {
                out.push_str(&escape_html(l));
                if k + 1 < lines.len() {
                    out.push('\n');
                }
            }
            if !lines.is_empty() {
                out.push('\n');
            }
            out.push_str("</code></pre>\n");
        }
        Block::FencedCode { info, lines } => {
            out.push_str("<pre><code");
            let lang = clean_info_word(info);
            if !lang.is_empty() {
                out.push_str(&format!(" class=\"language-{}\"", escape_href(&lang)));
            }
            out.push('>');
            for (k, l) in lines.iter().enumerate() {
                out.push_str(&escape_html(l));
                if k + 1 < lines.len() {
                    out.push('\n');
                }
            }
            if !lines.is_empty() {
                out.push('\n');
            }
            out.push_str("</code></pre>\n");
        }
        Block::HtmlBlock(lines) => {
            for l in lines {
                out.push_str(l);
                out.push('\n');
            }
        }
        Block::BlockQuote(children) => {
            out.push_str("<blockquote>\n");
            render_blocks(children, refs, out);
            out.push_str("</blockquote>\n");
        }
        Block::List { ordered, start, tight, items } => {
            if *ordered {
                if *start != 1 {
                    out.push_str(&format!("<ol start=\"{}\">\n", start));
                } else {
                    out.push_str("<ol>\n");
                }
            } else {
                out.push_str("<ul>\n");
            }
            for item in items {
                if *tight {
                    // Tight list: paragraphs render without <p>. Children
                    // are joined with newlines; `</li>` follows the last
                    // child directly (`<li>foo</li>`, `<li>foo\n<ul>…`).
                    out.push_str("<li>");
                    for (k, b) in item.iter().enumerate() {
                        match b {
                            Block::Paragraph(lines) => {
                                let inline =
                                    render_inline_text(&lines.join("\n"), refs);
                                if k > 0 && !out.ends_with('\n') {
                                    out.push('\n');
                                }
                                out.push_str(&inline);
                            }
                            _ => {
                                if !out.ends_with('\n') {
                                    out.push('\n');
                                }
                                render_block(b, refs, out);
                            }
                        }
                    }
                    out.push_str("</li>\n");
                } else if item.is_empty() {
                    out.push_str("<li></li>\n");
                } else {
                    // Loose list: always block form (`<li>` on its own line).
                    out.push_str("<li>\n");
                    render_blocks(item, refs, out);
                    out.push_str("</li>\n");
                }
            }
            if *ordered {
                out.push_str("</ol>\n");
            } else {
                out.push_str("</ul>\n");
            }
        }
    }
}

/// Convert a Markdown string to HTML (CommonMark 0.31.2 core).
///
/// Supports ATX/setext headings, thematic breaks, indented/fenced code,
/// HTML blocks (verbatim passthrough), blockquotes, ordered/bulleted lists
/// (tight/loose), link reference definitions, paragraphs with lazy
/// continuation, and full inline parsing (emphasis, links, images,
/// autolinks, code spans, entities, hard/soft breaks).
///
/// # Examples
///
/// ```
/// let html = pagina::markdown_to_html::convert("# Hello").unwrap();
/// assert_eq!(html, "<h1>Hello</h1>\n");
/// ```
pub fn convert(input: &str) -> Result<String> {
    // Split into lines (strip \r; keep tabs verbatim for tab-stop logic).
    let raw: Vec<String> = input
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|s| s.to_string())
        .collect();
    // Drop a single trailing empty line produced by a final newline.
    let mut raw = raw;
    if raw.last().map(|s| s.is_empty()).unwrap_or(false) {
        raw.pop();
    }
    let lines: Vec<PLine> = raw.into_iter().map(PLine::fresh).collect();
    // Two passes: the first collects all link reference definitions
    // (definitions apply regardless of position, even after use); the second
    // renders with the complete map. Refdef extraction is order-independent
    // (first definition wins), so structure is identical in both passes.
    let mut refs: RefDefs = RefDefs::new();
    let _ = parse_blocks(&lines, &mut refs);
    let blocks = parse_blocks(&lines, &mut refs);
    let mut out = String::new();
    render_blocks(&blocks, &refs, &mut out);
    Ok(out)
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
        assert_eq!(html, "<blockquote>\n<p>quote</p>\n</blockquote>\n");
    }

    #[test]
    fn test_code_fence() {
        let html = convert("```\ncode\n```").unwrap();
        assert_eq!(html, "<pre><code>code\n</code></pre>\n");
    }

    #[test]
    fn test_html_passthrough() {
        // Per CommonMark, raw HTML blocks pass through verbatim (not escaped).
        // Escaping still applies to normal text. XSS filtering is the
        // consumer's responsibility.
        let html = convert("<div>\nfoo\n</div>").unwrap();
        assert!(html.contains("<div>"));
        let html = convert("a <b>bold</b> c").unwrap();
        assert!(html.contains("<b>bold</b>"));
        // Normal text is still escaped.
        let html = convert("a < b").unwrap();
        assert!(html.contains("&lt;"));
    }

    #[test]
    fn test_multiple_paragraphs() {
        let html = convert("para 1\n\npara 2").unwrap();
        assert_eq!(html, "<p>para 1</p>\n<p>para 2</p>\n");
    }

    #[test]
    fn test_setext() {
        assert_eq!(convert("Foo\n===").unwrap(), "<h1>Foo</h1>\n");
        assert_eq!(convert("Foo\n---").unwrap(), "<h2>Foo</h2>\n");
    }

    #[test]
    fn test_refdef() {
        let html = convert("[foo]: /url \"title\"\n\n[foo]").unwrap();
        assert!(html.contains("<a href=\"/url\" title=\"title\">foo</a>"));
    }
}
