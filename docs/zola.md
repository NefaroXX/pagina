# Using pagina with Zola

Zola already renders its own Markdown, so the useful job for pagina is turning
**user-supplied or external Markdown** into HTML you control — GFM rendering,
syntax highlighting, sanitization — and plugging that HTML into a template or
shortcode.

## Frontmatter compatibility

Zola page files use **TOML front matter delimited by `+++`**. pagina detects
both `---` (YAML) and `+++` (TOML) leading blocks and strips them before
conversion in every mode, cargo feature or not:

- `pagina to-html --gfm content.md out.html` — the `+++` block never renders.
- With the `frontmatter` feature, `+++` blocks parse through a built-in TOML
  subset into `Frontmatter.data` (a `yaml_rust::Yaml` value).
- Without the feature, they parse into a flat `HashMap<String, String>`.

The TOML subset covers single-line values, `[table]` headers (including dotted
`[a.b]`) and dotted keys; multiline strings/arrays and `[[array-of-tables]]`
headers are outside the subset and yield `Yaml::BadValue` (detection and
verbatim `original` preservation are unaffected). Because `+++` has no
CommonMark meaning, no content guard is needed for Zola-style blocks.

## Zola has no generic build hooks

> **Unverified against every Zola release:** as of Zola 0.19 there are no
> third-party "build hooks" like mdBook preprocessors; the supported extension
> points are shortcodes (templates in `templates/shortcodes/`), Tera templates,
> and `load_data`. If your Zola version ships hooks, prefer them over the
> pre-build step below.

The standard pattern is therefore: a shell/xtask step generates HTML fragments
**before** `zola build`, and a shortcode loads and injects them.

## Pipeline A — generate fragments, then embed via shortcode

Step 1 — pre-build generation (this is the "build hook" slot). For **trusted**
Markdown the CLI is enough:

```bash
cargo install pagina
mkdir -p static/generated
for f in user_content/*.md; do
  pagina to-html --gfm "$f" "static/generated/$(basename "$f" .md).html"
done
zola build
```

`static/` is copied verbatim by Zola, so `load_data` finds the fragment and
the file is also served as a static asset. `+++` frontmatter in each input is
stripped automatically.

Step 2 — a shortcode that injects the fragment. Add
`templates/shortcodes/marked.html`:

```
{% set html = load_data(path="generated/" ~ name, format="plain") | safe %}
{{ html }}
```

Use it in a page:

```
{{ marked(name="band.html") }}
```

> - `load_data` with `format="plain"` reads any file as raw text (verified
>   against Zola's template docs); `| safe` stops Tera escaping the HTML.
> - Zola's Markdown renderer wraps *inline* HTML nodes in `<p>`. pagina emits
>   block-level fragments (`<h1>`, `<p>`, `<table>`, `<div>`…), which pass
>   through as block HTML.
> - `load_data` resolves relative to the Zola root, then `static/`, then
>   `content/`; keep fragments under `static/generated/`.

## Pipeline B — sanitize user content (library helper)

The GFM + sanitize path is **library/WASM-only**: the v0.2 `pagina` CLI has no
`--sanitize` flag (verified in `src/cli.rs`). For user-submitted Markdown,
compile a tiny generator that depends on pagina with the `sanitize` feature:

`tools/gen/Cargo.toml`:

```toml
[package]
name = "gen"
edition = "2021"

[dependencies]
pagina = { version = "0.2", features = ["sanitize"] }
```

`tools/gen/src/main.rs`:

```rust
use pagina::frontmatter::parse_with_frontmatter;
use pagina::html_escape::escape_html;
use pagina::markdown_to_html::convert_gfm_sanitized;

fn main() {
    for path in std::env::args_os().skip(1) {
        let path = path.into_string().unwrap();
        let md = std::fs::read_to_string(&path).unwrap();

        // Strip any `+++` / `---` frontmatter and grab the title.
        let (fm, body) = parse_with_frontmatter(&md);
        let heading = fm
            .as_ref()
            .and_then(|f| f.data.get("title"))
            .map(|t| format!("<h1>{}</h1>\n", escape_html(t)));

        // GFM rendering + denylist sanitizer in one call.
        let body_html = convert_gfm_sanitized(body).expect("convert");

        let dest = path.trim_end_matches(".md").to_string() + ".html";
        std::fs::write(dest, format!("{}{}", heading.unwrap_or_default(), body_html))
            .expect("write");
    }
}
```

Output to `static/generated/` so the Pipeline A shortcode can inject it:

```bash
cargo run --release -p gen -- user_content/*.md
zola build
```

`convert_gfm_sanitized` strips `script`/`style`/`iframe`/`object`/`embed`/
`form`/`base`/`link`/`meta`, drops `on*` and `style` attributes, and blocks
`javascript:`/`vbscript:`/`data:text/html` URLs in `href`/`src`.

> **Feature flag note:** this helper enables only `sanitize`, so
> `Frontmatter.data` is a `HashMap<String, String>` and `.get("title")`
> works. If you also enable `frontmatter`, `data` becomes `yaml_rust::Yaml`
> and the title lookup must be rewritten against the YAML mapping.

## Feature flags relevant here

| Feature | Needed for | Available via |
| --- | --- | --- |
| *(none)* | GFM conversion + automatic `+++` stripping | CLI `to-html --gfm` / `convert_gfm` |
| `frontmatter` | full parse of `+++` blocks into `Frontmatter.data` | stripping/detection works without it |
| `sanitize` | `convert_sanitized` / `convert_gfm_sanitized` for user content | library + WASM only (no CLI flag) |
| `syntax-highlight` | `SyntectAdapter` highlighted code in generated fragments | library only |
| `wasm` | browser-side conversion via the `pagina-wasm` npm package (`markdown_to_html`, `markdown_to_html_gfm`, `markdown_to_html_sanitized` with `--features wasm,sanitize`) | JS bundle |