# Releasing Minutae v4

`.github/workflows/release.yml` builds macOS (Apple silicon + Intel `.dmg`), Windows (NSIS `-setup.exe`, `.msi`, and a portable `.zip` for Scoop) and Linux (`.AppImage`, `.deb`, `.rpm` on Ubuntu 22.04). It attaches them to a **draft** GitHub Release along with `SHA256SUMS.txt`, `minutae.json` for Scoop, `minutae.rb` for Homebrew and the install and update-check scripts. Publish the draft by hand.

**Releasing costs nothing.** There is no Apple Developer membership and no Windows code-signing certificate. What that means for users, and how we get around it:

| Without | Effect | Our free answer |
|---|---|---|
| Apple notarization | Gatekeeper blocks the first launch of a quarantined app | `scripts/install.sh` verifies the download and clears quarantine for Minutae. Homebrew and manual DMG installs use **Open Anyway** |
| Stable macOS signing | With ad-hoc signing, macOS can ask for permissions again on each update | Use the same **self-signed certificate** for every release |
| Windows code signing | A browser-downloaded installer can trigger SmartScreen | Scoop and PowerShell installs usually avoid the browser's download mark. Windows 11 Smart App Control can still block unsigned code |

It works today with no secrets at all: macOS is ad-hoc signed and the app checks GitHub for new versions but can't install them itself. Each group of secrets below switches one thing on.

## Updates per install channel

The app distinguishes direct releases, Scoop and source builds (`apps/desktop/src-tauri/src/updater.rs`). Homebrew installs use the same binary as direct releases. Update those copies with Homebrew:

| Installed with | Detected by | Settings → Updates |
|---|---|---|
| Release download / `install.sh` / `install.ps1` | built with `MINUTAE_CHANNEL=release` | **Install and restart** when the release includes updater signatures. Otherwise rerun your installer or download the next release |
| Homebrew | app shows the same controls as a direct release | use `brew upgrade --cask minutae` so Homebrew tracks the installed version |
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

After publishing the first stable release, update the install status in the README and install guides.

Versions with a suffix (`0.5.0-beta.1`) are marked prerelease. MSI needs a plain `x.y.z` version; for a prerelease, expect only the NSIS installer on Windows.

## Set up signing and the updater (free)

On a Mac with the workspace dependencies installed and `gh` signed in with repository write access, run:

```sh
sh scripts/release-setup.sh --dry-run
sh scripts/release-setup.sh
```

The script asks before generating the certificate and updater key, and before writing each GitHub Actions secret or variable. It stores signing files under `~/minutae-signing` by default and reuses files already there. You can pass a different directory. If you have already released with either key, reuse the original signing material. The run summary identifies missing signing configuration and its effect on users.

Back up the signing files and passwords. Losing the certificate means users grant permissions once more. Losing the updater key means existing installations need to update through their original installer.

### Hand-test once (owner)

After the first release signed with the self-signed certificate: install it with `install.sh`, grant the microphone, then install the next release the same way and confirm Minutae still has the microphone without asking. This opens your mic, so only you run it.

### Paid options, not planned

Kept in the workflow so they can be switched on later without code changes: Apple Developer ID + notarization ($99/year; replaces the self-signed certificate in the same secrets), Azure Trusted Signing for Windows (about $10/month). The free **SignPath Foundation** programme is another Windows option for MIT projects; it needs an application and public CI builds.

## GitHub secrets and variables

Repository → Settings → Secrets and variables → Actions.

### macOS signing (secrets)

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | Base64 of the `.p12` from the certificate helper |
| `APPLE_CERTIFICATE_PASSWORD` | The `.p12` password from the helper's private password file when run through `release-setup.sh` |
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
| Secret | `TAURI_SIGNING_PRIVATE_KEY` | Contents of the updater private key (`minutae.key`) |
| Secret | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Its password (omit if none) |
| Variable | `TAURI_UPDATER_PUBKEY` | Contents of the updater public key (`minutae.key.pub`) |

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

### Homebrew

Create `CinematicGenius007/homebrew-tap` as a public repository before offering Homebrew installs. After publishing each stable release, copy its `minutae.rb` asset to `Casks/minutae.rb` in that tap and push the change. This is a manual step. `brew livecheck` reports a newer stable release, and `brew upgrade` uses the version and checksums from the tap.

Homebrew 7.0.16 requires trust for non-official casks and quarantines them on install. The [macOS install guide](install/macos.md) shows how to trust this cask and approve its first launch. Homebrew checks the new bundle against the old bundle's designated signing requirement before carrying that approval forward. Use the same self-signed certificate from the first release onward.

## How it fits together

| Piece | Where |
|---|---|
| Release workflow | `.github/workflows/release.yml` |
| Per-build config overlay (updater on, Windows signCommand) | `scripts/release-config.ts` → `tauri build --config src-tauri/tauri.release.conf.json` |
| Self-signed macOS certificate | `scripts/macos-self-signed-cert.sh` |
| Interactive GitHub signing setup | `scripts/release-setup.sh` |
| Scoop manifest | `scripts/scoop-manifest.ts` (run by the `checksums` job) |
| Homebrew cask | `scripts/homebrew-cask.ts` (run by the `checksums` job) |
| Version check / bump | `scripts/version.ts` |
| Hardened runtime, entitlements, usage strings | `tauri.conf.json` `bundle.macOS`, `Entitlements.plist`, `Info.plist` |
| Linux runtime packages | `tauri.conf.json` `bundle.linux.deb.depends` / `rpm.depends` (WebKitGTK and GTK are added by Tauri) |
| Updater (Rust state machine + commands) | `apps/desktop/src-tauri/src/updater.rs`, contract in `docs/IPC.md` |

Native libraries: whisper.cpp and sherpa-onnx/onnxruntime are linked **statically** (`minutae-diarize` default feature `static`), so the bundles carry no extra `.dylib` / `.so` / `.dll` files and nothing extra needs signing. Check with `otool -L Minutae.app/Contents/MacOS/minutae-desktop` (only `/System` and `/usr/lib` entries). If you ever switch to the `shared` feature, add the libraries to `bundle.macOS.frameworks` / `bundle.resources` so Tauri signs them, and add `com.apple.security.cs.disable-library-validation` only if they can't be signed with the same team ID.

## Supply chain

- Every action in `release.yml` and `v4.yml` is pinned to a commit SHA (the tag is in a comment). To update one, resolve the new tag with `gh api repos/<owner>/<action>/commits/<tag> --jq .sha`.
- Workflows are read-only by default. Only the `build` and `checksums` jobs get `contents: write`, to upload release files.
- The config overlay sees only a flag indicating whether the updater private key is present. Signing secrets are exported in the step before the Tauri build and cleared immediately after it. Dependency build scripts still run in the Tauri build step.
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

`actionlint .github/workflows/release.yml .github/workflows/v4.yml` checks workflow syntax. The `v4` workflow runs Playwright's Chromium and WebKit suites on `macos-latest` and uploads the report and screenshot diffs on failure. macOS image or browser rendering changes can affect screenshots made on a different Mac.
