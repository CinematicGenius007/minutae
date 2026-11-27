# Minutae website (`site/`)

Static home page for [Minutae](https://github.com/CinematicGenius007/minutae). No framework, no build step, no runtime dependency, no CDN, no third-party request. Not part of the bun workspaces, and not served: `.vercelignore` leaves this file out of the deployment.

## Files

| Path | What it is |
|---|---|
| `index.html` | The whole page. All text is final here and readable without JavaScript. Also SEO metadata and JSON-LD |
| `styles.css` | Fonts, tokens (from `docs/DESIGN-LANGUAGE.md` §13), base, layout, components, breakpoints, accessibility |
| `js/main.js` | Entry: theme, install tabs and transport now; each demo with `import()` as its section nears |
| `js/motion.js` | The one `requestAnimationFrame` loop, visibility helpers, time and easing |
| `js/ui.js` | Announcements, clipboard, tab and radio-group keys |
| `js/theme.js`, `js/install.js`, `js/transport.js` | What every visit needs |
| `js/demos/*.js` | One module per demo: `tape`, `counters`, `prompt`, `file-viewer`, `dictate`, `closer` |
| `llms.txt`, `agents.md`, `index.md` | Plain-text versions for AI engines and agents. `agents.md` defers to `docs/agents/INSTALL.md` |
| `vercel.json` | Headers (CSP and friends, content types, one-day cache for fonts and icons), clean URLs, ignored build step |
| `icons/`, `og.png`, `site.webmanifest`, `robots.txt`, `sitemap.xml` | Icons ("Turns" mark), social image, manifest, crawling |
| `fonts/` | Subsetted WOFF2 files and their OFL licences |

## Run and check

```sh
python3 -m http.server 8080 --bind 127.0.0.1 -d site   # http://127.0.0.1:8080/
node scripts/check-site.mjs                            # from the repository root
```

The local server sends none of the `vercel.json` headers. `check-site.mjs` (also run by `.github/workflows/site.yml`) checks the CSP hash, local references, third-party requests, JSON, the origin and these gzip budgets (1 KB = 1024 bytes):

| What | Budget |
|---|---|
| HTML + CSS | 15 KB |
| JavaScript at first paint (`main.js` and its static imports) | 5 KB |
| All JavaScript | 11 KB |
| Fonts | 35 KB |
| Whole first load | 70 KB |

When editing:

- The inline theme script is allowed by its hash. After changing it, the check prints the new hash for `vercel.json`.
- A new static import of `js/main.js` also needs a `<link rel="modulepreload">` in `index.html`; the check enforces it.
- Fonts are subsetted. A character the page has not used before needs a new subset (`pyftsubset` from the `@fontsource-variable/*` packages; the arrows in `geist-mono-symbols.woff2` come from the `geist` npm package, which has them). No Geist font has `⌥`, so the system draws it.
- Red (`--onair`) is only for something recording or listening.

## When the first release is published

1. `index.html`, install block: add `data-released` to `<div id="install">`. The closer then copies the release command.
2. Same block: move `aria-selected="true"` and `tabindex="0"` from `#tab-source` to `#tab-unix` (give the others `false` and `-1`), set `aria-labelledby="tab-unix"` on `#install-panel`, and put the unix `data-command` inside `.install-command`.
3. Delete `<p id="note-release">` and the `data-note="note-release"` attributes; reword `#note-agent` (or delete it the same way). Keep `#note-source`, without "today". The release tabs then have no note, so give the install row its drawn spacing back in `styles.css`: `.install-row` bottom padding `48px`, no `margin-bottom` on `.install-meta`.
4. JSON-LD: add `"downloadUrl": "https://github.com/CinematicGenius007/minutae/releases"` and point `installUrl` at `docs/install`.
5. `llms.txt`, `agents.md`, `index.md`: remove "No release has been published yet" and lead with the release commands. Check `docs/agents/INSTALL.md` first; it wins.
6. `js/install.js` needs no change: a tab without `data-note` hides every note.

## Deploying to Vercel

Create a project with **Root Directory** `site/`, no framework preset and no build command. `ignoreCommand` skips a build only when git can show that `site/` is unchanged since the previous deployment; otherwise it builds.

## Placeholder origin (`https://minutae.vercel.app`)

Replace it in every file below when the production domain is final (the check fails if any is missed):

1. `index.html`: `<link rel="canonical">`, `og:url`, `og:image`, `twitter:image`, JSON-LD `url`.
2. `robots.txt`: `Sitemap:`.
3. `sitemap.xml`: every `<loc>`.
4. `llms.txt`: the links under `## Links`.
5. `agents.md`: the `/llms.txt` link.
6. `index.md`: the `/agents.md` and `/llms.txt` links.
