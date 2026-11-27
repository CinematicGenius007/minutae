# Minutae Website (`site/`)

Static home page for [Minutae](https://github.com/CinematicGenius007/minutae). No framework, no build step, no runtime dependencies, no CDN or third-party requests.

## Files

- `index.html` — Semantic page, SEO metadata, JSON-LD `SoftwareApplication`, and final static content (readable without JavaScript).
- `styles.css` — Self-hosted subsetted `@font-face` declarations with metric-matched fallbacks, canonical light/dark CSS tokens from `docs/DESIGN-LANGUAGE.md`, responsive breakpoints (`1440`, `900–1199`, `< 900`), reduced-motion and forced-colors styles.
- `main.js` — Deferred ES module with one shared `requestAnimationFrame` loop and `IntersectionObserver` gating for all interactive demos and motion.
- `vercel.json` — Static Vercel config (`cleanUrls`, one-day cache for `/fonts/` and `/icons/` because their names are not versioned, short revalidation for HTML/text/Markdown, CSP, `X-Content-Type-Options`, `Referrer-Policy`, `Permissions-Policy`, and MIME types for `llms.txt` and `.md`).
- `llms.txt` — LLM-readable project summary (`llmstxt.org` format).
- `agents.md` — Install, update, verify, and uninstall guide for AI agents (points to `docs/agents/INSTALL.md` in the repository as the authority).
- `index.md` — Plain Markdown version of the home page.
- `robots.txt`, `sitemap.xml`, `site.webmanifest`, `og.png` — Crawl, manifest, and Open Graph image (`1200×630`).
- `icons/` — Placeholder icons generated from `assets/brand/favicon.svg` (`favicon.svg`, `favicon.ico`, `apple-touch-icon.png`, `icon-192.png`, `icon-512.png`, `icon-maskable-512.png`). Replace these files and the single inline `<svg class="wordmark-mark">` in `index.html` when the final logo is ready.
- `fonts/` — Subsetted WOFF2 files (`geist.woff2`, `geist-mono.woff2`, `geist-mono-box.woff2`, `newsreader.woff2`, `newsreader-italic.woff2`) and their OFL license files.

## Run locally

```sh
python3 -m http.server 8080 --bind 127.0.0.1 -d site
```

Then open `http://127.0.0.1:8080/`.

## Deploying to Vercel

Point a Vercel static project's **Root Directory** at `site/` with no framework preset and no build command. `site/vercel.json` configures headers and clean URLs automatically.

## Placeholder origin (`https://minutae.vercel.app`)

`https://minutae.vercel.app` is a placeholder origin used wherever an absolute URL is required. Replace it in every file listed below when the production domain is finalized:

1. `site/index.html` — `<link rel="canonical">`, `og:url`, `og:image`, `twitter:image`, and JSON-LD `url`.
2. `site/robots.txt` — `Sitemap:` URL.
3. `site/sitemap.xml` — all `<loc>` entries (`/`, `/llms.txt`, `/agents.md`, `/index.md`).
4. `site/llms.txt` — links under `## Links` (`Home page`, `Home page (Markdown)`, `Install brief for AI agents`).
5. `site/agents.md` — link to `/llms.txt` under `## Authoritative references`.
6. `site/index.md` — links to `/agents.md` and `/llms.txt`.
