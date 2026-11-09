//! `prism skills list|show`: what is installed in a scope, read-only.

use std::io::Write;

use clap::Subcommand;
use serde_json::{Value, json};

use super::{Ctx, Exit, Out};
use crate::agents::{AGENTS, Scope};
use crate::scan::{self, Placement};

#[derive(Subcommand)]
pub enum Cmd {
    /// List the skills in the scope.
    List,
    /// Show one skill.
    Show { name: String },
}

pub fn run(cmd: Cmd, ctx: &Ctx, scope: &Scope, out: &mut Out, err: &mut dyn Write) -> Exit {
    let inv = scan::scan(scope, &ctx.paths);
    match cmd {
        Cmd::List => {
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
        Cmd::Show { name } => {
            let Some(s) = inv.skills.iter().find(|s| s.name == name) else {
                let _ = writeln!(err, "no skill named {name} in {}", scope.title());
                return Exit::Failed;
            };
            let body_lines = s.manifest().map_or(0, |m| m.body.lines().count());
            let path = s.source.as_ref().map(|p| p.display().to_string());
            let mut text = format!("{}\n{}\n", s.name, s.description);
            text.push_str(&format!("path: {}\nbody: {body_lines} lines", path.as_deref().unwrap_or("-")));
            for (a, p) in AGENTS.iter().zip(&s.placements) {
                text.push_str(&format!("\n{:<10} {}", a.id, label(p)));
            }
            out.emit(text, json!({ "name": s.name, "description": s.description, "path": path, "body_lines": body_lines, "placements": placements(&s.placements) }));
            Exit::Ok
        }
    }
}

fn label(p: &Placement) -> &'static str {
    match p {
        Placement::Dir => "dir",
        Placement::Link(_) => "link",
        Placement::Linked => "linked",
        Placement::Broken(_) => "broken",
        Placement::Missing => "missing",
    }
}

fn placements(ps: &[Placement]) -> Value {
    AGENTS.iter().zip(ps).map(|(a, p)| (a.id.to_string(), Value::from(label(p)))).collect::<serde_json::Map<_, _>>().into()
}
