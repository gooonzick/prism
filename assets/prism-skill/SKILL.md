---
name: prism
description: Use to install, sync, link or remove agent skills across coding agents, to add, copy, edit or remove an MCP server in several agents' configs, or to check a setup for broken skill links and MCP servers that differ between agents. Drives the prism command line.
metadata:
  prism-version: "{{version}}"
---

# prism

`prism` manages agent skills and MCP servers for Claude Code, Codex, Cursor, OpenCode, Gemini CLI and other agents. A skill lives once in `.agents/skills` and each agent's skills folder links to it. MCP servers stay in each agent's own config file; `prism` edits them in place and leaves the rest of the file as it was.

## When to use

- The user wants a skill in more than one agent, or asks which agents have a skill.
- The user wants an MCP server added to, copied to, changed in or removed from agents.
- A skill or server works in one agent and not in another: run `prism doctor --json`.

Use `prism` instead of editing `~/.claude.json`, `~/.cursor/mcp.json`, `~/.codex/config.toml` or skill symlinks by hand.

## Rules

1. Add `--json` to every command and read stdout.
2. Commands that change or remove something print a plan and change nothing. Read the plan, tell the user what it does, then run the same command with `--apply --plan-id <plan_id>`. Exit 4 means the files changed since the plan: read the new plan and confirm again.
3. `mcp add`, `skills install` and `skills new` act at once, without a plan. Confirm with the user first.
4. Pass secrets with `KEY=@file` (read from a file) or `KEY=-` (read from stdin), never inline. `'KEY=${VAR}'` writes a reference the agent expands. Never print or echo secret values.
5. Env and header values print as `***`. Run `prism mcp show NAME --show-secrets` only when the user asks to see them.
6. `prism undo` reverts the last change, made from the command line or the Prism window.
7. Commands work on the global setup (the home folder). `--project DIR` works on that project instead.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Done, or nothing to do |
| 1 | Failed. JSON has `done` and `error`; `prism undo` reverts what was done |
| 2 | Bad usage: a flag is missing or wrong |
| 3 | `doctor` found problems |
| 4 | The plan changed since `--plan-id`; nothing was done |

## Commands

Skill agent ids: `claude codex opencode cursor gemini copilot windsurf goose kiro roo`.
MCP agent ids: `claude claude-desktop codex cursor opencode gemini windsurf kiro`.

```bash
prism agents --json                          # installed agents, which are sync targets
prism targets set claude codex cursor        # the agents a sync links into
prism projects list --json                   # also: add DIR, remove DIR, find DIR
prism doctor --all --json                    # global setup and every listed project
prism undo --json
prism setup --json                           # put prism on PATH, install this skill

prism skills list --json
prism skills show pdf --json
prism skills sync --json                     # every skill into the targets
prism skills sync pdf --to codex --json
prism skills link pdf --agent cursor --json
prism skills unlink pdf --agent cursor --json
prism skills remove pdf --json               # links go, folders go to the trash
prism skills copy pdf --to-project ~/code/app --json   # or --to-global
prism skills clean-broken --json
prism skills search react --json             # skills.sh
prism skills repo vercel-labs/agent-skills --json
prism skills install vercel-labs/agent-skills --skill web-design-guidelines --json
prism skills updates --json
prism skills update pdf --json
prism skills new release-notes --description "Use when writing release notes" --json

prism mcp list --json
prism mcp show github --json
prism mcp add github --command npx --arg -y --arg @modelcontextprotocol/server-github \
  --env GITHUB_TOKEN=@token.txt --to claude cursor codex --json
prism mcp add context7 --url https://mcp.context7.com/mcp \
  --header CONTEXT7_API_KEY=- --to claude cursor --json < key.txt
prism mcp copy context7 --to codex --from claude --json
prism mcp edit github --env GITHUB_TOKEN=@token.txt --unset-env DEBUG --json
prism mcp remove github --from cursor --json
prism mcp disable github --project ~/code/app --json   # enable turns it back on
prism mcp merge figma figma-dev --into figma --json
```

In a project, `mcp add --to claude` writes Claude Code's local scope (`~/.claude.json`); `--claude-scope project` writes `.mcp.json`. `mcp copy` needs `--from` when the copies differ.

## JSON

Plan, from any command run without `--apply`:

```json
{ "applied": false, "plan_id": "f7128198c881",
  "actions": ["Link pdf into Codex"],
  "conflicts": [{ "skill": "pdf", "reason": "Cursor has its own, different copy" }] }
```

MCP plans have `"skipped": [{ "agent": "codex", "reason": "..." }]` and `"warnings": ["..."]` instead of `conflicts`. With `--apply`: `"applied": true, "done": 2`. On failure: `"applied": false, "done": 1, "error": "..."`. After exit 4: `"plan_changed": true` and the new plan.

`skills list` (an array) and `skills show`:

```json
[{ "name": "pdf", "description": "...", "synced": false,
   "placements": { "agents": "dir", "claude": "linked", "codex": "missing", "cursor": "dir" } }]
{ "name": "pdf", "description": "...", "path": "/Users/me/.agents/skills/pdf", "body_lines": 40, "placements": {} }
```

Placements: `dir` (a folder), `linked` (a link to `.agents`), `link` (a link elsewhere), `broken`, `missing`.

`mcp list`:

```json
{ "servers": [{ "name": "context7", "status": "differs", "agents": ["claude", "cursor"],
                "transport": "http", "inherited": false, "disabled_for": [] }],
  "merges": [{ "names": ["figma", "figma-dev"], "reason": "same package figma-developer-mcp" }],
  "errors": [{ "file": "~/.codex/config.toml", "error": "..." }] }
```

Status: `in_sync`, `differs`, `read_only` (an app manages it; `prism` won't change it). `inherited`: a global server seen from a project. A merge hint names servers that are one server under several names: fold them with `mcp merge`.

`mcp show`:

```json
{ "name": "context7",
  "copies": [{ "agent": "claude", "file": "~/.claude.json", "place": "global", "enabled": true,
               "read_only": null, "extra_keys": [],
               "transport": { "type": "http", "url": "https://mcp.context7.com/mcp", "headers": { "KEY": "***" }, "sse": false } }],
  "diffs": [{ "field": "headers.KEY", "values": [{ "agent": "claude", "value": "***" }, { "agent": "cursor", "value": null }] }] }
```

A stdio transport is `{ "type": "stdio", "command": "npx", "args": [], "env": {}, "cwd": null }`. `place` is `global`, `local` (Claude Code's per-project entry in `~/.claude.json`) or `project`.

`doctor` (exit 3 when the list isn't empty):

```json
{ "problems": [{ "kind": "mcp_differs", "scope": "global", "subject": "context7", "detail": "headers.KEY" }] }
```

Kinds: `broken_link`, `invalid_skill`, `mcp_differs`, `mcp_unreadable`, `missing_project`.

Other results: `skills new` `{ "created", "path", "linked" }`, `skills install` `{ "installed", "linked", "log" }`, `skills updates` `{ "updates" }`, `skills update` `{ "updated", "log" }`, `undo` `{ "undone" }` (the change's name, or `null`), `setup` `{ "cli", "skill" }`, each with `status` and `message`. A skills.sh failure is `{ "error" }`.
