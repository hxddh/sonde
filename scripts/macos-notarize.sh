#!/usr/bin/env bash
# Notarizes and staples a signed .dmg with notarytool. Skipped (with a
# notice, or in a release a warning) without APPLE_ID, APPLE_TEAM_ID and
# APPLE_APP_PASSWORD.
set -euo pipefail
dmg="$1"
if [ -z "${APPLE_ID:-}" ] || [ -z "${APPLE_TEAM_ID:-}" ] || [ -z "${APPLE_APP_PASSWORD:-}" ]; then
  level=notice; [ "${SONDE_RELEASE:-}" = true ] && level=warning
  echo "::$level title=Not notarized::no notarization credentials (APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD): $dmg is not notarized"
  exit 0
fi
xcrun notarytool submit "$dmg" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" --wait
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
