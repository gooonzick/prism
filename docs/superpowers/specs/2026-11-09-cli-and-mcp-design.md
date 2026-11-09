# Prism: a CLI and MCP server management

Status: ready for approval · 2026-11-09

## Why

Prism keeps one copy of each skill in `.agents/skills` and links it into every coding agent. Two gaps remain for someone who works with several agents:

1. **MCP servers.** Each agent keeps its own list of servers, in its own file and format. George adds the same server by hand to Claude Code, Cursor, Gemini and Codex. On his machine `plata` sits in four configs with the same command and 16 identical env vars, and the copies have already drifted: `context7` has an API key header in Claude Code and none in Cursor; the Figma server is `framelink-mcp` via `bunx` in Claude Code and "Framelink MCP for Figma" via `npx` with different args in Cursor.
2. **Agents can't use Prism.** Everything Prism does needs the window. George wants agents to be able to do everything the UI does, so an agent in a session can install a skill or add an MCP server to all agents with one command.

## Goals

- Manage MCP servers the way Prism manages skills: one list across agents, you pick which agents get each server, global or per project.
- Show where copies of a server differ, and let you pick the right one.
- A `prism` CLI with the same capabilities as the UI, built for agents: JSON output, no prompts, clear exit codes.
- A `prism` skill that teaches agents to use the CLI.

## Non-goals (for this version)

- Removing secrets from configs. Prism copies values as they are; the user accepted plaintext tokens in agent configs. A Keychain-backed launcher (`prism mcp run NAME`) may come later as a per-server option.
- Proxying HTTP servers, OAuth flows, or showing claude.ai connectors (Claude Code keeps no local list of them; listing them would need an authenticated network call).
- Instruction files (`AGENTS.md`, `CLAUDE.md`), commands, subagents, usage stats.
- MCP for VS Code, Copilot, Goose and Roo Code (different shapes: `servers` + `inputs`, YAML, VS Code global storage). They can follow.
- Searching MCP registries for new servers.
- Windows.

## Anti-goals (a failure even if it works)

- **A broken or rewritten config.** Prism must never lose keys it doesn't understand, reorder or reformat a file beyond the entry it changed, drop comments from JSONC/TOML, or leave a half-written file. `~/.claude.json` holds ~100 keys of Claude Code state; damaging it costs the user their Claude Code setup.
- **Secrets leaking to agents or logs.** CLI output masks env and header values unless asked.
- **An agent deleting things without a plan.** Commands that change or remove existing state show a plan and only act with `--apply`.
- **Fighting app-managed servers.** Servers that apps install for themselves (Pencil writes one per agent with its own `--agent` flag; Codex's `node_repl` and `computer-use`) stay untouched.
- **The CLI drifting from the UI.** Both call the same core; the CLI is not a second implementation.

## Decisions made with George

- Adding or copying a server: you pick target agents, like skills. Not "everywhere by default".
- Secrets: copied as is in v1.
- The source of truth is the agents' own configs. Prism has no MCP store of its own (a store would be one more plaintext copy of every token, and a project-level one invites committing secrets).
- Copies of one server are matched by name, and by command or URL when names differ; Prism suggests merging differently named copies and merges only when the user confirms.
- App-managed servers are shown read-only.
- The CLI covers everything the UI does.

## MCP: the model

### Agents and files

Prism reads and writes these (verify each format against the agent's current docs while building; paths below are what George's machine shows today):

| Agent | Global | Project | Shape |
| --- | --- | --- | --- |
| Claude Code | `~/.claude.json` → `mcpServers` (user scope); `~/.claude.json` → `projects[<abs path>].mcpServers` (local scope) | `<proj>/.mcp.json` → `mcpServers` | `{type: stdio\|http\|sse, command, args, env, url, headers}` |
| Claude Desktop (macOS) | `~/Library/Application Support/Claude/claude_desktop_config.json` → `mcpServers` | none | stdio only |
| Codex | `$CODEX_HOME/config.toml` → `[mcp_servers.<name>]` | `<proj>/.codex/config.toml` if Codex supports it, else none | `command, args, env, env_vars, url, bearer_token_env_var, enabled, startup_timeout_sec` |
| Cursor | `~/.cursor/mcp.json` → `mcpServers` | `<proj>/.cursor/mcp.json` | no `type`; `url` or `command` |
| OpenCode | `$XDG_CONFIG_HOME/opencode/opencode.json` or `.jsonc` → `mcp` | `<proj>/opencode.json(c)` | `{type: local, command: [..], environment, enabled}` / `{type: remote, url, headers}` |
| Gemini CLI | `~/.gemini/settings.json` → `mcpServers` | `<proj>/.gemini/settings.json` | `command, args, env, cwd, url, httpUrl, headers` |
| Windsurf | `~/.codeium/windsurf/mcp_config.json` → `mcpServers` | none | `serverUrl` for remote |
| Kiro | `~/.kiro/settings/mcp.json` → `mcpServers` | `<proj>/.kiro/settings/mcp.json` | like Cursor |

`CLAUDE_CONFIG_DIR`, `CODEX_HOME` and `XDG_CONFIG_HOME` are respected, as for skills. An agent counts as installed by the same markers Prism uses for skills (Claude Desktop: its config folder exists).

Claude Code's local scope (servers under `projects[path]` in `~/.claude.json`) belongs to that project in Prism's view, next to `.mcp.json`. Where a project has both, Prism shows the scope of each copy. New project servers go to the local scope by default (see "Where new project servers go" below).

### What a scope shows

- **Global:** servers in the agents' global configs.
- **Project:** the project's own servers, plus the global servers that apply there, shown as inherited (dimmed, with "global" on the row). Inherited rows exist so you can disable a global server for one project, which is how George uses `disabledMcpServers` today. Editing an inherited server edits the global copy, and the plan says so.

### One server, many copies

Internally a server is `{name, transport}` where transport is `stdio {command, args, env, cwd}` or `http {url, headers}` (SSE as a flavour of http where an agent distinguishes it). Each agent's copy also keeps the fields Prism doesn't model (`enabled`, `startup_timeout_sec`, anything unknown), and writes them back untouched.

**Environment references** are part of the model: a value can be a literal or a reference to an environment variable. Prism reads each agent's syntax (`${VAR}` in Claude Code and Gemini, `${env:VAR}` in Cursor, `{env:VAR}` in OpenCode, Codex `env_vars = ["VAR"]` for pass-through and `bearer_token_env_var` for an HTTP bearer token) into one form and writes it back in the target agent's syntax. Copies that mean the same thing compare as equal. When a target agent can't express a reference (for example a bearer-token reference in an agent with no header support), the plan marks that agent as skipped with the reason, and Prism doesn't write a broken copy.

Prism groups copies across agents within a scope:

1. Same name → same server.
2. Different names, same URL, or same command and args after normalising the runner (`npx -y X` ≈ `bunx -y X` ≈ `pnpx X`) → a suggested merge, shown in the UI and in `prism mcp list`. Prism doesn't rename anything until the user accepts; accepting renames the odd copies to the chosen name.

A server's status in a scope:
- **In sync**: every copy has the same transport and the same env/header keys and values.
- **Differs**: copies disagree. Prism shows a field-level diff (`env.GITLAB_TOKEN: differs`, `headers.CONTEXT7_API_KEY: missing in Cursor`), with values masked.
- **Disabled** in a project (Claude Code `disabledMcpServers`, OpenCode/Codex `enabled = false`).

### Read-only servers

A copy is read-only when its command points inside an app bundle (`*.app/`) or into an agent's own install folder, or when the agent marks it as its own. Read-only servers show in the list with a lock and the app's name; edit, copy and remove are off for them. The UI has no override; the CLI refuses with a clear message.

### Actions

| Action | What it does | Needs `--apply` in the CLI |
| --- | --- | --- |
| Add | Writes a new server to the picked agents in the scope | no |
| Copy to agents | Writes an existing server to more agents. When copies differ, the user picks the source copy (UI) or passes `--from AGENT` (CLI); copying with a source also overwrites the differing copies, so this is how you resolve drift. The plan lists each overwrite | yes |
| Edit | Changes the given fields in every copy and leaves the other fields as each copy has them. To make all copies identical, use Copy with a source | yes |
| Remove | Removes it from the picked agents (all by default) | yes |
| Disable / enable in a project | Claude Code: `projects[path].disabledMcpServers` (for global and local servers) and `disabledMcpjsonServers` (for `.mcp.json` ones). Other agents: only where a per-project switch exists, such as `enabled = false` on a copy in the project's own config. Never by flipping `enabled` on a global entry, which would disable it everywhere. Where no per-project switch exists (Codex without a project config, Cursor), the action isn't offered for that agent | yes |
| Merge | Renames differently named copies to one name | yes |

### Writing configs safely

- Edit in place, touching only the server entry. JSON: keep key order and the file's indent. JSONC (OpenCode): comments survive. TOML (Codex): use a format-preserving editor (`toml_edit`), so comments and other tables stay as they were.
- Re-read the file right before writing, apply the change to what's there now, write to a temp file in the same folder, then rename. If the file changed between plan and apply, re-plan instead of overwriting.
- Before the first write to a file in a change, copy the original to Prism's own folder (`<config dir>/dev.prism.skills/backups/`, mode 0600, folder 0700) and record it in the journal (new step: `Rewrote { path, backup, hash_after }`). Undo restores the backup only if the file still has `hash_after`; otherwise it reports that the file changed since. Only the last change's backups are kept: starting a new change deletes the previous backups, since undo can't reach them anyway. Backups don't go to the Trash, which would pile up plaintext copies of every token.
- Writing a server that has literal secret values into a project file that git tracks or doesn't ignore (`.mcp.json`, `.cursor/mcp.json`, …) adds a warning to the plan: "this file is in git; the token would be committed".
- Create a missing config file (and folder) only when adding to it.
- Known risk: Claude Code rewrites `~/.claude.json` while it runs, and may write back its in-memory copy after Prism's change. Re-read-before-write narrows the window but can't close it. The builder should check whether a running Claude Code picks up external edits; if it clobbers them, say so in the UI ("restart Claude Code to load it") and the CLI output.

### Where new project servers go (George's decision)

For Claude Code, a server added or copied to a project goes to the local scope by default: `~/.claude.json` → `projects[<abs path>].mcpServers`. It stays on this machine, its tokens never reach the repository, and Claude Code doesn't ask for approval. The UI's Add dialog and the CLI (`--claude-scope project`) can choose `.mcp.json` instead, for servers meant for the whole team. Prism never pre-approves `.mcp.json` servers in Claude Code's approval lists, so an agent can't add and enable a shared server in one step; the plan says Claude Code will ask on its next start.

Other agents have no private per-project file, so their project servers go to the project's own config (`.cursor/mcp.json` and the like), with the git warning above when literal secrets are involved.

## The CLI

One binary. With no arguments, or with a folder, `prism` opens the window as today. A known subcommand as the first argument runs the CLI and never starts the GUI or GPU. The core modules (`agents`, `scan`, `sync`, `fsops`, `remote`, `skill`, `store`, and the new `mcp`) must not depend on `rune`; the CLI calls them directly, and the app calls the same functions.

### Commands

Scope flags on every command: `--project DIR` (default: global). Output: human text, or `--json`.

```
prism agents                              installed agents, which are sync targets
prism targets set AGENT...                default target agents (same setting as the Sync dialog)

prism skills list                         skills in the scope, per agent: link / copy / broken / missing
prism skills show NAME                    SKILL.md front matter, path, per-agent placement
prism skills new NAME --description TEXT
prism skills sync [NAME...] [--to AGENT...] [--apply]
prism skills link NAME --agent AGENT [--apply]
prism skills unlink NAME --agent AGENT [--apply]
prism skills remove NAME... [--apply]
prism skills copy NAME... (--to-project DIR | --to-global) [--apply]
prism skills clean-broken [--apply]
prism skills search QUERY                 skills.sh
prism skills repo SOURCE                  list a repo's skills
prism skills install SOURCE[@SKILL] [--skill NAME...] [--to AGENT...]
prism skills updates                      skills with a newer version
prism skills update [NAME...]

prism mcp list                            servers in the scope, per agent, with status and merge suggestions
prism mcp show NAME [--show-secrets]
prism mcp add NAME (--command CMD [--arg A]... [--env K=V]... | --url URL [--header K=V]...) --to AGENT... [--claude-scope local|project]
prism mcp copy NAME --to AGENT... [--from AGENT] [--apply]
prism mcp edit NAME [--command ...] [--arg ...] [--env K=V] [--unset-env K] [--url ...] [--header K=V] [--apply]
prism mcp remove NAME [--from AGENT...] [--apply]
prism mcp disable NAME | enable NAME      (--project required) [--apply]
prism mcp merge NAME --into NEW_NAME [--apply]

prism projects list | add DIR | remove DIR | find DIR
prism undo                                reverts the last change made by the window or the CLI
prism doctor                              broken links, invalid SKILL.md, differing MCP servers, unreadable configs
prism setup                               put `prism` on PATH and install the prism skill (see below)
```

`--env K=V` takes the value literally; `--env K=@file` and `--env K=-` (stdin) keep tokens out of shell history and agent transcripts. The skill tells agents to prefer those.

### Behaviour

- Never prompts. Missing input is an error with the flag that's needed.
- Without `--apply`, a changing command prints the plan (the same `Plan` the UI shows) and exits 0 with `"applied": false` and a `"plan_id"` in JSON: a hash of the planned actions and of the files they touch. With `--apply` it plans again, acts and prints what it did. `--apply --plan-id ID` refuses with exit 4 when the new plan's id differs, so an agent applies exactly the plan it showed; the skill tells agents to always pass it.
- Exit codes: 0 success or nothing to do, 1 the operation failed (partially applied changes are reported, and `prism undo` reverts them), 2 bad usage, 3 `doctor` found problems, 4 the plan changed since `--plan-id` was issued.
- `--json` output is stable and documented in the skill. Env and header values come back as `"***"` unless `--show-secrets`.
- `prism undo` and ⌘Z share one last change. The journal of the last change is saved next to `settings.json` (`last-change.json`); the window loads it, the CLI replaces it, and the window picks up the new one through its file watcher.
- The window watches MCP config files and `last-change.json` as well as the skill folders, so CLI changes show up on their own.

### `prism setup` and the prism skill

- macOS: symlinks `~/.local/bin/prism` (or `/usr/local/bin/prism` if `~/.local/bin` isn't on PATH and is writable) to the binary inside `Prism.app`. The app menu gets "Install command line tool…" doing the same.
- Linux: the tarball installer already puts `prism` in `~/.local/bin`; `setup` checks and reports.
- Installs the `prism` skill into the global `.agents/skills` and links it into the target agents. The skill ships inside the binary (`include_str!`), describes when to reach for Prism (installing or syncing skills, adding an MCP server to several agents, checking setup drift), the commands, the plan-then-`--apply` rule, `--json` shapes, and how to pass secrets via `@file` / stdin. `prism setup` updates it when the binary's version is newer.

## The UI

A new page, "MCP servers", next to the skills list for the selected scope (global or project), following the existing patterns:

- **List:** one row per server with agent chips (like the skills agent column), status (in sync / differs / disabled / read-only), and filters by status. Merge suggestions appear as a row-level hint.
- **Inspector:** transport, command or URL, env and header keys with masked values (click to reveal), and per-agent copies with a diff when they differ.
- **Add server dialog:** name, stdio or HTTP, command + args or URL, env / header pairs, target agents (checkboxes, defaulting to the sync targets).
- **Copy, edit, remove, disable, merge** open the same plan-first dialog the skills sync uses.
- Undo and notices work as for skills.

Exact layout is the builder's call, matching the current screens and the headless scene setup (add scenes for the new page and dialogs, plus README screenshots from `demo-home.sh`).

## Constraints

- Rust 1.99 (pinned), macOS and Linux, `cargo clippy --all-targets -- -D warnings` clean, CI green on the three runners.
- Tests stay headless on a throwaway `$HOME` (`Paths::isolated`). Each agent format gets round-trip tests from fixtures: read, change one server, write, and compare against the expected file byte for byte, including unknown keys and comments.
- No network in default tests; skills.sh calls stay behind `--ignored`.
- New dependencies are fine where they save real work (argument parsing, `toml_edit`, a JSONC editor that keeps comments). Keep the binary small.
- Code style as in the repo: short doc comments saying why, small modules, no UI types in the core.

## Left to the builder

- Module layout (suggested: `src/mcp/` with `model`, one file per agent format, `scan`, `plan`; `src/cli/`), argument parser, and the JSON schema details.
- How to detect app-managed servers beyond the `.app` rule, and the exact normalisation for matching copies.
- Whether Codex reads a project `.codex/config.toml`; if not, Codex is global-only for MCP.
- The order of work. Suggested: core MCP read model and `prism mcp list` first (read-only, safe to try on a real machine), then writers with round-trip tests, then the rest of the CLI, the persistent journal, the UI page, `setup` and the skill.
