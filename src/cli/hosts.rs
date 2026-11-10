//! `prism hosts`: the remote machines Prism can reach over ssh.

use std::io::Write;

use clap::Subcommand;
use serde_json::{Value, json};

use super::{Ctx, Exit, Out, usage};
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
    }
}
