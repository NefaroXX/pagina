# pagina playground

Browser demo for the published [`pagina-wasm`](https://www.npmjs.com/package/pagina-wasm)
npm package. No build step — vanilla HTML/CSS/JS that imports the package
directly from a CDN.

## Run locally

Any static file server works. From the repository root:

```bash
# Python
python -m http.server 8080 --directory playground
# or Node
npx serve playground
# or the one-liner with your favorite tool
```

Then open <http://localhost:8080>.

Or just open `playground/index.html` directly via `file://` — the demo loads
everything from the CDN, so it works without a server too.

## What it does

- Two panes: input textarea and read-only output textarea.
- Direction toggle: Markdown → HTML (`markdown_to_html` /
  `markdown_to_html_gfm`) or HTML → Markdown (`html_to_markdown` /
  `html_to_markdown_gfm`).
- **GFM** checkbox: selects the `*_gfm` converter variants (task lists,
  strikethrough, bare autolinks; tables render in both modes).
- **Sanitized preview** toggle: previews the output inside a sandboxed
  iframe (no scripts). `pagina-wasm@0.2.0` exports **no sanitize binding**
  (verified against `pkg/pagina.d.ts` and the published d.ts), so sanitized
  preview uses the optional DOMPurify CDN fallback — commented out in
  `index.html`. Uncomment its `<script src>` tag to enable real sanitization.
  The iframe is always `sandbox=""` regardless, so raw output cannot run
  scripts.

## Version pinning

`index.html` pins `pagina-wasm@0.2.0` (namespace: `pagina-wasm@0.2.0` in the
CDN URLs):

- `https://cdn.jsdelivr.net/npm/pagina-wasm@0.2.0/pagina.js` (default)
- `https://unpkg.com/pagina-wasm@0.2.0/pagina.js` (alternative, commented)

Both CDNs were verified to serve the package's `.wasm` with
`application/wasm` plus permissive CORS, which the package's native
wasm-ES-module glue requires (modern browsers only: Chrome 96+, Firefox 89+,
Safari 15.2+).

### Bump path (when a newer version is published)

1. Bump the crate version in `Cargo.toml`, then rebuild and publish:
   `wasm-pack build --target web --features wasm --out-dir pkg` (add
   `,sanitize` if you want the sanitized bindings exported), fix
   `pkg/package.json` name to `pagina-wasm`, then `npm publish` from `pkg/`.
   (See the README's *WebAssembly / npm* section.)
2. Update `CDN_URL` in `playground/index.html` to the new version.
3. If the new artifact ships `markdown_to_html_sanitized` /
   `markdown_to_html_gfm_sanitized`, switch the "Sanitized preview" branch
   in the script to call the wasm binding instead of DOMPurify.

## GitHub Pages deployment

`.github/workflows/pages.yml` deploys `playground/` to GitHub Pages on every
tag push (`v*`) and on manual `workflow_dispatch`. Deployment uses the
standard `actions/upload-pages-artifact` + `actions/deploy-pages` pair with
minimal permissions.

One-time repo setup (cannot be encoded in the workflow file): enable Pages in
the repository settings — **Settings → Pages → Source: "GitHub Actions"** —
and configure an `Environment: github-pages` (the workflow declares it; the
first run may ask you to approve it).

After a tag is pushed, the playground is served from the repo's Pages URL
(e.g. `https://<owner>.github.io/pagina/`).

## Notes

- Converter output is unsanitized by design (the crate's sanitizer is a
  denylist, not a security boundary — see the main README). The sandboxed
  iframe and the optional DOMPurify step exist because of this.
- No dependencies, no lockfile, no build tooling — the directory is plain
  static files.