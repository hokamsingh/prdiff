#!/bin/sh
# Render a Homebrew formula for a released version, using the checksums published with the release.
#   packaging/homebrew/render.sh v0.2.0 > Formula/prdiff.rb     (in your homebrew-tap repo)
set -eu
VERSION="${1:?usage: render.sh vX.Y.Z}"
REPO="hokamsingh/prdiff"
BASE="${PRDIFF_BASE_URL:-https://github.com/$REPO/releases}/download/$VERSION"
sum() { curl -fsSL "$BASE/prdiff-$1.tar.gz.sha256" | cut -d ' ' -f 1; }

cat <<RUBY
class Prdiff < Formula
  desc "Real-diff checker for PR descriptions: what actually changes when head lands on base"
  homepage "https://github.com/$REPO"
  version "${VERSION#v}"

  on_macos do
    if Hardware::CPU.arm?
      url "$BASE/prdiff-aarch64-apple-darwin.tar.gz"
      sha256 "$(sum aarch64-apple-darwin)"
    else
      url "$BASE/prdiff-x86_64-apple-darwin.tar.gz"
      sha256 "$(sum x86_64-apple-darwin)"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "$BASE/prdiff-aarch64-unknown-linux-musl.tar.gz"
      sha256 "$(sum aarch64-unknown-linux-musl)"
    else
      url "$BASE/prdiff-x86_64-unknown-linux-musl.tar.gz"
      sha256 "$(sum x86_64-unknown-linux-musl)"
    end
  end

  def install
    bin.install "prdiff"
    generate_completions_from_executable(bin/"prdiff", "completions")
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/prdiff --version")
  end
end
RUBY
