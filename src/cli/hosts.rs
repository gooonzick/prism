//! `prism hosts`: the remote machines Prism can reach over ssh, and `--host`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use clap::Subcommand;
use serde_json::{Value, json};

use super::{Ctx, Exit, Out, failed, usage};
use crate::hosts::install::{self, Health, VERSION};
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
            if ssh.is_empty() || ssh.starts_with('-') || ssh.chars().any(char::is_whitespace) {
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
        Cmd::Check { name } => check(ctx, &name, out, err),
        Cmd::Install { name, binary } => install(ctx, &name, binary.as_deref(), out, err),
    }
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
        Health::OtherVersion(v) => Some(v.clone()),
        Health::Missing | Health::Unsupported(_) => None,
    };
    out.emit(
        format!("prism on {} is {} ({} {})", host.name, health.describe(), platform.os, platform.arch),
        json!({ "name": host.name, "reachable": true, "health": health.key(), "remote_version": remote_version, "os": platform.os, "arch": platform.arch }),
    );
    if health == Health::Ready { Exit::Ok } else { Exit::Problems }
}

fn install(ctx: &Ctx, name: &str, binary: Option<&Path>, out: &mut Out, err: &mut dyn Write) -> Exit {
    let host = resolve(ctx, name);
    let runner = (ctx.make_runner)(&host);
    let platform = match install::check(runner.as_ref()) {
        Ok((_, platform)) => platform,
        Err(e) => return failed(out, err, &e),
    };
    if let Err(e) = install::install(runner.as_ref(), VERSION, &platform, binary) {
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

/// `--host NAME`: the same command line, minus `--host`, run by the host's `prism` once it is
/// this version. Its output and exit code are the remote's.
pub fn forward(name: &str, args: &[&str], ctx: &mut Ctx, out: &mut Out, err: &mut dyn Write) -> i32 {
    let host = resolve(ctx, name);
    let runner = (ctx.make_runner)(&host);
    match install::check(runner.as_ref()) {
        Ok((Health::Ready, _)) => {}
        Ok((health, _)) => {
            let msg = format!("prism on {0} is {1}; run prism hosts install {0}", host.name, health.describe());
            return failed(out, err, &msg) as i32;
        }
        Err(e) => return failed(out, err, &e) as i32,
    }
    let rest = without_host(args);
    // Secrets (`K=-`) travel over stdin; nothing else needs it, and reading it would block.
    let stdin = if rest.iter().any(|a| a == "-" || a.ends_with("=-")) {
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
