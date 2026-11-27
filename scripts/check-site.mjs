#!/usr/bin/env node
// Checks the static website in site/. No dependencies.
//
//   node scripts/check-site.mjs [site-folder]
//
// Fails when: an inline script is not allowed by the CSP in vercel.json; a local reference
// does not resolve; the page would request a third party; JSON or JSON-LD does not parse;
// a URL points at another origin than the canonical one; or a gzip budget is exceeded.

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const KB = 1024;
const BUDGETS = {
  "HTML + CSS": 15 * KB,
  "JavaScript at first paint": 5 * KB,
  "All JavaScript": 11 * KB,
  Fonts: 35 * KB,
  "Whole first load": 70 * KB,
};

// Hosts that text and links may name besides the site's own origin. None may serve a resource.
const LINKED_HOSTS = new Set([
  "github.com",
  "raw.githubusercontent.com",
  "opensource.org",
  "schema.org",
  "www.sitemaps.org",
  "www.w3.org",
]);

const site = resolve(
  process.argv[2] ?? join(dirname(fileURLToPath(import.meta.url)), "../site"),
);
const failures = [];
const fail = (file, message) =>
  failures.push(`${relative(site, file) || "."}: ${message}`);

const files = readdirSync(site, { recursive: true })
  .map((name) => join(site, name))
  .filter((path) => statSync(path).isFile());
const withExtension = (...extensions) =>
  files.filter((path) => extensions.some((e) => path.endsWith(e)));
const read = (path) => readFileSync(path, "utf8");

function parseJson(file, text, what = "JSON") {
  try {
    return JSON.parse(text);
  } catch (error) {
    fail(file, `${what} does not parse: ${error.message}`);
    return null;
  }
}

// --- HTML ------------------------------------------------------------------------------

const TAG =
  /<([a-zA-Z][\w:-]*)((?:\s+[^\s"'=<>/]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s"'=<>`]+))?)*)\s*\/?>/g;
const ATTRIBUTE =
  /([^\s"'=<>/]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?/g;
const SCRIPT = /<script\b([^>]*)>([\s\S]*?)<\/script>/gi;

const attributes = (source) =>
  Object.fromEntries(
    [...source.matchAll(ATTRIBUTE)].map(([, name, a, b, c]) => [
      name.toLowerCase(),
      a ?? b ?? c ?? "",
    ]),
  );

function parseHtml(file) {
  const source = read(file).replace(/<!--[\s\S]*?-->/g, "");
  const scripts = [...source.matchAll(SCRIPT)].map(([, attrs, body]) => ({
    ...attributes(attrs),
    body,
  }));
  const tags = [
    ...source.replace(SCRIPT, "<script$1></script>").matchAll(TAG),
  ].map(([, name, attrs]) => ({
    name: name.toLowerCase(),
    attrs: attributes(attrs),
  }));
  return { file, scripts, tags };
}

const pages = withExtension(".html").map(parseHtml);
const index = pages.find((page) => page.file === join(site, "index.html"));
if (!index) {
  console.error(`No index.html in ${site}`);
  process.exit(1);
}

// --- Origin ----------------------------------------------------------------------------

const canonical = index.tags.find(
  (tag) => tag.name === "link" && tag.attrs.rel === "canonical",
)?.attrs.href;
if (!canonical || !URL.canParse(canonical)) {
  console.error('index.html needs an absolute <link rel="canonical">.');
  process.exit(1);
}
const origin = new URL(canonical).origin;

function requireOrigin(file, what, value) {
  if (!value?.startsWith(`${origin}/`)) {
    fail(file, `${what} must be on ${origin}, found ${value ?? "nothing"}`);
  }
}

for (const name of ["og:url", "og:image", "twitter:image"]) {
  const meta = index.tags.find(
    ({ attrs }) => attrs.property === name || attrs.name === name,
  );
  requireOrigin(index.file, name, meta?.attrs.content);
}

for (const file of withExtension(
  ".html",
  ".md",
  ".txt",
  ".xml",
  ".webmanifest",
)) {
  // README.md is not deployed; font licences name their own sites.
  if (file.endsWith("README.md") || file.includes("LICENSE-")) continue;
  for (const [url] of read(file).matchAll(/https?:\/\/[^\s"'<>()`\\]+/g)) {
    const { origin: found, hostname } = new URL(url.replace(/[.,;:]+$/, ""));
    if (found !== origin && !LINKED_HOSTS.has(hostname)) {
      fail(
        file,
        `unexpected origin in ${url} (the site's origin is ${origin})`,
      );
    }
  }
}

// --- References ------------------------------------------------------------------------

/**
 * A reference must name a file of the site. `resource` marks something the browser loads,
 * which may only be our own; other references may link elsewhere.
 */
function checkReference(file, reference, { resource = true } = {}) {
  if (reference.startsWith("data:")) {
    fail(file, `data: URL, which the CSP blocks: ${reference.slice(0, 40)}`);
    return null;
  }
  const isRemote = /^([a-z][a-z+.-]*:|\/\/)/i.test(reference);
  if (isRemote && !reference.startsWith(`${origin}/`)) {
    if (resource) fail(file, `third-party resource: ${reference}`);
    return null;
  }
  const local = isRemote ? reference.slice(origin.length) : reference;
  const [path, fragment] = local.split("#");
  let target = file;
  if (path) {
    const clean = decodeURIComponent(path.split("?")[0]);
    target = clean.startsWith("/")
      ? join(site, clean)
      : resolve(dirname(file), clean);
    if (existsSync(target) && statSync(target).isDirectory()) {
      target = join(target, "index.html");
    } else if (!existsSync(target) && existsSync(`${target}.html`)) {
      target = `${target}.html`; // cleanUrls
    }
    if (!existsSync(target) || relative(site, target).startsWith("..")) {
      fail(file, `missing file: ${reference}`);
      return null;
    }
  }
  const page = pages.find((p) => p.file === target);
  if (fragment && page && !page.tags.some((t) => t.attrs.id === fragment)) {
    fail(file, `no element with id "${fragment}": ${reference}`);
  }
  return target;
}

const ID_REFERENCES = [
  "aria-controls",
  "aria-describedby",
  "aria-labelledby",
  "data-mark",
  "data-note",
  "for",
];

for (const { file, tags } of pages) {
  const ids = tags.map((tag) => tag.attrs.id).filter(Boolean);
  for (const id of ids.filter((id, i) => ids.indexOf(id) !== i)) {
    fail(file, `duplicate id "${id}"`);
  }
  for (const { name, attrs } of tags) {
    if (attrs.src) checkReference(file, attrs.src);
    if (attrs.srcset) {
      for (const candidate of attrs.srcset.split(",")) {
        checkReference(file, candidate.trim().split(/\s+/)[0]);
      }
    }
    if (attrs.href) {
      const isLink =
        name === "a" || ["canonical", "alternate"].includes(attrs.rel);
      checkReference(file, attrs.href, { resource: !isLink });
    }
    if (/image$/.test(attrs.property ?? attrs.name ?? "")) {
      checkReference(file, attrs.content);
    }
    for (const attribute of ID_REFERENCES) {
      for (const id of attrs[attribute]?.split(/\s+/) ?? []) {
        if (!ids.includes(id)) {
          fail(file, `${attribute} names a missing id "${id}"`);
        }
      }
    }
    // The CSP has no 'unsafe-inline', so these would silently do nothing.
    if (name === "style") fail(file, "<style> element is blocked by the CSP");
    for (const attribute of Object.keys(attrs)) {
      if (attribute === "style" || attribute.startsWith("on")) {
        fail(file, `${attribute}="" on <${name}> is blocked by the CSP`);
      }
    }
  }
}

for (const file of withExtension(".css")) {
  const css = read(file).replace(/\/\*[\s\S]*?\*\//g, "");
  for (const match of css.matchAll(
    /url\(\s*["']?([^"')]+)["']?\s*\)|@import\s+["']([^"']+)["']/g,
  )) {
    checkReference(file, match[1] ?? match[2]);
  }
}

for (const file of withExtension(".svg")) {
  for (const match of read(file).matchAll(
    /(?:href=["']|url\(["']?)([^"')#][^"')]*)/g,
  )) {
    checkReference(file, match[1]);
  }
}

const STATIC_IMPORT =
  /^\s*(?:import|export)\s(?:[^;"']*?\sfrom\s*)?["']([^"']+)["']/gm;
const DYNAMIC_IMPORT = /\bimport\(\s*["']([^"']+)["']/g;

/** file -> the files it imports statically (fetched before it can run). */
const staticImports = new Map();
for (const file of withExtension(".js")) {
  const source = read(file);
  const resolveImports = (pattern) =>
    [...source.matchAll(pattern)].flatMap(([, specifier]) => {
      if (!/^\.{0,2}\//.test(specifier)) {
        fail(file, `import must be a relative path: ${specifier}`);
        return [];
      }
      return checkReference(file, specifier) ?? [];
    });
  staticImports.set(file, resolveImports(STATIC_IMPORT));
  resolveImports(DYNAMIC_IMPORT);
  for (const [url] of source.matchAll(/(?:https?|wss?):\/\/[^\s"'`]+/g)) {
    fail(file, `absolute URL in a script: ${url}`);
  }
}

// --- JSON, manifest, sitemap, robots ----------------------------------------------------

for (const file of withExtension(".json", ".webmanifest")) {
  const data = parseJson(file, read(file));
  if (!data || !file.endsWith(".webmanifest")) continue;
  for (const { src } of data.icons ?? []) checkReference(file, src);
  checkReference(file, data.start_url ?? "/");
}

for (const { file, scripts } of pages) {
  for (const script of scripts) {
    if (script.type !== "application/ld+json") continue;
    const data = parseJson(file, script.body, "JSON-LD");
    if (!data) continue;
    requireOrigin(file, "JSON-LD url", data.url);
  }
}

const sitemap = join(site, "sitemap.xml");
const locations = existsSync(sitemap)
  ? [...read(sitemap).matchAll(/<loc>\s*([^<]*?)\s*<\/loc>/g)].map((m) => m[1])
  : [];
if (!locations.length) fail(sitemap, "missing, or lists no <loc>");
for (const location of locations) {
  requireOrigin(sitemap, "<loc>", location);
  checkReference(sitemap, location);
}

const robots = join(site, "robots.txt");
const sitemapLine = existsSync(robots)
  ? read(robots).match(/^Sitemap:\s*(\S+)/m)?.[1]
  : undefined;
requireOrigin(robots, "Sitemap:", sitemapLine);
if (sitemapLine) checkReference(robots, sitemapLine);

// Links to our own origin inside the text files.
for (const file of withExtension(".md", ".txt")) {
  if (file.endsWith("README.md")) continue;
  for (const [url] of read(file).matchAll(/https?:\/\/[^\s"'<>()`\\]+/g)) {
    if (url.startsWith(`${origin}/`)) {
      checkReference(file, url.replace(/[.,;:]+$/, ""));
    }
  }
}

// --- Content Security Policy ------------------------------------------------------------

const vercelFile = join(site, "vercel.json");
const vercel = existsSync(vercelFile)
  ? parseJson(vercelFile, read(vercelFile))
  : null;
const policy = vercel?.headers
  ?.find((rule) => rule.source === "/(.*)")
  ?.headers.find((header) => header.key === "Content-Security-Policy")?.value;
if (!policy) {
  fail(vercelFile, 'no Content-Security-Policy header for source "/(.*)"');
} else {
  const scriptSource =
    policy
      .split(";")
      .map((directive) => directive.trim().split(/\s+/))
      .find(([name]) => name === "script-src") ?? [];
  const allowed = new Set(
    scriptSource.filter((value) => value.startsWith("'sha256-")),
  );
  const used = new Set();
  for (const { file, scripts } of pages) {
    for (const { src, type, body } of scripts) {
      if (src !== undefined || type === "application/ld+json") continue;
      const digest = createHash("sha256").update(body).digest("base64");
      const hash = `'sha256-${digest}'`;
      used.add(hash);
      if (!allowed.has(hash)) {
        fail(file, `inline script not allowed: add ${hash} to script-src`);
      }
    }
  }
  for (const hash of allowed) {
    if (!used.has(hash)) {
      fail(vercelFile, `script-src allows ${hash}, which matches no script`);
    }
  }
}

// --- Budgets ----------------------------------------------------------------------------

const entries = index.scripts
  .filter((script) => script.type === "module" && script.src)
  .flatMap((script) => checkReference(index.file, script.src) ?? []);
const firstPaint = new Set(entries);
// A Set visits the items added while it is being walked, so this follows the whole graph.
for (const file of firstPaint) {
  for (const imported of staticImports.get(file) ?? []) {
    firstPaint.add(imported);
  }
}

// Without a preload, each level of static imports costs another round trip.
const preloaded = new Set(
  index.tags
    .filter((tag) => tag.attrs.rel === "modulepreload")
    .flatMap((tag) => checkReference(index.file, tag.attrs.href) ?? []),
);
for (const file of firstPaint) {
  if (!entries.includes(file) && !preloaded.has(file)) {
    fail(index.file, `add a modulepreload for ${relative(site, file)}`);
  }
}
for (const file of preloaded) {
  if (!firstPaint.has(file)) {
    fail(index.file, `${relative(site, file)} is preloaded but not needed`);
  }
}

const gzipped = (paths) =>
  [...paths].reduce(
    (sum, path) => sum + gzipSync(readFileSync(path), { level: 9 }).length,
    0,
  );
const size = (paths) =>
  paths.reduce((sum, path) => sum + statSync(path).size, 0);

const pageSize = gzipped([index.file, ...withExtension(".css")]);
const scriptSize = gzipped(withExtension(".js"));
const fontSize = size(withExtension(".woff2")); // already compressed
const measured = {
  "HTML + CSS": pageSize,
  "JavaScript at first paint": gzipped(firstPaint),
  "All JavaScript": scriptSize,
  Fonts: fontSize,
  // The worst case: every script and font, plus the icon and the manifest.
  "Whole first load":
    pageSize +
    scriptSize +
    fontSize +
    gzipped(withExtension("favicon.svg", "favicon.ico", "site.webmanifest")),
};

for (const [name, bytes] of Object.entries(measured)) {
  const budget = BUDGETS[name];
  console.log(
    `${bytes > budget ? "OVER" : "ok  "}  ${name.padEnd(26)} ${String(bytes).padStart(6)} / ${budget} bytes`,
  );
  if (bytes > budget) {
    fail(site, `${name} is ${bytes - budget} bytes over its budget`);
  }
}

if (failures.length) {
  console.error(`\n${failures.length} problem(s) in ${site}:`);
  for (const failure of new Set(failures)) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`\nSite checks passed (${files.length} files, origin ${origin}).`);
