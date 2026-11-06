#!/usr/bin/env bash
# Writes the Homebrew formula (terminal binary) and cask (macOS app) for
# version $1 from the release's SHA256SUMS, and pushes them to the tap
# (hxddh/homebrew-sonde). Skipped without HOMEBREW_TAP_TOKEN.
set -euo pipefail
version="$1"
sums="$2"
if [ -z "${HOMEBREW_TAP_TOKEN:-}" ]; then
  echo "::notice::no HOMEBREW_TAP_TOKEN: the tap is not updated"
  exit 0
fi
sha() { grep " $1\$" "$sums" | cut -d' ' -f1; }
root="$(cd "$(dirname "$0")/.." && pwd)"
tap="$(mktemp -d)"
git clone "https://x-access-token:${HOMEBREW_TAP_TOKEN}@github.com/hxddh/homebrew-sonde.git" "$tap"
mkdir -p "$tap/Formula" "$tap/Casks"
sed -e "s/@VERSION@/$version/g" \
    -e "s/@SHA_MACOS_ARM@/$(sha sonde-macos-aarch64.tar.gz)/" \
    -e "s/@SHA_MACOS_X86@/$(sha sonde-macos-x86_64.tar.gz)/" \
    -e "s/@SHA_LINUX_ARM@/$(sha sonde-linux-aarch64.tar.gz)/" \
    -e "s/@SHA_LINUX_X86@/$(sha sonde-linux-x86_64.tar.gz)/" \
    "$root/packaging/homebrew/sonde.rb.in" > "$tap/Formula/sonde.rb"
sed -e "s/@VERSION@/$version/g" -e "s/@SHA_DMG@/$(sha sonde-macos.dmg)/" \
    "$root/packaging/homebrew/sonde-app.rb.in" > "$tap/Casks/sonde-app.rb"
cd "$tap"
git add Formula/sonde.rb Casks/sonde-app.rb
git -c user.name="sonde release" -c user.email="release@sonde.invalid" commit -m "sonde $version"
git push origin HEAD
