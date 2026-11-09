//! `prism mcp`: the MCP servers found in every agent's own config.

use std::io::Write;

use clap::Subcommand;
use serde_json::{Value, json};

use super::{Ctx, Exit, Out};
use crate::agents::{Paths, Scope, tilde};
use crate::mcp::agents::MCP_AGENTS;
use crate::mcp::model::{EnvValue, McpCopy, Place, Transport, masked_args, masked_url};
use crate::mcp::scan::{self, FieldDiff, McpServer, Status};

#[derive(Subcommand)]
pub enum Cmd {
    /// List the servers, one row each, with the agents that have them.
    List,
    /// Show one server's copies and where they differ. Secrets print as `***`.
    Show {
        name: String,
        /// Print env, header and argument values as written.
        #[arg(long)]
        show_secrets: bool,
    },
}

pub fn run(cmd: Cmd, ctx: &mut Ctx, scope: &Scope, out: &mut Out, err: &mut dyn Write) -> Exit {
    match cmd {
        Cmd::List => list(ctx, scope, out),
        Cmd::Show { name, show_secrets } => show(ctx, scope, &name, show_secrets, out, err),
    }
}

fn agent_ids(agents: impl IntoIterator<Item = usize>) -> Vec<&'static str> {
    agents.into_iter().map(|a| MCP_AGENTS[a].id).collect()
}

fn status_key(s: Status) -> &'static str {
    match s {
        Status::InSync => "in_sync",
        Status::Differs => "differs",
        Status::ReadOnly => "read_only",
    }
}

fn status_word(s: Status) -> &'static str {
    match s {
        Status::InSync => "in sync",
        Status::Differs => "differs",
        Status::ReadOnly => "read-only",
    }
}

fn kind(t: &Transport) -> &'static str {
    match t {
        Transport::Stdio { .. } => "stdio",
        Transport::Http { .. } => "http",
    }
}

/// A parse error's first line. TOML errors go on to quote the offending line of the file, which
/// may be a secret; the first line is the position.
fn safe_error(message: &str) -> &str {
    message.lines().next().unwrap_or_default()
}

fn list(ctx: &Ctx, scope: &Scope, out: &mut Out) -> Exit {
    let inv = scan::scan(scope, &ctx.paths);
    let mut human = Vec::new();
    // One agent can hold a name in two places (user and local scope); the row names it once.
    let agents_of = |s: &McpServer| {
        let mut agents: Vec<usize> = s.copies.iter().map(|c| c.agent).collect();
        agents.dedup();
        agent_ids(agents)
    };
    for s in &inv.servers {
        let mut line = format!("{:<24} {:<28} {}", s.name, agents_of(s).join(","), status_word(s.status));
        if s.inherited {
            line.push_str(" (global)");
        }
        if !s.disabled_for.is_empty() {
            line.push_str(&format!("  disabled: {}", agent_ids(s.disabled_for.iter().copied()).join(",")));
        }
        human.push(line);
    }
    if human.is_empty() {
        human.push("No MCP servers".into());
    }
    for m in &inv.merges {
        human.push(format!("Maybe one server: {} ({})", m.names.join(", "), m.reason));
    }
    for (file, e) in &inv.errors {
        human.push(format!("Could not read {}: {}", tilde(file, &ctx.paths), safe_error(e)));
    }
    let servers: Vec<Value> = inv
        .servers
        .iter()
        .map(|s| {
            json!({
                "name": s.name,
                "inherited": s.inherited,
                "status": status_key(s.status),
                "agents": agents_of(s),
                "disabled_for": agent_ids(s.disabled_for.iter().copied()),
                "transport": kind(&s.copies[0].server.transport),
            })
        })
        .collect();
    let merges: Vec<Value> = inv.merges.iter().map(|m| json!({ "names": m.names, "reason": m.reason })).collect();
    let errors: Vec<Value> = inv.errors.iter().map(|(f, e)| json!({ "file": tilde(f, &ctx.paths), "error": safe_error(e) })).collect();
    out.emit(human.join("\n"), json!({ "servers": servers, "merges": merges, "errors": errors }));
    Exit::Ok
}

fn show(ctx: &Ctx, scope: &Scope, name: &str, secrets: bool, out: &mut Out, err: &mut dyn Write) -> Exit {
    let inv = scan::scan(scope, &ctx.paths);
    let Some(server) = inv.get(name) else {
        let _ = writeln!(err, "No MCP server named {name}");
        return Exit::Failed;
    };
    let copies: Vec<Value> = server.copies.iter().map(|c| copy_json(c, &ctx.paths, secrets)).collect();
    let diffs: Vec<Value> = server.diffs.iter().map(diff_json).collect();
    let json = json!({ "name": server.name, "copies": copies, "diffs": diffs });
    out.emit(human_show(server, &ctx.paths, secrets), json);
    Exit::Ok
}

fn place_key(p: &Place) -> &'static str {
    match p {
        Place::Global => "global",
        Place::ClaudeLocal(_) => "local",
        Place::Project(_) => "project",
    }
}

/// The text shown for an env or header value.
fn value(v: &EnvValue, secrets: bool) -> String {
    if secrets { raw(v) } else { v.masked() }
}

/// References as written in the model (`${NAME}`, `${NAME:-default}`).
fn raw(v: &EnvValue) -> String {
    use crate::mcp::model::Part;
    v.0.iter()
        .map(|p| match p {
            Part::Text(t) => t.clone(),
            Part::Var { name, default: None } => format!("${{{name}}}"),
            Part::Var { name, default: Some(d) } => format!("${{{name}:-{d}}}"),
        })
        .collect()
}

fn text(s: &str, secrets: bool) -> String {
    if secrets { s.into() } else { masked_url(s) }
}

fn transport_json(t: &Transport, secrets: bool) -> Value {
    match t {
        Transport::Stdio { command, args, env, cwd } => {
            let args = if secrets { args.clone() } else { masked_args(args) };
            let env: serde_json::Map<_, _> = env.iter().map(|(k, v)| (k.clone(), json!(value(v, secrets)))).collect();
            json!({ "type": "stdio", "command": text(command, secrets), "args": args, "env": env, "cwd": cwd.as_deref().map(|c| text(c, secrets)) })
        }
        Transport::Http { url, headers, sse } => {
            let headers: serde_json::Map<_, _> = headers.iter().map(|(k, v)| (k.clone(), json!(value(v, secrets)))).collect();
            json!({ "type": "http", "sse": sse, "url": text(url, secrets), "headers": headers })
        }
    }
}

fn copy_json(c: &McpCopy, paths: &Paths, secrets: bool) -> Value {
    json!({
        "agent": MCP_AGENTS[c.agent].id,
        "file": tilde(&c.file, paths),
        "place": place_key(&c.place),
        "enabled": c.enabled,
        "read_only": c.read_only,
        "transport": transport_json(&c.server.transport, secrets),
        // Extras may hold secrets, so only their names show.
        "extra_keys": c.extras.keys().collect::<Vec<_>>(),
    })
}

fn diff_json(d: &FieldDiff) -> Value {
    let values: Vec<Value> = d.values.iter().map(|(a, v)| json!({ "agent": MCP_AGENTS[*a].id, "value": v })).collect();
    json!({ "field": d.field, "values": values })
}

fn human_show(s: &McpServer, paths: &Paths, secrets: bool) -> String {
    let mut lines = vec![s.name.clone()];
    for c in &s.copies {
        let mut head = format!("  {} ({}) {}", MCP_AGENTS[c.agent].id, place_key(&c.place), tilde(&c.file, paths));
        if let Place::ClaudeLocal(p) | Place::Project(p) = &c.place {
            head.push_str(&format!(" [{}]", tilde(p, paths)));
        }
        head.push_str(if c.enabled { "  enabled" } else { "  disabled" });
        if let Some(app) = &c.read_only {
            head.push_str(&format!("  read-only (managed by {app})"));
        }
        lines.push(head);
        match transport_json(&c.server.transport, secrets) {
            Value::Object(t) => {
                for (key, v) in t.iter().filter(|(k, _)| k.as_str() != "type" && k.as_str() != "sse") {
                    match v {
                        Value::Null => {}
                        Value::Object(m) if m.is_empty() => {}
                        Value::Array(a) if a.is_empty() => {}
                        Value::Object(m) => {
                            lines.push(format!("    {key}:"));
                            lines.extend(m.iter().map(|(k, v)| format!("      {k} = {}", v.as_str().unwrap_or_default())));
                        }
                        Value::Array(a) => lines.push(format!("    {key}: {}", a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "))),
                        v => lines.push(format!("    {key}: {}", v.as_str().unwrap_or_default())),
                    }
                }
            }
            _ => unreachable!("transport_json returns an object"),
        }
        if !c.extras.is_empty() {
            lines.push(format!("    other fields: {}", c.extras.keys().cloned().collect::<Vec<_>>().join(", ")));
        }
    }
    if !s.diffs.is_empty() {
        lines.push("Differences:".into());
        for d in &s.diffs {
            lines.push(format!("  {}", d.field));
            for (a, v) in &d.values {
                lines.push(format!("    {:<16} {}", MCP_AGENTS[*a].id, v.as_deref().unwrap_or("(none)")));
            }
        }
    }
    lines.join("\n")
}
