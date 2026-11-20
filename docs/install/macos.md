# Install on macOS

Needs macOS 13 or later. System audio from other apps needs macOS 14.6+ (13–14.5 uses screen-capture audio instead).

## Install or update

```sh
curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/install.sh | sh
```

The script:

1. Asks GitHub for the latest release and downloads the `.dmg` for your chip (`aarch64` or `x64`).
2. Checks it against the release's `SHA256SUMS.txt`. A mismatch stops the install.
3. Copies `Minutae.app` to `/Applications`, or `~/Applications` when `/Applications` isn't writable. It never uses `sudo`.
4. Clears the download quarantine flag on the app.

Options: `--version 0.5.0` installs a specific release, `--dir <folder>` picks the folder, `--dry-run` only prints what would happen. Quit Minutae before updating.

### Why the quarantine step

Minutae isn't notarized by Apple (that needs a paid developer account). macOS blocks un-notarized apps that a browser downloaded. The script downloads the app itself and clears the flag only on `Minutae.app`; it doesn't change any system security setting.

If you'd rather download the `.dmg` by hand from [Releases](https://github.com/CinematicGenius007/minutae/releases): drag Minutae to Applications, open it once, then go to **System Settings → Privacy & Security** and click **Open Anyway**.

## Permissions

On first use Minutae asks for:

- **Microphone**: your side of a meeting, and dictation.
- **System audio** (Screen & System Audio Recording): the other people on the call.
- **Accessibility**: typing dictated text into other apps. Without it, dictation goes to the clipboard.

Release builds are always signed with the same certificate, so macOS keeps these permissions across updates.

## Check for updates

Settings → General → **Check for updates**, or:

```sh
curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/check-update.sh | sh
```

It prints the installed and latest versions and the command to update. It installs nothing.

## Uninstall

Quit Minutae, then delete `Minutae.app` from Applications. To remove your data too, delete `~/Library/Minutae`.
