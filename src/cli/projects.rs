//! `prism projects` and `prism targets`: the projects Prism lists and the agents it syncs into.

use std::io::Write;
use std::path::PathBuf;

use clap::Subcommand;
use serde_json::{Value, json};

use super::{Ctx, Exit, Out, usage};
use crate::agents::{self, AGENTS};
use crate::projects;

#[derive(Subcommand)]
pub enum Cmd {
    /// List the projects Prism knows.
    List,
    /// Add a project folder.
    Add { dir: PathBuf },
    /// Forget a project (its files stay).
    Remove { dir: PathBuf },
    /// Look for projects with skills under a folder, without adding them.
    Find { dir: PathBuf },
}

#[derive(Subcommand)]
pub enum TargetsCmd {
    /// Sync into exactly these agents.
    Set {
        #[arg(required = true)]
        agents: Vec<String>,
    },
}

/// A folder that exists, resolved like `--project` so saved paths match what scans use.
fn folder(dir: &PathBuf) -> Result<PathBuf, String> {
    std::fs::canonicalize(dir).ok().filter(|p| p.is_dir()).ok_or_else(|| format!("{} is not a folder", dir.display()))
}

pub fn run(cmd: Cmd, ctx: &mut Ctx, out: &mut Out, err: &mut dyn Write) -> Exit {
    match cmd {
        Cmd::List => {
            let rows: Vec<(PathBuf, usize)> = ctx
                .settings
                .projects
                .iter()
                .map(|p| (p.clone(), crate::scan::scan(&agents::Scope::Project(p.clone()), &ctx.paths).skills.len()))
                .collect();
            table(out, ctx, &rows);
            Exit::Ok
        }
        Cmd::Add { dir } => {
            let dir = match folder(&dir) {
                Ok(d) => d,
                Err(e) => return usage(err, &e),
            };
            let added = ctx.settings.add_project(dir.clone());
            ctx.settings.save_to(&ctx.paths);
            let shown = agents::tilde(&dir, &ctx.paths);
            out.emit(if added { format!("Added {shown}") } else { format!("{shown} is already listed") }, json!({ "path": dir, "added": added }));
            Exit::Ok
        }
        Cmd::Remove { dir } => {
            // A folder that is gone can still be forgotten, so fall back to the path as typed.
            let dir = folder(&dir).unwrap_or(dir);
            let before = ctx.settings.projects.len();
            ctx.settings.projects.retain(|p| *p != dir);
            let removed = ctx.settings.projects.len() < before;
            ctx.settings.save_to(&ctx.paths);
            let shown = agents::tilde(&dir, &ctx.paths);
            out.emit(if removed { format!("Removed {shown}") } else { format!("{shown} was not listed") }, json!({ "path": dir, "removed": removed }));
            Exit::Ok
        }
        Cmd::Find { dir } => {
            let dir = match folder(&dir) {
                Ok(d) => d,
                Err(e) => return usage(err, &e),
            };
            let found = projects::find_projects(&dir, &ctx.paths);
            table(out, ctx, &found);
            Exit::Ok
        }
    }
}

fn table(out: &mut Out, ctx: &Ctx, rows: &[(PathBuf, usize)]) {
    let mut text = format!("{:<6} {}", "SKILLS", "PATH");
    for (p, n) in rows {
        text.push_str(&format!("\n{n:<6} {}", agents::tilde(p, &ctx.paths)));
    }
    let json = rows.iter().map(|(p, n)| json!({ "path": p, "skills": n })).collect();
    out.emit(text, Value::Array(json));
}

pub fn run_targets(cmd: TargetsCmd, ctx: &mut Ctx, out: &mut Out, err: &mut dyn Write) -> Exit {
    let TargetsCmd::Set { agents } = cmd;
    let valid: Vec<&str> = AGENTS.iter().skip(1).map(|a| a.id).collect();
    if let Some(bad) = agents.iter().find(|a| !valid.contains(&a.as_str())) {
        return usage(err, &format!("unknown agent {bad}; valid ids: {}", valid.join(", ")));
    }
    let mut ids: Vec<String> = Vec::new();
    for a in agents {
        if !ids.contains(&a) {
            ids.push(a);
        }
    }
    ctx.settings.targets = Some(ids.clone());
    ctx.settings.save_to(&ctx.paths);
    out.emit(format!("Targets: {}", ids.join(", ")), json!({ "targets": ids }));
    Exit::Ok
}
