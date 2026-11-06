//! The window driven headlessly: clicks on real elements against a throwaway home folder.

use std::fs;
use std::path::{Path, PathBuf};

use rune::{Env, Runtime};

use crate::agents::{Paths, Scope, index_of};
use crate::app::App;
use crate::scan::Placement;
use crate::store::Settings;

fn skill(dir: &Path, name: &str) {
    fs::create_dir_all(dir.join(name)).unwrap();
    fs::write(dir.join(name).join("SKILL.md"), format!("---\nname: {name}\ndescription: The {name} skill\n---\n# {name}\n")).unwrap();
}

struct Harness {
    _home: tempfile::TempDir,
    root: PathBuf,
    rt: Runtime<App>,
}

impl Harness {
    /// A window on a project with one skill that only Claude has.
    fn project() -> Harness {
        let home = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        let project = root.join("work/app");
        skill(&project.join(".claude/skills"), "review");
        let settings = Settings { targets: Some(vec!["claude".into(), "codex".into(), "opencode".into()]), ..Settings::default() };
        let mut app = App::new(Paths::isolated(&root), settings);
        app.add_project(project);
        let env = Env { system_dark: false, scale: 2.0 };
        let mut rt = Runtime::new(app, crate::ui::view, (1180.0, 760.0), env);
        rt.frame(0.0);
        Harness { _home: home, root, rt }
    }

    fn click(&mut self, key: &str) {
        let (x, y) = self.rt.find(key).unwrap_or_else(|| panic!("no element keyed {key}"));
        self.rt.pointer_down(x, y);
        self.rt.pointer_up(x, y);
        self.settle();
    }

    fn settle(&mut self) {
        for _ in 0..40 {
            self.rt.frame(1.0 / 60.0);
        }
    }

    fn placement(&self, name: &str, agent: &str) -> Placement {
        self.rt.state.inv.get(name).unwrap().placements[index_of(agent).unwrap()].clone()
    }

    fn project_dir(&self) -> PathBuf {
        self.root.join("work/app")
    }
}

#[test]
fn sync_moves_a_lone_copy_into_agents_and_links_every_target() {
    let mut h = Harness::project();
    assert_eq!(h.rt.state.scope, Scope::Project(h.project_dir()));
    assert_eq!(h.placement("review", "claude"), Placement::Dir);

    h.click("sync");
    let plan = h.rt.state.sync_plan.clone().expect("the sync dialog is open");
    assert_eq!(plan.actions.len(), 3, "adopt, then link codex and opencode: {plan:?}");
    h.click("sync-apply");

    assert!(h.rt.state.sync_plan.is_none());
    assert_eq!(h.placement("review", "agents"), Placement::Dir);
    for agent in ["claude", "codex", "opencode"] {
        assert_eq!(h.placement("review", agent), Placement::Linked, "{agent}");
    }
    let p = h.project_dir();
    assert!(p.join(".agents/skills/review/SKILL.md").is_file());
    assert_eq!(fs::read_link(p.join(".opencode/skills/review")).unwrap(), PathBuf::from("../../.agents/skills/review"));
    assert!(!h.root.join(".agents").exists(), "nothing global was touched");
}

#[test]
fn a_switch_links_and_unlinks_one_agent() {
    let mut h = Harness::project();
    h.click("skill-review");
    assert_eq!(h.rt.state.selected.as_deref(), Some("review"));
    assert!(h.rt.state.preview.as_deref().is_some_and(|md| md.contains("# review")));

    h.click("link-codex");
    assert_eq!(h.placement("review", "agents"), Placement::Dir, "linking first moved it into .agents");
    assert_eq!(h.placement("review", "claude"), Placement::Linked);
    assert_eq!(h.placement("review", "codex"), Placement::Linked);

    h.click("link-codex");
    assert_eq!(h.placement("review", "codex"), Placement::Missing);
    assert_eq!(h.placement("review", "claude"), Placement::Linked, "other agents keep theirs");
}

#[test]
fn remove_asks_first_then_clears_every_agent() {
    let mut h = Harness::project();
    h.click("skill-review");
    h.click("remove");
    assert_eq!(h.rt.state.confirm_remove, Some(vec!["review".to_string()]));
    h.click("remove-cancel");
    assert!(h.rt.state.inv.get("review").is_some());

    h.click("remove");
    h.click("remove-confirm");
    assert!(h.rt.state.inv.get("review").is_none());
    assert!(!h.project_dir().join(".claude/skills/review").exists());
}

#[test]
fn the_sidebar_switches_scopes_and_pages() {
    let mut h = Harness::project();
    skill(&h.root.join(".agents/skills"), "global-one");
    h.click("scope-global");
    assert_eq!(h.rt.state.scope, Scope::Global);
    assert!(h.rt.state.inv.get("global-one").is_some());
    assert!(h.rt.state.inv.get("review").is_none());

    h.click("browse");
    assert_eq!(h.rt.state.page, crate::app::Page::Browse);
    assert!(h.rt.find("query").is_some());

    h.click("project-0");
    assert_eq!(h.rt.state.scope, Scope::Project(h.project_dir()));
}

/// A project with `alpha`, `beta` and `gamma`, each only in Claude's folder.
fn three_skills() -> Harness {
    let h = Harness::project();
    for name in ["alpha", "beta", "gamma"] {
        skill(&h.project_dir().join(".claude/skills"), name);
    }
    h
}

impl Harness {
    fn rescan(&mut self) {
        self.rt.update(|s, _| s.rescan());
        self.settle();
    }
}

#[test]
fn ticked_skills_sync_alone() {
    let mut h = three_skills();
    h.rescan();
    h.click("check-alpha");
    h.click("check-gamma");
    assert_eq!(h.rt.state.checked, ["alpha", "gamma"]);
    assert!(h.rt.find("bulk-bar").is_some());
    assert_eq!(h.rt.state.selected, None, "ticking doesn't open the inspector");

    h.click("bulk-sync");
    let plan = h.rt.state.sync_plan.clone().unwrap();
    let skills: std::collections::BTreeSet<&str> = plan.actions.iter().map(|a| match a {
        crate::sync::Action::Adopt { skill, .. }
        | crate::sync::Action::AdoptLink { skill, .. }
        | crate::sync::Action::Link { skill, .. }
        | crate::sync::Action::Replace { skill, .. } => skill.as_str(),
    }).collect();
    assert_eq!(skills.into_iter().collect::<Vec<_>>(), ["alpha", "gamma"]);

    // Changing the targets keeps the dialog on the ticked skills.
    h.click("target-opencode");
    assert_eq!(h.rt.state.sync_only.as_deref(), Some(&["alpha".to_string(), "gamma".to_string()][..]));
    h.click("sync-apply");

    for name in ["alpha", "gamma"] {
        assert_eq!(h.placement(name, "codex"), Placement::Linked, "{name}");
    }
    assert_eq!(h.placement("beta", "codex"), Placement::Missing, "unticked skills are left alone");
    assert_eq!(h.placement("review", "codex"), Placement::Missing);
    assert!(h.rt.state.checked.is_empty(), "a finished sync clears the ticks");
}

#[test]
fn ticked_skills_are_removed_together_after_one_confirmation() {
    let mut h = three_skills();
    h.rescan();
    h.click("check-alpha");
    h.click("check-beta");
    h.click("bulk-remove");
    assert_eq!(h.rt.state.confirm_remove, Some(vec!["alpha".to_string(), "beta".to_string()]));
    h.click("remove-confirm");

    let names: Vec<&str> = h.rt.state.inv.skills.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["gamma", "review"]);
    assert!(h.rt.state.checked.is_empty());
    assert!(h.rt.state.notice.as_ref().is_some_and(|n| n.title == "Removed 2 skills"));
}

#[test]
fn tick_all_follows_the_filter_and_toggles_off() {
    let mut h = three_skills();
    h.rescan();
    h.rt.update(|s, _| s.filter = rune::TextEdit::new("a"));
    h.settle();
    h.click("check-all");
    assert_eq!(h.rt.state.checked, ["alpha", "beta", "gamma"], "review has no a in its name or description");
    h.click("check-all");
    assert!(h.rt.state.checked.is_empty());

    h.click("check-beta");
    h.click("bulk-clear");
    assert!(h.rt.state.checked.is_empty());

    h.click("check-beta");
    h.click("scope-global");
    assert!(h.rt.state.checked.is_empty(), "ticks belong to one scope");
}

/// Installs a real skill from skills.sh through its CLI. Needs the network and Node:
/// `cargo test -- --ignored`.
#[test]
#[ignore]
fn installs_from_skills_sh_into_a_project_and_links_it() {
    let mut h = Harness::project();
    h.click("browse");
    let found = crate::remote::search("stop-slop").expect("search works");
    let hit = found.into_iter().find(|r| r.source == "hardikpandya/stop-slop").expect("stop-slop is listed");
    h.rt.update(move |s, _| s.select_remote(hit));
    h.settle();
    h.click("install");
    assert!(h.rt.state.installing.is_some());
    let start = std::time::Instant::now();
    while h.rt.state.installing.is_some() {
        assert!(start.elapsed().as_secs() < 180, "install timed out");
        std::thread::sleep(std::time::Duration::from_millis(200));
        h.rt.update(|s, _| {
            s.poll();
        });
    }
    assert!(h.rt.state.install_log.as_ref().is_some_and(|l| l.is_ok()), "{:?}", h.rt.state.install_log);
    let dir = h.project_dir();
    h.rt.update(move |s, _| s.open_scope(Scope::Project(dir)));
    assert_eq!(h.placement("stop-slop", "agents"), Placement::Dir);
    for agent in ["claude", "codex", "opencode"] {
        assert_eq!(h.placement("stop-slop", agent), Placement::Linked, "{agent}");
    }
    assert!(!h.root.join(".agents").exists(), "a project install stays in the project");
}
