# Using pagina with mdBook

pagina converts a Markdown **fragment** to an HTML **fragment**: the output has no
`<html>`/`<head>` wrapper, no template, and no table of contents. It therefore
complements mdBook's renderer rather than replacing it.

Two integration styles that make sense:

1. **Preprocessor** — rewrite each chapter body with pagina's HTML. mdBook's
   renderer passes raw HTML blocks through verbatim, so the surrounding book
   template (sidebar, search, "previous/next") keeps working.
2. **Pre-render step** — convert extra `.md` sources with the `pagina` CLI
   (build script or xtask) and use/build them alongside `mdbook build`.

## Quick facts (pagina v0.2)

- CLI: `pagina to-html [--gfm] <input.md> <output.html>` (`-` pipes stdin/stdout).
- GFM is a runtime flag, not a cargo feature: `--gfm` enables task lists,
  strikethrough, bare autolinks, footnotes, definition lists and dollar math.
  Pipe tables always render, with or without `--gfm`.
- A leading `---` (YAML) / `+++` (TOML) frontmatter block is detected and
  **silently stripped** before conversion, with or without the `frontmatter`
  cargo feature. Frontmatter never appears in the HTML body.
- Sanitization (`sanitize` feature) is library/WASM-only: the v0.2 CLI has no
  `--sanitize` flag. If you need sanitized output in a preprocessor, call the
  library API (`markdown_to_html::convert_gfm_sanitized`).

> **Do not use `{{#include frag.html}}` to embed a raw fragment.** mdBook's
> `links` preprocessor interprets included files as **Markdown**, not raw
> HTML, and will mangle the fragment. Use a preprocessor (below) or the
> pre-render step instead.

## Option 1 — preprocessor

An mdBook preprocessor is an external program that mdBook runs twice:
first with `supports <renderer>`, then with a JSON payload on stdin that is an
array `[context, book]`; the program writes the modified `Book` JSON to stdout.

> **Unverified:** the exact JSON shape and crate layout can shift between
> mdBook releases. The sketch below targets mdBook 0.4.x / `mdbook-preprocessor
> 0.1`, matching [mdBook's preprocessor docs]. Pin the deps to the mdBook
> version you actually use and test against an empty book first.
>
> [mdBook's preprocessor docs]: https://rust-lang.github.io/mdBook/for_developers/preprocessors.html

`Cargo.toml` of the `mdbook-pagina` binary:

```toml
[package]
name = "mdbook-pagina"
edition = "2021"

[dependencies]
mdbook = "0.4"
mdbook-preprocessor = "0.1"
pagina = "0.2"                      # GFM conversions need no cargo features
serde_json = "1"
```

`src/main.rs`:

```rust
use mdbook::book::{Book, BookItem};
use mdbook::preprocess::{Preprocessor, PreprocessorContext};
use mdbook_preprocessor::CmdPreprocessor;
use std::io;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);

    // mdBook first asks: does this preprocessor support the renderer?
    if args.next().as_deref() == Some("supports") {
        // The next argument is the renderer name; exit 0 = supported.
        return Ok(());
    }

    let (ctx, book) = CmdPreprocessor::parse_input(io::stdin())?;
    let processed = Pagina.run(&ctx, book)?;
    serde_json::to_writer(io::stdout(), &processed)?;
    Ok(())
}

struct Pagina;

impl Preprocessor for Pagina {
    fn name(&self) -> &str {
        "mdbook-pagina"
    }

    fn run(&self, _ctx: &PreprocessorContext, book: Book) -> Result<Book, mdbook::errors::Error> {
        book.for_each_mut(|item| {
            if let BookItem::Chapter(ch) = item {
                // frontmatter is stripped by pagina, body -> HTML fragment
                ch.content = match pagina::markdown_to_html::convert_gfm(&ch.content) {
                    Ok(html) => html,
                    Err(e) => format!("<pre>pagina error: {e}</pre>"),
                };
            }
        });
        Ok(book)
    }
}
```

Register it in `book.toml`, and run **after** the built-in `links`
preprocessor so `{{#include}}` has already been expanded:

```toml
[preprocessor.pagina]
command = "mdbook-pagina"
after = ["links"]
```

### Optional: derive a title from frontmatter

mdBook chapter titles normally come from `SUMMARY.md`; if you must render the
frontmatter `title` as a heading, read it yourself (the chapter body has
already lost it, because pagina strips frontmatter):

```rust
use pagina::frontmatter::parse_with_frontmatter;
use pagina::html_escape::escape_html;
use pagina::markdown_to_html::convert_gfm;

fn render_with_title(md: &str) -> String {
    let (fm, body) = parse_with_frontmatter(md);
    // No-feature build: `Frontmatter.data` is a HashMap<String, String>,
    // so `.get("title")` works. With the `frontmatter` feature it becomes
    // `yaml_rust::Yaml` and this lookup must be rewritten (see the
    // feature-flag note at the end of this guide).
    let heading = fm
        .as_ref()
        .and_then(|f| f.data.get("title"))
        .map(|t| format!("<h1>{}</h1>\n", escape_html(t)));
    let body_html = convert_gfm(body).unwrap_or_else(|e| format!("<pre>{e}</pre>"));
    format!("{}{}", heading.unwrap_or_default(), body_html)
}
```

## Option 2 — pre-render step (xtask / build script)

For converting guides or external `.md` files that **don't** live in `src/`,
generate HTML fragments outside mdBook and build them with the book:

```bash
cargo install pagina
mkdir -p generated
for f in chapters/*.md; do
  pagina to-html --gfm "$f" "generated/$(basename "$f" .md).html"
done
mdbook build
```

The `generated/` directory sits outside the book tree, so it won't be picked
up as chapters. Serve it next to the book output, or copy it into the book's
output directory afterwards.

The same thing as a small xtask (so no external `pagina` install is needed):

```rust
// xtask/src/main.rs — regenerates generated/*.html from chapters/*.md
use pagina::frontmatter::parse_with_frontmatter;
use pagina::html_escape::escape_html;
use pagina::markdown_to_html::convert_gfm;
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from("generated");
    std::fs::create_dir_all(&out).unwrap();

    for entry in std::fs::read_dir("chapters").unwrap() {
        let path = entry.unwrap().path();
        let md = std::fs::read_to_string(&path).unwrap();
        let (fm, body) = parse_with_frontmatter(&md);   // strips `---` / `+++`
        let heading = fm
            .as_ref()
            .and_then(|f| f.data.get("title"))          // HashMap w/o feature
            .map(|t| format!("<h1>{}</h1>\n", escape_html(t)));
        let body_html = convert_gfm(body).unwrap();
        let dest = out.join(path.file_stem().unwrap()).with_extension("html");
        std::fs::write(dest, format!("{}{}", heading.unwrap_or_default(), body_html)).unwrap();
    }
}
```

> **Feature flag note:** the snippet above reads `Frontmatter.data` as a
> `HashMap<String, String>` (no `frontmatter` feature). If you enable
> `frontmatter`, `data` becomes `yaml_rust::Yaml` and the `.get("title")` call
> must be rewritten against the YAML mapping.

## Feature flags relevant here

| Feature | Needed for | Available |
| --- | --- | --- |
| *(none)* | GFM conversion (`convert_gfm`) | `to-html --gfm` / `convert_gfm` |
| `frontmatter` | full `---` YAML / `+++` TOML-subset parse of `Frontmatter.data` | stripping/detection works without it |
| `syntax-highlight` | `SyntectAdapter` for highlighted code in converted chapters | not needed for escaped code |
| `sanitize` | `convert_sanitized` / `convert_gfm_sanitized` (library/WASM only) | CLI has no `--sanitize` |