# Prism: skills and MCP servers on other machines

Status: ready for approval · 2026-11-10

## Why

George works on more than one machine. Besides the Mac, there is a laptop at home running Linux
in WSL, reached over a tailnet; coding agents run there through t3code. Setting that laptop up
meant redoing by hand what Prism already does here: adding the mobbin MCP server to the agents'
configs and putting the emil design skills in place. Prism only sees the machine it runs on.

George wants both: to open another machine in Prism and work with it as with this one, and to
send a skill or a server from one machine to another. "Switch between environments."

## Goals

- **Environments.** Prism shows one machine at a time: this one, or a host reached over SSH.
  Switching changes the whole window: scopes, projects, skills, MCP servers, sync, doctor and
  undo all act on that machine.
- **Copy across.** "Copy to…" for skills and MCP servers offers other environments as
  destinations, with the same plan-then-apply flow as copying within one machine.
- **CLI.** Every command takes `--host NAME`; copying across machines works from the CLI, so an
  agent can do "put mobbin on the laptop".
- **Same safety on the far side.** Plans, the file-hash check before writing, backups, atomic
  writes, undo and secret masking hold on the remote machine exactly as they do locally.

## Non-goals (v1)

- Windows-side apps. On the laptop George uses only agents inside WSL; configs under
  `/mnt/c/Users/…` (Cursor or Claude Desktop installed on Windows) are not read.
- Windows or macOS-on-Intel as a remote. Remotes are Linux x86_64, Linux arm64 or macOS arm64,
  the platforms Prism already ships.
- Password or interactive SSH logins. Key-based or Tailscale SSH only; Prism never types a
  password.
- Continuous sync between machines, or a "these two machines differ" view. Copy is explicit.
- Live watching of remote files. A remote environment refreshes when opened, on ⌘R and after
  each change Prism makes.
- skills.sh installs on a remote (Browse stays on this machine; install here, then copy across).
  This one is a guess; it can move into v1 if George wants it.

## Anti-goals

- **Writing on a machine the user didn't pick.** Every write names the machine in its plan and
  in its toast ("on laptop"). An action never targets the remote because it was the last one
  opened.
- **A second implementation of the config logic.** The remote does its own reading and writing
  with the same core code; the Mac never edits a remote config by streaming file contents.
- **Secrets in new places.** No token lands in Prism's settings, in the SSH command line, in
  `ps` output or in a log. Masking rules apply to everything that comes back from a remote.
- **Hanging the window.** An unreachable or slow host shows a clear state within seconds; the
  window stays usable.

## Decisions made with George

- Both directions matter: opening a remote as an environment and copying to it.
- The laptop is reached over SSH (tailnet); `ssh` works from George's personal Mac. It does
  **not** work from the Mac this design was written on, so nothing here may depend on reaching
  that laptop during development.
- Only agents inside WSL count.
- `ssh laptop` lands straight in the WSL shell (`uname` says Linux), so a remote command needs no
  `wsl.exe` wrapper. Hosts whose SSH lands in Windows are out of scope for v1.
- The how below (Prism drives a `prism` binary on the remote) was played back and accepted.

## How it works

### The remote runs `prism`

Prism on the remote is the same program, driven over SSH with `--json`. The CLI already covers
everything the window does, with JSON output, plan ids, exit codes and masking. A remote
environment is therefore: `ssh <target> prism --json <command…>`, parsed on the Mac.

- **SSH.** The system `ssh` binary, so `~/.ssh/config`, agent forwarding and Tailscale SSH all
  work as in a terminal. Always non-interactive (`BatchMode=yes`) with a connect timeout
  (`ConnectTimeout=5`). Connection reuse (`ControlMaster`/`ControlPersist`) so one environment's
  many calls share one connection.
- **Secrets go over stdin, never argv.** The CLI already reads `K=-` values from stdin; anything
  carrying a secret (an MCP server's env, headers, args) travels the same way.
- **Version lock.** The remote `prism` must be the same version as the local one (the JSON is a
  contract between them). On connect Prism runs `prism --version` remotely; a missing or
  different version offers "Install prism X.Y.Z on laptop".
- **A headless `prism` for remotes (my call).** Today's Linux binary is the GUI: rune links
  `libxkbcommon` (and wayland, dbus) directly, so a minimal headless WSL may not start it. The
  build gains a cargo feature that leaves the window out (`--no-default-features` gives a
  CLI-only binary with no GUI libraries). Each release adds CLI-only archives
  `prism-cli-X.Y.Z-<os>-<arch>.tar.gz` for Linux x86_64, Linux arm64 and macOS arm64, plus a
  `SHA256SUMS` file. This changes what ships; the desktop downloads stay as they are.
- **Installing.** The remote downloads the CLI-only archive for its own OS and architecture
  (`uname -sm` picks it) into `~/.local/bin/prism`, verified against `SHA256SUMS`. When the
  remote can't download, or this is a development build with no release, Prism uploads a binary
  the user points it at (Settings → a file path). Prism never runs `sudo`.

### Environments in the window

- A switcher at the top of the sidebar: "This Mac" plus each saved host. The rest of the sidebar
  (Global, Projects) and every page follow the chosen environment.
- **Add host…** asks for an SSH target (a `~/.ssh/config` alias or `user@host`) and a display
  name, then connects, checks the version and offers the install. Hosts live in Prism's
  settings as `{ name, ssh }` only.
- Opening a remote shows a loading state, then its data. Unreachable: an empty state with the
  SSH error's last line and Retry. The previous environment's data is never shown under the new
  name.
- Everything that changes things (sync, link, copy, MCP add/edit/remove/merge/disable, projects,
  targets) runs on the chosen machine through its CLI, with the same dialogs and plans.
- **Projects on a remote** are typed paths (no native folder picker for a remote disk). The
  remote's own settings hold its project list and sync targets.
- **Undo** is per machine (my call): each machine keeps its own last change on its own disk.
  ⌘Z undoes the last change of the environment in view. A toast's Undo button acts on the
  machine the toast names, so after copying to the laptop from the Mac view, the toast's Undo
  takes back the laptop change while ⌘Z still means the Mac's. A copy across machines is one
  change in the target's journal (staged folder, rename and links together), so `prism undo` on
  the target reverses all of it.
- **Show values** on a remote calls `prism mcp show NAME --show-secrets` on demand for that one
  server, and the values are dropped when hidden again.
- Browse (skills.sh) and the "Install Command Line Tool…" menu item stay about this Mac.

### Copying across machines

"Copy to…" lists, after the scopes of this machine, the scopes of every saved host (Global and
its projects). Picking one on another machine:

- **A skill:** the folder is streamed as a tar archive over SSH into the target's
  `.agents/skills/NAME` (a staging folder first, renamed into place), then linked into the
  target's sync targets by the target's `prism`. An existing skill of that name is a conflict
  the plan shows; nothing is overwritten without the user choosing it.
- **An MCP server:** the source machine's definition (values as written) goes to the target's
  `prism mcp add` for the agents the user ticks there. The agent list in the dialog is the
  target's installed agents.
- The plan comes from the target ("on laptop: add mobbin to Claude Code, Codex") and applies
  with its plan id, as now.
- **Values that may not travel (my call).** The plan flags, as heads-up lines, values that
  look tied to the source machine: absolute paths under the source's home or `/opt/homebrew`,
  and `${VAR}` references (the variable must exist on the target). The copy still writes values
  as they are; the user fixes them afterwards with Edit… in the target environment.
- The source can be any environment and the target any other, including remote to remote.

### CLI

- `--host NAME` on every command runs it on that host (NAME from Prism's saved hosts, or an
  SSH target). Output is the remote's, unchanged.
- Copy across: `prism skills copy NAME --to-host laptop [--project DIR]` and
  `prism mcp copy NAME --to-host laptop --to AGENTS`, with the usual plan/`--apply --plan-id`.
- `prism hosts list|add|remove|check|install` manages the saved hosts and the remote binary.
- The prism skill (`assets/prism-skill/SKILL.md`) learns `--host`.

## Constraints

- Rust, rune, rune-kit, the existing core modules; no runtime dependency on the remote besides
  `tar`, `uname` and a POSIX shell. The release workflow gains the CLI-only archives and
  `SHA256SUMS`.
- `cargo test` and `cargo clippy --all-targets` stay green on macOS and Linux. Tests must not
  need a real remote: the SSH layer is a trait with a local runner that executes the command
  with a different `HOME`, so a "remote" in tests is a temp folder. One ignored end-to-end test
  may use a local SSH server (for example in a container).
- UI follows `DESIGN.md`.

## Left to the builder

- Exact command shapes added to the CLI for the transfer (for example a hidden
  `skills receive` reading a tar on stdin, and how a server definition is passed in full).
- How the window's state for a remote is built from the CLI's JSON, and what JSON fields the
  existing commands must gain to make that possible.
- Where the settings for hosts live and how a saved host is shown when its machine is asleep.
- Timeouts beyond the connect timeout, and what "slow" looks like in the UI.
