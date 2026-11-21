#!/bin/sh
# Is there a newer Minutae? Prints the installed and latest versions and the command that
# updates this copy. Read-only: it never downloads or installs anything.
#
#   curl -fsSL https://raw.githubusercontent.com/CinematicGenius007/minutae/main/scripts/check-update.sh | sh
#   sh scripts/check-update.sh [--current 0.4.0] [--json]
#
# Exit status: 0 up to date, 10 update available, 1 couldn't tell (not installed, offline…).
# The installed version is read from /Applications or ~/Applications/Minutae.app (macOS),
# the menu entry install.sh writes (Linux), or the checkout this script sits in.
set -eu

REPO="CinematicGenius007/minutae"
CURRENT=""
CHANNEL=""
JSON=0
while [ $# -gt 0 ]; do
  case "$1" in
    --current) [ $# -ge 2 ] || { echo "--current needs a value" >&2; exit 1; }; CURRENT="${2#v}"; CHANNEL=given; shift 2 ;;
    --json) JSON=1; shift ;;
    -h|--help) sed -n '2,11p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 1 ;;
  esac
done

if [ -z "$CURRENT" ]; then
  for app in /Applications/Minutae.app "$HOME/Applications/Minutae.app"; do
    if [ -f "$app/Contents/Info.plist" ]; then
      CURRENT="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist" 2>/dev/null || true)"
      CHANNEL=direct
      [ -n "$CURRENT" ] && break
    fi
  done
fi
if [ -z "$CURRENT" ]; then
  f="${XDG_DATA_HOME:-$HOME/.local/share}/applications/minutae.desktop"
  if [ -f "$f" ]; then
    CURRENT="$(sed -n 's/^X-Minutae-Version=//p' "$f" | head -n 1)"
    CHANNEL="$(sed -n 's/^X-Minutae-Channel=//p' "$f" | head -n 1)"
    CHANNEL="${CHANNEL:-direct}"
  fi
fi
SRC=""
HERE="$(cd "$(dirname "$0")" 2>/dev/null && pwd)" || HERE=""
if [ -n "$HERE" ] && [ -f "$HERE/../Cargo.toml" ] && [ -f "$HERE/../apps/desktop/src-tauri/tauri.conf.json" ]; then
  SRC="$(cd "$HERE/.." && pwd)"
  if [ -z "$CURRENT" ]; then
    CURRENT="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' "$SRC/Cargo.toml" | head -n 1)"
    CHANNEL=source
  fi
fi

fail() {
  if [ "$JSON" = 1 ]; then printf '{"status":"unknown","error":"%s"}\n' "$1"; else echo "$1" >&2; fi
  exit 1
}
[ -n "$CURRENT" ] || fail "Minutae doesn't look installed here. Pass --current <version> to compare anyway."

TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT INT TERM
API="https://api.github.com/repos/$REPO/releases/latest"
if command -v curl >/dev/null 2>&1; then
  CODE="$(curl -sSL -H 'Accept: application/vnd.github+json' -o "$TMP" -w '%{http_code}' "$API" 2>/dev/null || echo 000)"
else
  if wget -q -O "$TMP" "$API"; then CODE=200; else CODE=000; fi
fi
case "$CODE" in
  200) ;;
  404) fail "No published release yet." ;;
  403|429) fail "GitHub is limiting requests. Try again in an hour." ;;
  *) fail "Couldn't reach GitHub." ;;
esac
LATEST="$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\{0,1\}\([^"]*\)".*/\1/p' "$TMP" | head -n 1)"
[ -n "$LATEST" ] || fail "GitHub didn't list a published release."

# Compare x.y.z numerically; a pre-release suffix (-beta.1) sorts before the plain version.
newer() { # a b → true when b > a
  a="$1" b="$2"
  [ "$a" = "$b" ] && return 1
  an="${a%%-*}" bn="${b%%-*}"
  if [ "$an" = "$bn" ]; then
    # same numbers: plain beats pre-release
    [ "$a" != "$an" ] && [ "$b" = "$bn" ] && return 0
    return 1
  fi
  top="$(printf '%s\n%s\n' "$an" "$bn" | sort -t. -k1,1n -k2,2n -k3,3n | tail -n 1)"
  [ "$top" = "$bn" ]
}

case "$CHANNEL" in
  source) CMD="git -C '${SRC:-<your checkout>}' pull && sh '${SRC:-<your checkout>}/scripts/install.sh' --from-source" ;;
  *) CMD="curl -fsSL https://raw.githubusercontent.com/$REPO/main/scripts/install.sh | sh" ;;
esac

if newer "$CURRENT" "$LATEST"; then
  if [ "$JSON" = 1 ]; then
    printf '{"status":"available","current":"%s","latest":"%s","channel":"%s","command":"%s"}\n' \
      "$CURRENT" "$LATEST" "$CHANNEL" "$(printf '%s' "$CMD" | sed 's/"/\\"/g')"
  else
    echo "Minutae $LATEST is available (you have $CURRENT)."
    echo "Update with: $CMD"
    echo "Notes: https://github.com/$REPO/releases/tag/v$LATEST"
  fi
  exit 10
fi
if [ "$JSON" = 1 ]; then
  printf '{"status":"upToDate","current":"%s","latest":"%s","channel":"%s"}\n' "$CURRENT" "$LATEST" "$CHANNEL"
else
  echo "Minutae $CURRENT is up to date (latest: $LATEST)."
fi
