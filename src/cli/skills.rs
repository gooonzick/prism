//! `prism skills`: what is installed in a scope, and changing it. Every command that
//! changes something prints a plan first and acts only with `--apply`.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use serde_json::{Value, json};

use super::{Ctx, Exit, Out, usage};
use crate::agents::{self, AGENTS, CANONICAL, Scope};
use crate::fsops::Journal;
use crate::planid::{plan_id, tree_hash};
use crate::scan::{self, Inventory, Placement};
use crate::skill;
use crate::store::{self, LastChange};
use crate::sync::{self, Action, Conflict, label};

/// `--apply`, and `--plan-id` to refuse a plan that changed since it was printed.
#[derive(Args)]
pub struct Apply {
    /// Do it, instead of only printing the plan.
    #[arg(long)]
    apply: bool,
    /// With --apply: stop (exit 4) unless the plan still has this id.
    #[arg(long, value_name = "ID")]
    plan_id: Option<String>,
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
        /// Link into these agents instead of the targets.
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
    /// Copy skills to a project or to the global setup.
    Copy {
        #[arg(required = true)]
        names: Vec<String>,
        /// The project folder to copy into.
        #[arg(long, value_name = "DIR", conflicts_with = "to_global")]
        to_project: Option<PathBuf>,
        /// Copy into the global setup.
        #[arg(long)]
        to_global: bool,
        #[command(flatten)]
        apply: Apply,
    },
    /// Remove every broken link.
    CleanBroken {
        #[command(flatten)]
        apply: Apply,
    },
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
    plan_id(&p.actions, &state)
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
    if !journal.is_empty() {
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
                let _ = writeln!(err, "Stopped after {done} of {n}: {e}");
            }
            Exit::Failed
        }
    }
}

pub fn run(cmd: Cmd, ctx: &Ctx, scope: &Scope, out: &mut Out, err: &mut dyn Write) -> Exit {
    let inv = scan::scan(scope, &ctx.paths);
    match cmd {
        Cmd::List => list(ctx, &inv, out),
        Cmd::Show { name } => show(&inv, &name, scope, out, err),
        Cmd::New { name, description } => new_skill(ctx, scope, &inv, &name, &description, out, err),
        Cmd::Sync { names, to, apply } => {
            let targets = if to.is_empty() { Ok(ctx.targets()) } else { to.iter().map(|t| agent_index(t)).collect::<Result<Vec<_>, _>>() };
            let targets = match targets {
                Ok(t) => t,
                Err(e) => return usage(err, &e),
            };
            if let Some(missing) = names.iter().find(|n| inv.get(n).is_none()) {
                return missing_skill(err, missing, scope);
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
                return missing_skill(err, &name, scope);
            }
            let plan = sync::plan_one(&inv, &name, &[agent], &ctx.paths);
            drive(ctx, Planned::of_actions(scope, ctx, plan, label::link(&name, agent)), apply, out, err)
        }
        Cmd::Unlink { name, agent, apply } => unlink(ctx, scope, &inv, &name, &agent, apply, out, err),
        Cmd::Remove { names, apply } => remove(ctx, scope, &inv, names, apply, out, err),
        Cmd::Copy { names, to_project, to_global, apply } => {
            let dest = match (to_project.as_deref(), to_global) {
                (Some(dir), _) => ctx.scope(Some(dir)),
                (None, true) => Ok(Scope::Global),
                (None, false) => return usage(err, "say where to copy: --to-project DIR or --to-global"),
            };
            let dest = match dest {
                Ok(d) => d,
                Err(e) => return usage(err, &e),
            };
            if dest == *scope {
                return usage(err, "that is the scope the skills already are in");
            }
            if let Some(missing) = names.iter().find(|n| inv.get(n).is_none()) {
                return missing_skill(err, missing, scope);
            }
            copy(ctx, scope, &inv, names, dest, apply, out, err)
        }
        Cmd::CleanBroken { apply } => {
            let broken = sync::broken_links(&inv);
            let plan = Planned {
                actions: broken.iter().map(|(s, a)| format!("Remove {}'s broken link to {s}", AGENTS[*a].name)).collect(),
                conflicts: vec![],
                involved: broken.iter().map(|(s, _)| (scope.clone(), s.clone())).collect(),
                label: label::BROKEN.into(),
                run: Box::new(|j| sync::remove_broken(&inv, &ctx.paths, j).map_err(|e| (0, e))),
            };
            drive(ctx, plan, apply, out, err)
        }
    }
}

fn missing_skill(err: &mut dyn Write, name: &str, scope: &Scope) -> Exit {
    let _ = writeln!(err, "no skill named {name} in {}", scope.title());
    Exit::Failed
}

fn remove(ctx: &Ctx, scope: &Scope, inv: &Inventory, names: Vec<String>, apply: Apply, out: &mut Out, err: &mut dyn Write) -> Exit {
    if let Some(missing) = names.iter().find(|n| inv.get(n).is_none()) {
        return missing_skill(err, missing, scope);
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
    let Some(s) = inv.get(name) else { return missing_skill(err, name, scope) };
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
    // The links apply makes depend on the targets, so they are part of what the id covers.
    let linking: Vec<&str> = targets.iter().map(|&t| AGENTS[t].name).collect();
    let linking = if linking.is_empty() { "no links".to_string() } else { format!("link into {}", linking.join(", ")) };
    let plan = Planned {
        actions: copying.iter().map(|n| format!("Copy {n} to {to} ({linking})")).collect(),
        conflicts,
        involved,
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
