#!/usr/bin/env bash
# Builds the CLI-only Prism (no window) and packs it as dist/prism-cli-<version>-<os>-<arch>.tar.gz.
# The archive holds one member, `prism`: `prism hosts install` extracts exactly that name.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
case "$(uname -s)" in
  Darwin) OS=macos ;;
  Linux) OS=linux ;;
  *) echo "unsupported system: $(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  aarch64 | arm64) ARCH=arm64 ;;
  x86_64) ARCH=x86_64 ;;
  *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac
NAME="prism-cli-$VERSION-$OS-$ARCH.tar.gz"
TARGET="${CARGO_TARGET_DIR:-target}"

cargo build --release --no-default-features

mkdir -p dist
tar -czf "dist/$NAME" -C "$TARGET/release" prism
echo "Built dist/$NAME"
