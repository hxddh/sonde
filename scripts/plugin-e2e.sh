#!/usr/bin/env sh
# The plugin path end to end: sign the example plugin, trust its root,
# install it, refuse a plugin that asks for the network, open the plugin in
# `sonde tui` on the demo's Delta table, revoke it.
#
#   scripts/plugin-e2e.sh [path to a tui build of sonde]
set -eu
cd "$(dirname "$0")/.."
sonde="${1:-target/debug/sonde}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
export SONDE_CONFIG_DIR="$tmp/config" SONDE_DATA_DIR="$tmp/data"
scripts/sign-plugin.sh plugins/delta-lens "$tmp/keys" "$tmp/delta.nshpkg"
if "$sonde" plugin install "$tmp/delta.nshpkg"; then echo "installed without a trusted root"; exit 1; fi
"$sonde" plugin trust example "$tmp/keys/root.pub"
"$sonde" plugin install "$tmp/delta.nshpkg"
# a plugin asking for the network is refused
cp -r plugins/delta-lens "$tmp/rogue" && rm -rf "$tmp/rogue/.nsh"
sed -i.bak 's/"dev.sonde.plugins.delta"/"dev.sonde.plugins.rogue"/; s/"host": \[/"network": { "hosts": ["evil.example"] }, "host": [/' "$tmp/rogue/nsh.json"
scripts/sign-plugin.sh "$tmp/rogue" "$tmp/keys" "$tmp/rogue.nshpkg"
if "$sonde" plugin install "$tmp/rogue.nshpkg"; then echo "a plugin with network access was installed"; exit 1; fi
# open it in the terminal UI on the demo's Delta table, then F12 back
screen="$(python3 scripts/pty-screen.py "$sonde" tui --demo -- '["\\t", "\\t", "\\t", "\\t", "\\t", "\\t", "\\t", "\\t", "\\t", "\\t", "\\r", "wait:1.5", "\\t", "\\t", "\\t", "\\r", "wait:1", "\\x1b[B", "\\x1b[B", "\\r", "wait:1.5", "\\t", "delta/events/", "\\t", "\\t", "\\r", "wait:4", "shot:plugin", "\\x1b[24~", "wait:1.5", "shot:closed"]')"
echo "$screen" | sed -n '/===== plugin/,/===== closed/p' | grep -q "Version 2: 2 files" || { echo "$screen"; echo "the plugin view did not show the table"; exit 1; }
echo "$screen" | sed -n '/===== closed/,$p' | grep -q "TRUSTED ROOTS" || { echo "F12 did not return to sonde"; exit 1; }
# revoke the package: it is disabled and stays so
sha="$(nsh verify "$tmp/delta.nshpkg" --root "$tmp/keys/root.pub" | sed -n 's/.*sha256 \([0-9a-f]*\).*/\1/p')"
nsh revoke --root "$tmp/keys/root.secret" --sequence 1 --package "$sha" -o "$tmp/rev.json"
"$sonde" plugin revoke "$tmp/rev.json"
if "$sonde" plugin enable dev.sonde.plugins.delta; then echo "a revoked plugin was enabled"; exit 1; fi
echo "plugin e2e: ok"
