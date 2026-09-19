# idem — Publication Readiness Fixes

## Status: ✅ COMPLETE

All critical bugs fixed, all important improvements implemented, all checks green.

## Final Verification
- **Tests**: 37/37 pass (35 unit + 2 doc)
- **Clippy**: clean (zero warnings)
- **Fmt**: clean
- **Publish dry-run**: 15 files, 53.9KiB (12.2KiB compressed) — verified
- **Git**: committed on main

## Changes Made

### C1: `<br>` always emits newline
- Was: only emitted newline for `<br/>` (self-closing), bare `<br>` was silently dropped
- Now: always emits `\n`

### C2: HTML entities unescaped
- Was: `&amp;` passed through as `&amp;` in Markdown output
- Now: properly decoded to `&`, `<`, `>`, `"`, `'`, and numeric entities

### C3: Void elements handled correctly
- Was: `<img>` consumed all remaining tokens via `convert_until_end`
- Now: void elements (br, hr, img, input, meta, etc.) handled individually
- `<img src="..." alt="...">` → `![alt](src)`

### C4: Blank lines close all blocks
- Was: only closed paragraphs, lists/quotes merged across blank lines
- Now: blank lines close any open block state

### I1: stdin/stdout support
- `-` as input reads from stdin
- `-` as output writes to stdout
- Help text updated with examples

### I2: `_italic_` and `__bold__`
- Both `*`/`_` delimiters now supported
- `__bold__` → `<strong>bold</strong>`
- `_italic_` → `<em>italic</em>`

### I3: Horizontal rules
- `---`, `***`, `___` → `<hr>`

### I4: Clean public API
- Added convenience re-exports: `escape_html`, `unescape_html`, `html_to_markdown`, `markdown_to_html`
- Internal modules still accessible but clean API surface available at root

### I5: Doc comments
- Both `convert` functions have `///` doc comments with examples
- Doc tests pass
