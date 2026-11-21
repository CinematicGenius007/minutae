#!/bin/sh
# Install or update Minutae on macOS or Linux, for free and without Gatekeeper prompts.
#
#   curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/install.sh | sh
#   sh scripts/install.sh [--version 0.5.0] [--dir <path>] [--from-source] [--dry-run]
#
# Release install (default):
#   macOS  downloads Minutae_<v>_<aarch64|x64>.dmg, checks it against SHA256SUMS.txt, copies
#          Minutae.app to /Applications (or ~/Applications when /Applications isn't writable)
#          and clears the download quarantine flag. Minutae isn't notarized by Apple (that
#          costs money), so without this step macOS would block it.
#   Linux  downloads Minutae_<v>_amd64.AppImage, checks it, installs it as
#          ~/.local/bin/minutae and adds a menu entry. Needs FUSE 2 (libfuse2 / fuse2).
# --from-source: builds the checkout this script sits in (or clones one) with Rust, Bun and
#          cmake, then installs the result the same way. Nothing is downloaded from releases.
#
# Running it again updates Minutae. Your recordings and settings are never touched; they live
# in ~/Library/Minutae (macOS) or ~/.local/share/minutae (Linux).
#
# It never uses sudo. Quit Minutae before updating.
set -eu

# Everything runs inside main(), called on the last line, so a download cut short by
# `curl | sh` runs nothing at all.
main() {

REPO="CinematicGenius007/minutae"
VERSION=""
DEST=""
FROM_SOURCE=0
DRY=0

say() { printf '%s\n' "$*"; }
die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }
run() { if [ "$DRY" = 1 ]; then say "would run: $*"; else "$@"; fi; }
have() { command -v "$1" >/dev/null 2>&1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || die "--version needs a value"; VERSION="${2#v}"; shift 2 ;;
    --dir) [ $# -ge 2 ] || die "--dir needs a value"; DEST="$2"; shift 2 ;;
    --from-source) FROM_SOURCE=1; shift ;;
    --dry-run) DRY=1; shift ;;
    -h|--help) sed -n '2,22p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
done

OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS" in
  Darwin) OS=macos ;;
  Linux) OS=linux ;;
  *) die "this script is for macOS and Linux. On Windows use Scoop: scoop install https://github.com/$REPO/releases/latest/download/minutae.json" ;;
esac

if [ "$OS" = macos ]; then
  APP_NAME="Minutae.app"
  if [ -z "$DEST" ]; then
    if [ -w /Applications ]; then DEST=/Applications; else DEST="$HOME/Applications"; fi
  fi
  TARGET="$DEST/$APP_NAME"
else
  DEST="${DEST:-$HOME/.local/bin}"
  TARGET="$DEST/minutae"
fi

running() {
  if [ "$OS" = macos ]; then pgrep -f "$APP_NAME/Contents/MacOS/" >/dev/null 2>&1
  else pgrep -f "$TARGET" >/dev/null 2>&1 || pgrep -x minutae-desktop >/dev/null 2>&1; fi
}
if running && [ "$DRY" = 0 ]; then
  die "Minutae is running. Quit it (menu bar or tray → Quit) and run this again."
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM

fetch() { # url out
  if have curl; then curl -fsSL --retry 3 -o "$2" "$1"
  elif have wget; then wget -q -O "$2" "$1"
  else die "needs curl or wget"; fi
}

sha256() {
  if have shasum; then shasum -a 256 "$1" | cut -d' ' -f1
  else sha256sum "$1" | cut -d' ' -f1; fi
}

latest_version() {
  fetch "https://api.github.com/repos/$REPO/releases/latest" "$TMP/latest.json" \
    || die "couldn't find a published release on GitHub (offline, or none published yet). Try --from-source"
  sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\{0,1\}\([^"]*\)".*/\1/p' "$TMP/latest.json" | head -n 1
}

# The installed Linux copy records its version in the menu entry.
DESKTOP_FILE="${XDG_DATA_HOME:-$HOME/.local/share}/applications/minutae.desktop"

install_linux_binary() { # src-file version
  run mkdir -p "$DEST" "$(dirname "$DESKTOP_FILE")"
  # Same-folder copy + rename: atomic, and the old binary stays until the new one is complete.
  run install -m 0755 "$1" "$TARGET.new"
  run mv -f "$TARGET.new" "$TARGET"
  ICON_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/128x128/apps"
  run mkdir -p "$ICON_DIR"
  if [ "$DRY" = 0 ]; then
    fetch "https://raw.githubusercontent.com/$REPO/${3:-main}/apps/desktop/src-tauri/icons/128x128.png" "$ICON_DIR/minutae.png" \
      || say "(couldn't fetch the icon; the menu entry will use a generic one)"
    cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=Minutae
Comment=Local-first meeting notes and dictation
Exec="$TARGET"
Icon=minutae
Categories=Office;AudioVideo;
Terminal=false
X-Minutae-Version=$2
X-Minutae-Channel=${4:-direct}
EOF
  fi
  case ":$PATH:" in *":$DEST:"*) ;; *) say "Note: $DEST isn't on your PATH. Start Minutae from the app menu, or add it." ;; esac
}

install_macos_app() { # src .app
  # Copy next to the old app first and swap at the end, so a failed copy leaves the
  # installed version untouched.
  NEW="$DEST/.Minutae.app.new"
  OLD="$DEST/.Minutae.app.old"
  run mkdir -p "$DEST"
  run rm -rf "$NEW" "$OLD"
  run ditto "$1" "$NEW"
  # Downloads carry com.apple.quarantine; without notarization Gatekeeper would block the app.
  run xattr -dr com.apple.quarantine "$NEW" 2>/dev/null || true
  if [ -e "$TARGET" ]; then run mv "$TARGET" "$OLD"; fi
  if ! run mv "$NEW" "$TARGET"; then
    [ -e "$OLD" ] && mv "$OLD" "$TARGET"
    die "couldn't move the new Minutae.app into $DEST; the old version is still installed"
  fi
  run rm -rf "$OLD"
}

# ── From source ──────────────────────────────────────────────────────────────
if [ "$FROM_SOURCE" = 1 ]; then
  SRC=""
  HERE="$(cd "$(dirname "$0")" 2>/dev/null && pwd)" || HERE=""
  if [ -n "$HERE" ] && [ -f "$HERE/../apps/desktop/src-tauri/tauri.conf.json" ]; then
    SRC="$(cd "$HERE/.." && pwd)"
  else
    SRC="${MINUTAE_SRC:-$HOME/.local/src/minutae}"
    have git || die "needs git to fetch the source"
    if [ -d "$SRC/.git" ]; then run git -C "$SRC" pull --ff-only
    else run git clone --depth 1 "https://github.com/$REPO.git" "$SRC"; fi
  fi
  missing=""
  for tool in cargo bun cmake; do have "$tool" || missing="$missing $tool"; done
  if [ -n "$missing" ]; then
    say "Missing:$missing"
    say "  Rust:  https://rustup.rs"
    say "  Bun:   https://bun.sh"
    if [ "$OS" = macos ]; then say "  cmake: brew install cmake (and Xcode Command Line Tools: xcode-select --install)"
    else say "  Linux packages: see docs/install/source.md"; fi
    die "install the tools above, then run this again"
  fi
  say "Building Minutae from $SRC (first build takes 10–20 minutes)…"
  run sh -c "cd '$SRC' && bun install --frozen-lockfile"
  V="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' "$SRC/Cargo.toml" | head -n 1)"
  if [ "$OS" = macos ]; then
    run sh -c "cd '$SRC/apps/desktop' && bun run tauri build --bundles app"
    APP="$(ls -d "$SRC"/target/release/bundle/macos/Minutae.app 2>/dev/null || true)"
    [ -n "$APP" ] || [ "$DRY" = 1 ] || die "build finished but Minutae.app wasn't found under target/release/bundle/macos"
    install_macos_app "${APP:-$SRC/target/release/bundle/macos/Minutae.app}"
    say "Installed Minutae $V (built from source) to $TARGET."
    say "Source builds are signed ad hoc, so macOS asks for permissions again after each rebuild."
  else
    run sh -c "cd '$SRC/apps/desktop' && bun run tauri build --no-bundle"
    install_linux_binary "$SRC/target/release/minutae-desktop" "$V" main source
    say "Installed Minutae $V (built from source) to $TARGET."
  fi
  say "Update later with: git -C '$SRC' pull && sh '$SRC/scripts/install.sh' --from-source"
  exit 0
fi

# ── Release ──────────────────────────────────────────────────────────────────
[ -n "$VERSION" ] || VERSION="$(latest_version)"
[ -n "$VERSION" ] || die "couldn't read the latest version from GitHub"
TAG="v$VERSION"
# MINUTAE_RELEASE_BASE: test hook (e.g. file:///tmp/fake-release).
BASE="${MINUTAE_RELEASE_BASE:-https://github.com/$REPO/releases/download/$TAG}"

if [ "$OS" = macos ]; then
  case "$ARCH" in
    arm64) FILE="Minutae_${VERSION}_aarch64.dmg" ;;
    x86_64) FILE="Minutae_${VERSION}_x64.dmg" ;;
    *) die "unsupported Mac ($ARCH)" ;;
  esac
else
  case "$ARCH" in
    x86_64|amd64) FILE="Minutae_${VERSION}_amd64.AppImage" ;;
    *) die "no Linux release for $ARCH yet. Build it instead: sh install.sh --from-source" ;;
  esac
fi

say "Installing Minutae $VERSION ($FILE)…"
if [ "$DRY" = 1 ]; then
  say "would download $BASE/$FILE and $BASE/SHA256SUMS.txt, verify, and install to $TARGET"
  exit 0
fi
fetch "$BASE/$FILE" "$TMP/$FILE" || die "couldn't download $BASE/$FILE"
fetch "$BASE/SHA256SUMS.txt" "$TMP/SHA256SUMS.txt" || die "couldn't download SHA256SUMS.txt for $TAG"
WANT="$(awk -v f="$FILE" '$2 == f || $2 == "*"f { print $1 }' "$TMP/SHA256SUMS.txt" | head -n 1)"
[ -n "$WANT" ] || die "$FILE isn't listed in SHA256SUMS.txt"
GOT="$(sha256 "$TMP/$FILE")"
[ "$WANT" = "$GOT" ] || die "checksum mismatch for $FILE (expected $WANT, got $GOT). Nothing was installed."
# The checksum only proves the download isn't corrupt (it comes from the same release page).
# Build provenance proves GitHub Actions built the file from this repo; it needs a signed-in
# GitHub CLI, so it's checked whenever one is available.
if [ -z "${MINUTAE_RELEASE_BASE:-}" ] && have gh && gh auth token >/dev/null 2>&1; then
  gh attestation verify "$TMP/$FILE" --repo "$REPO" >/dev/null 2>&1 \
    || die "$FILE has no valid build provenance for $REPO (gh attestation verify failed). Nothing was installed."
  say "Verified build provenance (built by GitHub Actions from $REPO)."
else
  say "Checksum OK. (Sign in to the GitHub CLI, gh, to also verify where the file was built.)"
fi

if [ "$OS" = macos ]; then
  MNT="$TMP/mnt"
  mkdir -p "$MNT"
  hdiutil attach -nobrowse -readonly -quiet -mountpoint "$MNT" "$TMP/$FILE" || die "couldn't open $FILE"
  if [ ! -d "$MNT/$APP_NAME" ]; then hdiutil detach -quiet "$MNT" || true; die "$APP_NAME isn't in $FILE"; fi
  install_macos_app "$MNT/$APP_NAME"
  hdiutil detach -quiet "$MNT" || hdiutil detach -force -quiet "$MNT" || true
else
  install_linux_binary "$TMP/$FILE" "$VERSION" "$TAG" direct
fi

say "Installed Minutae $VERSION to $TARGET."
say "Check for updates any time: Settings → General → Updates, or"
say "  curl -fsSL https://raw.githubusercontent.com/$REPO/main/scripts/check-update.sh | sh"
}

main "$@"
