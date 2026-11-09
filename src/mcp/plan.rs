//! MCP changes as plans: what each request would write, file by file, shown before anything
//! is written, then applied through the journal so undo can take it back.

use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::agents::{Paths, Scope};
use crate::fsops::{Journal, content_hash};
use crate::mcp::agents::{Flavor, MCP_AGENTS};
use crate::mcp::formats;
use crate::mcp::model::{EnvValue, Extras, McpCopy, Place, Server, Transport};
use crate::mcp::scan::{McpInventory, McpServer, Status, alike};
use crate::planid;

const CLAUDE_RUNNING: &str = "quit Claude Code first, or a running session may write over this change";
const CLAUDE_APPROVAL: &str = "Claude Code will ask to approve it on its next start";
const GLOBAL_EDIT: &str = "edits the global server";

/// Where Claude Code keeps a new project server.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClaudeScope {
    /// `~/.claude.json` → `projects[<path>].mcpServers`: stays on this machine.
    #[default]
    Local,
    /// `<project>/.mcp.json`: shared with the team through the repository.
    Project,
}

/// One field change of an edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditChange {
    Command(String),
    Args(Vec<String>),
    SetEnv(String, EnvValue),
    UnsetEnv(String),
    Url(String),
    SetHeader(String, EnvValue),
    UnsetHeader(String),
}

/// What the user asked for. Agents are indices into `MCP_AGENTS`.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Add { server: Server, to: Vec<usize>, claude_scope: ClaudeScope },
    Copy { name: String, to: Vec<usize>, from: Option<usize> },
    Edit { name: String, changes: Vec<EditChange> },
    /// `from` empty: every copy in the scope.
    Remove { name: String, from: Vec<usize> },
    SetDisabled { name: String, disabled: bool },
    Merge { names: Vec<String>, into: String },
}

/// One write to one config file.
#[derive(Clone, Debug, PartialEq)]
pub enum McpAction {
    /// `replaces`: the agent has this server there already. `from`: the agent whose copy this
    /// is (a copy or an overwrite); `None` for an add or an edit.
    Write { agent: usize, file: PathBuf, place: Place, server: Server, extras: Extras, replaces: bool, from: Option<usize> },
    Delete { agent: usize, file: PathBuf, place: Place, name: String },
    Rename { agent: usize, file: PathBuf, place: Place, from: String, to: String },
    /// `project` is where the switch applies, `place` where the copy lives. `here`: `project`
    /// is the plan's own scope, so the plan says "this project".
    SetDisabled { agent: usize, file: PathBuf, project: PathBuf, place: Place, name: String, disabled: bool, here: bool },
}

impl McpAction {
    pub fn agent(&self) -> usize {
        match self {
            McpAction::Write { agent, .. } | McpAction::Delete { agent, .. } | McpAction::Rename { agent, .. } | McpAction::SetDisabled { agent, .. } => *agent,
        }
    }

    pub fn file(&self) -> &Path {
        match self {
            McpAction::Write { file, .. } | McpAction::Delete { file, .. } | McpAction::Rename { file, .. } | McpAction::SetDisabled { file, .. } => file,
        }
    }

    /// One line for the plan, naming servers and agents only: never a value.
    pub fn describe(&self) -> String {
        let (head, agent) = self.wording();
        format!("{head}{}", agent.unwrap_or_default())
    }

    /// The line split before the agent's name when it ends with it, so lines that differ only
    /// in the agent can share one line (`Edit plata in Claude Code, Codex`).
    fn wording(&self) -> (String, Option<&'static str>) {
        let name_of = |a: usize| MCP_AGENTS[a].name;
        let at_end = |head: String| (head, Some(name_of(self.agent())));
        match self {
            McpAction::Write { server, replaces: true, from: Some(src), agent, .. } => {
                (format!("Overwrite {}'s {} with {}'s", name_of(*agent), server.name, name_of(*src)), None)
            }
            McpAction::Write { server, replaces: true, from: None, .. } => at_end(format!("Edit {} in ", server.name)),
            McpAction::Write { server, replaces: false, .. } => at_end(format!("Add {} to ", server.name)),
            McpAction::Delete { name, .. } => at_end(format!("Remove {name} from ")),
            McpAction::Rename { from, to, .. } => at_end(format!("Rename {from} to {to} in ")),
            McpAction::SetDisabled { project, name, disabled, here, .. } => {
                let verb = if *disabled { "Disable" } else { "Enable" };
                let place = if *here { "this project".to_string() } else { project.display().to_string() };
                at_end(format!("{verb} {name} for {place} in "))
            }
        }
    }

    /// The writer behind the action, run on `text`, the file as it is now.
    fn run(&self, text: &str) -> Result<String, String> {
        let flavor = MCP_AGENTS[self.agent()].flavor;
        match self {
            McpAction::Write { place, server, extras, .. } => formats::upsert(flavor, text, place, server, extras),
            McpAction::Delete { place, name, .. } => formats::remove(flavor, text, place, name),
            McpAction::Rename { place, from, to, .. } => formats::rename(flavor, text, place, from, to),
            McpAction::SetDisabled { project, place, name, disabled, .. } => formats::set_disabled(flavor, text, project, place, name, *disabled),
        }
    }

    /// The section a parse check reads: Claude Code's switches live in `~/.claude.json`,
    /// whichever file the copy is in.
    fn read_place(&self) -> Place {
        match self {
            McpAction::SetDisabled { agent, .. } if MCP_AGENTS[*agent].flavor == Flavor::ClaudeCode => Place::Global,
            McpAction::Write { place, .. } | McpAction::Delete { place, .. } | McpAction::Rename { place, .. } | McpAction::SetDisabled { place, .. } => place.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpPlan {
    pub actions: Vec<McpAction>,
    /// Agents left out, with the reason.
    pub skipped: Vec<(usize, String)>,
    pub warnings: Vec<String>,
    /// Every file the plan writes, with its hash when planned (`None`: it doesn't exist yet).
    pub files: Vec<(PathBuf, Option<u64>)>,
}

impl McpPlan {
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// The plan as shown: lines that differ only in the agent share one line
    /// (`Edit plata in Claude Code, Codex`), in the order they first come up.
    pub fn lines(&self) -> Vec<String> {
        let mut out: Vec<(String, Vec<&str>)> = Vec::new();
        for action in &self.actions {
            let (head, agent) = action.wording();
            let agents: Vec<&str> = agent.into_iter().collect();
            match out.iter_mut().find(|(h, a)| *h == head && a.is_empty() == agents.is_empty()) {
                Some((_, known)) => known.extend(agents.into_iter().filter(|a| !known.contains(a)).collect::<Vec<_>>()),
                None => out.push((head, agents)),
            }
        }
        out.into_iter().map(|(head, agents)| format!("{head}{}", agents.join(", "))).collect()
    }

    /// Changes when what the plan does or the files it was made from change, so
    /// `--apply --plan-id` refuses a stale plan. Values never enter it, only names and paths.
    pub fn id(&self) -> String {
        let mut lines: Vec<String> = self.actions.iter().map(|a| format!("{} @ {}", a.describe(), a.file().display())).collect();
        lines.extend(self.skipped.iter().map(|(a, why)| format!("skip {}: {why}", MCP_AGENTS[*a].name)));
        planid::plan_id(&lines, &self.files)
    }
}

/// What `req` would write in `inv`'s scope. Refuses read-only servers and files the scan
/// couldn't read; agents whose config can't hold the server go to `skipped` with the reason.
pub fn plan(inv: &McpInventory, req: &Request, paths: &Paths) -> Result<McpPlan, String> {
    let mut p = Planner { inv, paths, texts: HashMap::new(), plan: McpPlan::default() };
    match req {
        Request::Add { server, to, claude_scope } => p.add(server, to, *claude_scope)?,
        Request::Copy { name, to, from } => p.copy(name, to, *from)?,
        Request::Edit { name, changes } => p.edit(name, changes)?,
        Request::Remove { name, from } => p.remove(name, from)?,
        Request::SetDisabled { name, disabled } => p.set_disabled(name, *disabled)?,
        Request::Merge { names, into } => p.merge(names, into)?,
    }
    Ok(p.plan)
}

/// Writes the plan, one file at a time in plan order: each file is re-read, checked against
/// its planned hash, edited in memory by all of its actions and written once through `j`.
/// Returns the files written; on failure, how many were written before it (undo takes them back).
pub fn apply(plan: &McpPlan, paths: &Paths, j: &mut Journal) -> Result<usize, (usize, String)> {
    let mut files: Vec<&Path> = Vec::new();
    for action in &plan.actions {
        if !files.contains(&action.file()) {
            files.push(action.file());
        }
    }
    let mut written = 0;
    for file in files {
        // The bytes checked are the bytes edited, so nothing slips in between.
        let bytes = match std::fs::read(file) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == ErrorKind::NotFound => None,
            Err(e) => return Err((written, cant_read(file, &e.to_string()))),
        };
        let planned = plan.files.iter().find(|(p, _)| p == file).map(|(_, h)| *h);
        if planned != Some(bytes.as_deref().map(content_hash)) {
            return Err((written, format!("{} changed since the plan", file.display())));
        }
        let text = String::from_utf8(bytes.unwrap_or_default()).map_err(|_| (written, cant_read(file, "not UTF-8 text")))?;
        let mut out = text.clone();
        for action in plan.actions.iter().filter(|a| a.file() == file) {
            out = action.run(&out).map_err(|e| (written, format!("{}: {e}", action.describe())))?;
        }
        if out == text && file.exists() {
            continue;
        }
        j.write_file(file, out.as_bytes(), paths).map_err(|e| (written, format!("can't write {}: {e}", file.display())))?;
        written += 1;
    }
    Ok(written)
}

fn cant_read(file: &Path, why: &str) -> String {
    format!("can't read {}: {why}", file.display())
}

fn agent_name(agent: usize) -> &'static str {
    MCP_AGENTS[agent].name
}

/// Claude Code's `~/.claude.json` (or `$CLAUDE_CONFIG_DIR/.claude.json`).
fn claude_json(paths: &Paths) -> PathBuf {
    let claude = MCP_AGENTS.iter().find(|a| a.flavor == Flavor::ClaudeCode).expect("Claude Code is an MCP agent");
    claude.files(&Scope::Global, paths).remove(0).0
}

/// A copy the scope owns: everything in the global scope, the project's own copies in a project.
fn in_scope(scope: &Scope, place: &Place) -> bool {
    scope.is_global() || *place != Place::Global
}

fn unique(agents: &[usize]) -> Vec<usize> {
    let mut out = Vec::new();
    for &a in agents {
        if !out.contains(&a) {
            out.push(a);
        }
    }
    out
}

/// App-managed servers are the app's to change.
fn editable(server: &McpServer) -> Result<(), String> {
    if server.status != Status::ReadOnly {
        return Ok(());
    }
    let app = server.copies.iter().find_map(|c| c.read_only.clone()).unwrap_or_else(|| "an app".into());
    Err(format!("{} is managed by {app}; Prism leaves it alone", server.name))
}

/// A copy's extras only fit an agent of the same dialect.
fn extras_for(source: &McpCopy, target: usize) -> Extras {
    if MCP_AGENTS[source.agent].flavor == MCP_AGENTS[target].flavor { source.extras.clone() } else { Extras::new() }
}

/// A value written as is, not as a reference: possibly a token.
fn has_literal(server: &Server) -> bool {
    match &server.transport {
        Transport::Stdio { env, .. } => env.values().any(EnvValue::is_literal),
        Transport::Http { headers, .. } => headers.values().any(EnvValue::is_literal),
    }
}

/// Whether git would pick `file` up: tracked, or inside a work tree and not ignored. Paths go
/// to git relative to `root`, since temp and symlinked folders confuse its absolute-path checks.
/// Without git there's nothing to warn about.
fn in_git(root: &Path, file: &Path) -> bool {
    let rel = file.strip_prefix(root).unwrap_or(file);
    let git = |args: &[&str], with_file: bool| {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(root).args(args);
        if with_file {
            cmd.arg("--").arg(rel);
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().ok()
    };
    if git(&["ls-files", "--error-unmatch"], true).is_some_and(|s| s.success()) {
        return true;
    }
    let inside = git(&["rev-parse", "--is-inside-work-tree"], false).is_some_and(|s| s.success());
    // `check-ignore` exits 1 for "not ignored"; 128 is an error, not an answer.
    inside && git(&["check-ignore", "-q"], true).is_some_and(|s| s.code() == Some(1))
}

/// Projects in `~/.claude.json` whose `disabledMcpServers` lists `name` but not `into`.
fn projects_disabling(text: &str, name: &str, into: &str) -> Vec<PathBuf> {
    let Ok(doc) = jsonc_parser::parse_to_serde_value::<Value>(text, &Default::default()) else { return Vec::new() };
    let Some(projects) = doc.get("projects").and_then(Value::as_object) else { return Vec::new() };
    let lists = |v: &Value, n: &str| v.get("disabledMcpServers").and_then(Value::as_array).is_some_and(|l| l.iter().any(|x| x.as_str() == Some(n)));
    projects.iter().filter(|(_, v)| lists(v, name) && !lists(v, into)).map(|(k, _)| PathBuf::from(k)).collect()
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    a == b || real(a) == real(b)
}

/// The changes `changes` make to `t`. A change for the other kind of server is an error.
fn edited(t: &Transport, changes: &[EditChange], name: &str) -> Result<Transport, String> {
    let mut t = t.clone();
    for change in changes {
        match (&mut t, change) {
            (Transport::Stdio { command, .. }, EditChange::Command(c)) => *command = c.clone(),
            (Transport::Stdio { args, .. }, EditChange::Args(a)) => *args = a.clone(),
            (Transport::Stdio { env, .. }, EditChange::SetEnv(k, v)) => {
                env.insert(k.clone(), v.clone());
            }
            (Transport::Stdio { env, .. }, EditChange::UnsetEnv(k)) => {
                env.remove(k);
            }
            (Transport::Http { url, .. }, EditChange::Url(u)) => *url = u.clone(),
            // Header names are case-insensitive: the new spelling replaces any other.
            (Transport::Http { headers, .. }, EditChange::SetHeader(k, v)) => {
                headers.retain(|h, _| !h.eq_ignore_ascii_case(k));
                headers.insert(k.clone(), v.clone());
            }
            (Transport::Http { headers, .. }, EditChange::UnsetHeader(k)) => headers.retain(|h, _| !h.eq_ignore_ascii_case(k)),
            (Transport::Stdio { .. }, _) => return Err(format!("{name} runs a command; it has no URL or headers")),
            (Transport::Http { .. }, _) => return Err(format!("{name} is a remote server; it has no command, args or env")),
        }
    }
    Ok(t)
}

/// Builds a plan, running each action's writer on the files as the plan so far leaves them.
struct Planner<'a> {
    inv: &'a McpInventory,
    paths: &'a Paths,
    texts: HashMap<PathBuf, String>,
    plan: McpPlan,
}

impl<'a> Planner<'a> {
    /// The file's text as the plan so far leaves it; a missing file reads as empty. A file the
    /// scan couldn't read stops the plan: writing it could lose what Prism can't see.
    fn text(&mut self, file: &Path) -> Result<String, String> {
        if let Some((_, why)) = self.inv.errors.iter().find(|(p, _)| p == file) {
            return Err(cant_read(file, why));
        }
        if let Some(text) = self.texts.get(file) {
            return Ok(text.clone());
        }
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
            Err(e) => return Err(cant_read(file, &e.to_string())),
        };
        self.texts.insert(file.into(), text.clone());
        Ok(text)
    }

    fn skip(&mut self, agent: usize, reason: String) {
        if !self.plan.skipped.iter().any(|(a, r)| *a == agent && *r == reason) {
            self.plan.skipped.push((agent, reason));
        }
    }

    fn warn(&mut self, warning: impl Into<String>) {
        let warning = warning.into();
        if !self.plan.warnings.contains(&warning) {
            self.plan.warnings.push(warning);
        }
    }

    /// Adds `action` if its writer takes it; otherwise the agent is skipped with the writer's
    /// reason (it can't express a value, say). Returns whether it was added.
    fn push(&mut self, action: McpAction) -> Result<bool, String> {
        let file = action.file().to_path_buf();
        let text = self.text(&file)?;
        formats::read(MCP_AGENTS[action.agent()].flavor, &text, &action.read_place()).map_err(|e| cant_read(&file, &e))?;
        let out = match action.run(&text) {
            Ok(out) => out,
            Err(why) => {
                self.skip(action.agent(), why);
                return Ok(false);
            }
        };
        self.texts.insert(file.clone(), out);
        if !self.plan.files.iter().any(|(p, _)| *p == file) {
            self.plan.files.push((file.clone(), planid::file_hash(&file)));
        }
        if file == claude_json(self.paths) {
            self.warn(CLAUDE_RUNNING);
        }
        if let McpAction::Write { place: Place::Project(root), server, .. } = &action
            && has_literal(server)
            && in_git(root, &file)
        {
            self.warn(format!("{} is in git; the token would be committed", file.display()));
        }
        self.plan.actions.push(action);
        Ok(true)
    }

    fn server(&self, name: &str) -> Result<&'a McpServer, String> {
        self.inv.get(name).ok_or_else(|| format!("no server {name}"))
    }

    /// Where a new server for `agent` goes, or `None` (skipped with the reason). Claude Code
    /// projects use the local scope unless `.mcp.json` was asked for.
    fn target(&mut self, agent: usize, claude_scope: ClaudeScope) -> Option<(PathBuf, Place)> {
        let a = &MCP_AGENTS[agent];
        let scope = &self.inv.scope;
        if !a.detected(self.paths) {
            self.skip(agent, format!("{} isn't installed", a.name));
            return None;
        }
        let files = a.files(scope, self.paths);
        let found = match (a.flavor, scope, claude_scope) {
            (Flavor::OpenCode, ..) => Some(a.new_server_file(scope, self.paths)),
            (Flavor::ClaudeCode, Scope::Project(_), ClaudeScope::Project) => files.into_iter().find(|(_, p)| matches!(p, Place::Project(_))),
            _ if files.is_empty() => None,
            _ => Some(a.new_server_file(scope, self.paths)),
        };
        if found.is_none() {
            self.skip(agent, format!("{} has no project config", a.name));
        }
        found
    }

    fn add(&mut self, server: &Server, to: &[usize], claude_scope: ClaudeScope) -> Result<(), String> {
        if to.is_empty() {
            return Err("pick at least one agent".into());
        }
        if let Some(app) = crate::mcp::model::read_only(server) {
            return Err(format!("{} is managed by {app}; Prism leaves it alone", server.name));
        }
        let scope = &self.inv.scope;
        let existing = self.inv.get(&server.name);
        if let Some(s) = existing {
            editable(s)?;
        }
        for agent in unique(to) {
            if existing.is_some_and(|s| s.copies.iter().any(|c| c.agent == agent && in_scope(scope, &c.place))) {
                self.skip(agent, format!("{} already has {}", agent_name(agent), server.name));
                continue;
            }
            let Some((file, place)) = self.target(agent, claude_scope) else { continue };
            let shared = MCP_AGENTS[agent].flavor == Flavor::ClaudeCode && matches!(place, Place::Project(_));
            let write = McpAction::Write { agent, file, place, server: server.clone(), extras: Extras::new(), replaces: false, from: None };
            if self.push(write)? && shared {
                self.warn(CLAUDE_APPROVAL);
            }
        }
        Ok(())
    }

    /// Writes `name` to the agents in `to` that lack it. With `from`, that agent's copy is the
    /// source and every differing copy in the scope is overwritten with it.
    fn copy(&mut self, name: &str, to: &[usize], from: Option<usize>) -> Result<(), String> {
        let s = self.server(name)?;
        editable(s)?;
        let scope = &self.inv.scope;
        let source = match from {
            Some(f) => s.copies.iter().find(|c| c.agent == f).ok_or_else(|| format!("{} has no copy of {name}", agent_name(f)))?,
            None if s.status == Status::Differs => return Err("copies differ; pick one with --from".into()),
            None => &s.copies[0],
        };
        if from.is_some() {
            for c in s.copies.iter().filter(|c| in_scope(scope, &c.place) && !alike(&c.server.transport, &source.server.transport)) {
                let extras = extras_for(source, c.agent);
                let server = Server { name: name.into(), transport: source.server.transport.clone() };
                self.push(McpAction::Write { agent: c.agent, file: c.file.clone(), place: c.place.clone(), server, extras, replaces: true, from: Some(source.agent) })?;
            }
        }
        for agent in unique(to) {
            if s.copies.iter().any(|c| c.agent == agent) {
                continue;
            }
            let Some((file, place)) = self.target(agent, ClaudeScope::Local) else { continue };
            let extras = extras_for(source, agent);
            self.push(McpAction::Write { agent, file, place, server: source.server.clone(), extras, replaces: false, from: Some(source.agent) })?;
        }
        Ok(())
    }

    /// Applies `changes` to every copy, each keeping its other fields. An inherited row's copies
    /// are the global ones, and the plan says so.
    fn edit(&mut self, name: &str, changes: &[EditChange]) -> Result<(), String> {
        let s = self.server(name)?;
        editable(s)?;
        if changes.is_empty() {
            return Err("nothing to change".into());
        }
        for c in &s.copies {
            let transport = edited(&c.server.transport, changes, name)?;
            if transport == c.server.transport {
                continue;
            }
            let server = Server { name: name.into(), transport };
            let write = McpAction::Write { agent: c.agent, file: c.file.clone(), place: c.place.clone(), server, extras: Extras::new(), replaces: true, from: None };
            if self.push(write)? && !self.inv.scope.is_global() && c.place == Place::Global {
                self.warn(GLOBAL_EDIT);
            }
        }
        Ok(())
    }

    /// Removes the scope's own copies; a global server is removed from the global scope.
    fn remove(&mut self, name: &str, from: &[usize]) -> Result<(), String> {
        let s = self.server(name)?;
        editable(s)?;
        let scope = &self.inv.scope;
        if s.inherited {
            return Err(format!("{name} comes from the global configs; remove it there, or disable it for this project"));
        }
        for agent in unique(from) {
            if !s.copies.iter().any(|c| c.agent == agent && in_scope(scope, &c.place)) {
                self.skip(agent, format!("{} has no copy of {name}", agent_name(agent)));
            }
        }
        for c in s.copies.iter().filter(|c| in_scope(scope, &c.place) && (from.is_empty() || from.contains(&c.agent))) {
            self.push(McpAction::Delete { agent: c.agent, file: c.file.clone(), place: c.place.clone(), name: name.into() })?;
        }
        Ok(())
    }

    /// Claude Code switches servers per project in `~/.claude.json`; OpenCode and Codex only
    /// through a copy in the project's own config. Never a global entry's own switch.
    fn set_disabled(&mut self, name: &str, disabled: bool) -> Result<(), String> {
        let Scope::Project(root) = &self.inv.scope else {
            return Err("disable and enable apply to a project; pick one with --project".into());
        };
        let s = self.server(name)?;
        let claude_file = claude_json(self.paths);
        for c in &s.copies {
            let file = match (MCP_AGENTS[c.agent].flavor, &c.place) {
                (Flavor::ClaudeCode, place) => {
                    let lists = formats::disabled_in(&self.text(&claude_file)?, root);
                    let list = if matches!(place, Place::Project(_)) { lists.mcp_json } else { lists.servers };
                    if list.iter().any(|n| n == name) == disabled {
                        continue;
                    }
                    claude_file.clone()
                }
                (Flavor::OpenCode | Flavor::Codex, Place::Project(_)) if c.enabled != disabled => continue,
                (Flavor::OpenCode | Flavor::Codex, Place::Project(_)) => c.file.clone(),
                _ => {
                    self.skip(c.agent, format!("{} has no per-project switch", agent_name(c.agent)));
                    continue;
                }
            };
            let action = McpAction::SetDisabled { agent: c.agent, file, project: root.clone(), place: c.place.clone(), name: name.into(), disabled, here: true };
            self.push(action)?;
        }
        Ok(())
    }

    /// Renames the scope's copies of `names` to `into`. Claude Code's per-project switches
    /// follow the new name.
    fn merge(&mut self, names: &[String], into: &str) -> Result<(), String> {
        if names.is_empty() {
            return Err("name the servers to merge".into());
        }
        let mut servers: Vec<&McpServer> = Vec::new();
        for name in names {
            let s = self.server(name)?;
            editable(s)?;
            if !servers.iter().any(|known| known.name == s.name) {
                servers.push(s);
            }
        }
        let scope = &self.inv.scope;
        let mut taken: HashSet<(PathBuf, Place)> = HashSet::new();
        if let Some(s) = self.inv.get(into) {
            editable(s)?;
            taken.extend(s.copies.iter().filter(|c| in_scope(scope, &c.place)).map(|c| (c.file.clone(), c.place.clone())));
        }
        for s in servers.into_iter().filter(|s| s.name != into) {
            for c in s.copies.iter().filter(|c| in_scope(scope, &c.place)) {
                if !taken.insert((c.file.clone(), c.place.clone())) {
                    return Err(format!("{} already has {into}", agent_name(c.agent)));
                }
                let rename = McpAction::Rename { agent: c.agent, file: c.file.clone(), place: c.place.clone(), from: s.name.clone(), to: into.into() };
                if self.push(rename)? && MCP_AGENTS[c.agent].flavor == Flavor::ClaudeCode {
                    self.carry_disabled(c, &s.name, into)?;
                }
            }
        }
        Ok(())
    }

    /// Lists `into` wherever Claude Code skipped the renamed copy: every project disabling a
    /// user-scope name, the copy's own project for a local or `.mcp.json` one.
    fn carry_disabled(&mut self, c: &McpCopy, old: &str, into: &str) -> Result<(), String> {
        let file = claude_json(self.paths);
        let text = self.text(&file)?;
        let in_list = |root: &Path, mcp_json: bool| {
            let lists = formats::disabled_in(&text, root);
            let list = if mcp_json { lists.mcp_json } else { lists.servers };
            list.iter().any(|n| n == old) && !list.iter().any(|n| n == into)
        };
        let projects = match &c.place {
            Place::Global => projects_disabling(&text, old, into),
            Place::ClaudeLocal(root) => if in_list(root, false) { vec![root.clone()] } else { vec![] },
            Place::Project(root) => if in_list(root, true) { vec![root.clone()] } else { vec![] },
        };
        for project in projects {
            let here = matches!(&self.inv.scope, Scope::Project(root) if same_dir(root, &project));
            self.push(McpAction::SetDisabled { agent: c.agent, file: file.clone(), project, place: c.place.clone(), name: into.into(), disabled: true, here })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::process::Command;

    use serde_json::json;

    use super::*;
    use crate::agents::Scope;
    use crate::mcp::agents::{MCP_AGENTS, index_of};
    use crate::mcp::scan::{Status, scan};
    use crate::mcp::model::Transport;

    fn agent(id: &str) -> usize {
        index_of(id).unwrap()
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    /// A throwaway home with Claude Code, Cursor and Gemini CLI installed.
    fn home() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::isolated(dir.path());
        std::fs::create_dir_all(&paths.claude).unwrap();
        std::fs::create_dir_all(dir.path().join(".cursor")).unwrap();
        std::fs::create_dir_all(dir.path().join(".gemini")).unwrap();
        (dir, paths)
    }

    fn claude_json(paths: &Paths) -> PathBuf {
        paths.home.join(".claude.json")
    }

    fn cursor_json(paths: &Paths) -> PathBuf {
        paths.home.join(".cursor/mcp.json")
    }

    fn gemini_json(paths: &Paths) -> PathBuf {
        paths.home.join(".gemini/settings.json")
    }

    fn project(paths: &Paths) -> PathBuf {
        let root = paths.home.join("work/app");
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn stdio(name: &str, env: &[(&str, EnvValue)]) -> Server {
        let env: BTreeMap<String, EnvValue> = env.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        Server { name: name.into(), transport: Transport::Stdio { command: "npx".into(), args: vec!["-y".into(), format!("{name}-mcp")], env, cwd: None } }
    }

    fn add(server: Server, to: &[&str]) -> Request {
        Request::Add { server, to: to.iter().map(|id| agent(id)).collect(), claude_scope: ClaudeScope::Local }
    }

    fn lines(plan: &McpPlan) -> Vec<String> {
        plan.actions.iter().map(McpAction::describe).collect()
    }

    fn run(plan: &McpPlan, paths: &Paths) -> Journal {
        let mut j = Journal::default();
        apply(plan, paths, &mut j).unwrap();
        j
    }

    const CONTEXT7_CLAUDE: &str = r#"{"type": "http", "url": "https://mcp.context7.com/mcp", "headers": {"CONTEXT7_API_KEY": "${CTX7}"}}"#;

    #[test]
    fn add_writes_to_picked_agents_only() {
        let (_dir, paths) = home();
        write(&claude_json(&paths), "{\n  \"numStartups\": 3,\n  \"mcpServers\": {}\n}\n");
        let inv = scan(&Scope::Global, &paths);
        let plan = plan(&inv, &add(stdio("plata", &[("PLATA_TOKEN", EnvValue::var("PLATA_TOKEN"))]), &["claude", "cursor"]), &paths).unwrap();
        assert_eq!(lines(&plan), ["Add plata to Claude Code", "Add plata to Cursor"]);
        assert_eq!(plan.files.iter().map(|f| f.0.clone()).collect::<Vec<_>>(), [claude_json(&paths), cursor_json(&paths)]);
        assert_eq!(plan.files[1].1, None, "Cursor's file doesn't exist yet");
        assert_eq!(run(&plan, &paths).steps.len(), 2);

        assert!(!gemini_json(&paths).exists(), "Gemini wasn't picked");
        assert!(read(&claude_json(&paths)).contains("\"numStartups\": 3"));
        let after = scan(&Scope::Global, &paths);
        let plata = after.get("plata").unwrap();
        assert_eq!(plata.copies.iter().map(|c| c.agent).collect::<Vec<_>>(), [agent("claude"), agent("cursor")]);
        assert_eq!(plata.status, Status::InSync);
    }

    #[test]
    fn add_to_project_uses_claude_local_by_default() {
        let (_dir, paths) = home();
        let root = project(&paths);
        let inv = scan(&Scope::Project(root.clone()), &paths);
        let plan = plan(&inv, &add(stdio("plata", &[]), &["claude"]), &paths).unwrap();
        let McpAction::Write { file, place, replaces, .. } = &plan.actions[0] else { panic!("a write") };
        assert_eq!((file, place, *replaces), (&claude_json(&paths), &Place::ClaudeLocal(root.clone()), false));
        assert!(plan.warnings.iter().any(|w| w == "quit Claude Code first, or a running session may write over this change"), "{:?}", plan.warnings);
        assert!(!plan.warnings.iter().any(|w| w.contains("approve")));
        run(&plan, &paths);

        assert!(!root.join(".mcp.json").exists());
        let after = scan(&Scope::Project(root.clone()), &paths);
        assert_eq!(after.get("plata").unwrap().copies[0].place, Place::ClaudeLocal(root));
    }

    #[test]
    fn add_with_project_scope_warns_about_approval() {
        let (_dir, paths) = home();
        let root = project(&paths);
        let inv = scan(&Scope::Project(root.clone()), &paths);
        let req = Request::Add { server: stdio("plata", &[]), to: vec![agent("claude")], claude_scope: ClaudeScope::Project };
        let plan = plan(&inv, &req, &paths).unwrap();
        let McpAction::Write { file, place, .. } = &plan.actions[0] else { panic!("a write") };
        assert_eq!((file, place), (&root.join(".mcp.json"), &Place::Project(root.clone())));
        assert!(plan.warnings.iter().any(|w| w == "Claude Code will ask to approve it on its next start"), "{:?}", plan.warnings);
        run(&plan, &paths);

        assert!(read(&root.join(".mcp.json")).contains("plata"));
        assert!(!claude_json(&paths).exists(), "Prism never writes the approval lists");
    }

    #[test]
    fn copy_with_differing_copies_needs_from() {
        let (_dir, paths) = home();
        write(&claude_json(&paths), &format!(r#"{{"mcpServers": {{"context7": {CONTEXT7_CLAUDE}}}}}"#));
        write(&cursor_json(&paths), r#"{"mcpServers": {"context7": {"url": "https://mcp.context7.com/mcp"}}}"#);
        let inv = scan(&Scope::Global, &paths);
        let req = Request::Copy { name: "context7".into(), to: vec![agent("gemini")], from: None };
        assert_eq!(plan(&inv, &req, &paths), Err("copies differ; pick one with --from".into()));
    }

    #[test]
    fn copy_from_overwrites_drift() {
        let (_dir, paths) = home();
        write(&claude_json(&paths), &format!(r#"{{"mcpServers": {{"context7": {CONTEXT7_CLAUDE}}}}}"#));
        write(&cursor_json(&paths), r#"{"mcpServers": {"context7": {"url": "https://mcp.context7.com/mcp", "disabled": false}}}"#);
        let inv = scan(&Scope::Global, &paths);
        let req = Request::Copy { name: "context7".into(), to: vec![agent("gemini")], from: Some(agent("claude")) };
        let plan = plan(&inv, &req, &paths).unwrap();
        assert_eq!(lines(&plan), ["Overwrite Cursor's context7 with Claude Code's", "Add context7 to Gemini CLI"]);
        assert!(matches!(plan.actions[0], McpAction::Write { replaces: true, .. }));
        run(&plan, &paths);

        let after = scan(&Scope::Global, &paths);
        let context7 = after.get("context7").unwrap();
        assert_eq!(context7.copies.len(), 3);
        assert_eq!(context7.status, Status::InSync);
        assert!(read(&cursor_json(&paths)).contains("\"disabled\": false"), "Cursor's own fields stay");
    }

    #[test]
    fn edit_inherited_targets_global_with_warning() {
        let (_dir, paths) = home();
        write(&claude_json(&paths), r#"{"mcpServers": {"plata": {"command": "npx", "args": ["plata-mcp"], "env": {"MODE": "dev"}}}}"#);
        let root = project(&paths);
        let inv = scan(&Scope::Project(root), &paths);
        assert!(inv.get("plata").unwrap().inherited);
        let req = Request::Edit { name: "plata".into(), changes: vec![EditChange::SetEnv("MODE".into(), EnvValue::literal("prod"))] };
        let plan = plan(&inv, &req, &paths).unwrap();
        assert_eq!(lines(&plan), ["Edit plata in Claude Code"]);
        let McpAction::Write { file, place, extras, .. } = &plan.actions[0] else { panic!("a write") };
        assert_eq!((file, place), (&claude_json(&paths), &Place::Global));
        assert!(extras.is_empty());
        assert!(plan.warnings.iter().any(|w| w == "edits the global server"), "{:?}", plan.warnings);
        run(&plan, &paths);

        let global = scan(&Scope::Global, &paths);
        let Transport::Stdio { env, .. } = &global.get("plata").unwrap().copies[0].server.transport else { panic!("stdio") };
        assert_eq!(env["MODE"], EnvValue::literal("prod"));
    }

    #[test]
    fn remove_all_then_undo_restores_files() {
        let (_dir, paths) = home();
        let claude = "{\n    \"mcpServers\": {\n        \"plata\": {\"command\": \"npx\", \"args\": [\"plata-mcp\"]},\n        \"keep\": {\"command\": \"keep\"}\n    }\n}\n";
        let cursor = "{\n  \"mcpServers\": {\n    \"plata\": {\"command\": \"npx\", \"args\": [\"plata-mcp\"]}\n  }\n}";
        write(&claude_json(&paths), claude);
        write(&cursor_json(&paths), cursor);
        let inv = scan(&Scope::Global, &paths);
        let plan = plan(&inv, &Request::Remove { name: "plata".into(), from: vec![] }, &paths).unwrap();
        assert_eq!(lines(&plan), ["Remove plata from Claude Code", "Remove plata from Cursor"]);
        crate::store::begin_change(&paths);
        let journal = run(&plan, &paths);
        assert!(scan(&Scope::Global, &paths).get("plata").is_none());
        assert!(read(&claude_json(&paths)).contains("\"keep\""));

        journal.undo(&paths).unwrap();
        assert_eq!(read(&claude_json(&paths)), claude);
        assert_eq!(read(&cursor_json(&paths)), cursor);
    }

    #[test]
    fn read_only_server_is_refused() {
        let (_dir, paths) = home();
        write(&claude_json(&paths), r#"{"mcpServers": {"pencil": {"command": "/Applications/Pencil.app/Contents/Resources/mcp-server"}}}"#);
        let inv = scan(&Scope::Global, &paths);
        let requests = [
            Request::Copy { name: "pencil".into(), to: vec![agent("cursor")], from: None },
            Request::Edit { name: "pencil".into(), changes: vec![EditChange::Args(vec!["--x".into()])] },
            Request::Remove { name: "pencil".into(), from: vec![] },
            Request::Merge { names: vec!["pencil".into()], into: "pen".into() },
        ];
        for req in requests {
            let err = plan(&inv, &req, &paths).unwrap_err();
            assert!(err.contains("Pencil"), "{req:?}: {err}");
        }
    }

    fn git_available() -> bool {
        Command::new("git").arg("--version").output().is_ok_and(|o| o.status.success())
    }

    #[test]
    fn secret_into_tracked_file_warns() {
        if !git_available() {
            eprintln!("git isn't installed; skipping");
            return;
        }
        let (_dir, paths) = home();
        let root = project(&paths);
        assert!(Command::new("git").arg("-C").arg(&root).args(["init", "-q"]).status().unwrap().success());
        let inv = scan(&Scope::Project(root.clone()), &paths);
        let in_git = |plan: &McpPlan| plan.warnings.iter().any(|w| w.ends_with("is in git; the token would be committed"));

        let literal = plan(&inv, &add(stdio("plata", &[("PLATA_TOKEN", EnvValue::literal("fake-token"))]), &["cursor"]), &paths).unwrap();
        assert!(in_git(&literal), "{:?}", literal.warnings);
        assert!(literal.warnings.iter().all(|w| !w.contains("fake-token")), "warnings carry no values");

        let reference = plan(&inv, &add(stdio("plata", &[("PLATA_TOKEN", EnvValue::var("PLATA_TOKEN"))]), &["cursor"]), &paths).unwrap();
        assert!(!in_git(&reference), "a reference is no secret");

        let local = plan(&inv, &add(stdio("plata", &[("PLATA_TOKEN", EnvValue::literal("fake-token"))]), &["claude"]), &paths).unwrap();
        assert!(!in_git(&local), "Claude's local scope stays in ~/.claude.json");

        write(&root.join(".gitignore"), ".cursor/\n");
        let ignored = plan(&inv, &add(stdio("plata", &[("PLATA_TOKEN", EnvValue::literal("fake-token"))]), &["cursor"]), &paths).unwrap();
        assert!(!in_git(&ignored), "{:?}", ignored.warnings);

        write(&root.join(".cursor/mcp.json"), "{}");
        assert!(Command::new("git").arg("-C").arg(&root).args(["add", "-f", ".cursor/mcp.json"]).status().unwrap().success());
        let tracked = plan(&inv, &add(stdio("plata", &[("PLATA_TOKEN", EnvValue::literal("fake-token"))]), &["cursor"]), &paths).unwrap();
        assert!(in_git(&tracked), "a tracked file warns even when ignored: {:?}", tracked.warnings);
    }

    #[test]
    fn lines_join_agents_that_share_a_line() {
        let (_dir, paths) = home();
        write(&claude_json(&paths), r#"{"mcpServers": {"plata": {"command": "npx", "args": ["plata-mcp"]}}}"#);
        write(&cursor_json(&paths), r#"{"mcpServers": {"plata": {"command": "npx", "args": ["plata-mcp"]}}}"#);
        let inv = scan(&Scope::Global, &paths);
        let req = Request::Edit { name: "plata".into(), changes: vec![EditChange::Args(vec!["plata-mcp@2".into()])] };
        let plan = plan(&inv, &req, &paths).unwrap();
        assert_eq!(lines(&plan), ["Edit plata in Claude Code", "Edit plata in Cursor"]);
        assert_eq!(plan.lines(), ["Edit plata in Claude Code, Cursor"]);

        let wrong = Request::Edit { name: "plata".into(), changes: vec![EditChange::Url("https://x.example.com".into())] };
        assert!(super::plan(&inv, &wrong, &paths).unwrap_err().contains("no URL"));
        let same = Request::Edit { name: "plata".into(), changes: vec![EditChange::Args(vec!["plata-mcp".into()])] };
        assert!(super::plan(&inv, &same, &paths).unwrap().is_empty(), "nothing to write when nothing changes");
    }

    #[test]
    fn plan_refuses_unreadable_file() {
        let (_dir, paths) = home();
        write(&cursor_json(&paths), "{ \"mcpServers\": { oops");
        let inv = scan(&Scope::Global, &paths);
        assert_eq!(inv.errors.len(), 1);
        let err = plan(&inv, &add(stdio("plata", &[]), &["cursor"]), &paths).unwrap_err();
        assert!(err.starts_with(&format!("can't read {}: ", cursor_json(&paths).display())), "{err}");
        assert!(!err.contains("oops"), "errors never quote file text");
        assert_eq!(read(&cursor_json(&paths)), "{ \"mcpServers\": { oops");
    }

    #[test]
    fn apply_refuses_when_file_changed_since_plan() {
        let (_dir, paths) = home();
        write(&cursor_json(&paths), r#"{"mcpServers": {"plata": {"command": "npx"}}}"#);
        let inv = scan(&Scope::Global, &paths);
        let plan = plan(&inv, &Request::Remove { name: "plata".into(), from: vec![] }, &paths).unwrap();
        let changed = r#"{"mcpServers": {"plata": {"command": "npx"}, "other": {"command": "x"}}}"#;
        write(&cursor_json(&paths), changed);

        let mut j = Journal::default();
        let (written, err) = apply(&plan, &paths, &mut j).unwrap_err();
        assert_eq!((written, err), (0, format!("{} changed since the plan", cursor_json(&paths).display())));
        assert!(j.is_empty());
        assert_eq!(read(&cursor_json(&paths)), changed);
    }

    #[test]
    fn skipped_agent_gets_reason() {
        let (_dir, paths) = home();
        let desktop = MCP_AGENTS[agent("claude-desktop")].files(&Scope::Global, &paths).remove(0).0;
        std::fs::create_dir_all(desktop.parent().unwrap()).unwrap();
        let inv = scan(&Scope::Global, &paths);
        let server = stdio("plata", &[("PLATA_TOKEN", EnvValue::var("PLATA_TOKEN"))]);

        let plan_both = plan(&inv, &add(server.clone(), &["claude", "claude-desktop"]), &paths).unwrap();
        assert_eq!(lines(&plan_both), ["Add plata to Claude Code"]);
        assert_eq!(plan_both.skipped.len(), 1);
        assert_eq!(plan_both.skipped[0].0, agent("claude-desktop"));
        assert!(plan_both.skipped[0].1.contains("environment references"), "{:?}", plan_both.skipped);
        assert!(plan_both.files.iter().all(|(f, _)| *f != desktop));

        let only = plan(&inv, &add(server, &["claude-desktop"]), &paths).unwrap();
        assert!(only.actions.is_empty() && only.files.is_empty());
        assert_eq!(only.skipped.len(), 1);
    }

    #[test]
    fn disable_needs_a_project_and_uses_claude_lists() {
        let (_dir, paths) = home();
        write(&claude_json(&paths), r#"{"mcpServers": {"plata": {"command": "npx"}}}"#);
        write(&cursor_json(&paths), r#"{"mcpServers": {"plata": {"command": "npx"}}}"#);
        let req = Request::SetDisabled { name: "plata".into(), disabled: true };
        assert!(plan(&scan(&Scope::Global, &paths), &req, &paths).unwrap_err().contains("project"));

        let root = project(&paths);
        let inv = scan(&Scope::Project(root.clone()), &paths);
        let plan = plan(&inv, &req, &paths).unwrap();
        assert_eq!(lines(&plan), ["Disable plata for this project in Claude Code"]);
        assert_eq!(plan.skipped, [(agent("cursor"), "Cursor has no per-project switch".to_string())]);
        run(&plan, &paths);

        let after = scan(&Scope::Project(root), &paths);
        assert_eq!(after.get("plata").unwrap().disabled_for, [agent("claude")]);
        let doc: serde_json::Value = serde_json::from_str(&read(&claude_json(&paths))).unwrap();
        assert_eq!(doc["mcpServers"]["plata"], json!({"command": "npx"}), "the global entry isn't switched off");
    }

    #[test]
    fn merge_renames_and_carries_claude_disabled_state() {
        let (_dir, paths) = home();
        let doc = json!({
            "mcpServers": { "framelink-mcp": { "command": "npx", "args": ["-y", "figma-developer-mcp"] } },
            "projects": { "/work/app": { "disabledMcpServers": ["framelink-mcp"] } }
        });
        write(&claude_json(&paths), &serde_json::to_string_pretty(&doc).unwrap());
        write(&cursor_json(&paths), r#"{"mcpServers": {"figma": {"command": "npx", "args": ["-y", "figma-developer-mcp"]}}}"#);
        let inv = scan(&Scope::Global, &paths);
        let req = Request::Merge { names: vec!["framelink-mcp".into(), "figma".into()], into: "figma".into() };
        let plan = plan(&inv, &req, &paths).unwrap();
        assert_eq!(lines(&plan), ["Rename framelink-mcp to figma in Claude Code", "Disable figma for /work/app in Claude Code"]);
        run(&plan, &paths);

        let after = scan(&Scope::Global, &paths);
        assert!(after.get("framelink-mcp").is_none());
        assert_eq!(after.get("figma").unwrap().copies.len(), 2);
        let doc: serde_json::Value = serde_json::from_str(&read(&claude_json(&paths))).unwrap();
        assert_eq!(doc["projects"]["/work/app"]["disabledMcpServers"], json!(["framelink-mcp", "figma"]));

        write(&cursor_json(&paths), r#"{"mcpServers": {"figma": {"command": "x"}, "framelink-mcp": {"command": "y"}}}"#);
        let inv = scan(&Scope::Global, &paths);
        let req = Request::Merge { names: vec!["framelink-mcp".into()], into: "figma".into() };
        assert_eq!(super::plan(&inv, &req, &paths).unwrap_err(), "Cursor already has figma");
    }

    #[test]
    fn plan_id_follows_the_files() {
        let (_dir, paths) = home();
        write(&cursor_json(&paths), r#"{"mcpServers": {"plata": {"command": "npx"}}}"#);
        let req = Request::Remove { name: "plata".into(), from: vec![] };
        let first = plan(&scan(&Scope::Global, &paths), &req, &paths).unwrap();
        assert_eq!(first.id(), plan(&scan(&Scope::Global, &paths), &req, &paths).unwrap().id());
        write(&cursor_json(&paths), r#"{"mcpServers": {"plata": {"command": "npx"}, "x": {"command": "x"}}}"#);
        assert_ne!(first.id(), plan(&scan(&Scope::Global, &paths), &req, &paths).unwrap().id());
    }
}
