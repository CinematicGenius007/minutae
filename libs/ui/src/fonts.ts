/**
 * Bundles Geist, Geist Mono and Newsreader (OFL) locally and registers them
 * under the exact family names the tokens use ("Geist", "Geist Mono",
 * "Newsreader"), so no OS font is ever needed. Import once at app start.
 */
import geistLatin from "@fontsource-variable/geist/files/geist-latin-wght-normal.woff2?url";
import geistLatinExt from "@fontsource-variable/geist/files/geist-latin-ext-wght-normal.woff2?url";
import geistLatinItalic from "@fontsource-variable/geist/files/geist-latin-wght-italic.woff2?url";
import geistLatinExtItalic from "@fontsource-variable/geist/files/geist-latin-ext-wght-italic.woff2?url";
import monoLatin from "@fontsource-variable/geist-mono/files/geist-mono-latin-wght-normal.woff2?url";
import monoLatinExt from "@fontsource-variable/geist-mono/files/geist-mono-latin-ext-wght-normal.woff2?url";
import serifLatin from "@fontsource-variable/newsreader/files/newsreader-latin-standard-normal.woff2?url";
import serifLatinExt from "@fontsource-variable/newsreader/files/newsreader-latin-ext-standard-normal.woff2?url";
import serifLatinItalic from "@fontsource-variable/newsreader/files/newsreader-latin-standard-italic.woff2?url";

const LATIN =
  "U+0000-00FF,U+0131,U+0152-0153,U+02BB-02BC,U+02C6,U+02DA,U+02DC,U+0304,U+0308,U+0329,U+2000-206F,U+20AC,U+2122,U+2191,U+2193,U+2212,U+2215,U+FEFF,U+FFFD";
const LATIN_EXT =
  "U+0100-02BA,U+02BD-02C5,U+02C7-02CC,U+02CE-02D7,U+02DD-02FF,U+0304,U+0308,U+0329,U+1D00-1DBF,U+1E00-1E9F,U+1EF2-1EFF,U+2020,U+20A0-20AB,U+20AD-20C0,U+2113,U+2C60-2C7F,U+A720-A7FF";

interface Face {
  family: string;
  url: string;
  weight: string;
  style?: "normal" | "italic";
  range?: string;
}

const FACES: Face[] = [
  { family: "Geist", url: geistLatinExt, weight: "100 900", range: LATIN_EXT },
  { family: "Geist", url: geistLatin, weight: "100 900", range: LATIN },
  { family: "Geist", url: geistLatinExtItalic, weight: "100 900", style: "italic", range: LATIN_EXT },
  { family: "Geist", url: geistLatinItalic, weight: "100 900", style: "italic", range: LATIN },
  { family: "Geist Mono", url: monoLatinExt, weight: "100 900", range: LATIN_EXT },
  { family: "Geist Mono", url: monoLatin, weight: "100 900", range: LATIN },
  { family: "Newsreader", url: serifLatinExt, weight: "200 800", range: LATIN_EXT },
  { family: "Newsreader", url: serifLatin, weight: "200 800", range: LATIN },
  { family: "Newsreader", url: serifLatinItalic, weight: "200 800", style: "italic", range: LATIN },
];

export function fontFaceCss(faces: Face[] = FACES): string {
  return faces
    .map(
      (f) =>
        `@font-face{font-family:"${f.family}";font-style:${f.style ?? "normal"};font-display:swap;font-weight:${f.weight};src:url("${f.url}") format("woff2-variations"),url("${f.url}") format("woff2");${f.range ? `unicode-range:${f.range};` : ""}}`,
    )
    .join("\n");
}

const STYLE_ID = "minutae-fonts";

export function installFonts(doc: Document | undefined = globalThis.document): void {
  if (!doc || doc.getElementById(STYLE_ID)) return;
  const style = doc.createElement("style");
  style.id = STYLE_ID;
  style.textContent = fontFaceCss();
  doc.head.appendChild(style);
}

installFonts();
