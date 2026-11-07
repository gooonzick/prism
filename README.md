<div align="center">

<img src="assets/icon.svg" width="128" alt="Prism icon">

# Prism

**One home for your agent skills, linked into every coding agent.**

A native macOS app to browse the skills on your machine, install new ones from [skills.sh](https://skills.sh), and keep Claude Code, Codex, OpenCode, Cursor and the rest in sync, globally or per project.

![Rust](https://img.shields.io/badge/Rust-1.99-orange?logo=rust)
![macOS](https://img.shields.io/badge/macOS-13%2B-lightgrey?logo=apple)
![License: MIT](https://img.shields.io/badge/license-MIT-blue)

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/detail-dark.png">
  <img src="docs/detail.png" alt="Prism showing global skills, their agents and a skill's details">
</picture>

</div>

## Why

Every coding agent looks for skills in its own folder: `~/.claude/skills`, `~/.codex/skills`, `~/.config/opencode/skills`, and more in every project. Install a skill for one agent and the others never see it. Copies drift apart, links break, and nothing shows you the whole picture.

Prism keeps **one real copy** of each skill in `.agents/skills`, the same place the [`skills` CLI](https://github.com/vercel-labs/skills) puts them, and links it into every agent you use.

```
~/.agents/skills/find-skills            ← the skill itself
~/.claude/skills/find-skills            → ../../.agents/skills/find-skills
~/.config/opencode/skills/find-skills   → ../../../.agents/skills/find-skills
```

## Features

- **See everything in one place.** Every skill in `~` or in a project, which agents have it, and whether each one is a link, its own copy, or a broken link.
- **Sync with a preview.** Prism shows the full plan before it changes anything:
  - a skill only one agent has moves into `.agents` and is linked back;
  - identical copies are replaced with links;
  - folders that differ are skipped, never overwritten.
- **Install from skills.sh.** Search the index, preview `SKILL.md`, list every skill in a repository and pick the ones you want. Installs and updates run through the official `skills` CLI.
- **Updates.** Prism checks your skills against their sources in the background and updates one, several, or all of them.
- **Projects.** Add project folders one by one, or let *Find projects* scan a folder for projects that already have skills. Skills move between scopes with *Copy to*.
- **Safe by default.** Nothing is deleted outright: replaced folders go to the Trash. The last change (a sync, remove, cleanup or copy) can be undone with ⌘Z.
- **Live.** Prism watches the skill folders, so changes from the terminal, an editor or git show up on their own.
- **Fast and native.** Written in Rust on [Rune](https://github.com/anomalyco/rune), with a GPU-drawn UI in light and dark.

<table>
  <tr>
    <td><img src="docs/sync.png" alt="Sync dialog with the planned changes"></td>
    <td><img src="docs/browse.png" alt="Searching skills.sh"></td>
  </tr>
  <tr>
    <td align="center">Every sync shows its plan first</td>
    <td align="center">Search and install from skills.sh</td>
  </tr>
</table>

## Supported agents

| Agent | Global folder | Project folder |
| --- | --- | --- |
| Shared store | `~/.agents/skills` | `.agents/skills` |
| Claude Code | `~/.claude/skills` | `.claude/skills` |
| Codex | `~/.codex/skills` | `.codex/skills` |
| OpenCode | `~/.config/opencode/skills` | `.opencode/skills` |
| Cursor | `~/.cursor/skills` | `.cursor/skills` |
| Gemini CLI | `~/.gemini/skills` | `.gemini/skills` |
| GitHub Copilot | `~/.copilot/skills` | `.github/skills` |
| Windsurf | `~/.codeium/windsurf/skills` | `.windsurf/skills` |
| Goose | `~/.config/goose/skills` | `.goose/skills` |
| Kiro | `~/.kiro/skills` | `.kiro/skills` |
| Roo Code | `~/.roo/skills` | `.roo/skills` |

By default Prism links into the agents installed on your machine; you can change the list in the Sync dialog. `CLAUDE_CONFIG_DIR`, `CODEX_HOME` and `XDG_CONFIG_HOME` are respected.

## Install

Prism builds from source. You need [Rust](https://rustup.rs) (the pinned toolchain installs itself) and, for skills.sh, Node.js with `pnpx`, `npx` or `bunx`.

```bash
git clone https://github.com/gooonzick/prism.git
cd prism
./scripts/bundle-macos.sh --install
```

This builds `Prism.app` and copies it to `~/Applications`, where Spotlight and Launchpad find it. To run it straight from the terminal, optionally opening a project:

```bash
cargo run --release -- ~/path/to/project
```

## Keyboard

| Keys | Action |
| --- | --- |
| ↑ ↓ | Move through the list |
| ⇧↑ ⇧↓, ⇧-click | Select a range |
| ⌘-click, Space | Select a skill |
| ⌘A | Select all |
| Esc | Clear the selection |
| ⌘N | New skill |
| ⌘Z | Undo the last change |
| ⌘R | Rescan |
| ⌘F | Filter |
| ⌘K | Search skills.sh |
| ⇧⌘B | Toggle the sidebar |

## Development

```bash
cargo test                    # core and UI tests, headless, on a throwaway $HOME
cargo test -- --ignored       # also hit skills.sh and GitHub
./scripts/demo-home.sh        # rebuild the screenshots in docs/ from a demo home
```

Any screen renders without a window, which is handy for checking layout:

```bash
./target/debug/prism --snap out.png --scene sync --theme dark
```

Scenes: `detail`, `sync`, `remove`, `bulk`, `bulk-remove`, `updates`, `copy`, `new-skill`, `browse`, `repo`, `notice`.

| Path | What it does |
| --- | --- |
| `src/agents.rs` | the agents and their skill folders |
| `src/scan.rs` | what is installed where: folder, link, foreign or broken link |
| `src/sync.rs` | sync plans, linking, removing, copying, broken links |
| `src/fsops.rs` | the change journal behind undo, and the Trash |
| `src/remote.rs` | skills.sh search, previews from GitHub, the `skills` CLI |
| `src/app/` | app state and actions |
| `src/ui/` | the screens, built with rune-kit |

## License

[MIT](LICENSE)
