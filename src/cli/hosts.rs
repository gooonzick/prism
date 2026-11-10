//! `prism hosts`: the remote machines Prism can reach over ssh, and `--host`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use clap::Subcommand;
use serde_json::{Value, json};

use super::{Ctx, Exit, Out, failed, usage};
use crate::hosts::copy::Failed;
use crate::hosts::install::{self, Health, VERSION};
use crate::hosts::runner::Runner;
use crate::hosts::{self, Host};

#[derive(Subcommand)]
pub enum Cmd {
    /// List the saved hosts.
    List,
    /// Save a host under a name.
    Add {
        name: String,
        /// What follows `ssh`: user@host or an alias from ~/.ssh/config.
        #[arg(long, value_name = "TARGET")]
        ssh: String,
    },
    /// Forget a host.
    Remove { name: String },
    /// Reach a host and check that its prism is this version.
    Check { name: String },
    /// Put this version of prism in the host's ~/.local/bin.
    Install {
        name: String,
        /// Upload this file instead of downloading the release.
        #[arg(long, value_name = "PATH")]
        binary: Option<PathBuf>,
        /// Replace a ~/.local/bin/prism there that is another program.
        #[arg(long)]
        force: bool,
    },
}

pub fn run(cmd: Cmd, ctx: &mut Ctx, out: &mut Out, err: &mut dyn Write) -> Exit {
    match cmd {
        Cmd::List => {
            let mut text = format!("{:<32} {}", "NAME", "SSH");
            for h in &ctx.settings.hosts {
                text.push_str(&format!("\n{:<32} {}", h.name, h.ssh));
            }
            out.emit(text, Value::Array(ctx.settings.hosts.iter().map(|h| json!({ "name": h.name, "ssh": h.ssh })).collect()));
            Exit::Ok
        }
        Cmd::Add { name, ssh } => {
            if let Err(e) = hosts::valid_name(&name) {
                return usage(err, &e);
            }
            let ssh = ssh.trim().to_string();
            if !hosts::valid_target(&ssh) {
                return usage(err, "--ssh needs a target like user@host, with no spaces and not starting with '-'");
            }
            if hosts::find(&ctx.settings.hosts, &name).is_some() {
                return usage(err, &format!("{name} is already saved; remove it first"));
            }
            ctx.settings.hosts.push(Host { name: name.clone(), ssh: ssh.clone() });
            ctx.settings.save_to(&ctx.paths);
            out.emit(format!("Added {name} ({ssh})"), json!({ "name": name, "ssh": ssh, "added": true }));
            Exit::Ok
        }
        Cmd::Remove { name } => {
            let before = ctx.settings.hosts.len();
            ctx.settings.hosts.retain(|h| h.name != name);
            let removed = ctx.settings.hosts.len() < before;
            ctx.settings.save_to(&ctx.paths);
            out.emit(if removed { format!("Removed {name}") } else { format!("{name} was not saved") }, json!({ "name": name, "removed": removed }));
            Exit::Ok
        }
        Cmd::Check { name } | Cmd::Install { name, .. } if hosts::reserved(&name) => {
            usage(err, &format!("{name} is this machine; hosts check and install reach another one"))
        }
        Cmd::Check { name } => check(ctx, &name, out, err),
        Cmd::Install { name, binary, force } => install(ctx, &name, binary.as_deref(), force, out, err),
    }
}

/// `--to-host NAME`: another machine, never this one.
pub(super) fn other_machine(name: &str) -> Result<(), String> {
    if hosts::reserved(name) {
        return Err(format!("{name} is this machine; copy here with --to-project or --to-global, without --to-host"));
    }
    Ok(())
}

/// A folder on another machine: a full path, since a relative one would be taken from wherever
/// the shell there starts.
pub(super) fn remote_folder(dir: &Path) -> Result<(), String> {
    if dir.is_absolute() {
        return Ok(());
    }
    Err(format!("{} is a relative path; on another machine give the folder's full path, starting with /", dir.display()))
}

/// A saved host by name, else `name` taken as an ssh target.
fn resolve(ctx: &Ctx, name: &str) -> Host {
    hosts::find(&ctx.settings.hosts, name).cloned().unwrap_or_else(|| Host { name: name.into(), ssh: name.into() })
}

/// Ready exits 0, any other health 3 (as `doctor` does for problems), unreachable 1.
fn check(ctx: &Ctx, name: &str, out: &mut Out, err: &mut dyn Write) -> Exit {
    let host = resolve(ctx, name);
    let runner = (ctx.make_runner)(&host);
    let (health, platform) = match install::check(runner.as_ref()) {
        Ok(found) => found,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            if out.json {
                let unknown = Value::Null;
                out.emit("", json!({ "name": host.name, "reachable": false, "health": unknown, "remote_version": unknown, "os": unknown, "arch": unknown, "error": e }));
            }
            return Exit::Failed;
        }
    };
    let remote_version = match &health {
        Health::Ready => Some(VERSION.to_string()),
        Health::OtherVersion(v, _) => Some(v.clone()),
        Health::Missing(_) | Health::Unsupported(_) => None,
    };
    out.emit(
        format!("prism on {} is {} ({} {})", host.name, health.describe(), platform.os, platform.arch),
        json!({ "name": host.name, "reachable": true, "health": health.key(), "remote_version": remote_version, "os": platform.os, "arch": platform.arch, "why": health.why() }),
    );
    if health == Health::Ready { Exit::Ok } else { Exit::Problems }
}

fn install(ctx: &Ctx, name: &str, binary: Option<&Path>, force: bool, out: &mut Out, err: &mut dyn Write) -> Exit {
    let host = resolve(ctx, name);
    let runner = (ctx.make_runner)(&host);
    let platform = match install::check(runner.as_ref()) {
        Ok((_, platform)) => platform,
        Err(e) => return failed(out, err, &e),
    };
    if let Err(e) = install::install(runner.as_ref(), VERSION, &platform, binary, force) {
        return failed(out, err, &format!("Can't install prism on {}: {e}", host.name));
    }
    // What was put there must run as this version.
    match install::check(runner.as_ref()) {
        Ok((Health::Ready, _)) => {
            out.emit(format!("Installed prism {VERSION} on {}", host.name), json!({ "name": host.name, "installed": true, "version": VERSION }));
            Exit::Ok
        }
        Ok((health, _)) => failed(out, err, &format!("Installed, but prism on {} is {}", host.name, health.describe())),
        Err(e) => failed(out, err, &e),
    }
}

/// A saved host (or ssh target) and its runner, once its `prism` is this version; else why not.
pub(super) fn ready(ctx: &Ctx, name: &str) -> Result<(Host, Box<dyn Runner>), String> {
    let host = resolve(ctx, name);
    let runner = (ctx.make_runner)(&host);
    match install::check(runner.as_ref())? {
        (Health::Ready, _) => Ok((host, runner)),
        (health, _) => Err(format!("prism on {0} is {1}; run prism hosts install {0}", host.name, health.describe())),
    }
}

/// This machine as a runner: the source of a copy to a host.
pub(super) fn this(ctx: &Ctx) -> Box<dyn Runner> {
    (ctx.make_runner)(&Host { name: "this".into(), ssh: "this".into() })
}

/// A copy to `host` that failed: why, and when it did part of it there, how to take that back.
pub(super) fn copy_failed(out: &mut Out, err: &mut dyn Write, host: &str, f: Failed) -> Exit {
    let hint = if f.undoable { format!("\nRun prism --host {host} undo to take back what was done.") } else { String::new() };
    let _ = writeln!(err, "{}{hint}", f.why.trim());
    if out.json {
        out.emit("", json!({ "error": f.why.trim(), "undoable": f.undoable }));
    }
    Exit::Failed
}

/// Prints a host's answer to a copy (its plan or result, lines already naming it) the way
/// local plans print. Exit 4 when the plan changed since `--plan-id`.
pub(super) fn print_remote(out: &mut Out, v: Value) -> Exit {
    let strs = |key: &str| v[key].as_array().map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect::<Vec<_>>()).unwrap_or_default();
    let (id, applied, changed) = (v["plan_id"].as_str().unwrap_or_default(), v["applied"] == true, v["plan_changed"] == true);
    let actions = strs("actions");
    let steps: String = actions.iter().map(|a| format!("\n  - {a}")).collect();
    let mut text = match (applied, actions.is_empty()) {
        (true, _) => format!("Done:{steps}"),
        (false, true) => "Nothing to do".to_string(),
        (false, false) => format!("Plan {id}:{steps}"),
    };
    if let Some(conflicts) = v["conflicts"].as_array().filter(|c| !c.is_empty()) {
        text.push_str("\nConflicts (left alone):");
        for c in conflicts {
            text.push_str(&format!("\n  - {}: {}", c["skill"].as_str().unwrap_or_default(), c["reason"].as_str().unwrap_or_default()));
        }
    }
    for s in v["skipped"].as_array().into_iter().flatten() {
        let agent = s["agent"].as_str().unwrap_or_default();
        let name = crate::mcp::agents::MCP_AGENTS.iter().find(|a| a.id == agent).map_or(agent, |a| a.name);
        text.push_str(&format!("\nSkipped {name}: {}", s["reason"].as_str().unwrap_or_default()));
    }
    for w in strs("warnings") {
        text.push_str(&format!("\nWarning: {w}"));
    }
    if !applied && !actions.is_empty() {
        text.push_str(&format!("\nRun again with --apply --plan-id {id} to do it."));
    }
    if changed {
        text = format!("The plan changed; nothing was done. It now is:\n{text}");
    }
    out.emit(text, v);
    if changed { Exit::PlanChanged } else { Exit::Ok }
}

/// `--host NAME`: the same command line, minus `--host`, run by the host's `prism` once it is
/// this version. Its output and exit code are the remote's.
pub fn forward(name: &str, args: &[&str], ctx: &mut Ctx, out: &mut Out, err: &mut dyn Write) -> i32 {
    let runner = match ready(ctx, name) {
        Ok((_, runner)) => runner,
        Err(e) => return failed(out, err, &e) as i32,
    };
    let rest = without_host(args);
    // Secrets (`K=-`) travel over stdin; nothing else needs it, and reading it would block.
    // What follows `--` belongs to a command line the remote stores, not to us.
    let stdin = if rest.iter().take_while(|a| *a != "--").any(|a| a == "-" || a.ends_with("=-")) {
        let mut reader = ctx.stdin.take().unwrap_or_else(|| Box::new(std::io::stdin()));
        let mut data = Vec::new();
        if let Err(e) = reader.read_to_end(&mut data) {
            return failed(out, err, &format!("could not read stdin: {e}")) as i32;
        }
        Some(data)
    } else {
        None
    };
    match runner.prism(&rest, stdin) {
        Ok(o) => {
            let _ = out.out.write_all(&o.stdout);
            let _ = err.write_all(o.stderr.as_bytes());
            o.code
        }
        Err(e) => failed(out, err, &e) as i32,
    }
}

/// `args` without `--host NAME` / `--host=NAME`; what follows `--` is left alone.
fn without_host(args: &[&str]) -> Vec<String> {
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(&a) = it.next() {
        match a {
            "--" => {
                rest.push(a.to_string());
                rest.extend(it.map(|a| a.to_string()));
                break;
            }
            "--host" => {
                it.next();
            }
            _ if a.starts_with("--host=") => {}
            _ => rest.push(a.to_string()),
        }
    }
    rest
}

#[cfg(test)]
mod tests {
    use super::without_host;

    #[test]
    fn host_flag_is_dropped_wherever_it_is() {
        assert_eq!(without_host(&["--host", "x", "doctor", "--json"]), ["doctor", "--json"]);
        assert_eq!(without_host(&["doctor", "--host=x", "--json"]), ["doctor", "--json"]);
        assert_eq!(without_host(&["mcp", "add", "a", "--", "cmd", "--host", "y"]), ["mcp", "add", "a", "--", "cmd", "--host", "y"]);
    }
}
