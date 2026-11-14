#!/usr/bin/env sh
# Installs the sonde terminal binary from GitHub Releases, verifying its
# SHA-256 and, where gh is installed, its build provenance.
#
#   curl -fsSL https://raw.githubusercontent.com/hxddh/sonde/main/install.sh | sh
#   SONDE_VERSION=20.0.0 SONDE_INSTALL_DIR=$HOME/bin sh install.sh
#   SONDE_VERSION=19 sh install.sh    # the newest 20.x
#   SONDE_VERIFY=1 sh install.sh    # fail unless gh confirms the provenance
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
# Alpine and other musl systems take the static build
libc=""
if [ "$os-$arch" = linux-x86_64 ] && { [ -n "${SONDE_STATIC:-}" ] || ls /lib/ld-musl-* >/dev/null 2>&1; }; then libc=-musl; fi
asset="sonde-$os-$arch$libc.tar.gz"
# A major alone (20) is its newest release (20.x.y), prereleases left out
case "$version" in
  latest | *.*) ;;
  *)
    major="${version#v}"
    auth=""
    [ -z "${GH_TOKEN:-}" ] || auth="Authorization: Bearer $GH_TOKEN"
    version="$(curl -fsSL ${auth:+-H "$auth"} "https://api.github.com/repos/$repo/releases?per_page=100" |
      sed -n 's/.*"tag_name": *"v\{0,1\}\([0-9][0-9.]*\)".*/\1/p' | grep "^$major\.[0-9]*\.[0-9]*$" |
      sort -t. -k1,1n -k2,2n -k3,3n | tail -n 1)" || version=""
    [ -n "$version" ] || fail "no release of sonde $major found"
    ;;
esac
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
# The checksum comes from where the archive does. Where the GitHub CLI is
# installed and signed in, the archive's build provenance is checked too:
# built by this repository's release workflow, from its own commit.
# SONDE_VERIFY=0 skips it; SONDE_VERIFY=1 requires it.
verify="${SONDE_VERIFY:-auto}"
if [ "$verify" != 0 ]; then
  if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
    gh attestation verify "$tmp/$asset" --repo "$repo" >/dev/null ||
      fail "$asset has no build provenance from $repo (gh attestation verify failed)"
    echo "verified: $asset was built by $repo's release workflow"
  elif [ "$verify" = 1 ]; then
    fail "SONDE_VERIFY=1 needs the GitHub CLI (gh), signed in, to check the build provenance"
  else
    echo "(not verified beyond its checksum: install gh and sign in to check where it was built)"
  fi
fi
tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$dir"
install -m 0755 "$tmp/sonde" "$dir/sonde"
echo "installed $("$dir/sonde" --version) to $dir/sonde"
case ":$PATH:" in *":$dir:"*) ;; *) echo "add $dir to your PATH" ;; esac
