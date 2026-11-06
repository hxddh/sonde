#!/usr/bin/env bash
# Signs a .app with a Developer ID (hardened runtime, timestamp).
# Without MACOS_CERTIFICATE_P12 the app keeps the ad hoc signature
# `nsh package` gave it, and the step says so: the flow stays complete.
#
#   MACOS_CERTIFICATE_P12 (base64 .p12), MACOS_CERTIFICATE_PASSWORD,
#   MACOS_SIGNING_IDENTITY ("Developer ID Application: Name (TEAMID)")
set -euo pipefail
app="$1"
if [ -z "${MACOS_CERTIFICATE_P12:-}" ]; then
  echo "::notice::no Developer ID certificate: $app stays ad hoc signed"
  codesign --verify --verbose "$app"
  exit 0
fi
keychain="$RUNNER_TEMP/sonde-signing.keychain-db"
password="$(uuidgen)"
security create-keychain -p "$password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
echo "$MACOS_CERTIFICATE_P12" | base64 --decode > "$RUNNER_TEMP/cert.p12"
security import "$RUNNER_TEMP/cert.p12" -k "$keychain" -P "$MACOS_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s -k "$password" "$keychain"
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
rm -f "$RUNNER_TEMP/cert.p12"
codesign --force --deep --options runtime --timestamp --sign "$MACOS_SIGNING_IDENTITY" "$app"
codesign --verify --strict --verbose=2 "$app"
spctl --assess --type execute --verbose "$app" || echo "::notice::spctl rejects it until it is notarized"
