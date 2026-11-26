# Install Minutae

Minutae is free and open source. Installing, updating and building it costs nothing.

No release has been published yet. Use the [source build guide](source.md) to install today.

| You have | Install with | Guide |
|---|---|---|
| macOS 13+ (Apple silicon or Intel) | Shell script, then Homebrew or manual DMG | [macos.md](macos.md) |
| Windows 10/11 (x64) | Scoop, or the PowerShell installer | [windows.md](windows.md) |
| Linux (x86_64) | the same `install.sh` one-liner as macOS | [linux.md](linux.md) |
| Any of them, from the code | `sh scripts/install.sh --from-source` (Windows: see the guide) | [source.md](source.md) |

Update through the same route you installed with. Use `brew upgrade --cask minutae` or `scoop update minutae` for package manager installs. Settings → General → **Check for updates** checks for a new release. The platform guides describe which release copies can also install updates from Settings.

**AI agents** installing Minutae for someone: follow [../agents/INSTALL.md](../agents/INSTALL.md).

Your recordings and settings are never touched by installs, updates or uninstalls:
macOS `~/Library/Minutae/`, Windows `%APPDATA%\Minutae\`, Linux `~/.local/share/minutae/`.
