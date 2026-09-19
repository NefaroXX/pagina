# md2html — Publication Readiness Fixes

## Status
- [x] Phase 1: Critical bugs (C1-C4)
- [x] Phase 2: Important fixes (I1-I5)
- [ ] Phase 3: Test + verify

## Changes Made

### C1: `<br>` always emits newline (html_to_markdown.rs)
- Removed conditional on self_closing — `\n` always emitted

### C2: HTML entity unescaping (html_to_markdown.rs)
- Added `unescape_html()` import and call in text node processing
- Entities like `&amp;`, `&lt;`, `&#65;` now properly decoded in output

### C3: Void elements handled correctly (html_to_markdown.rs)
- Added VOID_ELEMENTS constant list
- `br` → newline, `hr` → `---`, `img` → `![alt](src)`
- Other void elements (input, meta, link, etc.) silently skipped

### C4: Blank lines close all blocks (markdown_to_html.rs)
- Blank lines now close list/blockquote/code-fence blocks, not just paragraphs

### I1: stdin/stdout support (cli.rs, main.rs)
- `-` as input reads from stdin, `-` as output writes to stdout
- Help text updated with examples

### I2: `_italic_` and `__bold__` (inline_parser.rs)
- Both `*`/`_` delimiters now supported for bold and italic
- `_` added to special character set

### I3: Horizontal rules (markdown_to_html.rs)
- `---`, `***`, `___` lines recognized and converted to `<hr>`

### I4: Internal modules hidden (lib.rs)
- `cli`, `html_to_markdown`, `inline_parser`, `markdown_to_html` → `pub(crate)`
- Only `error` and `html_escape` remain `pub`

### I5: Doc comments (html_to_markdown.rs, markdown_to_html.rs)
- Added `///` doc comments with examples to both `convert` functions
