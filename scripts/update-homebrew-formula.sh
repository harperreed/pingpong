#!/bin/bash
# ABOUTME: Generates the Homebrew formula for the pingpong release assets.
# ABOUTME: Writes Formula/pingpong.rb into a checked-out Homebrew tap repository.

set -euo pipefail

required_env=(
    "TAP_REPO_PATH"
    "RELEASE_TAG"
    "RELEASE_VERSION"
    "INTEL_ASSET_PATH"
    "ARM_ASSET_PATH"
)

for name in "${required_env[@]}"; do
    if [ -z "${!name:-}" ]; then
        echo "Missing required environment variable: $name" >&2
        exit 1
    fi
done

RELEASE_REPOSITORY="${RELEASE_REPOSITORY:-harperreed/pingpong}"
FORMULA_PATH="$TAP_REPO_PATH/Formula/pingpong.rb"
INTEL_ASSET_NAME=$(basename "$INTEL_ASSET_PATH")
ARM_ASSET_NAME=$(basename "$ARM_ASSET_PATH")

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

INTEL_SHA256=$(sha256_file "$INTEL_ASSET_PATH")
ARM_SHA256=$(sha256_file "$ARM_ASSET_PATH")

mkdir -p "$(dirname "$FORMULA_PATH")"

cat > "$FORMULA_PATH" <<FORMULA
# typed: false
# frozen_string_literal: true

class Pingpong < Formula
  desc "A beautiful TUI ping utility for monitoring network connectivity"
  homepage "https://github.com/${RELEASE_REPOSITORY}"
  version "${RELEASE_VERSION}"
  license "MIT"
  depends_on :macos

  if Hardware::CPU.intel?
    url "https://github.com/${RELEASE_REPOSITORY}/releases/download/${RELEASE_TAG}/${INTEL_ASSET_NAME}", using: :nounzip
    sha256 "${INTEL_SHA256}"
  end

  if Hardware::CPU.arm?
    url "https://github.com/${RELEASE_REPOSITORY}/releases/download/${RELEASE_TAG}/${ARM_ASSET_NAME}", using: :nounzip
    sha256 "${ARM_SHA256}"
  end

  def install
    if Hardware::CPU.arm?
      bin.install "${ARM_ASSET_NAME}" => "pingpong"
    else
      bin.install "${INTEL_ASSET_NAME}" => "pingpong"
    end
  end

  test do
    system "#{bin}/pingpong", "--help"
  end
end
FORMULA
