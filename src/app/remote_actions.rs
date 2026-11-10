//! Changes on the host in view. Each window action is the host's own CLI command: a change is
//! planned there and applied with that plan's id, then the host's state is read again. The
//! commands run off the UI thread one at a time, in the order they were asked, so two quick
//! clicks never write on the host at once. Values typed for an MCP server go on stdin
//! (`KEY=-`, several apart by NUL bytes), never in a command line.

use std::path::PathBuf;

use rune::TextEdit;
use serde_json::Value;

use super::App;
use super::mcp_edit::{McpChange, McpDialog};
use crate::agents::{AGENTS, Scope};
use crate::hosts::Host;
use crate::hosts::copy::{parse, why};
use crate::hosts::runner::Runner;
use crate::mcp::agents::MCP_AGENTS;
use crate::mcp::model::{EnvValue, Transport};
use crate::mcp::plan::{ClaudeScope, EditChange};
use crate::mcp::scan::McpServer;
use crate::plan_view::PlanView;
use crate::scan::Placement;
use crate::skill;
use crate::sync::{self, plural};

/// A command for the host: its words before `--`, the names after it (so a name can't read as
/// a flag), and what goes on its stdin. No `Debug`: stdin may hold secrets.
#[derive(Clone, PartialEq)]
pub struct HostCommand {
    args: Vec<String>,
    names: Vec<String>,
    stdin: Option<Vec<u8>>,
}

impl HostCommand {
    fn new(args: Vec<String>, names: Vec<String>) -> HostCommand {
        HostCommand { args, names, stdin: None }
    }

    /// The command line, with `extra` flags and `--json`, then the names.
    fn argv(&self, extra: &[&str]) -> Vec<String> {
        let mut argv = self.args.clone();
        argv.extend(extra.iter().map(|e| e.to_string()));
        argv.push("--json".into());
        if !self.names.is_empty() {
            argv.push("--".into());
            argv.extend(self.names.iter().cloned());
        }
        argv
    }
}

/// A plan the host made, as a dialog shows it.
pub struct HostPlan {
    /// What it plans; `None` for a plan the window could tell without asking (nothing to do).
    command: Option<HostCommand>,
    /// `None` while the host plans it; `Err` when it couldn't.
    pub view: Option<Result<PlanView, String>>,
    /// The host is applying it.
    pub applying: bool,
    /// Why the plan shown is not the one first shown.
    pub notice: Option<String>,
}

impl HostPlan {
    fn asking(command: HostCommand) -> HostPlan {
        HostPlan { command: Some(command), view: None, applying: false, notice: None }
    }

    fn known(view: Result<PlanView, String>) -> HostPlan {
        HostPlan { command: None, view: Some(view), applying: false, notice: None }
    }

    /// The plan's lines and id, when it has something to apply.
    fn ready(&self) -> Option<(HostCommand, String)> {
        let Some(Ok(view)) = &self.view else { return None };
        if self.applying || view.lines.is_empty() {
            return None;
        }
        Some((self.command.clone()?, view.id.clone()?))
    }
}

/// Add project… on a host: a typed path, since no folder picker reaches its disk.
pub struct HostProject {
    pub path: TextEdit,
    pub error: Option<String>,
}

/// What the host is asked to do.
#[derive(Clone, PartialEq)]
pub(super) enum Work {
    /// Plan the command.
    Plan(HostCommand),
    /// Apply the plan with this id.
    Apply(HostCommand, String),
    /// Plan the command, then apply that plan; `strict` refuses a plan that skips anything.
    Act { command: HostCommand, strict: bool },
    /// A command that acts at once, or only reads: its JSON.
    Run(HostCommand),
}

/// What the host answered.
pub(super) enum Outcome {
    Planned(PlanView),
    Done(PlanView),
    /// The plan changed since its id (exit 4): the new one, nothing done.
    Changed(PlanView),
    /// There was nothing to do.
    Nothing,
    /// A command's JSON and exit code.
    Answer(i32, Value),
    /// Why it failed; `undoable` when part of it was done.
    Failed { why: String, undoable: bool },
}

/// What a finished change says in its toast; some count what was done.
pub(super) enum Title {
    Text(String),
    Copied { names: Vec<String>, to: String },
    Broken,
    Synced,
}

impl Title {
    fn text(&self, done: usize) -> String {
        match self {
            Title::Text(t) => t.clone(),
            Title::Copied { names, to } if names.len() == 1 && done == 1 => format!("Copied {} to {to}", names[0]),
            Title::Copied { to, .. } => format!("Copied {} to {to}", plural(done, "skill", "skills")),
            Title::Broken => format!("Removed {}", plural(done, "broken link", "broken links")),
            Title::Synced => format!("Synced: {}", plural(done, "change", "changes")),
        }
    }
}

/// What landing the answer does.
pub(super) enum Then {
    /// A change made at once, and its toast.
    Change(Title),
    /// The sync dialog's plan, for this command.
    SyncPlan(HostCommand),
    SyncApply { only: bool },
    /// The MCP plan dialog's plan, for this command.
    McpPlan(HostCommand),
    McpApply { done: String, select: Option<String> },
    McpAdd { name: String },
    NewSkill { name: String },
    Targets,
    ProjectAdded,
    ProjectForgotten(PathBuf),
    /// The unmasked server, for the Edit form.
    EditForm(String),
    /// The unmasked server, for Show values.
    Reveal(String),
    Undo,
}

pub struct HostJob {
    host: Host,
    work: Work,
    then: Then,
}

pub struct HostAnswer {
    host: String,
    then: Then,
    outcome: Outcome,
}

/// Runs `c` with `extra` flags: its exit code and JSON, or why there is none.
fn call(r: &dyn Runner, c: &HostCommand, extra: &[&str]) -> Result<(i32, Value), String> {
    let out = r.prism(&c.argv(extra), c.stdin.clone())?;
    parse(&out.stdout).map(|v| (out.code, v)).ok_or_else(|| why(&out))
}

fn error_of(v: &Value) -> Option<String> {
    v.get("error").and_then(Value::as_str).map(String::from)
}

/// Each line and skip reason ends with the machine it is about.
fn on_host(mut view: PlanView, host: &str) -> PlanView {
    for line in &mut view.lines {
        line.push_str(&format!(" on {host}"));
    }
    for (_, why) in &mut view.skipped {
        why.push_str(&format!(" on {host}"));
    }
    view
}

fn plan(r: &dyn Runner, host: &str, c: &HostCommand) -> Result<PlanView, String> {
    let (_, v) = call(r, c, &[])?;
    if let Some(e) = error_of(&v) {
        return Err(e);
    }
    PlanView::from_json(&v).map(|view| on_host(view, host)).map_err(|e| format!("{host} sent a plan this window can't read: {e}"))
}

fn apply(r: &dyn Runner, host: &str, c: &HostCommand, id: &str) -> Result<Outcome, String> {
    let (code, v) = call(r, c, &["--apply", "--plan-id", id])?;
    if let Some(why) = error_of(&v) {
        let undoable = v.get("done").and_then(Value::as_u64).is_some_and(|n| n > 0);
        return Ok(Outcome::Failed { why, undoable });
    }
    let view = on_host(PlanView::from_json(&v).map_err(|e| format!("{host} sent a result this window can't read: {e}"))?, host);
    Ok(match (code, v["applied"] == true) {
        (4, _) => Outcome::Changed(view),
        (_, true) => Outcome::Done(view),
        _ => Outcome::Nothing,
    })
}

/// Does `work` on the host `r` reaches.
fn work(r: &dyn Runner, host: &str, work: Work) -> Outcome {
    let result = match work {
        Work::Plan(c) => plan(r, host, &c).map(Outcome::Planned),
        Work::Apply(c, id) => apply(r, host, &c, &id),
        Work::Act { command, strict } => plan(r, host, &command).and_then(|view| {
            if !view.skipped.is_empty() && (strict || view.lines.is_empty()) {
                return Err(view.skipped.iter().map(|(_, why)| why.as_str()).collect::<Vec<_>>().join("\n"));
            }
            match &view.id {
                _ if view.lines.is_empty() => Ok(Outcome::Nothing),
                Some(id) => apply(r, host, &command, id),
                None => Err(format!("{host} sent a plan with no id")),
            }
        }),
        Work::Run(c) => call(r, &c, &[]).and_then(|(code, v)| match error_of(&v) {
            Some(e) => Err(e),
            None => Ok(Outcome::Answer(code, v)),
        }),
    };
    result.unwrap_or_else(|why| Outcome::Failed { why, undoable: false })
}

/// The skipped agents (or skills) and the heads-ups, one per line, for a toast.
fn notes(view: &PlanView) -> String {
    let skipped = view.skipped.iter().map(|(who, why)| format!("Skipped {who}: {why}"));
    skipped.chain(view.warnings.iter().map(|w| format!("Heads-up: {w}"))).collect::<Vec<_>>().join("\n")
}

/// `--env`/`--header` arguments for `values`, each `KEY=-`, and the values for stdin.
fn secret_args(flag: &str, values: &[(String, EnvValue)], args: &mut Vec<String>, stdin: &mut Vec<String>) {
    for (key, value) in values {
        args.push(format!("--{flag}={key}=-"));
        stdin.push(value.written());
    }
}

/// The values for stdin, apart by NUL bytes; none when no argument reads stdin.
fn stdin_of(values: Vec<String>) -> Option<Vec<u8>> {
    if values.is_empty() { None } else { Some(values.join("\0").into_bytes()) }
}

fn ids(agents: &[usize]) -> Vec<String> {
    agents.iter().map(|&a| format!("--to={}", MCP_AGENTS[a].id)).collect()
}

impl App {
    /// `words` with the open scope's `--project`.
    fn scoped(&self, words: &[&str]) -> Vec<String> {
        let mut args: Vec<String> = words.iter().map(|w| w.to_string()).collect();
        if let Scope::Project(dir) = &self.scope {
            args.push(format!("--project={}", dir.display()));
        }
        args
    }

    fn on_host(&self, text: &str) -> String {
        format!("{text} on {}", self.env_label())
    }

    /// Queues `work` for the host in view; it runs once what was asked before it has landed.
    /// The same work asked twice in a row (a double click) runs once.
    pub(super) fn run_on_host(&mut self, work: Work, then: Then) {
        let Some(host) = self.host().cloned() else { return };
        if self.host_jobs.back().map(|j| &j.work).or(self.host_running.as_ref()) == Some(&work) {
            return;
        }
        self.host_jobs.push_back(HostJob { host, work, then });
        self.next_host_job();
    }

    fn next_host_job(&mut self) {
        if self.host_running.is_some() {
            return;
        }
        let Some(job) = self.host_jobs.pop_front() else { return };
        self.host_running = Some(job.work.clone());
        let make = self.make_runner.clone();
        self.host_act.request(job, move |job| {
            let runner = make(&job.host);
            let outcome = work(runner.as_ref(), &job.host.name, job.work);
            HostAnswer { host: job.host.name, then: job.then, outcome }
        });
    }

    /// Forgets the work asked of the last host: it was for a machine no longer in view.
    pub(super) fn drop_host_work(&mut self) {
        self.host_jobs.clear();
        self.host_act.cancel();
        self.host_running = None;
        self.host_last_change = None;
        self.host_values.clear();
    }

    /// Something is still running on the host, or waiting to.
    #[cfg(test)]
    pub fn host_busy(&self) -> bool {
        self.host_running.is_some() || !self.host_jobs.is_empty()
    }

    /// Lands an answer from the host in view, then starts the next job.
    pub(super) fn land_host(&mut self, answer: HostAnswer) {
        self.host_running = None;
        if self.host().is_some_and(|h| h.name == answer.host) {
            self.land_host_answer(answer.then, answer.outcome);
        }
        self.next_host_job();
    }

    /// Reads the host's state again, after a change there.
    fn reload_host(&mut self) {
        self.load_remote(self.scope.clone(), false);
    }

    /// A finished change's toast: Undo takes it back on the host.
    fn host_done(&mut self, title: String, detail: String) {
        self.notify(self.on_host(&title), detail, false);
        if let Some(n) = &mut self.notice {
            n.undo = true;
        }
    }

    fn host_failed(&mut self, why: String, undoable: bool) {
        let why = if undoable { format!("{why}\nUndo takes back what was done.") } else { why };
        self.notify(self.on_host("Couldn't finish that"), why, true);
        if let Some(n) = &mut self.notice {
            n.undo = undoable;
        }
    }

    fn plan_changed(&mut self, view: &PlanView) {
        self.notify(format!("The files changed on {}; nothing was done", self.env_label()), view.lines.join("\n"), true);
    }

    fn land_host_answer(&mut self, then: Then, outcome: Outcome) {
        match (then, outcome) {
            (Then::Change(title), outcome) => {
                match outcome {
                    Outcome::Done(view) => self.host_done(title.text(view.lines.len()), notes(&view)),
                    Outcome::Changed(view) => self.plan_changed(&view),
                    Outcome::Failed { why, undoable } => self.host_failed(why, undoable),
                    _ => self.notify(self.on_host("Nothing to change"), String::new(), false),
                }
                self.reload_host();
            }
            (Then::SyncPlan(asked), outcome) => {
                if let Some(p) = self.sync_host.as_mut().filter(|p| p.command.as_ref() == Some(&asked)) {
                    p.view = Some(match outcome {
                        Outcome::Planned(view) => Ok(view),
                        Outcome::Failed { why, .. } => Err(why),
                        _ => Err("The host answered with something other than a plan".into()),
                    });
                }
            }
            (Then::SyncApply { only }, outcome) => {
                match outcome {
                    Outcome::Changed(view) if self.sync_host.is_some() => {
                        let notice = Some(format!("The files changed on {}; here is the new plan", self.env_label()));
                        if let Some(p) = &mut self.sync_host {
                            (p.view, p.applying, p.notice) = (Some(Ok(view)), false, notice);
                        }
                    }
                    Outcome::Changed(view) => self.plan_changed(&view),
                    Outcome::Done(view) => {
                        self.close_sync();
                        if only {
                            self.checked.clear();
                        }
                        self.host_done(Title::Synced.text(view.lines.len()), String::new());
                    }
                    Outcome::Failed { why, undoable } => {
                        self.close_sync();
                        self.host_failed(why, undoable);
                    }
                    _ => self.close_sync(),
                }
                self.reload_host();
            }
            (Then::McpPlan(asked), outcome) => {
                if let Some(McpDialog { host_plan: Some(p), .. }) = &mut self.mcp_dialog
                    && p.command.as_ref() == Some(&asked)
                {
                    p.view = Some(match outcome {
                        Outcome::Planned(view) => Ok(view),
                        Outcome::Failed { why, .. } => Err(why),
                        _ => Err("The host answered with something other than a plan".into()),
                    });
                }
            }
            (Then::McpApply { done, select }, outcome) => {
                match outcome {
                    Outcome::Changed(view) if self.mcp_dialog.is_some() => {
                        let notice = format!("The files changed on {}; here is the new plan", self.env_label());
                        if let Some(d) = &mut self.mcp_dialog {
                            d.notice = Some(notice);
                            if let Some(p) = &mut d.host_plan {
                                (p.view, p.applying) = (Some(Ok(view)), false);
                            }
                        }
                    }
                    Outcome::Changed(view) => self.plan_changed(&view),
                    Outcome::Done(_) => {
                        self.mcp_dialog = None;
                        self.host_done(done, String::new());
                        if let Some(name) = select {
                            self.mcp_selected = Some(name);
                        }
                    }
                    Outcome::Failed { why, undoable } => {
                        self.mcp_dialog = None;
                        self.host_failed(why, undoable);
                    }
                    _ => self.mcp_dialog = None,
                }
                self.reload_host();
            }
            (Then::McpAdd { name }, Outcome::Answer(_, v)) => {
                let view = PlanView::from_json(&v).map(|view| on_host(view, &self.env_label())).unwrap_or_default();
                if v["applied"] == true {
                    self.mcp_form = None;
                    self.host_done(format!("Added {name}"), notes(&view));
                    self.select_mcp(&name);
                    self.reload_host();
                } else {
                    self.notify(self.on_host(&format!("Couldn't add {name}")), notes(&view), true);
                }
            }
            (Then::McpAdd { name }, outcome) => {
                let why = match outcome {
                    Outcome::Failed { why, .. } => why,
                    _ => format!("Couldn't add {name}"),
                };
                match self.mcp_form.as_mut().filter(|f| f.editing.is_none()) {
                    Some(form) => form.error = Some(why),
                    None => self.notify(self.on_host(&format!("Couldn't add {name}")), why, true),
                }
            }
            (Then::NewSkill { name }, Outcome::Answer(code, _)) => {
                self.new_skill = None;
                let detail = if code == 0 { String::new() } else { "It couldn't be linked into every target".to_string() };
                self.notify(self.on_host(&format!("Created {name}")), detail, code != 0);
                self.selected = Some(name);
                self.reload_host();
            }
            (Then::NewSkill { name }, outcome) => {
                let why = match outcome {
                    Outcome::Failed { why, .. } => why,
                    _ => format!("Couldn't create {name}"),
                };
                match &mut self.new_skill {
                    Some(form) => form.error = Some(why),
                    None => self.notify(self.on_host(&format!("Couldn't create {name}")), why, true),
                }
            }
            (Then::Targets, outcome) => {
                if let Outcome::Failed { why, .. } = outcome {
                    self.notify(self.on_host("Couldn't change the sync targets"), why, true);
                }
                self.reload_host();
            }
            (Then::ProjectAdded, Outcome::Answer(_, v)) => {
                self.host_project = None;
                match v["path"].as_str() {
                    Some(dir) => self.connect_remote(Scope::Project(PathBuf::from(dir))),
                    None => self.reload_host(),
                }
            }
            (Then::ProjectAdded, outcome) => {
                let why = match outcome {
                    Outcome::Failed { why, .. } => why,
                    _ => "The host didn't add it".to_string(),
                };
                match &mut self.host_project {
                    Some(form) => form.error = Some(why),
                    None => self.notify(self.on_host("Couldn't add the project"), why, true),
                }
            }
            (Then::ProjectForgotten(dir), outcome) => {
                if let Outcome::Failed { why, .. } = outcome {
                    self.notify(self.on_host("Couldn't forget the project"), why, true);
                }
                if self.scope == Scope::Project(dir) {
                    self.connect_remote(Scope::Global);
                } else {
                    self.reload_host();
                }
            }
            (Then::EditForm(name), outcome) => match server_of(outcome, &name) {
                Ok(server) if self.mcp_form.is_none() => self.fill_mcp_edit(&name, &server),
                Ok(_) => {}
                Err(why) => self.notify(self.on_host(&format!("Couldn't read {name}")), why, true),
            },
            (Then::Reveal(name), outcome) => match server_of(outcome, &name) {
                Ok(server) if self.mcp_revealed.contains(&name) => {
                    self.host_values.insert(name, server);
                }
                Ok(_) => {}
                Err(why) => {
                    self.mcp_revealed.remove(&name);
                    self.notify(self.on_host(&format!("Couldn't read {name}")), why, true);
                }
            },
            (Then::Undo, outcome) => {
                match outcome {
                    Outcome::Answer(_, v) => match v["undone"].as_str() {
                        Some(label) => self.notify(self.on_host(&format!("Undid: {label}")), String::new(), false),
                        None => self.notify(self.on_host("Nothing to undo"), String::new(), false),
                    },
                    Outcome::Failed { why, .. } => self.notify(self.on_host("Couldn't fully undo"), why, true),
                    _ => {}
                }
                self.reload_host();
            }
        }
    }

    // Skills.

    pub(super) fn host_toggle_link(&mut self, name: &str, agent: usize) {
        let Some(s) = self.inv.get(name) else { return };
        let (verb, label) = match &s.placements[agent] {
            Placement::Linked | Placement::Link(_) | Placement::Broken(_) => ("unlink", sync::label::unlink(name, agent)),
            Placement::Missing => ("link", sync::label::link(name, agent)),
            Placement::Dir => {
                let why = format!("{} has its own folder; use \"Use link\" or move it to the trash", AGENTS[agent].name);
                self.notify("Couldn't finish that".into(), why, true);
                return;
            }
        };
        let args = self.scoped(&["skills", verb, &format!("--agent={}", AGENTS[agent].id)]);
        let command = HostCommand::new(args, vec![name.into()]);
        self.run_on_host(Work::Act { command, strict: true }, Then::Change(Title::Text(super::installed::sentence(&label))));
    }

    pub(super) fn host_replace_with_link(&mut self, name: &str, agent: usize) {
        let args = self.scoped(&["skills", "sync", &format!("--to={}", AGENTS[agent].id)]);
        let command = HostCommand::new(args, vec![name.into()]);
        self.run_on_host(Work::Act { command, strict: true }, Then::Change(Title::Text(format!("{name} now lives in .agents"))));
    }

    pub(super) fn host_trash_copy(&mut self, name: &str, agent: usize) {
        let args = self.scoped(&["skills", "trash", &format!("--agent={}", AGENTS[agent].id)]);
        let command = HostCommand::new(args, vec![name.into()]);
        let title = format!("Moved {}'s copy of {name} to the trash", AGENTS[agent].name);
        self.run_on_host(Work::Act { command, strict: true }, Then::Change(Title::Text(title)));
    }

    pub(super) fn host_remove(&mut self, names: &[String]) {
        let command = HostCommand::new(self.scoped(&["skills", "remove"]), names.to_vec());
        let title = match names {
            [one] => format!("Removed {one}"),
            _ => format!("Removed {} skills", names.len()),
        };
        self.run_on_host(Work::Act { command, strict: true }, Then::Change(Title::Text(title)));
    }

    pub(super) fn host_remove_broken(&mut self) {
        let command = HostCommand::new(self.scoped(&["skills", "clean-broken"]), Vec::new());
        self.run_on_host(Work::Act { command, strict: false }, Then::Change(Title::Broken));
    }

    pub(super) fn host_copy_to(&mut self, names: Vec<String>, dest: &Scope) {
        let to = match dest {
            Scope::Global => "--to-global".to_string(),
            Scope::Project(dir) => format!("--to-project={}", dir.display()),
        };
        let command = HostCommand::new(self.scoped(&["skills", "copy", &to]), names.clone());
        let title = Title::Copied { names, to: self.scope_label(dest) };
        self.run_on_host(Work::Act { command, strict: false }, Then::Change(title));
    }

    /// The sync dialog for `only` (or everything), with the host's plan once it is in.
    pub(super) fn host_open_sync(&mut self, only: Option<Vec<String>>) {
        let command = HostCommand::new(self.scoped(&["skills", "sync"]), only.clone().unwrap_or_default());
        self.sync_only = only;
        self.sync_host = Some(HostPlan::asking(command.clone()));
        self.run_on_host(Work::Plan(command.clone()), Then::SyncPlan(command));
    }

    pub(super) fn host_apply_sync(&mut self) {
        let Some(p) = &mut self.sync_host else { return };
        let Some((command, id)) = p.ready() else { return };
        p.applying = true;
        let only = self.sync_only.is_some();
        self.run_on_host(Work::Apply(command, id), Then::SyncApply { only });
    }

    /// Ticks or unticks a sync target in the host's settings, then plans the open sync again.
    pub(super) fn host_toggle_target(&mut self, agent: usize) {
        let mut targets = self.targets();
        match targets.iter().position(|&a| a == agent) {
            Some(i) => {
                targets.remove(i);
            }
            None => {
                targets.push(agent);
                targets.sort();
            }
        }
        if targets.is_empty() {
            self.notify(self.on_host("Couldn't change the sync targets"), "Keep at least one agent to sync into".into(), true);
            return;
        }
        let ids: Vec<String> = targets.iter().map(|&a| AGENTS[a].id.to_string()).collect();
        // Shown at once; the host's state confirms it.
        self.settings.targets = Some(ids.clone());
        self.run_on_host(Work::Run(HostCommand::new(vec!["targets".into(), "set".into()], ids)), Then::Targets);
        if self.sync_host.is_some() {
            self.host_open_sync(self.sync_only.clone());
        }
    }

    pub(super) fn host_create_skill(&mut self) {
        let Some(form) = &mut self.new_skill else { return };
        let name = form.name.text.trim().to_string();
        if let Err(e) = skill::valid_name(&name) {
            form.error = Some(e);
            return;
        }
        if self.inv.get(&name).is_some() {
            form.error = Some(format!("{name} already exists here"));
            return;
        }
        let description = format!("--description={}", form.description.text);
        let command = HostCommand::new(self.scoped(&["skills", "new", &description]), vec![name.clone()]);
        self.run_on_host(Work::Run(command), Then::NewSkill { name });
    }

    // Projects.

    pub fn open_host_project(&mut self) {
        let mut path = TextEdit::new("");
        path.focused = true;
        self.host_project = Some(HostProject { path, error: None });
    }

    /// Adds the typed folder to the host's projects, then opens it.
    pub fn save_host_project(&mut self) {
        let host = self.env_label();
        let Some(form) = &mut self.host_project else { return };
        let path = form.path.text.trim().to_string();
        if !path.starts_with('/') {
            form.error = Some(format!("Type the folder's full path on {host}, starting with /"));
            return;
        }
        form.error = None;
        self.host_add_project(PathBuf::from(path));
    }

    pub(super) fn host_add_project(&mut self, dir: PathBuf) {
        let command = HostCommand::new(vec!["projects".into(), "add".into()], vec![dir.display().to_string()]);
        self.run_on_host(Work::Run(command), Then::ProjectAdded);
    }

    pub(super) fn host_forget_project(&mut self, dir: &PathBuf) {
        // Gone from the sidebar at once; the host's state confirms it.
        self.settings.projects.retain(|p| p != dir);
        self.stats.remove(&Scope::Project(dir.clone()));
        let command = HostCommand::new(vec!["projects".into(), "remove".into()], vec![dir.display().to_string()]);
        self.run_on_host(Work::Run(command), Then::ProjectForgotten(dir.clone()));
    }

    // Undo.

    /// Something can be undone on the machine in view.
    pub fn can_undo(&self) -> bool {
        if self.is_remote() { self.host_last_change.is_some() } else { self.last_change.is_some() }
    }

    pub(super) fn host_undo(&mut self) {
        self.run_on_host(Work::Run(HostCommand::new(vec!["undo".into()], Vec::new())), Then::Undo);
    }

    // MCP servers.

    /// Asks the host for the server's values, unmasked, for the Edit form.
    pub(super) fn host_open_mcp_edit(&mut self, name: &str) {
        let command = HostCommand::new(self.scoped(&["wire", "server"]), vec![name.into()]);
        self.run_on_host(Work::Run(command), Then::EditForm(name.into()));
    }

    /// Show values on a host: that server's values, asked for now and dropped when hidden.
    pub(super) fn host_toggle_reveal(&mut self, name: &str) {
        if self.mcp_revealed.remove(name) {
            self.host_values.remove(name);
            return;
        }
        self.mcp_revealed.insert(name.to_string());
        let command = HostCommand::new(self.scoped(&["wire", "server"]), vec![name.into()]);
        self.run_on_host(Work::Run(command), Then::Reveal(name.into()));
    }

    /// The server as the inspector shows it: on a host with its values shown, the unmasked one.
    pub fn shown_server<'a>(&'a self, server: &'a McpServer) -> &'a McpServer {
        self.host_values.get(&server.name).filter(|_| self.mcp_revealed.contains(&server.name)).unwrap_or(server)
    }

    /// Adds the server on the host, as the Add form describes it; the form stays open until the
    /// host answers, to show why it couldn't.
    pub(super) fn host_add_mcp(&mut self, name: String, transport: Transport, agents: &[usize], claude_scope: ClaudeScope) {
        let mut args = self.scoped(&["mcp", "add"]);
        let mut values = Vec::new();
        match transport {
            Transport::Stdio { command, args: words, env, .. } => {
                args.push(format!("--command={command}"));
                args.extend(words.iter().map(|a| format!("--arg={a}")));
                secret_args("env", &env.into_iter().collect::<Vec<_>>(), &mut args, &mut values);
            }
            Transport::Http { url, headers, .. } => {
                args.push(format!("--url={url}"));
                secret_args("header", &headers.into_iter().collect::<Vec<_>>(), &mut args, &mut values);
            }
        }
        args.extend(ids(agents));
        args.push(format!("--claude-scope={}", if claude_scope == ClaudeScope::Project { "project" } else { "local" }));
        let command = HostCommand { args, names: vec![name.clone()], stdin: stdin_of(values) };
        self.run_on_host(Work::Run(command), Then::McpAdd { name });
    }

    /// Plans the open MCP dialog's change on the host.
    pub(super) fn host_replan_mcp(&mut self) {
        let Some(d) = &self.mcp_dialog else { return };
        let differs = self.mcp.get(d.change.name()).is_some_and(|s| s.status == crate::mcp::scan::Status::Differs);
        let plan = match &d.change {
            McpChange::Copy { from: None, .. } if differs => None,
            McpChange::Copy { to, .. } if to.is_empty() => Some(HostPlan::known(Ok(PlanView::default()))),
            McpChange::Remove { from, .. } if from.is_empty() => Some(HostPlan::known(Ok(PlanView::default()))),
            McpChange::Merge { into, .. } if into.text.trim().is_empty() => Some(HostPlan::known(Err("Name the merged server".into()))),
            change => Some(HostPlan::asking(self.mcp_command(change))),
        };
        let asked = plan.as_ref().and_then(|p| p.command.clone());
        if let Some(d) = &mut self.mcp_dialog {
            d.plan = None;
            d.host_plan = plan;
        }
        if let Some(command) = asked {
            self.run_on_host(Work::Plan(command.clone()), Then::McpPlan(command));
        }
    }

    /// The host's command for a dialog's change.
    fn mcp_command(&self, change: &McpChange) -> HostCommand {
        match change {
            McpChange::Copy { name, to, from } => {
                let mut args = self.scoped(&["mcp", "copy"]);
                args.extend(ids(to));
                args.extend(from.map(|a| format!("--from={}", MCP_AGENTS[a].id)));
                HostCommand::new(args, vec![name.clone()])
            }
            McpChange::Remove { name, from } => {
                let mut args = self.scoped(&["mcp", "remove"]);
                args.extend(from.iter().map(|&a| format!("--from={}", MCP_AGENTS[a].id)));
                HostCommand::new(args, vec![name.clone()])
            }
            McpChange::Switch { name, disabled } => HostCommand::new(self.scoped(&["mcp", if *disabled { "disable" } else { "enable" }]), vec![name.clone()]),
            McpChange::Merge { names, into } => HostCommand::new(self.scoped(&["mcp", "merge", &format!("--into={}", into.text.trim())]), names.clone()),
            McpChange::Edit { name, changes } => {
                let mut args = self.scoped(&["mcp", "edit"]);
                let (mut env, mut headers) = (Vec::new(), Vec::new());
                for change in changes {
                    match change {
                        EditChange::Command(c) => args.push(format!("--command={c}")),
                        EditChange::Args(words) if words.is_empty() => args.push("--no-args".into()),
                        EditChange::Args(words) => args.extend(words.iter().map(|a| format!("--arg={a}"))),
                        EditChange::Url(u) => args.push(format!("--url={u}")),
                        EditChange::UnsetEnv(k) => args.push(format!("--unset-env={k}")),
                        EditChange::UnsetHeader(k) => args.push(format!("--unset-header={k}")),
                        EditChange::SetEnv(k, v) => env.push((k.clone(), v.clone())),
                        EditChange::SetHeader(k, v) => headers.push((k.clone(), v.clone())),
                    }
                }
                // The CLI reads the env values first, then the header ones.
                let mut values = Vec::new();
                secret_args("env", &env, &mut args, &mut values);
                secret_args("header", &headers, &mut args, &mut values);
                HostCommand { args, names: vec![name.clone()], stdin: stdin_of(values) }
            }
        }
    }

    pub(super) fn host_apply_mcp_plan(&mut self) {
        let Some(d) = &mut self.mcp_dialog else { return };
        let Some((command, id)) = d.host_plan.as_ref().and_then(HostPlan::ready) else { return };
        if let Some(p) = &mut d.host_plan {
            p.applying = true;
        }
        let select = match &d.change {
            McpChange::Merge { into, .. } => Some(into.text.trim().to_string()),
            _ => None,
        };
        let done = d.change.done();
        self.run_on_host(Work::Apply(command, id), Then::McpApply { done, select });
    }
}

/// The unmasked server in a `wire server` answer. serde's errors quote the input, and the input
/// holds secrets: say only what failed.
fn server_of(outcome: Outcome, name: &str) -> Result<McpServer, String> {
    match outcome {
        Outcome::Answer(_, v) => serde_json::from_value(v).map_err(|_| format!("The host sent a definition of {name} Prism can't read")),
        Outcome::Failed { why, .. } => Err(why),
        _ => Err(format!("The host sent no definition of {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_a_double_dash_and_json_comes_before() {
        let c = HostCommand::new(vec!["skills".into(), "remove".into()], vec!["--apply".into()]);
        assert_eq!(c.argv(&["--plan-id", "x"]), ["skills", "remove", "--plan-id", "x", "--json", "--", "--apply"]);
    }

    #[test]
    fn lines_and_skips_name_the_host() {
        let view = PlanView { lines: vec!["Link a into Codex".into()], skipped: vec![("b".into(), "already in Global (~)".into())], warnings: vec!["w".into()], id: None };
        let view = on_host(view, "box");
        assert_eq!(view.lines, ["Link a into Codex on box"]);
        assert_eq!(view.skipped[0].1, "already in Global (~) on box");
        assert_eq!(view.warnings, ["w"], "heads-ups say their own machine");
    }
}
