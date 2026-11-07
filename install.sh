#!/usr/bin/env sh
# Installs the sonde terminal binary from GitHub Releases, verifying its
# SHA-256.
#
#   curl -fsSL https://raw.githubusercontent.com/hxddh/sonde/main/install.sh | sh
#   SONDE_VERSION=2.0.0 SONDE_INSTALL_DIR=$HOME/bin sh install.sh
set -eu
repo="hxddh/sonde"
version="${SONDE_VERSION:-latest}"
dir="${SONDE_INSTALL_DIR:-$HOME/.local/bin}"
fail() { echo "install.sh: $*" >&2; exit 1; }
command -v curl >/dev/null 2>&1 || fail "curl is required"
os="$(uname -s)"; arch="$(uname -m)"
case "$os" in
  Linux) os=linux ;;
  Darwin) os=macos ;;
  *) fail "no sonde build for $os" ;;
esac
case "$arch" in
  x86_64|amd64) arch=x86_64 ;;
  arm64|aarch64) arch=aarch64 ;;
  *) fail "no sonde build for $arch" ;;
esac
asset="sonde-$os-$arch.tar.gz"
if [ "$version" = latest ]; then
  base="https://github.com/$repo/releases/latest/download"
else
  base="https://github.com/$repo/releases/download/v${version#v}"
fi
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
echo "downloading $base/$asset"
curl -fsSL "$base/$asset" -o "$tmp/$asset"
curl -fsSL "$base/$asset.sha256" -o "$tmp/$asset.sha256"
expected="$(cut -d' ' -f1 "$tmp/$asset.sha256")"
if command -v sha256sum >/dev/null 2>&1; then actual="$(sha256sum "$tmp/$asset" | cut -d' ' -f1)"; else actual="$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1)"; fi
[ "$expected" = "$actual" ] || fail "checksum mismatch: expected $expected, got $actual"
tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$dir"
install -m 0755 "$tmp/sonde" "$dir/sonde"
echo "installed $("$dir/sonde" --version) to $dir/sonde"
case ":$PATH:" in *":$dir:"*) ;; *) echo "add $dir to your PATH" ;; esac
