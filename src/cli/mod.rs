//! The `prism` command line. It shares the binary with the window: `main` hands known
//! subcommands to [`run`] before Rune starts, everything else still opens the window.

use std::ffi::OsString;
use std::fmt::Display;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};

use crate::agents::{self, Paths, Scope};
use crate::store::{self, Settings};

mod doctor;
mod mcp;
mod projects;
pub mod setup;
mod skills;
#[cfg(test)]
mod tests;

/// Process exit codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// Where `K=-` values come from; `None` is the process's stdin. Tests feed their own.
    pub stdin: Option<Box<dyn Read>>,
    /// `$PATH`, for `setup` to find where `prism` goes. Tests set their own.
    pub path_var: Option<OsString>,
}

impl Ctx {
    fn from_env() -> Ctx {
        let paths = Paths::from_env();
        let settings = Settings::load_from(&paths);
        Ctx { paths, settings, stdin: None, path_var: std::env::var_os("PATH") }
    }

    /// The scope `--project` names (any folder, registered or not), else the global one.
    fn scope(&self, project: Option<&Path>) -> Result<Scope, String> {
        let Some(dir) = project else { return Ok(Scope::Global) };
        match std::fs::canonicalize(dir) {
            Ok(p) if p.is_dir() => Ok(Scope::Project(p)),
            _ => Err(format!("{} is not a folder", dir.display())),
        }
    }

    /// Agent indices a sync links into, as the window computes them.
    fn targets(&self) -> Vec<usize> {
        agents::target_indices(self.settings.targets.as_deref(), &self.paths)
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

/// Whether `args` are a command line: help or the version, or a subcommand, maybe after the
/// global flags (`--json`, `--project DIR`). Everything else is the window's.
fn is_cli(args: &[&str]) -> bool {
    if matches!(args.first(), Some(&("-h" | "--help" | "-V" | "--version" | "help"))) {
        return true;
    }
    let mut rest = args;
    loop {
        rest = match rest {
            ["--json", tail @ ..] => tail,
            ["--project", _, tail @ ..] => tail,
            [flag, tail @ ..] if flag.starts_with("--project=") => tail,
            _ => break,
        };
    }
    rest.first().is_some_and(|word| SUBCOMMANDS.contains(word))
}

#[derive(Parser)]
#[command(name = "prism", version, about = "Manage agent skills and MCP servers")]
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
    /// List and show skills.
    Skills {
        #[command(subcommand)]
        command: skills::Cmd,
    },
    /// List, show and change MCP servers in every agent's config.
    Mcp {
        #[command(subcommand)]
        command: mcp::Cmd,
    },
    /// List, add, remove and find projects.
    Projects {
        #[command(subcommand)]
        command: projects::Cmd,
    },
    /// Choose which agents a sync links into.
    Targets {
        #[command(subcommand)]
        command: projects::TargetsCmd,
    },
    /// Take back the last change, from the window or the command line.
    Undo,
    /// Check a scope for broken links, invalid skills and drifting MCP servers.
    Doctor {
        /// Check the global setup and every project Prism lists.
        #[arg(long)]
        all: bool,
    },
    /// Put `prism` on PATH and install the prism skill for agents.
    Setup,
}

/// `None` when `args` aren't a command line (see [`is_cli`]), so the caller opens the window.
pub fn run(args: Vec<OsString>) -> Option<i32> {
    let argv: Vec<&str> = args.iter().map(|a| a.to_str().unwrap_or("")).collect();
    if !is_cli(&argv) {
        return None;
    }
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    // Help, the version and usage errors need no settings: parse before reading any.
    let cli = match parse(&argv, &mut out, &mut err) {
        Ok(cli) => cli,
        Err(code) => return Some(code),
    };
    let mut ctx = Ctx::from_env();
    Some(execute(cli, &mut ctx, &mut out, &mut err))
}

/// Runs one command line (without the program name) on `ctx` and returns its exit code; tests
/// give their own home and streams.
#[cfg(test)]
pub fn run_with(args: &[&str], ctx: &mut Ctx, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match parse(args, out, err) {
        Ok(cli) => execute(cli, ctx, out, err),
        Err(code) => code,
    }
}

/// The parsed command line, or the exit code once clap has printed help, the version or why
/// the line is wrong.
fn parse(args: &[&str], out: &mut dyn Write, err: &mut dyn Write) -> Result<Cli, i32> {
    Cli::try_parse_from(std::iter::once("prism").chain(args.iter().copied())).map_err(|e| {
        // --help and --version are not mistakes.
        let code = if e.use_stderr() { Exit::Usage } else { Exit::Ok };
        let _ = if e.use_stderr() { write!(err, "{e}") } else { write!(out, "{e}") };
        code as i32
    })
}

fn execute(cli: Cli, ctx: &mut Ctx, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut out = Out { json: cli.global.json, out };
    let exit = match cli.command {
        Command::Agents => {
            agents_cmd(ctx, &mut out);
            Exit::Ok
        }
        Command::Undo => undo_cmd(ctx, &mut out, err),
        Command::Doctor { all } => match ctx.scope(cli.global.project.as_deref()) {
            Ok(scope) => doctor::run(all, &scope, ctx, &mut out),
            Err(e) => usage(err, &e),
        },
        Command::Skills { command } => match ctx.scope(cli.global.project.as_deref()) {
            Ok(scope) => skills::run(command, ctx, &scope, &mut out, err),
            Err(e) => usage(err, &e),
        },
        Command::Mcp { command } => match ctx.scope(cli.global.project.as_deref()) {
            Ok(scope) => mcp::run(command, ctx, &scope, &mut out, err),
            Err(e) => usage(err, &e),
        },
        Command::Projects { command } => projects::run(command, ctx, &mut out, err),
        Command::Targets { command } => projects::run_targets(command, ctx, &mut out, err),
        Command::Setup if cli.global.project.is_some() => usage(err, "setup works on the global setup; drop --project"),
        Command::Setup => setup::run(ctx, &mut out),
    };
    exit as i32
}

fn usage(err: &mut dyn Write, msg: &str) -> Exit {
    let _ = writeln!(err, "{msg}");
    Exit::Usage
}

/// What follows a partial failure's message: how to take back what was done.
fn undo_hint(undoable: bool) -> &'static str {
    if undoable { "\nRun prism undo to take back what was done." } else { "" }
}

/// A refusal or failure: the message on stderr, and in JSON mode as `{"error"}` on stdout too.
fn failed(out: &mut Out, err: &mut dyn Write, msg: &str) -> Exit {
    let msg = msg.trim();
    let _ = writeln!(err, "{msg}");
    if out.json {
        out.emit("", serde_json::json!({ "error": msg }));
    }
    Exit::Failed
}

fn undo_cmd(ctx: &Ctx, out: &mut Out, err: &mut dyn Write) -> Exit {
    let Some(change) = store::load_last_change(&ctx.paths) else {
        out.emit("Nothing to undo", serde_json::json!({ "undone": null }));
        return Exit::Ok;
    };
    match change.journal.undo(&ctx.paths) {
        Ok(_) => {
            store::save_last_change(&ctx.paths, None);
            store::drop_backups(&ctx.paths);
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
