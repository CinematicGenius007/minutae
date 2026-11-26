# Install on macOS

Needs macOS 13 or later on Apple silicon or Intel. Minutae uses a Core Audio process tap for meeting audio on macOS 14.4 and later and ScreenCaptureKit audio on macOS 13.0–14.3.

Release installs become available once the first release is published. For now, see [Build from source](source.md).

## 1. Install with the shell script

```sh
curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/install.sh | sh
```

The script downloads the right DMG for your chip and verifies it against `SHA256SUMS.txt`. If you have a signed-in GitHub CLI, it also verifies the build attestation. It installs to `/Applications` or `~/Applications` if the former isn't writable. The script clears quarantine on `Minutae.app` so it opens without a Gatekeeper prompt.

To update, quit Minutae and rerun the command. If the release includes updater signatures, you can also use **Settings → General → Check for updates** and choose **Install and restart**. The script accepts `--version 0.5.0`, `--dir <folder>` and `--dry-run` after `sh -s --`.

## 2. Install with Homebrew

This route becomes available when the maintainers publish the cask to their tap. With Homebrew 7.0.16 or later, run:

```sh
brew tap CinematicGenius007/tap
brew trust --cask CinematicGenius007/tap/minutae
brew install --cask CinematicGenius007/tap/minutae
```

Homebrew quarantines the new app. Try to open Minutae once, then go to **System Settings → Privacy & Security → Open Anyway** and confirm. Update and uninstall through Homebrew:

```sh
brew upgrade --cask minutae
brew uninstall --cask minutae
```

Homebrew carries the approval forward when the new bundle satisfies the old bundle's signing requirement. This works across releases signed with the project's same certificate. Use `brew upgrade` even if Minutae's settings offer the in-app updater. `brew uninstall --cask --zap minutae` removes app caches and saved window state and preserves `~/Library/Minutae/`.

## 3. Download the DMG manually

Download the matching DMG and `SHA256SUMS.txt` from [Releases](https://github.com/CinematicGenius007/minutae/releases). Compare `shasum -a 256 <downloaded-dmg>` with its checksum. Open the DMG, drag `Minutae.app` to Applications, and try opening the app. After the blocked launch, choose **System Settings → Privacy & Security → Open Anyway** and confirm.

To update, use **Install and restart** if Settings offers it, or quit Minutae, verify the next DMG and replace the app. A manual replacement may require **Open Anyway** again.

## Permissions and data

On use, macOS can ask for microphone access for meetings and dictation, system audio access for the other people on a call, and Accessibility to paste dictated text. On macOS 13.0–14.3, system audio capture uses the Screen Recording permission. Without Accessibility, dictation goes to the clipboard.

Releases signed with the same project certificate retain these permissions across updates. When a build is signed ad hoc, macOS can ask again after an update.

For shell-script or DMG installs, quit Minutae and delete `Minutae.app` to uninstall. Your recordings and other data stay in `~/Library/Minutae/` by default.
