#!/usr/bin/env bun
// Writes the Scoop manifest (minutae.json) for one release. release.yml uploads it as a
// release asset, so this always points at the newest published release:
//
//   scoop install https://github.com/CinematicGenius007/minutae/releases/latest/download/minutae.json
//
// Scoop remembers that URL and re-reads it on `scoop update minutae`, so no bucket repo is
// needed. `checkver` / `autoupdate` are there so the same file also works in a bucket later.
//
//   bun scripts/scoop-manifest.ts <version> <sha256 of the portable zip> <out.json>
import { writeFileSync } from "node:fs";

const REPO = "CinematicGenius007/minutae";
const [version, hash, out] = process.argv.slice(2);
if (!version || !/^[0-9a-f]{64}$/i.test(hash ?? "") || !out) {
  throw new Error("usage: bun scripts/scoop-manifest.ts <version> <sha256> <out.json>");
}

const zipName = (v: string) => `Minutae_${v}_x64-portable.zip`;
const url = (v: string) => `https://github.com/${REPO}/releases/download/v${v}/${zipName(v)}`;

const manifest = {
  version,
  description: "Local-first meeting notes and dictation. Records, transcribes and summarizes on your computer.",
  homepage: `https://github.com/${REPO}`,
  license: "MIT",
  notes: [
    "Minutae keeps its data in %APPDATA%\\Minutae, so updates and uninstalls leave it alone.",
    "Update with: scoop update minutae",
  ],
  architecture: { "64bit": { url: url(version), hash: hash.toLowerCase() } },
  shortcuts: [["Minutae.exe", "Minutae"]],
  checkver: { github: `https://github.com/${REPO}` },
  autoupdate: { architecture: { "64bit": { url: url("$version") } } },
};

writeFileSync(out, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`wrote ${out} for ${version}`);
