#!/usr/bin/env bash
# Signs a .app with a Developer ID (hardened runtime, timestamp), inside
# out: each nested piece of code first, the deepest first, then the app
# (codesign --deep is not how Apple asks for it). Without
# MACOS_CERTIFICATE_P12 the app keeps the ad hoc signature `nsh package`
# gave it, and the step says so: the flow stays complete. In a release
# (SONDE_RELEASE=true) that is a warning on the run, which a person sees on
# the release's page; elsewhere a notice.
#
#   MACOS_CERTIFICATE_P12 (base64 .p12), MACOS_CERTIFICATE_PASSWORD,
#   MACOS_SIGNING_IDENTITY ("Developer ID Application: Name (TEAMID)")
set -euo pipefail
app="$1"
if [ -z "${MACOS_CERTIFICATE_P12:-}" ]; then
  level=notice; [ "${SONDE_RELEASE:-}" = true ] && level=warning
  echo "::$level title=Not signed::no Developer ID certificate (MACOS_CERTIFICATE_P12): $app stays ad hoc signed, and Gatekeeper refuses it on another Mac"
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
# shellcheck disable=SC2046 # the keychains, one word each
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
rm -f "$RUNNER_TEMP/cert.p12"

sign() {
  codesign --force --options runtime --timestamp --sign "$MACOS_SIGNING_IDENTITY" "$@"
}
main="$app/Contents/MacOS/$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")"
# nested code, the deepest path first: Mach-O files (libraries, helpers),
# then the bundles that hold them (frameworks, helper apps, extensions)
while IFS= read -r f; do
  [ "$f" = "$main" ] && continue
  if file -b "$f" | grep -q "Mach-O"; then sign "$f"; fi
done < <(find "$app/Contents" -type f -perm -u+x -o -type f -name "*.dylib" | awk '{ print length, $0 }' | sort -rn | cut -d' ' -f2-)
while IFS= read -r b; do
  sign "$b"
done < <(find "$app/Contents" -depth -type d \( -name "*.framework" -o -name "*.app" -o -name "*.xpc" -o -name "*.appex" \))
# then the app, with the entitlements nsh gave it
sign --preserve-metadata=entitlements "$app"
codesign --verify --strict --deep --verbose=2 "$app"
spctl --assess --type execute --verbose "$app" || echo "::notice::spctl rejects it until it is notarized"
