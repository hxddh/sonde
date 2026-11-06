#!/usr/bin/env sh
# Signs a plugin as its publisher would: a root key (offline in real life),
# a release key certified for `dev.sonde.plugins.*`, and `nsh publish`.
#
#   scripts/sign-plugin.sh <plugin-dir> <keys-dir> [out.nshpkg]
#
# Keys are created in <keys-dir> on first use (never commit *.secret).
set -eu
plugin="$1"
keys="$2"
out="${3:-$plugin/.nsh/out/plugin.nshpkg}"
mkdir -p "$keys" "$(dirname "$out")"
[ -f "$keys/root.secret" ] || nsh keygen "$keys/root"
[ -f "$keys/release.secret" ] || nsh keygen "$keys/release"
[ -f "$keys/release.cert" ] || nsh certify --root "$keys/root.secret" --key "$keys/release.pub" --apps 'dev.sonde.plugins.*' -o "$keys/release.cert"
nsh publish "$plugin/App.tsx" --key "$keys/release.secret" --cert "$keys/release.cert" -o "$out"
nsh verify "$out" --root "$keys/root.pub"
