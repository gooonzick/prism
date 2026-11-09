//! The `prism` command line. It shares the binary with the window: `main` hands known
//! subcommands to [`run`] before Rune starts, everything else still opens the window.

use std::ffi::OsString;
use std::fmt::Display;
use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::agents::{self, Paths};
use crate::store::{self, Settings};

#[cfg(test)]
mod tests;

/// Process exit codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // later commands use the rest
pub enum Exit {
    Ok = 0,
    Failed = 1,
    Usage = 2,
    Problems = 3,
    PlanChanged = 4,
}

/// What every command works on.
pub struct Ctx {
    pub paths: Paths,
    pub settings: Settings,
}

impl Ctx {
    fn from_env() -> Ctx {
        let paths = Paths::from_env();
        let settings = Settings::load_from(&paths);
        Ctx { paths, settings }
    }
}

/// Where output goes, as text or as JSON.
pub struct Out<'a> {
    json: bool,
    out: &'a mut dyn Write,
}

impl Out<'_> {
    /// Prints `human`, or `json` when `--json` is set.
    fn emit(&mut self, human: impl Display, json: serde_json::Value) {
        let _ = if self.json {
            writeln!(self.out, "{}", serde_json::to_string_pretty(&json).unwrap_or_default())
        } else {
            writeln!(self.out, "{human}")
        };
    }
}

/// Words that route to the CLI. Anything else (a folder, Rune's `--snap`) opens the window.
const SUBCOMMANDS: &[&str] = &["agents", "targets", "skills", "mcp", "projects", "undo", "doctor", "setup"];

#[derive(Parser)]
#[command(name = "prism", about = "Manage agent skills and MCP servers")]
struct Cli {
    #[command(flatten)]
    global: Global,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct Global {
    /// Work on this project instead of the global setup.
    #[arg(long, global = true, value_name = "DIR")]
    project: Option<PathBuf>,
    /// Print JSON instead of text.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Command {
    /// List the supported agents and which are installed.
    Agents,
    /// Take back the last change, from the window or the command line.
    Undo,
}

/// `None` when `args[0]` isn't a subcommand, so the caller opens the window.
pub fn run(args: Vec<OsString>) -> Option<i32> {
    let first = args.first()?.to_str()?;
    if !SUBCOMMANDS.contains(&first) {
        return None;
    }
    let argv: Vec<&str> = args.iter().map(|a| a.to_str().unwrap_or("")).collect();
    let mut ctx = Ctx::from_env();
    Some(run_with(&argv, &mut ctx, &mut std::io::stdout(), &mut std::io::stderr()))
}

/// Runs one command line (without the program name) and returns its exit code.
pub fn run_with(args: &[&str], ctx: &mut Ctx, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let cli = match Cli::try_parse_from(std::iter::once("prism").chain(args.iter().copied())) {
        Ok(cli) => cli,
        Err(e) => {
            // --help and --version are not mistakes.
            let code = if e.use_stderr() { Exit::Usage } else { Exit::Ok };
            let _ = if e.use_stderr() { write!(err, "{e}") } else { write!(out, "{e}") };
            return code as i32;
        }
    };
    let mut out = Out { json: cli.global.json, out };
    let exit = match cli.command {
        Command::Agents => {
            agents_cmd(ctx, &mut out);
            Exit::Ok
        }
        Command::Undo => undo_cmd(ctx, &mut out, err),
    };
    exit as i32
}

fn undo_cmd(ctx: &Ctx, out: &mut Out, err: &mut dyn Write) -> Exit {
    let Some(change) = store::load_last_change(&ctx.paths) else {
        out.emit("Nothing to undo", serde_json::json!({ "undone": null }));
        return Exit::Ok;
    };
    match change.journal.undo(&ctx.paths) {
        Ok(_) => {
            store::save_last_change(&ctx.paths, None);
            out.emit(format!("Undid: {}", change.label), serde_json::json!({ "undone": change.label }));
            Exit::Ok
        }
        Err((e, left)) => {
            // What is still undone stays the last change, to try again after fixing the file.
            store::save_last_change(&ctx.paths, Some(&store::LastChange { label: change.label, journal: left }));
            let _ = writeln!(err, "{e}");
            Exit::Failed
        }
    }
}

fn agents_cmd(ctx: &Ctx, out: &mut Out) {
    let targets = ctx.settings.targets.clone().unwrap_or_else(|| agents::default_targets(&ctx.paths));
    let rows: Vec<_> = agents::AGENTS
        .iter()
        .map(|a| (a.id, a.name, a.detected(&ctx.paths), targets.iter().any(|t| t == a.id)))
        .collect();
    let mut table = format!("{:<10} {:<16} {:<9} {}", "ID", "NAME", "DETECTED", "TARGET");
    for (id, name, detected, target) in &rows {
        let yes = |b: &bool| if *b { "yes" } else { "no" };
        table.push_str(&format!("\n{id:<10} {name:<16} {:<9} {}", yes(detected), yes(target)));
    }
    let json = rows
        .iter()
        .map(|(id, name, detected, target)| serde_json::json!({ "id": id, "name": name, "detected": detected, "target": target }))
        .collect();
    out.emit(table, serde_json::Value::Array(json));
}
