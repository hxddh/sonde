#!/usr/bin/env sh
# Installs the sonde terminal binary from GitHub Releases, verifying its
# SHA-256 and, where gh is installed, its build provenance.
#
#   curl -fsSL https://raw.githubusercontent.com/hxddh/sonde/main/install.sh | sh
#   SONDE_VERSION=30.0.0 SONDE_INSTALL_DIR=$HOME/bin sh install.sh
#   SONDE_VERSION=23 sh install.sh    # the newest 23.x (23.1: the newest 23.1.x)
#   SONDE_STATIC=1 sh install.sh    # Linux x86_64: the static (musl) build
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
# The Linux builds need glibc 2.35 or newer (built on Ubuntu 22.04). Alpine
# and other musl systems take the static x86_64 build, and so does an older
# glibc: the glibc build is tried after it is unpacked (below). aarch64 has
# no static build.
musl=""
if [ "$os" = linux ] && { [ -n "${SONDE_STATIC:-}" ] || ls /lib/ld-musl-* >/dev/null 2>&1; }; then musl=1; fi
# this system's C library, for a message: "musl", "glibc 2.31"
libc_name() {
  if ls /lib/ld-musl-* >/dev/null 2>&1; then echo musl; else getconf GNU_LIBC_VERSION 2>/dev/null || echo "an unknown C library"; fi
}
libc=""
if [ -n "$musl" ]; then
  if [ "$arch" != x86_64 ]; then
    [ -z "${SONDE_STATIC:-}" ] || fail "SONDE_STATIC=1: there is no static sonde build for Linux aarch64 (only for x86_64)"
    fail "no sonde build for Linux aarch64 on musl: the aarch64 build needs glibc 2.35 or newer, and there is no static one"
  fi
  libc=-musl
fi
asset="sonde-$os-$arch$libc.tar.gz"
# A major alone (30) is its newest release (30.x.y), a major and minor (30.0)
# the newest 30.0.y; prereleases left out
case "$version" in
  latest | *.*.*) ;;
  *)
    want="${version#v}"
    case "$want" in
      *[!0-9.]* | *..* | .* | *.) fail "SONDE_VERSION=$version: a version (30.0.0), a major (30), a major and minor (30.0), or latest" ;;
      *.*) pattern="^$(echo "$want" | sed 's/\./\\./')\.[0-9]*$" ;;
      *) pattern="^$want\.[0-9]*\.[0-9]*$" ;;
    esac
    auth=""
    [ -z "${GH_TOKEN:-}" ] || auth="Authorization: Bearer $GH_TOKEN"
    version="$(curl -fsSL ${auth:+-H "$auth"} "https://api.github.com/repos/$repo/releases?per_page=100" |
      sed -n 's/.*"tag_name": *"v\{0,1\}\([0-9][0-9.]*\)".*/\1/p' | grep "$pattern" |
      sort -t. -k1,1n -k2,2n -k3,3n | tail -n 1)" || version=""
    [ -n "$version" ] || fail "no release of sonde $want found"
    ;;
esac
if [ "$version" = latest ]; then
  base="https://github.com/$repo/releases/latest/download"
else
  base="https://github.com/$repo/releases/download/v${version#v}"
fi
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
# The archive $1, its checksum and (where gh can) its build provenance
# checked, unpacked in $tmp/$1.d.
fetch() {
  a="$1"
  echo "downloading $base/$a"
  curl -fsSL "$base/$a" -o "$tmp/$a" || fail "could not download $base/$a"
  curl -fsSL "$base/$a.sha256" -o "$tmp/$a.sha256" || fail "could not download $base/$a.sha256"
  expected="$(cut -d' ' -f1 "$tmp/$a.sha256")"
  if command -v sha256sum >/dev/null 2>&1; then actual="$(sha256sum "$tmp/$a" | cut -d' ' -f1)"; else actual="$(shasum -a 256 "$tmp/$a" | cut -d' ' -f1)"; fi
  [ "$expected" = "$actual" ] || fail "checksum mismatch: expected $expected, got $actual"
  # The checksum comes from where the archive does. Where the GitHub CLI is
  # installed and signed in, the archive's build provenance is checked too:
  # built by this repository's release workflow, from its own commit.
  # SONDE_VERIFY=0 skips it; SONDE_VERIFY=1 requires it.
  verify="${SONDE_VERIFY:-auto}"
  if [ "$verify" != 0 ]; then
    if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
      gh attestation verify "$tmp/$a" --repo "$repo" >/dev/null ||
        fail "$a has no build provenance from $repo (gh attestation verify failed)"
      echo "verified: $a was built by $repo's release workflow"
    elif [ "$verify" = 1 ]; then
      fail "SONDE_VERIFY=1 needs the GitHub CLI (gh), signed in, to check the build provenance"
    else
      echo "(not verified beyond its checksum: install gh and sign in to check where it was built)"
    fi
  fi
  mkdir -p "$tmp/$a.d"
  tar -xzf "$tmp/$a" -C "$tmp/$a.d"
}
fetch "$asset"
bin="$tmp/$asset.d/sonde"
# a binary that cannot start here is never installed
if ! "$bin" --version >/dev/null 2>&1; then
  if [ "$os-$arch$libc" = linux-x86_64 ]; then
    echo "$asset cannot start here ($(libc_name); it needs glibc 2.35 or newer): taking the static build"
    asset="sonde-linux-x86_64-musl.tar.gz"
    fetch "$asset"
    bin="$tmp/$asset.d/sonde"
  elif [ "$os-$arch" = linux-aarch64 ]; then
    fail "$asset cannot start here ($(libc_name)): the aarch64 build needs glibc 2.35 or newer, and there is no static one"
  fi
fi
version_said="$("$bin" --version 2>&1)" || fail "$asset cannot start here: ${version_said:-no output}"
mkdir -p "$dir"
install -m 0755 "$bin" "$dir/sonde"
echo "installed $version_said to $dir/sonde"
# the manual pages beside a bin folder, where man looks for a PATH entry's
# (~/.local/bin: ~/.local/share/man); completions: sonde completions <shell>
pages="$(dirname "$bin")/man/man1"
if [ "$(basename "$dir")" = bin ] && [ -d "$pages" ]; then
  man1="$(dirname "$dir")/share/man/man1"
  mkdir -p "$man1" && cp "$pages"/*.1 "$man1"/ && echo "manual pages in $man1 (man sonde)"
fi
case ":$PATH:" in *":$dir:"*) ;; *) echo "add $dir to your PATH" ;; esac
