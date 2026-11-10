//! `prism wire`: what a window needs from another machine, as JSON. Hidden from help: it is
//! the window's, not a person's.

use std::io::Write;

use clap::Subcommand;
use serde::Serialize;
use serde_json::json;

use super::{Ctx, Exit, Out, failed};
use crate::agents::Scope;
use crate::{mcp, planid, scan, wire};

#[derive(Subcommand)]
pub enum Cmd {
    /// The window's state: skills, MCP servers (masked), counts, settings.
    State,
    /// One MCP server, unmasked: for showing its values and copying it.
    Server { name: String },
    /// A skill's SKILL.md body, as the inspector previews it.
    SkillMd { name: String },
    /// The hash of a skill's folder, for copies: of the folder it resolves to, as a receiver
    /// unpacking its contents sees it.
    SkillHash { name: String },
}

pub fn run(cmd: Cmd, ctx: &Ctx, scope: &Scope, out: &mut Out, err: &mut dyn Write) -> Exit {
    match cmd {
        Cmd::State => print(out, &wire::state(scope, &ctx.paths, &ctx.settings)),
        Cmd::Server { name } => match mcp::scan::scan(scope, &ctx.paths).get(&name) {
            Some(server) => print(out, server),
            None => failed(out, err, &format!("No MCP server named {name}")),
        },
        Cmd::SkillMd { name } => match scan::scan(scope, &ctx.paths).get(&name) {
            Some(skill) => print(out, &json!({ "body": skill.manifest().map(|m| m.body) })),
            None => failed(out, err, &format!("No skill named {name}")),
        },
        Cmd::SkillHash { name } => match scan::scan(scope, &ctx.paths).get(&name) {
            Some(skill) => {
                let folder = skill.source.as_deref().and_then(|p| std::fs::canonicalize(p).ok());
                print(out, &json!({ "hash": folder.as_deref().and_then(planid::tree_hash) }))
            }
            None => failed(out, err, &format!("No skill named {name}")),
        },
    }
}

/// One line of compact JSON, whatever `--json` says.
fn print(out: &mut Out, value: &impl Serialize) -> Exit {
    match serde_json::to_string(value) {
        Ok(text) => {
            let _ = writeln!(out.out, "{text}");
            Exit::Ok
        }
        Err(e) => {
            let _ = writeln!(out.out, "{}", json!({ "error": e.to_string() }));
            Exit::Failed
        }
    }
}
