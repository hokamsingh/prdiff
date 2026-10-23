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
  url="$BASE_URL/latest/download/$asset"
else
  url="$BASE_URL/download/$VERSION/$asset"
fi

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q "$1" -O "$2"; }
else
  fail "need curl or wget"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "Downloading $url"
fetch "$url" "$tmp/$asset" || fail "download failed (is there a release for $asset?)"
fetch "$url.sha256" "$tmp/$asset.sha256" || fail "checksum download failed"

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
