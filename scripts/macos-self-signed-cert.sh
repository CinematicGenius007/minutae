#!/bin/sh
# Makes the free, self-signed macOS code-signing certificate for Minutae releases.
#
# Why: without a paid Apple Developer ID, release builds used to be ad-hoc signed. An ad-hoc
# signature has no stable identity, so macOS forgets the Microphone, Screen & System Audio
# and Accessibility permissions after every update. Signing every release with the SAME
# self-signed certificate gives each build the same identity, so permissions survive
# updates. It does NOT satisfy Gatekeeper (that needs Apple notarization); install.sh
# clears the download quarantine instead.
#
# Run once, on any Mac, then add the printed values as GitHub Actions secrets:
#   APPLE_CERTIFICATE           base64 of the .p12 (copied to the clipboard)
#   APPLE_CERTIFICATE_PASSWORD  the password printed below
#   APPLE_SIGNING_IDENTITY      Minutae Self-Signed
# Keep the .p12 somewhere safe (password manager). Losing it means the next release has a
# new identity and users grant permissions once more. Valid for 10 years.
#
#   sh scripts/macos-self-signed-cert.sh [output-dir]
set -eu

NAME="Minutae Self-Signed"
OUT="${1:-$HOME/minutae-signing}"
mkdir -p "$OUT"
chmod 700 "$OUT"
KEY="$OUT/minutae-self-signed.key"
CRT="$OUT/minutae-self-signed.crt"
P12="$OUT/minutae-self-signed.p12"
if [ -e "$P12" ]; then
  echo "$P12 already exists. Reuse it; a new certificate resets users' permissions." >&2
  exit 1
fi

PASS="$(openssl rand -base64 24 | tr -d '/+=' | cut -c1-24)"
CONF="$(mktemp)"
trap 'rm -f "$CONF"' EXIT
cat > "$CONF" <<CNF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
subjectKeyIdentifier = hash
CNF

openssl req -x509 -newkey rsa:2048 -sha256 -days 3650 -nodes \
  -keyout "$KEY" -out "$CRT" -config "$CONF" 2>/dev/null
# -legacy: macOS `security import` can't read OpenSSL 3's default p12 encryption.
LEGACY=""
openssl pkcs12 -help 2>&1 | grep -q -- '-legacy' && LEGACY="-legacy"
openssl pkcs12 -export $LEGACY -inkey "$KEY" -in "$CRT" -name "$NAME" \
  -out "$P12" -passout "pass:$PASS"
chmod 600 "$KEY" "$P12"

if command -v pbcopy >/dev/null 2>&1; then
  base64 < "$P12" | tr -d '\n' | pbcopy
  COPIED="(copied to the clipboard)"
else
  COPIED="(run: base64 < \"$P12\" | tr -d '\\n')"
fi
cat <<MSG
Created $P12

Add these repository secrets (GitHub → Settings → Secrets and variables → Actions):
  APPLE_CERTIFICATE           base64 of the .p12 $COPIED
  APPLE_CERTIFICATE_PASSWORD  $PASS
  APPLE_SIGNING_IDENTITY      $NAME

Then store $OUT in your password manager and delete it from disk.
MSG
