# Installing, updating, and removing Minutae (AI agent guide)

This page summarizes how a coding agent installs, updates, verifies, or removes Minutae on a user's machine. The repository's [`docs/agents/INSTALL.md`](https://github.com/CinematicGenius007/minutae/blob/main/docs/agents/INSTALL.md) is the authority — read it (or the copy at tag `v<version>` if a specific version was requested) before running commands.

## 1. Check the machine

- OS: `uname -s` on macOS/Linux (`Darwin` / `Linux`); `$PSVersionTable.Platform` or `$env:OS` on Windows.
- CPU: `uname -m` (`arm64` / `x86_64`); `$env:PROCESSOR_ARCHITECTURE` on Windows.
- macOS version: `sw_vers -productVersion` (requires macOS 13+).
- Existing install: `sh scripts/check-update.sh --json` (macOS/Linux) or `scripts/check-update.ps1 -Json` (Windows) reports `status` and `channel`.

## 2. Install

Pick the route matching the user's machine:

- **macOS 13+ (arm64 or x86_64) or Linux x86_64:**
  ```sh
  curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/install.sh | sh
  ```
  Supports `--version <v>`, `--dir <folder>`, and `--dry-run`. The script verifies `SHA256SUMS.txt` and, when `gh` is installed and signed in, runs `gh attestation verify`. On macOS it installs to `/Applications` (or `~/Applications`) and clears quarantine only on `Minutae.app`. On Linux it installs the AppImage to `~/.local/bin/minutae` (FUSE 2 is required; installing `libfuse2` / `fuse` is the user's call).
- **Windows 10/11 (x64) via Scoop:**
  ```powershell
  scoop install https://github.com/CinematicGenius007/minutae/releases/latest/download/minutae.json
  ```
- **Linux arm64, or building from code:**
  ```sh
  git clone https://github.com/CinematicGenius007/minutae && cd minutae && sh scripts/install.sh --from-source
  ```
  (On Windows, see [`docs/install/source.md`](https://github.com/CinematicGenius007/minutae/blob/main/docs/install/source.md).)

## 3. Update

Always update through the same channel the app was installed from. Ask the user to quit Minutae first.

- macOS / Linux release: re-run `install.sh` (or run `check-update.sh` to see the exact command).
- Windows Scoop: `scoop update minutae`.
- Source install: `git pull && sh scripts/install.sh --from-source` (Windows: `git pull; bun install; bun run build`).

## 4. Verify (without launching)

- `check-update` reports `upToDate` with the installed version.
- Binary present: macOS `/Applications/Minutae.app` or `~/Applications/Minutae.app`; Linux `~/.local/bin/minutae`; Windows `scoop list minutae`.
- **Do not launch Minutae to test it.** First launch prompts for microphone and system audio permissions, which must be the user's choice.

## 5. Remove (uninstall)

Only remove when the user asks, and **never delete the user's data folder** unless they explicitly ask to remove their recordings:

- **macOS:** quit Minutae and remove `Minutae.app` from `/Applications` (or `~/Applications`). User data stays in `~/Library/Minutae/`.
- **Windows:** `scoop uninstall minutae`. User data stays in `%APPDATA%\Minutae\`.
- **Linux:** remove `~/.local/bin/minutae`, `~/.local/share/applications/minutae.desktop`, and `~/.local/share/icons/hicolor/128x128/apps/minutae.png`. User data stays in `~/.local/share/minutae/`.

## 6. Hard rules (never do these)

- Never use `sudo` or run installers as administrator.
- Never disable Gatekeeper or SmartScreen system-wide (`spctl --master-disable`, registry edits).
- Never grant, reset, or edit OS privacy permissions (`tccutil`, TCC database, Accessibility lists).
- Never open the microphone, camera, or screen capture, or start a recording.
- Never skip checksum or provenance verification, and never install from a mirror, fork, or unofficial package.
- Never read or copy credentials of other apps (Codex, Claude, Grok, API keys) or pass skip-permission / auto-approve flags to any AI CLI.
- Never delete `~/Library/Minutae`, `%APPDATA%\Minutae`, or `~/.local/share/minutae`.

## Authoritative references

- [`docs/agents/INSTALL.md`](https://github.com/CinematicGenius007/minutae/blob/main/docs/agents/INSTALL.md)
- [`docs/install/macos.md`](https://github.com/CinematicGenius007/minutae/blob/main/docs/install/macos.md)
- [`docs/install/windows.md`](https://github.com/CinematicGenius007/minutae/blob/main/docs/install/windows.md)
- [`docs/install/linux.md`](https://github.com/CinematicGenius007/minutae/blob/main/docs/install/linux.md)
- [`docs/install/source.md`](https://github.com/CinematicGenius007/minutae/blob/main/docs/install/source.md)
- [LLM summary (`/llms.txt`)](https://minutae.vercel.app/llms.txt)
