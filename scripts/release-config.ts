#!/usr/bin/env bun
// Writes the per-build Tauri config overlay used by .github/workflows/release.yml
// (`tauri build --config <file>`). Everything is optional: with no secrets the overlay is
// `{}` and the build is unsigned, without updater artifacts.
//
//   bun scripts/release-config.ts <out.json>
//
// Reads (from the environment, empty = missing):
//   TAURI_SIGNING_PRIVATE_KEY   updater signing key → bundle.createUpdaterArtifacts = true
//   TAURI_UPDATER_PUBKEY        updater public key (else the one committed in tauri.conf.json)
//   AZURE_CLIENT_ID / AZURE_CLIENT_SECRET / AZURE_TENANT_ID +
//   AZURE_SIGNING_ENDPOINT / AZURE_SIGNING_ACCOUNT / AZURE_SIGNING_PROFILE
//                               Windows: Azure Trusted Signing → bundle.windows.signCommand
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const env = (k: string) => process.env[k]?.trim() || "";
const out = process.argv[2];
if (!out) throw new Error("usage: bun scripts/release-config.ts <out.json>");

const conf = JSON.parse(readFileSync(join(import.meta.dir, "../apps/desktop/src-tauri/tauri.conf.json"), "utf8"));
const overlay: Record<string, Record<string, unknown>> = {};
const notes: string[] = [];

// Updater: artifacts (.app.tar.gz / .nsis.zip / .AppImage + .sig, latest.json) need the
// private key at build time; the app needs the public key compiled in.
const pubkey = env("TAURI_UPDATER_PUBKEY") || conf.plugins?.updater?.pubkey?.trim() || "";
// CI only passes a presence flag here. The private key goes to the build itself.
const hasSigningKey = env("HAS_TAURI_SIGNING_KEY") === "true" || !!env("TAURI_SIGNING_PRIVATE_KEY");
if (hasSigningKey && pubkey) {
  overlay.bundle = { ...overlay.bundle, createUpdaterArtifacts: true };
  overlay.plugins = { updater: { ...conf.plugins.updater, pubkey } };
  notes.push("updater: on (signed update artifacts + latest.json)");
} else {
  const missing = [!hasSigningKey && "TAURI_SIGNING_PRIVATE_KEY", !pubkey && "the updater public key"].filter(Boolean).join(" and ");
  const warning = `Missing ${missing}. This build can check for new releases, but cannot install updates in the app. See docs/RELEASE.md for setup.`;
  notes.push(`updater: ${warning}`);
  const summary = env("GITHUB_STEP_SUMMARY");
  if (summary) appendFileSync(summary, `- **In-app updates disabled.** ${warning}\n`);
}

// Windows: Azure Trusted Signing through trusted-signing-cli (installed by the workflow).
const azure = ["AZURE_CLIENT_ID", "AZURE_CLIENT_SECRET", "AZURE_TENANT_ID", "AZURE_SIGNING_ENDPOINT", "AZURE_SIGNING_ACCOUNT", "AZURE_SIGNING_PROFILE"];
if (process.platform === "win32" || env("RELEASE_CONFIG_OS") === "windows") {
  if (azure.every((k) => env(k))) {
    overlay.bundle = {
      ...overlay.bundle,
      windows: {
        signCommand: `trusted-signing-cli -e ${env("AZURE_SIGNING_ENDPOINT")} -a ${env("AZURE_SIGNING_ACCOUNT")} -c ${env("AZURE_SIGNING_PROFILE")} -d Minutae %1`,
      },
    };
    notes.push("windows signing: Azure Trusted Signing");
  } else {
    notes.push("windows signing: off (unsigned; SmartScreen will warn)");
  }
}

writeFileSync(out, `${JSON.stringify(overlay, null, 2)}\n`);
for (const n of notes) console.log(n);
