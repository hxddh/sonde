#!/usr/bin/env bash
# A compressed .dmg holding the app and a link to /Applications.
set -euo pipefail
app="$1"
dmg="$2"
stage="$(mktemp -d)"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
# hdiutil's own size estimate is sometimes too small ("No space left on
# device"): give the image its size, with room to spare, and retry
size_mb=$(( $(du -sm "$stage" | cut -f1) * 12 / 10 + 20 ))
for attempt in 1 2 3; do
  if hdiutil create -volname "sonde" -srcfolder "$stage" -fs HFS+ -size "${size_mb}m" -ov -format UDZO "$dmg"; then
    break
  fi
  [ "$attempt" = 3 ] && exit 1
  echo "hdiutil failed (attempt $attempt), retrying"; sleep 5
done
rm -rf "$stage"
if [ -n "${MACOS_SIGNING_IDENTITY:-}" ] && [ -n "${MACOS_CERTIFICATE_P12:-}" ]; then
  codesign --force --timestamp --sign "$MACOS_SIGNING_IDENTITY" "$dmg"
fi
