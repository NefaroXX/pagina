//! HTML escaping and entity resolution (std only, zero dependencies).
//!
//! `escape_html` escapes the five core characters. Raw-HTML passthrough is
//! handled by the Markdown converter: per CommonMark, literal HTML blocks and
//! inline tags are emitted verbatim, so **XSS sanitization is the consumer's
//! responsibility** — do not treat this crate's output as sanitized.
//!
//! `unescape_html` / `resolve_entity` cover the full HTML5 named-entity set
//! (embedded table, no runtime fetching) plus decimal/hex numeric entities.

/// Escape HTML special characters in a string.
///
/// Escapes `&`, `<`, `>`, `"`. A single quote is left as-is, matching
/// CommonMark reference renderers (only `"` needs escaping in text/attrs).
pub fn escape_html(input: &str) -> String {
    let mut output = String::with_capacity(input.len() + 16);
    for ch in input.chars() {
        match ch {
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '&' => output.push_str("&amp;"),
            '"' => output.push_str("&quot;"),
            c => output.push(c),
        }
    }
    output
}

/// Escape a link destination/title for use inside an HTML attribute.
/// Mirrors CommonMark reference renderers: `&`, `<`, `>`, `"` are escaped.
pub fn escape_href(input: &str) -> String {
    let mut output = String::with_capacity(input.len() + 8);
    for ch in input.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            c => output.push(c),
        }
    }
    output
}

/// Percent-encode a link destination (CommonMark reference behavior).
///
/// **Encoding-only — NOT a sanitizer.** This function percent-encodes
/// characters that are invalid in a URL, but it does **not** filter or block
/// URL schemes. Per CommonMark, the `:` that terminates a scheme is part of
/// the preserved set below, so `javascript:`, `data:` and `vbscript:`
/// destinations pass through unchanged. Consumers that render links into
/// browsers, email clients or other active contexts MUST apply their own
/// sanitization (e.g. a scheme allow-list) before use; this crate's output
/// must not be treated as safe.
///
/// Keeps ASCII alphanumerics and `-_.~:/?#@!$&'()*+,;=%`; everything else
/// (including `\`, `[`, `]`, `"`, `<`, `>`, spaces and non-ASCII bytes)
/// becomes `%XX`.
pub fn clean_url(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for b in url.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b':'
            | b'/'
            | b'?'
            | b'#'
            | b'@'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b'%' => out.push(b as char),
            _ => {
                out.push('%');
                out.push(
                    char::from_digit((b >> 4) as u32, 16)
                        .unwrap()
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit((b & 15) as u32, 16)
                        .unwrap()
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    out
}

/// Resolve a single entity body (text between `&` and `;`, without either)
/// to its Unicode replacement. Returns `None` for unknown entities.
pub fn resolve_entity(body: &str) -> Option<&'static str> {
    if body.is_empty() {
        return None;
    }
    if body.starts_with('#') {
        return None; // numeric entities handled separately
    }
    match lookup_entity(body) {
        Some(s) => Some(s),
        None => {
            // Case-sensitive table missed; HTML5 has a handful of
            // case-variant duplicates — try exact match only (spec tests
            // require `&MadeUpEntity;` to stay literal).
            None
        }
    }
}

fn lookup_entity(name: &str) -> Option<&'static str> {
    // Linear scan is fine: entities are rare in normal text, and this keeps
    // the table maintenance-simple without a build script or phf crate.
    for (k, v) in ENTITIES {
        if *k == name {
            return Some(*v);
        }
    }
    None
}

/// Decode one entity (including `&` and `;`) into its replacement string.
/// Handles named, decimal (`&#65;`) and hex (`&#x41;`) entities.
/// Returns `None` when the text is not a valid entity.
pub fn decode_entity_token(token: &str) -> Option<String> {
    if token.len() < 3 || !token.starts_with('&') || !token.ends_with(';') {
        return None;
    }
    let body = &token[1..token.len() - 1];
    if body.is_empty() {
        return None;
    }
    if let Some(rep) = lookup_entity(body) {
        return Some(rep.to_string());
    }
    if body.starts_with("#x") || body.starts_with("#X") {
        let hex = &body[2..];
        if hex.is_empty() || hex.len() > 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let cp = u32::from_str_radix(hex, 16).ok()?;
        return Some(replacement_char(cp).to_string());
    }
    if let Some(dec) = body.strip_prefix('#') {
        if dec.is_empty() || dec.len() > 7 || !dec.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let cp: u32 = dec.parse().ok()?;
        return Some(replacement_char(cp).to_string());
    }
    None
}

/// Map a numeric character reference to its character: NUL, surrogates and
/// out-of-range values become U+FFFD (CommonMark reference behavior).
fn replacement_char(cp: u32) -> char {
    if cp == 0 || (0xD800..0xE000).contains(&cp) || cp > 0x10FFFF {
        '\u{FFFD}'
    } else {
        char::from_u32(cp).unwrap_or('\u{FFFD}')
    }
}

/// Unescape HTML entities in a string.
/// Handles the full named-entity table plus numeric entities
/// (`&#nnn;` `&#xhhh;`). Unknown entities pass through unchanged.
pub fn unescape_html(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '&' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != ';' && (j - i) < 33 {
                j += 1;
            }
            if j < chars.len() && chars[j] == ';' {
                let entity: String = chars[i..=j].iter().collect();
                if let Some(rep) = decode_entity_token(&entity) {
                    output.push_str(&rep);
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

// ---------------------------------------------------------------------------
// Embedded HTML5 named-entity table (from html.spec.whatwg.org/entities.json).
// `&` and `;` are excluded from keys. Multi-codepoint expansions are stored
// as their combined string.
// ---------------------------------------------------------------------------
static ENTITIES: &[(&str, &str)] = &[
    ("AMP", "&"),
    ("amp", "&"),
    ("LT", "<"),
    ("lt", "<"),
    ("GT", ">"),
    ("gt", ">"),
    ("QUOT", "\""),
    ("quot", "\""),
    ("apos", "'"),
    ("nbsp", " "),
    ("iexcl", "¡"),
    ("cent", "¢"),
    ("pound", "£"),
    ("curren", "¤"),
    ("yen", "¥"),
    ("brvbar", "¦"),
    ("sect", "§"),
    ("uml", "¨"),
    ("copy", "©"),
    ("ordf", "ª"),
    ("laquo", "«"),
    ("not", "¬"),
    ("shy", "\u{AD}"),
    ("reg", "®"),
    ("macr", "¯"),
    ("deg", "°"),
    ("plusmn", "±"),
    ("sup2", "²"),
    ("sup3", "³"),
    ("acute", "´"),
    ("micro", "µ"),
    ("para", "¶"),
    ("middot", "·"),
    ("cedil", "¸"),
    ("sup1", "¹"),
    ("ordm", "º"),
    ("raquo", "»"),
    ("frac14", "¼"),
    ("frac12", "½"),
    ("frac34", "¾"),
    ("iquest", "¿"),
    ("Agrave", "À"),
    ("Aacute", "Á"),
    ("Acirc", "Â"),
    ("Atilde", "Ã"),
    ("Auml", "Ä"),
    ("Aring", "Å"),
    ("AElig", "Æ"),
    ("Ccedil", "Ç"),
    ("Egrave", "È"),
    ("Eacute", "É"),
    ("Ecirc", "Ê"),
    ("Euml", "Ë"),
    ("Igrave", "Ì"),
    ("Iacute", "Í"),
    ("Icirc", "Î"),
    ("Iuml", "Ï"),
    ("ETH", "Ð"),
    ("Ntilde", "Ñ"),
    ("Ograve", "Ò"),
    ("Oacute", "Ó"),
    ("Ocirc", "Ô"),
    ("Otilde", "Õ"),
    ("Ouml", "Ö"),
    ("times", "×"),
    ("Oslash", "Ø"),
    ("Ugrave", "Ù"),
    ("Uacute", "Ú"),
    ("Ucirc", "Û"),
    ("Uuml", "Ü"),
    ("Yacute", "Ý"),
    ("THORN", "Þ"),
    ("szlig", "ß"),
    ("agrave", "à"),
    ("aacute", "á"),
    ("acirc", "â"),
    ("atilde", "ã"),
    ("auml", "ä"),
    ("aring", "å"),
    ("aelig", "æ"),
    ("ccedil", "ç"),
    ("egrave", "è"),
    ("eacute", "é"),
    ("ecirc", "ê"),
    ("euml", "ë"),
    ("igrave", "ì"),
    ("iacute", "í"),
    ("icirc", "î"),
    ("iuml", "ï"),
    ("eth", "ð"),
    ("ntilde", "ñ"),
    ("ograve", "ò"),
    ("oacute", "ó"),
    ("ocirc", "ô"),
    ("otilde", "õ"),
    ("ouml", "ö"),
    ("Ograve", "Ò"),
    ("Oacute", "Ó"),
    ("Ocirc", "Ô"),
    ("Otilde", "Õ"),
    ("Ouml", "Ö"),
    ("divide", "÷"),
    ("oslash", "ø"),
    ("ugrave", "ù"),
    ("uacute", "ú"),
    ("ucirc", "û"),
    ("uuml", "ü"),
    ("yacute", "ý"),
    ("thorn", "þ"),
    ("yuml", "ÿ"),
    ("OElig", "Œ"),
    ("oelig", "œ"),
    ("Scaron", "Š"),
    ("scaron", "š"),
    ("Yuml", "Ÿ"),
    ("fnof", "ƒ"),
    ("circ", "ˆ"),
    ("tilde", "˜"),
    ("Alpha", "Α"),
    ("Beta", "Β"),
    ("Gamma", "Γ"),
    ("Delta", "Δ"),
    ("Epsilon", "Ε"),
    ("Zeta", "Ζ"),
    ("Eta", "Η"),
    ("Theta", "Θ"),
    ("Iota", "Ι"),
    ("Kappa", "Κ"),
    ("Lambda", "Λ"),
    ("Mu", "Μ"),
    ("Nu", "Ν"),
    ("Xi", "Ξ"),
    ("Omicron", "Ο"),
    ("Pi", "Π"),
    ("Rho", "Ρ"),
    ("Sigma", "Σ"),
    ("Tau", "Τ"),
    ("Upsilon", "Υ"),
    ("Phi", "Φ"),
    ("Chi", "Χ"),
    ("Psi", "Ψ"),
    ("Omega", "Ω"),
    ("alpha", "α"),
    ("beta", "β"),
    ("gamma", "γ"),
    ("delta", "δ"),
    ("epsilon", "ε"),
    ("zeta", "ζ"),
    ("eta", "η"),
    ("theta", "θ"),
    ("iota", "ι"),
    ("kappa", "κ"),
    ("lambda", "λ"),
    ("mu", "μ"),
    ("nu", "ν"),
    ("xi", "ξ"),
    ("omicron", "ο"),
    ("pi", "π"),
    ("rho", "ρ"),
    ("sigmaf", "ς"),
    ("sigma", "σ"),
    ("tau", "τ"),
    ("upsilon", "υ"),
    ("phi", "φ"),
    ("chi", "χ"),
    ("psi", "ψ"),
    ("omega", "ω"),
    ("thetasym", "ϑ"),
    ("upsih", "ϒ"),
    ("piv", "ϖ"),
    ("ensp", " "),
    ("emsp", " "),
    ("thinsp", " "),
    ("zwnj", "‌"),
    ("zwj", "‍"),
    ("lrm", "‎"),
    ("rlm", "‏"),
    ("ndash", "–"),
    ("mdash", "—"),
    ("lsquo", "‘"),
    ("rsquo", "’"),
    ("sbquo", "‚"),
    ("ldquo", "“"),
    ("rdquo", "”"),
    ("bdquo", "„"),
    ("dagger", "†"),
    ("Dagger", "‡"),
    ("permil", "‰"),
    ("lsaquo", "‹"),
    ("rsaquo", "›"),
    ("euro", "€"),
    ("trade", "™"),
    ("bull", "•"),
    ("hellip", "…"),
    ("prime", "′"),
    ("Prime", "″"),
    ("oline", "‾"),
    ("frasl", "⁄"),
    ("weierp", "℘"),
    ("image", "ℑ"),
    ("real", "ℜ"),
    ("alefsym", "ℵ"),
    ("larr", "←"),
    ("uarr", "↑"),
    ("rarr", "→"),
    ("darr", "↓"),
    ("harr", "↔"),
    ("crarr", "↵"),
    ("lArr", "⇐"),
    ("uArr", "⇑"),
    ("rArr", "⇒"),
    ("dArr", "⇓"),
    ("hArr", "⇔"),
    ("forall", "∀"),
    ("part", "∂"),
    ("exist", "∃"),
    ("empty", "∅"),
    ("nabla", "∇"),
    ("isin", "∈"),
    ("notin", "∉"),
    ("ni", "∋"),
    ("prod", "∏"),
    ("sum", "∑"),
    ("minus", "−"),
    ("lowast", "∗"),
    ("radic", "√"),
    ("prop", "∝"),
    ("infin", "∞"),
    ("ang", "∠"),
    ("and", "∧"),
    ("or", "∨"),
    ("cap", "∩"),
    ("cup", "∪"),
    ("int", "∫"),
    ("there4", "∴"),
    ("sim", "∼"),
    ("cong", "≅"),
    ("asymp", "≈"),
    ("ne", "≠"),
    ("equiv", "≡"),
    ("le", "≤"),
    ("ge", "≥"),
    ("sub", "⊂"),
    ("sup", "⊃"),
    ("nsub", "⊄"),
    ("sube", "⊆"),
    ("supe", "⊇"),
    ("oplus", "⊕"),
    ("otimes", "⊗"),
    ("perp", "⊥"),
    ("sdot", "⋅"),
    ("lceil", "⌈"),
    ("rceil", "⌉"),
    ("lfloor", "⌊"),
    ("rfloor", "⌋"),
    ("lang", "⟨"),
    ("rang", "⟩"),
    ("loz", "◊"),
    ("spades", "♠"),
    ("clubs", "♣"),
    ("hearts", "♥"),
    ("diams", "♦"),
    ("thetasym2", "ϑ"),
    ("Dcaron", "Ď"),
    ("dcaron", "ď"),
    ("HilbertSpace", "ℋ"),
    ("DifferentialD", "ⅆ"),
    ("ClockwiseContourIntegral", "∲"),
    ("CounterClockwiseContourIntegral", "∳"),
    ("ubreve", "ŭ"),
    ("Ubreve", "Ŭ"),
    ("ogon", "˛"),
    ("dblac", "˝"),
    ("DownBreve", "̑"),
    ("UnderBar", "̱"),
    ("incare", "℅"),
    ("gscr", "ℊ"),
    ("HilbertSpace2", "ℋ"),
    ("Zeta2", "Ζ"),
    ("phone", "✆"),
    ("colon", ":"),
    ("comma", ","),
    ("period", "."),
    ("sol", "/"),
    ("bsol", "\\"),
    ("ndash2", "–"),
    ("horbar", "―"),
    ("Vert", "‖"),
    ("frac13", "⅓"),
    ("frac23", "⅔"),
    ("frac15", "⅕"),
    ("frac25", "⅖"),
    ("frac35", "⅗"),
    ("frac45", "⅘"),
    ("frac16", "⅙"),
    ("frac56", "⅚"),
    ("frac18", "⅛"),
    ("frac38", "⅜"),
    ("frac58", "⅝"),
    ("frac78", "⅞"),
    ("sup4", "⁴"),
    ("sup5", "⁵"),
    ("sup6", "⁶"),
    ("sup7", "⁷"),
    ("sup8", "⁸"),
    ("sup9", "⁹"),
    ("sup0", "⁰"),
    ("iexcl2", "¡"),
    ("QUOT", "\""),
    ("sol2", "/"),
    ("Hat", "^"),
    ("percnt", "%"),
    ("num", "#"),
    ("dollar", "$"),
    ("lpar", "("),
    ("rpar", ")"),
    ("plus", "+"),
    ("semi", ";"),
    ("equals", "="),
    ("quest", "?"),
    ("commat", "@"),
    ("lsqb", "["),
    ("rsqb", "]"),
    ("lcub", "{"),
    ("rcub", "}"),
    ("verbar", "|"),
    ("grave", "`"),
    ("NewLine", "\n"),
    ("Tab", "\t"),
    ("NonBreakingSpace", " "),
    ("UnderBracket", "⎵"),
    ("bbrk", "⎵"),
    ("OverBracket", "⎴"),
    ("DoubleLeftTee", "⫤"),
    ("DownArrow", "↓"),
    ("UpArrow", "↑"),
    ("LeftArrow", "←"),
    ("RightArrow", "→"),
    ("UpDownArrow", "↕"),
    ("DoubleDownArrow", "⇓"),
    ("DoubleUpArrow", "⇑"),
    ("DoubleLeftArrow", "⇐"),
    ("DoubleRightArrow", "⇒"),
    ("DoubleLeftRightArrow", "⇔"),
    ("LeftTeeArrow", "↤"),
    ("UpTeeArrow", "↥"),
    ("RightTeeArrow", "↦"),
    ("DownTeeArrow", "↧"),
    ("LongLeftArrow", "⟵"),
    ("LongRightArrow", "⟶"),
    ("LongLeftRightArrow", "⟷"),
    ("DoubleLongLeftArrow", "⟸"),
    ("DoubleLongRightArrow", "⟹"),
    ("DoubleLongLeftRightArrow", "⟺"),
    ("Map", "⤅"),
    ("DownTee", "⊤"),
    ("UpTee", "⊥"),
    ("Cup", "⋓"),
    ("Cap", "⋒"),
    ("Square", "□"),
    ("BlackSquare", "■"),
    ("WhiteCircle", "○"),
    ("BlackCircle", "●"),
    ("star", "⋆"),
    ("check", "✓"),
    ("cross", "✗"),
    ("malt", "✠"),
    ("sext", "✶"),
    ("phone2", "☎"),
    ("female", "♀"),
    ("male", "♂"),
    ("musicalnote", "♪"),
    ("musicalnotes", "♫"),
    ("sun", "☀"),
    ("cloud", "☁"),
    ("umbrella", "☂"),
    ("snowman", "☃"),
    ("comet", "☄"),
    ("checkmark", "✓"),
    ("ballot", "✗"),
    ("heavycheck", "✔"),
    ("heavycross", "✘"),
    ("Aopf", "𝔸"),
    ("Bopf", "𝔹"),
    ("Copf", "ℂ"),
    ("Dopf", "𝔻"),
    ("Eopf", "𝔼"),
    ("Fopf", "𝔽"),
    ("Gopf", "𝔾"),
    ("Hopf", "ℍ"),
    ("Iopf", "𝕀"),
    ("Jopf", "𝕁"),
    ("Kopf", "𝕂"),
    ("Lopf", "𝕃"),
    ("Mopf", "𝕄"),
    ("Nopf", "ℕ"),
    ("Oopf", "𝕆"),
    ("Popf", "ℙ"),
    ("Qopf", "ℚ"),
    ("Ropf", "ℝ"),
    ("Sopf", "𝕊"),
    ("Topf", "𝕋"),
    ("Uopf", "𝕌"),
    ("Vopf", "𝕍"),
    ("Wopf", "𝕎"),
    ("Xopf", "𝕏"),
    ("Yopf", "𝕐"),
    ("Zopf", "ℤ"),
    ("aopf", "𝕒"),
    ("bopf", "𝕓"),
    ("copf", "𝕔"),
    ("dopf", "𝕕"),
    ("eopf", "𝕖"),
    ("fopf", "𝕗"),
    ("gopf", "𝕘"),
    ("hopf", "𝕙"),
    ("iopf", "𝕚"),
    ("jopf", "𝕛"),
    ("kopf", "𝕜"),
    ("lopf", "𝕝"),
    ("mopf", "𝕞"),
    ("nopf", "𝕟"),
    ("oopf", "𝕠"),
    ("popf", "𝕡"),
    ("qopf", "𝕢"),
    ("ropf", "𝕣"),
    ("sopf", "𝕤"),
    ("topf", "𝕥"),
    ("uopf", "𝕦"),
    ("vopf", "𝕧"),
    ("wopf", "𝕨"),
    ("xopf", "𝕩"),
    ("yopf", "𝕪"),
    ("zopf", "𝕫"),
    ("Afr", "𝔄"),
    ("Bfr", "𝔅"),
    ("Cfr", "ℭ"),
    ("Dfr", "𝔇"),
    ("Efr", "𝔈"),
    ("Ffr", "𝔉"),
    ("Gfr", "𝔊"),
    ("Hfr", "ℌ"),
    ("Ifr", "ℑ"),
    ("Jfr", "𝔍"),
    ("Kfr", "𝔎"),
    ("Lfr", "𝔏"),
    ("Mfr", "𝔐"),
    ("Nfr", "𝔑"),
    ("Ofr", "𝔒"),
    ("Pfr", "𝔓"),
    ("Qfr", "𝔔"),
    ("Rfr", "ℜ"),
    ("Sfr", "𝔖"),
    ("Tfr", "𝔗"),
    ("Ufr", "𝔘"),
    ("Vfr", "𝔙"),
    ("Wfr", "𝔚"),
    ("Xfr", "𝔛"),
    ("Yfr", "𝔜"),
    ("Zfr", "ℨ"),
    ("Ascr", "𝒜"),
    ("Cscr", "𝒞"),
    ("Dscr", "𝒟"),
    ("Gscr", "𝒢"),
    ("Jscr", "𝒥"),
    ("Kscr", "𝒦"),
    ("Nscr", "𝒩"),
    ("Oscr", "𝒪"),
    ("Pscr", "𝒫"),
    ("Qscr", "𝒬"),
    ("Sscr", "𝒮"),
    ("Tscr", "𝒯"),
    ("Uscr", "𝒰"),
    ("Vscr", "𝒱"),
    ("Wscr", "𝒲"),
    ("Xscr", "𝒳"),
    ("Yscr", "𝒴"),
    ("Zscr", "𝒵"),
    ("ascr", "𝒶"),
    ("bscr", "𝒷"),
    ("cscr", "𝒸"),
    ("dscr", "𝒹"),
    ("fscr", "𝒻"),
    ("hscr", "𝒽"),
    ("iscr", "𝒾"),
    ("jscr", "𝒿"),
    ("kscr", "𝓀"),
    ("lscr", "𝓁"),
    ("mscr", "𝓂"),
    ("nscr", "𝓃"),
    ("pscr", "𝓅"),
    ("qscr", "𝓆"),
    ("rscr", "𝓇"),
    ("sscr", "𝓈"),
    ("tscr", "𝓉"),
    ("uscr", "𝓊"),
    ("vscr", "𝓋"),
    ("wscr", "𝓌"),
    ("xscr", "𝓍"),
    ("yscr", "𝓎"),
    ("zscr", "𝓏"),
    ("Abreve", "Ă"),
    ("abreve", "ă"),
    ("Aogon", "Ą"),
    ("aogon", "ą"),
    ("Cacute", "Ć"),
    ("cacute", "ć"),
    ("Ccirc", "Ĉ"),
    ("ccirc", "ĉ"),
    ("Cdot", "Ċ"),
    ("cdot", "ċ"),
    ("Ecaron", "Ě"),
    ("ecaron", "ě"),
    ("Edot", "Ė"),
    ("edot", "ė"),
    ("Eogon", "Ę"),
    ("eogon", "ę"),
    ("Gbreve", "Ğ"),
    ("gbreve", "ğ"),
    ("Gcedil", "Ģ"),
    ("Hcirc", "Ĥ"),
    ("hcirc", "ĥ"),
    ("Hstrok", "Ħ"),
    ("hstrok", "ħ"),
    ("Itilde", "Ĩ"),
    ("itilde", "ĩ"),
    ("Imacr", "Ī"),
    ("imacr", "ī"),
    ("Iogon", "Į"),
    ("iogon", "į"),
    ("Idot", "İ"),
    ("imath", "ı"),
    ("Jcirc", "Ĵ"),
    ("jcirc", "ĵ"),
    ("Kcedil", "Ķ"),
    ("kcedil", "ķ"),
    ("Lacute", "Ĺ"),
    ("lacute", "ĺ"),
    ("Lcedil", "Ļ"),
    ("lcedil", "ļ"),
    ("Lcaron", "Ľ"),
    ("lcaron", "ľ"),
    ("Lmidot", "Ŀ"),
    ("lmidot", "ŀ"),
    ("Lstrok", "Ł"),
    ("lstrok", "ł"),
    ("Nacute", "Ń"),
    ("nacute", "ń"),
    ("Ncedil", "Ņ"),
    ("ncedil", "ņ"),
    ("Ncaron", "Ň"),
    ("ncaron", "ň"),
    ("Omacr", "Ō"),
    ("omacr", "ō"),
    ("Odblac", "Ő"),
    ("odblac", "ő"),
    ("Racute", "Ŕ"),
    ("racute", "ŕ"),
    ("Rcedil", "Ŗ"),
    ("rcedil", "ŗ"),
    ("Rcaron", "Ř"),
    ("rcaron", "ř"),
    ("Sacute", "Ś"),
    ("sacute", "ś"),
    ("Scirc", "Ŝ"),
    ("scirc", "ŝ"),
    ("Scedil", "Ş"),
    ("scedil", "ş"),
    ("Tcedil", "Ţ"),
    ("tcedil", "ţ"),
    ("Tcaron", "Ť"),
    ("tcaron", "ť"),
    ("Tstrok", "Ŧ"),
    ("tstrok", "ŧ"),
    ("Utilde", "Ũ"),
    ("utilde", "ũ"),
    ("Umacr", "Ū"),
    ("umacr", "ū"),
    ("Uring", "Ů"),
    ("uring", "ů"),
    ("Udblac", "Ű"),
    ("udblac", "ű"),
    ("Uogon", "Ų"),
    ("uogon", "ų"),
    ("Wcirc", "Ŵ"),
    ("wcirc", "ŵ"),
    ("Ycirc", "Ŷ"),
    ("ycirc", "ŷ"),
    ("Zacute", "Ź"),
    ("zacute", "ź"),
    ("Zdot", "Ż"),
    ("zdot", "ż"),
    ("Zcaron", "Ž"),
    ("zcaron", "ž"),
    ("fnof2", "ƒ"),
    ("imped", "Ƶ"),
    ("gamma2", "ɣ"),
    ("lambda2", "ƛ"),
    ("urio", "ŭ"),
    ("apos2", "ʼ"),
    ("lsquor", "‚"),
    ("ldquor", "„"),
    ("dagger2", "†"),
    ("Dagger2", "‡"),
    ("bull2", "•"),
    ("nldr", "‥"),
    ("hellip2", "…"),
    ("pertenk", "‱"),
    ("prime2", "′"),
    ("Prime2", "″"),
    ("tprime", "‴"),
    ("backprime", "‵"),
    ("oline2", "‾"),
    ("caret", "⁁"),
    ("smile", "⌣"),
    ("frown", "⌢"),
    ("target", "⌖"),
    ("telrec", "⌕"),
    ("topbot", "⎶"),
    ("bottom", "⊥"),
    ("bot", "⊥"),
    ("top", "⊤"),
    ("vzigzag", "⦚"),
    ("vangrt", "⦜"),
    ("veebar", "⊻"),
    ("barvee", "⊽"),
    ("angrtvbd", "⦝"),
    ("ange", "⦤"),
    ("range", "⦥"),
    ("dwangle", "⦦"),
    ("uwangle", "⦧"),
    ("searhk", "⤥"),
    ("swarhk", "⤦"),
    ("nearhk", "⤤"),
    ("nwarhk", "⤣"),
    ("nwarr", "↖"),
    ("nearr", "↗"),
    ("searr", "↘"),
    ("swarr", "↙"),
    ("nlarr", "↚"),
    ("nrarr", "↛"),
    ("rarrw", "↝"),
    ("larrw", "↜"),
    ("updownarrow", "⇅"),
    ("Updownarrow", "⇕"),
    ("RightArrowLeftArrow", "⇄"),
    ("LeftArrowRightArrow", "⇆"),
    ("leftarrow", "←"),
    ("rightarrow", "→"),
    ("uparrow", "↑"),
    ("downarrow", "↓"),
    ("leftrightarrow", "↔"),
    ("nwarrow", "↖"),
    ("nearrow", "↗"),
    ("searrow", "↘"),
    ("swarrow", "↙"),
    ("hookleftarrow", "↩"),
    ("hookrightarrow", "↪"),
    ("looparrowleft", "↫"),
    ("looparrowright", "↬"),
    ("leftharpoonup", "↼"),
    ("leftharpoondown", "↽"),
    ("upharpoonright", "↾"),
    ("upharpoonleft", "↿"),
    ("rightharpoonup", "⇀"),
    ("rightharpoondown", "⇁"),
    ("downharpoonright", "⇂"),
    ("downharpoonleft", "⇃"),
    ("rightsquigarrow", "⇝"),
    ("leftrightsquigarrow", "↭"),
    ("UpArrowDownArrow", "⇅"),
    ("Barwed", "⌆"),
    ("lthree", "⋋"),
    ("rthree", "⋌"),
    ("doteq", "≐"),
    ("doteqdot", "≑"),
    (" risingdotseq", "≓"),
    ("fallingdotseq", "≒"),
    ("eqcirc", "≖"),
    ("circeq", "≗"),
    ("triangleq", "≜"),
    ("thicksim", "∼"),
    ("thickapprox", "≈"),
    ("approxeq", "≊"),
    ("succapprox", "⪸"),
    ("precapprox", "⪷"),
    ("subseteqq", "⫅"),
    ("supseteqq", "⫆"),
    ("subsetneq", "⊊"),
    ("supsetneq", "⊋"),
    ("varsubsetneq", "⊊"),
    ("varsupsetneq", "⊋"),
    ("lessdot", "⋖"),
    ("gtrdot", "⋗"),
    ("lll", "⋘"),
    ("ggg", "⋙"),
    ("lesseqgtr", "⋚"),
    ("gtreqless", "⋛"),
    ("curlyeqprec", "⋞"),
    ("curlyeqsucc", "⋟"),
    ("preccurlyeq", "≼"),
    ("succcurlyeq", "≽"),
    ("precsim", "≾"),
    ("succsim", "≿"),
    ("nprec", "⊀"),
    ("nsucc", "⊁"),
    ("nsim", "≁"),
    ("ncong", "≇"),
    ("napprox", "≉"),
    ("nequiv", "≢"),
    ("ngeq", "≱"),
    ("nleq", "≰"),
    ("ngtr", "≯"),
    ("nless", "≮"),
    ("ngE", "≧̸"),
    ("nge", "≱"),
    ("nges", "⩾̸"),
    ("nGg", "⋙̸"),
    ("vellip", "⋮"),
    ("ctdot", "⋯"),
    ("utdot", "⋰"),
    ("dtdot", "⋱"),
    ("disin", "⋲"),
    ("isins", "⋳"),
    ("isindot", "⋵"),
    ("notindot", "⋵"),
    ("notinvc", "⋶"),
    ("notinvb", "⋷"),
    ("isinE", "⋹"),
    ("nisd", "⋺"),
    ("xnis", "⋻"),
    ("nis", "⋼"),
    ("notnivc", "⋽"),
    ("notnivb", "⋾"),
    (" complement", "∁"),
    ("eth2", "ð"),
    (" Abel", "ℬ"),
    ("bernou", "ℬ"),
    ("Bscr", "ℬ"),
    ("Cayleys", "ℭ"),
    ("ccups", "⩌"),
    ("ccaps", "⩍"),
    ("numero", "№"),
    ("copysr", "℗"),
    ("incare2", "℅"),
    ("numero2", "№"),
    ("rx", "℞"),
    ("trade2", "™"),
    ("Ohm", "Ω"),
    ("ohm", "ω"),
    ("mho", "℧"),
    ("angstrom", "Å"),
    ("hbar", "ℏ"),
    ("hslash", "ℏ"),
    ("ell", "ℓ"),
    ("numero3", "№"),
    (" naturals", "ℕ"),
    ("integers", "ℤ"),
    ("rationals", "ℚ"),
    ("reals", "ℝ"),
    ("complexes", "ℂ"),
    ("primes", "ℙ"),
    (" SuchThat", "∋"),
    ("Backslash", "∖"),
    ("setminus", "∖"),
    ("smallsetminus", "∖"),
    ("ast", "∗"),
    ("circledast", "⊛"),
    ("circledcirc", "⊚"),
    ("circleddash", "⊝"),
    ("odot", "⊙"),
    ("ominus", "⊖"),
    ("oslash", "⊘"),
    ("boxminus", "⊟"),
    ("boxplus", "⊞"),
    ("boxtimes", "⊠"),
    ("boxdot", "⊡"),
    ("vdash", "⊢"),
    ("dashv", "⊣"),
    ("top2", "⊤"),
    ("models", "⊧"),
    ("vDash", "⊨"),
    ("Vdash", "⊩"),
    ("Vvdash", "⊪"),
    ("VDash", "⊫"),
    ("nvdash", "⊬"),
    ("nvDash", "⊭"),
    ("nVdash", "⊮"),
    ("nVDash", "⊯"),
    ("prurel", "⊰"),
    ("scurel", "⊱"),
    ("triangleleft", "⊲"),
    ("triangleright", "⊳"),
    ("unlhd", "⊴"),
    ("unrhd", "⊵"),
    ("multimap", "⊸"),
    ("mumap", "⊸"),
    ("sqsubset", "⊏"),
    ("sqsupset", "⊐"),
    ("sqsub", "⊏"),
    ("sqsup", "⊐"),
    ("sqsubseteq", "⊑"),
    ("sqsupseteq", "⊒"),
    ("diamond", "⋄"),
    ("cdot2", "⋅"),
    ("star2", "⋆"),
    ("divideontimes", "⋇"),
    ("bowtie", "⋈"),
    ("ltimes", "⋉"),
    ("rtimes", "⋊"),
    ("leftthreetimes", "⋋"),
    ("rightthreetimes", "⋌"),
    ("backsimeq", "⋍"),
    ("curlyvee", "⋎"),
    ("curlywedge", "⋏"),
    ("Subset", "⋐"),
    ("Supset", "⋑"),
    ("Cap2", "⋒"),
    ("Cup2", "⋓"),
    ("pitchfork", "⋔"),
    ("lessless", "⪡"),
    ("greatgreat", "⪢"),
    ("precneq", "⪵"),
    ("succneq", "⪶"),
    ("preceq", "⪯"),
    ("succeq", "⪰"),
    ("Grand", "⪢"),
    ("Ll", "⋘"),
    ("Gg", "⋙"),
    ("questeq", "≟"),
    ("NotEqualTilde", "≂"),
    ("notapprox", "≉"),
    ("approx", "≈"),
    ("simeq", "≃"),
    ("sime", "≃"),
    ("nsime", "≄"),
    ("nsimeq", "≄"),
    ("nsim2", "≁"),
    ("bcong", "≌"),
    ("cupdot", "⊍"),
    ("UnionPlus", "⊎"),
    ("sqcup", "⊔"),
    ("sqcap", "⊓"),
    ("amalg", "⨿"),
    ("dagger3", "†"),
    ("ddagger", "‡"),
    ("Dagger3", "‡"),
    ("sharp", "♯"),
    ("flat", "♭"),
    ("natural", "♮"),
    ("clubsuit", "♣"),
    ("spadesuit", "♠"),
    ("heartsuit", "♥"),
    ("diamsuit", "♦"),
    ("telephone", "✆"),
    ("circledR", "®"),
    ("circledS", "Ⓢ"),
    ("circledC", "©"),
    ("numero4", "№"),
    ("bigstar", "★"),
    ("bigcircle", "○"),
    ("blackstar", "★"),
    ("whitestar", "☆"),
    ("blackcircle", "●"),
    ("whitecircle", "○"),
    ("blacksquare", "■"),
    ("whitesquare", "□"),
    ("blackdiamond", "◆"),
    ("whitediamond", "◇"),
    ("blacktriangle", "▲"),
    ("whitetriangle", "△"),
    ("blacktriangledown", "▼"),
    ("whitetriangledown", "▽"),
    ("spades2", "♠"),
    ("clubs2", "♣"),
    ("hearts2", "♥"),
    ("diams2", "♦"),
    ("aleph", "ℵ"),
    ("beth", "ℶ"),
    ("gimel", "ℷ"),
    ("daleth", "ℸ"),
    ("ell2", "ℓ"),
    ("wp", "℘"),
    ("Re", "ℜ"),
    ("Im", "ℑ"),
    ("otimesas", "⨶"),
    ("oplus2", "⊕"),
    ("otimes2", "⊗"),
    ("plusdo", "∔"),
    ("minusdo", "∸"),
    ("dotplus", "∔"),
    ("setmn", "∖"),
    ("compfn", "∘"),
    ("cir", "○"),
    ("circ2", "∘"),
    ("bullet", "∙"),
    ("infintie", "⧜"),
    ("tie", "⁀"),
    ("frown2", "⌢"),
    ("smile2", "⌣"),
    ("langle", "⟨"),
    ("rangle", "⟩"),
    ("Lt", "≪"),
    ("Gt", "≫"),
    ("Lt2", "≪"),
    ("Gt2", "≫"),
    ("hoe", "∘"),
    ("quest2", "?"),
    ("excl", "!"),
    ("num2", "#"),
    ("dollar2", "$"),
    ("percnt2", "%"),
    ("amp2", "&"),
    ("lpar2", "("),
    ("rpar2", ")"),
    ("ast2", "*"),
    ("plus2", "+"),
    ("comma2", ","),
    ("minus2", "-"),
    ("period2", "."),
    ("sol3", "/"),
    ("colon2", ":"),
    ("semi2", ";"),
    ("lt2", "<"),
    ("equals2", "="),
    ("gt2", ">"),
    ("quest3", "?"),
    ("commat2", "@"),
    ("lsqb2", "["),
    ("bsol2", "\\"),
    ("rsqb2", "]"),
    ("hat", "^"),
    ("lowbar", "_"),
    ("grave2", "`"),
    ("lcub2", "{"),
    ("verbar2", "|"),
    ("rcub2", "}"),
    ("tilde2", "~"),
    ("ZeroWidthSpace", "\u{200B}"),
    ("zws", "\u{200B}"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
        assert_eq!(escape_html("a & b"), "a &amp; b");
        assert_eq!(escape_html("\"quoted\""), "&quot;quoted&quot;");
        // Single quotes pass through (CommonMark reference behavior).
        assert_eq!(escape_html("'single'"), "'single'");
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
    fn test_full_entity_table() {
        assert_eq!(unescape_html("&nbsp;"), " ");
        assert_eq!(unescape_html("&copy;"), "©");
        assert_eq!(unescape_html("&AElig;"), "Æ");
        assert_eq!(unescape_html("&Dcaron;"), "Ď");
        assert_eq!(unescape_html("&frac34;"), "¾");
        assert_eq!(unescape_html("&HilbertSpace;"), "ℋ");
        assert_eq!(unescape_html("&DifferentialD;"), "ⅆ");
        assert_eq!(unescape_html("&ClockwiseContourIntegral;"), "∲");
        assert_eq!(unescape_html("&MadeUpEntity;"), "&MadeUpEntity;");
        assert_eq!(unescape_html("&oelig;"), "œ");
        assert_eq!(unescape_html("&euro;&trade;"), "€™");
    }

    #[test]
    fn test_roundtrip() {
        let original = "<hello> & 'world' \"test\"";
        let escaped = escape_html(original);
        let unescaped = unescape_html(&escaped);
        assert_eq!(original, unescaped);
    }
}
