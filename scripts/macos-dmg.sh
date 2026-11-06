#!/usr/bin/env bash
# A compressed .dmg holding the app and a link to /Applications.
set -euo pipefail
app="$1"
dmg="$2"
stage="$(mktemp -d)"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -volname "sonde" -srcfolder "$stage" -ov -format UDZO "$dmg"
rm -rf "$stage"
if [ -n "${MACOS_SIGNING_IDENTITY:-}" ] && [ -n "${MACOS_CERTIFICATE_P12:-}" ]; then
  codesign --force --timestamp --sign "$MACOS_SIGNING_IDENTITY" "$dmg"
fi
