# Install on macOS

Needs macOS 13 or later. Other people's audio is captured with a Core Audio process tap on macOS 14.4+, and with screen-capture audio (ScreenCaptureKit, needs Screen Recording) on 13.0–14.3.

## Install or update

```sh
curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/install.sh | sh
```

The script:

1. Asks GitHub for the latest release and downloads the `.dmg` for your chip (`aarch64` or `x64`).
2. Checks it against the release's `SHA256SUMS.txt`. A mismatch stops the install. This catches a broken download, not a tampered release: the checksums come from the same page.
3. If the GitHub CLI (`gh`) is installed and signed in, also runs `gh attestation verify`, which proves GitHub Actions built the file from this repository. A failure stops the install.
4. Copies `Minutae.app` to `/Applications`, or `~/Applications` when `/Applications` isn't writable. It never uses `sudo`.
5. Clears the download quarantine flag on the app. The new copy goes next to the old one first and replaces it only once it's complete, so a failed update leaves the old version in place.

Options: `--version 0.5.0` installs a specific release, `--dir <folder>` picks the folder, `--dry-run` only prints what would happen. Quit Minutae before updating.

### Why the quarantine step

Minutae isn't notarized by Apple (that needs a paid developer account). macOS blocks un-notarized apps that a browser downloaded. The script downloads the app itself and clears the flag only on `Minutae.app`; it doesn't change any system security setting.

If you'd rather download the `.dmg` by hand from [Releases](https://github.com/CinematicGenius007/minutae/releases): drag Minutae to Applications, open it once, then go to **System Settings → Privacy & Security** and click **Open Anyway**.

## Permissions

On first use Minutae asks for:

- **Microphone**: your side of a meeting, and dictation.
- **System audio** (Screen & System Audio Recording): the other people on the call.
- **Accessibility**: typing dictated text into other apps. Without it, dictation goes to the clipboard.

When a release is signed with the project's certificate (set up once by the maintainer, see `docs/RELEASE.md`), macOS keeps these permissions across updates. A release built without it is signed ad hoc, and macOS asks again after that update. Builds from source are always ad hoc.

## Check for updates

Settings → General → **Check for updates**, or:

```sh
curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/check-update.sh | sh
```

It prints the installed and latest versions and the command to update. It installs nothing.

## Uninstall

Quit Minutae, then delete `Minutae.app` from Applications. To remove your data too, delete `~/Library/Minutae`.
