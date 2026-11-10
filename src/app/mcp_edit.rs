//! Changing MCP servers from the window: the Add/Edit form, and the plan dialog that copy,
//! edit, remove, disable/enable and merge show before anything is written.

use std::collections::BTreeMap;

use rune::TextEdit;

use super::{App, HostPlan, Target};
use crate::agents::AGENTS;
use crate::mcp::agents::{Flavor, MCP_AGENTS, index_of};
use crate::mcp::model::{EnvValue, Place, Server, Transport, masked_args, masked_url};
use crate::mcp::plan::{self, ClaudeScope, EditChange, McpPlan, Request};
use crate::mcp::scan::{McpServer, Status};

/// Shown over a plan that was made again because a file changed under the dialog.
pub const FILES_CHANGED: &str = "The files changed; here is the new plan";

/// An env or header row of the Add/Edit dialog. The value field is masked until `shown`.
pub struct McpPair {
    pub key: TextEdit,
    pub value: TextEdit,
    pub shown: bool,
}

/// The Add dialog's fields; Edit reuses it with the server's values.
pub struct McpForm {
    /// The server being edited, as its first editable copy has it; `None` adds a new one.
    pub editing: Option<(String, Transport)>,
    pub name: TextEdit,
    pub http: bool,
    /// The command and its args, split on spaces; double quotes keep a space in an arg.
    pub command: TextEdit,
    pub url: TextEdit,
    /// The command line as prefilled for an edit: text left as it was changes nothing.
    pub line_before: String,
    /// The prefilled command line or URL holds a likely secret, so it is masked until shown.
    pub line_secret: bool,
    pub line_shown: bool,
    pub env: Vec<McpPair>,
    pub headers: Vec<McpPair>,
    /// Ticked agents, indices into `MCP_AGENTS`.
    pub agents: Vec<usize>,
    pub claude_scope: ClaudeScope,
    pub error: Option<String>,
}

/// What a plan dialog is for, with the user's picks.
pub enum McpChange {
    Copy { name: String, to: Vec<usize>, from: Option<usize> },
    /// `definition`: the transport as the edit leaves it, which a host takes on stdin.
    Edit { name: String, changes: Vec<EditChange>, definition: Transport },
    Remove { name: String, from: Vec<usize> },
    Switch { name: String, disabled: bool },
    /// Boxed: a text field is far larger than the other variants.
    Merge { names: Vec<String>, into: Box<TextEdit> },
}

impl McpPair {
    fn new(key: &str, value: &str) -> McpPair {
        McpPair { key: TextEdit::new(key), value: TextEdit::new(value).secure(), shown: false }
    }
}

impl McpForm {
    /// The env rows for a command, the header rows for a URL.
    pub fn pairs(&self, header: bool) -> &[McpPair] {
        if header { &self.headers } else { &self.env }
    }

    fn pairs_mut(&mut self, header: bool) -> &mut Vec<McpPair> {
        if header { &mut self.headers } else { &mut self.env }
    }

    /// Row `i`, for its fields' edits.
    pub fn pairs_edit(&mut self, header: bool, i: usize) -> &mut McpPair {
        &mut self.pairs_mut(header)[i]
    }

    /// The rows as typed, keyed; a row with a value but no key is an error.
    fn typed(&self) -> Result<Vec<(String, String)>, String> {
        let mut out: Vec<(String, String)> = Vec::new();
        // Header names match without case, as the agents read them.
        let same = |a: &str, b: &str| if self.http { a.eq_ignore_ascii_case(b) } else { a == b };
        for p in self.pairs(self.http) {
            let key = p.key.text.trim();
            match (key.is_empty(), p.value.text.is_empty()) {
                (true, true) => continue,
                (true, false) => return Err("Name each value, or remove its row".into()),
                _ if out.iter().any(|(k, _)| same(k, key)) => {
                    return Err(format!("Two {} are named {key}", if self.http { "headers" } else { "variables" }));
                }
                _ => out.push((key.to_string(), p.value.text.clone())),
            }
        }
        Ok(out)
    }

    /// The command line, split, or why there is none.
    fn command_line(&self) -> Result<Vec<String>, String> {
        let parts = split_command(&self.command.text);
        if parts.is_empty() { Err("Give the command to run".into()) } else { Ok(parts) }
    }

    fn url_text(&self) -> Result<String, String> {
        let url = self.url.text.trim();
        if url.is_empty() { Err("Give the server's URL".into()) } else { Ok(url.to_string()) }
    }

    /// The transport the fields describe.
    pub fn transport(&self) -> Result<Transport, String> {
        let values = |pairs: Vec<(String, String)>| pairs.into_iter().map(|(k, v)| (k, EnvValue::parse(&v))).collect::<BTreeMap<_, _>>();
        if self.http {
            let url = self.url_text()?;
            return Ok(Transport::Http { url, headers: values(self.typed()?), sse: false });
        }
        let mut parts = self.command_line()?;
        let command = parts.remove(0);
        Ok(Transport::Stdio { command, args: parts, env: values(self.typed()?), cwd: None })
    }

    /// The fields that differ from `old`, as edits. A value left as prefilled keeps its form,
    /// so a reference such as `Bearer ${TOKEN}` stays one.
    fn changes(&self, old: &Transport) -> Result<Vec<EditChange>, String> {
        let typed = self.typed()?;
        let diff = |old: &BTreeMap<String, EnvValue>, set: fn(String, EnvValue) -> EditChange, unset: fn(String) -> EditChange| {
            // Unsets first: header names match without case, so a renamed `authorization`
            // set before unsetting `Authorization` would go with it.
            let mut out: Vec<EditChange> = old.keys().filter(|k| !typed.iter().any(|(t, _)| t == *k)).map(|k| unset(k.clone())).collect();
            for (k, text) in &typed {
                if old.get(k).is_none_or(|v| v.written() != *text) {
                    out.push(set(k.clone(), EnvValue::parse(text)));
                }
            }
            out
        };
        let mut changes = Vec::new();
        match old {
            Transport::Stdio { command, args, env, .. } => {
                // A line left as prefilled is no change, whatever the split would make of it.
                if self.command.text != self.line_before {
                    let mut parts = self.command_line()?;
                    let new_command = parts.remove(0);
                    if new_command != *command {
                        changes.push(EditChange::Command(new_command));
                    }
                    if parts != *args {
                        changes.push(EditChange::Args(parts));
                    }
                }
                changes.extend(diff(env, EditChange::SetEnv, EditChange::UnsetEnv));
            }
            Transport::Http { url, headers, .. } => {
                let new_url = self.url_text()?;
                if new_url != *url {
                    changes.push(EditChange::Url(new_url));
                }
                changes.extend(diff(headers, EditChange::SetHeader, EditChange::UnsetHeader));
            }
        }
        Ok(changes)
    }
}

/// Words split on whitespace; double quotes keep whitespace inside one word, and `\"` and
/// `\\` stand for a quote and a backslash. Any other backslash is itself (`C:\dir`).
pub fn split_command(line: &str) -> Vec<String> {
    let (mut out, mut word, mut quoted, mut any) = (Vec::new(), String::new(), false, false);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if matches!(chars.peek(), Some('"' | '\\')) => {
                word.extend(chars.next());
                any = true;
            }
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut word));
                }
                any = false;
            }
            c => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(word);
    }
    out
}

/// The command line as [`split_command`] reads it back.
fn join_command(command: &str, args: &[String]) -> String {
    std::iter::once(command).chain(args.iter().map(String::as_str)).map(quote_word).collect::<Vec<_>>().join(" ")
}

/// One word for [`split_command`]: quotes escaped, a backslash escaped only where it would
/// otherwise read as an escape, and the whole in quotes when empty or holding whitespace.
fn quote_word(w: &str) -> String {
    let chars: Vec<char> = w.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' if matches!(chars.get(i + 1), None | Some('"' | '\\')) => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    if w.is_empty() || w.contains(char::is_whitespace) { format!("\"{out}\"") } else { out }
}

impl McpChange {
    pub fn name(&self) -> &str {
        match self {
            McpChange::Copy { name, .. } | McpChange::Edit { name, .. } | McpChange::Remove { name, .. } | McpChange::Switch { name, .. } => name,
            McpChange::Merge { names, .. } => &names[0],
        }
    }

    fn request(&self) -> Request {
        match self {
            McpChange::Copy { name, to, from } => Request::Copy { name: name.clone(), to: to.clone(), from: *from },
            McpChange::Edit { name, changes, .. } => Request::Edit { name: name.clone(), changes: changes.clone() },
            McpChange::Remove { name, from } => Request::Remove { name: name.clone(), from: from.clone() },
            McpChange::Switch { name, disabled } => Request::SetDisabled { name: name.clone(), disabled: *disabled },
            McpChange::Merge { names, into } => Request::Merge { names: names.clone(), into: into.text.trim().to_string() },
        }
    }

    /// The last change's label, as the CLI words it.
    fn label(&self) -> String {
        match self {
            McpChange::Copy { name, .. } => format!("copy {name}"),
            McpChange::Edit { name, .. } => format!("edit {name}"),
            McpChange::Remove { name, .. } => format!("remove {name}"),
            McpChange::Switch { name, disabled: true } => format!("disable {name}"),
            McpChange::Switch { name, disabled: false } => format!("enable {name}"),
            McpChange::Merge { into, .. } => format!("merge into {}", into.text.trim()),
        }
    }

    /// The toast once it is done.
    pub(super) fn done(&self) -> String {
        match self {
            McpChange::Copy { name, .. } => format!("Copied {name}"),
            McpChange::Edit { name, .. } => format!("Edited {name}"),
            McpChange::Remove { name, .. } => format!("Removed {name}"),
            McpChange::Switch { name, disabled: true } => format!("Disabled {name} in this project"),
            McpChange::Switch { name, disabled: false } => format!("Enabled {name} in this project"),
            McpChange::Merge { into, .. } => format!("Merged into {}", into.text.trim()),
        }
    }
}

/// The plan dialog for copy, edit, remove, disable/enable and merge.
pub struct McpDialog {
    pub change: McpChange,
    /// `None` while the plan waits for a pick (a copy's source); `Err` when the planner refused.
    pub plan: Option<Result<McpPlan, String>>,
    /// The plan on a host: made there, `None` while the plan waits for a pick.
    pub host_plan: Option<HostPlan>,
    /// A copy to another machine: where there, and that machine's plan.
    pub across: Option<Target>,
    /// Why the plan shown is not the one first shown.
    pub notice: Option<String>,
}

impl App {
    // Which agents can take a server.

    /// The agent can hold a server in this scope: it has a config here (OpenCode makes one).
    pub fn mcp_can_hold(&self, agent: usize) -> bool {
        if self.is_remote() {
            return self.remote_can_hold(agent);
        }
        let a = &MCP_AGENTS[agent];
        a.flavor == Flavor::OpenCode || !a.files(&self.scope, &self.paths).is_empty()
    }

    /// The sync targets that are MCP agents, installed and able to hold a server here.
    fn mcp_defaults(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self.targets().iter().filter_map(|&i| index_of(AGENTS[i].id)).filter(|&a| self.mcp_detected(a) && self.mcp_can_hold(a)).collect();
        out.sort();
        out
    }

    /// Each agent with a copy of `server` that this scope owns, once.
    pub fn mcp_own_agents(&self, server: &McpServer) -> Vec<usize> {
        let mut out: Vec<usize> = server.copies.iter().filter(|c| self.scope.is_global() || c.place != Place::Global).map(|c| c.agent).collect();
        out.dedup();
        out
    }

    // The Add and Edit dialog.

    pub fn open_mcp_add(&mut self) {
        let mut name = TextEdit::new("");
        name.focused = true;
        self.mcp_form = Some(McpForm {
            editing: None,
            name,
            http: false,
            command: TextEdit::new(""),
            url: TextEdit::new(""),
            line_before: String::new(),
            line_secret: false,
            line_shown: false,
            env: Vec::new(),
            headers: Vec::new(),
            agents: self.mcp_defaults(),
            claude_scope: ClaudeScope::Local,
            error: None,
        });
    }

    /// The Add dialog filled from the server's first editable copy; values stay masked. A host's
    /// values come masked in its state, so its form waits for the server as written there.
    pub fn open_mcp_edit(&mut self, name: &str) {
        if self.is_remote() {
            return self.host_open_mcp_edit(name);
        }
        let Some(server) = self.mcp.get(name).cloned() else { return };
        self.fill_mcp_edit(name, &server);
    }

    pub(super) fn fill_mcp_edit(&mut self, name: &str, server: &McpServer) {
        let Some(copy) = server.copies.iter().find(|c| c.read_only.is_none()) else { return };
        let t = copy.server.transport.clone();
        let pairs = |m: &BTreeMap<String, EnvValue>| m.iter().map(|(k, v)| McpPair::new(k, &v.written())).collect::<Vec<_>>();
        let (http, command, url, line_secret, env, headers) = match &t {
            Transport::Stdio { command, args, env, .. } => {
                let masked = std::iter::once(masked_url(command)).chain(masked_args(args)).collect::<Vec<_>>();
                let secret = masked.iter().ne(std::iter::once(command).chain(args));
                (false, join_command(command, args), String::new(), secret, pairs(env), Vec::new())
            }
            Transport::Http { url, headers, .. } => (true, String::new(), url.clone(), masked_url(url) != *url, Vec::new(), pairs(headers)),
        };
        let line = |text: String| if line_secret { TextEdit::new(text).secure() } else { TextEdit::new(text) };
        let line_before = command.clone();
        self.mcp_form = Some(McpForm {
            editing: Some((name.to_string(), t)),
            name: TextEdit::new(name),
            http,
            command: line(command),
            url: line(url),
            line_before,
            line_secret,
            line_shown: false,
            env,
            headers,
            agents: Vec::new(),
            claude_scope: ClaudeScope::Local,
            error: None,
        });
    }

    pub fn close_mcp_form(&mut self) {
        self.mcp_form = None;
    }

    pub fn set_mcp_kind(&mut self, http: bool) {
        if let Some(f) = &mut self.mcp_form {
            f.http = http;
            f.error = None;
        }
    }

    pub fn add_mcp_pair(&mut self, header: bool) {
        if let Some(f) = &mut self.mcp_form {
            f.pairs_mut(header).push(McpPair::new("", ""));
        }
    }

    pub fn remove_mcp_pair(&mut self, header: bool, i: usize) {
        if let Some(f) = &mut self.mcp_form
            && i < f.pairs(header).len()
        {
            f.pairs_mut(header).remove(i);
        }
    }

    /// Shows or masks one value field.
    pub fn toggle_mcp_pair_shown(&mut self, header: bool, i: usize) {
        if let Some(p) = self.mcp_form.as_mut().and_then(|f| f.pairs_mut(header).get_mut(i)) {
            p.shown = !p.shown;
        }
    }

    pub fn toggle_mcp_line_shown(&mut self) {
        if let Some(f) = &mut self.mcp_form {
            f.line_shown = !f.line_shown;
        }
    }

    pub fn toggle_mcp_form_agent(&mut self, agent: usize) {
        if let Some(f) = &mut self.mcp_form {
            toggle(&mut f.agents, agent);
        }
    }

    pub fn set_claude_scope(&mut self, scope: ClaudeScope) {
        if let Some(f) = &mut self.mcp_form {
            f.claude_scope = scope;
        }
    }

    /// Add writes at once, as in the CLI; Edit shows its plan first.
    pub fn save_mcp_form(&mut self) {
        let remote = self.is_remote();
        let Some(form) = &mut self.mcp_form else { return };
        form.error = None;
        if let Some((name, old)) = &form.editing {
            match form.changes(old) {
                Ok(changes) if changes.is_empty() => form.error = Some("Nothing changed".into()),
                Ok(changes) => match plan::edited(old, &changes, name) {
                    Ok(definition) => {
                        let name = name.clone();
                        self.mcp_form = None;
                        self.open_mcp_plan(McpChange::Edit { name, changes, definition });
                    }
                    Err(e) => form.error = Some(e),
                },
                Err(e) => form.error = Some(e),
            }
            return;
        }
        // Checked and planned against the configs as they are now.
        self.rescan_mcp();
        let Some(form) = &mut self.mcp_form else { return };
        let name = form.name.text.trim().to_string();
        let checked = if name.is_empty() {
            Err("Name the server".to_string())
        } else if self.mcp.get(&name).is_some_and(|s| !s.inherited) {
            Err(format!("{name} is already here; use Copy to add it to more agents"))
        } else {
            form.transport().and_then(|t| if form.agents.is_empty() { Err("Pick at least one agent".into()) } else { Ok(t) })
        };
        let transport = match checked {
            Ok(t) => t,
            Err(e) => {
                form.error = Some(e);
                return;
            }
        };
        if remote {
            form.error = None;
            let (agents, claude_scope) = (form.agents.clone(), form.claude_scope);
            return self.host_add_mcp(name, transport, &agents, claude_scope);
        }
        let request = Request::Add { server: Server { name: name.clone(), transport }, to: form.agents.clone(), claude_scope: form.claude_scope };
        let planned = plan::plan(&self.mcp, &request, &self.paths);
        let Some(form) = &mut self.mcp_form else { return };
        let plan = match planned {
            Ok(p) => p,
            Err(e) => {
                form.error = Some(e);
                return;
            }
        };
        if plan.is_empty() {
            self.notify(format!("Couldn't add {name}"), notes(&plan), true);
            return;
        }
        self.write_mcp_add(&name, plan);
    }

    /// Writes a planned Add and reports it. A file that changed since the plan keeps the form
    /// open, before the last change's backups go.
    pub fn write_mcp_add(&mut self, name: &str, plan: McpPlan) {
        // A plan made here names this machine's files; a host adds through its own CLI.
        if self.is_remote() {
            return;
        }
        if plan::verify(&plan).is_err() {
            if let Some(f) = &mut self.mcp_form {
                f.error = Some("The files changed; try again".into());
            }
            return;
        }
        self.mcp_form = None;
        let mut j = self.new_journal();
        let n = plan.files.len();
        let result = plan::apply(&plan, &self.paths, &mut j).map(|_| format!("Added {name}")).map_err(|(done, e)| format!("Stopped after {done} of {n} files: {e}"));
        self.changed(format!("add {name}"), j, result);
        if let Some(n) = self.notice.as_mut().filter(|n| !n.error) {
            n.detail = notes(&plan);
        }
        if self.mcp.get(name).is_some() {
            self.select_mcp(name);
        }
    }

    // The plan dialog.

    /// Copies to the agents without a copy; when the copies differ, waits for a source.
    pub fn open_mcp_copy(&mut self, name: &str) {
        let Some(s) = self.mcp.get(name) else { return };
        let to = self.mcp_defaults().into_iter().filter(|a| !s.copies.iter().any(|c| c.agent == *a)).collect();
        // Another machine can take it too: its scopes and agents are asked for now.
        self.look_at_machines();
        self.open_mcp_plan(McpChange::Copy { name: name.into(), to, from: None });
    }

    /// Removes from every agent with a copy here, unless some are unticked.
    pub fn confirm_mcp_remove(&mut self, name: &str) {
        let Some(s) = self.mcp.get(name) else { return };
        let from = self.mcp_own_agents(s);
        self.open_mcp_plan(McpChange::Remove { name: name.into(), from });
    }

    /// Enables a server some agent skips in this project, else disables it.
    pub fn toggle_mcp_disabled(&mut self, name: &str) {
        let Some(s) = self.mcp.get(name) else { return };
        let disabled = s.disabled_for.is_empty();
        self.open_mcp_plan(McpChange::Switch { name: name.into(), disabled });
    }

    /// Merges the servers into the first one's name, unless another is typed.
    pub fn open_mcp_merge(&mut self, names: Vec<String>) {
        if names.is_empty() {
            return;
        }
        let into = Box::new(TextEdit::new(names[0].clone()));
        self.open_mcp_plan(McpChange::Merge { names, into });
    }

    fn open_mcp_plan(&mut self, change: McpChange) {
        self.mcp_dialog = Some(McpDialog { change, plan: None, host_plan: None, across: None, notice: None });
        self.replan_mcp();
    }

    pub fn close_mcp_dialog(&mut self) {
        self.mcp_dialog = None;
        self.across.close();
    }

    pub fn toggle_mcp_copy_to(&mut self, agent: usize) {
        if self.toggle_across_agent(agent) {
            return;
        }
        if let Some(McpDialog { change: McpChange::Copy { to, .. }, .. }) = &mut self.mcp_dialog {
            toggle(to, agent);
            self.replan_mcp();
        }
    }

    pub fn set_mcp_copy_from(&mut self, agent: usize) {
        if let Some(McpDialog { change: McpChange::Copy { from, .. }, .. }) = &mut self.mcp_dialog {
            *from = Some(agent);
            self.replan_mcp();
        }
    }

    pub fn toggle_mcp_remove_from(&mut self, agent: usize) {
        if let Some(McpDialog { change: McpChange::Remove { from, .. }, .. }) = &mut self.mcp_dialog {
            toggle(from, agent);
            self.replan_mcp();
        }
    }

    /// Plans the open dialog's change again, against the configs as they are now.
    pub fn replan_mcp(&mut self) {
        // A copy to another machine is planned there, whichever machine is in view.
        if self.mcp_dialog.as_ref().is_some_and(|d| d.across.is_some()) {
            return self.replan_mcp_across();
        }
        if self.is_remote() {
            return self.host_replan_mcp();
        }
        let Some(d) = &self.mcp_dialog else { return };
        let differs = self.mcp.get(d.change.name()).is_some_and(|s| s.status == Status::Differs);
        let plan = match &d.change {
            McpChange::Copy { from: None, .. } if differs => None,
            // No agent ticked means none, where the planner reads it as all.
            McpChange::Remove { from, .. } if from.is_empty() => Some(Ok(McpPlan::default())),
            McpChange::Merge { into, .. } if into.text.trim().is_empty() => Some(Err("Name the merged server".into())),
            change => Some(plan::plan(&self.mcp, &change.request(), &self.paths)),
        };
        if let Some(d) = &mut self.mcp_dialog {
            d.plan = plan;
        }
    }

    /// Writes the dialog's plan. A plan whose files changed since is planned again and shown
    /// with a notice, before the last change's backups go.
    pub fn apply_mcp_plan(&mut self) {
        if self.mcp_dialog.as_ref().is_some_and(|d| d.across.is_some()) {
            return self.apply_mcp_across();
        }
        if self.is_remote() {
            return self.host_apply_mcp_plan();
        }
        let Some(McpDialog { plan: Some(Ok(plan)), .. }) = &self.mcp_dialog else { return };
        if plan.is_empty() {
            return;
        }
        if plan::verify(plan).is_err() {
            self.rescan_mcp();
            self.replan_mcp();
            if let Some(d) = &mut self.mcp_dialog {
                d.notice = Some(FILES_CHANGED.into());
            }
            return;
        }
        let Some(McpDialog { change, plan: Some(Ok(plan)), .. }) = self.mcp_dialog.take() else { return };
        let mut j = self.new_journal();
        let n = plan.files.len();
        let result = plan::apply(&plan, &self.paths, &mut j).map(|_| change.done()).map_err(|(done, e)| format!("Stopped after {done} of {n} files: {e}"));
        self.changed(change.label(), j, result);
        if let McpChange::Merge { into, .. } = &change
            && self.mcp.get(into.text.trim()).is_some()
        {
            self.mcp_selected = Some(into.text.trim().to_string());
        }
    }
}

fn toggle(list: &mut Vec<usize>, agent: usize) {
    match list.iter().position(|&a| a == agent) {
        Some(i) => {
            list.remove(i);
        }
        None => {
            list.push(agent);
            list.sort();
        }
    }
}

/// The skipped agents and warnings, one per line, for a toast.
fn notes(plan: &McpPlan) -> String {
    let skipped = plan.skipped.iter().map(|(a, why)| format!("Skipped {}: {why}", MCP_AGENTS[*a].name));
    skipped.chain(plan.warnings.iter().map(|w| format!("Heads-up: {w}"))).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(http: bool, pairs: &[(&str, &str)]) -> McpForm {
        let rows = pairs.iter().map(|(k, v)| McpPair::new(k, v)).collect::<Vec<_>>();
        let (env, headers) = if http { (Vec::new(), rows) } else { (rows, Vec::new()) };
        McpForm {
            editing: None,
            name: TextEdit::new("x"),
            http,
            command: TextEdit::new(""),
            url: TextEdit::new("https://h.example.com/mcp"),
            line_before: String::new(),
            line_secret: false,
            line_shown: false,
            env,
            headers,
            agents: vec![0],
            claude_scope: ClaudeScope::Local,
            error: None,
        }
    }

    #[test]
    fn split_command_keeps_quoted_spaces_and_round_trips() {
        assert_eq!(split_command(r#"  npx -y "a b" "" c"#), ["npx", "-y", "a b", "", "c"]);
        assert_eq!(split_command(r#"x say\"hi\" {\"a\":1} C:\dir"#), ["x", r#"say"hi""#, r#"{"a":1}"#, r"C:\dir"]);
        let args: Vec<String> = ["-y", "a b", "", r#"say "hi""#, r#"{"a":1}"#, r"C:\dir", r"ends\", r"two\\"].iter().map(|a| a.to_string()).collect();
        let mut back = split_command(&join_command("npx", &args));
        assert_eq!(back.remove(0), "npx");
        assert_eq!(back, args);
    }

    #[test]
    fn an_untouched_command_line_is_no_change() {
        let args: Vec<String> = ["--json", r#"{"a":1}"#, r#"say "hi""#].iter().map(|a| a.to_string()).collect();
        let old = Transport::Stdio { command: "npx".into(), args: args.clone(), env: BTreeMap::new(), cwd: None };
        let mut f = form(false, &[]);
        f.line_before = join_command("npx", &args);
        f.command = TextEdit::new(f.line_before.clone());
        assert!(f.changes(&old).unwrap().is_empty());
        f.command = TextEdit::new(f.line_before.replacen("npx", "bunx", 1));
        assert_eq!(f.changes(&old).unwrap(), [EditChange::Command("bunx".into())], "the args read back as they were");
    }

    #[test]
    fn a_header_renamed_by_case_is_unset_before_it_is_set() {
        let old = Transport::Http {
            url: "https://h.example.com/mcp".into(),
            headers: [("Authorization".to_string(), EnvValue(vec![crate::mcp::model::Part::Text("Bearer ".into()), crate::mcp::model::Part::Var { name: "T".into(), default: None }]))].into(),
            sse: false,
        };
        let changes = form(true, &[("authorization", "Bearer ${T}")]).changes(&old).unwrap();
        assert_eq!(changes[0], EditChange::UnsetHeader("Authorization".into()));
        assert!(matches!(&changes[1], EditChange::SetHeader(k, _) if k == "authorization"));
        // Left as prefilled, a mixed value stays a reference rather than becoming literal text.
        assert!(form(true, &[("Authorization", "Bearer ${T}")]).changes(&old).unwrap().is_empty());
    }
}
