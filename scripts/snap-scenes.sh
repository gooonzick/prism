#!/usr/bin/env bash
# Render every scene in light and dark from a demo home: scripts/snap-scenes.sh <out-dir>
set -euo pipefail
cd "$(dirname "$0")/.."
out="${1:?usage: snap-scenes.sh <out-dir>}"
mkdir -p "$out"
cargo build -q
demo=$(scripts/demo-home.sh "$out/readme" | tail -n 1 | sed 's/.*demo home in //')
scenes=$(grep -o '("[a-z-]*", |s|' src/main.rs | sed 's/("\([a-z-]*\)".*/\1/')
for scene in none $scenes; do
  for theme in light dark; do
    args=()
    # The gallery has one column per group of blocks; it needs more width than a screen.
    size=1180x760
    [ "$scene" = ds ] && size=1500x760
    [ "$scene" = none ] || args=(--scene "$scene")
    env -u CLAUDE_CONFIG_DIR -u CODEX_HOME -u XDG_CONFIG_HOME HOME="$demo" \
      target/debug/prism --snap "$out/$scene-$theme.png" --size "$size" ${args[@]+"${args[@]}"} --theme "$theme"
  done
done
echo "Snapshots in $out"
