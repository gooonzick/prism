#!/usr/bin/env bash
# Builds a throwaway home folder with a few skills in every state Prism shows (synced, an
# agent's own copy, half linked, a broken link) and renders the README screenshots from it.
# Usage: scripts/demo-home.sh [out dir, default docs]
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${1:-docs}"

DEMO="$(cd "$(mktemp -d)" && pwd -P)/home"
PROJECT="$DEMO/Development/acme-web"
skill() { # dir name description
  mkdir -p "$1/$2"
  printf -- '---\nname: %s\ndescription: %s\n---\n\n# %s\n\n%s\n\n## When to use\n\n- The task matches the description above\n- You want the same result every time\n' "$2" "$3" "$2" "$3" >"$1/$2/SKILL.md"
}
link() { # agent dir, name
  mkdir -p "$1"
  ln -s "../../.agents/skills/$2" "$1/$2"
}

# Global: three synced skills, one only in .agents, two Claude-only copies, one broken link.
mkdir -p "$DEMO/.config/opencode"
A="$DEMO/.agents/skills"
skill "$A" find-skills "Finds and installs agent skills when you ask how to do something new."
skill "$A" frontend-design "Builds distinctive, production-grade interfaces instead of generic layouts."
skill "$A" vercel-react-best-practices "React and Next.js performance guidelines from Vercel Engineering."
skill "$A" web-design-guidelines "Reviews UI code against interface and accessibility guidelines."
for s in find-skills frontend-design vercel-react-best-practices; do
  link "$DEMO/.claude/skills" "$s"
  mkdir -p "$DEMO/.config/opencode/skills"
  ln -s "../../../.agents/skills/$s" "$DEMO/.config/opencode/skills/$s"
done
skill "$DEMO/.claude/skills" pdf "Reads, fills and merges PDF files, and extracts tables from them."
skill "$DEMO/.claude/skills" commit-messages "Writes commit messages that explain why a change was made."
ln -s "$DEMO/Development/old-prompts/skills/prompt-library" "$DEMO/.config/opencode/skills/prompt-library"

# A project: one skill synced, one Claude-only.
skill "$PROJECT/.agents/skills" api-conventions "How endpoints, errors and pagination are shaped in this codebase."
link "$PROJECT/.claude/skills" api-conventions
skill "$PROJECT/.claude/skills" release-checklist "Steps to cut, tag and announce a release of acme-web."

SETTINGS="$DEMO/Library/Application Support/dev.prism.skills"
mkdir -p "$SETTINGS"
printf '{ "projects": ["%s"] }\n' "$PROJECT" >"$SETTINGS/settings.json"

cargo build -q
mkdir -p "$OUT"
snap() { # file, then extra args
  local file="$1"; shift
  env -u CLAUDE_CONFIG_DIR -u CODEX_HOME -u XDG_CONFIG_HOME HOME="$DEMO" \
    target/debug/prism --snap "$OUT/$file" --size 1180x760 "$@"
}
snap global.png --theme light
snap detail.png --scene detail --theme light
snap detail-dark.png --scene detail --theme dark
snap sync.png --scene sync --theme light
snap browse.png --scene browse --theme light
snap project.png --theme light "$PROJECT"
echo "Screenshots in $OUT, demo home in $DEMO"
