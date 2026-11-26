import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const CSS = readFileSync(resolve(import.meta.dir, "tokens.css"), "utf8");

type TokenMap = Record<string, string>;

function extractTokens(block: string): TokenMap {
  const out: TokenMap = {};
  for (const m of block.matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/gi)) {
    const name = m[1];
    const value = m[2];
    if (name && value) out[name] = value.trim();
  }
  return out;
}

/** Extracts the CSS body inside the first balanced `{ ... }` starting at `startIndex`. */
function balancedBlock(src: string, openBraceIndex: number): string {
  let depth = 0;
  for (let i = openBraceIndex; i < src.length; i++) {
    if (src[i] === "{") depth++;
    else if (src[i] === "}") {
      depth--;
      if (depth === 0) return src.slice(openBraceIndex + 1, i);
    }
  }
  throw new Error("Unbalanced CSS braces in tokens.css");
}

function findBlock(src: string, selectorPattern: RegExp): string {
  const m = selectorPattern.exec(src);
  if (!m) throw new Error(`Missing CSS block matching ${selectorPattern}`);
  const brace = src.indexOf("{", m.index);
  return balancedBlock(src, brace);
}

function parseHex(hex: string): [number, number, number] {
  const clean = hex.trim();
  if (!/^#[0-9a-f]{6}$/i.test(clean)) throw new Error(`Expected 6-digit hex color, got: ${hex}`);
  return [
    Number.parseInt(clean.slice(1, 3), 16),
    Number.parseInt(clean.slice(3, 5), 16),
    Number.parseInt(clean.slice(5, 7), 16),
  ];
}

function relativeLuminance(hex: string): number {
  const [r, g, b] = parseHex(hex).map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * (r ?? 0) + 0.7152 * (g ?? 0) + 0.0722 * (b ?? 0);
}

function contrastRatio(a: string, b: string): number {
  const la = relativeLuminance(a);
  const lb = relativeLuminance(b);
  const lighter = Math.max(la, lb);
  const darker = Math.min(la, lb);
  return (lighter + 0.05) / (darker + 0.05);
}

/** Blends foreground `fg` at alpha `t` over background `bg` (sRGB). */
function blend(fg: string, bg: string, alpha: number): string {
  const f = parseHex(fg);
  const b = parseHex(bg);
  const channels = f.map((v, i) => Math.round(v * alpha + (b[i] ?? 0) * (1 - alpha)));
  return `#${channels.map((c) => c.toString(16).padStart(2, "0")).join("")}`;
}

/** Resolves `var(--token)` chains and simple `color-mix(in srgb, var(--a) N%, transparent)` over `base`. */
function resolveColor(tokens: TokenMap, expr: string, backdrop?: string): string {
  const trimmed = expr.trim();
  if (/^#[0-9a-f]{6}$/i.test(trimmed)) return trimmed;
  const varMatch = /^var\((--[a-z0-9-]+)\)$/i.exec(trimmed);
  if (varMatch?.[1]) {
    const next = tokens[varMatch[1]];
    if (!next) throw new Error(`Undefined token ${varMatch[1]}`);
    return resolveColor(tokens, next, backdrop);
  }
  const mixMatch = /^color-mix\(in srgb,\s*var\((--[a-z0-9-]+)\)\s+(\d+)%,\s*transparent\)$/i.exec(trimmed);
  if (mixMatch?.[1] && mixMatch[2] && backdrop) {
    const fg = resolveColor(tokens, `var(${mixMatch[1]})`);
    const bg = resolveColor(tokens, backdrop);
    return blend(fg, bg, Number(mixMatch[2]) / 100);
  }
  throw new Error(`Unsupported color expression: ${expr}`);
}

// Split tokens.css into base sections and the prefers-contrast: more section.
const contrastIdx = CSS.indexOf("@media (prefers-contrast: more)");
if (contrastIdx === -1) throw new Error("tokens.css must define @media (prefers-contrast: more)");
const baseCss = CSS.slice(0, contrastIdx);
const contrastBlock = balancedBlock(CSS, CSS.indexOf("{", contrastIdx));

// Light tokens: merge all top-level :root { ... } blocks in baseCss.
const lightBase: TokenMap = {};
for (const m of baseCss.matchAll(/(?:^|\n):root\s*\{/g)) {
  Object.assign(lightBase, extractTokens(balancedBlock(baseCss, baseCss.indexOf("{", m.index))));
}

// Dark explicit blocks (`:root[data-theme="dark"]`) in baseCss.
const darkExplicit: TokenMap = { ...lightBase };
for (const m of baseCss.matchAll(/:root\[data-theme="dark"\]\s*\{/g)) {
  Object.assign(darkExplicit, extractTokens(balancedBlock(baseCss, baseCss.indexOf("{", m.index))));
}

// Dark prefers-color-scheme blocks (`@media (prefers-color-scheme: dark)`) in baseCss.
const darkMedia: TokenMap = { ...lightBase };
for (const m of baseCss.matchAll(/@media\s*\(prefers-color-scheme:\s*dark\)\s*\{/g)) {
  const inner = balancedBlock(baseCss, baseCss.indexOf("{", m.index));
  const rootBody = findBlock(inner, /:root:not\(\[data-theme="light"\]\)\s*\{/);
  Object.assign(darkMedia, extractTokens(rootBody));
}

// High-contrast blocks inside `@media (prefers-contrast: more)`.
const lightMore: TokenMap = { ...lightBase, ...extractTokens(findBlock(contrastBlock, /(?:^|\n)\s*:root\s*\{/)) };
const darkMoreExplicit: TokenMap = {
  ...darkExplicit,
  ...extractTokens(findBlock(contrastBlock, /:root\[data-theme="dark"\]\s*\{/)),
};
const darkMoreMediaInner = findBlock(contrastBlock, /@media\s*\(prefers-color-scheme:\s*dark\)\s*\{/);
const darkMoreMedia: TokenMap = {
  ...darkMedia,
  ...extractTokens(findBlock(darkMoreMediaInner, /:root:not\(\[data-theme="light"\]\)\s*\{/)),
};

const COLOR_KEYS = [
  "--bg",
  "--ink",
  "--muted",
  "--faint",
  "--surface",
  "--raised",
  "--line",
  "--line-strong",
  "--control-edge",
  "--on-ink",
  "--onair",
  "--onair-fill",
  "--on-onair",
  "--destructive",
  "--ok",
  "--hud",
  "--toggle-knob-off",
] as const;

describe("tokens.css WCAG contrast", () => {
  test("explicit dark and prefers-color-scheme dark blocks match identically", () => {
    for (const k of COLOR_KEYS) {
      expect(darkExplicit[k], `dark token ${k}`).toBe(darkMedia[k]);
      expect(darkMoreExplicit[k], `prefers-contrast:more dark token ${k}`).toBe(darkMoreMedia[k]);
    }
  });

  for (const [themeName, tokens] of [
    ["light", lightBase],
    ["dark", darkExplicit],
  ] as const) {
    describe(`${themeName} theme`, () => {
      const textPairs: [string, string, string][] = [
        ["--ink on --bg", "--ink", "--bg"],
        ["--ink on --surface", "--ink", "--surface"],
        ["--ink on --raised", "--ink", "--raised"],
        ["--muted on --bg", "--muted", "--bg"],
        ["--muted on --surface", "--muted", "--surface"],
        ["--muted on --raised", "--muted", "--raised"],
        ["--placeholder on --surface", "--placeholder", "--surface"],
        ["--placeholder on --bg", "--placeholder", "--bg"],
        ["--on-ink on --ink", "--on-ink", "--ink"],
        ["--on-ink-muted on --ink", "--on-ink-muted", "--ink"],
        ["--on-onair on --onair-fill", "--on-onair", "--onair-fill"],
        ["--destructive on --bg", "--destructive", "--bg"],
        ["--destructive on --surface", "--destructive", "--surface"],
        ["--on-hud on --hud", "--on-hud", "--hud"],
        ["--on-hud-muted on --hud", "--on-hud-muted", "--hud"],
      ];

      for (const [label, fgTok, bgTok] of textPairs) {
        test(`text ${label} >= 4.5:1`, () => {
          const bg = resolveColor(tokens, `var(${bgTok})`);
          const fg = resolveColor(tokens, `var(${fgTok})`, `var(${bgTok})`);
          const ratio = contrastRatio(fg, bg);
          expect(ratio, `${themeName} ${label}: ${fg} on ${bg} = ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5);
        });
      }

      const uiPairs: [string, string, string][] = [
        ["--control-edge on --bg (control boundary)", "--control-edge", "--bg"],
        ["--control-edge on --surface (control boundary / disabled text)", "--control-edge", "--surface"],
        ["--toggle-knob-off on --control-edge (off toggle knob)", "--toggle-knob-off", "--control-edge"],
        ["--on-ink on --ink (on toggle knob)", "--on-ink", "--ink"],
        ["focus ring --ink on --bg", "--ink", "--bg"],
        ["focus ring --ink on --surface", "--ink", "--surface"],
        ["focus ring --ink on --raised", "--ink", "--raised"],
      ];

      for (const [label, fgTok, bgTok] of uiPairs) {
        test(`non-text ${label} >= 3.0:1`, () => {
          const bg = resolveColor(tokens, `var(${bgTok})`);
          const fg = resolveColor(tokens, `var(${fgTok})`, `var(${bgTok})`);
          const ratio = contrastRatio(fg, bg);
          expect(ratio, `${themeName} ${label}: ${fg} on ${bg} = ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(3.0);
        });
      }
    });
  }

  for (const [themeName, base, more] of [
    ["light", lightBase, lightMore],
    ["dark", darkExplicit, darkMoreExplicit],
  ] as const) {
    test(`prefers-contrast: more strengthens muted, control-edge and hairlines in ${themeName}`, () => {
      const bg = resolveColor(more, "var(--bg)");
      const surface = resolveColor(more, "var(--surface)");
      const raised = resolveColor(more, "var(--raised)");

      const baseMuted = contrastRatio(resolveColor(base, "var(--muted)"), bg);
      const moreMuted = contrastRatio(resolveColor(more, "var(--muted)"), bg);
      expect(moreMuted).toBeGreaterThan(baseMuted);
      expect(moreMuted).toBeGreaterThanOrEqual(7.0);
      expect(contrastRatio(resolveColor(more, "var(--muted)"), surface)).toBeGreaterThanOrEqual(7.0);
      expect(contrastRatio(resolveColor(more, "var(--muted)"), raised)).toBeGreaterThanOrEqual(7.0);

      const baseEdge = contrastRatio(resolveColor(base, "var(--control-edge)"), bg);
      const moreEdge = contrastRatio(resolveColor(more, "var(--control-edge)"), bg);
      expect(moreEdge).toBeGreaterThan(baseEdge);
      expect(moreEdge).toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(resolveColor(more, "var(--control-edge)"), surface)).toBeGreaterThanOrEqual(4.5);

      const baseLine = contrastRatio(resolveColor(base, "var(--line)"), bg);
      const moreLine = contrastRatio(resolveColor(more, "var(--line)"), bg);
      expect(moreLine).toBeGreaterThan(baseLine);
      expect(moreLine).toBeGreaterThanOrEqual(3.0);
    });
  }
});
