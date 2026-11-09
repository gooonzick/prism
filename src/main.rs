//! Prism: browse the agent skills on this machine, install more from skills.sh, and
//! keep `.agents/skills` linked into `.claude`, `.codex`, `.opencode` and the other agents,
//! globally or per project.
//!
//! `cargo run -- [project folder]` opens the window; `-- --snap out.png` renders it headless.

mod agents;
mod app;
mod cli;
mod fsops;
mod remote;
mod scan;
mod skill;
mod store;
mod sync;
mod ui;
#[cfg(test)]
mod ui_tests;

use rune::{AppSpec, Tx, Waker};

use crate::agents::Paths;
use crate::app::App;
use crate::store::Settings;

fn init() -> App {
    let paths = Paths::from_env();
    let settings = Settings::load_from(&paths);
    App::new(paths, settings)
}

fn setup(s: &mut App, waker: Waker) {
    s.set_waker(waker);
}

/// How long a notice stays up.
const NOTICE_SECS: f64 = 5.0;

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
type Scene = (&'static str, fn(&mut App));

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
    ("updates", |s| {
        let names: Vec<String> = s.inv.skills.iter().take(2).map(|x| x.name.clone()).collect();
        s.updates.insert(s.scope.clone(), names.clone());
        names.iter().for_each(|n| s.toggle_checked(n));
        s.select(&names[0]);
    }),
    ("new-skill", |s| s.open_new_skill()),
    ("copy", |s| {
        let names: Vec<String> = s.inv.skills.iter().take(2).map(|x| x.name.clone()).collect();
        s.open_copy(names);
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
];

fn main() {
    if let Some(code) = cli::run(std::env::args_os().skip(1).collect()) {
        std::process::exit(code);
    }
    rune::prewarm_gpu();
    let spec = AppSpec {
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(ui::on_key),
        min_size: Some((820.0, 520.0)),
        scenes: SCENES,
        ..AppSpec::new("Prism", (1180.0, 760.0), init, ui::view)
    };
    rune::App::new(spec).chrome(ui::chrome()).run();
}
