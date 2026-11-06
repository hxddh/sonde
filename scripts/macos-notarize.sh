#!/usr/bin/env bash
# Notarizes and staples a signed .dmg with notarytool. Skipped (with a
# notice) without APPLE_ID, APPLE_TEAM_ID and APPLE_APP_PASSWORD.
set -euo pipefail
dmg="$1"
if [ -z "${APPLE_ID:-}" ] || [ -z "${APPLE_TEAM_ID:-}" ] || [ -z "${APPLE_APP_PASSWORD:-}" ]; then
  echo "::notice::no notarization credentials: $dmg is not notarized"
  exit 0
fi
xcrun notarytool submit "$dmg" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" --wait
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
