#!/bin/sh
# Owner setup for zero-cost macOS signing and the Tauri updater.
# Run from a Mac with GitHub CLI authentication and the workspace dependencies in place.
#   sh scripts/release-setup.sh [--dry-run] [signing-directory]
set +x
set -eu
umask 077

repo=CinematicGenius007/minutae
dry_run=0
if [ "${1:-}" = --dry-run ]; then dry_run=1; shift; fi
case "${1:-}" in
  -h|--help) sed -n '2,4s/^# //p' "$0"; exit 0 ;;
  -*) echo 'Usage: sh scripts/release-setup.sh [--dry-run] [signing-directory]' >&2; exit 1 ;;
esac
[ "$#" -le 1 ] || { echo 'Specify one signing directory.' >&2; exit 1; }
root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
out=${1:-$HOME/minutae-signing}
case "$out" in /*) ;; *) out="$PWD/$out" ;; esac
cert="$out/minutae-self-signed.p12"
cert_password="$out/minutae-self-signed.password"
updater="$out/minutae.key"

if [ "$dry_run" = 1 ]; then
  cat <<EOF
Would reuse existing signing material or ask to generate it locally in $out.
Would ask before setting each GitHub Actions value on $repo with gh:
  Secrets: APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD, APPLE_SIGNING_IDENTITY,
           TAURI_SIGNING_PRIVATE_KEY, TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  Variable: TAURI_UPDATER_PUBKEY
EOF
  exit 0
fi

[ "$(uname -s)" = Darwin ] || { echo 'Run this setup on macOS.' >&2; exit 1; }
[ -t 0 ] || { echo 'Run this setup in an interactive terminal.' >&2; exit 1; }
for tool in gh bun openssl; do
  command -v "$tool" >/dev/null || { echo "Install $tool first." >&2; exit 1; }
done

confirm() {
  printf '%s [y/N] ' "$1" >&2
  IFS= read -r reply || return 1
  case "$reply" in y|Y|yes|YES) return 0 ;; *) return 1 ;; esac
}

if [ ! -f "$cert" ] && confirm "Generate the self-signed certificate in $out?"; then
  sh "$root/scripts/macos-self-signed-cert.sh" "$out" --password-file
fi
if [ ! -f "$updater" ] && confirm "Generate the Tauri updater key in $out?"; then
  mkdir -p "$out"
  chmod 700 "$out"
  echo 'Choose a password when prompted. You will need it when setting the updater password secret.'
  (unset CI; bun run --cwd "$root/apps/desktop" tauri signer generate -w "$updater")
fi

if [ -f "$cert" ]; then
  if [ ! -s "$cert_password" ]; then
    echo "Missing $cert_password. Restore the existing certificate's password there with mode 600, then rerun." >&2
    exit 1
  fi
  if confirm "Set APPLE_CERTIFICATE on $repo?"; then
    encoded=$(base64 < "$cert")
    printf '%s' "$encoded" | tr -d '\n\r' | gh secret set APPLE_CERTIFICATE --repo "$repo"
    unset encoded
  fi
  if confirm "Set APPLE_CERTIFICATE_PASSWORD on $repo?"; then
    gh secret set APPLE_CERTIFICATE_PASSWORD --repo "$repo" < "$cert_password"
  fi
  if confirm "Set APPLE_SIGNING_IDENTITY on $repo?"; then
    printf '%s' 'Minutae Self-Signed' | gh secret set APPLE_SIGNING_IDENTITY --repo "$repo"
  fi
fi

if [ -f "$updater" ]; then
  if [ ! -s "$updater.pub" ]; then
    echo "Missing $updater.pub. Restore the matching public key, then rerun." >&2
    exit 1
  fi
  if confirm "Set TAURI_SIGNING_PRIVATE_KEY_PASSWORD on $repo?"; then
    tty_settings=$(stty -g)
    trap 'stty "$tty_settings"' EXIT
    trap 'exit 1' HUP INT TERM
    printf 'Updater key password: ' >&2
    stty -echo
    IFS= read -r updater_password
    stty "$tty_settings"
    trap - EXIT HUP INT TERM
    printf '\n' >&2
    [ -n "$updater_password" ] || { echo 'Enter the password used to generate the updater key.' >&2; exit 1; }
    printf '%s' "$updater_password" | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo "$repo"
    unset updater_password
  fi
  if confirm "Set TAURI_SIGNING_PRIVATE_KEY on $repo?"; then
    gh secret set TAURI_SIGNING_PRIVATE_KEY --repo "$repo" < "$updater"
  fi
  if confirm "Set TAURI_UPDATER_PUBKEY on $repo?"; then
    gh variable set TAURI_UPDATER_PUBKEY --repo "$repo" < "$updater.pub"
  fi
fi

echo "Back up any signing material in $out and its passwords in your password manager. Reuse them for every release."
