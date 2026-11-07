#!/usr/bin/env bash
# Builds Prism for Linux and packs it as dist/prism-<version>-linux-<arch>.tar.gz: the binary,
# a desktop entry and the icon, laid out like ~/.local. With --install it also installs them
# into ~/.local, so Prism shows up in the app launcher.
set -euo pipefail
cd "$(dirname "$0")/.."

ID="dev.prism.skills"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
ARCH="$(uname -m)"
NAME="prism-$VERSION-linux-$ARCH"
STAGE="dist/$NAME"
TARGET="${CARGO_TARGET_DIR:-target}"

cargo build --release
cargo build --release --example render_icon

rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/share/applications"
cp "$TARGET/release/prism" "$STAGE/bin/prism"
for size in 128 256 512; do
  mkdir -p "$STAGE/share/icons/hicolor/${size}x${size}/apps"
  "$TARGET/release/examples/render_icon" assets/icon.svg "$STAGE/share/icons/hicolor/${size}x${size}/apps/$ID.png" $size
done
cat >"$STAGE/share/applications/$ID.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Prism
Comment=Browse, install and sync agent skills across coding agents
Exec=prism
Icon=$ID
Terminal=false
Categories=Development;Utility;
Keywords=skills;agents;claude;codex;opencode;
DESKTOP
cat >"$STAGE/install.sh" <<'INSTALL'
#!/usr/bin/env sh
# Installs Prism into ~/.local (or $PREFIX): the binary, its desktop entry and icons.
set -eu
cd "$(dirname "$0")"
PREFIX="${PREFIX:-$HOME/.local}"
mkdir -p "$PREFIX"
cp -R bin share "$PREFIX/"
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$PREFIX/share/icons/hicolor" || true
echo "Installed Prism into $PREFIX. Run it from the app launcher, or as: prism"
INSTALL
chmod +x "$STAGE/install.sh"
cp LICENSE README.md "$STAGE/"

tar -czf "dist/$NAME.tar.gz" -C dist "$NAME"
echo "Built dist/$NAME.tar.gz"

if [[ "${1:-}" == "--install" ]]; then
  "$STAGE/install.sh"
fi
