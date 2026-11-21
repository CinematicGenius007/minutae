# Releasing Minutae v4

`.github/workflows/release.yml` builds macOS (Apple silicon + Intel `.dmg`), Windows (NSIS `-setup.exe`, `.msi`, and a portable `.zip` for Scoop) and Linux (`.AppImage`, `.deb`, `.rpm` on Ubuntu 22.04) and attaches them to a **draft** GitHub Release with `SHA256SUMS.txt` and the Scoop manifest `minutae.json`. You publish the draft by hand.

**Releasing costs nothing.** There is no Apple Developer membership and no Windows code-signing certificate. What that means for users, and how we get around it:

| Without | Effect | Our free answer |
|---|---|---|
| Apple notarization ($99/yr) | A `.dmg` downloaded in a browser is blocked by Gatekeeper | `scripts/install.sh` downloads the release itself and clears the quarantine flag, so Gatekeeper never sees it |
| A paid signing identity | Ad-hoc signatures change every build, so macOS forgets mic / system-audio / accessibility permissions after each update | A **self-signed certificate** (`scripts/macos-self-signed-cert.sh`) used for every release keeps one identity |
| Windows code signing | SmartScreen warns on the installer | Windows installs go through **Scoop**, which doesn't trigger SmartScreen |

It works today with no secrets at all: macOS is ad-hoc signed and the app checks GitHub for new versions but can't install them itself. Each group of secrets below switches one thing on.

## Updates per install channel

The app works out how it was installed (`apps/desktop/src-tauri/src/updater.rs`) and only ever shows one way to update:

| Installed with | Detected by | Settings → Updates |
|---|---|---|
| Release download / `install.sh` | built with `MINUTAE_CHANNEL=release` | **Install and restart** when the updater key exists; otherwise shows the `install.sh` command (macOS/Linux) or the releases link (Windows) |
| Scoop | exe under `…\scoop\apps\` | shows `scoop update minutae` |
| Source build | anything else | shows `git pull && ./scripts/install.sh --from-source` |

Checks only run when the user presses **Check for updates**. Without the updater key the check asks the GitHub API (`/repos/…/releases/latest`), so it works with no secrets. `MINUTAE_UPDATE_CHECK=off` turns checks off. `scripts/check-update.sh` / `.ps1` do the same check from a terminal.

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

## What to create (all free)

| For | What | How |
|---|---|---|
| Stable macOS identity (permissions survive updates) | Self-signed code-signing certificate | `sh scripts/macos-self-signed-cert.sh` → three secrets (below) |
| In-app "Install and restart" | Updater key pair | `bun --cwd apps/desktop tauri signer generate -w ~/.tauri/minutae.key` |

Keep both in a password manager. Losing the certificate means users grant permissions once more; losing the updater key means installed copies can't self-update and users reinstall with `install.sh`.

### Hand-test once (owner)

After the first release signed with the self-signed certificate: install it with `install.sh`, grant the microphone, then install the next release the same way and confirm Minutae still has the microphone without asking. This opens your mic, so only you run it.

### Paid options, not planned

Kept in the workflow so they can be switched on later without code changes: Apple Developer ID + notarization ($99/year; replaces the self-signed certificate in the same secrets), Azure Trusted Signing for Windows (about $10/month). The free **SignPath Foundation** programme is another Windows option for MIT projects; it needs an application and public CI builds.

## GitHub secrets and variables

Repository → Settings → Secrets and variables → Actions.

### macOS signing (secrets)

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | base64 of the `.p12` from `scripts/macos-self-signed-cert.sh` (it copies it to the clipboard) |
| `APPLE_CERTIFICATE_PASSWORD` | The password the script prints |
| `APPLE_SIGNING_IDENTITY` | `Minutae Self-Signed` (required: the certificate isn't trusted by macOS, so Tauri can't find it on its own) |

Checked locally: `codesign` signs with the untrusted self-signed identity, and the designated requirement is `identifier "com.nebula.minutae" and certificate leaf = H"<cert hash>"`, the same for every build.

### macOS notarization (paid, optional; pick one)

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

### Windows signing (Azure Trusted Signing, paid, optional)

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

With both keys present the build sets `bundle.createUpdaterArtifacts`, signs the update bundles (`.app.tar.gz`, NSIS `-setup.exe`, `.AppImage`) and uploads `latest.json`. Without them the app still finds new versions through the GitHub API and shows the command that updates it.

Linux: only the AppImage updates itself. `.deb` / `.rpm` users update through a new download.

### Scoop

Nothing to set up. Each release uploads `minutae.json`, and users install from the "latest" URL:

```powershell
scoop install https://github.com/CinematicGenius007/minutae/releases/latest/download/minutae.json
```

Scoop remembers that URL and re-reads it on `scoop update minutae`. The manifest has `checkver` / `autoupdate`, so it can move into a bucket repo later (`scoop bucket add minutae <repo>`) without changes. Generated by `scripts/scoop-manifest.ts`.

## How it fits together

| Piece | Where |
|---|---|
| Release workflow | `.github/workflows/release.yml` |
| Per-build config overlay (updater on, Windows signCommand) | `scripts/release-config.ts` → `tauri build --config src-tauri/tauri.release.conf.json` |
| Self-signed macOS certificate | `scripts/macos-self-signed-cert.sh` |
| Scoop manifest | `scripts/scoop-manifest.ts` (run by the `checksums` job) |
| Version check / bump | `scripts/version.ts` |
| Hardened runtime, entitlements, usage strings | `tauri.conf.json` `bundle.macOS`, `Entitlements.plist`, `Info.plist` |
| Linux runtime packages | `tauri.conf.json` `bundle.linux.deb.depends` / `rpm.depends` (WebKitGTK and GTK are added by Tauri) |
| Updater (Rust state machine + commands) | `apps/desktop/src-tauri/src/updater.rs`, contract in `docs/IPC.md` |

Native libraries: whisper.cpp and sherpa-onnx/onnxruntime are linked **statically** (`minutae-diarize` default feature `static`), so the bundles carry no extra `.dylib` / `.so` / `.dll` files and nothing extra needs signing. Check with `otool -L Minutae.app/Contents/MacOS/minutae-desktop` (only `/System` and `/usr/lib` entries). If you ever switch to the `shared` feature, add the libraries to `bundle.macOS.frameworks` / `bundle.resources` so Tauri signs them, and add `com.apple.security.cs.disable-library-validation` only if they can't be signed with the same team ID.

## Supply chain

- Every action in `release.yml` and `v4.yml` is pinned to a commit SHA (the tag is in a comment). To update one, resolve the new tag with `gh api repos/<owner>/<action>/commits/<tag> --jq .sha`.
- Workflows are read-only by default. Only the `build` and `checksums` jobs get `contents: write`, to upload release files.
- Signing secrets are exported only in the step right before the Tauri build, and cleared right after it. Earlier steps (tool installs, the config overlay) never see them. Dependency build scripts still run in that build step; that can't be avoided without splitting compile and signing.
- The `checksums` job attests build provenance for every file in `SHA256SUMS.txt` (free on public repos). Anyone can check a download with `gh attestation verify <file> --repo CinematicGenius007/minutae`. `install.sh` does this automatically when `gh` is signed in. `SHA256SUMS.txt` on its own only catches corrupt downloads, because it sits on the same release page as the files.
- Updates waiting to restart: on macOS and Linux the installed update waits for any recording or dictation to finish before the app restarts. On Windows the installer closes the app itself, so the app checks for a recording only before the download starts.

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
