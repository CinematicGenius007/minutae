#!/usr/bin/env bun
// Minutae v4 version: one source, `[workspace.package] version` in the root Cargo.toml.
// tauri.conf.json has no `version`, so Tauri reads it from apps/desktop/src-tauri/Cargo.toml
// (which inherits the workspace version). The JS packages are kept equal for tidiness.
//
//   bun scripts/version.ts              print the version
//   bun scripts/version.ts 0.5.0        set it everywhere (+ Cargo.lock), then commit and tag v0.5.0
//   bun scripts/version.ts --check 0.5.0   exit 1 unless every file says 0.5.0 (release workflow)
//   bun scripts/version.ts --check      exit 1 unless every file agrees
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const CARGO = join(root, "Cargo.toml");
const PACKAGES = ["apps/desktop/package.json", "libs/ui/package.json"].map((p) => join(root, p));
const TAURI_CONF = join(root, "apps/desktop/src-tauri/tauri.conf.json");
const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;

function cargoVersion(): string {
  const m = readFileSync(CARGO, "utf8").match(/\[workspace\.package\][^[]*?\nversion = "([^"]+)"/);
  if (!m) throw new Error("No [workspace.package] version in Cargo.toml");
  return m[1];
}

function versions(): Record<string, string> {
  const out: Record<string, string> = { "Cargo.toml": cargoVersion() };
  for (const p of PACKAGES) out[p.slice(root.length + 1)] = JSON.parse(readFileSync(p, "utf8")).version;
  return out;
}

function check(expected?: string): number {
  const v = versions();
  const want = expected ?? v["Cargo.toml"];
  let ok = true;
  for (const [file, got] of Object.entries(v)) {
    if (got !== want) {
      console.error(`${file}: ${got} (expected ${want})`);
      ok = false;
    }
  }
  if ("version" in JSON.parse(readFileSync(TAURI_CONF, "utf8"))) {
    console.error("tauri.conf.json must not set `version` (it comes from Cargo.toml).");
    ok = false;
  }
  if (ok) console.log(`version ${want} ✓`);
  return ok ? 0 : 1;
}

function set(next: string) {
  if (!SEMVER.test(next)) throw new Error(`Not a version: ${next} (use 1.2.3 or 1.2.3-beta.1)`);
  const cargo = readFileSync(CARGO, "utf8").replace(
    /(\[workspace\.package\][^[]*?\nversion = ")[^"]+(")/,
    `$1${next}$2`,
  );
  writeFileSync(CARGO, cargo);
  for (const p of PACKAGES) {
    writeFileSync(p, readFileSync(p, "utf8").replace(/("version":\s*")[^"]+(")/, `$1${next}$2`));
  }
  // Workspace crate versions live in Cargo.lock too (CI builds with --locked).
  const r = Bun.spawnSync(["cargo", "update", "--workspace", "--offline"], { cwd: root, stdio: ["inherit", "inherit", "inherit"] });
  if (r.exitCode !== 0) throw new Error("cargo update --workspace failed");
  // bun.lock records workspace package versions (CI installs with --frozen-lockfile).
  const b = Bun.spawnSync(["bun", "install"], { cwd: root, stdio: ["inherit", "inherit", "inherit"] });
  if (b.exitCode !== 0) throw new Error("bun install failed");
  console.log(`Set ${next}. Next: git commit -am "chore: release v${next}" && git tag v${next} && git push --follow-tags`);
}

const args = process.argv.slice(2);
if (args[0] === "--check") process.exit(check(args[1]?.replace(/^v/, "")));
else if (args[0]) set(args[0].replace(/^v/, ""));
else console.log(cargoVersion());
