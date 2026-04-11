# Contributing to Minutae

Thanks for your interest! This document covers everything you need to run,
build, and contribute to Minutae without an Apple Developer account.

---

## Running locally (dev mode)

### 1. Install prerequisites

```bash
# Rust (required for Tauri)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Xcode Command Line Tools
xcode-select --install

# Node.js ≥ 20 (via Homebrew or nodejs.org)
brew install node

# pnpm
npm i -g pnpm
```

### 2. Clone and run

```bash
git clone https://github.com/your-org/minutae
cd minutae
pnpm install
pnpm tauri dev        # opens the app with hot-reload
```

The first run compiles Rust (~3–5 min). Subsequent runs are fast because of cargo's
incremental compilation.

---

## Building a release binary locally

```bash
pnpm tauri build
```

Output is in `src-tauri/target/release/bundle/`:

```
macos/
  Minutae.app        ← drag to /Applications
  Minutae_0.1.0_x64.dmg   (or aarch64)
```

**No signing required for personal/developer use.** The `.app` you build yourself
is not quarantined by macOS because it was never downloaded from the internet —
Gatekeeper only checks apps that have the `com.apple.quarantine` extended attribute.

---

## Sharing unsigned builds with others

If you build a DMG and send it to someone else, their Mac will quarantine it on
download. They just need to run one command after dragging the app to `/Applications`:

```bash
xattr -cr /Applications/Minutae.app
```

That removes the quarantine attribute and lets the app open normally. It is safe
for apps from trusted sources and is the standard approach for unsigned open-source
Mac apps.

Alternatively, they can right-click → **Open** in Finder the first time, which
presents an override button.

---

## About code signing and notarization

Signing requires an **Apple Developer Program** membership (~$99 USD/year).
Notarization (required for seamless distribution on macOS 13+) requires the same.

For an open-source project, the recommended paths are:

| Path | Cost | Friction |
|---|---|---|
| Build from source | Free | One-time 5-min setup |
| Unsigned DMG + `xattr -cr` | Free | One command on first install |
| Signed + notarized release | $99/yr | Zero friction for end users |

We currently ship **unsigned builds** from CI. If you want to help set up signing,
open an issue — it requires access to the repository secrets.

### CI signing (for maintainers)

Add these secrets to the GitHub repo (**Settings → Secrets → Actions**):

| Secret | How to get it |
|---|---|
| `APPLE_CERTIFICATE` | Export your Developer ID Application cert from Keychain as `.p12`, then `base64 -i cert.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | The password you set when exporting the `.p12` |
| `APPLE_SIGNING_IDENTITY` | `security find-identity -v -p codesigning` → copy the "Developer ID Application: ..." string |
| `APPLE_ID` | Your Apple ID email |
| `APPLE_PASSWORD` | App-specific password from [appleid.apple.com](https://appleid.apple.com) |
| `APPLE_TEAM_ID` | 10-character string from [developer.apple.com/account](https://developer.apple.com/account) |

Once set, the release workflow signs and notarizes automatically on every `v*` tag push.

---

## Code quality

```bash
pnpm typecheck          # tsc --noEmit (strict)
pnpm lint               # Biome check
pnpm lint:fix           # Biome check --write (auto-fix safe rules)
pnpm format             # Biome format --write

cd src-tauri
cargo check             # Rust type check
cargo clippy            # Rust lints
```

All CI checks must pass before merging.

---

## Project conventions

- **No `any` in TypeScript.** Strict mode is on.
- **Biome for formatting.** Run `pnpm lint:fix` before committing.
- **CSS excluded from Biome.** Tailwind v4 `@theme {}` syntax isn't supported yet.
- **Zustand stores** follow the `enqueue` + `_processNext` queue pattern for async jobs.
- **Providers are interfaces.** Don't hardcode Gemini/Claude/OpenAI — go through the factory functions in `src/lib/providers/`.
- **Local-first.** Nothing should require internet except AI API calls (which are opt-in).

---

## Opening a PR

1. Fork and create a branch: `git checkout -b feat/your-feature`
2. Make your changes and run all checks
3. Open a PR against `main` with a clear description
4. Reference the relevant phase/item in [PLAN.md](./PLAN.md) if applicable

For significant features or architectural changes, open an issue first to discuss.
