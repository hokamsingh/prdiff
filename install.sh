#!/bin/sh
# Install prdiff from a GitHub release.
#   curl -fsSL https://raw.githubusercontent.com/hokamsingh/prdiff/main/install.sh | sh
# Environment: PRDIFF_VERSION (default: latest), PRDIFF_BIN_DIR (default: ~/.local/bin),
#              PRDIFF_BASE_URL (default: the GitHub releases URL; used for testing).
set -eu

REPO="hokamsingh/prdiff"
VERSION="${PRDIFF_VERSION:-latest}"
BIN_DIR="${PRDIFF_BIN_DIR:-$HOME/.local/bin}"
BASE_URL="${PRDIFF_BASE_URL:-https://github.com/$REPO/releases}"

fail() { echo "install.sh: $*" >&2; exit 1; }

case "$(uname -s)" in
  Linux)  os=unknown-linux-musl ;;
  Darwin) os=apple-darwin ;;
  *) fail "unsupported OS: $(uname -s) (build from source: cargo install --git https://github.com/$REPO)" ;;
esac
case "$(uname -m)" in
  x86_64|amd64)  arch=x86_64 ;;
  arm64|aarch64) arch=aarch64 ;;
  *) fail "unsupported CPU: $(uname -m)" ;;
esac
asset="prdiff-$arch-$os.tar.gz"

if [ "$VERSION" = "latest" ]; then
  url_dir="$BASE_URL/latest/download"
else
  url_dir="$BASE_URL/download/$VERSION"
fi

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q "$1" -O "$2"; }
else
  fail "need curl or wget"
fi

# A private repository answers 404 to anonymous downloads. With the GitHub CLI logged in, read the release
# through the REST API instead (not used when PRDIFF_BASE_URL is set).
use_gh=0
if [ -z "${PRDIFF_BASE_URL:-}" ] && command -v gh >/dev/null 2>&1 && gh api "repos/$REPO" --jq .full_name >/dev/null 2>&1; then
  use_gh=1
fi

gh_fetch() { # <asset name> <destination>
  rel="releases/latest"
  [ "$VERSION" = "latest" ] || rel="releases/tags/$VERSION"
  id=$(gh api "repos/$REPO/$rel" --jq ".assets[] | select(.name == \"$1\") | .id") || return 1
  [ -n "$id" ] || return 1
  gh api "repos/$REPO/releases/assets/$id" -H "Accept: application/octet-stream" > "$2"
}

get() { # <asset name> <destination>
  fetch "$url_dir/$1" "$2" 2>/dev/null && return 0
  [ "$use_gh" = 1 ] && gh_fetch "$1" "$2"
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "Downloading $asset ($VERSION)"
get "$asset" "$tmp/$asset" || fail "download failed (is there a release for $asset? for a private repository, log in with: gh auth login)"
get "$asset.sha256" "$tmp/$asset.sha256" || fail "checksum download failed"

want=$(cut -d ' ' -f 1 "$tmp/$asset.sha256")
if command -v sha256sum >/dev/null 2>&1; then
  got=$(sha256sum "$tmp/$asset" | cut -d ' ' -f 1)
elif command -v shasum >/dev/null 2>&1; then
  got=$(shasum -a 256 "$tmp/$asset" | cut -d ' ' -f 1)
else
  fail "need sha256sum or shasum to verify the download"
fi
[ "$want" = "$got" ] || fail "checksum mismatch (expected $want, got $got)"

tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$BIN_DIR"
install -m 755 "$tmp/prdiff" "$BIN_DIR/prdiff"
echo "Installed $("$BIN_DIR/prdiff" --version) to $BIN_DIR/prdiff"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "Note: $BIN_DIR is not on your PATH." ;;
esac
