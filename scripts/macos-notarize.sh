#!/usr/bin/env bash
# Notarizes a signed .app or .dmg with notarytool and an App Store Connect
# API key, then staples the ticket to it (a .app is sent as a zip; the
# ticket goes on the .app itself). Skipped, and said (a notice, or in a
# release a warning), without the key or without a Developer ID signature
# to notarize.
#
#   APPLE_API_KEY (the .p8 key's text, or base64 of it), APPLE_API_KEY_ID,
#   APPLE_API_ISSUER (the issuer id)
set -euo pipefail
target="$1"
level=notice; [ "${SONDE_RELEASE:-}" = true ] && level=warning
signed=$(codesign -dv --verbose=2 "$target" 2>&1 || true)
if ! grep -q '^Authority=Developer ID Application' <<<"$signed"; then
  echo "::$level title=Not notarized::$target is not signed with a Developer ID, so it cannot be notarized"
  exit 0
fi
if [ -z "${APPLE_API_KEY:-}" ] || [ -z "${APPLE_API_KEY_ID:-}" ] || [ -z "${APPLE_API_ISSUER:-}" ]; then
  echo "::$level title=Not notarized::no App Store Connect API key (APPLE_API_KEY, APPLE_API_KEY_ID, APPLE_API_ISSUER): $target is not notarized"
  exit 0
fi
key="$RUNNER_TEMP/AuthKey_$APPLE_API_KEY_ID.p8"
case "$APPLE_API_KEY" in
  *"BEGIN PRIVATE KEY"*) printf '%s\n' "$APPLE_API_KEY" > "$key" ;;
  *) printf '%s' "$APPLE_API_KEY" | base64 --decode > "$key" ;;
esac
trap 'rm -f "$key"' EXIT
sent="$target"
if [ -d "$target" ]; then
  sent="$RUNNER_TEMP/$(basename "$target").zip"
  ditto -c -k --keepParent "$target" "$sent"
fi
xcrun notarytool submit "$sent" --key "$key" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER" --wait
xcrun stapler staple "$target"
xcrun stapler validate "$target"
