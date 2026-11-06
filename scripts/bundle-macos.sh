#!/usr/bin/env bash
# Builds "Skills Explorer.app" in target/release, and with --install copies it to
# ~/Applications. Besides cargo it needs only iconutil and codesign, which macOS ships.
set -euo pipefail
cd "$(dirname "$0")/.."

NAME="Skills Explorer"
ID="dev.skills-explorer"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
APP="target/release/$NAME.app"

cargo build --release

# The icon: the SVG rendered at every size an iconset holds, transparent around the tile.
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cargo build --release --example render_icon
mkdir -p "$WORK/icon.iconset"
for size in 16 32 128 256 512; do
  target/release/examples/render_icon assets/icon.svg "$WORK/icon.iconset/icon_${size}x${size}.png" $size
  target/release/examples/render_icon assets/icon.svg "$WORK/icon.iconset/icon_${size}x${size}@2x.png" $((size * 2))
done
iconutil -c icns "$WORK/icon.iconset" -o "$WORK/AppIcon.icns"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/skills-explorer "$APP/Contents/MacOS/skills-explorer"
cp "$WORK/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleIdentifier</key><string>$ID</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>skills-explorer</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
# Ad-hoc signature, so Gatekeeper treats it as one app rather than a loose binary.
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true
echo "Built $APP"

if [[ "${1:-}" == "--install" ]]; then
  mkdir -p "$HOME/Applications"
  rm -rf "$HOME/Applications/$NAME.app"
  cp -R "$APP" "$HOME/Applications/"
  echo "Installed to ~/Applications/$NAME.app"
fi
