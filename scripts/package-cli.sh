#!/usr/bin/env bash
# Builds the CLI-only Prism (no window) and packs it as dist/prism-cli-<version>-<os>-<arch>.tar.gz.
# On Linux it is a static musl build, so it runs on hosts with an older glibc than the builder's.
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

if [[ "$OS" == linux ]]; then
  # Static (musl), so it starts on any Linux, whatever its glibc. Needs musl-gcc for ring's C
  # code: `apt-get install musl-tools`.
  TRIPLE="$(uname -m | sed 's/^arm64$/aarch64/')-unknown-linux-musl"
  if command -v rustup >/dev/null 2>&1; then rustup target add "$TRIPLE"; fi
  cargo build --release --locked --no-default-features --target "$TRIPLE"
  OUT="$TARGET/$TRIPLE/release"
else
  cargo build --release --locked --no-default-features
  OUT="$TARGET/release"
fi

mkdir -p dist
tar -czf "dist/$NAME" -C "$OUT" prism
echo "Built dist/$NAME"
