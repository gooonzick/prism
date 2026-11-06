//! Skills Explorer: browse the agent skills on this machine, install more from skills.sh, and
//! keep `.agents/skills` linked into `.claude`, `.codex`, `.opencode` and the other agents,
//! globally or per project.
//!
//! `cargo run -- [project folder]` opens the window; `-- --snap out.png` renders it headless.

mod agents;
mod app;
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
    App::new(Paths::from_env(), Settings::load())
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
    ("remove", |s| s.confirm_remove = s.inv.skills.first().map(|x| x.name.clone())),
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
];

fn main() {
    rune::prewarm_gpu();
    let spec = AppSpec {
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(ui::on_key),
        min_size: Some((820.0, 520.0)),
        scenes: SCENES,
        ..AppSpec::new("Skills Explorer", (1180.0, 760.0), init, ui::view)
    };
    rune::App::new(spec).chrome(ui::chrome()).run();
}
