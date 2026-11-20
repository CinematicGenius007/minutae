# Releasing Minutae v4

`.github/workflows/release.yml` builds macOS (Apple silicon + Intel `.dmg`), Windows (NSIS `-setup.exe` + `.msi`, WebView2 bootstrapper) and Linux (`.AppImage`, `.deb`, `.rpm` on Ubuntu 22.04) and attaches them to a **draft** GitHub Release with `SHA256SUMS.txt`. You publish the draft by hand.

It works today with no secrets: the builds are unsigned (macOS ad-hoc) and the in-app updater is off. Each group of secrets below turns one thing on. Nothing else changes.

The legacy Tauri v1 app has its own workflow, `release-legacy.yml`, on `legacy-v*` tags.

## Cut a release

1. Set the version (one source: `[workspace.package] version` in the root `Cargo.toml`; `tauri.conf.json` has no `version` and Tauri reads it from Cargo):
   ```bash
   bun scripts/version.ts 0.5.0        # Cargo.toml, Cargo.lock, apps/desktop + libs/ui package.json
   bun scripts/version.ts --check      # all files agree
   ```
2. Commit, tag, push:
   ```bash
   git commit -am "chore: release v0.5.0"
   git tag v0.5.0 && git push origin main --follow-tags
   ```
   Or run **Actions → Release → Run workflow** with `tag: v0.5.0` (the tag is created at the chosen branch).
3. The `prepare` job fails if the tag and `Cargo.toml` disagree.
4. When all jobs pass, open the draft release, write the notes, and **Publish**. The updater only sees published (non-draft, non-prerelease) releases: `releases/latest/download/latest.json`.

Versions with a suffix (`0.5.0-beta.1`) are marked prerelease. MSI needs a plain `x.y.z` version; for a prerelease, expect only the NSIS installer on Windows.

## What to buy or create

| For | What | Cost |
|---|---|---|
| macOS signing + notarization | Apple Developer Program membership, then a **Developer ID Application** certificate | $99/year |
| Windows signing | **Azure Trusted Signing** (now "Artifact Signing") account + certificate profile (identity validation takes a few days) | about $10/month |
| In-app updates | An updater key pair from `bun --cwd apps/desktop tauri signer generate -w ~/.tauri/minutae.key` | free |

OV/EV code-signing certificates from other vendors now ship on hardware tokens or cloud HSMs, so they can't be a plain `.pfx` secret. If you use one, put its vendor's signing command in `bundle.windows.signCommand` (see `scripts/release-config.ts`) instead of Azure.

## GitHub secrets and variables

Repository → Settings → Secrets and variables → Actions.

### macOS signing (secrets)

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | Developer ID Application certificate + private key exported from Keychain as `.p12`, then `base64 -i cert.p12 \| pbcopy` |
| `APPLE_CERTIFICATE_PASSWORD` | The password you set when exporting the `.p12` |
| `APPLE_SIGNING_IDENTITY` | Optional. e.g. `Developer ID Application: Your Name (TEAMID)`. Tauri finds it from the certificate when omitted |

### macOS notarization (secrets, pick one)

App Store Connect API key (recommended, no 2FA):

| Secret | Value |
|---|---|
| `APPLE_API_ISSUER` | Issuer ID (App Store Connect → Users and Access → Integrations → Keys) |
| `APPLE_API_KEY` | Key ID |
| `APPLE_API_PRIVATE_KEY` | Contents of the downloaded `AuthKey_<KEYID>.p8` |

Or Apple ID:

| Secret | Value |
|---|---|
| `APPLE_ID` | Apple ID email |
| `APPLE_PASSWORD` | App-specific password from appleid.apple.com |
| `APPLE_TEAM_ID` | 10-character team ID |

Signing without notarization works, but Gatekeeper still warns. Notarization needs signing.

### Windows signing (Azure Trusted Signing)

| Kind | Name | Value |
|---|---|---|
| Secret | `AZURE_CLIENT_ID` | App registration (service principal) with the *Trusted Signing Certificate Profile Signer* role |
| Secret | `AZURE_CLIENT_SECRET` | Its client secret |
| Secret | `AZURE_TENANT_ID` | Directory (tenant) ID |
| Variable | `AZURE_SIGNING_ENDPOINT` | e.g. `https://wus2.codesigning.azure.net` (your account's region) |
| Variable | `AZURE_SIGNING_ACCOUNT` | Trusted Signing account name |
| Variable | `AZURE_SIGNING_PROFILE` | Certificate profile name |

When all six are set, the workflow installs `trusted-signing-cli` and adds `bundle.windows.signCommand`, so the app `.exe`, the NSIS installer and the MSI are signed.

### In-app updater

| Kind | Name | Value |
|---|---|---|
| Secret | `TAURI_SIGNING_PRIVATE_KEY` | Contents of `~/.tauri/minutae.key` |
| Secret | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Its password (omit if none) |
| Variable | `TAURI_UPDATER_PUBKEY` | Contents of `~/.tauri/minutae.key.pub` |

Instead of the variable you can commit the public key to `apps/desktop/src-tauri/tauri.conf.json` → `plugins.updater.pubkey` (it is public). Committing it also turns updates on for local release builds.

Keep the private key and its password in a password manager. If you lose them, installed apps can never update again and users must reinstall by hand.

With both keys present the build sets `bundle.createUpdaterArtifacts`, signs the update bundles (`.app.tar.gz`, NSIS `-setup.exe`, `.AppImage`) and uploads `latest.json`. Without them the app's Settings → General → Updates row says "This build doesn't update itself." The app doesn't crash and doesn't make network calls.

Linux: only the AppImage updates itself. `.deb` / `.rpm` users update through a new download.

## How it fits together

| Piece | Where |
|---|---|
| Release workflow | `.github/workflows/release.yml` |
| Per-build config overlay (updater on, Windows signCommand) | `scripts/release-config.ts` → `tauri build --config src-tauri/tauri.release.conf.json` |
| Version check / bump | `scripts/version.ts` |
| Hardened runtime, entitlements, usage strings | `tauri.conf.json` `bundle.macOS`, `Entitlements.plist`, `Info.plist` |
| Linux runtime packages | `tauri.conf.json` `bundle.linux.deb.depends` / `rpm.depends` (WebKitGTK and GTK are added by Tauri) |
| Updater (Rust state machine + commands) | `apps/desktop/src-tauri/src/updater.rs`, contract in `docs/IPC.md` |

Native libraries: whisper.cpp and sherpa-onnx/onnxruntime are linked **statically** (`minutae-diarize` default feature `static`), so the bundles carry no extra `.dylib` / `.so` / `.dll` files and nothing extra needs signing. Check with `otool -L Minutae.app/Contents/MacOS/minutae-desktop` (only `/System` and `/usr/lib` entries). If you ever switch to the `shared` feature, add the libraries to `bundle.macOS.frameworks` / `bundle.resources` so Tauri signs them, and add `com.apple.security.cs.disable-library-validation` only if they can't be signed with the same team ID.

## Test locally

```bash
# Unsigned bundle (what CI produces with no secrets)
bun --cwd apps/desktop tauri build --debug --bundles app,dmg

# Ad-hoc signature + hardened runtime + updater artifacts with a throwaway key
bun --cwd apps/desktop tauri signer generate --ci -p "" -w /tmp/k.key
TAURI_SIGNING_PRIVATE_KEY="$(cat /tmp/k.key)" TAURI_UPDATER_PUBKEY="$(cat /tmp/k.key.pub)" \
  bun scripts/release-config.ts /tmp/release.conf.json
APPLE_SIGNING_IDENTITY=- TAURI_SIGNING_PRIVATE_KEY="$(cat /tmp/k.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
  bun --cwd apps/desktop tauri build --debug --bundles app,dmg --config /tmp/release.conf.json
```

`actionlint .github/workflows/release.yml` checks the workflow syntax.
