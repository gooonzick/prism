//! Copying to another machine from the window: the other environments a Copy to… dialog lists,
//! the target's plan for a pick there, the copy itself, and a toast's Undo on the machine it
//! names. Both ends are runners (this machine's own `prism` for "This Mac"), so a copy from a
//! host to this machine, or between two hosts, goes the same way. All of it runs off the UI
//! thread: a copy streams a tar over ssh and may take seconds.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use rune::{Latest, Waker};
use serde_json::Value;

use super::env::{RemoteState, Unreached, load};
use super::mcp_edit::{McpChange, McpDialog};
use super::{App, SetWaker};
use crate::agents::{Paths, Scope};
use crate::hosts::Host;
use crate::hosts::copy::{copy_server, copy_skills, parse, this_machine, why};
use crate::hosts::runner::{MakeRunner, Runner, quote};
use crate::mcp::agents::MCP_AGENTS;
use crate::mcp::plan::ClaudeScope;
use crate::mcp::scan::Status;
use crate::plan_view::PlanView;
use crate::scan::ScopeStats;
use crate::sync::plural;
use crate::wire::WireState;

/// What the window knows of another machine: enough to list its scopes and its agents.
pub struct MachineView {
    pub paths: Paths,
    pub projects: Vec<PathBuf>,
    pub stats: HashMap<Scope, ScopeStats>,
    /// Its global skills, to say which a copy into its Global would skip.
    pub global_names: HashSet<String>,
    /// Per entry of `MCP_AGENTS`: installed there.
    pub mcp_detected: Vec<bool>,
}

impl MachineView {
    fn from_state(w: WireState) -> MachineView {
        let global_names = if w.scope.is_global() { w.inv.skills.iter().map(|s| s.name.clone()).collect() } else { HashSet::new() };
        MachineView { paths: w.paths, projects: w.settings.projects, stats: w.stats.into_iter().collect(), global_names, mcp_detected: w.mcp_detected }
    }

    /// Global, then each project.
    pub fn scopes(&self) -> Vec<Scope> {
        std::iter::once(Scope::Global).chain(self.projects.iter().cloned().map(Scope::Project)).collect()
    }

    /// A scope as that machine names it: its home is `~`.
    pub fn label(&self, scope: &Scope) -> String {
        crate::sync::label::scope(scope, &self.paths)
    }

    /// The MCP agents installed there.
    pub fn agents(&self) -> Vec<usize> {
        (0..MCP_AGENTS.len()).filter(|&a| self.mcp_detected.get(a).copied().unwrap_or(false)).collect()
    }
}

/// Another machine a copy can go to.
pub struct Machine {
    /// "This Mac", or the host's name.
    pub label: String,
    host: Host,
    /// `None` while it is asked; `Err` when it can't be reached or needs prism.
    pub view: Option<Result<MachineView, String>>,
}

impl Machine {
    /// A short word for element keys: "this", or the host's name.
    pub fn key(&self) -> &str {
        &self.host.name
    }
}

/// Where a plan on another machine stands.
pub enum Planned {
    /// Something to pick first; the note says what.
    Waiting(String),
    Asking,
    Done(Result<PlanView, String>),
}

/// A destination on another machine, and that machine's plan for it.
pub struct Target {
    /// "This Mac", or the host's name.
    pub machine: String,
    pub dest: Scope,
    /// The MCP agents to copy to there; `None` until its agents are known. Skills don't use it.
    pub agents: Option<Vec<usize>>,
    pub plan: Planned,
    /// The machine is applying the plan.
    pub applying: bool,
    /// Why the plan shown is not the one first shown.
    pub notice: Option<String>,
    /// The request whose answer this waits for.
    seq: u64,
}

impl Target {
    fn new(machine: &str, dest: Scope, agents: Option<Vec<usize>>) -> Target {
        Target { machine: machine.to_string(), dest, agents, plan: Planned::Asking, applying: false, notice: None, seq: 0 }
    }

    /// The plan's id, when it has something to apply and nothing is applying.
    fn ready(&self) -> Option<String> {
        match &self.plan {
            Planned::Done(Ok(view)) if !self.applying && !view.lines.is_empty() => view.id.clone(),
            _ => None,
        }
    }
}

#[derive(Clone)]
enum What {
    /// The skills, each with its folder on the source machine.
    Skills(Vec<(String, PathBuf)>),
    Server { name: String, from_agent: Option<usize>, agents: Vec<usize> },
}

/// One copy: from the machine in view to another one.
#[derive(Clone)]
struct Job {
    from: Host,
    from_label: String,
    from_scope: Scope,
    to: Host,
    to_label: String,
    dest: Scope,
    what: What,
}

/// What a finished copy says in its toast.
#[derive(Clone)]
enum Done {
    Skills { names: Vec<String>, to: String },
    Server { name: String, to: String },
}

impl Done {
    fn title(&self, copied: usize, machine: &str) -> String {
        match self {
            Done::Skills { names, to } if names.len() == 1 && copied == 1 => format!("Copied {} to {to} on {machine}", names[0]),
            Done::Skills { to, .. } => format!("Copied {} to {to} on {machine}", plural(copied, "skill", "skills")),
            Done::Server { name, to } => format!("Copied {name} to {to} on {machine}"),
        }
    }
}

/// A write on another machine: a copy with its plan's id, or a toast's Undo.
enum Write {
    Copy { job: Box<Job>, id: String, seq: u64, done: Done },
    Undo { host: Host, machine: String },
}

/// What a write did there.
pub struct Written {
    machine: String,
    what: Wrote,
    result: Result<Value, String>,
}

enum Wrote {
    Copy { seq: u64, done: Done },
    Undo,
}

/// Copies to other machines: the open dialog's machines and pick, and the work in flight.
#[derive(Default)]
pub struct Across {
    /// The other machines the open Copy to… dialog lists, this one's first.
    pub machines: Vec<Machine>,
    load: Latest<Vec<(String, Result<MachineView, String>)>>,
    /// The skill dialog's pick on another machine.
    pub skills: Option<Target>,
    seq: u64,
    plan: Latest<(u64, Result<Value, String>)>,
    /// Writes waiting their turn, oldest first: one runs at a time, and none is ever dropped.
    writes: VecDeque<Write>,
    /// The one running; it is never replaced, so its answer always lands.
    write: Latest<Written>,
    writing: bool,
    /// The MCP copy dialog's Machine and Scope menus are open.
    pub machine_menu: bool,
    pub scope_menu: bool,
}

impl Across {
    pub(super) fn set_waker(&self, waker: Waker) {
        for w in [&self.load as &dyn SetWaker, &self.plan, &self.write] {
            w.set(waker.clone());
        }
    }

    /// Forgets the open dialog's machines and plan. A copy or an undo in flight still lands:
    /// its toast names its machine.
    pub(super) fn close(&mut self) {
        self.machines.clear();
        self.load.cancel();
        self.skills = None;
        self.plan.cancel();
        self.machine_menu = false;
        self.scope_menu = false;
    }
}

/// This machine, as a runner sees it.
fn this_host() -> Host {
    Host { name: "this".into(), ssh: "this".into() }
}

/// The folder `dir` names on the machine `r` reaches, links resolved: the folder a skill's hash
/// is taken of. A remote shell's startup files may print first: the folder is the last line.
fn folder_on(r: &dyn Runner, label: &str, dir: &std::path::Path) -> Result<PathBuf, String> {
    let out = r.shell(&format!("cd -P -- {} && pwd -P", quote(&dir.to_string_lossy())), None)?;
    let text = String::from_utf8_lossy(&out.stdout);
    match text.lines().map(str::trim).rfind(|l| !l.is_empty()) {
        Some(folder) if out.code == 0 => Ok(PathBuf::from(folder)),
        _ => Err(format!("Can't read {} on {label}: {}", dir.display(), why(&out))),
    }
}

/// Plans the copy on its target, or with `apply` the plan's id, does it.
fn run(make: &MakeRunner, job: &Job, apply: Option<&str>) -> Result<Value, String> {
    let (from, to) = (make(&job.from), make(&job.to));
    match &job.what {
        What::Skills(skills) => {
            let mut folders = Vec::new();
            for (name, dir) in skills {
                folders.push((name.clone(), folder_on(from.as_ref(), &job.from_label, dir)?));
            }
            copy_skills(from.as_ref(), &job.from_label, &job.from_scope, &folders, to.as_ref(), &job.to_label, &job.dest, apply)
        }
        What::Server { name, from_agent, agents } => {
            copy_server(from.as_ref(), &job.from_label, name, *from_agent, &job.from_scope, to.as_ref(), &job.to_label, &job.dest, ClaudeScope::Local, agents, apply)
        }
    }
}

/// `prism undo` on the machine `r` reaches: its answer, or why it failed.
fn undo_there(r: &dyn Runner) -> Result<Value, String> {
    let out = r.prism(&["undo".into(), "--json".into()], None)?;
    match parse(&out.stdout) {
        Some(v) if v.get("error").is_none() && out.code == 0 => Ok(v),
        Some(v) => Err(v["error"].as_str().map_or_else(|| why(&out), String::from)),
        None => Err(why(&out)),
    }
}

/// What another machine is like, or why the window can't copy there.
fn look_at(r: &dyn Runner, host: &Host, label: &str) -> Result<MachineView, String> {
    match load(r, host, &Scope::Global, false).0 {
        Ok(w) => Ok(MachineView::from_state(*w)),
        Err(RemoteState::NeedsInstall(health)) => Err(format!("prism on {label} is {}; switch to {label} to install it", health.describe())),
        Err(RemoteState::Unreachable(why)) => Err(why),
        Err(_) => Err(format!("Can't reach {label}")),
    }
}

/// A target's answer as a plan view. The target names itself in its lines and conflicts; the
/// agents it skips get its name here.
fn view_of(v: &Value, machine: &str) -> Result<PlanView, String> {
    let mut view = PlanView::from_json(v).map_err(|e| format!("{machine} sent a plan this window can't read: {e}"))?;
    let suffix = format!(" on {machine}");
    for (_, why) in &mut view.skipped {
        if !why.ends_with(&suffix) {
            why.push_str(&suffix);
        }
    }
    Ok(view)
}

impl App {
    /// The machine in view, as a runner names it and as plans name it.
    fn in_view(&self) -> (Host, String) {
        (self.host().cloned().unwrap_or_else(this_host), self.env_label())
    }

    /// The machine a label names: this one, or a saved host.
    fn host_named(&self, label: &str) -> Option<Host> {
        if label == this_machine() {
            return Some(this_host());
        }
        crate::hosts::find(&self.local_settings().hosts, label).cloned()
    }

    /// A copy can go somewhere: another scope here, or another machine.
    pub fn can_copy(&self) -> bool {
        !self.other_scopes().is_empty() || self.is_remote() || !self.local_settings().hosts.is_empty()
    }

    /// What the window knows of the other machine `label`.
    pub fn machine(&self, label: &str) -> Option<&Machine> {
        self.across.machines.iter().find(|m| m.label == label)
    }

    fn machine_view(&self, label: &str) -> Option<&MachineView> {
        self.machine(label).and_then(|m| m.view.as_ref()).and_then(|v| v.as_ref().ok())
    }

    /// Lists every other machine for a Copy to… dialog: this one from what the window keeps of
    /// it, each saved host asked now (all at once, off the UI thread).
    pub(super) fn look_at_machines(&mut self) {
        self.across.close();
        if let Some(local) = &self.local {
            let view = MachineView {
                paths: local.paths.clone(),
                projects: local.settings.projects.clone(),
                stats: local.stats.clone(),
                global_names: local.global_names.clone(),
                mcp_detected: MCP_AGENTS.iter().map(|a| a.detected(&local.paths)).collect(),
            };
            self.across.machines.push(Machine { label: this_machine().to_string(), host: this_host(), view: Some(Ok(view)) });
        }
        let in_view = self.host().map(|h| h.name.clone());
        let hosts: Vec<Host> = self.local_settings().hosts.iter().filter(|h| Some(&h.name) != in_view.as_ref()).cloned().collect();
        if hosts.is_empty() {
            return;
        }
        self.across.machines.extend(hosts.iter().map(|h| Machine { label: h.name.clone(), host: h.clone(), view: None }));
        let make = self.make_runner.clone();
        self.across.load.request(hosts, move |hosts| {
            // A host that doesn't answer holds up the others no longer than its own timeout.
            std::thread::scope(|scope| {
                let asks: Vec<_> = hosts
                    .iter()
                    .map(|host| {
                        let make = make.clone();
                        scope.spawn(move || (host.name.clone(), look_at(make(host).as_ref(), host, &host.name)))
                    })
                    .collect();
                asks.into_iter().zip(&hosts).map(|(ask, host)| ask.join().unwrap_or_else(|_| (host.name.clone(), Err(format!("Can't reach {}", host.name))))).collect()
            })
        });
    }

    /// Lands the answers that came in. True when something visible changed.
    pub(super) fn poll_across(&mut self) -> bool {
        let mut changed = false;
        if let Some(answers) = self.across.load.take() {
            self.land_machines(answers);
            changed = true;
        }
        if let Some((seq, result)) = self.across.plan.take() {
            self.land_across_plan(seq, result);
            changed = true;
        }
        if let Some(Written { machine, what, result }) = self.across.write.take() {
            self.across.writing = false;
            match what {
                Wrote::Copy { seq, done } => self.land_applied(seq, machine, done, result),
                Wrote::Undo => self.land_undo(machine, result),
            }
            self.next_write();
            changed = true;
        }
        changed
    }

    /// A dialog still waits for a machine's scopes or plan.
    #[cfg(test)]
    pub fn across_asking(&self) -> bool {
        self.across.load.is_pending() || self.across.plan.is_pending()
    }

    /// Something is still being asked of another machine.
    #[cfg(test)]
    pub fn across_pending(&self) -> bool {
        let a = &self.across;
        a.load.is_pending() || a.plan.is_pending() || a.write.is_pending()
    }

    /// A write runs, or waits its turn, or its answer waits for a poll.
    #[cfg(test)]
    pub fn across_writing(&self) -> bool {
        self.across.writing || !self.across.writes.is_empty()
    }

    fn land_machines(&mut self, answers: Vec<(String, Result<MachineView, String>)>) {
        for (name, view) in answers {
            if let Some(m) = self.across.machines.iter_mut().find(|m| m.host.name == name) {
                m.view = Some(view);
            }
        }
        // An MCP copy waiting for that machine's agents gets them ticked, then its plan.
        let waiting = match &self.mcp_dialog {
            Some(McpDialog { across: Some(t), .. }) if t.agents.is_none() => Some(t.machine.clone()),
            _ => None,
        };
        if let Some(label) = waiting {
            let agents = self.machine_view(&label).map(MachineView::agents);
            if let Some(McpDialog { across: Some(t), .. }) = &mut self.mcp_dialog {
                t.agents = agents;
            }
            self.replan_mcp_across();
        }
    }

    /// Asks the target for a plan; its answer lands where a target waits with this number.
    fn ask_plan(&mut self, job: Job) -> u64 {
        self.across.seq += 1;
        let make = self.make_runner.clone();
        self.across.plan.request((self.across.seq, job), move |(seq, job)| (seq, run(&make, &job, None)));
        self.across.seq
    }

    /// The target, skill or MCP, waiting for answer `seq`.
    fn target_for(&mut self, seq: u64) -> Option<&mut Target> {
        if let Some(t) = self.across.skills.as_mut().filter(|t| t.seq == seq) {
            return Some(t);
        }
        match &mut self.mcp_dialog {
            Some(McpDialog { across: Some(t), .. }) if t.seq == seq => Some(t),
            _ => None,
        }
    }

    fn land_across_plan(&mut self, seq: u64, result: Result<Value, String>) {
        if let Some(t) = self.target_for(seq) {
            t.plan = Planned::Done(result.and_then(|v| view_of(&v, &t.machine)));
        }
    }

    // Skills.

    /// The skills of the open Copy to… dialog, each with its folder on the machine in view.
    fn picked_skills(&self) -> Result<Vec<(String, PathBuf)>, String> {
        let names = self.copy_dialog.as_deref().unwrap_or_default();
        names
            .iter()
            .map(|n| match self.inv.get(n).and_then(|s| s.source.clone()) {
                Some(dir) => Ok((n.clone(), dir)),
                None => Err(format!("{n} has no folder in {}", self.scope_label(&self.scope))),
            })
            .collect()
    }

    fn skills_job(&self, t: &Target) -> Result<Job, String> {
        let (from, from_label) = self.in_view();
        let to = self.host_named(&t.machine).ok_or_else(|| format!("No saved host named {}", t.machine))?;
        let what = What::Skills(self.picked_skills()?);
        Ok(Job { from, from_label, from_scope: self.scope.clone(), to, to_label: t.machine.clone(), dest: t.dest.clone(), what })
    }

    /// Picks `dest` on the other machine `machine` for the open skill dialog: its plan comes next.
    pub fn copy_to_machine(&mut self, machine: &str, dest: Scope) {
        let mut t = Target::new(machine, dest, None);
        match self.skills_job(&t) {
            Ok(job) => t.seq = self.ask_plan(job),
            Err(e) => t.plan = Planned::Done(Err(e)),
        }
        self.across.skills = Some(t);
    }

    /// Closes the skill Copy to… dialog.
    pub fn close_copy(&mut self) {
        self.copy_dialog = None;
        self.across.close();
    }

    /// Copies the picked skills with the target's plan.
    pub fn apply_copy_across(&mut self) {
        let Some(t) = &self.across.skills else { return };
        let Some(id) = t.ready() else { return };
        let job = match self.skills_job(t) {
            Ok(job) => job,
            Err(e) => return self.notify(format!("Couldn't finish that on {}", t.machine), e, true),
        };
        let names = self.copy_dialog.clone().unwrap_or_default();
        let to = self.machine_view(&t.machine).map_or_else(|| self.scope_label(&t.dest), |v| v.label(&t.dest));
        let seq = t.seq;
        if let Some(t) = &mut self.across.skills {
            t.applying = true;
        }
        self.copy_across(job, id, seq, Done::Skills { names, to });
    }

    fn copy_across(&mut self, job: Job, id: String, seq: u64, done: Done) {
        self.across.writes.push_back(Write::Copy { job: Box::new(job), id, seq, done });
        self.next_write();
    }

    /// Starts the oldest waiting write once the one running has landed.
    fn next_write(&mut self) {
        if self.across.writing {
            return;
        }
        let Some(write) = self.across.writes.pop_front() else { return };
        self.across.writing = true;
        let make = self.make_runner.clone();
        self.across.write.request(write, move |write| match write {
            Write::Copy { job, id, seq, done } => Written { machine: job.to_label.clone(), result: run(&make, &job, Some(&id)), what: Wrote::Copy { seq, done } },
            Write::Undo { host, machine } => Written { machine, result: undo_there(make(&host).as_ref()), what: Wrote::Undo },
        });
    }

    /// A copy's answer: a toast whose Undo acts on the target, wherever the window is by then.
    fn land_applied(&mut self, seq: u64, machine: String, done: Done, result: Result<Value, String>) {
        let open = self.target_for(seq).is_some();
        let view = result.and_then(|v| Ok((v["applied"] == true, v["plan_changed"] == true, view_of(&v, &machine)?)));
        match view {
            Ok((_, true, view)) if open => {
                let notice = format!("The files changed on {machine}; here is the new plan");
                if let Some(t) = self.target_for(seq) {
                    (t.plan, t.applying, t.notice) = (Planned::Done(Ok(view)), false, Some(notice));
                }
            }
            Ok((_, true, view)) => self.notify(format!("Nothing was done; the files changed on {machine}"), view.lines.join("\n"), true),
            Ok((true, _, view)) => {
                self.close_across_dialog(open);
                self.notify(done.title(view.lines.len(), &machine), super::remote_actions::notes(&view), false);
                if let Some(n) = &mut self.notice {
                    n.undo = true;
                    n.machine = Some(machine.clone());
                }
                self.look_again(&machine);
            }
            Ok((false, _, view)) => {
                self.close_across_dialog(open);
                self.notify(format!("Nothing to change on {machine}"), super::remote_actions::notes(&view), false);
            }
            Err(why) => {
                self.close_across_dialog(open);
                self.notify(format!("Couldn't finish that on {machine}"), why, true);
            }
        }
    }

    /// Closes the dialog a finished copy came from, when it is still the open one.
    fn close_across_dialog(&mut self, open: bool) {
        if !open {
            return;
        }
        if self.across.skills.is_some() {
            self.close_copy();
        } else {
            self.mcp_dialog = None;
            self.across.close();
        }
    }

    /// Reads `machine` again when it is the one in view: a copy or an undo changed it.
    fn look_again(&mut self, machine: &str) {
        if machine == this_machine() {
            self.recount_stash();
        }
        if machine != self.env_label() {
            return;
        }
        if self.is_remote() {
            self.load_remote(self.scope.clone(), false);
        } else {
            self.last_change = crate::store::load_last_change(&self.paths);
            self.rescan();
            self.refresh_stats();
        }
    }

    /// Counts this machine's scopes again while a host is in view, so the next Copy to…
    /// shows what a copy or an undo there changed.
    fn recount_stash(&mut self) {
        let Some(local) = &mut self.local else { return };
        let targets = crate::agents::target_indices(local.settings.targets.as_deref(), &local.paths);
        let scopes = std::iter::once(Scope::Global).chain(local.settings.projects.iter().cloned().map(Scope::Project));
        for scope in scopes {
            let inv = crate::scan::scan(&scope, &local.paths);
            if scope.is_global() {
                local.global_names = inv.skills.iter().map(|s| s.name.clone()).collect();
            }
            local.stats.insert(scope, crate::scan::stats_of(&inv, &targets));
        }
    }

    // MCP servers.

    /// Copies the open MCP copy to the machine `label` (this one, or another), as the Machine
    /// menu picks it.
    pub fn set_mcp_copy_machine(&mut self, label: &str) {
        self.across.machine_menu = false;
        let agents = self.machine_view(label).map(MachineView::agents);
        let here = label == self.env_label();
        let Some(d) = self.mcp_dialog.as_mut().filter(|d| matches!(d.change, McpChange::Copy { .. })) else { return };
        if d.across.as_ref().is_some_and(|t| t.applying) {
            return;
        }
        d.notice = None;
        d.across = if here { None } else { Some(Target::new(label, Scope::Global, agents)) };
        self.replan_mcp();
    }

    /// The Scope menu: where on the other machine the server goes.
    pub fn set_mcp_copy_scope(&mut self, dest: Scope) {
        self.across.scope_menu = false;
        if let Some(McpDialog { across: Some(t), .. }) = &mut self.mcp_dialog
            && !t.applying
        {
            t.dest = dest;
            self.replan_mcp_across();
        }
    }

    /// Ticks or unticks an agent on the other machine; true when the copy goes there.
    pub(super) fn toggle_across_agent(&mut self, agent: usize) -> bool {
        let Some(McpDialog { across: Some(t), .. }) = &mut self.mcp_dialog else { return false };
        if !t.applying
            && let Some(agents) = &mut t.agents
        {
            match agents.iter().position(|&a| a == agent) {
                Some(i) => {
                    agents.remove(i);
                }
                None => {
                    agents.push(agent);
                    agents.sort();
                }
            }
            self.replan_mcp_across();
        }
        true
    }

    fn server_job(&self) -> Option<Job> {
        let d = self.mcp_dialog.as_ref()?;
        let (McpChange::Copy { name, from, .. }, Some(t)) = (&d.change, &d.across) else { return None };
        let (from_host, from_label) = self.in_view();
        let what = What::Server { name: name.clone(), from_agent: *from, agents: t.agents.clone().unwrap_or_default() };
        Some(Job { from: from_host, from_label, from_scope: self.scope.clone(), to: self.host_named(&t.machine)?, to_label: t.machine.clone(), dest: t.dest.clone(), what })
    }

    /// Plans the open MCP copy on the other machine.
    pub(super) fn replan_mcp_across(&mut self) {
        let Some(McpDialog { change: McpChange::Copy { name, from, .. }, across: Some(t), .. }) = &self.mcp_dialog else { return };
        let differs = self.mcp.get(name).is_some_and(|s| s.status == Status::Differs);
        let machine = self.machine(&t.machine).map(|m| m.view.as_ref().map(|v| v.is_ok()));
        let plan = match (machine, &t.agents) {
            _ if differs && from.is_none() => Some(Planned::Waiting("Pick the copy to copy from.".into())),
            // Still asking it, or it can't take a copy: its line in the picks says why.
            (Some(None), _) => Some(Planned::Asking),
            (None | Some(Some(false)), _) => Some(Planned::Done(Ok(PlanView::default()))),
            (_, Some(agents)) if agents.is_empty() => Some(Planned::Done(Ok(PlanView::default()))),
            _ => None,
        };
        let job = if plan.is_none() { self.server_job() } else { None };
        let seq = job.map(|job| self.ask_plan(job));
        if let Some(McpDialog { across: Some(t), .. }) = &mut self.mcp_dialog {
            t.plan = plan.unwrap_or(Planned::Asking);
            t.seq = seq.unwrap_or(0);
        }
    }

    /// Copies the server with the other machine's plan.
    pub(super) fn apply_mcp_across(&mut self) {
        let Some(McpDialog { change: McpChange::Copy { name, .. }, across: Some(t), .. }) = &self.mcp_dialog else { return };
        let Some(id) = t.ready() else { return };
        let to = self.machine_view(&t.machine).map_or_else(|| self.scope_label(&t.dest), |v| v.label(&t.dest));
        let (done, seq) = (Done::Server { name: name.clone(), to }, t.seq);
        let Some(job) = self.server_job() else { return };
        if let Some(McpDialog { across: Some(t), .. }) = &mut self.mcp_dialog {
            t.applying = true;
        }
        self.copy_across(job, id, seq, done);
    }

    // Undo from a toast.

    /// The toast's Undo: on the machine the toast names, wherever the window is.
    /// Once pressed it is gone from the toast, so a second press can't undo a second change.
    pub fn undo_toast(&mut self) {
        let Some(n) = self.notice.as_mut().filter(|n| n.undo) else { return };
        n.undo = false;
        match n.machine.clone() {
            Some(machine) if machine != self.env_label() => self.undo_on(machine),
            _ => self.undo(),
        }
    }

    /// Runs `prism undo` on another machine, off the UI thread.
    fn undo_on(&mut self, machine: String) {
        let Some(host) = self.host_named(&machine) else {
            return self.notify(format!("No saved host named {machine}"), String::new(), true);
        };
        self.across.writes.push_back(Write::Undo { host, machine });
        self.next_write();
    }

    fn land_undo(&mut self, machine: String, result: Result<Value, String>) {
        match result {
            Ok(v) => match v["undone"].as_str() {
                Some(label) => self.notify(format!("Undid: {label} on {machine}"), String::new(), false),
                None => self.notify(format!("Nothing to undo on {machine}"), String::new(), false),
            },
            Err(why) => self.notify(format!("Couldn't fully undo on {machine}"), why, true),
        }
        self.look_again(&machine);
    }

    /// Opens the Copy to… dialog for `names` with `answers` as the other machines, reaching
    /// none: for scenes.
    pub fn show_copy_to_host(&mut self, names: Vec<String>, answers: Vec<(crate::hosts::Host, Result<Box<WireState>, String>)>) {
        self.make_runner = std::sync::Arc::new(|h: &Host| Box::new(Unreached(h.name.clone())) as Box<dyn Runner>);
        for (host, _) in &answers {
            if crate::hosts::find(&self.settings.hosts, &host.name).is_none() {
                self.settings.hosts.push(host.clone());
            }
        }
        self.open_copy(names);
        self.across.load.cancel();
        let answers = answers.into_iter().map(|(h, r)| (h.name, r.map(|w| MachineView::from_state(*w)))).collect();
        self.land_machines(answers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_names_what_it_copied_and_where() {
        let one = Done::Skills { names: vec!["pdf".into()], to: "Global (~)".into() };
        assert_eq!(one.title(1, "laptop"), "Copied pdf to Global (~) on laptop");
        assert_eq!(one.title(0, "laptop"), "Copied 0 skills to Global (~) on laptop");
        let two = Done::Skills { names: vec!["a".into(), "b".into()], to: "~/site".into() };
        assert_eq!(two.title(2, "This Mac"), "Copied 2 skills to ~/site on This Mac");
    }

    #[test]
    fn skipped_agents_name_the_target_once() {
        let v = serde_json::json!({ "actions": [], "skipped": [{ "agent": "claude", "reason": "already has it" }, { "agent": "cursor", "reason": "x on laptop" }] });
        let view = view_of(&v, "laptop").unwrap();
        assert_eq!(view.skipped[0].1, "already has it on laptop");
        assert_eq!(view.skipped[1].1, "x on laptop");
    }
}
