#!/usr/bin/env bash
# Build spike-cli + wrap MinutaeSpike into an ad-hoc signed .app
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

swift build -c release --product spike-cli
BIN="$(swift build -c release --product spike-cli --show-bin-path)/spike-cli"

APP="$ROOT/AppBundle/MinutaeSpike.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/MinutaeSpike"
# Prefer GUI binary when it builds; fall back to CLI wrapper for packaging proof
if swift build -c release --product MinutaeSpike 2>/tmp/minutae-spike-gui-build.log; then
  GUI="$(swift build -c release --product MinutaeSpike --show-bin-path)/MinutaeSpike"
  cp "$GUI" "$APP/Contents/MacOS/MinutaeSpike"
  echo "packaged GUI binary"
else
  echo "GUI build failed; packaging spike-cli as placeholder — see /tmp/minutae-spike-gui-build.log"
fi

cp "$ROOT/Sources/MinutaeSpike/Info.plist" "$APP/Contents/Info.plist"
# Minimal PkgInfo
printf 'APPL????' > "$APP/Contents/PkgInfo"

codesign --force --deep --sign - "$APP"
codesign -dv --verbose=2 "$APP" 2>&1 | tee /tmp/minutae-spike-codesign.txt
echo "app: $APP"
echo "cdhash: $(codesign -dv "$APP" 2>&1 | awk -F= '/CDHash/{print $2; exit}')"
