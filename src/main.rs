//! Prism: browse the agent skills on this machine, install more from skills.sh, and
//! keep `.agents/skills` linked into `.claude`, `.codex`, `.opencode` and the other agents,
//! globally or per project.
//!
//! `cargo run -- [project folder]` opens the window; `-- --snap out.png` renders it headless.

// Much of the shared code (scan, sync, store) is only called from the window.
#![cfg_attr(not(feature = "gui"), allow(dead_code))]

mod agents;
#[cfg(feature = "gui")]
mod app;
mod cli;
mod fsops;
mod hosts;
mod mcp;
mod plan_view;
mod planid;
mod projects;
mod remote;
mod scan;
mod skill;
mod store;
mod sync;
#[cfg(feature = "gui")]
mod ui;
#[cfg(all(test, feature = "gui"))]
mod ui_tests;
mod wire;

#[cfg(feature = "gui")]
use rune::{AppSpec, Tx, Waker};

#[cfg(feature = "gui")]
use crate::agents::Paths;
#[cfg(feature = "gui")]
use crate::app::App;
#[cfg(feature = "gui")]
use crate::store::Settings;

#[cfg(feature = "gui")]
fn init() -> App {
    let paths = Paths::from_env();
    let settings = Settings::load_from(&paths);
    App::new(paths, settings)
}

#[cfg(feature = "gui")]
fn setup(s: &mut App, waker: Waker) {
    s.set_waker(waker);
}

/// The app menu's "Install Command Line Tool…".
#[cfg(all(feature = "gui", target_os = "macos"))]
fn install_cli_tool(s: &mut App, _: &mut Tx) {
    s.install_cli_tool();
}

/// How long a notice stays up.
#[cfg(feature = "gui")]
const NOTICE_SECS: f64 = 5.0;

#[cfg(feature = "gui")]
fn poll(s: &mut App, clock: f64, tx: &mut Tx) -> bool {
    let mut changed = s.poll();
    match (&s.notice, s.notice_at) {
        (Some(_), None) => {
            s.notice_at = Some(clock);
            tx.wake_after(std::time::Duration::from_secs_f64(NOTICE_SECS));
        }
        (Some(_), Some(at)) if clock - at >= NOTICE_SECS => {
            s.notice = None;
            s.notice_at = None;
            changed = true;
        }
        (None, Some(_)) => s.notice_at = None,
        _ => {}
    }
    changed
}

/// Headless scenes (`--scene NAME`) for checking the layout without clicking.
#[cfg(feature = "gui")]
type Scene = (&'static str, fn(&mut App));

#[cfg(feature = "gui")]
static SCENES: &[Scene] = &[
    ("detail", |s| {
        if let Some(name) = s.inv.skills.first().map(|x| x.name.clone()) {
            s.select(&name);
        }
    }),
    ("sync", |s| s.open_sync()),
    ("remove", |s| s.confirm_remove = s.inv.skills.first().map(|x| vec![x.name.clone()])),
    ("bulk", |s| {
        let names: Vec<String> = s.inv.skills.iter().take(3).map(|x| x.name.clone()).collect();
        names.iter().for_each(|n| s.toggle_checked(n));
    }),
    ("bulk-remove", |s| {
        let names: Vec<String> = s.inv.skills.iter().take(3).map(|x| x.name.clone()).collect();
        names.iter().for_each(|n| s.toggle_checked(n));
        s.confirm_remove_checked();
    }),
    ("browse", |s| {
        s.open_browse();
        s.query = rune::TextEdit::new("react");
        let skill = |source: &str, name: &str, installs| remote::RemoteSkill {
            id: format!("{source}/{name}"),
            source: source.into(),
            skill_id: name.into(),
            name: name.into(),
            installs,
        };
        s.results = Ok(vec![
            skill("vercel-labs/agent-skills", "vercel-react-best-practices", 876_158),
            skill("vercel-labs/skills", "find-skills", 1_204_331),
            skill("anthropics/skills", "frontend-design", 402_118),
        ]);
        s.remote = s.results.as_ref().ok().and_then(|r| r.first().cloned());
        s.remote_md = Some(Ok("---\nname: vercel-react-best-practices\n---\n# React best practices\n\nUse **server components** by default.\n\n- Fetch on the server\n- Keep client bundles small".into()));
        s.inspector.toggle();
    }),
    ("notice", |s| s.notify("Synced: 3 changes".into(), String::new(), false)),
    // Add project… on a folder of projects: the ones with skills inside it.
    ("found", |s| {
        let dev = s.paths.home.join("Development");
        let projects = vec![(dev.join("acme-web"), 2, true), (dev.join("api-gateway"), 1, true), (dev.join("design-system"), 4, false)];
        s.found = Some(app::Found { root: dev, projects });
    }),
    ("updates", |s| {
        let names: Vec<String> = s.inv.skills.iter().take(2).map(|x| x.name.clone()).collect();
        s.updates.insert(s.scope.clone(), names.clone());
        names.iter().for_each(|n| s.toggle_checked(n));
        s.select(&names[0]);
    }),
    ("new-skill", |s| s.open_new_skill()),
    ("mcp", |s| {
        s.open_mcp();
        // A server whose copies differ shows the most: its diff under the copies.
        let pick = s.mcp.servers.iter().find(|x| !x.diffs.is_empty()).or(s.mcp.servers.first()).map(|x| x.name.clone());
        if let Some(name) = pick {
            s.select_mcp(&name);
        }
    }),
    ("mcp-add", |s| {
        s.open_mcp();
        s.open_mcp_add();
        s.add_mcp_pair(false);
        if let Some(f) = &mut s.mcp_form {
            f.name = rune::TextEdit::new("github");
            f.command = rune::TextEdit::new("npx -y @modelcontextprotocol/server-github");
            f.env[0].key = rune::TextEdit::new("GITHUB_TOKEN");
            f.env[0].value = rune::TextEdit::new("${GITHUB_TOKEN}").secure();
        }
    }),
    ("mcp-copy", |s| {
        s.open_mcp();
        // Copies that differ show the most: the source picker, then the overwrites it plans.
        let editable = |x: &&mcp::scan::McpServer| x.status != mcp::scan::Status::ReadOnly;
        let differs = s.mcp.servers.iter().filter(editable).find(|x| x.status == mcp::scan::Status::Differs).map(|x| (x.name.clone(), x.copies[0].agent));
        let pick = differs.clone().map(|(n, _)| n).or_else(|| s.mcp.servers.iter().find(editable).map(|x| x.name.clone()));
        if let Some(name) = pick {
            s.select_mcp(&name);
            s.open_mcp_copy(&name);
        }
        if let Some((_, from)) = differs {
            s.set_mcp_copy_from(from);
        }
    }),
    ("copy", |s| {
        let names: Vec<String> = s.inv.skills.iter().take(2).map(|x| x.name.clone()).collect();
        s.open_copy(names);
    }),
    ("copy-to-host", |s| {
        let names: Vec<String> = s.inv.skills.iter().take(2).map(|x| x.name.clone()).collect();
        let state = wire::state(&agents::Scope::Global, &s.paths, &s.settings);
        let laptop = hosts::Host { name: "laptop".into(), ssh: "me@laptop.lan".into() };
        let why = "Can't reach laptop: ssh: connect to host laptop.lan port 22: Connection refused".to_string();
        s.show_copy_to_host(names, vec![(devbox(), Ok(Box::new(state))), (laptop, Err(why))]);
    }),
    ("repo", |s| {
        s.open_browse();
        s.source = rune::TextEdit::new("vercel-labs/agent-skills");
        s.repo = Some(app::RepoListing {
            source: "vercel-labs/agent-skills".into(),
            skills: Some(Ok(vec![
                ("vercel-react-best-practices".into(), "React and Next.js performance guidelines.".into(), true),
                ("deploy-to-vercel".into(), "Deploy applications and websites to Vercel.".into(), false),
                ("web-design-guidelines".into(), "Review UI code for interface guidelines.".into(), true),
            ])),
        });
        s.inspector.toggle();
    }),
    // A host in view: its state as this machine's own would read, so no host is reached.
    ("env-remote", |s| {
        let state = wire::state(&s.scope, &s.paths, &s.settings);
        s.show_remote(devbox(), Ok(Box::new(state)));
        if let Some(name) = s.inv.skills.first().map(|x| x.name.clone()) {
            s.select(&name);
        }
    }),
    ("env-unreachable", |s| {
        let why = "Can't reach devbox: ssh: connect to host devbox.lan port 22: Connection refused".to_string();
        s.show_remote(devbox(), Err(app::RemoteState::Unreachable(why)));
    }),
];

/// The host the environment scenes show.
#[cfg(feature = "gui")]
fn devbox() -> hosts::Host {
    hosts::Host { name: "devbox".into(), ssh: "me@devbox.lan".into() }
}

fn main() {
    if let Some(code) = cli::run(std::env::args_os().skip(1).collect()) {
        std::process::exit(code);
    }
    #[cfg(feature = "gui")]
    window();
    #[cfg(not(feature = "gui"))]
    {
        cli::run(vec!["--help".into()]);
        std::process::exit(2);
    }
}

#[cfg(feature = "gui")]
fn window() {
    rune::prewarm_gpu();
    let spec = AppSpec {
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(ui::on_key),
        min_size: Some((820.0, 520.0)),
        scenes: SCENES,
        // Commands, paths and keys read as code: the system monospace (SF Mono on macOS).
        families: ("Inter", "ui-monospace"),
        ..AppSpec::new("Prism", (1180.0, 760.0), init, ui::view)
    };
    let app = rune::App::new(spec).chrome(ui::chrome());
    // Native menus are macOS only; on Linux the tarball's installer puts prism on PATH.
    #[cfg(target_os = "macos")]
    let app = app.menu_item(rune::MenuItem::new("Install Command Line Tool…", install_cli_tool));
    app.run();
}
