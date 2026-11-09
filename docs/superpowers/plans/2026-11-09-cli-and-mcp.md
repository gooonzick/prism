# Prism CLI and MCP Servers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `prism` CLI with everything the window does, and MCP server management across agents in both the CLI and the window.

**Architecture:** The core modules (`agents`, `scan`, `sync`, `fsops`, `remote`, `skill`, `store`) stay free of `rune`; a new `mcp` core sits beside them. `src/cli/` parses arguments with clap and calls the core; `src/app/` and `src/ui/` call the same core. One binary: `main` hands known subcommands to the CLI before Rune starts.

**Tech Stack:** Rust 1.99 (pinned in `rust-toolchain.toml`), Rune/rune-kit (UI), clap 4 (derive), `jsonc-parser` 0.34 with the `cst` feature (all JSON and JSONC edits, comment- and whitespace-preserving), `toml_edit` (Codex), serde_json, tempfile (tests).

**Spec:** `docs/superpowers/specs/2026-11-09-cli-and-mcp-design.md`. Read it before any task; this plan argues from it.

## Global Constraints

- `cargo clippy --all-targets -- -D warnings` clean and `cargo test` green after every task; CI runs both on Linux x86_64, Linux arm64 and macOS.
- Tests are headless, on a throwaway home (`Paths::isolated`), never touch the network (network tests stay `#[ignore]`), and never read the real `~`.
- Core modules never import `rune` or `rune_kit`.
- A config write changes only the server entry it targets; the rest of the file stays byte-for-byte (keys, order, whitespace, comments).
- Env and header values print as `***` unless `--show-secrets` (CLI) or the user reveals them (UI).
- CLI never prompts. Exit codes: 0 success or nothing to do, 1 operation failed, 2 bad usage, 3 `doctor` found problems, 4 plan changed since `--plan-id`.
- Commands that change or remove existing state print a plan and act only with `--apply`. `add`, `install`, `new` act at once.
- App-managed MCP copies (command inside a `*.app/` bundle) are read-only everywhere.
- Claude Code project servers default to the local scope (`~/.claude.json` → `projects[<abs path>].mcpServers`); `.mcp.json` only on request; Prism never writes Claude Code's `.mcp.json` approval lists.
- Config backups live in `<state>/backups/` (0700 folder, 0600 files), only for the last change.
- Code style: short doc comments that say why, small modules, names as in the existing code; commits `feat: …` / `fix: …` / `docs: …` like the history.

## Review Focus

1. **A config the user has hand-edited in an unusual way** (JSONC with trailing commas and comments in `opencode.jsonc`, a Codex TOML with inline tables or comments between servers, a `.claude.json` with ~100 unrelated keys): writes must leave everything else byte-identical. Pinned by the `*_round_trip_*` fixtures in Task 10.
2. **A config that fails to parse** (half-written by the agent, invalid JSON): `scan` reports it in `errors` and Prism never writes to it. Pinned in Task 7 (`unreadable_config_is_reported_not_rewritten`) and Task 11 (`plan_refuses_unreadable_file`).
3. **The file changes between plan and apply** (Claude Code rewrites `~/.claude.json` while running): apply refuses instead of overwriting. Pinned in Task 11 (`apply_refuses_when_file_changed_since_plan`).
4. **Secrets reaching output**: `--json`, human output, error messages and `doctor` all mask. Pinned in Task 9 (`list_json_masks_env_and_headers`) and Task 12 (`errors_never_echo_values`).
5. **Two Prism processes at once** (the window open while an agent runs the CLI): the window sees CLI changes and `undo` in either reverts the CLI's change. Pinned in Task 2 (`last_change_round_trips_through_disk`) and Task 14 (`window_picks_up_a_cli_change`).

---

## File Structure

| File | Responsibility |
| --- | --- |
| `src/main.rs` | Dispatch: CLI subcommand → `cli::run`, else the window |
| `src/agents.rs` | + `Paths::state` (Prism's own folder) |
| `src/store.rs` | Settings at `paths.state`; `LastChange` save/load; backups folder |
| `src/fsops.rs` | + `Step::Rewrote`, `Journal::write_file`, serde for `Journal` |
| `src/planid.rs` | `plan_id` hashing, shared by skills and MCP plans |
| `src/mcp/mod.rs` | Re-exports; `McpInventory`, `scan` |
| `src/mcp/model.rs` | `Server`, `Transport`, `Value`/`Part`, `Copy`, `Place`, diffs |
| `src/mcp/agents.rs` | `MCP_AGENTS` table: files per scope, flavour |
| `src/mcp/formats/{mod,json,claude,cursor,gemini,opencode,codex,windsurf,kiro,desktop}.rs` | Read/upsert/remove/disable per flavour |
| `src/mcp/scan.rs` | Grouping copies into servers, status, merges, read-only, inherited |
| `src/mcp/plan.rs` | `Request` → `McpPlan`; `apply` |
| `src/cli/mod.rs` | clap tree, `run`, output and exit codes |
| `src/cli/{skills,mcp,projects,doctor,setup}.rs` | Command handlers |
| `src/cli/tests.rs` | CLI tests through `run_with` |
| `src/app/mcp.rs` | Window state and actions for the MCP page |
| `src/ui/mcp.rs`, `src/ui/mcp_dialogs.rs` | MCP page, inspector, dialogs |
| `assets/prism-skill/SKILL.md` | The skill `prism setup` installs |

---

### Task 1: CLI entry point and `prism agents`

**Files:**
- Modify: `Cargo.toml` (add `clap = { version = "4", features = ["derive"] }`)
- Modify: `src/main.rs`
- Create: `src/cli/mod.rs`, `src/cli/tests.rs`

**Interfaces:**
- Produces:
  - `pub struct Ctx { pub paths: Paths, pub settings: Settings }`
  - `pub fn run(args: Vec<OsString>) -> Option<i32>`: `None` when `args[0]` isn't a subcommand (so a folder still opens the window); else parses, builds `Ctx` from the environment, runs, returns the exit code.
  - `pub fn run_with(args: &[&str], ctx: &mut Ctx, out: &mut dyn Write, err: &mut dyn Write) -> i32` for tests.
  - `pub enum Exit { Ok = 0, Failed = 1, Usage = 2, Problems = 3, PlanChanged = 4 }`
  - `pub struct Out { json: bool, … }` with `fn emit(&mut self, human: impl Display, json: serde_json::Value)`.
  - Global flags on every subcommand: `--project DIR`, `--json`.
  - Subcommand names: `agents`, `targets`, `skills`, `mcp`, `projects`, `undo`, `doctor`, `setup`.

- [ ] **Step 1: Write the failing tests** in `src/cli/tests.rs`:
  - `a_folder_is_not_a_subcommand`: `run(vec!["/tmp".into()]) == None`, and `run(vec![])` is `None`.
  - `agents_lists_detected_agents_as_json`: isolated home with `~/.claude` and `~/.codex` dirs; `run_with(&["agents","--json"])` returns 0 and the JSON is an array of `{id, name, detected, target}` with `claude` and `codex` `detected: true`, `cursor` `detected: false`.
  - `unknown_flag_is_usage_error`: `run_with(&["agents","--bogus"])` returns 2 and writes to `err`.
- [ ] **Step 2: Run** `cargo test cli::tests` — expect compile failure (no `cli` module).
- [ ] **Step 3: Implement** `src/cli/mod.rs` and in `main()` call `if let Some(code) = cli::run(std::env::args_os().skip(1).collect()) { std::process::exit(code) }` before `rune::prewarm_gpu()`. clap errors map to exit 2.
- [ ] **Step 4: Run** `cargo test cli::tests` — PASS; `cargo run -q -- agents` prints a table.
- [ ] **Step 5: Commit** `feat(cli): prism agents and the subcommand entry point`

### Task 2: Prism's state folder, a journal on disk, and config rewrites

**Files:**
- Modify: `src/agents.rs` (`Paths`), `src/store.rs`, `src/fsops.rs`, `src/app/mod.rs` (`undo`, `last_change`), `src/cli/mod.rs` (`undo`)
- Test: `src/fsops.rs` tests, `src/store.rs` tests, `src/cli/tests.rs`

**Interfaces:**
- Produces:
  - `Paths::state: PathBuf`: `dirs::config_dir()/dev.prism.skills` in `from_env`; `home/.config/dev.prism.skills` in `isolated`. `Settings::load_from(&Paths)` / `save_to(&Paths)` replace the `dirs::config_dir()` calls (keep `migrate`).
  - `#[derive(Serialize, Deserialize)]` on `Journal` and `Step`.
  - `Step::Rewrote { path: PathBuf, backup: Option<PathBuf>, hash_after: u64 }` (`backup: None` means the file didn't exist before).
  - `Journal::write_file(&mut self, path: &Path, contents: &[u8], paths: &Paths) -> io::Result<()>`: first touch of `path` in this journal copies the original to `paths.state/backups/<n>-<file name>` (0600; folder 0700), writes `<path>.prism-tmp` in the same folder, renames over `path`, records `Rewrote`.
  - Undo of `Rewrote`: restore the backup (or delete the file when `backup` is `None`) only if the file's hash is `hash_after`; else error `"{path} changed since"`.
  - `pub struct LastChange { pub label: String, pub journal: Journal }`; `store::save_last_change(&Paths, Option<&LastChange>)`, `store::load_last_change(&Paths) -> Option<LastChange>` at `state/last-change.json`. `store::begin_change(&Paths)` empties `state/backups/`.
  - `App::last_change` loads from disk in `App::new` and is saved on every change and after undo (`None`).
  - CLI `prism undo`: loads, undoes, clears; prints `Undid: <label>`; exit 1 with the error if undo stops early; exit 0 with "Nothing to undo" when there's none.

- [ ] **Step 1: Write the failing tests:**
  - `rewrite_backs_up_and_undo_restores` (fsops): write `a.json` = `{"x":1}`, `write_file` with `{"x":2}`, assert backup exists with mode 0600 and the original bytes; undo → file is `{"x":1}`.
  - `rewrite_of_new_file_undo_deletes_it`.
  - `undo_refuses_after_outside_edit`: rewrite, then overwrite the file by hand, `undo` → `Err` containing `"changed since"`, file keeps the hand edit.
  - `last_change_round_trips_through_disk` (store): save a `LastChange` with a `Linked` and a `Rewrote` step, load equals it.
  - `cli_undo_reverts_last_change` (cli): seed a last change that linked a skill, `run_with(&["undo"])` → 0, link gone, `load_last_change` is `None`.
- [ ] **Step 2: Run** `cargo test fsops store cli::tests::cli_undo` — FAIL.
- [ ] **Step 3: Implement** as in Interfaces. Hash with `std::hash::DefaultHasher` over the bytes.
- [ ] **Step 4: Run** `cargo test` — PASS (the existing UI undo tests too).
- [ ] **Step 5: Commit** `feat: keep the last change on disk so the CLI and the window share undo`

### Task 3: Plan ids

**Files:**
- Create: `src/planid.rs`
- Modify: `src/main.rs` (`mod planid;`)

**Interfaces:**
- Produces: `pub fn plan_id(lines: &[String], state: &[(PathBuf, Option<u64>)]) -> String`: 12 lowercase hex chars of a `DefaultHasher` over the lines and each path with its content hash (`None` = missing). `pub fn file_hash(path: &Path) -> Option<u64>`. `pub fn tree_hash(path: &Path) -> Option<u64>` (a skill folder: names, link targets and file bytes, sorted).

- [ ] **Step 1: Tests** `same_plan_same_id`, `changed_file_changes_id`, `id_is_12_hex`.
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat: plan ids for apply-what-you-saw`

### Task 4: Skills, projects and targets in the CLI (read-only)

**Files:**
- Create: `src/cli/skills.rs`, `src/cli/projects.rs`
- Modify: `src/cli/mod.rs`, `src/app/discover.rs` (make `find_projects` reachable without `App`; it already is `pub fn`)

**Interfaces:**
- Consumes: `scan::scan`, `skill::read`, `agents::{AGENTS, default_targets}`, `discover::find_projects`.
- Produces commands and JSON:
  - `skills list` → `[{name, description, placements: {<agent id>: "dir"|"link"|"linked"|"broken"|"missing"}, synced: bool}]`
  - `skills show NAME` → `{name, description, path, body_lines, placements}`; unknown name → exit 1 `no skill named NAME in <scope>`.
  - `projects list|add DIR|remove DIR|find DIR` → `add`/`remove` save settings; `find` returns `[{path, skills}]` without adding.
  - `targets set AGENT...` → saves `settings.targets`; unknown id → exit 2 listing valid ids.
  - `--project DIR` canonicalises DIR; a project not in settings still works (the CLI doesn't require registration).

- [ ] **Step 1: Tests** in `cli/tests.rs`: `skills_list_json_shows_placements` (a Claude-only skill and a synced one), `skills_show_unknown_is_exit_1`, `projects_add_persists_to_state_dir`, `targets_set_rejects_unknown_agent`.
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(cli): list and show skills, manage projects and targets`

### Task 5: Changing skills from the CLI

**Files:**
- Modify: `src/cli/skills.rs`; `src/app/installed.rs` (move `valid_name` and the SKILL.md template of `create_skill` into `src/skill.rs` so both use them)

**Interfaces:**
- Consumes: `sync::{plan, plan_many, apply, link, unlink, remove, remove_broken, copy_to}`, `planid`, `store::{begin_change, save_last_change}`.
- Produces commands: `skills sync [NAME...] [--to AGENT...] [--apply] [--plan-id ID]`, `link NAME --agent A`, `unlink NAME --agent A`, `remove NAME...`, `copy NAME... (--to-project DIR | --to-global)`, `clean-broken`, `new NAME --description TEXT` (immediate).
- JSON for planning commands: `{"applied": bool, "plan_id": "…", "actions": ["<Action::describe()>", …], "conflicts": [{"skill","reason"}]}`; after apply also `"done": n`. Plan id inputs: action descriptions + `tree_hash` of each involved skill path in each agent.
- `--apply --plan-id X` with a different fresh id → exit 4, nothing changed, JSON `{"applied": false, "plan_id": "<new>"}`.
- Every apply: `begin_change`, run with a fresh `Journal`, save `LastChange { label }` (labels as the window uses: `Synced: 3 changes`, `Removed review`, …).

- [ ] **Step 1: Tests:** `sync_without_apply_changes_nothing`, `sync_apply_links_and_records_last_change`, `apply_with_stale_plan_id_exits_4` (plan, then add a file to the skill, apply with old id), `remove_needs_apply`, `new_skill_is_immediate`.
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(cli): sync, link, remove, copy and create skills`

### Task 6: skills.sh from the CLI

**Files:**
- Modify: `src/cli/skills.rs`

**Interfaces:**
- Consumes: `remote::{search, list_repo, install, check_updates, update, parse_source}`, and the linking that `app/remote.rs::link_installed` does — move that function's body into `sync::link_installed(scope, paths, targets, skills) -> Result<usize, String>` so the window and CLI share it.
- Produces: `skills search QUERY`, `skills repo SOURCE`, `skills install SOURCE[@SKILL] [--skill NAME...] [--to AGENT...]`, `skills updates`, `skills update [NAME...]`. No Node → exit 1 with the existing "Neither pnpx, npx nor bunx…" message.

- [ ] **Step 1: Tests:** `install_bad_source_is_usage_error` (`skills install "not a source!"` → exit 2, no process spawned), plus `#[ignore]` network tests `search_finds_find_skills`, `install_links_into_targets`.
- [ ] **Step 2–4:** run (FAIL), implement, run (PASS).
- [ ] **Step 5: Commit** `feat(cli): search, install and update skills from skills.sh`

### Task 7: MCP model, agents table and readers

**Files:**
- Create: `src/mcp/mod.rs`, `src/mcp/model.rs`, `src/mcp/agents.rs`, `src/mcp/formats/*.rs`
- Modify: `Cargo.toml` (`jsonc-parser = { version = "0.34", features = ["cst", "serde"] }`, `toml_edit = "0.22"` — check the latest versions and the CST API on docs.rs first)

**Interfaces:**
- Produces (`model.rs`):
  - `pub enum Part { Text(String), Var { name: String, default: Option<String> } }`; `pub struct EnvValue(pub Vec<Part>)` with `is_literal()`, `masked() -> String` (literals → `***`, vars kept: `${GITLAB_TOKEN}`).
  - `pub enum Transport { Stdio { command: String, args: Vec<String>, env: BTreeMap<String, EnvValue>, cwd: Option<String> }, Http { url: String, headers: BTreeMap<String, EnvValue>, sse: bool } }`
  - `pub struct Server { pub name: String, pub transport: Transport }`
  - `pub enum Place { Global, ClaudeLocal(PathBuf), Project(PathBuf) }`
  - `pub type Extras = serde_json::Map<String, serde_json::Value>` (Codex TOML extras are converted to JSON values and back)
  - `pub struct McpCopy { pub agent: usize, pub file: PathBuf, pub place: Place, pub server: Server, pub extras: Extras, pub enabled: bool, pub read_only: Option<String> }` (not `Copy`: that would shadow the trait)
- Produces (`agents.rs`): `pub enum Flavor { ClaudeCode, ClaudeDesktop, Codex, Cursor, OpenCode, Gemini, Windsurf, Kiro }`; `pub struct McpAgent { pub id: &'static str, pub name: &'static str, pub flavor: Flavor }`; `pub static MCP_AGENTS: &[McpAgent]` with ids `claude`, `claude-desktop`, `codex`, `cursor`, `opencode`, `gemini`, `windsurf`, `kiro` (same ids as `agents::AGENTS` where shared); `McpAgent::files(&self, scope: &Scope, paths: &Paths) -> Vec<(PathBuf, Place)>` per the spec's table (OpenCode: `.jsonc` if it exists, else `.json`; Claude Desktop only on macOS; Codex project file only if Codex reads one — check its docs; Claude Code project scope yields both `ClaudeLocal(project)` from `~/.claude.json` and `Project` from `.mcp.json`); `McpAgent::detected(&Paths) -> bool`. Every `agent: usize` in `mcp` indexes `MCP_AGENTS`, not `agents::AGENTS`; the CLI maps ids through `mcp::agents::index_of(id)`.
- Produces (`formats`): `pub fn read(flavor: Flavor, text: &str, place: &Place) -> Result<Vec<(Server, Extras, bool /*enabled*/)>, String>`, plus `pub fn disabled_in(text: &str, project: &Path) -> Vec<String>` for Claude Code (`projects[path].disabledMcpServers` ∪ `disabledMcpjsonServers`).
- Env reference syntaxes to decode (verify each against current docs; when an agent has none, every value is literal): Claude Code `${VAR}` / `${VAR:-default}`; Gemini `$VAR` / `${VAR}`; Cursor and Windsurf `${env:VAR}`; OpenCode `{env:VAR}`; Kiro `${VAR}`; Claude Desktop none; Codex: `env_vars = ["X"]` → `env.X = Var(X)`, `bearer_token_env_var = "T"` → `headers.Authorization = "Bearer " + Var(T)`, `env_http_headers = {H = "V"}` → `headers.H = Var(V)`, `http_headers` literal.
- Read-only: `read_only = Some(app name)` when `command` contains `.app/` (app name = the bundle's file stem).

- [ ] **Step 1: Tests** (fixtures as inline strings in `src/mcp/formats/*` tests, shaped like the configs found on George's machine but with fake values):
  - `claude_reads_user_local_and_project_scopes` (stdio with env, http with header, a `projects[path]` entry)
  - `cursor_reads_url_without_type_as_http`
  - `opencode_reads_local_command_array_and_env_refs` (`{env:TOKEN}` → `Var`)
  - `codex_reads_env_vars_and_bearer_as_references`
  - `gemini_reads_dollar_refs`, `windsurf_reads_server_url`, `kiro_reads_mcp_servers`, `desktop_reads_stdio`
  - `unknown_fields_land_in_extras` (Codex `startup_timeout_sec`, Claude `type` is modelled not extra)
  - `app_bundle_command_is_read_only`
  - `unreadable_config_is_reported_not_rewritten` (garbage text → `Err`)
- [ ] **Step 2: Run** `cargo test mcp::` — FAIL. **Step 3: Implement.** **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(mcp): read MCP servers from every agent's config`

### Task 8: Grouping copies into servers

**Files:**
- Create: `src/mcp/scan.rs`

**Interfaces:**
- Consumes: Task 7.
- Produces:
  - `pub enum Status { InSync, Differs, ReadOnly }`
  - `pub struct FieldDiff { pub field: String /* "command", "args", "url", "env.KEY", "headers.KEY" */, pub values: Vec<(usize /*agent*/, Option<String> /*masked; None = missing*/)> }`
  - `pub struct McpServer { pub name: String, pub copies: Vec<McpCopy>, pub inherited: bool, pub status: Status, pub diffs: Vec<FieldDiff>, pub disabled_for: Vec<usize> }`
  - `pub struct MergeHint { pub names: Vec<String>, pub reason: String }`
  - `pub struct McpInventory { pub scope: Scope, pub servers: Vec<McpServer>, pub merges: Vec<MergeHint>, pub errors: Vec<(PathBuf, String)> }` with `get(&self, name) -> Option<&McpServer>`
  - `pub fn scan(scope: &Scope, paths: &Paths) -> McpInventory`: reads every detected agent's files for the scope; in a project also the global copies, as `inherited: true` servers unless the project defines the same name; groups by name; status `ReadOnly` if any copy is read-only, `Differs` if transports differ after decoding refs, else `InSync`; `merges` for different names with the same URL or the same command+args after normalising runners (`npx -y X`, `npx X`, `bunx -y X`, `bunx X`, `pnpx X` → `X`).

- [ ] **Step 1: Tests:** `same_name_groups_across_agents`, `different_env_value_differs_with_masked_diff` (diff field `env.TOKEN`, values `***`), `missing_header_shows_as_none`, `refs_in_different_syntax_are_equal` (`${env:T}` in Cursor and `${T}` in Claude → `InSync`), `npx_and_bunx_same_package_suggest_merge`, `project_scope_shows_global_as_inherited`, `claude_disabled_list_marks_disabled_for`, `unreadable_file_goes_to_errors`.
- [ ] **Step 2–4:** FAIL, implement, PASS.
- [ ] **Step 5: Commit** `feat(mcp): group copies, detect drift and suggest merges`

### Task 9: `prism mcp list` and `prism mcp show` (first safe milestone)

**Files:**
- Create: `src/cli/mcp.rs`

**Interfaces:**
- Produces:
  - `mcp list` JSON: `{"servers":[{"name","inherited","status":"in_sync"|"differs"|"read_only","agents":[ids],"disabled_for":[ids],"transport":"stdio"|"http"}],"merges":[{"names","reason"}],"errors":[{"file","error"}]}`
  - `mcp show NAME [--show-secrets]`: `{"name","copies":[{"agent","file","place","enabled","read_only","transport":{…values masked…}}],"diffs":[…]}`
  - Human output: one line per server, agent short names, status word.

- [ ] **Step 1: Tests:** `list_json_masks_env_and_headers` (no literal fixture value appears anywhere in stdout), `show_secrets_reveals_values`, `show_unknown_is_exit_1`.
- [ ] **Step 2–4:** FAIL, implement, PASS. Then run `cargo run -q -- mcp list` on this machine and check by eye that it matches what the configs hold (read-only, safe).
- [ ] **Step 5: Commit** `feat(cli): prism mcp list and show`

### Task 10: Writers that keep the rest of the file

**Files:**
- Modify: `src/mcp/formats/*.rs`; create `src/mcp/formats/json.rs` (shared CST helpers)

**Interfaces:**
- Produces, per flavor:
  - `pub fn upsert(flavor: Flavor, text: &str, place: &Place, server: &Server, extras: &Extras) -> Result<String, String>`: `Err("<agent> can't express <field>")` when a value or transport has no encoding (e.g. a `Var` in Claude Desktop, a non-bearer header ref in Codex). Empty `text` means a new file: write the minimal document with 2-space indent.
  - `pub fn remove(flavor: Flavor, text: &str, place: &Place, name: &str) -> Result<String, String>`
  - `pub fn rename(flavor: Flavor, text: &str, place: &Place, from: &str, to: &str) -> Result<String, String>`
  - `pub fn set_disabled(flavor: Flavor, text: &str, project: &Path, name: &str, disabled: bool) -> Result<String, String>`: Claude Code edits `projects[path].disabledMcpServers` (for `ClaudeLocal`/inherited copies) or `disabledMcpjsonServers` (for `.mcp.json` copies); OpenCode/Codex set `enabled` only on a copy in the project's own file; other flavours `Err`.
- JSON flavours edit through `jsonc_parser::cst` only; Codex through `toml_edit::DocumentMut`.

- [ ] **Step 1: Tests** (`expected` strings written out in full):
  - `claude_round_trip_touches_only_one_entry`: a `.claude.json` with unrelated keys, odd spacing and a `projects` map; upsert `gitlab` env change → output equals the input with only that value changed.
  - `opencode_round_trip_keeps_comments_and_trailing_commas`
  - `codex_round_trip_keeps_comments_and_other_tables` (incl. `[mcp_servers.x.env]` sub-table style and inline-table style)
  - `cursor_upsert_into_empty_file_creates_document`
  - `remove_last_server_leaves_empty_object`
  - `desktop_rejects_env_reference`
  - `claude_disable_adds_to_project_list_once` (idempotent)
  - `rename_keeps_position`
- [ ] **Step 2–4:** FAIL, implement, PASS.
- [ ] **Step 5: Commit** `feat(mcp): write servers into each agent's config, leaving the rest as it was`

### Task 11: MCP plans and apply

**Files:**
- Create: `src/mcp/plan.rs`

**Interfaces:**
- Consumes: Tasks 2, 3, 8, 10.
- Produces:
  - `pub enum ClaudeScope { Local, Project }` (default `Local`)
  - `pub enum EditChange { Command(String), Args(Vec<String>), SetEnv(String, EnvValue), UnsetEnv(String), Url(String), SetHeader(String, EnvValue), UnsetHeader(String) }`
  - `pub enum Request { Add { server: Server, to: Vec<usize>, claude_scope: ClaudeScope }, Copy { name: String, to: Vec<usize>, from: Option<usize> }, Edit { name: String, changes: Vec<EditChange> }, Remove { name: String, from: Vec<usize> /* empty = all */ }, SetDisabled { name: String, disabled: bool }, Merge { names: Vec<String>, into: String } }`
  - `pub enum McpAction { Write { agent: usize, file: PathBuf, place: Place, server: Server, extras: Extras, replaces: bool }, Delete { agent: usize, file: PathBuf, place: Place, name: String }, Rename { agent: usize, file: PathBuf, place: Place, from: String, to: String }, SetDisabled { agent: usize, file: PathBuf, name: String, disabled: bool } }` with `describe()` (wording like `sync::Action::describe`, e.g. `Add plata to Cursor`, `Overwrite Cursor's context7 with Claude Code's`).
  - `pub struct McpPlan { pub actions: Vec<McpAction>, pub skipped: Vec<(usize, String)>, pub warnings: Vec<String>, pub files: Vec<(PathBuf, Option<u64>)> }` with `id() -> String` via `planid`.
  - `pub fn plan(inv: &McpInventory, req: &Request, paths: &Paths) -> Result<McpPlan, String>`. Rules from the spec: read-only servers → `Err`; `Copy` with differing copies and no `from` → `Err("copies differ; pick one with --from")`; `Copy` with `from` overwrites differing copies (`replaces: true`); `Edit` applies changes to every copy, editing an inherited server targets the global copies and adds the warning `"edits the global server"`; `SetDisabled` needs a project scope; agents that can't encode → `skipped` with the reason; writing literal secrets into a file that `git ls-files --error-unmatch` tracks or `git check-ignore` doesn't ignore → warning `"<file> is in git; the token would be committed"`; Claude Code project targets use `ClaudeLocal` unless `ClaudeScope::Project`; `.mcp.json` writes add the warning `"Claude Code will ask to approve it on its next start"`; files from `inv.errors` → `Err("can't read <file>: …")`; any write to `~/.claude.json` adds the warning `"restart running Claude Code sessions to load it"`. Before finishing this task, check by hand (a scratch `CLAUDE_CONFIG_DIR`, a running `claude`, an external edit, then `/mcp` in that session) whether Claude Code overwrites external edits to `~/.claude.json`; if it does, make the warning say `"quit Claude Code first, or it may overwrite this change"` and note the finding in the commit message.
  - `pub fn apply(plan: &McpPlan, paths: &Paths, j: &mut Journal) -> Result<usize, (usize, String)>`: for each file in order, check `file_hash` equals the planned hash (else `Err("<file> changed since the plan")`), apply all its actions to the text in memory, `j.write_file` once.

- [ ] **Step 1: Tests:** `add_writes_to_picked_agents_only`, `add_to_project_uses_claude_local_by_default`, `add_with_project_scope_warns_about_approval`, `copy_with_differing_copies_needs_from`, `copy_from_overwrites_drift`, `edit_inherited_targets_global_with_warning`, `remove_all_then_undo_restores_files`, `read_only_server_is_refused`, `secret_into_tracked_file_warns` (git init in the temp project), `plan_refuses_unreadable_file`, `apply_refuses_when_file_changed_since_plan`, `skipped_agent_gets_reason`.
- [ ] **Step 2–4:** FAIL, implement, PASS.
- [ ] **Step 5: Commit** `feat(mcp): plan and apply adds, copies, edits, removals and merges`

### Task 12: MCP changes from the CLI

**Files:**
- Modify: `src/cli/mcp.rs`

**Interfaces:**
- Consumes: Task 11.
- Produces: `mcp add NAME (--command CMD [--arg A]... [--env K=V]... | --url URL [--header K=V]...) --to AGENT... [--claude-scope local|project]` (immediate), `mcp copy NAME --to AGENT... [--from AGENT]`, `mcp edit NAME [--command] [--arg]... [--env K=V] [--unset-env K] [--url] [--header K=V] [--unset-header K]`, `mcp remove NAME [--from AGENT...]`, `mcp disable NAME` / `mcp enable NAME` (require `--project`), `mcp merge NAME... --into NEW` — all but `add` take `--apply [--plan-id ID]`.
- Values: `K=V` literal; `K=@path` reads the file (trailing newline trimmed); `K=-` reads stdin once; `K=${VAR}` is a reference.
- JSON like skills plans plus `"skipped":[{"agent","reason"}]`, `"warnings":[…]`.

- [ ] **Step 1: Tests:** `add_then_list_shows_server_in_both_agents`, `env_from_file_is_read_and_masked`, `copy_without_apply_changes_nothing`, `stale_plan_id_exits_4`, `disable_without_project_is_usage_error`, `errors_never_echo_values` (a failing add with a secret value: the secret appears in neither stdout nor stderr).
- [ ] **Step 2–4:** FAIL, implement, PASS.
- [ ] **Step 5: Commit** `feat(cli): add, copy, edit, remove, disable and merge MCP servers`

### Task 13: `prism doctor`

**Files:**
- Create: `src/cli/doctor.rs`

**Interfaces:**
- Produces: `doctor` (scope from `--project`, or `--all` for global plus every project in settings) reporting: broken skill links (`sync::broken_links`), skills whose `SKILL.md` lacks `name` or `description` (`skill::read`), MCP servers with status `Differs`, MCP `errors`. JSON `{"problems":[{"kind","scope","subject","detail"}]}`; exit 3 when any, 0 when none.

- [ ] **Step 1: Tests:** `clean_home_exits_0`, `broken_link_and_drift_exit_3`.
- [ ] **Step 2–4:** FAIL, implement, PASS.
- [ ] **Step 5: Commit** `feat(cli): prism doctor`

### Task 14: The MCP page in the window

**Files:**
- Create: `src/app/mcp.rs`, `src/ui/mcp.rs`
- Modify: `src/app/mod.rs` (`Page::Mcp`, fields), `src/app/watch.rs`, `src/ui/mod.rs` (toolbar switch, view dispatch), `src/main.rs` (scene `mcp`), `src/ui_tests.rs`

**Interfaces:**
- Consumes: Task 8.
- Produces:
  - `Page::Mcp`; `App` fields `mcp: McpInventory`, `mcp_selected: Option<String>`, `mcp_filter: McpFilter { All, Differs, Disabled, ReadOnly }`, `mcp_revealed: HashSet<String>` (servers whose values are shown).
  - `App::open_mcp()`, `App::select_mcp(&str)`, `App::rescan_mcp()`; `open_scope` keeps the current page (skills or MCP) and rescans both.
  - Toolbar segmented switch keyed `tab-skills` / `tab-mcp` on the scope pages. Rows keyed `mcp-row-<name>` with agent chips (like the skills agent column), status, inherited rows dimmed with a "global" tag, merge hints under the list. Inspector: transport, command/URL, env/header keys with masked values and a reveal toggle keyed `mcp-reveal`, per-agent copies, diff table.
  - The watcher also watches every MCP file of the scope (parent folder when missing) and `paths.state/last-change.json`; a change to the latter reloads `last_change`.
- Layout follows the existing pages; add the `mcp` scene to `SCENES`.

- [ ] **Step 1: Tests (`ui_tests.rs`):** `mcp_tab_lists_servers_with_agents` (Claude + Cursor fixture, click `tab-mcp`, `mcp-row-plata` exists), `inspector_masks_until_reveal`, `inherited_global_server_shows_in_project`, `window_picks_up_a_cli_change` (app on the MCP page; call `cli::run_with(["mcp","add",…])` against the same home; drive `poll` until the row appears, as in `a_skill_added_on_disk_shows_up_without_a_rescan`).
- [ ] **Step 2–4:** FAIL, implement, PASS. Render `cargo run -q -- --snap /tmp/mcp.png --scene mcp` and look at it.
- [ ] **Step 5: Commit** `feat(ui): MCP servers page`

### Task 15: MCP dialogs in the window

**Files:**
- Create: `src/ui/mcp_dialogs.rs`
- Modify: `src/app/mcp.rs`, `src/ui/mod.rs`, `src/main.rs` (scenes `mcp-add`, `mcp-copy`), `src/ui_tests.rs`

**Interfaces:**
- Consumes: Task 11.
- Produces: `App::open_mcp_add()`, `open_mcp_copy(name)`, `open_mcp_edit(name)`, `confirm_mcp_remove(name)`, `toggle_mcp_disabled(name)`, `open_mcp_merge(names)`, and `apply_mcp_plan()` which runs `mcp::plan::apply`, sets `last_change`, notifies like the skills sync, rescans.
  - Add dialog: name, stdio/HTTP switch, command + args or URL, env/header rows (add/remove), Claude Code scope picker shown only in a project (default "Only me"), agent checkboxes defaulting to `settings.targets` ∩ MCP agents. Keys `mcp-add-name`, `mcp-add-command`, `mcp-add-save`.
  - Copy/edit/remove/disable/merge open a plan dialog like `dialogs::sync`: actions, skipped agents with reasons, warnings, Apply button keyed `mcp-apply`. Copy shows a source picker when copies differ.

- [ ] **Step 1: Tests:** `add_dialog_writes_server_to_ticked_agents`, `copy_dialog_requires_source_when_copies_differ`, `remove_then_undo_from_toast`, `read_only_server_has_no_actions`.
- [ ] **Step 2–4:** FAIL, implement, PASS. Snap `mcp-add` and `mcp-copy`.
- [ ] **Step 5: Commit** `feat(ui): add, copy, edit, remove and merge MCP servers from the window`

### Task 16: `prism setup`, the prism skill, menu item, README

**Files:**
- Create: `assets/prism-skill/SKILL.md`, `src/cli/setup.rs`
- Modify: `src/main.rs` (app menu item "Install command line tool…" via Rune's native menu API), `README.md`, `scripts/demo-home.sh` (MCP configs in the demo home; `docs/mcp.png`), `scripts/package-linux.sh` (nothing, if `bin/prism` is already there — verify)

**Interfaces:**
- Produces:
  - `prism setup`: macOS — symlink `~/.local/bin/prism` → current executable when `~/.local/bin` is on `PATH`, else `/usr/local/bin/prism` if writable, else exit 1 telling the user the `ln -s` command; Linux — report whether `prism` on `PATH` is this binary. Then install the skill: write `assets/prism-skill/SKILL.md` (`include_str!`) to global `.agents/skills/prism/SKILL.md` when missing or when its `metadata.prism-version` is older than `CARGO_PKG_VERSION`, and link it into `settings.targets` with `sync::plan_one` + `apply`. `--json` reports `{cli, skill}`.
  - The skill content: when to use Prism (install/sync skills, add an MCP server to several agents, check drift), every command with one example, the rule "run without `--apply`, read the plan, then rerun with `--apply --plan-id <id>`", the JSON shapes from Tasks 4, 5, 9, 12, and "pass secrets with `K=@file` or `K=-`, never inline".
  - The menu item does the CLI part of `setup` and shows a notice with the path.
  - README: a "Command line" section, an "MCP servers" feature bullet and screenshot, the keyboard table unchanged.

- [ ] **Step 1: Tests:** `setup_installs_skill_and_links_it`, `setup_upgrades_older_skill_only`, `setup_reports_path_problem_without_failing_skill_part`.
- [ ] **Step 2–4:** FAIL, implement, PASS. Run `./scripts/demo-home.sh` and check `docs/mcp.png`.
- [ ] **Step 5: Commit** `feat: prism setup, the prism skill and docs for the CLI and MCP`

---

## Done when

- `cargo clippy --all-targets -- -D warnings` and `cargo test` pass locally and in CI.
- On this Mac: `prism mcp list` shows `plata` in four agents as in sync, `context7` as differs (header missing in Cursor), and a merge hint for the two Figma servers; nothing was written. A copy of `context7` from Claude Code to Cursor with `--apply` fixes the drift, and `prism undo` restores `~/.cursor/mcp.json` byte for byte. (Do this with George watching; it touches his real configs.)
