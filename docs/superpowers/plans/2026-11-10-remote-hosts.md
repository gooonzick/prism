# Remote Hosts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prism opens another machine over SSH as an environment and copies skills and MCP servers between machines, from the window and the CLI.

**Architecture:** The remote runs the same `prism` (a CLI-only build) driven over the system `ssh` binary. The CLI gains `--host`, a hidden `wire` command group that dumps the window's whole state as JSON, and receive commands for copies across machines. The window keeps one set of data fields (`inv`, `mcp`, `paths`, `settings`) and fills them from the local disk or from the remote's `wire state`; actions in a remote environment become CLI calls through a `Runner`.

**Tech Stack:** Rust 1.99, clap 4, serde/serde_json, rune + rune-kit (rev 7ead7d6, now optional), system `ssh` and `tar`.

**Spec:** `docs/superpowers/specs/2026-11-10-remote-hosts-design.md`

## Global Constraints

- Remotes: Linux x86_64, Linux arm64, macOS arm64. `ssh` lands in a POSIX shell (WSL counts; a Windows shell does not).
- SSH is always non-interactive: `-o BatchMode=yes -o ConnectTimeout=5`, connection reuse with `ControlMaster=auto`, `ControlPersist=60`. Prism never types a password and never runs `sudo`.
- Secrets never go in argv, settings, logs or `ps`: they travel over stdin. Everything a remote returns for display is masked exactly as local output is.
- Local and remote `prism` must report the same `--version`; anything else is "needs install", never a best-effort call.
- The remote binary lives at `~/.local/bin/prism`; remote commands run as `PATH="$HOME/.local/bin:$PATH" prism …`.
- Every write names its machine: plan lines and toasts end with "on <host name>" when the machine is not this one. This machine is "This Mac" on macOS and "This computer" elsewhere.
- Undo is per machine: ⌘Z acts on the environment in view; a toast's Undo acts on the machine the toast names.
- Tests never need a real host: `Local` runner (Task 3) stands in, with a temp `HOME`. `cargo test` and `cargo clippy --all-targets` stay green with default features and with `--no-default-features`.
- UI follows `DESIGN.md` (type ladder, one `Contrast` per view, closed status words, copy rules).

## Review Focus

1. Host unreachable or asleep: the window shows "Can't reach laptop" with ssh's last stderr line and Retry within ~6 s, never a frozen window (Task 10 test `unreachable_host_shows_retry`).
2. A remote `prism` of another version, or none: no command runs against it except `--version`; the user is offered the install (Task 4 test `version_mismatch_refuses_commands`).
3. Arguments with spaces, quotes, `$` and `;` survive ssh's re-parsing byte for byte (Task 3 test `quote_round_trips_hostile_args`).
4. A skill folder with nested files, a symlink inside and a dotfile arrives identical, and a name clash on the target is a conflict, not an overwrite (Task 7 test `receive_keeps_tree_and_refuses_clash`).
5. Secrets: `wire state` and `--host` output carry no plaintext env/header/arg token, and the ssh argv never contains one (Task 6 `wire_state_masks_everything`, Task 8 `secret_never_in_argv`).

---

### Task 1: A CLI-only build

**Files:**
- Modify: `Cargo.toml`, `src/main.rs`, `src/app/watch.rs` (test gate only), `.github/workflows/ci.yml`

**Interfaces:**
- Produces: cargo feature `gui` (default on). `cargo build --release --no-default-features` gives a `prism` with no rune, rune-kit or notify linked; it runs every CLI command and, with no CLI arguments, prints the top-level help and exits 2.

- [ ] **Step 1:** In `Cargo.toml` add `[features] default = ["gui"]`, `gui = ["dep:rune", "dep:rune-kit", "dep:notify"]`, and mark `rune`, `rune-kit`, `notify` `optional = true`.
- [ ] **Step 2:** Gate with `#[cfg(feature = "gui")]`: `mod app;`, `mod ui;`, `mod ui_tests;`, and in `src/main.rs` every rune import, `init`, `setup`, `poll`, `NOTICE_SECS`, `Scene`/`SCENES`, `install_cli_tool` and the window half of `main()`. Keep `mod remote`, `mod cli` and the rest ungated. Without `gui`, `main()` calls `cli::run`; when that returns `None` it runs `cli::run(vec!["--help".into()])` and exits 2.
- [ ] **Step 3:** Run `cargo build --no-default-features && otool -L target/debug/prism` (macOS) — Expected: no `xkbcommon`, `wayland` or `dbus` and no rune symbols; `cargo test --no-default-features` passes (window tests excluded by the gate).
- [ ] **Step 4:** In `.github/workflows/ci.yml` add to each job a step `cargo clippy --no-default-features --all-targets -- -D warnings` and `cargo test --no-default-features`, run before the GUI libraries are installed on Linux (so a stray rune dependency fails CI).
- [ ] **Step 5:** Commit `feat: a CLI-only build without the window (--no-default-features)`.

### Task 2: Saved hosts and `prism hosts`

**Files:**
- Create: `src/hosts.rs`, `src/cli/hosts.rs`
- Modify: `src/store.rs`, `src/cli/mod.rs`, `src/main.rs`, `src/cli/tests.rs`

**Interfaces:**
- Produces:
  - `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] pub struct Host { pub name: String, pub ssh: String }` in `src/hosts.rs`.
  - `Settings.hosts: Vec<Host>` with `#[serde(default)]`.
  - `pub fn find<'a>(hosts: &'a [Host], name: &str) -> Option<&'a Host>`; `pub fn valid_name(name: &str) -> Result<(), String>` (1–32 chars of `[A-Za-z0-9._-]`; "local" and "this" are reserved).
  - CLI `prism hosts list | add NAME --ssh TARGET | remove NAME` (immediate, no plan; they only touch Prism's own settings). JSON: `list` → `[{name, ssh}]`, `add` → `{name, ssh, added}`, `remove` → `{name, removed}`.
  - `"hosts"` added to `SUBCOMMANDS`.

- [ ] **Step 1: Tests** in `src/cli/tests.rs`: `hosts_add_list_remove` (add `laptop --ssh george@laptop`, list shows it, remove, list empty; settings file round-trips); `hosts_add_rejects_bad_names` (`"bad name"`, `"local"`, a 33-char name → exit 2, settings unchanged); `hosts_add_twice_is_usage_error` (same name → exit 2).
- [ ] **Step 2:** Run `cargo test hosts_` — Expected: fail (no `hosts` command).
- [ ] **Step 3:** Implement the module, the settings field and `cli/hosts.rs` in the style of `cli/projects.rs`.
- [ ] **Step 4:** Run `cargo test hosts_` — Expected: pass; `cargo test` all green.
- [ ] **Step 5:** Commit `feat: saved hosts and prism hosts list/add/remove`.

### Task 3: The runner: ssh and a local stand-in

**Files:**
- Create: `src/hosts/runner.rs` (turn `src/hosts.rs` into `src/hosts/mod.rs`)
- Test: in the same file

**Interfaces:**
- Produces:
  - `pub struct Output { pub code: i32, pub stdout: Vec<u8>, pub stderr: String }`
  - `pub trait Runner: Send + Sync { fn prism(&self, args: &[String], stdin: Option<Vec<u8>>) -> Result<Output, String>; fn shell(&self, script: &str, stdin: Option<Vec<u8>>) -> Result<Output, String>; }` — `Err` only when the machine can't be reached (ssh exit 255 or spawn failure), carrying "Can't reach {name}: {last non-empty stderr line}".
  - `pub fn quote(arg: &str) -> String` — POSIX single-quoting (`'` → `'\''`).
  - `pub struct Ssh { pub name: String, pub target: String }` with `pub fn argv(&self, remote_command: &str) -> Vec<String>` and `impl Runner`. `argv` is exactly `["ssh", "-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ControlMaster=auto", "-o", "ControlPath=<control dir>/%C", "-o", "ControlPersist=60", target, "--", remote_command]`. The control dir is `/tmp/prism-ssh-<uid>` created 0700 (macOS limits a socket path to 104 bytes; `%C` alone is 40).
  - `prism()` runs `PATH="$HOME/.local/bin:$PATH" prism` followed by each arg `quote`d.
  - `#[cfg(test)] pub struct Local { pub name: String, pub home: PathBuf }`: `prism()` calls `cli::run_with` in process with `Ctx { paths: Paths::isolated(&home), settings: Settings::load_from(..), stdin: <given>, path_var: None }`; `shell()` runs `sh -c script` with `HOME=home`.
  - `pub struct Here;` — this machine as a `Runner`: `prism()` spawns `std::env::current_exe()` with the args, `shell()` runs `sh -c`. Copies (Task 8) treat source and target alike through it.
  - `pub fn runner_for(host: &Host) -> Box<dyn Runner>` (the `Ssh` one).
  - `pub type MakeRunner = Arc<dyn Fn(&Host) -> Box<dyn Runner> + Send + Sync>`: how the CLI (`Ctx`) and the window (`App`) get a runner, so tests can hand out `Local` runners per host name.

- [ ] **Step 1: Tests:** `quote_round_trips_hostile_args` (for each of `a b`, `it's`, `$HOME`, `a;rm -rf x`, `` `x` ``, `"q"`, empty string: `sh -c "printf %s <quote(arg)>"` prints the arg exactly); `ssh_argv_is_non_interactive` (argv equals the list above for target `george@laptop`, command `true`); `secret_never_in_argv` (a `Runner` call with stdin `FIXTURE_SECRET` produces an argv without it); `local_runner_runs_cli` (`Local.prism(["agents","--json"])` returns code 0 and JSON).
- [ ] **Step 2:** Run `cargo test hosts::runner` — Expected: fail.
- [ ] **Step 3:** Implement. `Ssh::prism`/`shell` spawn `ssh` with `std::process::Command`, write stdin then close it, read stdout/stderr to the end; exit 255 → `Err`.
- [ ] **Step 4:** Run `cargo test hosts::runner` — Expected: pass.
- [ ] **Step 5:** Commit `feat: run prism on a host over ssh, with a local stand-in for tests`.

### Task 4: Check, install and `--host`

**Files:**
- Create: `src/hosts/install.rs`
- Modify: `src/cli/hosts.rs`, `src/cli/mod.rs`, `src/cli/tests.rs`

**Interfaces:**
- Consumes: `Runner`, `Host`, `quote` (Task 3).
- Produces:
  - `pub enum Health { Ready, Missing, OtherVersion(String), Unsupported(String) }` and `pub fn check(r: &dyn Runner) -> Result<(Health, Platform), String>` — one `shell` call: `uname -sm; PATH="$HOME/.local/bin:$PATH" prism --version 2>/dev/null`.
  - `pub struct Platform { pub os: String, pub arch: String }` from `uname -sm`: `Linux x86_64` → (`linux`, `x86_64`), `Linux aarch64` → (`linux`, `arm64`), `Darwin arm64` → (`macos`, `arm64`); anything else → `Unsupported`.
  - `pub fn cli_archive(version: &str, p: &Platform) -> String` = `prism-cli-{version}-{os}-{arch}.tar.gz`; release base URL `https://github.com/gooonzick/prism/releases/download/v{version}/`.
  - `pub fn install(r: &dyn Runner, version: &str, p: &Platform, binary: Option<&Path>) -> Result<(), String>`: without `binary`, one `shell` script that downloads the archive and `SHA256SUMS` with `curl -fsSL` (falling back to `wget -qO`), checks the sum with `sha256sum -c` or `shasum -a 256 -c`, unpacks `prism` to `~/.local/bin/prism.new`, `chmod 755`, renames over `~/.local/bin/prism`. With `binary`, the file goes over stdin to `cat > ~/.local/bin/prism.new` and the same rename. A failed checksum leaves the old binary in place.
  - CLI `prism hosts check NAME` → `{name, reachable, health: "ready"|"missing"|"other_version"|"unsupported", remote_version, os, arch}`; `prism hosts install NAME [--binary PATH]`.
  - Global flag `--host NAME` (clap `Global`, `global = true`; `is_cli` skips `--host X` and `--host=X`). With it, `execute` resolves NAME via `hosts::find` (else uses NAME as a raw ssh target), runs `check`, refuses with exit 1 and "prism on {name} is {state}; run prism hosts install {name}" unless `Ready`, and otherwise forwards the original argv minus `--host` through `Runner::prism`, forwarding this process's stdin only when an arg is `-` or ends with `=-`. It writes the remote stdout/stderr as is and exits with the remote code; a reach error exits 1.

- [ ] **Step 1: Tests** (with `Local` and a fake `Runner` that returns canned `Output`s): `platform_from_uname` (the three mappings and one `Unsupported`); `cli_archive_names` (`prism-cli-0.2.0-linux-arm64.tar.gz`); `version_mismatch_refuses_commands` (fake answers `prism 0.1.0` while local is `CARGO_PKG_VERSION` → `--host x skills list` exits 1, the fake saw only the check script); `host_flag_forwards_and_keeps_exit_code` (a `Local` remote with a broken link → `--host x doctor --json` exits 3 with the remote's JSON); `install_script_never_uses_sudo` (the script text contains no `sudo` and verifies before the rename).
- [ ] **Step 2:** Run `cargo test hosts::install host_flag` — Expected: fail.
- [ ] **Step 3:** Implement. `Ctx` gains `make_runner: MakeRunner` (production: `Arc::new(|h| runner_for(h))`; tests: `Local` runners keyed by host name).
- [ ] **Step 4:** Run the tests — Expected: pass.
- [ ] **Step 5:** Commit `feat: prism --host, hosts check and install`.

### Task 5: Release the CLI-only archives

**Files:**
- Create: `scripts/package-cli.sh`
- Modify: `.github/workflows/release.yml`, `README.md`

**Interfaces:**
- Produces: release assets `prism-cli-X.Y.Z-linux-x86_64.tar.gz`, `prism-cli-X.Y.Z-linux-arm64.tar.gz`, `prism-cli-X.Y.Z-macos-arm64.tar.gz` (each holding one file, `prism`) and `SHA256SUMS` covering every asset. Names match `cli_archive` (Task 4).

- [ ] **Step 1:** `scripts/package-cli.sh` builds `cargo build --release --no-default-features`, maps `uname -m` (`aarch64` → `arm64`) and `uname -s` (`Darwin` → `macos`, `Linux` → `linux`), writes `dist/prism-cli-$VERSION-$os-$arch.tar.gz`.
- [ ] **Step 2:** In `release.yml` run it in the `linux` matrix (before installing GUI libraries is not required here) and the `macos` job; in `release`, run `cd dist && sha256sum * > SHA256SUMS` before `gh release create`.
- [ ] **Step 3:** Run `./scripts/package-cli.sh && tar -tzf dist/prism-cli-*-macos-arm64.tar.gz` — Expected: `prism`. Run the binary inside with `agents --json` — Expected: exit 0.
- [ ] **Step 4:** README "Command line" gets a "Other machines" paragraph naming the archives and `prism hosts install`.
- [ ] **Step 5:** Commit `build: release CLI-only archives and SHA256SUMS`.

### Task 6: `prism wire`: the window's state as JSON

**Files:**
- Create: `src/wire.rs`, `src/cli/wire.rs`
- Modify: `src/scan.rs`, `src/agents.rs`, `src/mcp/model.rs`, `src/mcp/scan.rs`, `src/app/mod.rs` (move `ScopeStats`), `src/cli/mod.rs`, `src/cli/tests.rs`

**Interfaces:**
- Produces:
  - `Serialize, Deserialize` on `Paths`, `Scope`, `Placement`, `Skill`, `Inventory`, `EnvValue`, `Part`, `Transport`, `Server`, `Place`, `McpCopy`, `Status`, `FieldDiff`, `McpServer`, `McpInventory` (and whatever they contain). `Extras` serialize as their key names only.
  - `ScopeStats` and `stats_of(inv: &Inventory, targets: &[usize]) -> ScopeStats` move to `src/scan.rs` (the window imports them from there).
  - `pub fn masked(inv: &McpInventory) -> McpInventory` in `src/mcp/scan.rs`: every `EnvValue` → its `masked()` form as a literal, args → `masked_args`, url/command/cwd → `masked_url`; `FieldDiff.written` → the masked values.
  - `#[derive(Serialize, Deserialize)] pub struct WireState { pub version: String, pub paths: Paths, pub settings: Settings, pub scope: Scope, pub inv: Inventory, pub mcp: McpInventory, pub stats: Vec<(Scope, ScopeStats)>, pub last_change: Option<String>, pub detected: Vec<bool>, pub mcp_detected: Vec<bool> }` in `src/wire.rs`, with `settings.hosts` emptied and `mcp` masked; `pub fn state(scope: &Scope, paths: &Paths, settings: &Settings) -> WireState`.
  - Hidden CLI group `prism wire` (`#[command(hide = true)]`): `wire state [--project DIR]` → `WireState`; `wire server NAME [--project DIR]` → the unmasked `McpServer` (for Show values and copy sources); `wire skill-md NAME [--project DIR]` → `{body}` (the SKILL.md body the inspector previews); `wire skill-hash NAME [--project DIR]` → `{hash}` (`fsops::tree_hash` of its folder, for copies).

- [ ] **Step 1: Tests:** `wire_state_round_trips` (fixture home with skills in every `Placement` and MCP servers; `serde_json::from_str::<WireState>(out)` equals `wire::state(..)` field by field); `wire_state_masks_everything` (the `mcp_home()` fixture: output contains none of `FIXTURES`); `wire_server_is_unmasked` (contains `FIXTURE_ENV_SECRET`); `wire_is_hidden_from_help` (`prism --help` has no `wire`).
- [ ] **Step 2:** Run `cargo test wire_` — Expected: fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test wire_` and `cargo test` — Expected: pass.
- [ ] **Step 5:** Commit `feat: prism wire state/server/skill-md for remote environments`.

### Task 7: Receiving a skill from another machine

**Files:**
- Modify: `src/sync.rs`, `src/sync/label.rs` (or wherever `label` lives), `src/cli/skills.rs`, `src/cli/tests.rs`

**Interfaces:**
- Produces:
  - `pub fn receive(paths: &Paths, dest: &Scope, name: &str, tar: &mut dyn Read, targets: &[usize], j: &mut Journal) -> Result<(), String>` in `src/sync.rs`: unpacks with `tar -xf - -C <staging>` into `.agents/skills/.prism-incoming-<pid>-<name>`, renames to `.agents/skills/<name>` (journal `Created`), then links into `targets` like `copy_to`. A staging folder is removed on any failure.
  - CLI `prism skills receive NAME (--to-global | --to-project DIR) --source-hash HASH --from LABEL [--apply --plan-id ID]` (hidden): without `--apply` prints a plan (`actions: ["Copy NAME from LABEL to <scope> (link into …)"]`, conflict "already in <scope>" when the name exists); the plan id covers name, dest, targets and `HASH`. With `--apply` it reads the tar from stdin. Label `sync::label::receive(name, from)` = `"copy {name} from {from}"`.
  - CLI `prism skills trash NAME --agent A [--apply --plan-id ID]` (parity for the window's "Move this copy to the trash", using `sync::trash_copy`).

- [ ] **Step 1: Tests:** `receive_keeps_tree_and_refuses_clash` (a tar of a skill with `sub/a.md`, `.hidden`, and a relative symlink → identical tree on the target; a second receive of the same name is a conflict and leaves the first intact); `receive_plan_id_covers_hash` (different `--source-hash` → different plan id; applying with a stale id exits 4); `receive_then_undo_removes_it` (`prism undo` on the target removes folder and links); `skills_trash_moves_copy` (an agent's own copy goes to the trash; undo brings it back).
- [ ] **Step 2:** Run `cargo test receive_ skills_trash` — Expected: fail.
- [ ] **Step 3:** Implement. The sender gets the hash from `wire skill-hash` (Task 6).
- [ ] **Step 4:** Run the tests — Expected: pass.
- [ ] **Step 5:** Commit `feat: skills receive and skills trash`.

### Task 8: Copying across machines from the CLI

**Files:**
- Create: `src/mcp/portable.rs`, `src/hosts/copy.rs`
- Modify: `src/cli/skills.rs`, `src/cli/mcp.rs`, `src/mcp/plan.rs`, `src/cli/tests.rs`, `assets/prism-skill/SKILL.md`

**Interfaces:**
- Consumes: `Runner` (3), `check` (4), `skills receive` (7), `wire server` (6).
- Produces:
  - `pub fn warnings(server: &Server, source_home: &Path) -> Vec<String>` in `src/mcp/portable.rs`: one line per value that starts with `source_home`, `/opt/homebrew` or `/usr/local/Cellar` ("{field} points at {path} on {source}; check it exists here"), and per `${VAR}` reference ("{field} needs ${VAR} set here").
  - CLI `prism mcp receive NAME --to AGENT… [--claude-scope local|project] --from LABEL --source-home DIR [--apply --plan-id ID]` (hidden): reads a JSON `Server` from stdin on plan and on apply; plans `Request::Add` on this machine, adds the portability warnings; plan id covers the server JSON.
  - Source-side commands: `prism skills copy NAMES… --to-host HOST (--to-global | --to-project DIR) [--apply --plan-id ID]` and `prism mcp copy NAME --to-host HOST --to AGENT… [--from AGENT] [--apply --plan-id ID]`. They run `check` on HOST, ask HOST for the plan (`skills receive …` / `mcp receive …` with the definition on stdin), print it with every line suffixed " on HOST", and on `--apply` stream the tar (`tar -C <source> -cf - .`, symlinks kept) or the unmasked `Server` JSON. The plan id is the target's.
  - In `src/hosts/copy.rs`, shared by the CLI and the window (Task 12), with source and target both `Runner`s (`Here` for this machine), so remote → this machine and remote → remote work the same way:
    - `pub fn copy_skill(from: &dyn Runner, from_label: &str, source_dir: &Path, name: &str, to: &dyn Runner, to_label: &str, dest: &Scope, apply: Option<&str>) -> Result<Value, String>` — the tar comes from `from.shell("tar -C <quoted dir> -cf - .")`, the hash from `from.prism(["wire","skill-hash",…])` (add `wire skill-hash NAME [--project DIR]` → `{hash}` to Task 6's group).
    - `pub fn copy_server(from: &dyn Runner, from_label: &str, name: &str, from_agent: Option<usize>, from_scope: &Scope, to: &dyn Runner, to_label: &str, agents: &[usize], apply: Option<&str>) -> Result<Value, String>` — the definition comes from `from.prism(["wire","server",…])`.
    - Both return the target's plan JSON with every action line suffixed " on {to_label}".

- [ ] **Step 1: Tests** (two `Local` homes, A as this machine and B as the host): `skill_copy_to_host_plan_then_apply` (plan lists "Copy pdf from This computer to global (link into Claude Code) on laptop"; apply with the id → B has the folder and link; A unchanged); `mcp_copy_to_host_warns_on_paths` (a server with `/opt/homebrew/bin/node` and `${TOKEN}` → two warnings; after apply B's Claude config has the server with the secret, A's output never printed it); `copy_to_unready_host_refuses` (B reports another version → exit 1, nothing written).
- [ ] **Step 2:** Run `cargo test _to_host` — Expected: fail.
- [ ] **Step 3:** Implement; then add `--host` and `--to-host` to `assets/prism-skill/SKILL.md` with one example each.
- [ ] **Step 4:** Run the tests — Expected: pass.
- [ ] **Step 5:** Commit `feat: copy skills and MCP servers to another machine from the CLI`.

### Task 9: Dialogs render a plan view

**Files:**
- Create: `src/plan_view.rs`
- Modify: `src/ui/dialogs.rs`, `src/ui/mcp_dialogs.rs`, `src/app/installed.rs`, `src/app/mcp_edit.rs`

**Interfaces:**
- Produces: `pub struct PlanView { pub lines: Vec<String>, pub skipped: Vec<(String, String)>, pub warnings: Vec<String>, pub id: Option<String> }` with `From<(&sync::Plan, &Paths)>`, `From<(&McpPlan, &Paths)>` and `pub fn from_json(v: &serde_json::Value) -> Result<PlanView, String>` (the CLI plan JSON of skills and mcp commands: `actions`, `conflicts`/`skipped`, `warnings`, `plan_id`). The sync and MCP dialogs draw only from `PlanView`; applying still uses the local plan objects for This Mac.

- [ ] **Step 1: Tests:** `plan_view_from_cli_json` (skills plan JSON with a conflict and mcp plan JSON with a warning both map); existing UI snapshot tests (`src/ui_tests.rs`) pass unchanged — this task changes no pixels.
- [ ] **Step 2:** Run `cargo test plan_view` — Expected: fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test` — Expected: all pass, snapshots identical.
- [ ] **Step 5:** Commit `refactor: dialogs draw a PlanView`.

### Task 10: Environments in the window

**Files:**
- Create: `src/app/env.rs`, `src/ui/hosts.rs`
- Modify: `src/app/mod.rs`, `src/app/watch.rs`, `src/ui/mod.rs`, `src/ui/dialogs.rs`, `src/ui/mcp_dialogs.rs`, `src/ui/inspector.rs`, `src/ui_tests.rs`

**Interfaces:**
- Consumes: `Host`, `Runner`, `check`, `install` (2–4), `WireState` (6).
- Produces:
  - `pub enum Env { Local, Remote { host: Host, state: RemoteState } }`, `pub enum RemoteState { Connecting, NeedsInstall(Health), Unreachable(String), Ready }`; `App.env: Env`, `App.local: Option<Stash>` (the local `inv`, `mcp`, `paths`, `settings`, `scope`, `stats` while a remote is in view), `App.remote_detected: Option<(Vec<bool>, Vec<bool>)>`.
  - `App.make_runner: MakeRunner` (Task 3); `App::switch_env(&mut self, to: Option<&str>)` (None = this machine); remote loading runs `check` then `wire state` on a `rune::Latest` thread and lands in `poll()`, filling `inv`, `mcp`, `paths`, `settings`, `scope`, `stats` from `WireState`.
  - `App::detected(&self, agent: usize) -> bool` and `App::mcp_detected(&self, agent: usize) -> bool`: every UI call of `agent.detected(&s.paths)` (dialogs.rs:62, mcp_dialogs.rs:188, 314, mcp_edit.rs:279) goes through these. The preview uses `wire skill-md` on a remote; existence checks (dialogs.rs:209, app/remote.rs:50) use the inventory instead of the disk.
  - The watcher is off while a remote is in view; ⌘R reloads the remote state.
  - Sidebar: an environment switcher above Global (`Select` keyed `env-switch`: "This Mac" + host names + "Add host…"); "Add host…" opens a dialog (`TextInput` for name and ssh target, `Contrast` "Add host"), then connects. `RemoteState::NeedsInstall` shows `empty()` with "prism {version} isn't on {name}" and a `Contrast` "Install prism"; `Unreachable` shows `empty()` with the error and a `Ghost` "Retry". Browse (skills.sh) shows "skills.sh installs go to this Mac; copy them to {name} afterwards" when a remote is in view.

- [ ] **Step 1: Tests** (`src/ui_tests.rs`, with `Local` runners handed out by `App.make_runner`): `switching_to_host_shows_its_skills` (host B's skills appear, A's are stashed and come back on switching back); `unreachable_host_shows_retry` (a fake runner returning `Err` → the empty state with Retry, within one poll); `needs_install_offers_install` (fake reports another version); snapshot scenes `env-remote` and `env-unreachable` render in light and dark.
- [ ] **Step 2:** Run `cargo test env_ switching_ unreachable_ needs_install` — Expected: fail.
- [ ] **Step 3:** Implement. Add the scenes to `SCENES` in `src/main.rs`.
- [ ] **Step 4:** Run the tests and `target/debug/prism --snap /tmp/e.png --scene env-remote --theme dark`; look at the image — Expected: the switcher shows the host name, the list shows the remote's skills.
- [ ] **Step 5:** Commit `feat: open another machine as an environment`.

### Task 11: Acting on a remote environment

**Files:**
- Create: `src/app/remote_actions.rs`
- Modify: `src/app/installed.rs`, `src/app/mcp_edit.rs`, `src/app/mod.rs`, `src/app/discover.rs`, `src/ui/inspector.rs`, `src/ui/mcp.rs`, `src/ui_tests.rs`

**Interfaces:**
- Consumes: `Env` (10), `PlanView` (9), `Runner` (3), CLI commands (existing + `skills trash`).
- Produces: in a remote environment every action method below builds CLI args, gets the plan from the host (a `PlanView` in the existing dialog, lines suffixed " on {host}"), applies with `--apply --plan-id`, then reloads `wire state`; all calls run off the UI thread.

| Window action | Remote command |
| --- | --- |
| `apply_sync` (all / only) | `skills sync [NAMES…] [--to …]` |
| `toggle_link` | `skills link` / `skills unlink NAME --agent A` |
| `replace_with_link` | `skills sync NAME --to A` |
| `trash_copy` | `skills trash NAME --agent A` |
| `remove`, `remove_broken` | `skills remove NAMES…`, `skills clean-broken` |
| `copy_to` (within the host) | `skills copy NAMES… --to-global/--to-project` |
| `create_skill` | `skills new NAME --description D` |
| `toggle_target` | `targets set …` |
| `add_project`/`forget_project` | `projects add/remove DIR` (typed path, see below) |
| MCP add/edit/remove/copy/merge/disable | `mcp add/edit/remove/copy/merge/disable/enable` with `K=-` values on stdin |
| `undo` | `undo` |

  - "Add project…" on a remote opens a dialog with a `TextInput` for an absolute path on that machine (no folder picker).
  - Show values on a remote fetches `wire server NAME` for that server only and drops it when hidden.
  - Toasts end with " on {host}".

- [ ] **Step 1: Tests** (`Local` host B): `remote_link_toggle_writes_on_host` (toggle → B's link exists, A untouched); `remote_mcp_edit_secret_goes_by_stdin` (edit an env value → B's config has it; the runner's argv never contains it); `remote_undo_acts_on_host` (⌘Z in B's view undoes B's last change, A's last change intact); `remote_add_project_takes_typed_path`.
- [ ] **Step 2:** Run them — Expected: fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test` — Expected: pass.
- [ ] **Step 5:** Commit `feat: every window action works on a remote environment`.

### Task 12: Copy to another machine from the window

**Files:**
- Modify: `src/ui/dialogs.rs` (`copy`), `src/ui/mcp_dialogs.rs`, `src/app/installed.rs`, `src/app/mcp_edit.rs`, `src/app/mod.rs` (`Notice` gains `machine: Option<String>`), `src/ui/mod.rs` (toast Undo), `src/ui_tests.rs`

**Interfaces:**
- Consumes: `copy_skill`, `copy_server` (8), `Env` (10), `PlanView` (9).
- Produces:
  - Skill "Copy to…" lists this environment's other scopes, then one group per other environment (This Mac or each host) with its Global and projects (from that machine's `projects list`, fetched when the dialog opens; a host that can't be reached shows its error line in the group). Picking one shows the target's plan, then applies.
  - MCP "Copy to…" gains a "Machine" `Select` (this one by default); another machine lists that machine's detected agents as checkboxes (its `agents --json`), and the plan's warnings show under `heading("Heads-up")`.
  - `Notice.machine`: a toast's Undo runs `undo` on that machine; ⌘Z keeps acting on the environment in view.

- [ ] **Step 1: Tests:** `copy_skill_to_host_from_window` (dialog lists the host's Global; apply → B has it; toast says "on laptop"); `toast_undo_targets_named_machine` (after the copy, the toast's Undo removes it on B, while A's last change is untouched); `mcp_copy_to_host_shows_warnings`.
- [ ] **Step 2:** Run them — Expected: fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test`, then snapshot the copy dialog (`--scene` added: `copy-to-host`) in both themes and look at it.
- [ ] **Step 5:** Commit `feat: copy skills and servers to another machine from the window`.

### Task 13: End to end over a real ssh, and docs

**Files:**
- Create: `tests/e2e_ssh.rs` (or `src/hosts/e2e.rs` under `#[cfg(test)]`), `scripts/e2e-ssh.sh`
- Modify: `README.md`, `DESIGN.md` (environment switcher under "Where things live"), `docs/design/layout.md` (switcher, unreachable and install states)

- [ ] **Step 1:** `scripts/e2e-ssh.sh` starts a Linux container with `openssh-server`, an authorized throwaway key and the CLI-only Linux binary in `~/.local/bin`, and prints an ssh target; an `#[ignore]` test `e2e_copy_skill_and_server_over_ssh` uses it with the real `Ssh` runner: `hosts add`, `hosts check` ready, `skills copy --to-host` plan + apply, `mcp copy --to-host` with a secret, `--host … undo`.
- [ ] **Step 2:** Run `./scripts/e2e-ssh.sh && cargo test -- --ignored e2e_` — Expected: pass.
- [ ] **Step 3:** README: "Other machines" section (hosts, the switcher, copy across, `--host`); a screenshot of the remote environment from `scripts/demo-home.sh` (add a `Local`-backed demo host scene).
- [ ] **Step 4:** Commit `test: end to end over ssh; docs: other machines`.

**Done when:** on George's personal Mac, `prism hosts add laptop --ssh <target>` then `prism hosts install laptop`, the window's switcher opens the laptop, and copying mobbin and the emil skills from This Mac to the laptop leaves them working in the laptop's Claude Code (checked there with `claude mcp list` and the skills folder).
