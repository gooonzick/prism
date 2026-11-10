//! `prism skills`: what is installed in a scope, and changing it. Every command that
//! changes something prints a plan first and acts only with `--apply`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde_json::{Value, json};

use super::{Ctx, Exit, Out, failed, usage};
use crate::agents::{self, AGENTS, CANONICAL, Scope};
use crate::fsops::Journal;
use crate::hosts::copy::copy_skills;
use crate::planid::{plan_id, tree_hash};
use crate::scan::{self, Inventory, Placement};
use crate::remote;
use crate::skill;
use crate::store::{self, LastChange};
use crate::sync::{self, Action, Conflict, label};

/// `--apply`, and `--plan-id` to refuse a plan that changed since it was printed.
#[derive(Args)]
pub struct Apply {
    /// Do it, instead of only printing the plan.
    #[arg(long)]
    pub(super) apply: bool,
    /// With --apply: stop (exit 4) unless the plan still has this id.
    #[arg(long, value_name = "ID")]
    pub(super) plan_id: Option<String>,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// List the skills in the scope.
    List,
    /// Show one skill.
    Show { name: String },
    /// Move skills into .agents and link them into the target agents.
    Sync {
        /// Only these skills (default: all).
        names: Vec<String>,
        /// Link into these agents instead of the targets; `agents` alone only moves the skills
        /// into .agents.
        #[arg(long, value_name = "AGENT", num_args = 1..)]
        to: Vec<String>,
        #[command(flatten)]
        apply: Apply,
    },
    /// Link a skill into one agent.
    Link {
        name: String,
        #[arg(long, value_name = "AGENT")]
        agent: String,
        #[command(flatten)]
        apply: Apply,
    },
    /// Remove one agent's link to a skill.
    Unlink {
        name: String,
        #[arg(long, value_name = "AGENT")]
        agent: String,
        #[command(flatten)]
        apply: Apply,
    },
    /// Remove skills: links go, folders go to the trash.
    Remove {
        #[arg(required = true)]
        names: Vec<String>,
        #[command(flatten)]
        apply: Apply,
    },
    /// Copy skills to a project or to the global setup, here or on another machine.
    Copy {
        #[arg(required = true)]
        names: Vec<String>,
        /// The project folder to copy into (with --to-host, a folder on that host).
        #[arg(long, value_name = "DIR", conflicts_with = "to_global")]
        to_project: Option<PathBuf>,
        /// Copy into the global setup.
        #[arg(long)]
        to_global: bool,
        /// Copy to this saved host (or ssh target) instead of this machine.
        #[arg(long, value_name = "HOST")]
        to_host: Option<String>,
        #[command(flatten)]
        apply: Apply,
    },
    /// Move one agent's own copy of a skill to the trash.
    Trash {
        name: String,
        #[arg(long, value_name = "AGENT")]
        agent: String,
        #[command(flatten)]
        apply: Apply,
    },
    /// Take skills another machine sends: with --apply, their folders as one tar on stdin.
    /// One skill as NAME --source-hash HASH (the tar holds its folder's contents), or any
    /// number as --skill NAME=HASH (the tar holds each folder under its name).
    #[command(hide = true)]
    Receive {
        #[arg(conflicts_with = "skills", requires = "source_hash")]
        name: Option<String>,
        /// The project folder to copy into.
        #[arg(long, value_name = "DIR", conflicts_with = "to_global")]
        to_project: Option<PathBuf>,
        /// Copy into the global setup.
        #[arg(long)]
        to_global: bool,
        /// The sender's hash of the skill's folder (`wire skill-hash`); what arrives must match.
        #[arg(long, value_name = "HASH", requires = "name")]
        source_hash: Option<u64>,
        /// A skill and the sender's hash of its folder (repeat for more).
        #[arg(long = "skill", value_name = "NAME=HASH", required_unless_present = "name")]
        skills: Vec<String>,
        /// How plan lines name the sending machine.
        #[arg(long, value_name = "LABEL")]
        from: String,
        #[command(flatten)]
        apply: Apply,
    },
    /// Remove every broken link.
    CleanBroken {
        #[command(flatten)]
        apply: Apply,
    },
    /// Search skills.sh.
    Search { query: String },
    /// List the skills in a repository.
    Repo { source: String },
    /// Install from skills.sh and link into the targets (acts at once).
    Install {
        /// `owner/repo`, `owner/repo@skill`, or a GitHub or skills.sh URL.
        source: String,
        /// Install only these skills (default: all).
        #[arg(long, value_name = "NAME", num_args = 1..)]
        skill: Vec<String>,
        /// Link into these agents instead of the targets.
        #[arg(long, value_name = "AGENT", num_args = 1..)]
        to: Vec<String>,
    },
    /// List the skills that have an update.
    Updates,
    /// Update skills from their sources (default: every one with an update).
    Update { names: Vec<String> },
    /// Create a skill in .agents and link it into the targets (acts at once).
    New {
        name: String,
        #[arg(long, default_value = "")]
        description: String,
    },
}

/// Does a plan's steps: `Ok(n)` is how many were done, `Err` how many were and why it stopped.
type Run<'a> = Box<dyn FnOnce(&mut Journal) -> Result<usize, (usize, String)> + 'a>;

/// What a changing command would do, and how to do it.
struct Planned<'a> {
    actions: Vec<String>,
    conflicts: Vec<Conflict>,
    /// Skills the plan touches, in the scope they are in: their folders and links in every
    /// agent are hashed into the plan id.
    involved: Vec<(Scope, String)>,
    /// Skills coming from another machine with the sender's hash of each, also in the plan id.
    sent: Vec<(String, u64)>,
    label: String,
    run: Run<'a>,
}

impl<'a> Planned<'a> {
    fn of_actions(scope: &Scope, ctx: &'a Ctx, plan: sync::Plan, label: String) -> Self {
        let named = plan.actions.iter().map(action_skill).chain(plan.conflicts.iter().map(|c| c.skill.as_str()));
        let involved = named.map(|n| (scope.clone(), n.to_string())).collect();
        let (scope, actions) = (scope.clone(), plan.actions);
        Planned {
            actions: actions.iter().map(Action::describe).collect(),
            conflicts: plan.conflicts,
            involved,
            sent: vec![],
            label,
            run: Box::new(move |j| sync::apply(&scope, &ctx.paths, &actions, j)),
        }
    }
}

fn action_skill(a: &Action) -> &str {
    match a {
        Action::Adopt { skill, .. } | Action::AdoptLink { skill, .. } | Action::Link { skill, .. } | Action::Replace { skill, .. } => skill,
    }
}

/// An agent id the CLI links into (not the `.agents` store itself).
fn agent_index(id: &str) -> Result<usize, String> {
    match agents::index_of(id) {
        Some(i) if i != CANONICAL => Ok(i),
        _ => Err(format!("unknown agent {id}; valid ids: {}", AGENTS.iter().skip(1).map(|a| a.id).collect::<Vec<_>>().join(", "))),
    }
}

/// The plan's id: what it says, plus a hash of every involved skill in the canonical store
/// and in every agent. Planning and the `--apply` re-check both come through here.
fn id_of(ctx: &Ctx, p: &Planned) -> String {
    let mut state = Vec::new();
    for (scope, name) in &p.involved {
        for a in AGENTS {
            let at = a.dir(scope, &ctx.paths).join(name);
            if !state.iter().any(|(seen, _)| *seen == at) {
                let hash = tree_hash(&at);
                state.push((at, hash));
            }
        }
    }
    let mut lines = p.actions.clone();
    lines.extend(p.sent.iter().map(|(n, h)| format!("sent {n} as {h}")));
    plan_id(&lines, &state)
}

fn plan_json(p: &Planned, id: &str, applied: bool) -> Value {
    let conflicts: Vec<Value> = p.conflicts.iter().map(|c| json!({ "skill": c.skill, "reason": c.reason })).collect();
    json!({ "applied": applied, "plan_id": id, "actions": p.actions, "conflicts": conflicts })
}

fn conflicts_text(conflicts: &[Conflict]) -> String {
    let mut text = "Conflicts (left alone):".to_string();
    for c in conflicts {
        text.push_str(&format!("\n  - {}: {}", c.skill, c.reason));
    }
    text
}

fn plan_text(p: &Planned, id: &str) -> String {
    let mut parts = Vec::new();
    if !p.actions.is_empty() {
        let lines: String = p.actions.iter().map(|a| format!("\n  - {a}")).collect();
        parts.push(format!("Plan {id}:{lines}"));
    }
    if !p.conflicts.is_empty() {
        parts.push(conflicts_text(&p.conflicts));
    }
    if parts.is_empty() {
        return "Nothing to do".into();
    }
    if !p.actions.is_empty() {
        parts.push(format!("Run again with --apply --plan-id {id} to do it."));
    }
    parts.join("\n")
}

/// Prints the plan, or with `--apply` does it (unless it changed since `--plan-id`).
fn drive(ctx: &Ctx, mut plan: Planned, a: Apply, out: &mut Out, err: &mut dyn Write) -> Exit {
    if a.plan_id.is_some() && !a.apply {
        return usage(err, "--plan-id only goes with --apply");
    }
    let id = id_of(ctx, &plan);
    if !a.apply {
        out.emit(plan_text(&plan, &id), plan_json(&plan, &id, false));
        return Exit::Ok;
    }
    if let Some(old) = a.plan_id.as_deref().filter(|old| *old != id) {
        let mut v = plan_json(&plan, &id, false);
        v["plan_changed"] = json!(true);
        out.emit(format!("The plan changed since {old}; nothing was done. It now is:\n{}", plan_text(&plan, &id)), v);
        return Exit::PlanChanged;
    }
    if plan.actions.is_empty() {
        out.emit(plan_text(&plan, &id), plan_json(&plan, &id, false));
        return Exit::Ok;
    }
    // Backups of the last change go only now that there is a change to make.
    store::begin_change(&ctx.paths);
    let mut journal = Journal::default();
    let n = plan.actions.len();
    let run = std::mem::replace(&mut plan.run, Box::new(|_| Ok(0)));
    let result = run(&mut journal);
    let undoable = !journal.is_empty();
    if undoable {
        store::save_last_change(&ctx.paths, Some(&LastChange { label: plan.label.clone(), journal }));
    }
    match result {
        Ok(done) => {
            let mut v = plan_json(&plan, &id, true);
            v["done"] = json!(done);
            let mut text = format!("Done: {}", sync::plural(done, "change", "changes"));
            if !plan.conflicts.is_empty() {
                text.push_str(&format!("\n{}", conflicts_text(&plan.conflicts)));
            }
            out.emit(text, v);
            Exit::Ok
        }
        Err((done, e)) => {
            if out.json {
                let mut v = plan_json(&plan, &id, false);
                v["done"] = json!(done);
                v["error"] = json!(e);
                out.emit("", v);
            } else {
                let _ = writeln!(err, "Stopped after {done} of {n}: {e}{}", super::undo_hint(undoable));
            }
            Exit::Failed
        }
    }
}

pub fn run(cmd: Cmd, ctx: &mut Ctx, scope: &Scope, out: &mut Out, err: &mut dyn Write) -> Exit {
    // The skills.sh commands work on the CLI's own output, not on the inventory.
    match cmd {
        Cmd::Receive { name, to_project, to_global, source_hash, skills, from, apply } => {
            let dest = match dest_scope(ctx, to_project.as_deref(), to_global) {
                Ok(d) => d,
                Err(e) => return usage(err, &e),
            };
            let (sent, named) = match (name, source_hash) {
                (Some(name), Some(hash)) => (Ok(vec![(name, hash)]), false),
                (None, _) => (skills.iter().map(|s| sent_skill(s)).collect::<Result<Vec<_>, _>>(), true),
                (Some(_), None) => (Err("give --source-hash with the skill's name".to_string()), false),
            };
            let sent = match sent {
                Ok(s) => s,
                Err(e) => return usage(err, &e),
            };
            // Only an apply reads the tar; a plan must not wait on stdin.
            let tar = if apply.apply { ctx.stdin.take() } else { None };
            receive(ctx, Received { sent, named, dest, from, tar }, apply, out, err)
        }
        Cmd::Search { query } => search(&query, out, err),
        Cmd::Repo { source } => repo(&source, out, err),
        Cmd::Install { source, skill, to } => install(ctx, scope, &source, skill, to, out, err),
        Cmd::Updates => updates(ctx, scope, out, err),
        Cmd::Update { names } => update(ctx, scope, names, out, err),
        cmd => run_local(cmd, ctx, scope, out, err),
    }
}

fn run_local(cmd: Cmd, ctx: &Ctx, scope: &Scope, out: &mut Out, err: &mut dyn Write) -> Exit {
    let inv = scan::scan(scope, &ctx.paths);
    match cmd {
        Cmd::List => list(ctx, &inv, out),
        Cmd::Show { name } => show(&inv, &name, scope, out, err),
        Cmd::New { name, description } => new_skill(ctx, scope, &inv, &name, &description, out, err),
        Cmd::Sync { names, to, apply } => {
            // `.agents` is no agent to link into, but as the only one it asks for the move alone.
            let target = |t: &String| if t == AGENTS[CANONICAL].id { Ok(CANONICAL) } else { agent_index(t) };
            let targets = if to.is_empty() { Ok(ctx.targets()) } else { to.iter().map(target).collect::<Result<Vec<_>, _>>() };
            let targets = match targets {
                Ok(t) => t,
                Err(e) => return usage(err, &e),
            };
            if let Some(missing) = names.iter().find(|n| inv.get(n).is_none()) {
                return missing_skill(out, err, missing, scope);
            }
            let plan = if names.is_empty() { sync::plan(&inv, &targets, &ctx.paths) } else { sync::plan_many(&inv, &names, &targets, &ctx.paths) };
            let n = plan.actions.len();
            drive(ctx, Planned::of_actions(scope, ctx, plan, label::sync(n)), apply, out, err)
        }
        Cmd::Link { name, agent, apply } => {
            let agent = match agent_index(&agent) {
                Ok(a) => a,
                Err(e) => return usage(err, &e),
            };
            if inv.get(&name).is_none() {
                return missing_skill(out, err, &name, scope);
            }
            let plan = sync::plan_one(&inv, &name, &[agent], &ctx.paths);
            drive(ctx, Planned::of_actions(scope, ctx, plan, label::link(&name, agent)), apply, out, err)
        }
        Cmd::Unlink { name, agent, apply } => unlink(ctx, scope, &inv, &name, &agent, apply, out, err),
        Cmd::Remove { names, apply } => remove(ctx, scope, &inv, names, apply, out, err),
        Cmd::Trash { name, agent, apply } => trash(ctx, scope, &inv, &name, &agent, apply, out, err),
        Cmd::Copy { names, to_project, to_global, to_host: Some(host), apply } => {
            let checked = super::hosts::other_machine(&host).and_then(|()| to_project.as_deref().map_or(Ok(()), super::hosts::remote_folder));
            if let Err(e) = checked {
                return usage(err, &e);
            }
            let dest = match (to_project, to_global) {
                // A folder on the host: the host resolves it.
                (Some(dir), _) => Scope::Project(dir),
                (None, true) => Scope::Global,
                (None, false) => return usage(err, "say where to copy: --to-project DIR or --to-global"),
            };
            copy_to_host(ctx, scope, &inv, names, &host, &dest, apply, out, err)
        }
        Cmd::Copy { names, to_project, to_global, to_host: None, apply } => {
            let dest = match dest_scope(ctx, to_project.as_deref(), to_global) {
                Ok(d) => d,
                Err(e) => return usage(err, &e),
            };
            if dest == *scope {
                return usage(err, "that is the scope the skills already are in");
            }
            if let Some(missing) = names.iter().find(|n| inv.get(n).is_none()) {
                return missing_skill(out, err, missing, scope);
            }
            copy(ctx, scope, &inv, names, dest, apply, out, err)
        }
        Cmd::Search { .. } | Cmd::Repo { .. } | Cmd::Install { .. } | Cmd::Updates | Cmd::Update { .. } | Cmd::Receive { .. } => {
            unreachable!("routed in run")
        }
        Cmd::CleanBroken { apply } => {
            let broken = sync::broken_links(&inv);
            let plan = Planned {
                actions: broken.iter().map(|(s, a)| format!("Remove {}'s broken link to {s}", AGENTS[*a].name)).collect(),
                conflicts: vec![],
                involved: broken.iter().map(|(s, _)| (scope.clone(), s.clone())).collect(),
                sent: vec![],
                label: label::BROKEN.into(),
                run: Box::new(|j| sync::remove_broken(&inv, &ctx.paths, j).map_err(|e| (0, e))),
            };
            drive(ctx, plan, apply, out, err)
        }
    }
}

/// Refuses a plan for a skill the scope doesn't have.
fn missing_skill(out: &mut Out, err: &mut dyn Write, name: &str, scope: &Scope) -> Exit {
    failed(out, err, &format!("no skill named {name} in {}", scope.title()))
}

fn remove(ctx: &Ctx, scope: &Scope, inv: &Inventory, names: Vec<String>, apply: Apply, out: &mut Out, err: &mut dyn Write) -> Exit {
    if let Some(missing) = names.iter().find(|n| inv.get(n).is_none()) {
        return missing_skill(out, err, missing, scope);
    }
    let describe = |n: &String| {
        let s = inv.get(n).expect("checked above");
        let at: Vec<&str> = (0..AGENTS.len()).filter(|&a| !inv.shares_canonical[a] && s.placements[a].present()).map(|a| AGENTS[a].name).collect();
        format!("Remove {n} ({}; folders go to the trash)", at.join(", "))
    };
    let plan = Planned {
        actions: names.iter().map(describe).collect(),
        conflicts: vec![],
        involved: names.iter().map(|n| (scope.clone(), n.clone())).collect(),
        sent: vec![],
        label: label::remove(&names),
        run: Box::new(|j| {
            for (done, n) in names.iter().enumerate() {
                sync::remove(inv, &ctx.paths, n, j).map_err(|e| (done, e))?;
            }
            Ok(names.len())
        }),
    };
    drive(ctx, plan, apply, out, err)
}

#[allow(clippy::too_many_arguments)]
fn unlink(ctx: &Ctx, scope: &Scope, inv: &Inventory, name: &str, agent: &str, apply: Apply, out: &mut Out, err: &mut dyn Write) -> Exit {
    let agent = match agent_index(agent) {
        Ok(a) => a,
        Err(e) => return usage(err, &e),
    };
    let Some(s) = inv.get(name) else { return missing_skill(out, err, name, scope) };
    let (mut actions, mut conflicts) = (Vec::new(), Vec::new());
    match &s.placements[agent] {
        Placement::Linked | Placement::Link(_) | Placement::Broken(_) => actions.push(format!("Unlink {name} from {}", AGENTS[agent].name)),
        Placement::Dir => conflicts.push(Conflict { skill: name.into(), reason: format!("{} has its own folder, not a link", AGENTS[agent].name) }),
        Placement::Missing => {}
    }
    let plan = Planned {
        actions,
        conflicts,
        involved: vec![(scope.clone(), name.to_string())],
        sent: vec![],
        label: label::unlink(name, agent),
        run: Box::new(|j| sync::unlink(scope, &ctx.paths, name, agent, j).map(|_| 1).map_err(|e| (0, e))),
    };
    drive(ctx, plan, apply, out, err)
}

#[allow(clippy::too_many_arguments)]
fn copy(ctx: &Ctx, scope: &Scope, inv: &Inventory, names: Vec<String>, dest: Scope, apply: Apply, out: &mut Out, err: &mut dyn Write) -> Exit {
    let to = label::scope(&dest, &ctx.paths);
    let there = AGENTS[CANONICAL].dir(&dest, &ctx.paths);
    let (mut copying, mut conflicts) = (Vec::new(), Vec::new());
    for n in names {
        if std::fs::symlink_metadata(there.join(&n)).is_ok() {
            conflicts.push(Conflict { skill: n, reason: format!("already in {to}") });
        } else {
            copying.push(n);
        }
    }
    let targets = ctx.targets();
    let mut involved: Vec<(Scope, String)> = copying.iter().map(|n| (scope.clone(), n.clone())).collect();
    involved.extend(copying.iter().chain(conflicts.iter().map(|c| &c.skill)).map(|n| (dest.clone(), n.clone())));
    let linking = linking(&targets);
    let plan = Planned {
        actions: copying.iter().map(|n| format!("Copy {n} to {to} ({linking})")).collect(),
        conflicts,
        involved,
        sent: vec![],
        label: label::copy(&to),
        run: Box::new(|j| {
            for (done, n) in copying.iter().enumerate() {
                sync::copy_to(inv, &ctx.paths, n, &dest, &targets, j).map_err(|e| (done, e))?;
            }
            Ok(copying.len())
        }),
    };
    drive(ctx, plan, apply, out, err)
}

/// `skills copy --to-host`: the host plans the copy of every skill at once and, with its plan
/// id, takes them as one tar: one change there, which one `undo` there takes back.
#[allow(clippy::too_many_arguments)]
fn copy_to_host(ctx: &Ctx, scope: &Scope, inv: &Inventory, names: Vec<String>, host: &str, dest: &Scope, a: Apply, out: &mut Out, err: &mut dyn Write) -> Exit {
    if a.plan_id.is_some() && !a.apply {
        return usage(err, "--plan-id only goes with --apply");
    }
    let mut skills: Vec<(String, PathBuf)> = Vec::new();
    for n in names {
        if skills.iter().any(|(seen, _)| *seen == n) {
            continue;
        }
        let Some(s) = inv.get(&n) else { return missing_skill(out, err, &n, scope) };
        match s.source.as_deref().and_then(|p| std::fs::canonicalize(p).ok()) {
            Some(dir) => skills.push((n, dir)),
            None => return failed(out, err, &format!("{n} has no folder in {}", scope.title())),
        }
    }
    let (host, to) = match super::hosts::ready(ctx, host) {
        Ok(found) => found,
        Err(e) => return failed(out, err, &e),
    };
    let from = super::hosts::this(ctx);
    let copy = |apply: Option<&str>| copy_skills(from.as_ref(), crate::hosts::copy::this_machine(), scope, &skills, to.as_ref(), &host.name, dest, apply);
    // --apply alone applies the plan as it is now.
    let id = match (a.apply, a.plan_id) {
        (false, _) => None,
        (true, Some(id)) => Some(id),
        (true, None) => match copy(None) {
            Ok(plan) => plan["plan_id"].as_str().map(String::from),
            Err(e) => return failed(out, err, &e),
        },
    };
    match copy(id.as_deref()) {
        Ok(v) => super::hosts::print_remote(out, v),
        Err(e) => failed(out, err, &e),
    }
}

/// Where `--to-project DIR` or `--to-global` says to copy.
fn dest_scope(ctx: &Ctx, to_project: Option<&Path>, to_global: bool) -> Result<Scope, String> {
    match (to_project, to_global) {
        (Some(dir), _) => ctx.scope(Some(dir)),
        (None, true) => Ok(Scope::Global),
        (None, false) => Err("say where to copy: --to-project DIR or --to-global".into()),
    }
}

/// The links a copy makes, for its plan line. They depend on the targets, so the line
/// (and with it the plan id) changes when the targets do.
fn linking(targets: &[usize]) -> String {
    let names: Vec<&str> = targets.iter().map(|&t| AGENTS[t].name).collect();
    if names.is_empty() { "no links".to_string() } else { format!("link into {}", names.join(", ")) }
}

/// `NAME=HASH` from `--skill`. The hash is digits, so the last `=` splits it off.
fn sent_skill(arg: &str) -> Result<(String, u64), String> {
    let (name, hash) = arg.rsplit_once('=').ok_or_else(|| format!("--skill {arg}: give NAME=HASH"))?;
    let hash = hash.parse().map_err(|_| format!("--skill {arg}: the hash is not a number"))?;
    Ok((name.to_string(), hash))
}

/// Skills another machine sends, and where they go.
struct Received {
    sent: Vec<(String, u64)>,
    /// Each skill is a top-level folder of the tar (`--skill`), not the tar's contents.
    named: bool,
    dest: Scope,
    from: String,
    /// The tar of their folders; `None` is the process's stdin.
    tar: Option<Box<dyn Read>>,
}

/// The receiving end of a copy from another machine: the plan names the sender, and the id
/// covers the sender's hash of each skill, so a skill changed there since is a changed plan.
/// All of it is one change: one `undo` takes back every skill it placed.
fn receive(ctx: &Ctx, r: Received, apply: Apply, out: &mut Out, err: &mut dyn Write) -> Exit {
    for (i, (name, _)) in r.sent.iter().enumerate() {
        if let Err(e) = sync::check_folder_name(name) {
            return usage(err, &e);
        }
        if r.sent[..i].iter().any(|(n, _)| n == name) {
            return usage(err, &format!("{name} is given twice"));
        }
    }
    let Received { sent, named, dest, from, tar } = r;
    let to = label::scope(&dest, &ctx.paths);
    let targets = ctx.targets();
    let there = AGENTS[CANONICAL].dir(&dest, &ctx.paths);
    let (mut actions, mut conflicts, mut placing) = (Vec::new(), Vec::new(), Vec::new());
    for (name, _) in &sent {
        if std::fs::symlink_metadata(there.join(name)).is_ok() {
            conflicts.push(Conflict { skill: name.clone(), reason: format!("already in {to}") });
        } else {
            actions.push(format!("Copy {name} from {from} to {to} ({})", linking(&targets)));
            placing.push(name.clone());
        }
    }
    let names: Vec<String> = sent.iter().map(|(n, _)| n.clone()).collect();
    let plan = Planned {
        actions,
        conflicts,
        involved: names.iter().map(|n| (dest.clone(), n.clone())).collect(),
        sent: sent.clone(),
        label: label::receive(&placing, &from),
        run: Box::new(move |j| {
            let mut tar = tar.unwrap_or_else(|| Box::new(std::io::stdin()));
            let incoming = sync::Incoming { sent: &sent, named, place: &placing };
            sync::receive_many(&ctx.paths, &dest, &incoming, &mut tar, &targets, j)
        }),
    };
    drive(ctx, plan, apply, out, err)
}

/// Moves an agent's own copy of a skill to the trash, as the window's "Move this copy to the
/// trash" does. Links are not copies: `unlink` takes those.
#[allow(clippy::too_many_arguments)]
fn trash(ctx: &Ctx, scope: &Scope, inv: &Inventory, name: &str, agent: &str, apply: Apply, out: &mut Out, err: &mut dyn Write) -> Exit {
    let agent = match agent_index(agent) {
        Ok(a) => a,
        Err(e) => return usage(err, &e),
    };
    let Some(s) = inv.get(name) else { return missing_skill(out, err, name, scope) };
    let who = AGENTS[agent].name;
    let (mut actions, mut conflicts) = (Vec::new(), Vec::new());
    let mut conflict = |reason: String| conflicts.push(Conflict { skill: name.into(), reason });
    match &s.placements[agent] {
        _ if inv.shares_canonical[agent] => conflict(format!("{who} uses the .agents folder itself; remove the skill instead")),
        Placement::Dir => actions.push(format!("Move {who}'s copy of {name} to the trash")),
        Placement::Linked | Placement::Link(_) | Placement::Broken(_) => conflict(format!("{who} has a link, not its own copy; unlink it instead")),
        Placement::Missing => conflict(format!("{who} has no copy of {name}")),
    }
    let plan = Planned {
        actions,
        conflicts,
        involved: vec![(scope.clone(), name.to_string())],
        sent: vec![],
        label: label::trash_copy(name, agent),
        run: Box::new(|j| sync::trash_copy(scope, &ctx.paths, name, agent, j).map(|_| 1).map_err(|e| (0, e))),
    };
    drive(ctx, plan, apply, out, err)
}

/// Creates the skill and links it into the targets, as the window's New skill does. Like
/// the window, it does not become the last change.
fn new_skill(ctx: &Ctx, scope: &Scope, inv: &Inventory, name: &str, description: &str, out: &mut Out, err: &mut dyn Write) -> Exit {
    if let Err(e) = skill::valid_name(name) {
        return usage(err, &e);
    }
    let dir = AGENTS[CANONICAL].dir(scope, &ctx.paths).join(name);
    if inv.get(name).is_some() || std::fs::symlink_metadata(&dir).is_ok() {
        let _ = writeln!(err, "{name} already exists in {}", scope.title());
        return Exit::Failed;
    }
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join("SKILL.md"), skill::template(name, description))) {
        let _ = writeln!(err, "{}: {e}", dir.display());
        return Exit::Failed;
    }
    let plan = sync::plan_one(&scan::scan(scope, &ctx.paths), name, &ctx.targets(), &ctx.paths);
    let linked = sync::apply(scope, &ctx.paths, &plan.actions, &mut Journal::default());
    let path = dir.join("SKILL.md");
    let (done, note) = match &linked {
        Ok(n) => (*n, String::new()),
        Err((n, e)) => (*n, format!("\nCould not link it everywhere: {e}")),
    };
    out.emit(format!("Created {}{note}", path.display()), json!({ "created": name, "path": path, "linked": done }));
    if linked.is_ok() { Exit::Ok } else { Exit::Failed }
}

fn list(ctx: &Ctx, inv: &Inventory, out: &mut Out) -> Exit {
    let targets = ctx.targets();
    let mut table = format!("{:<28} {:<7} {}", "NAME", "SYNCED", "AGENTS");
    let mut rows = Vec::new();
    for s in &inv.skills {
        let synced = s.synced(&targets);
        let at: Vec<&str> = AGENTS.iter().zip(&s.placements).filter(|(_, p)| p.present()).map(|(a, _)| a.id).collect();
        table.push_str(&format!("\n{:<28} {:<7} {}", s.name, if synced { "yes" } else { "no" }, at.join(" ")));
        rows.push(json!({ "name": s.name, "description": s.description, "placements": placements(&s.placements), "synced": synced }));
    }
    out.emit(table, Value::Array(rows));
    Exit::Ok
}

fn show(inv: &Inventory, name: &str, scope: &Scope, out: &mut Out, err: &mut dyn Write) -> Exit {
    let Some(s) = inv.skills.iter().find(|s| s.name == name) else {
        let _ = writeln!(err, "no skill named {name} in {}", scope.title());
        return Exit::Failed;
    };
    let body_lines = s.manifest().map_or(0, |m| m.body.lines().count());
    let path = s.source.as_ref().map(|p| p.display().to_string());
    let mut text = format!("{}\n{}\n", s.name, s.description);
    text.push_str(&format!("path: {}\nbody: {body_lines} lines", path.as_deref().unwrap_or("-")));
    for (a, p) in AGENTS.iter().zip(&s.placements) {
        text.push_str(&format!("\n{:<10} {}", a.id, placement_label(p)));
    }
    out.emit(text, json!({ "name": s.name, "description": s.description, "path": path, "body_lines": body_lines, "placements": placements(&s.placements) }));
    Exit::Ok
}

fn placement_label(p: &Placement) -> &'static str {
    match p {
        Placement::Dir => "dir",
        Placement::Link(_) => "link",
        Placement::Linked => "linked",
        Placement::Broken(_) => "broken",
        Placement::Missing => "missing",
    }
}

fn placements(ps: &[Placement]) -> Value {
    AGENTS.iter().zip(ps).map(|(a, p)| (a.id.to_string(), Value::from(placement_label(p)))).collect::<serde_json::Map<_, _>>().into()
}

fn search(query: &str, out: &mut Out, err: &mut dyn Write) -> Exit {
    let found = match remote::search(query) {
        Ok(f) => f,
        Err(e) => return failed(out, err, &e),
    };
    let text = found.iter().map(|r| format!("{:<44} {:>9} installs", r.id, r.installs)).collect::<Vec<_>>().join("\n");
    let rows = found.iter().map(|r| json!({ "id": r.id, "source": r.source, "skill": r.skill_id, "name": r.name, "installs": r.installs }));
    out.emit(if text.is_empty() { "No skills found".to_string() } else { text }, Value::Array(rows.collect()));
    Exit::Ok
}

/// Lists a repository named as `install` takes it, as the window's field does.
fn repo(source: &str, out: &mut Out, err: &mut dyn Write) -> Exit {
    let Some((source, _)) = remote::parse_source(source) else {
        return usage(err, &format!("{source} is not a skills source; use owner/repo, owner/repo@skill or a URL"));
    };
    let skills = match remote::list_repo(&source) {
        Ok(s) => s,
        Err(e) => return failed(out, err, &e),
    };
    let text = skills.iter().map(|(n, d)| format!("{n:<28} {d}")).collect::<Vec<_>>().join("\n");
    out.emit(text, Value::Array(skills.iter().map(|(n, d)| json!({ "name": n, "description": d })).collect()));
    Exit::Ok
}

/// Installs into `.agents` and links into the targets, as the window does. Like the window,
/// it does not become the last change.
fn install(ctx: &Ctx, scope: &Scope, source: &str, mut skills: Vec<String>, to: Vec<String>, out: &mut Out, err: &mut dyn Write) -> Exit {
    let Some((source, at)) = remote::parse_source(source) else {
        return usage(err, &format!("{source} is not a skills source; use owner/repo, owner/repo@skill or a URL"));
    };
    let targets = if to.is_empty() { Ok(ctx.targets()) } else { to.iter().map(|t| agent_index(t)).collect::<Result<Vec<_>, _>>() };
    let targets = match targets {
        Ok(t) => t,
        Err(e) => return usage(err, &e),
    };
    skills.extend(at);
    skills.dedup();
    let log = match remote::install(&source, &skills, scope, &ctx.paths) {
        Ok(l) => l,
        Err(e) => return failed(out, err, &e),
    };
    let installed = if skills.is_empty() { vec!["*".to_string()] } else { skills.clone() };
    let linked = sync::link_installed(scope, &ctx.paths, &targets, &installed);
    let (n, note) = match &linked {
        Ok(n) => (*n, String::new()),
        Err(e) => (0, format!("\nCould not link it: {e}")),
    };
    out.emit(format!("Installed {} from {source} into {} ({}){note}", installed.join(", "), scope.title(), sync::plural(n, "link", "links")), json!({ "installed": installed, "linked": n, "log": log }));
    if linked.is_ok() { Exit::Ok } else { Exit::Failed }
}

fn updates(ctx: &Ctx, scope: &Scope, out: &mut Out, err: &mut dyn Write) -> Exit {
    match remote::check_updates(scope, &ctx.paths) {
        Ok(names) => {
            let text = if names.is_empty() { "Everything is up to date".to_string() } else { names.join("\n") };
            out.emit(text, json!({ "updates": names }));
            Exit::Ok
        }
        Err(e) => failed(out, err, &e),
    }
}

fn update(ctx: &Ctx, scope: &Scope, mut names: Vec<String>, out: &mut Out, err: &mut dyn Write) -> Exit {
    if names.is_empty() {
        names = match remote::check_updates(scope, &ctx.paths) {
            Ok(n) => n,
            Err(e) => return failed(out, err, &e),
        };
        if names.is_empty() {
            out.emit("Everything is up to date", json!({ "updated": [], "log": "" }));
            return Exit::Ok;
        }
    }
    match remote::update(&names, scope, &ctx.paths) {
        Ok(log) => {
            out.emit(format!("Updated {}", names.join(", ")), json!({ "updated": names, "log": log }));
            Exit::Ok
        }
        Err(e) => failed(out, err, &e),
    }
}
