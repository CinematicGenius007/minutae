# Install Minutae

Minutae is free and open source. Installing, updating and building it costs nothing.

| You have | Install with | Guide |
|---|---|---|
| macOS 13+ (Apple silicon or Intel) | `curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/install.sh \| sh` | [macos.md](macos.md) |
| Windows 10/11 (x64) | `scoop install https://github.com/CinematicGenius007/minutae/releases/latest/download/minutae.json` | [windows.md](windows.md) |
| Linux (x86_64) | the same `install.sh` one-liner as macOS | [linux.md](linux.md) |
| Any of them, from the code | `sh scripts/install.sh --from-source` (Windows: see the guide) | [source.md](source.md) |

**Updating:** Settings → General → **Check for updates** shows the one command that updates your copy. From a terminal, `scripts/check-update.sh` (macOS/Linux) or `scripts/check-update.ps1` (Windows) does the same check without installing anything.

**AI agents** installing Minutae for someone: follow [../agents/INSTALL.md](../agents/INSTALL.md).

Your recordings and settings are never touched by installs, updates or uninstalls:
macOS `~/Library/Minutae/`, Windows `%APPDATA%\Minutae\`, Linux `~/.local/share/minutae/`.
