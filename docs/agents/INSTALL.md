# Minutae: install brief for AI agents

You were asked to install, update, check or build Minutae for a person. This file tells you which guide to follow and what not to do. It's written for coding agents (Claude Code, Codex, Cursor, Gemini, …) acting on someone's own machine.

Working on Minutae's code instead? Read [CLAUDE.md](../../CLAUDE.md), not this file.

## 1. Use the brief that matches the version

This file describes the release it was tagged with. If the person wants a specific version, read this file at that tag:
`https://github.com/CinematicGenius007/minutae/blob/v<version>/docs/agents/INSTALL.md`.
Each release's notes link their own copy. Otherwise use `main`.

## 2. Work out the machine

| Check | Command |
|---|---|
| OS | `uname -s` (`Darwin` = macOS, `Linux`); Windows: `$PSVersionTable.Platform` / `$env:OS` |
| CPU | `uname -m` (`arm64`, `x86_64`); Windows: `$env:PROCESSOR_ARCHITECTURE` |
| macOS version | `sw_vers -productVersion` (needs 13+) |
| Already installed? | `sh scripts/check-update.sh --json` (macOS/Linux) or `scripts/check-update.ps1 -Json` (Windows). `status` + `channel` tell you what's there |

## 3. Pick the route

| Machine | Route | Follow |
|---|---|---|
| macOS 13+, arm64 or x86_64 | `install.sh` | [docs/install/macos.md](../install/macos.md) |
| Windows 10/11 x64 | Scoop | [docs/install/windows.md](../install/windows.md) |
| Linux x86_64 | `install.sh` | [docs/install/linux.md](../install/linux.md) |
| Linux arm64, or the person wants a build from code | from source | [docs/install/source.md](../install/source.md) |
| Already installed | update through the **same** channel it came from | `check-update` prints the command (`scoop update minutae`, `install.sh`, or `git pull && … --from-source`) |

Commands, exactly:

```sh
# macOS / Linux, release
curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/install.sh | sh
# a specific version
curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/install.sh | sh -s -- --version 0.5.0
# preview without changing anything
curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/install.sh | sh -s -- --dry-run
```

```powershell
# Windows (Scoop must be installed: https://scoop.sh)
scoop install https://github.com/CinematicGenius007/minutae/releases/latest/download/minutae.json
scoop update minutae
```

Read `scripts/install.sh` before you run it if your rules ask you to review downloaded scripts; it's short and commented.

## 4. Verify

- `check-update` reports `upToDate` with the version you installed.
- macOS: `ls /Applications/Minutae.app ~/Applications/Minutae.app` (one exists). Linux: `ls ~/.local/bin/minutae`. Windows: `scoop list minutae`.

That's enough. **Don't launch Minutae to test it**: it asks for the microphone and system audio, and opening those is the person's choice. Tell them it's installed and that the first launch will ask for permissions.

## 5. Never

- Use `sudo` or run the installer as administrator. Nothing here needs it (the Linux FUSE package is the person's call; tell them the command).
- Turn off Gatekeeper or SmartScreen system-wide (`spctl --master-disable`, registry edits). `install.sh` clears quarantine on `Minutae.app` only.
- Grant, reset or edit privacy permissions for the person (`tccutil`, TCC database, Accessibility lists). macOS asks them directly.
- Open the microphone, camera or screen capture, or start a recording, without the person asking for that run.
- Skip the checksum check, or install from a mirror, fork or unofficial package.
- Read or copy credentials of other apps (Codex, Claude, Grok, API keys). Minutae's AI settings are set up by the person in the app.
- Pass skip-permission or auto-approve flags to any AI CLI as part of setup.
- Delete `~/Library/Minutae`, `%APPDATA%\Minutae` or `~/.local/share/minutae`. That's the person's recordings.

## 6. When something fails

| Message | Do |
|---|---|
| `No published release yet` / `couldn't find a published release` | Use the source route, or tell the person no release exists yet |
| `checksum mismatch` | Stop. Report it. Don't retry from another source |
| `Minutae is running` | Ask the person to quit Minutae, then rerun |
| `no Linux release for aarch64` | Source route |
| AppImage won't start (`libfuse.so.2`) | Tell the person to install FUSE 2 (see linux.md) |
| GitHub rate limit (403/429) | Wait, or pass `--version <v>` so the script skips the API lookup |
