//! `prism doctor`: what is broken in a scope, read-only. Exit 3 when anything is.

use std::collections::HashSet;
use std::path::PathBuf;

use serde_json::json;

use super::{Ctx, Exit, Out};
use crate::agents::{self, AGENTS, Scope};
use crate::mcp::scan::{self as mcp_scan, Status};
use crate::scan::{self, Placement};
use crate::sync;

struct Problem {
    kind: &'static str,
    scope: String,
    subject: String,
    detail: String,
}

/// The scopes to check: the named one, or with `--all` global plus every listed project.
pub fn run(all: bool, scope: &Scope, ctx: &Ctx, out: &mut Out) -> Exit {
    let mut problems = Vec::new();
    let mut seen = HashSet::new();
    if all {
        check(&Scope::Global, ctx, true, &mut seen, &mut problems);
        for project in &ctx.settings.projects {
            if project.is_dir() {
                check(&Scope::Project(project.clone()), ctx, true, &mut seen, &mut problems);
            } else {
                let shown = agents::tilde(project, &ctx.paths);
                problems.push(Problem { kind: "missing_project", scope: shown.clone(), subject: shown, detail: "the folder no longer exists".into() });
            }
        }
    } else {
        check(scope, ctx, false, &mut seen, &mut problems);
    }
    report(&problems, out);
    if problems.is_empty() { Exit::Ok } else { Exit::Problems }
}

fn scope_name(scope: &Scope, ctx: &Ctx) -> String {
    match scope {
        Scope::Global => "global".into(),
        Scope::Project(p) => agents::tilde(p, &ctx.paths),
    }
}

/// `all`: every scope is being checked, so what belongs to the global scope is reported there only.
fn check(scope: &Scope, ctx: &Ctx, all: bool, seen: &mut HashSet<PathBuf>, problems: &mut Vec<Problem>) {
    let name = scope_name(scope, ctx);
    let mut add = |kind, subject: String, detail: String| problems.push(Problem { kind, scope: name.clone(), subject, detail });

    let inv = scan::scan(scope, &ctx.paths);
    for (skill, agent) in sync::broken_links(&inv) {
        let target = match inv.get(&skill).map(|s| &s.placements[agent]) {
            Some(Placement::Broken(t)) => agents::tilde(t, &ctx.paths),
            _ => String::new(),
        };
        add("broken_link", skill, format!("{} links to {target}, which is gone", AGENTS[agent].name));
    }
    for s in &inv.skills {
        let Some(dir) = s.source.as_deref() else { continue };
        let detail = match crate::skill::read(dir) {
            None => "SKILL.md is unreadable".to_string(),
            Some(m) => {
                let blank = |v: &Option<String>| v.as_deref().is_none_or(|t| t.trim().is_empty());
                let missing: Vec<&str> = [("name", blank(&m.name)), ("description", blank(&m.description))].into_iter().filter(|(_, b)| *b).map(|(f, _)| f).collect();
                if missing.is_empty() {
                    continue;
                }
                format!("SKILL.md has no {}", missing.join(" or "))
            }
        };
        add("invalid_skill", s.name.clone(), detail);
    }

    let mcp = mcp_scan::scan(scope, &ctx.paths);
    for s in mcp.servers.iter().filter(|s| s.status == Status::Differs && !(all && s.inherited)) {
        let fields: Vec<&str> = s.diffs.iter().map(|d| d.field.as_str()).collect();
        // A server a project only inherits drifts in the global scope; say so when it shows up here.
        let prefix = if s.inherited { "global: " } else { "" };
        add("mcp_differs", s.name.clone(), format!("{prefix}{}", fields.join(", ")));
    }
    // A project scan reads the global files too; under --all each file is reported once.
    for (file, message) in mcp.errors.iter().filter(|(f, _)| !all || seen.insert(f.clone())) {
        add("mcp_unreadable", agents::tilde(file, &ctx.paths), message.clone());
    }
}

fn report(problems: &[Problem], out: &mut Out) {
    let mut text = String::new();
    let mut last: Option<&str> = None;
    for p in problems {
        if last != Some(p.scope.as_str()) {
            if last.is_some() {
                text.push('\n');
            }
            text.push_str(&format!("{}\n", p.scope));
            last = Some(&p.scope);
        }
        text.push_str(&format!("  {:<15} {}: {}\n", p.kind, p.subject, p.detail));
    }
    let human = if problems.is_empty() { "No problems found.".to_string() } else { text.trim_end().to_string() };
    let rows: Vec<_> = problems.iter().map(|p| json!({ "kind": p.kind, "scope": p.scope, "subject": p.subject, "detail": p.detail })).collect();
    out.emit(human, json!({ "problems": rows }));
}
