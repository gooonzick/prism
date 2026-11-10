//! `prism mcp`: the MCP servers found in every agent's own config.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use clap::{ArgGroup, Subcommand};
use serde_json::{Value, json};

use super::skills::Apply;
use super::{Ctx, Exit, Out, failed, usage};
use crate::agents::{Paths, Scope, tilde};
use crate::fsops::{Journal, content_hash};
use crate::hosts::copy::copy_server;
use crate::mcp::agents::{MCP_AGENTS, index_of};
use crate::mcp::model::{EnvValue, McpCopy, Place, Server, Transport, masked_args, masked_url};
use crate::mcp::plan::{self, ClaudeScope, EditChange, McpPlan, Request};
use crate::mcp::portable;
use crate::mcp::scan::{self, FieldDiff, McpServer, Status, safe_error};
use crate::store::{self, LastChange};

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
    /// Add a server to agents (acts at once).
    #[command(group(ArgGroup::new("transport").required(true).args(["command", "url"])))]
    Add {
        name: String,
        /// The program that starts a local server.
        #[arg(long, value_name = "CMD")]
        command: Option<String>,
        /// An argument for the program (repeat for more).
        #[arg(long = "arg", value_name = "A", allow_hyphen_values = true, requires = "command")]
        args: Vec<String>,
        /// `KEY=VALUE`, `KEY=@file`, `KEY=-` (stdin) or `KEY=${VAR}`.
        #[arg(long, value_name = "K=V", requires = "command")]
        env: Vec<String>,
        /// The address of a remote server.
        #[arg(long, value_name = "URL")]
        url: Option<String>,
        /// `KEY=VALUE`, `KEY=@file`, `KEY=-` (stdin) or `KEY=${VAR}`.
        #[arg(long, value_name = "K=V", requires = "url")]
        header: Vec<String>,
        /// The agents to add it to.
        #[arg(long, value_name = "AGENT", num_args = 1.., required = true)]
        to: Vec<String>,
        /// Where Claude Code keeps a project server: local (this machine) or project (`.mcp.json`).
        #[arg(long, value_name = "SCOPE", default_value = "local", value_parser = ["local", "project"])]
        claude_scope: String,
    },
    /// Copy a server into other agents, here or on another machine.
    Copy {
        name: String,
        #[arg(long, value_name = "AGENT", num_args = 1.., required = true)]
        to: Vec<String>,
        /// Copy this agent's version. Needed when the copies differ; otherwise the shared
        /// definition is copied.
        #[arg(long, value_name = "AGENT")]
        from: Option<String>,
        /// Copy to these agents on this saved host (or ssh target), in its global setup
        /// unless --to-project says otherwise.
        #[arg(long, value_name = "HOST")]
        to_host: Option<String>,
        /// With --to-host: copy into this project, a folder on the host.
        #[arg(long, value_name = "DIR", requires = "to_host")]
        to_project: Option<PathBuf>,
        /// With --to-project: where Claude Code keeps it, local (the host's ~/.claude.json)
        /// or project (`.mcp.json`).
        #[arg(long, value_name = "SCOPE", value_parser = ["local", "project"], requires = "to_project")]
        claude_scope: Option<String>,
        #[command(flatten)]
        apply: Apply,
    },
    /// Take a server another machine sends: its definition as JSON on stdin.
    #[command(hide = true)]
    Receive {
        name: String,
        #[arg(long, value_name = "AGENT", num_args = 1.., required = true)]
        to: Vec<String>,
        #[arg(long, value_name = "SCOPE", default_value = "local", value_parser = ["local", "project"])]
        claude_scope: String,
        /// How warnings name the sending machine.
        #[arg(long, value_name = "LABEL")]
        from: String,
        /// How warnings name this machine: the sender's name for it.
        #[arg(long, value_name = "LABEL")]
        target: String,
        /// The sending machine's home folder: paths into it may not exist here.
        #[arg(long, value_name = "DIR")]
        source_home: PathBuf,
        #[command(flatten)]
        apply: Apply,
    },
    /// Change fields of a server in every agent that has it.
    Edit {
        name: String,
        #[arg(long, value_name = "CMD")]
        command: Option<String>,
        /// Replaces the arguments (repeat for more).
        #[arg(long = "arg", value_name = "A", allow_hyphen_values = true)]
        args: Vec<String>,
        #[arg(long, value_name = "K=V")]
        env: Vec<String>,
        #[arg(long, value_name = "KEY")]
        unset_env: Vec<String>,
        #[arg(long, value_name = "URL")]
        url: Option<String>,
        #[arg(long, value_name = "K=V")]
        header: Vec<String>,
        #[arg(long, value_name = "KEY")]
        unset_header: Vec<String>,
        #[command(flatten)]
        apply: Apply,
    },
    /// Remove a server (default: from every agent).
    Remove {
        name: String,
        #[arg(long, value_name = "AGENT", num_args = 1..)]
        from: Vec<String>,
        #[command(flatten)]
        apply: Apply,
    },
    /// Switch a server off for the project (needs --project).
    Disable {
        name: String,
        #[command(flatten)]
        apply: Apply,
    },
    /// Switch a server back on for the project (needs --project).
    Enable {
        name: String,
        #[command(flatten)]
        apply: Apply,
    },
    /// Fold servers that are one into a single name.
    Merge {
        #[arg(required = true)]
        names: Vec<String>,
        #[arg(long, value_name = "NEW")]
        into: String,
        #[command(flatten)]
        apply: Apply,
    },
}

pub fn run(cmd: Cmd, ctx: &mut Ctx, scope: &Scope, out: &mut Out, err: &mut dyn Write) -> Exit {
    let now = Apply { apply: true, plan_id: None };
    let (request, label, apply) = match cmd {
        Cmd::List => return list(ctx, scope, out),
        Cmd::Show { name, show_secrets } => return show(ctx, scope, &name, show_secrets, out, err),
        Cmd::Add { name, command, args, env, url, header, to, claude_scope } => {
            let built = (|| {
                let mut values = Values::new(ctx);
                let transport = match (command, url) {
                    (Some(command), _) => Transport::Stdio { command, args, env: values.pairs("--env", &env)?, cwd: None },
                    (None, Some(url)) => Transport::Http { url, headers: values.pairs("--header", &header)?, sse: false },
                    (None, None) => return Err("give --command or --url".to_string()),
                };
                let claude_scope = if claude_scope == "project" { ClaudeScope::Project } else { ClaudeScope::Local };
                Ok(Request::Add { server: Server { name: name.clone(), transport }, to: agent_indices(&to)?, claude_scope })
            })();
            match built {
                Ok(r) => (r, format!("add {name}"), now),
                Err(e) => return usage(err, &e),
            }
        }
        Cmd::Copy { name, to, from, to_host: Some(host), to_project, claude_scope, apply } => {
            let dest = to_project.map_or(Scope::Global, Scope::Project);
            let claude_scope = if claude_scope.as_deref() == Some("project") { ClaudeScope::Project } else { ClaudeScope::Local };
            let there = Destination { host, dest, claude_scope };
            return copy_to_host(ctx, scope, &name, &to, from.as_deref(), &there, apply, out, err);
        }
        Cmd::Receive { name, to, claude_scope, from, target, source_home, apply } => {
            let claude_scope = if claude_scope == "project" { ClaudeScope::Project } else { ClaudeScope::Local };
            return match agent_indices(&to) {
                Ok(to) => receive(ctx, scope, Received { name, to, claude_scope, from, target, source_home }, apply, out, err),
                Err(e) => usage(err, &e),
            };
        }
        Cmd::Copy { name, to, from, to_host: None, apply, .. } => {
            let built = agent_indices(&to).and_then(|to| Ok(Request::Copy { name: name.clone(), to, from: from.as_deref().map(agent_index).transpose()? }));
            match built {
                Ok(r) => (r, format!("copy {name}"), apply),
                Err(e) => return usage(err, &e),
            }
        }
        Cmd::Edit { name, command, args, env, unset_env, url, header, unset_header, apply } => {
            let built = (|| {
                let mut values = Values::new(ctx);
                let mut changes = Vec::new();
                changes.extend(command.map(EditChange::Command));
                if !args.is_empty() {
                    changes.push(EditChange::Args(args));
                }
                changes.extend(values.pairs("--env", &env)?.into_iter().map(|(k, v)| EditChange::SetEnv(k, v)));
                changes.extend(unset_env.into_iter().map(EditChange::UnsetEnv));
                changes.extend(url.map(EditChange::Url));
                changes.extend(values.pairs("--header", &header)?.into_iter().map(|(k, v)| EditChange::SetHeader(k, v)));
                changes.extend(unset_header.into_iter().map(EditChange::UnsetHeader));
                if changes.is_empty() {
                    return Err("nothing to change: give --command, --arg, --env, --url, --header, --unset-env or --unset-header".to_string());
                }
                Ok(Request::Edit { name: name.clone(), changes })
            })();
            match built {
                Ok(r) => (r, format!("edit {name}"), apply),
                Err(e) => return usage(err, &e),
            }
        }
        Cmd::Remove { name, from, apply } => match agent_indices(&from) {
            Ok(from) => (Request::Remove { name: name.clone(), from }, format!("remove {name}"), apply),
            Err(e) => return usage(err, &e),
        },
        Cmd::Disable { name, apply } => (Request::SetDisabled { name: name.clone(), disabled: true }, format!("disable {name}"), apply),
        Cmd::Enable { name, apply } => (Request::SetDisabled { name: name.clone(), disabled: false }, format!("enable {name}"), apply),
        Cmd::Merge { names, into, apply } => (Request::Merge { names, into: into.clone() }, format!("merge into {into}"), apply),
    };
    if matches!(request, Request::SetDisabled { .. }) && matches!(scope, Scope::Global) {
        return usage(err, "disabling or enabling a server applies to one project: add --project DIR");
    }
    let immediate = matches!(request, Request::Add { .. });
    let inv = scan::scan(scope, &ctx.paths);
    match plan::plan(&inv, &request, &ctx.paths) {
        Ok(plan) => drive(ctx, plan, label, apply, immediate, out, err),
        Err(e) => failed(out, err, &e),
    }
}

/// A server another machine sends, and where it goes.
struct Received {
    name: String,
    to: Vec<usize>,
    claude_scope: ClaudeScope,
    from: String,
    target: String,
    source_home: PathBuf,
}

/// The receiving end of `mcp copy --to-host`: plans the definition on stdin as `mcp add` would,
/// with heads-ups for what may not exist here. The plan id covers the definition, so a server
/// changed on the sender since the plan is a changed plan.
fn receive(ctx: &mut Ctx, scope: &Scope, r: Received, apply: Apply, out: &mut Out, err: &mut dyn Write) -> Exit {
    let mut reader = ctx.stdin.take().unwrap_or_else(|| Box::new(std::io::stdin()));
    let mut bytes = Vec::new();
    // serde's errors quote the input, and the input holds secrets: say only what failed.
    let server = match reader.read_to_end(&mut bytes).ok().and_then(|_| serde_json::from_slice::<Server>(&bytes).ok()) {
        Some(s) => s,
        None => return failed(out, err, &format!("the definition of {} on stdin can't be read", r.name)),
    };
    if server.name != r.name {
        return failed(out, err, &format!("the definition on stdin is not {}", r.name));
    }
    let inv = scan::scan(scope, &ctx.paths);
    let request = Request::Add { server, to: r.to, claude_scope: r.claude_scope };
    let mut plan = match plan::plan(&inv, &request, &ctx.paths) {
        Ok(p) => p,
        Err(e) => return failed(out, err, &e),
    };
    let Request::Add { server, .. } = &request else { unreachable!("built above") };
    plan.sent = serde_json::to_vec(server).ok().map(|b| content_hash(&b));
    for w in portable::warnings(server, &r.source_home, &r.from, &r.target) {
        if !plan.warnings.contains(&w) {
            plan.warnings.push(w);
        }
    }
    drive(ctx, plan, format!("copy {} from {}", r.name, r.from), apply, false, out, err)
}

/// Where `mcp copy --to-host` puts a server: a scope on the host (a project is a folder there).
struct Destination {
    host: String,
    dest: Scope,
    claude_scope: ClaudeScope,
}

/// `mcp copy --to-host`: the host plans the copy into `there.dest` and, with the plan's id,
/// takes the definition this machine sends. The plan id is the host's.
#[allow(clippy::too_many_arguments)]
fn copy_to_host(ctx: &Ctx, scope: &Scope, name: &str, to: &[String], from: Option<&str>, there: &Destination, a: Apply, out: &mut Out, err: &mut dyn Write) -> Exit {
    if a.plan_id.is_some() && !a.apply {
        return usage(err, "--plan-id only goes with --apply");
    }
    let (agents, from_agent) = match agent_indices(to).and_then(|to| Ok((to, from.map(agent_index).transpose()?))) {
        Ok(found) => found,
        Err(e) => return usage(err, &e),
    };
    let (host, target) = match super::hosts::ready(ctx, &there.host) {
        Ok(found) => found,
        Err(e) => return failed(out, err, &e),
    };
    let source = super::hosts::this(ctx);
    let label = crate::hosts::copy::this_machine();
    let copy = |apply: Option<&str>| {
        copy_server(source.as_ref(), label, name, from_agent, scope, target.as_ref(), &host.name, &there.dest, there.claude_scope, &agents, apply)
    };
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

/// An MCP agent id from the command line.
fn agent_index(id: &str) -> Result<usize, String> {
    index_of(id).ok_or_else(|| format!("unknown agent {id}; valid ids: {}", MCP_AGENTS.iter().map(|a| a.id).collect::<Vec<_>>().join(", ")))
}

fn agent_indices(ids: &[String]) -> Result<Vec<usize>, String> {
    ids.iter().map(|id| agent_index(id)).collect()
}

/// Turns `KEY=VALUE` arguments into env or header values. Errors name the key or the file,
/// never a value: the value may be a secret.
struct Values<'a> {
    ctx: &'a mut Ctx,
    stdin_used: bool,
}

impl<'a> Values<'a> {
    fn new(ctx: &'a mut Ctx) -> Self {
        Values { ctx, stdin_used: false }
    }

    fn pairs(&mut self, flag: &str, args: &[String]) -> Result<BTreeMap<String, EnvValue>, String> {
        let mut map = BTreeMap::new();
        for arg in args {
            // The argument itself is not echoed: with no `=`, it may be the secret.
            let Some((key, v)) = arg.split_once('=').filter(|(k, _)| !k.is_empty()) else {
                return Err(format!("{flag} needs KEY=VALUE"));
            };
            map.insert(key.to_string(), self.value(flag, key, v)?);
        }
        Ok(map)
    }

    fn value(&mut self, flag: &str, key: &str, v: &str) -> Result<EnvValue, String> {
        if v == "-" {
            if std::mem::replace(&mut self.stdin_used, true) {
                return Err(format!("{flag} {key}=-: stdin can be read only once"));
            }
            let mut reader = self.ctx.stdin.take().unwrap_or_else(|| Box::new(std::io::stdin()));
            return read_text(&mut reader).map(|t| EnvValue::literal(&t)).map_err(|_| format!("{flag} {key}=-: could not read stdin as text"));
        }
        if let Some(path) = v.strip_prefix('@') {
            let text = std::fs::read(path).ok().and_then(|b| String::from_utf8(b).ok());
            return text.map(|t| EnvValue::literal(trim_newline(&t))).ok_or_else(|| format!("{flag} {key}: cannot read {}", Path::new(path).display()));
        }
        Ok(EnvValue::parse(v))
    }
}

fn read_text(r: &mut dyn Read) -> std::io::Result<String> {
    let mut text = String::new();
    r.read_to_string(&mut text)?;
    Ok(trim_newline(&text).to_string())
}

/// Files and stdin end in a newline the value doesn't have.
fn trim_newline(t: &str) -> &str {
    let t = t.strip_suffix('\n').unwrap_or(t);
    t.strip_suffix('\r').unwrap_or(t)
}

fn plan_json(plan: &McpPlan, lines: &[String], id: &str, applied: bool) -> Value {
    let skipped: Vec<Value> = plan.skipped.iter().map(|(a, why)| json!({ "agent": MCP_AGENTS[*a].id, "reason": why })).collect();
    json!({ "applied": applied, "plan_id": id, "actions": lines, "skipped": skipped, "warnings": plan.warnings })
}

/// The skipped agents and warnings, as lines under a plan or a result.
fn notes(plan: &McpPlan) -> String {
    let skipped = plan.skipped.iter().map(|(a, why)| format!("\nSkipped {}: {why}", MCP_AGENTS[*a].name));
    let warnings = plan.warnings.iter().map(|w| format!("\nWarning: {w}"));
    skipped.chain(warnings).collect()
}

fn plan_text(plan: &McpPlan, lines: &[String], id: &str) -> String {
    if lines.is_empty() {
        return format!("Nothing to do{}", notes(plan));
    }
    let steps: String = lines.iter().map(|l| format!("\n  - {l}")).collect();
    format!("Plan {id}:{steps}{}\nRun again with --apply --plan-id {id} to do it.", notes(plan))
}

/// Prints the plan, or with `--apply` (or for `add`, at once) does it, unless it changed
/// since `--plan-id`. Same contract and JSON as `skills`.
fn drive(ctx: &Ctx, plan: McpPlan, label: String, a: Apply, immediate: bool, out: &mut Out, err: &mut dyn Write) -> Exit {
    if a.plan_id.is_some() && !a.apply {
        return usage(err, "--plan-id only goes with --apply");
    }
    let (lines, id) = (plan.lines(&ctx.paths), plan.id());
    if !a.apply {
        out.emit(plan_text(&plan, &lines, &id), plan_json(&plan, &lines, &id, false));
        return Exit::Ok;
    }
    if let Some(old) = a.plan_id.as_deref().filter(|old| *old != id) {
        let mut v = plan_json(&plan, &lines, &id, false);
        v["plan_changed"] = json!(true);
        out.emit(format!("The plan changed since {old}; nothing was done. It now is:\n{}", plan_text(&plan, &lines, &id)), v);
        return Exit::PlanChanged;
    }
    if plan.is_empty() {
        out.emit(plan_text(&plan, &lines, &id), plan_json(&plan, &lines, &id, false));
        // An add that every agent already has is done; one an agent couldn't take failed.
        let failed = immediate && plan.skipped.iter().any(|(a, _)| !plan.present.contains(a));
        return if failed { Exit::Failed } else { Exit::Ok };
    }
    // Backups of the last change go only once the plan still matches the files.
    let mut journal = Journal::default();
    let result = plan::verify(&plan).map_err(|e| (0, e)).and_then(|()| {
        store::begin_change(&ctx.paths);
        plan::apply(&plan, &ctx.paths, &mut journal)
    });
    let undoable = !journal.is_empty();
    if undoable {
        store::save_last_change(&ctx.paths, Some(&LastChange { label, journal }));
    }
    match result {
        Ok(done) => {
            let mut v = plan_json(&plan, &lines, &id, true);
            v["done"] = json!(done);
            let steps: String = lines.iter().map(|l| format!("\n  - {l}")).collect();
            out.emit(format!("Done:{steps}{}", notes(&plan)), v);
            Exit::Ok
        }
        Err((done, e)) => {
            if out.json {
                let mut v = plan_json(&plan, &lines, &id, false);
                v["done"] = json!(done);
                v["error"] = json!(e);
                out.emit("", v);
            } else {
                let _ = writeln!(err, "Stopped after {done} of {} files: {e}{}", plan.files.len(), super::undo_hint(undoable));
            }
            Exit::Failed
        }
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
    let diffs: Vec<Value> = server.diffs.iter().map(|d| diff_json(d, secrets)).collect();
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
    if secrets { v.written() } else { v.masked() }
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

fn diff_json(d: &FieldDiff, secrets: bool) -> Value {
    let values: Vec<Value> = d.shown(secrets).map(|(a, v)| json!({ "agent": MCP_AGENTS[a].id, "value": v })).collect();
    json!({ "field": d.field, "values": values, "hidden_only": d.hidden_only() })
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
            for (a, v) in d.shown(secrets) {
                lines.push(format!("    {:<16} {}", MCP_AGENTS[a].id, v.unwrap_or("(none)")));
            }
            if d.hidden_only() && !secrets {
                lines.push("    (only hidden values differ; --show-secrets shows them)".into());
            }
        }
    }
    lines.join("\n")
}
