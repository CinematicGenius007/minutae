# Install on Linux

x86_64. Tested on Ubuntu 22.04+ and Fedora; any distro with WebKitGTK 4.1 should work. Needs PulseAudio or PipeWire (with its Pulse layer) for meeting audio.

## Install or update

```sh
curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/install.sh | sh
```

The script downloads the AppImage for the latest release, checks it against `SHA256SUMS.txt` (and, with a signed-in GitHub CLI, its build provenance via `gh attestation verify`), installs it as `~/.local/bin/minutae`, and adds Minutae to your app menu. It never uses `sudo`.

The AppImage needs FUSE 2: `sudo apt install libfuse2` (Ubuntu 22.04: `libfuse2`, 24.04: `libfuse2t64`) or `sudo dnf install fuse fuse-libs`.

Options: `--version 0.5.0`, `--dir <folder>`, `--dry-run`. Quit Minutae before updating.

### Packages instead

Each release also has a `.deb` (Debian/Ubuntu) and `.rpm` (Fedora): `sudo apt install ./Minutae_<v>_amd64.deb` or `sudo dnf install ./Minutae-<v>-1.x86_64.rpm`. They don't update themselves; install the next one the same way.

## Check for updates

Settings → General → **Check for updates**, or:

```sh
curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/check-update.sh | sh
```

## Global shortcuts on Wayland

On Wayland, shortcuts go through the desktop's GlobalShortcuts portal; GNOME and KDE ask once to allow them.

## Uninstall

Delete `~/.local/bin/minutae`, `~/.local/share/applications/minutae.desktop` and `~/.local/share/icons/hicolor/128x128/apps/minutae.png`. Your data is in `~/.local/share/minutae`.
