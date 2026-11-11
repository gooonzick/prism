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
        rt.on_key = Some(crate::ui::on_key);
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

impl Harness {
    fn click_with(&mut self, key: &str, mods: rune::Mods) {
        self.rt.physical_input(rune::PhysicalInput::Modifiers(mods));
        self.rt.frame(0.0);
        self.click(key);
        self.rt.physical_input(rune::PhysicalInput::Modifiers(rune::Mods::default()));
        self.rt.frame(0.0);
    }
}

#[test]
fn cmd_click_toggles_and_shift_click_ticks_a_range() {
    let mut h = three_skills();
    h.rescan();
    let cmd = rune::Mods { cmd: true, ..rune::Mods::default() };
    let shift = rune::Mods { shift: true, ..rune::Mods::default() };

    h.click_with("skill-alpha", cmd);
    assert_eq!(h.rt.state.checked, ["alpha"]);
    assert_eq!(h.rt.state.selected, None, "a Cmd-click ticks instead of opening");

    h.click_with("skill-gamma", shift);
    assert_eq!(h.rt.state.checked, ["alpha", "beta", "gamma"], "Shift extends from the last row clicked");

    h.click_with("skill-beta", cmd);
    assert_eq!(h.rt.state.checked, ["alpha", "gamma"]);

    // A plain click opens a row and moves the anchor, leaving ticks alone.
    h.click("skill-review");
    assert_eq!(h.rt.state.selected.as_deref(), Some("review"));
    assert_eq!(h.rt.state.checked, ["alpha", "gamma"]);
    h.click_with("skill-beta", shift);
    assert_eq!(h.rt.state.checked, ["alpha", "beta", "gamma", "review"]);
}

#[test]
fn shift_on_a_checkbox_ticks_a_range_too() {
    let mut h = three_skills();
    h.rescan();
    h.click("check-alpha");
    h.click_with("check-gamma", rune::Mods { shift: true, ..rune::Mods::default() });
    assert_eq!(h.rt.state.checked, ["alpha", "beta", "gamma"]);
}

fn key(h: &mut Harness, key: rune::Key, mods: rune::Mods) {
    h.rt.key(rune::KeyEvent { key, mods });
    h.settle();
}

#[test]
fn status_filters_and_broken_link_cleanup_with_undo() {
    let mut h = three_skills();
    let claude = h.project_dir().join(".claude/skills");
    std::os::unix::fs::symlink("/nowhere/gone", claude.join("gone")).unwrap();
    h.rescan();
    h.rt.update(|s, _| s.set_status_filter(crate::app::Filter::Broken));
    h.settle();
    assert_eq!(h.rt.state.visible().len(), 1);
    assert!(h.rt.find("skill-gone").is_some() && h.rt.find("skill-alpha").is_none());

    h.click("clean-broken");
    assert!(h.rt.state.inv.get("gone").is_none());
    assert_eq!(h.rt.state.status_filter, crate::app::Filter::All, "the filter it emptied lets go");
    assert!(h.rt.state.notice.as_ref().is_some_and(|n| n.undo));

    key(&mut h, rune::Key::Command('z'), rune::Mods { cmd: true, ..rune::Mods::default() });
    assert!(h.rt.state.inv.get("gone").is_some(), "Undo puts the link back");
}

#[test]
fn a_sync_is_undone_in_one_step() {
    let mut h = Harness::project();
    h.click("sync");
    h.click("sync-apply");
    assert_eq!(h.placement("review", "codex"), Placement::Linked);
    h.rt.update(|s, _| s.undo());
    h.settle();
    assert_eq!(h.placement("review", "agents"), Placement::Missing);
    assert_eq!(h.placement("review", "claude"), Placement::Dir);
    assert_eq!(h.placement("review", "codex"), Placement::Missing);
    assert!(h.rt.state.last_change.is_none());
}

#[test]
fn copy_a_project_skill_to_global() {
    let mut h = Harness::project();
    h.click("skill-review");
    h.click("copy");
    assert!(h.rt.state.copy_dialog.is_some());
    h.click("copy-to-0");
    assert!(h.root.join(".agents/skills/review/SKILL.md").is_file());
    assert_eq!(fs::read_link(h.root.join(".claude/skills/review")).unwrap(), PathBuf::from("../../.agents/skills/review"));
    assert_eq!(h.rt.state.stats[&Scope::Global].total, 1);
    assert!(h.project_dir().join(".claude/skills/review/SKILL.md").is_file(), "the original stays");

    h.click("scope-global");
    h.click("project-0");
    assert!(h.rt.find("shadow-tag").is_some(), "the project copy is marked as also global");
}

#[test]
fn a_new_skill_is_written_linked_and_selected() {
    let mut h = Harness::project();
    h.click("new-skill");
    h.rt.update(|s, _| {
        let f = s.new_skill.as_mut().unwrap();
        f.name = rune::TextEdit::new("Bad Name");
    });
    h.click("new-create");
    assert!(h.rt.state.new_skill.as_ref().unwrap().error.is_some());

    h.rt.update(|s, _| {
        let f = s.new_skill.as_mut().unwrap();
        f.name = rune::TextEdit::new("pdf-tools");
        f.description = rune::TextEdit::new("Use when: working with PDFs");
    });
    h.click("new-create");
    assert!(h.rt.state.new_skill.is_none());
    let md = fs::read_to_string(h.project_dir().join(".agents/skills/pdf-tools/SKILL.md")).unwrap();
    assert_eq!(crate::skill::parse(&md).description.as_deref(), Some("Use when: working with PDFs"));
    assert_eq!(h.placement("pdf-tools", "claude"), Placement::Linked);
    assert_eq!(h.rt.state.selected.as_deref(), Some("pdf-tools"));
}

#[test]
fn arrows_move_the_selection_and_space_ticks() {
    let mut h = three_skills();
    h.rescan();
    let none = rune::Mods::default();
    key(&mut h, rune::Key::Down, none);
    assert_eq!(h.rt.state.selected.as_deref(), Some("alpha"));
    key(&mut h, rune::Key::Down, none);
    assert_eq!(h.rt.state.selected.as_deref(), Some("beta"));
    key(&mut h, rune::Key::Text(" ".into()), none);
    assert_eq!(h.rt.state.checked, ["beta"]);
    key(&mut h, rune::Key::Down, rune::Mods { shift: true, ..none });
    assert_eq!(h.rt.state.checked, ["beta", "gamma"]);
    key(&mut h, rune::Key::Up, none);
    key(&mut h, rune::Key::Up, none);
    key(&mut h, rune::Key::Up, none);
    assert_eq!(h.rt.state.selected.as_deref(), Some("alpha"), "it stops at the top");
}

#[test]
fn found_projects_are_added_as_ticked() {
    let mut h = Harness::project();
    let (a, b) = (h.root.join("dev/a"), h.root.join("dev/b"));
    h.rt.update(move |s, _| s.land_discovery_for_test(a, b));
    h.settle();
    assert!(h.rt.find("found-dialog").is_some() || h.rt.state.found.is_some());
    h.click("found-1");
    h.click("found-add");
    assert_eq!(h.rt.state.settings.projects.len(), 2, "the project already listed, plus the one left ticked");
}

impl Harness {
    /// Add project… on `dir`, waiting for the search inside it to land.
    fn add_folder(&mut self, dir: PathBuf) {
        self.rt.update(move |s, _| s.add_folder(dir));
        let start = std::time::Instant::now();
        while self.rt.state.discovering {
            assert!(start.elapsed().as_secs() < 20, "the search timed out");
            std::thread::sleep(std::time::Duration::from_millis(20));
            self.rt.update(|s, _| {
                s.poll();
            });
        }
        self.settle();
    }
}

#[test]
fn add_project_takes_a_folder_with_skills_as_is() {
    let mut h = Harness::project();
    let dir = h.root.join("dev/site");
    skill(&dir.join(".agents/skills"), "deploy");
    h.add_folder(dir.clone());
    assert!(h.rt.state.settings.projects.contains(&dir));
    assert_eq!(h.rt.state.scope, Scope::Project(dir), "the new project opens");
    assert!(h.rt.state.found.is_none());
}

#[test]
fn add_project_on_a_folder_of_projects_offers_the_ones_inside() {
    let mut h = Harness::project();
    let (a, b) = (h.root.join("dev/a"), h.root.join("dev/b"));
    skill(&a.join(".claude/skills"), "one");
    skill(&b.join(".agents/skills"), "two");
    h.add_folder(h.root.join("dev"));
    assert!(h.rt.find("found-add").is_some(), "the dialog lists them; notice: {:?}", h.rt.state.notice.as_ref().map(|n| &n.title));
    let offered: Vec<PathBuf> = h.rt.state.found.as_ref().unwrap().projects.iter().map(|p| p.0.clone()).collect();
    assert_eq!(offered, [a, b]);
    assert!(!h.rt.state.settings.projects.contains(&h.root.join("dev")), "the parent itself is not added");
    h.click("found-add");
    assert_eq!(h.rt.state.settings.projects.len(), 3);
}

#[test]
fn add_project_adds_an_empty_folder_but_never_home() {
    let mut h = Harness::project();
    let empty = h.root.join("dev/new");
    fs::create_dir_all(&empty).unwrap();
    h.add_folder(empty.clone());
    assert!(h.rt.state.settings.projects.contains(&empty), "nothing inside has skills: the folder itself");

    let mut h2 = Harness::project();
    fs::remove_dir_all(h2.project_dir()).unwrap();
    h2.rt.update(|s, _| s.settings.projects.clear());
    let home = h2.root.clone();
    h2.add_folder(home.clone());
    assert!(!h2.rt.state.settings.projects.contains(&home), "home is the Global scope");
    assert!(h2.rt.state.notice.is_some());
}

#[test]
fn add_project_on_a_listed_project_just_opens_it() {
    let mut h = Harness::project();
    let dir = h.project_dir();
    h.rt.update(|s, _| s.open_scope(Scope::Global));
    h.add_folder(dir.clone());
    assert_eq!(h.rt.state.settings.projects, std::slice::from_ref(&dir));
    assert_eq!(h.rt.state.scope, Scope::Project(dir));
}

#[test]
fn updates_show_as_tags_and_a_filter() {
    let mut h = three_skills();
    h.rescan();
    let scope = h.rt.state.scope.clone();
    h.rt.update(move |s, _| s.updates.insert(scope, vec!["beta".into()]).map(|_| ()).unwrap_or(()));
    h.settle();
    assert_eq!(h.rt.state.filter_count(crate::app::Filter::Updates), 1);
    assert!(h.rt.find("update-tag").is_some(), "beta wears an update tag");
    h.click("check-beta");
    assert!(h.rt.find("bulk-update").is_some());
    h.click("skill-beta");
    assert!(h.rt.find("update-banner").is_some());
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

#[test]
fn a_new_change_clears_the_backups_of_the_last_one() {
    let mut h = Harness::project();
    let paths = h.rt.state.paths.clone();
    let root = paths.home.clone();
    let (a, b) = (root.join("a.json"), root.join("b.json"));
    std::fs::write(&a, "0").unwrap();
    std::fs::write(&b, "0").unwrap();
    let backups = |name: &str| paths.state.join("backups").join(format!("0-{name}"));
    h.rt.update(|s, _| {
        let mut j = s.new_journal();
        j.write_file(&a, b"1", &s.paths).unwrap();
    });
    assert!(backups("a.json").exists());
    h.rt.update(|s, _| {
        let mut j = s.new_journal();
        j.write_file(&b, b"1", &s.paths).unwrap();
    });
    assert!(!backups("a.json").exists());
    assert!(backups("b.json").exists());
}

#[test]
fn a_full_undo_in_the_window_leaves_no_backups() {
    let mut h = Harness::project();
    let paths = h.rt.state.paths.clone();
    let a = paths.home.join("a.json");
    std::fs::write(&a, "token").unwrap();
    h.rt.update(|s, _| {
        let mut j = s.new_journal();
        j.write_file(&a, b"1", &s.paths).unwrap();
        crate::store::save_last_change(&s.paths, Some(&crate::store::LastChange { label: "edit a".into(), journal: j }));
    });
    assert!(paths.state.join("backups").exists());
    h.rt.update(|s, _| s.undo());
    assert_eq!(std::fs::read(&a).unwrap(), b"token");
    assert!(!paths.state.join("backups").exists(), "no copy of the original stays behind");
}

/// Claude Code and Cursor installed, both with the same global `plata` server, whose env holds a
/// literal token and a reference, and which Claude Code skips in the project; the window opens on the project, as in [`Harness::project`].
fn mcp_home() -> Harness {
    let mut h = Harness::project();
    fs::create_dir_all(h.root.join(".claude")).unwrap();
    fs::create_dir_all(h.root.join(".cursor")).unwrap();
    let plata = serde_json::json!({
        "command": "npx",
        "args": ["-y", "plata-mcp", "--api-key=fake-arg-secret"],
        "env": { "PLATA_TOKEN": "fake-env-secret", "PLATA_HOST": "${PLATA_HOST}" }
    });
    let project = h.project_dir().display().to_string();
    let claude = serde_json::json!({
        "mcpServers": { "plata": plata },
        "projects": { project: { "mcpServers": {}, "disabledMcpServers": ["plata"] } }
    });
    fs::write(h.root.join(".claude.json"), claude.to_string()).unwrap();
    // The same server in Cursor's syntax for references.
    let mut cursor = plata.clone();
    cursor["env"]["PLATA_HOST"] = "${env:PLATA_HOST}".into();
    fs::write(h.root.join(".cursor/mcp.json"), serde_json::json!({ "mcpServers": { "plata": cursor } }).to_string()).unwrap();
    h.rescan();
    h
}

impl Harness {
    /// Every text drawn in the window, one per line.
    fn texts(&self) -> String {
        self.rt.elements().into_iter().filter_map(|e| e.text).collect::<Vec<_>>().join("\n")
    }
}

#[test]
fn mcp_tab_lists_servers_with_agents() {
    let mut h = mcp_home();
    h.click("scope-global");
    h.click("tab-mcp");
    assert_eq!(h.rt.state.page, crate::app::Page::Mcp);
    assert!(h.rt.find("mcp-row-plata").is_some());
    for agent in ["claude", "cursor"] {
        assert!(h.rt.find(&format!("mcp-row-plata/agents/agent-{agent}")).is_some(), "{agent} chip");
    }
    assert!(h.texts().contains("in sync"), "{}", h.texts());

    // Another scope keeps the MCP page; the skills tab goes back.
    h.click("project-0");
    assert_eq!(h.rt.state.page, crate::app::Page::Mcp);
    assert_eq!(h.rt.state.mcp.scope, Scope::Project(h.project_dir()));
    h.click("tab-skills");
    assert_eq!(h.rt.state.page, crate::app::Page::Installed);
    assert!(h.rt.find("skill-review").is_some());
}

#[test]
fn the_merge_hint_reads_as_one_sentence() {
    let mut h = Harness::project();
    fs::create_dir_all(h.root.join(".cursor")).unwrap();
    let servers = serde_json::json!({ "mcpServers": {
        "figma": { "command": "npx", "args": ["-y", "figma-developer-mcp", "--stdio"] },
        "figma-dev": { "command": "npx", "args": ["figma-developer-mcp"] },
    } });
    fs::write(h.root.join(".cursor/mcp.json"), servers.to_string()).unwrap();
    h.click("scope-global");
    h.click("tab-mcp");
    let shown = h.texts();
    assert!(shown.contains("Maybe one server: figma, figma-dev (same package figma-developer-mcp, args differ)"), "{shown}");
}

#[test]
fn inspector_masks_until_reveal() {
    let mut h = mcp_home();
    h.click("scope-global");
    h.click("tab-mcp");
    h.click("mcp-row-plata");
    assert_eq!(h.rt.state.mcp_selected.as_deref(), Some("plata"));
    let shown = h.texts();
    assert!(shown.contains("PLATA_TOKEN") && shown.contains("${PLATA_HOST}"), "{shown}");
    assert!(shown.contains("--api-key=***"), "{shown}");
    assert!(!shown.contains("fake-env-secret") && !shown.contains("fake-arg-secret"), "{shown}");

    h.click("mcp-reveal");
    assert!(h.rt.state.mcp_revealed.contains("plata"));
    let shown = h.texts();
    assert!(shown.contains("fake-env-secret") && shown.contains("--api-key=fake-arg-secret"), "{shown}");
    assert!(shown.contains("${PLATA_HOST}"), "references show as written: {shown}");

    h.click("mcp-reveal");
    assert!(!h.texts().contains("fake-env-secret"));
}

#[test]
fn inherited_global_server_shows_in_project() {
    let mut h = mcp_home();
    let own = h.project_dir().join(".cursor/mcp.json");
    fs::create_dir_all(own.parent().unwrap()).unwrap();
    fs::write(&own, r#"{"mcpServers":{"local-db":{"command":"db-mcp"}}}"#).unwrap();
    h.rescan();
    h.click("tab-mcp");
    assert_eq!(h.rt.state.scope, Scope::Project(h.project_dir()));
    assert!(h.rt.state.mcp.get("plata").is_some_and(|s| s.inherited));
    assert!(h.rt.find("mcp-row-plata/tags/global-tag").is_some(), "the inherited row says global");
    assert!(h.rt.find("mcp-row-plata/tags/disabled-tag").is_some(), "Claude Code skips it here");
    assert!(h.rt.find("mcp-row-local-db").is_some());
    assert!(h.rt.find("mcp-row-local-db/tags/global-tag").is_none(), "the project's own server isn't");
}

#[test]
fn window_picks_up_a_cli_change() {
    let mut h = mcp_home();
    h.click("tab-mcp");
    let wakes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = wakes.clone();
    h.rt.update(move |s, _| {
        s.watch_with(std::sync::Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }))
    });
    std::thread::sleep(std::time::Duration::from_millis(300));

    let project = h.project_dir().display().to_string();
    let mut ctx = crate::cli::Ctx { paths: h.rt.state.paths.clone(), settings: h.rt.state.settings.clone(), stdin: None, path_var: None, make_runner: crate::hosts::runner::no_runner() };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = ["mcp", "add", "fresh", "--command", "fresh-mcp", "--to", "cursor", "--project", &project];
    let code = crate::cli::run_with(&args, &mut ctx, &mut out, &mut err);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));

    let start = std::time::Instant::now();
    while h.rt.state.mcp.get("fresh").is_none() {
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "the window never saw the new server");
        std::thread::sleep(std::time::Duration::from_millis(50));
        h.rt.update(|s, _| {
            s.poll();
        });
    }
    h.settle();
    assert!(h.rt.find("mcp-row-fresh").is_some());
    assert!(h.rt.state.last_change.as_ref().is_some_and(|c| c.label.contains("fresh")), "the CLI's change is the one to undo");
}

// MCP dialogs.

impl Harness {
    /// Clicks the button whose accessible name is `label` (the toast's actions carry no key).
    fn click_button(&mut self, label: &str) {
        let e = self.rt.elements().into_iter().find(|e| e.role == "button" && e.label.as_deref() == Some(label) && !e.exiting);
        let r = e.unwrap_or_else(|| panic!("no button {label}")).rect;
        let (x, y) = (r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
        self.rt.pointer_down(x, y);
        self.rt.pointer_up(x, y);
        self.settle();
    }

    /// Fills the open Add/Edit dialog's fields through the app, as typing would.
    fn fill_form(&mut self, f: impl FnOnce(&mut crate::app::McpForm) + 'static) {
        self.rt.update(move |s, _| f(s.mcp_form.as_mut().expect("the MCP form is open")));
        self.settle();
    }

    fn mcp_global(&mut self) {
        self.click("scope-global");
        self.click("tab-mcp");
    }
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap()
}

fn json(p: &Path) -> serde_json::Value {
    serde_json::from_str(&read(p)).unwrap()
}

#[test]
fn add_dialog_writes_server_to_ticked_agents() {
    let mut h = mcp_home();
    h.mcp_global();
    h.click("mcp-add");
    let form = h.rt.state.mcp_form.as_ref().expect("the Add dialog is open");
    assert_eq!(form.agents, [crate::mcp::agents::index_of("claude").unwrap()], "the sync targets that hold MCP servers");
    h.fill_form(|f| {
        f.name = rune::TextEdit::new("fresh");
        f.command = rune::TextEdit::new(r#"npx -y fresh-mcp --root "/a b""#);
    });
    h.click("mcp-add-env-add");
    h.fill_form(|f| {
        f.env[0].key = rune::TextEdit::new("FRESH_TOKEN");
        f.env[0].value = rune::TextEdit::new("${FRESH_TOKEN}").secure();
    });
    h.click("mcp-add-agent-cursor");
    h.click("mcp-add-save");

    assert!(h.rt.state.mcp_form.is_none(), "{:?}", h.rt.state.mcp_form.as_ref().and_then(|f| f.error.clone()));
    let claude = json(&h.root.join(".claude.json"));
    let fresh = &claude["mcpServers"]["fresh"];
    assert_eq!(fresh["command"], "npx");
    assert_eq!(fresh["args"], serde_json::json!(["-y", "fresh-mcp", "--root", "/a b"]));
    assert_eq!(fresh["env"]["FRESH_TOKEN"], "${FRESH_TOKEN}", "a typed ${{VAR}} is a reference");
    assert_eq!(claude["mcpServers"]["plata"]["env"]["PLATA_TOKEN"], "fake-env-secret", "the rest stays");
    let cursor = json(&h.root.join(".cursor/mcp.json"));
    assert_eq!(cursor["mcpServers"]["fresh"]["env"]["FRESH_TOKEN"], "${env:FRESH_TOKEN}", "in Cursor's syntax");
    assert!(h.rt.state.mcp.get("fresh").is_some());
    let n = h.rt.state.notice.as_ref().unwrap();
    assert!(!n.error && n.undo, "{}", n.title);
}

#[test]
fn copy_dialog_requires_source_when_copies_differ() {
    let mut h = mcp_home();
    fs::create_dir_all(h.root.join(".gemini")).unwrap();
    let cursor = h.root.join(".cursor/mcp.json");
    fs::write(&cursor, r#"{"mcpServers":{"plata":{"command":"npx","args":["-y","plata-mcp@2"]}}}"#).unwrap();
    h.mcp_global();
    h.click("mcp-row-plata");
    h.click("mcp-copy");
    let d = h.rt.state.mcp_dialog.as_ref().expect("the copy dialog is open");
    assert!(d.plan.is_none(), "no plan until a source is picked");
    assert!(h.texts().contains("Pick the copy to copy from"), "{}", h.texts());
    h.click("mcp-apply");
    assert!(h.rt.state.mcp_dialog.is_some(), "Apply does nothing without a source");
    assert_eq!(json(&cursor)["mcpServers"]["plata"]["args"], serde_json::json!(["-y", "plata-mcp@2"]));

    h.click("mcp-copy-from-claude");
    h.click("mcp-copy-to-gemini");
    let lines = h.rt.state.mcp_dialog.as_ref().and_then(|d| d.plan.clone()).unwrap().unwrap().lines(&h.rt.state.paths);
    assert_eq!(lines, ["Overwrite Cursor's plata with Claude Code's", "Add plata to Gemini CLI"]);
    assert!(h.texts().contains("Add plata to Gemini CLI"), "the plan is drawn");
    h.click("mcp-apply");
    assert!(h.rt.state.mcp_dialog.is_none());
    assert_eq!(json(&cursor)["mcpServers"]["plata"]["args"], serde_json::json!(["-y", "plata-mcp", "--api-key=fake-arg-secret"]));
    assert!(json(&h.root.join(".gemini/settings.json"))["mcpServers"]["plata"].is_object());
    assert_eq!(h.rt.state.last_change.as_ref().unwrap().label, "copy plata");
}

#[test]
fn remove_then_undo_from_toast() {
    let mut h = mcp_home();
    let (claude, cursor) = (h.root.join(".claude.json"), h.root.join(".cursor/mcp.json"));
    let before = (read(&claude), read(&cursor));
    h.mcp_global();
    h.click("mcp-row-plata");
    h.click("mcp-remove");
    assert!(h.rt.find("mcp-remove-from-claude").is_some() && h.rt.find("mcp-remove-from-cursor").is_some());
    h.click("mcp-apply");
    assert!(h.rt.state.mcp.get("plata").is_none());
    assert!(json(&claude)["mcpServers"].get("plata").is_none());
    assert_eq!(h.rt.state.last_change.as_ref().unwrap().label, "remove plata");

    h.click_button("Undo");
    assert_eq!((read(&claude), read(&cursor)), before, "undo puts both files back byte for byte");
    assert!(h.rt.state.mcp.get("plata").is_some());
}

#[test]
fn read_only_server_has_no_actions() {
    let mut h = mcp_home();
    let claude = h.root.join(".claude.json");
    let mut doc = json(&claude);
    doc["mcpServers"]["pencil"] = serde_json::json!({ "command": "/Applications/Pencil.app/Contents/Resources/mcp" });
    fs::write(&claude, doc.to_string()).unwrap();
    h.mcp_global();
    h.click("mcp-row-plata");
    for key in ["mcp-copy", "mcp-edit", "mcp-remove"] {
        assert!(h.rt.find(key).is_some(), "plata offers {key}");
    }
    h.click("mcp-row-pencil");
    for key in ["mcp-copy", "mcp-edit", "mcp-remove", "mcp-disable", "mcp-enable"] {
        assert!(h.rt.find(key).is_none(), "pencil offers no {key}");
    }
}

#[test]
fn inherited_row_offers_edit_and_switch_only() {
    let mut h = mcp_home();
    h.click("tab-mcp");
    h.click("mcp-row-plata");
    assert!(h.rt.find("mcp-enable").is_some(), "Claude Code skips plata here");
    assert!(h.rt.find("mcp-edit").is_some(), "editing an inherited row edits the global server");
    for key in ["mcp-copy", "mcp-remove"] {
        assert!(h.rt.find(key).is_none(), "a global server offers no {key} in a project");
    }
    h.click("mcp-edit");
    h.fill_form(|f| f.env.retain(|p| p.key.text != "PLATA_HOST"));
    h.click("mcp-add-save");
    let d = h.rt.state.mcp_dialog.as_ref().expect("the edit plan is open");
    assert!(d.plan.clone().unwrap().unwrap().warnings.iter().any(|w| w == "edits the global server"));
    h.click("mcp-cancel");
    h.click("mcp-enable");
    h.click("mcp-apply");
    let claude = json(&h.root.join(".claude.json"));
    let project = h.project_dir().display().to_string();
    assert_eq!(claude["projects"][&project]["disabledMcpServers"], serde_json::json!([]));
    assert_eq!(h.rt.state.last_change.as_ref().unwrap().label, "enable plata");
}

#[test]
fn apply_refused_when_files_changed_replans() {
    let mut h = mcp_home();
    let (claude, cursor) = (h.root.join(".claude.json"), h.root.join(".cursor/mcp.json"));
    let original = read(&claude);
    h.mcp_global();
    // A first change, to undo at the end: its backups must survive the refused apply.
    h.click("mcp-add");
    h.fill_form(|f| {
        f.name = rune::TextEdit::new("first");
        f.command = rune::TextEdit::new("first-mcp");
    });
    h.click("mcp-add-save");
    assert_eq!(h.rt.state.last_change.as_ref().unwrap().label, "add first");
    h.click("mcp-row-plata");
    h.click("mcp-remove");
    let first = h.rt.state.mcp_dialog.as_ref().and_then(|d| d.plan.clone()).unwrap().unwrap().id();
    // Someone else writes Cursor's config while the dialog is open.
    let mut doc = json(&cursor);
    doc["mcpServers"]["other"] = serde_json::json!({ "command": "other-mcp" });
    fs::write(&cursor, doc.to_string()).unwrap();
    h.click("mcp-apply");

    let d = h.rt.state.mcp_dialog.as_ref().expect("the dialog stays open");
    assert_eq!(d.notice.as_deref(), Some("The files changed; here is the new plan"));
    assert_ne!(d.plan.clone().unwrap().unwrap().id(), first);
    assert!(h.texts().contains("The files changed; here is the new plan"));
    assert!(json(&cursor)["mcpServers"]["plata"].is_object(), "nothing was written");
    assert_eq!(h.rt.state.last_change.as_ref().unwrap().label, "add first", "the refused apply is no change");
    h.click("mcp-cancel");
    key(&mut h, rune::Key::Command('z'), rune::Mods { cmd: true, ..rune::Mods::default() });
    assert_eq!(read(&claude), original, "the first change still undoes");

    h.click("mcp-row-plata");
    h.click("mcp-remove");

    h.click("mcp-apply");
    assert!(h.rt.state.mcp_dialog.is_none());
    let after = json(&cursor);
    assert!(after["mcpServers"].get("plata").is_none() && after["mcpServers"]["other"].is_object());
}

#[test]
fn add_validation_messages() {
    let mut h = mcp_home();
    let claude = h.root.join(".claude.json");
    let before = read(&claude);
    h.mcp_global();
    h.click("mcp-add");
    let error = |h: &Harness| h.rt.state.mcp_form.as_ref().and_then(|f| f.error.clone()).unwrap_or_default();

    h.click("mcp-add-save");
    assert_eq!(error(&h), "Name the server");
    h.fill_form(|f| f.name = rune::TextEdit::new("plata"));
    h.click("mcp-add-save");
    assert_eq!(error(&h), "plata is already here; use Copy to add it to more agents");
    h.fill_form(|f| f.name = rune::TextEdit::new("fresh"));
    h.click("mcp-add-save");
    assert_eq!(error(&h), "Give the command to run");
    assert!(h.texts().contains("Give the command to run"), "the message is drawn");
    h.fill_form(|f| f.command = rune::TextEdit::new("fresh-mcp"));
    h.click("mcp-add-env-add");
    h.click("mcp-add-env-add");
    h.fill_form(|f| {
        f.env[0].key = rune::TextEdit::new("TOKEN");
        f.env[1].key = rune::TextEdit::new("TOKEN");
        f.env[1].value = rune::TextEdit::new("fake-dup-secret").secure();
    });
    h.click("mcp-add-save");
    assert_eq!(error(&h), "Two variables are named TOKEN");
    assert!(!h.texts().contains("fake-dup-secret"));
    h.click("mcp-add-kind-http");
    h.click("mcp-add-save");
    assert_eq!(error(&h), "Give the server's URL");
    h.fill_form(|f| {
        f.url = rune::TextEdit::new("https://fresh.example.com/mcp");
        f.headers = Vec::new();
    });
    h.click("mcp-add-header-add");
    h.click("mcp-add-header-add");
    h.fill_form(|f| {
        f.headers[0].key = rune::TextEdit::new("Authorization");
        f.headers[1].key = rune::TextEdit::new("authorization");
    });
    h.click("mcp-add-save");
    assert_eq!(error(&h), "Two headers are named authorization", "header names match without case");
    h.click("mcp-add-header-1-remove");
    h.fill_form(|f| f.url = rune::TextEdit::new("https://fresh.example.com/mcp"));
    h.click("mcp-add-agent-claude");
    h.click("mcp-add-save");
    assert_eq!(error(&h), "Pick at least one agent");
    assert_eq!(read(&claude), before, "nothing was written");
}

#[test]
fn cmd_n_opens_add_on_mcp_page() {
    let mut h = mcp_home();
    let cmd = rune::Mods { cmd: true, ..rune::Mods::default() };
    key(&mut h, rune::Key::Command('n'), cmd);
    assert!(h.rt.state.new_skill.is_some() && h.rt.state.mcp_form.is_none(), "on the skills page it is New skill");
    h.click("new-cancel");
    h.click("tab-mcp");
    key(&mut h, rune::Key::Command('n'), cmd);
    assert!(h.rt.state.mcp_form.is_some() && h.rt.state.new_skill.is_none());
    assert!(h.rt.find("mcp-add-name").is_some());

    // An open form or plan keeps what it has.
    h.fill_form(|f| f.name = rune::TextEdit::new("typed"));
    key(&mut h, rune::Key::Command('n'), cmd);
    assert_eq!(h.rt.state.mcp_form.as_ref().unwrap().name.text, "typed");
    h.click("mcp-add-cancel");
    h.click("mcp-row-plata");
    h.click("mcp-enable");
    key(&mut h, rune::Key::Command('n'), cmd);
    assert!(h.rt.state.mcp_form.is_none() && h.rt.state.mcp_dialog.is_some());
}

#[test]
fn undo_after_add_restores_config_bytes() {
    let mut h = mcp_home();
    let claude = h.root.join(".claude.json");
    let before = read(&claude);
    h.mcp_global();
    h.click("mcp-add");
    h.fill_form(|f| {
        f.name = rune::TextEdit::new("fresh");
        f.command = rune::TextEdit::new("fresh-mcp");
    });
    h.click("mcp-add-save");
    assert_ne!(read(&claude), before);
    assert_eq!(h.rt.state.last_change.as_ref().unwrap().label, "add fresh");
    key(&mut h, rune::Key::Command('z'), rune::Mods { cmd: true, ..rune::Mods::default() });
    assert_eq!(read(&claude), before);
    assert!(h.rt.state.mcp.get("fresh").is_none());
}

#[test]
fn edit_masks_prefilled_values_and_plans_changed_fields_only() {
    let mut h = mcp_home();
    h.mcp_global();
    h.click("mcp-row-plata");
    h.click("mcp-edit");
    let shown = h.texts();
    assert!(!shown.contains("fake-env-secret") && !shown.contains("fake-arg-secret"), "{shown}");
    let i = h.rt.state.mcp_form.as_ref().unwrap().env.iter().position(|p| p.key.text == "PLATA_TOKEN").unwrap();
    h.click(&format!("mcp-add-env-{i}-show"));
    assert!(h.rt.state.mcp_form.as_ref().unwrap().env[i].shown);
    h.fill_form(move |f| f.env[i].value = rune::TextEdit::new("new-secret").secure());
    h.click("mcp-add-save");

    assert!(h.rt.state.mcp_form.is_none());
    let lines = h.rt.state.mcp_dialog.as_ref().and_then(|d| d.plan.clone()).unwrap().unwrap().lines(&h.rt.state.paths);
    assert_eq!(lines, ["Edit plata in Claude Code, Cursor (env.PLATA_TOKEN)"]);
    h.click("mcp-apply");
    let cursor = json(&h.root.join(".cursor/mcp.json"));
    assert_eq!(cursor["mcpServers"]["plata"]["env"]["PLATA_TOKEN"], "new-secret");
    assert_eq!(cursor["mcpServers"]["plata"]["env"]["PLATA_HOST"], "${env:PLATA_HOST}", "the reference stays a reference");
    assert_eq!(h.rt.state.last_change.as_ref().unwrap().label, "edit plata");
}

#[test]
fn merge_from_a_hint_renames_the_odd_copy() {
    let mut h = mcp_home();
    let cursor = h.root.join(".cursor/mcp.json");
    fs::write(&cursor, r#"{"mcpServers":{"plata-two":{"command":"npx","args":["-y","plata-mcp","--api-key=fake-arg-secret"],"env":{"PLATA_TOKEN":"fake-env-secret","PLATA_HOST":"${env:PLATA_HOST}"}}}}"#).unwrap();
    h.mcp_global();
    assert!(!h.rt.state.mcp.merges.is_empty(), "plata and plata-two look like one server");
    h.click("mcp-merge-0");
    let d = h.rt.state.mcp_dialog.as_ref().expect("the merge dialog is open");
    assert!(d.plan.is_some());
    h.click("mcp-apply");
    assert!(json(&cursor)["mcpServers"]["plata"].is_object());
    assert_eq!(h.rt.state.last_change.as_ref().unwrap().label, "merge into plata");
}

#[test]
fn return_in_the_name_field_adds_the_server() {
    let mut h = mcp_home();
    h.mcp_global();
    h.click("mcp-add");
    h.fill_form(|f| f.command = rune::TextEdit::new("fresh-mcp"));
    h.click("mcp-add-name");
    h.rt.paste("fresh");
    h.settle();
    assert_eq!(h.rt.state.mcp_form.as_ref().unwrap().name.text, "fresh");
    key(&mut h, rune::Key::Enter, rune::Mods::default());
    assert!(h.rt.state.mcp_form.is_none());
    assert!(h.rt.state.mcp.get("fresh").is_some());
}

#[test]
fn refused_add_keeps_the_last_change_undoable() {
    let mut h = mcp_home();
    fs::create_dir_all(h.root.join(".gemini")).unwrap();
    let (claude, cursor, gemini) = (h.root.join(".claude.json"), h.root.join(".cursor/mcp.json"), h.root.join(".gemini/settings.json"));
    let before = (read(&claude), read(&cursor));
    h.mcp_global();
    h.click("mcp-row-plata");
    h.click("mcp-remove");
    h.click("mcp-apply");
    assert_eq!(h.rt.state.last_change.as_ref().unwrap().label, "remove plata");

    // An Add planned for Gemini, whose file then changes before it is written.
    h.click("mcp-add");
    h.fill_form(|f| {
        f.name = rune::TextEdit::new("fresh");
        f.command = rune::TextEdit::new("fresh-mcp");
    });
    let g = gemini.clone();
    h.rt.update(move |s, _| {
        let server = crate::mcp::model::Server { name: "fresh".into(), transport: s.mcp_form.as_ref().unwrap().transport().unwrap() };
        let to = vec![crate::mcp::agents::index_of("gemini").unwrap()];
        let request = crate::mcp::plan::Request::Add { server, to, claude_scope: Default::default() };
        let plan = crate::mcp::plan::plan(&s.mcp, &request, &s.paths).unwrap();
        fs::write(&g, r#"{"theme":"dark"}"#).unwrap();
        s.write_mcp_add("fresh", plan);
    });
    h.settle();
    let form = h.rt.state.mcp_form.as_ref().expect("the form stays open");
    assert_eq!(form.error.as_deref(), Some("The files changed; try again"));
    assert_eq!(read(&gemini), r#"{"theme":"dark"}"#, "nothing was written");
    assert_eq!(h.rt.state.last_change.as_ref().unwrap().label, "remove plata");

    h.click("mcp-add-cancel");
    key(&mut h, rune::Key::Command('z'), rune::Mods { cmd: true, ..rune::Mods::default() });
    assert_eq!((read(&claude), read(&cursor)), before, "the remove still undoes byte for byte");
}

// Environments.

/// A host's home whose `prism` reports `version`, with Claude Code installed.
fn host_home(version: &str) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(bin.join("prism"), format!("#!/bin/sh\necho \"prism {version}\"\n")).unwrap();
    fs::set_permissions(bin.join("prism"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir_all(home.path().join(".claude")).unwrap();
    home
}

/// A host that can't be reached.
struct Down;

impl crate::hosts::runner::Runner for Down {
    fn prism(&self, _: &[String], _: Option<Vec<u8>>) -> Result<crate::hosts::runner::Output, String> {
        Err("Can't reach box: ssh: connect to host box port 22: Connection refused".into())
    }
    fn shell(&self, _: &str, _: Option<Vec<u8>>) -> Result<crate::hosts::runner::Output, String> {
        Err("Can't reach box: ssh: connect to host box port 22: Connection refused".into())
    }
}

impl Harness {
    /// Saves host `box` (in memory) and hands out `runners` for it.
    fn with_host(&mut self, runners: crate::hosts::runner::MakeRunner) {
        self.rt.update(move |s, _| {
            s.settings.hosts.push(crate::hosts::Host { name: "box".into(), ssh: "me@box".into() });
            s.make_runner = runners;
        });
        self.settle();
    }

    /// Lands the host's answers as they come, until nothing more is asked of it: a change, then
    /// the state read after it.
    fn land_remote(&mut self) {
        let start = std::time::Instant::now();
        let pending = |s: &App| s.remote_load.is_pending() || s.remote_preview.is_pending() || s.host_act.is_pending() || s.across_pending();
        loop {
            while pending(&self.rt.state) {
                assert!(start.elapsed().as_secs() < 10, "the host never answered");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            // Nothing runs now: a poll that lands nothing asks nothing new either.
            let mut landed = false;
            self.rt.update(|s, _| landed = s.poll());
            self.settle();
            if !landed && !pending(&self.rt.state) && !self.rt.state.host_busy() {
                break;
            }
        }
    }

    fn remote_state(&self) -> Option<crate::app::RemoteState> {
        match &self.rt.state.env {
            crate::app::Env::Remote { state, .. } => Some(state.clone()),
            crate::app::Env::Local => None,
        }
    }
}

#[test]
fn switching_to_host_shows_its_skills() {
    let mut h = Harness::project();
    let remote = host_home(crate::hosts::install::VERSION);
    skill(&remote.path().join(".agents/skills"), "far");
    h.with_host(crate::hosts::runner::local_runners(&[("box", remote.path())]));
    let project = h.project_dir();

    h.rt.update(|s, _| s.switch_env(Some("box")));
    assert_eq!(h.remote_state(), Some(crate::app::RemoteState::Connecting));
    assert!(h.rt.state.inv.get("review").is_none(), "this machine's skills are stashed");
    h.land_remote();
    assert_eq!(h.remote_state(), Some(crate::app::RemoteState::Ready));
    assert_eq!(h.rt.state.scope, Scope::Global);
    assert!(h.rt.state.inv.get("far").is_some());
    assert!(h.rt.find("skill-far").is_some());
    assert!(h.texts().contains("box"), "the switcher names the host");
    assert_eq!(h.rt.state.local.as_ref().unwrap().scope, Scope::Project(project.clone()));
    assert!(h.rt.state.local.as_ref().unwrap().inv.get("review").is_some());
    assert!(h.rt.state.detected(index_of("claude").unwrap()), "the host's agents, from its state");

    // The preview comes from the host too.
    h.click("skill-far");
    h.land_remote();
    assert!(h.rt.state.preview.as_deref().is_some_and(|md| md.contains("# far")), "{:?}", h.rt.state.preview);

    h.rt.update(|s, _| s.switch_env(None));
    h.settle();
    assert!(h.rt.state.local.is_none() && !h.rt.state.is_remote());
    assert_eq!(h.rt.state.scope, Scope::Project(project));
    assert!(h.rt.state.inv.get("review").is_some() && h.rt.state.inv.get("far").is_none());
    assert!(h.rt.find("skill-review").is_some());
}

/// A host whose shell startup files print a line before every command's own output.
struct Chatty(Box<dyn crate::hosts::runner::Runner>);

impl crate::hosts::runner::Runner for Chatty {
    fn prism(&self, args: &[String], stdin: Option<Vec<u8>>) -> Result<crate::hosts::runner::Output, String> {
        let mut out = self.0.prism(args, stdin)?;
        out.stdout.splice(0..0, b"Welcome to box!\n".iter().copied());
        Ok(out)
    }
    fn shell(&self, script: &str, stdin: Option<Vec<u8>>) -> Result<crate::hosts::runner::Output, String> {
        let mut out = self.0.shell(script, stdin)?;
        out.stdout.splice(0..0, b"Welcome to box!\n".iter().copied());
        Ok(out)
    }
}

#[test]
fn a_chatty_host_still_loads_and_previews() {
    let mut h = Harness::project();
    let remote = host_home(crate::hosts::install::VERSION);
    skill(&remote.path().join(".agents/skills"), "far");
    let home = remote.path().to_path_buf();
    h.with_host(std::sync::Arc::new(move |host: &crate::hosts::Host| {
        Box::new(Chatty(crate::hosts::runner::local_runners(&[("box", &home)])(host))) as Box<dyn crate::hosts::runner::Runner>
    }));
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();
    assert_eq!(h.remote_state(), Some(crate::app::RemoteState::Ready));
    assert!(h.rt.state.inv.get("far").is_some());
    h.click("skill-far");
    h.land_remote();
    assert!(h.rt.state.preview.as_deref().is_some_and(|md| md.contains("# far")), "{:?}", h.rt.state.preview);
}

#[test]
fn unreachable_host_shows_retry() {
    let mut h = Harness::project();
    h.with_host(std::sync::Arc::new(|_: &crate::hosts::Host| Box::new(Down) as Box<dyn crate::hosts::runner::Runner>));
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();
    assert!(matches!(h.remote_state(), Some(crate::app::RemoteState::Unreachable(e)) if e.contains("Connection refused")));
    assert!(h.texts().contains("Can't reach box"));
    assert!(h.rt.find("env-retry").is_some());
    assert!(h.rt.find("installed").is_none(), "no list in place of the host's");

    h.click("env-retry");
    assert_eq!(h.remote_state(), Some(crate::app::RemoteState::Connecting));
    h.land_remote();
    assert!(h.rt.find("env-retry").is_some());

    // Another scope on a host that isn't in yet connects again too.
    h.click("scope-global");
    assert_eq!(h.remote_state(), Some(crate::app::RemoteState::Connecting));
    h.land_remote();
}

#[test]
fn needs_install_offers_install() {
    let mut h = Harness::project();
    let remote = host_home("0.0.1");
    h.with_host(crate::hosts::runner::local_runners(&[("box", remote.path())]));
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();
    assert_eq!(h.remote_state(), Some(crate::app::RemoteState::NeedsInstall(crate::hosts::install::Health::OtherVersion("0.0.1".into(), None))));
    let version = crate::hosts::install::VERSION;
    assert!(h.texts().contains(&format!("prism {version} isn't on box")), "{}", h.texts());
    assert!(h.rt.find("env-install").is_some());
}

#[test]
fn install_leaves_another_program_and_says_why() {
    let mut h = Harness::project();
    // A `prism` there that is something else: the install refuses before it fetches anything.
    let remote = host_home("0.0.1");
    let theirs = remote.path().join(".local/bin/prism");
    fs::write(&theirs, "#!/bin/sh\necho 'Prism image tools 4.1'\n").unwrap();
    h.with_host(crate::hosts::runner::local_runners(&[("box", remote.path())]));
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();
    assert_eq!(h.remote_state(), Some(crate::app::RemoteState::NeedsInstall(crate::hosts::install::Health::Missing(None))));
    h.click("env-install");
    h.land_remote();
    let notice = h.rt.state.notice.as_ref().expect("a toast");
    assert_eq!(notice.title, "Couldn't install prism on box");
    assert!(notice.error && notice.detail.contains("another program") && notice.detail.contains("--force"), "{}", notice.detail);
    assert!(fs::read_to_string(&theirs).unwrap().contains("image tools"), "left alone");
}

#[test]
fn add_host_saves_it_and_connects() {
    let mut h = Harness::project();
    let remote = host_home(crate::hosts::install::VERSION);
    h.rt.update(|s, _| s.make_runner = crate::hosts::runner::local_runners(&[("box", remote.path())]));
    // From the switcher's menu.
    h.click("env-switch-trigger");
    assert!(h.rt.state.env_menu);
    let option = h.rt.elements().into_iter().find(|e| e.role == "option" && e.label.as_deref() == Some("Add host…")).expect("the menu offers Add host…");
    let r = option.rect;
    h.rt.pointer_down(r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
    h.rt.pointer_up(r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
    h.settle();
    assert!(h.rt.find("host-add").is_some());
    h.rt.update(|s, _| {
        let f = s.add_host.as_mut().unwrap();
        f.name = rune::TextEdit::new("box");
        f.ssh = rune::TextEdit::new("-oProxyCommand=x");
    });
    h.click("host-add");
    assert!(h.rt.state.add_host.as_ref().unwrap().error.is_some(), "a target ssh reads as an option is refused");
    h.rt.update(|s, _| s.add_host.as_mut().unwrap().ssh = rune::TextEdit::new("me@box"));
    h.click("host-add");
    assert!(h.rt.state.add_host.is_none());
    assert_eq!(h.rt.state.local_settings().hosts, vec![crate::hosts::Host { name: "box".into(), ssh: "me@box".into() }]);
    h.land_remote();
    assert_eq!(h.remote_state(), Some(crate::app::RemoteState::Ready));
}

#[test]
fn browse_on_a_host_installs_here() {
    let mut h = Harness::project();
    let remote = host_home(crate::hosts::install::VERSION);
    h.with_host(crate::hosts::runner::local_runners(&[("box", remote.path())]));
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();
    h.click("browse");
    let here = if cfg!(target_os = "macos") { "this Mac" } else { "this computer" };
    assert!(h.texts().contains(&format!("skills.sh installs go to {here}; copy them to box afterwards")), "{}", h.texts());
    assert_eq!(h.rt.state.local_scope(), &Scope::Project(h.project_dir()), "installs go to this machine's scope");
}

#[test]
fn the_mcp_tab_keeps_the_hosts_servers() {
    let mut h = Harness::project();
    let remote = host_home(crate::hosts::install::VERSION);
    fs::write(remote.path().join(".claude.json"), r#"{"mcpServers":{"far-mcp":{"command":"far"}}}"#).unwrap();
    h.with_host(crate::hosts::runner::local_runners(&[("box", remote.path())]));
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();
    assert!(h.rt.state.mcp.get("far-mcp").is_some());
    // The host's files aren't on this machine: a scan here with its paths would find nothing.
    fs::remove_file(remote.path().join(".claude.json")).unwrap();

    h.click("tab-mcp");
    assert!(h.rt.state.mcp.get("far-mcp").is_some(), "this machine's disk wasn't scanned with the host's paths");
    assert!(h.rt.find("mcp-row-far-mcp").is_some());
}

#[test]
fn an_install_on_one_host_never_reports_on_another() {
    let mut h = Harness::project();
    let b = host_home(crate::hosts::install::VERSION);
    let b_home = b.path().to_path_buf();
    h.rt.update(move |s, _| {
        s.settings.hosts.push(crate::hosts::Host { name: "a".into(), ssh: "me@a".into() });
        s.settings.hosts.push(crate::hosts::Host { name: "b".into(), ssh: "me@b".into() });
        let local = crate::hosts::runner::local_runners(&[("b", &b_home)]);
        s.make_runner = std::sync::Arc::new(move |host: &crate::hosts::Host| {
            if host.name == "a" { Box::new(Down) as Box<dyn crate::hosts::runner::Runner> } else { local(host) }
        });
    });
    h.rt.update(|s, _| s.switch_env(Some("a")));
    h.land_remote();
    h.rt.update(|s, _| s.install_prism());
    assert!(h.rt.state.installing_prism);

    h.rt.update(|s, _| s.switch_env(Some("b")));
    h.settle();
    assert!(!h.rt.state.installing_prism);
    assert!(!h.texts().contains("Installing prism"), "b isn't being installed");
    h.land_remote();
    assert_eq!(h.remote_state(), Some(crate::app::RemoteState::Ready));
    assert!(h.rt.state.notice.is_none(), "no toast for b");

    // An answer from a, landing late, is not b's.
    h.rt.update(|s, _| {
        let result = Ok(Box::new(crate::wire::state(&Scope::Global, &s.paths, &s.settings)));
        s.land_remote(crate::app::RemoteAnswer { host: "a".into(), result, installed: true, install_error: None });
    });
    assert!(h.rt.state.notice.is_none());
    assert_eq!(h.rt.state.host().map(|x| x.name.as_str()), Some("b"));
}

/// A host that answers slowly, then can't be reached.
struct Slow;

impl crate::hosts::runner::Runner for Slow {
    fn prism(&self, _: &[String], _: Option<Vec<u8>>) -> Result<crate::hosts::runner::Output, String> {
        std::thread::sleep(std::time::Duration::from_millis(300));
        Err("Can't reach box: timed out".into())
    }
    fn shell(&self, _: &str, _: Option<Vec<u8>>) -> Result<crate::hosts::runner::Output, String> {
        std::thread::sleep(std::time::Duration::from_millis(300));
        Err("Can't reach box: timed out".into())
    }
}

#[test]
fn a_reload_during_an_install_never_strands_it() {
    let mut h = Harness::project();
    h.with_host(std::sync::Arc::new(|_: &crate::hosts::Host| Box::new(Slow) as Box<dyn crate::hosts::runner::Runner>));
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();
    h.rt.update(|s, _| s.install_prism());
    assert!(h.rt.state.installing_prism);

    // A scope click and ⌘R while it runs: the install's answer is still the one that lands.
    h.click("scope-global");
    h.rt.update(|s, _| s.rescan());
    h.land_remote();
    assert!(!h.rt.state.installing_prism, "the flag clears when the answer lands");
    assert!(!h.texts().contains("Installing prism"));
    assert!(matches!(h.remote_state(), Some(crate::app::RemoteState::Unreachable(_))));

    // Install works again; any answer for the host in view clears it.
    h.rt.update(|s, _| s.install_prism());
    assert!(h.rt.state.installing_prism);
    h.rt.update(|s, _| {
        s.remote_load.cancel();
        let result = Err(crate::app::RemoteState::Unreachable("Can't reach box: timed out".into()));
        s.land_remote(crate::app::RemoteAnswer { host: "box".into(), result, installed: false, install_error: None });
    });
    assert!(!h.rt.state.installing_prism);
}

// Acting on a host.

/// Each command line a host ran, with what went on its stdin.
type Log = std::sync::Arc<std::sync::Mutex<Vec<(Vec<String>, Option<Vec<u8>>)>>>;

/// A host's runner that records each command line and what went on its stdin.
struct Recorded {
    inner: Box<dyn crate::hosts::runner::Runner>,
    log: Log,
}

impl crate::hosts::runner::Runner for Recorded {
    fn prism(&self, args: &[String], stdin: Option<Vec<u8>>) -> Result<crate::hosts::runner::Output, String> {
        self.log.lock().unwrap().push((args.to_vec(), stdin.clone()));
        self.inner.prism(args, stdin)
    }
    fn shell(&self, script: &str, stdin: Option<Vec<u8>>) -> Result<crate::hosts::runner::Output, String> {
        self.log.lock().unwrap().push((vec![script.to_string()], stdin.clone()));
        self.inner.shell(script, stdin)
    }
}

/// A host `box` whose `far` lives in `.agents` only, in view and loaded.
fn on_host() -> (Harness, tempfile::TempDir) {
    let mut h = Harness::project();
    let remote = host_home(crate::hosts::install::VERSION);
    skill(&remote.path().join(".agents/skills"), "far");
    h.with_host(crate::hosts::runner::local_runners(&[("box", remote.path())]));
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();
    (h, remote)
}

#[test]
fn remote_link_toggle_writes_on_host() {
    let (mut h, remote) = on_host();
    h.click("skill-far");
    h.land_remote();
    h.click("link-claude");
    h.land_remote();
    let link = remote.path().join(".claude/skills/far");
    assert!(fs::symlink_metadata(&link).is_ok_and(|m| m.file_type().is_symlink()), "the host has the link");
    assert_eq!(h.placement("far", "claude"), Placement::Linked, "the host's state was read again");
    let notice = h.rt.state.notice.as_ref().expect("a toast");
    assert!(notice.title.ends_with(" on box") && !notice.error, "{}", notice.title);
    // This machine is untouched.
    assert!(fs::symlink_metadata(h.root.join(".claude/skills/far")).is_err());
    assert_eq!(crate::store::load_last_change(&Paths::isolated(&h.root)), None);
}

#[test]
fn remote_mcp_edit_secret_goes_by_stdin() {
    let mut h = Harness::project();
    let remote = host_home(crate::hosts::install::VERSION);
    let config = remote.path().join(".claude.json");
    fs::write(&config, r#"{"mcpServers":{"far-mcp":{"command":"far","env":{"TOKEN":"FIXTURE_OLD_SECRET","OTHER":"x"}}}}"#).unwrap();
    let log: Log = std::sync::Arc::default();
    let (home, record) = (remote.path().to_path_buf(), log.clone());
    h.with_host(std::sync::Arc::new(move |host: &crate::hosts::Host| {
        let inner = crate::hosts::runner::local_runners(&[("box", &home)])(host);
        Box::new(Recorded { inner, log: record.clone() }) as Box<dyn crate::hosts::runner::Runner>
    }));
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();
    h.click("tab-mcp");
    h.click("mcp-row-far-mcp");
    h.click("mcp-edit");
    h.land_remote();
    let form = h.rt.state.mcp_form.as_ref().expect("the Edit form opens with the host's values");
    let value = |k: &str| form.env.iter().find(|p| p.key.text == k).map(|p| p.value.text.clone());
    assert_eq!(value("TOKEN").as_deref(), Some("FIXTURE_OLD_SECRET"), "unmasked, from wire server");

    h.fill_form(|f| {
        for p in &mut f.env {
            p.value = rune::TextEdit::new(if p.key.text == "TOKEN" { "FIXTURE_NEW_SECRET" } else { "FIXTURE_NEW_OTHER" });
        }
    });
    h.click("mcp-add-save");
    h.land_remote();
    assert!(h.rt.state.mcp_form.is_none(), "the form and its values are gone");
    assert!(h.texts().contains("on box"), "the plan names the host: {}", h.texts());
    h.click("mcp-apply");
    h.land_remote();

    let written = json(&config);
    assert_eq!(written["mcpServers"]["far-mcp"]["env"]["TOKEN"], "FIXTURE_NEW_SECRET");
    assert_eq!(written["mcpServers"]["far-mcp"]["env"]["OTHER"], "FIXTURE_NEW_OTHER");
    let notice = h.rt.state.notice.as_ref().expect("a toast");
    assert_eq!(notice.title, "Edited far-mcp on box");
    let log = log.lock().unwrap();
    assert!(log.iter().any(|(_, stdin)| stdin.as_deref().is_some_and(|d| String::from_utf8_lossy(d).contains("FIXTURE_NEW_SECRET"))), "the values went on stdin");
    for (argv, _) in log.iter() {
        let line = argv.join(" ");
        assert!(!line.contains("FIXTURE_NEW") && !line.contains("FIXTURE_OLD"), "a value in argv: {line}");
    }
}

#[test]
fn remote_undo_acts_on_host() {
    let mut h = Harness::project();
    // A change on this machine first.
    h.click("skill-review");
    h.click("link-codex");
    let here = Paths::isolated(&h.root);
    let mine = crate::store::load_last_change(&here).expect("this machine's last change").label;
    let remote = host_home(crate::hosts::install::VERSION);
    skill(&remote.path().join(".agents/skills"), "far");
    h.with_host(crate::hosts::runner::local_runners(&[("box", remote.path())]));
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();
    h.click("skill-far");
    h.land_remote();
    h.click("link-claude");
    h.land_remote();
    let link = remote.path().join(".claude/skills/far");
    assert!(fs::symlink_metadata(&link).is_ok());

    key(&mut h, rune::Key::Command('z'), rune::Mods { cmd: true, ..rune::Mods::default() });
    h.land_remote();
    assert!(fs::symlink_metadata(&link).is_err(), "the host's change is undone");
    let notice = h.rt.state.notice.as_ref().expect("a toast");
    assert!(notice.title.starts_with("Undid: ") && notice.title.ends_with(" on box"), "{}", notice.title);
    assert_eq!(crate::store::load_last_change(&here).map(|c| c.label), Some(mine), "this machine's last change stays");
    assert!(fs::symlink_metadata(h.project_dir().join(".codex/skills/review")).is_ok(), "and so does its link");
}

#[test]
fn remote_add_project_takes_typed_path() {
    let (mut h, remote) = on_host();
    let site = fs::canonicalize(remote.path()).unwrap().join("work/site");
    skill(&site.join(".claude/skills"), "page");
    h.click("add-project");
    assert!(h.rt.find("host-project-path").is_some(), "a field for a path, not a folder picker");
    h.click("host-project-path");
    h.rt.key(rune::KeyEvent { key: rune::Key::Text(site.display().to_string()), mods: rune::Mods::default() });
    h.settle();
    h.click("host-project-add");
    h.land_remote();
    assert!(h.rt.find("host-project-path").is_none(), "the dialog closed");
    let saved = Settings::load_from(&Paths::isolated(remote.path()));
    assert_eq!(saved.projects, vec![site.clone()], "the host lists it");
    assert!(Settings::load_from(&Paths::isolated(&h.root)).projects.iter().all(|p| *p != site), "this machine doesn't");
    assert_eq!(h.rt.state.scope, Scope::Project(site));
    assert!(h.rt.state.inv.get("page").is_some(), "the project's skills, from the host");
}

#[test]
fn remote_sync_shows_a_changed_plan_and_applies_the_new_one() {
    let mut h = Harness::project();
    let remote = host_home(crate::hosts::install::VERSION);
    skill(&remote.path().join(".claude/skills"), "far");
    fs::create_dir_all(remote.path().join(".codex")).unwrap();
    h.with_host(crate::hosts::runner::local_runners(&[("box", remote.path())]));
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();

    h.click("sync");
    assert!(h.texts().contains("Asking box for the plan…"), "{}", h.texts());
    h.land_remote();
    let lines = h.rt.state.sync_host.as_ref().and_then(|p| p.view.clone()).expect("the host's plan").unwrap().lines;
    assert!(!lines.is_empty() && lines.iter().all(|l| l.ends_with(" on box")), "{lines:?}");
    assert!(h.rt.find("sync-line-0").is_some());

    // The skill changes on the host before Apply: its plan id no longer matches.
    fs::write(remote.path().join(".claude/skills/far/extra.md"), "new").unwrap();
    h.click("sync-apply");
    h.land_remote();
    let plan = h.rt.state.sync_host.as_ref().expect("the dialog stays, with the new plan");
    assert_eq!(plan.notice.as_deref(), Some("The files changed on box; here is the new plan"));
    assert!(!plan.applying && matches!(plan.view, Some(Ok(_))));
    assert!(!remote.path().join(".agents/skills/far").exists(), "nothing was done");

    h.click("sync-apply");
    h.land_remote();
    assert!(h.rt.state.sync_host.is_none());
    assert!(remote.path().join(".agents/skills/far/extra.md").is_file());
    assert_eq!(h.placement("far", "codex"), Placement::Linked);
    let notice = h.rt.state.notice.as_ref().expect("a toast");
    assert!(notice.title.starts_with("Synced: ") && notice.title.ends_with(" on box") && notice.undo, "{}", notice.title);
}

#[test]
fn remote_show_values_fetches_the_server_and_drops_it_when_hidden() {
    let mut h = Harness::project();
    let remote = host_home(crate::hosts::install::VERSION);
    fs::write(remote.path().join(".claude.json"), r#"{"mcpServers":{"far-mcp":{"command":"far","env":{"TOKEN":"FIXTURE_HOST_SECRET"}}}}"#).unwrap();
    h.with_host(crate::hosts::runner::local_runners(&[("box", remote.path())]));
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();
    h.click("tab-mcp");
    h.click("mcp-row-far-mcp");
    assert!(!h.texts().contains("FIXTURE_HOST_SECRET"), "masked until shown");
    h.click("mcp-reveal");
    h.land_remote();
    assert!(h.texts().contains("FIXTURE_HOST_SECRET"), "{}", h.texts());
    h.click("mcp-reveal");
    assert!(h.rt.state.host_values.is_empty(), "dropped when hidden");
    assert!(!h.texts().contains("FIXTURE_HOST_SECRET"));
}

#[test]
fn remote_mcp_add_writes_on_host() {
    let mut h = Harness::project();
    let remote = host_home(crate::hosts::install::VERSION);
    h.with_host(crate::hosts::runner::local_runners(&[("box", remote.path())]));
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();
    h.click("tab-mcp");
    h.click("scope-global");
    h.land_remote();
    h.click("mcp-add");
    assert_eq!(h.rt.state.mcp_form.as_ref().unwrap().agents, [crate::mcp::agents::index_of("claude").unwrap()], "the host's agents");
    h.fill_form(|f| {
        f.name = rune::TextEdit::new("fresh");
        f.command = rune::TextEdit::new("npx -y fresh-mcp");
    });
    h.click("mcp-add-env-add");
    h.fill_form(|f| {
        f.env[0].key = rune::TextEdit::new("FRESH_TOKEN");
        f.env[0].value = rune::TextEdit::new("${FRESH_TOKEN}").secure();
    });
    h.click("mcp-add-save");
    h.land_remote();
    assert!(h.rt.state.mcp_form.is_none(), "{:?}", h.rt.state.mcp_form.as_ref().and_then(|f| f.error.clone()));
    let fresh = json(&remote.path().join(".claude.json"))["mcpServers"]["fresh"].clone();
    assert_eq!(fresh["args"], serde_json::json!(["-y", "fresh-mcp"]));
    assert_eq!(fresh["env"]["FRESH_TOKEN"], "${FRESH_TOKEN}", "a typed ${{VAR}} stays a reference through stdin");
    assert!(h.rt.state.mcp.get("fresh").is_some() && h.rt.state.mcp_selected.as_deref() == Some("fresh"));
    assert_eq!(h.rt.state.notice.as_ref().unwrap().title, "Added fresh on box");
    assert!(!h.root.join(".claude.json").exists(), "nothing here");
}

impl Harness {
    /// Host `box` at `home`, in view and loaded, its command lines recorded.
    fn recorded_host(&mut self, home: &Path) -> Log {
        let log: Log = std::sync::Arc::default();
        let (home, record) = (home.to_path_buf(), log.clone());
        self.with_host(std::sync::Arc::new(move |host: &crate::hosts::Host| {
            let inner = crate::hosts::runner::local_runners(&[("box", &home)])(host);
            Box::new(Recorded { inner, log: record.clone() }) as Box<dyn crate::hosts::runner::Runner>
        }));
        self.rt.update(|s, _| s.switch_env(Some("box")));
        self.land_remote();
        log
    }
}

/// No command line the host ran holds `fixture`.
fn argv_free_of(log: &Log, fixture: &str) {
    for (argv, _) in log.lock().unwrap().iter() {
        let line = argv.join(" ");
        assert!(!line.contains(fixture), "a value in argv: {line}");
    }
}

#[test]
fn remote_mcp_tokens_in_urls_and_args_go_by_stdin() {
    let mut h = Harness::project();
    let remote = host_home(crate::hosts::install::VERSION);
    let log = h.recorded_host(remote.path());
    h.click("tab-mcp");

    // Add: a token in an arg, and one in a URL.
    h.click("mcp-add");
    h.fill_form(|f| {
        f.name = rune::TextEdit::new("cmd");
        f.command = rune::TextEdit::new("npx -y cmd-mcp --api-key=FIXTURE_ARG_TOKEN");
    });
    h.click("mcp-add-save");
    h.land_remote();
    h.click("mcp-add");
    h.fill_form(|f| {
        f.name = rune::TextEdit::new("web");
        f.http = true;
        f.url = rune::TextEdit::new("https://mcp.example.com/mcp?key=FIXTURE_URL_TOKEN");
    });
    h.click("mcp-add-save");
    h.land_remote();
    let config = remote.path().join(".claude.json");
    assert_eq!(json(&config)["mcpServers"]["cmd"]["args"][2], "--api-key=FIXTURE_ARG_TOKEN");
    assert_eq!(json(&config)["mcpServers"]["web"]["url"], "https://mcp.example.com/mcp?key=FIXTURE_URL_TOKEN");

    // Edit both: the new tokens go the same way.
    for (row, edit) in [("cmd", "npx -y cmd-mcp --api-key=FIXTURE_ARG_NEW"), ("web", "https://mcp.example.com/mcp?key=FIXTURE_URL_NEW")] {
        h.click(&format!("mcp-row-{row}"));
        h.click("mcp-edit");
        h.land_remote();
        let text = edit.to_string();
        h.fill_form(move |f| if f.http { f.url = rune::TextEdit::new(text) } else { f.command = rune::TextEdit::new(text) });
        h.click("mcp-add-save");
        h.land_remote();
        h.click("mcp-apply");
        h.land_remote();
    }
    assert_eq!(json(&config)["mcpServers"]["cmd"]["args"][2], "--api-key=FIXTURE_ARG_NEW");
    assert_eq!(json(&config)["mcpServers"]["web"]["url"], "https://mcp.example.com/mcp?key=FIXTURE_URL_NEW");
    argv_free_of(&log, "FIXTURE");
    let stdin = log.lock().unwrap().iter().filter_map(|(_, d)| d.as_ref().map(|d| String::from_utf8_lossy(d).into_owned())).collect::<Vec<_>>().join("\n");
    assert!(["FIXTURE_ARG_TOKEN", "FIXTURE_URL_TOKEN", "FIXTURE_ARG_NEW", "FIXTURE_URL_NEW"].iter().all(|t| stdin.contains(t)), "every value went on stdin");
}

#[test]
fn remote_show_values_and_edit_both_read_the_server() {
    let mut h = Harness::project();
    let remote = host_home(crate::hosts::install::VERSION);
    fs::write(remote.path().join(".claude.json"), r#"{"mcpServers":{"far-mcp":{"command":"far","env":{"TOKEN":"FIXTURE_HOST_SECRET"}}}}"#).unwrap();
    h.with_host(crate::hosts::runner::local_runners(&[("box", remote.path())]));
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();
    h.click("tab-mcp");
    h.click("mcp-row-far-mcp");
    // The same `wire server` twice in a row, for two different uses: neither is dropped.
    h.rt.update(|s, _| {
        s.toggle_reveal("far-mcp");
        s.open_mcp_edit("far-mcp");
    });
    h.land_remote();
    assert!(h.rt.state.host_values.contains_key("far-mcp"), "Show values got the server");
    assert!(h.rt.state.mcp_form.is_some(), "and so did Edit…");
}

/// A host that is a folder here, answering after a pause.
struct Late(Box<dyn crate::hosts::runner::Runner>);

impl crate::hosts::runner::Runner for Late {
    fn prism(&self, args: &[String], stdin: Option<Vec<u8>>) -> Result<crate::hosts::runner::Output, String> {
        std::thread::sleep(std::time::Duration::from_millis(150));
        self.0.prism(args, stdin)
    }
    fn shell(&self, script: &str, stdin: Option<Vec<u8>>) -> Result<crate::hosts::runner::Output, String> {
        self.0.shell(script, stdin)
    }
}

#[test]
fn a_plan_in_flight_is_dropped_when_the_machine_changes() {
    let (mut h, remote) = on_host();
    let home = remote.path().to_path_buf();
    h.rt.update(move |s, _| {
        s.make_runner = std::sync::Arc::new(move |host: &crate::hosts::Host| {
            Box::new(Late(crate::hosts::runner::local_runners(&[("box", &home)])(host))) as Box<dyn crate::hosts::runner::Runner>
        });
    });
    h.click("sync");
    assert!(h.rt.state.host_act.is_pending(), "box is planning the sync");
    h.rt.update(|s, _| s.switch_env(None));
    h.land_remote();
    assert!(!h.rt.state.is_remote() && !h.rt.state.host_busy());
    assert!(h.rt.state.notice.is_none() && h.rt.state.sync_host.is_none(), "nothing about box's plan on this machine");
    assert!(h.rt.state.inv.get("review").is_some() && h.rt.state.inv.get("far").is_none(), "this machine's view stays");
}

#[test]
fn a_write_in_flight_lands_after_switching_away() {
    let (mut h, remote) = on_host();
    let home = remote.path().to_path_buf();
    h.rt.update(move |s, _| {
        s.make_runner = std::sync::Arc::new(move |host: &crate::hosts::Host| {
            Box::new(SlowApply(crate::hosts::runner::local_runners(&[("box", &home)])(host))) as Box<dyn crate::hosts::runner::Runner>
        });
    });
    let (claude, codex) = (index_of("claude").unwrap(), index_of("codex").unwrap());
    h.rt.update(move |s, _| s.toggle_link("far", claude));
    assert!(h.rt.state.host_act.is_pending(), "the link is in flight");
    h.rt.update(|s, _| s.switch_env(None));
    assert!(h.rt.state.host_busy(), "switching away leaves it running on box");

    // It lands while this machine is in view: its toast names box, and its Undo acts there.
    h.land_remote();
    let link = remote.path().join(".claude/skills/far");
    assert!(fs::symlink_metadata(&link).is_ok(), "box has the link");
    assert!(!h.rt.state.is_remote() && h.rt.state.inv.get("review").is_some() && h.rt.state.inv.get("far").is_none(), "this machine's view stays");
    let notice = h.rt.state.notice.as_ref().expect("a toast about box");
    assert!(notice.title.ends_with(" on box") && !notice.error, "{}", notice.title);
    assert!(notice.undo && notice.machine.as_deref() == Some("box"));
    h.click_button("Undo");
    h.land_remote();
    assert!(fs::symlink_metadata(&link).is_err(), "undone on box");
    let title = &h.rt.state.notice.as_ref().unwrap().title;
    assert!(title.starts_with("Undid: ") && title.ends_with(" on box"), "{title}");

    // Back on box, a change asked while the one from before still runs waits for it.
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();
    h.rt.update(move |s, _| s.toggle_link("far", claude));
    h.rt.update(|s, _| {
        s.switch_env(None);
        s.switch_env(Some("box"));
    });
    let start = std::time::Instant::now();
    while h.remote_state() != Some(crate::app::RemoteState::Ready) {
        assert!(start.elapsed().as_secs() < 10, "box never loaded");
        std::thread::sleep(std::time::Duration::from_millis(10));
        h.rt.update(|s, _| {
            s.poll();
        });
    }
    assert!(h.rt.state.host_act.is_pending(), "the first link still runs");
    h.rt.update(move |s, _| s.toggle_link("far", codex));
    assert_eq!(h.rt.state.host_jobs.len(), 1, "the second waits its turn");
    h.land_remote();
    assert!(fs::symlink_metadata(&link).is_ok() && fs::symlink_metadata(remote.path().join(".codex/skills/far")).is_ok(), "both landed on box");
}

#[test]
fn a_host_toast_undo_leaves_a_later_change_there() {
    let (mut h, remote) = on_host();
    h.click("skill-far");
    h.land_remote();
    h.click("link-claude");
    h.land_remote();
    let link = remote.path().join(".claude/skills/far");
    assert!(fs::symlink_metadata(&link).is_ok());
    let there = Paths::isolated(remote.path());
    let mine = crate::store::load_last_change(&there).expect("the link is box's last change").label;
    assert_eq!(h.rt.state.notice.as_ref().unwrap().label.as_deref(), Some(mine.as_str()), "the toast knows its change");
    crate::store::save_last_change(&there, Some(&crate::store::LastChange { label: "sync (1 change)".into(), journal: crate::fsops::Journal::default() }));

    h.click_button("Undo");
    h.land_remote();
    let notice = h.rt.state.notice.as_ref().expect("a toast");
    assert_eq!(notice.title, "Couldn't undo on box");
    assert!(notice.detail.starts_with(&format!("{mine} is no longer the last change")), "{}", notice.detail);
    assert!(fs::symlink_metadata(&link).is_ok(), "the link stays");
}

#[test]
fn remote_actions_in_a_host_project_name_it() {
    let mut h = Harness::project();
    let remote = host_home(crate::hosts::install::VERSION);
    let site = fs::canonicalize(remote.path()).unwrap().join("work/site");
    skill(&site.join(".agents/skills"), "page");
    let log = h.recorded_host(remote.path());
    let dir = site.clone();
    h.rt.update(move |s, _| s.add_project(dir));
    h.land_remote();
    assert_eq!(h.rt.state.scope, Scope::Project(site.clone()));
    h.click("skill-page");
    h.land_remote();
    h.click("link-claude");
    h.land_remote();
    assert!(fs::symlink_metadata(site.join(".claude/skills/page")).is_ok_and(|m| m.file_type().is_symlink()), "linked in the host's project");
    assert!(fs::symlink_metadata(remote.path().join(".claude/skills/page")).is_err(), "not globally");
    let project = format!("--project={}", site.display());
    assert!(log.lock().unwrap().iter().any(|(argv, _)| argv.first().is_some_and(|w| w == "skills") && argv.contains(&project)), "the command names the project");
}

// Copying to another machine.

impl Harness {
    /// Saves host `laptop` (in memory) at `home`; "this" is this window's home.
    fn with_laptop(&mut self, home: &Path) {
        let runners = crate::hosts::runner::local_runners(&[("this", &self.root.clone()), ("laptop", home)]);
        self.rt.update(move |s, _| {
            s.settings.hosts.push(crate::hosts::Host { name: "laptop".into(), ssh: "me@laptop".into() });
            s.make_runner = runners;
        });
        self.settle();
    }

    /// Copies the selected `review` to laptop's Global through the dialog: pick, plan, apply.
    fn copy_review_to_laptop(&mut self) {
        self.click("skill-review");
        self.click("copy");
        self.land_remote();
        self.click("copy-to-laptop-0");
        self.land_remote();
        self.click("copy-apply");
        self.land_remote();
    }
}

#[test]
fn copy_skill_to_host_from_window() {
    let mut h = Harness::project();
    let laptop = host_home(crate::hosts::install::VERSION);
    h.with_laptop(laptop.path());
    h.click("skill-review");
    h.click("copy");
    h.land_remote();
    assert!(h.rt.find("copy-to-0").is_some(), "this machine's other scopes come first");
    assert!(h.rt.find("copy-to-laptop-0").is_some(), "the host's Global is listed");

    h.click("copy-to-laptop-0");
    h.land_remote();
    let this = crate::hosts::copy::this_machine();
    let line = format!("Copy review from {this} to Global (~) (link into Claude Code) on laptop");
    assert!(h.texts().contains(&line), "the host's plan: {}", h.texts());
    let got = laptop.path().join(".agents/skills/review");
    assert!(!got.exists(), "a plan changes nothing");

    h.click("copy-apply");
    h.land_remote();
    assert!(got.join("SKILL.md").is_file(), "laptop has it");
    assert!(fs::symlink_metadata(laptop.path().join(".claude/skills/review")).is_ok_and(|m| m.file_type().is_symlink()), "linked there");
    assert!(h.rt.state.copy_dialog.is_none(), "the dialog closed");
    let notice = h.rt.state.notice.as_ref().expect("a toast");
    assert_eq!(notice.title, "Copied review to Global (~) on laptop");
    assert!(notice.undo && !notice.error);
    assert!(!h.root.join(".agents/skills/review").exists(), "nothing changed here");
}

#[test]
fn toast_undo_targets_named_machine() {
    let mut h = Harness::project();
    // A change on this machine first.
    h.click("skill-review");
    h.click("link-codex");
    let here = Paths::isolated(&h.root);
    let mine = crate::store::load_last_change(&here).expect("this machine's last change").label;
    let laptop = host_home(crate::hosts::install::VERSION);
    h.with_laptop(laptop.path());
    h.copy_review_to_laptop();
    let got = laptop.path().join(".agents/skills/review");
    assert!(got.is_dir());

    h.click_button("Undo");
    h.land_remote();
    assert!(!got.exists() && fs::symlink_metadata(laptop.path().join(".claude/skills/review")).is_err(), "the copy is taken back on laptop");
    let notice = h.rt.state.notice.as_ref().expect("a toast");
    assert!(notice.title.starts_with("Undid: ") && notice.title.ends_with(" on laptop"), "{}", notice.title);
    assert_eq!(crate::store::load_last_change(&here).map(|c| c.label), Some(mine), "this machine's last change stays");
    let codex = h.project_dir().join(".codex/skills/review");
    assert!(fs::symlink_metadata(&codex).is_ok(), "and so does its link");

    // ⌘Z still means the machine in view.
    key(&mut h, rune::Key::Command('z'), rune::Mods { cmd: true, ..rune::Mods::default() });
    assert!(fs::symlink_metadata(&codex).is_err(), "this machine's change is undone");
}

#[test]
fn a_copy_that_stops_after_placing_offers_undo_there() {
    let mut h = Harness::project();
    let laptop = host_home(crate::hosts::install::VERSION);
    // Claude Code there can't take the link: its skills folder is a file.
    fs::write(laptop.path().join(".claude/skills"), "not a folder").unwrap();
    h.with_laptop(laptop.path());
    h.copy_review_to_laptop();
    let got = laptop.path().join(".agents/skills/review");
    assert!(got.join("SKILL.md").is_file(), "placed before the link failed");
    let notice = h.rt.state.notice.as_ref().expect("a toast");
    assert_eq!(notice.title, "Couldn't finish that on laptop");
    assert!(notice.error && notice.detail.ends_with("Undo takes back what was done."), "{}", notice.detail);
    assert!(notice.undo && notice.machine.as_deref() == Some("laptop"), "Undo acts on laptop");

    h.click_button("Undo");
    h.land_remote();
    assert!(!got.exists(), "the placed skill is taken back on laptop");
    assert!(h.rt.state.notice.as_ref().unwrap().title.starts_with("Undid: "), "{}", h.rt.state.notice.as_ref().unwrap().title);
}

#[test]
fn a_toast_undo_here_leaves_a_later_change() {
    let mut h = Harness::project();
    h.click("skill-review");
    h.click("link-codex");
    let codex = h.project_dir().join(".codex/skills/review");
    assert!(fs::symlink_metadata(&codex).is_ok());
    let here = Paths::isolated(&h.root);
    let later = crate::store::LastChange { label: "sync (1 change)".into(), journal: crate::fsops::Journal::default() };
    crate::store::save_last_change(&here, Some(&later));

    h.click_button("Undo");
    let notice = h.rt.state.notice.as_ref().expect("a toast");
    assert_eq!(notice.title, "Couldn't undo");
    assert!(notice.detail.ends_with("is no longer the last change; nothing was undone"), "{}", notice.detail);
    assert!(fs::symlink_metadata(&codex).is_ok(), "the link stays");
    assert_eq!(crate::store::load_last_change(&here), Some(later), "and so does the later change");
}

#[test]
fn a_toast_undo_leaves_a_later_change_there() {
    let mut h = Harness::project();
    let laptop = host_home(crate::hosts::install::VERSION);
    h.with_laptop(laptop.path());
    h.copy_review_to_laptop();
    let got = laptop.path().join(".agents/skills/review");
    assert!(got.is_dir());
    // Another change on laptop since, from its own CLI.
    let there = Paths::isolated(laptop.path());
    crate::store::save_last_change(&there, Some(&crate::store::LastChange { label: "sync (1 change)".into(), journal: crate::fsops::Journal::default() }));

    h.click_button("Undo");
    h.land_remote();
    let notice = h.rt.state.notice.as_ref().expect("a toast");
    assert_eq!(notice.title, "Couldn't undo on laptop");
    let this = crate::hosts::copy::this_machine();
    assert_eq!(notice.detail, format!("copy review from {this} is no longer the last change; nothing was undone"));
    assert!(got.is_dir(), "the copy stays");
    assert_eq!(crate::store::load_last_change(&there).map(|c| c.label).as_deref(), Some("sync (1 change)"), "and so does the later change");
}

#[test]
fn mcp_copy_to_host_shows_warnings() {
    let mut h = mcp_home();
    let (claude, cursor) = (h.root.join(".claude.json"), h.root.join(".cursor/mcp.json"));
    let before = (read(&claude), read(&cursor));
    let laptop = host_home(crate::hosts::install::VERSION);
    h.with_laptop(laptop.path());
    h.mcp_global();
    h.click("mcp-row-plata");
    h.click("mcp-copy");
    h.land_remote();
    assert!(h.rt.find("mcp-copy-machine-trigger").is_some(), "a Machine select");

    h.rt.update(|s, _| s.set_mcp_copy_machine("laptop"));
    h.settle();
    h.land_remote();
    assert!(h.rt.find("mcp-copy-to-claude").is_some(), "laptop's agents");
    assert!(h.rt.find("mcp-copy-to-cursor").is_none(), "Cursor isn't on laptop");
    let texts = h.texts();
    assert!(texts.contains("Add plata to Claude Code on laptop"), "{texts}");
    assert!(texts.contains("Heads-up") && texts.contains("env.PLATA_HOST needs ${PLATA_HOST} set on laptop"), "{texts}");
    assert!(!texts.contains("fake-env-secret"), "values stay masked");

    h.click("mcp-apply");
    h.land_remote();
    assert!(h.rt.state.mcp_dialog.is_none(), "the dialog closed");
    let written = json(&laptop.path().join(".claude.json"));
    assert_eq!(written["mcpServers"]["plata"]["env"]["PLATA_TOKEN"], "fake-env-secret", "the value as written travels");
    let notice = h.rt.state.notice.as_ref().expect("a toast");
    assert_eq!(notice.title, "Copied plata to Global (~) on laptop");
    assert!(notice.undo);
    assert_eq!((read(&claude), read(&cursor)), before, "nothing changed here");
}

#[test]
fn copy_from_a_host_to_this_machine() {
    let mut h = Harness::project();
    let remote = host_home(crate::hosts::install::VERSION);
    skill(&remote.path().join(".agents/skills"), "far");
    let root = h.root.clone();
    h.with_host(crate::hosts::runner::local_runners(&[("box", remote.path()), ("this", &root)]));
    h.rt.update(|s, _| s.switch_env(Some("box")));
    h.land_remote();
    h.click("skill-far");
    h.click("copy");
    h.land_remote();
    assert!(h.rt.find("copy-to-this-1").is_some(), "this machine's project, from what the window keeps of it");
    h.click("copy-to-this-0");
    h.land_remote();
    h.click("copy-apply");
    h.land_remote();
    assert!(h.root.join(".agents/skills/far/SKILL.md").is_file(), "this machine has it");
    let notice = h.rt.state.notice.as_ref().expect("a toast");
    let this = crate::hosts::copy::this_machine();
    assert_eq!(notice.title, format!("Copied far to Global (~) on {this}"));
    assert!(h.rt.state.is_remote(), "box stays in view");
    let local = h.rt.state.local.as_ref().unwrap();
    assert_eq!(local.stats[&Scope::Global].total, 1, "this machine's counts are taken again");
    assert!(local.global_names.contains("far"), "so the next Copy to… says Global has it");

    h.click_button("Undo");
    h.land_remote();
    assert!(!h.root.join(".agents/skills/far").exists(), "the toast's Undo acted here, not on box");
    assert!(remote.path().join(".agents/skills/far").is_dir());
}

/// A host that is a folder here, slow to apply.
struct SlowApply(Box<dyn crate::hosts::runner::Runner>);

impl crate::hosts::runner::Runner for SlowApply {
    fn prism(&self, args: &[String], stdin: Option<Vec<u8>>) -> Result<crate::hosts::runner::Output, String> {
        if args.iter().any(|a| a == "--apply") {
            std::thread::sleep(std::time::Duration::from_millis(800));
        }
        self.0.prism(args, stdin)
    }
    fn shell(&self, script: &str, stdin: Option<Vec<u8>>) -> Result<crate::hosts::runner::Output, String> {
        self.0.shell(script, stdin)
    }
}

impl Harness {
    /// "this" is this window's home; laptop is `home`, its runner wrapped by `wrap`.
    fn with_laptop_as(&mut self, home: &Path, wrap: impl Fn(Box<dyn crate::hosts::runner::Runner>) -> Box<dyn crate::hosts::runner::Runner> + Send + Sync + 'static) {
        let (root, home) = (self.root.clone(), home.to_path_buf());
        let runners = std::sync::Arc::new(move |h: &crate::hosts::Host| {
            let local = crate::hosts::runner::local_runners(&[("this", &root), ("laptop", &home)])(h);
            if h.name == "laptop" { wrap(local) } else { local }
        });
        self.rt.update(move |s, _| {
            s.settings.hosts.push(crate::hosts::Host { name: "laptop".into(), ssh: "me@laptop".into() });
            s.make_runner = runners;
        });
        self.settle();
    }

    /// Lands a dialog's scopes or plan, leaving any write in flight running.
    fn land_asks(&mut self) {
        let start = std::time::Instant::now();
        while self.rt.state.across_asking() {
            assert!(start.elapsed().as_secs() < 10, "the machine never answered");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        self.rt.update(|s, _| {
            s.poll();
        });
        self.settle();
    }

    /// Lands every write, one poll at a time: each toast that showed, in order.
    fn land_toasts(&mut self) -> Vec<String> {
        let start = std::time::Instant::now();
        let mut seen: Vec<String> = Vec::new();
        while self.rt.state.across_pending() || self.rt.state.across_writing() {
            assert!(start.elapsed().as_secs() < 20, "the machine never answered");
            std::thread::sleep(std::time::Duration::from_millis(10));
            self.rt.update(|s, _| {
                s.poll();
            });
            if let Some(n) = &self.rt.state.notice
                && seen.last() != Some(&n.title)
            {
                seen.push(n.title.clone());
            }
        }
        self.settle();
        seen
    }
}

#[test]
fn copies_in_flight_all_land_with_their_toasts() {
    let mut h = Harness::project();
    skill(&h.project_dir().join(".claude/skills"), "notes");
    h.rescan();
    let laptop = host_home(crate::hosts::install::VERSION);
    h.with_laptop_as(laptop.path(), |r| Box::new(SlowApply(r)));
    h.click("skill-review");
    h.click("copy");
    h.land_asks();
    h.click("copy-to-laptop-0");
    h.land_asks();
    h.click("copy-apply");
    assert!(h.rt.state.across_writing(), "the first copy is in flight");
    h.click("copy-cancel");
    assert!(h.rt.state.copy_dialog.is_none() && h.rt.state.across_writing(), "closing leaves it running");

    // A second copy, asked while the first still runs.
    h.click("skill-notes");
    h.click("copy");
    h.land_asks();
    h.click("copy-to-laptop-0");
    h.land_asks();
    h.click("copy-apply");
    let toasts = h.land_toasts();
    assert_eq!(toasts, ["Copied review to Global (~) on laptop", "Copied notes to Global (~) on laptop"]);
    assert!(laptop.path().join(".agents/skills/review/SKILL.md").is_file() && laptop.path().join(".agents/skills/notes/SKILL.md").is_file());
}

#[test]
fn a_double_pressed_toast_undo_runs_one_undo() {
    let mut h = Harness::project();
    let laptop = host_home(crate::hosts::install::VERSION);
    let log: Log = std::sync::Arc::default();
    let record = log.clone();
    h.with_laptop_as(laptop.path(), move |inner| Box::new(Recorded { inner, log: record.clone() }));
    h.copy_review_to_laptop();
    assert!(laptop.path().join(".agents/skills/review").is_dir());
    h.rt.update(|s, _| {
        s.undo_toast();
        s.undo_toast();
    });
    assert!(!h.rt.state.notice.as_ref().unwrap().undo, "the toast's Undo is gone once pressed");
    h.land_remote();
    assert!(!laptop.path().join(".agents/skills/review").exists());
    let undos = log.lock().unwrap().iter().filter(|(argv, _)| argv.first().is_some_and(|w| w == "undo")).count();
    assert_eq!(undos, 1, "one undo ran on laptop");
}

// The shell.

/// The pane defaults leave room for all three panes in the smallest window.
#[test]
fn pane_widths() {
    use crate::ds::size;
    const { assert!(232.0 + size::LIST_MIN + size::INSPECTOR_MIN <= 860.0) };
    assert_eq!(crate::ui::MIN_WINDOW, (860.0, 520.0));
    let h = Harness::project();
    let (sidebar, inspector) = (h.rt.state.sidebar, h.rt.state.inspector);
    assert_eq!(sidebar.width(), 232.0);
    assert!(sidebar.width() + size::LIST_MIN + size::INSPECTOR_MIN <= crate::ui::MIN_WINDOW.0);
    assert_eq!((inspector.min_width(), inspector.max_width()), (size::INSPECTOR_MIN, size::INSPECTOR_READ_MAX));
    assert!(!inspector.is_open(), "the inspector starts closed and opens on a selection");
}

/// The window at its minimum, with a skill selected.
fn smallest_with_selection() -> Harness {
    let mut h = Harness::project();
    h.rt.set_viewport(crate::ui::MIN_WINDOW, 2.0);
    h.settle();
    h.click("skill-review");
    h
}

#[test]
fn inspector_shows_at_minimum_window() {
    use crate::ds::size;
    let h = smallest_with_selection();
    let (pane, opacity) = h.rt.presented("inspector").expect("the inspector at 860");
    assert!(opacity > 0.99, "the inspector is visible: {opacity}");
    assert!(pane[2] >= size::INSPECTOR_MIN, "the inspector is {} wide at 860", pane[2]);
    let (list, _) = h.rt.presented("main-pane").expect("the list");
    assert!(list[2] >= size::LIST_MIN, "the list is {} wide at 860", list[2]);
}

#[test]
fn inspector_hides_when_sidebar_is_wide() {
    use crate::ds::size;
    let mut h = smallest_with_selection();
    let stored = h.rt.state.inspector.width();
    h.rt.state.sidebar.set_width(300.0);
    h.rt.invalidate();
    h.settle();
    assert!(h.rt.find("inspector").is_none(), "a 300 sidebar at 860: the list keeps its room and the inspector goes");
    assert!(h.rt.find("skill-review").is_some(), "the list is still there");

    h.rt.state.sidebar.set_width(232.0);
    h.rt.invalidate();
    h.settle();
    let (pane, _) = h.rt.presented("inspector").expect("back with the default sidebar");
    assert!(pane[2] >= size::INSPECTOR_MIN, "{pane:?}");
    assert_eq!(h.rt.state.inspector.width(), stored, "the stored width was never touched");
    h.rt.set_viewport((1180.0, 760.0), 2.0);
    h.settle();
    let (pane, _) = h.rt.presented("inspector").unwrap();
    assert_eq!(pane[2], stored, "a wide window draws it at the stored width");
}

#[test]
fn toolbar_title_follows_tabs() {
    let h = Harness::project();
    let (leading, _) = h.rt.presented("leading").expect("the toolbar's leading slot");
    let (tab, _) = h.rt.presented("tab-mcp").expect("the MCP tab");
    let (title, _) = h.rt.presented("title").expect("the title");
    let inside = |r: [f32; 4]| r[0] >= leading[0] && r[0] + r[2] <= leading[0] + leading[2] + 0.5;
    assert!(inside(tab) && inside(title), "tabs {tab:?} and title {title:?} in the leading slot {leading:?}");
    assert!(tab[0] + tab[2] < title[0], "the title {title:?} after the tabs {tab:?}");
}

// The Skills page.

#[test]
fn skill_rows_are_one_anatomy() {
    use crate::ds::size;
    let h = Harness::project();
    let (row, _) = h.rt.presented("skill-review").expect("the review row");
    assert_eq!(row[3], size::ROW_2, "a row with a description is two lines");
    let (filter, _) = h.rt.presented("filter").expect("the filter");
    let (check, _) = h.rt.presented("check-review").expect("the row's checkbox");
    let (all, _) = h.rt.presented("check-all").expect("the select-all checkbox");
    let (name, _) = h.rt.presented("skill-review-name").expect("the row's name");
    // One content edge: the filter, the select-all checkbox and the rows' checkboxes; the name follows.
    assert_eq!(check[0], filter[0], "the row's checkbox {check:?} on the filter's edge {filter:?}");
    assert_eq!(all[0], check[0], "select-all {all:?} over the rows' checkboxes {check:?}");
    assert!(name[0] > check[0] + check[2], "the name {name:?} after the checkbox {check:?}");
    let (status, _) = h.rt.presented("skill-review-trailing").expect("the status column");
    assert!((status[0] + status[2] - (filter[0] + filter[2])).abs() < 0.5, "the status {status:?} ends on the filter's right edge {filter:?}");
}

#[test]
fn bulk_bar_replaces_summary() {
    let mut h = Harness::project();
    h.click("check-review");
    assert!(h.rt.find("bulk-bar").is_some());
    assert!(h.rt.find("bulk-sync").is_some());
    let (filter, _) = h.rt.presented("filter").unwrap();
    let (remove, _) = h.rt.presented("bulk-remove").expect("Remove… in the bulk bar");
    assert!((remove[0] + remove[2] - (filter[0] + filter[2])).abs() < 0.5, "Remove… {remove:?} ends on the content edge {filter:?}");
    h.click("bulk-clear");
    assert!(h.rt.state.checked.is_empty());
    assert!(h.rt.find("bulk-bar").is_none(), "the summary is back");
}

/// The toolbar's elements as drawn: none may cover another.
fn toolbar_overlaps(h: &Harness) -> Vec<String> {
    let keys = ["tab-skills", "tab-mcp", "title", "new-skill", "check-updates", "rescan", "toolbar-more", "sync"];
    let rects: Vec<(&str, [f32; 4])> = keys.iter().filter_map(|&k| h.rt.presented(k).map(|(r, _)| (k, r))).collect();
    let (pane, _) = h.rt.presented("main-pane").unwrap();
    let mut bad = Vec::new();
    for (i, (a, ra)) in rects.iter().enumerate() {
        if ra[0] < pane[0] || ra[0] + ra[2] > pane[0] + pane[2] + 0.5 {
            bad.push(format!("{a} {ra:?} outside the pane {pane:?}"));
        }
        for (b, rb) in &rects[i + 1..] {
            let apart = ra[0] + ra[2] <= rb[0] + 0.5 || rb[0] + rb[2] <= ra[0] + 0.5;
            if !apart && ra[2] > 0.0 && rb[2] > 0.0 {
                bad.push(format!("{a} {ra:?} overlaps {b} {rb:?}"));
            }
        }
    }
    bad
}

#[test]
fn skills_toolbar_fits_the_smallest_window() {
    let h = smallest_with_selection();
    assert!(h.rt.find("inspector").is_some(), "the inspector is open");
    let bad = toolbar_overlaps(&h);
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    assert!(h.rt.find("sync").is_some() && h.rt.find("toolbar-more").is_some(), "Sync and the menu stay");
    assert!(h.rt.find("title").is_none(), "no room for the title");

    let wide = Harness::project();
    assert!(toolbar_overlaps(&wide).is_empty(), "{}", toolbar_overlaps(&wide).join("\n"));
    for k in ["title", "new-skill", "check-updates", "rescan", "sync"] {
        assert!(wide.rt.find(k).is_some(), "{k} in a wide toolbar");
    }
    assert!(wide.rt.find("toolbar-more").is_none(), "no menu when everything fits");
}

#[test]
fn narrow_toolbar_keeps_its_shortcuts_and_menu() {
    let mut h = smallest_with_selection();
    assert!(h.rt.find("new-skill").is_none(), "New skill is in the menu");
    key(&mut h, rune::Key::Command('n'), rune::Mods { cmd: true, ..rune::Mods::default() });
    assert!(h.rt.state.new_skill.is_some(), "⌘N still opens New skill");
    h.rt.update(|s, _| s.new_skill = None);
    h.settle();

    h.click("toolbar-more");
    assert!(h.rt.state.toolbar_menu);
    let label = |h: &Harness, i: usize| h.rt.elements().into_iter().find(|e| e.key.as_deref() == Some(format!("toolbar-more-popup-item{i}").as_str())).and_then(|e| e.label);
    assert_eq!(label(&h, 0).as_deref(), Some("New skill"));
    assert_eq!(label(&h, 1).as_deref(), Some("Check skills.sh for updates"));
    assert_eq!(label(&h, 2).as_deref(), Some("Rescan"));
    h.click("toolbar-more-popup-item0");
    assert!(h.rt.state.new_skill.is_some(), "the menu's New skill opens it");
    assert!(!h.rt.state.toolbar_menu, "choosing closes the menu");

    h.rt.update(|s, _| s.new_skill = None);
    h.settle();
    h.click("toolbar-more");
    h.click("tab-mcp");
    assert!(!h.rt.state.toolbar_menu, "another page lets go of the menu");
}

impl Harness {
    /// Whether the narrow toolbar's menu item `i` takes presses (a disabled item isn't focusable).
    fn menu_item_enabled(&self, i: usize) -> bool {
        let key = format!("toolbar-more-popup-item{i}");
        self.rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key.as_str())).unwrap_or_else(|| panic!("no {key}")).focusable
    }
}

#[test]
fn narrow_menu_disables_the_check_while_checking_and_on_a_host() {
    let mut h = smallest_with_selection();
    let scope = h.rt.state.scope.clone();
    h.rt.update(move |s, _| {
        s.checking.insert(scope);
    });
    h.settle();
    h.click("toolbar-more");
    assert!(h.menu_item_enabled(0) && h.menu_item_enabled(2), "New skill and Rescan stay");
    assert!(!h.menu_item_enabled(1), "Checking for updates… is disabled while it runs");

    let (mut h, _remote) = on_host();
    h.rt.set_viewport(crate::ui::MIN_WINDOW, 2.0);
    h.settle();
    h.click("skill-far");
    assert!(h.rt.find("new-skill").is_none(), "narrow on the host too");
    h.click("toolbar-more");
    assert!(!h.menu_item_enabled(1), "a host's skills aren't checked against skills.sh");
    assert!(h.menu_item_enabled(0) && h.menu_item_enabled(2));
}

#[test]
fn cmd_r_rescans_in_a_narrow_toolbar() {
    let mut h = smallest_with_selection();
    assert!(h.rt.find("rescan").is_none(), "Rescan is in the menu");
    skill(&h.project_dir().join(".claude/skills"), "fresh");
    key(&mut h, rune::Key::Command('r'), rune::Mods { cmd: true, ..rune::Mods::default() });
    assert!(h.rt.state.inv.get("fresh").is_some(), "⌘R rescanned");
}

#[test]
fn selecting_a_row_lets_go_of_a_stale_menu() {
    let mut h = smallest_with_selection();
    h.click("toolbar-more");
    assert!(h.rt.state.toolbar_menu);
    // Widening hides the menu with the flag still set; a new selection drops it.
    h.rt.set_viewport((1180.0, 760.0), 2.0);
    h.settle();
    assert!(h.rt.find("toolbar-more").is_none());
    h.click("skill-review");
    assert!(!h.rt.state.toolbar_menu, "a selection clears it");
    h.rt.set_viewport(crate::ui::MIN_WINDOW, 2.0);
    h.settle();
    assert!(h.rt.find("toolbar-more-popup-item0").is_none(), "the menu doesn't reopen by itself");
}

/// What a presented element covers, by key.
fn rect_of(h: &Harness, key: &str) -> [f32; 4] {
    h.rt.presented(key).unwrap_or_else(|| panic!("no element keyed {key}")).0
}

#[test]
fn skill_actions_fit_one_row_at_minimum() {
    use crate::ds::size;
    let long = "vercel-react-best-practices-for-large-apps";
    // 315 points of title: one line at 320. The longer one has to wrap.
    let longer = "vercel-react-best-practices-for-large-apps-and-monorepos";
    let mut h = Harness::project();
    skill(&h.project_dir().join(".claude/skills"), long);
    skill(&h.project_dir().join(".claude/skills"), longer);
    h.rescan();
    h.rt.set_viewport(crate::ui::MIN_WINDOW, 2.0);
    h.settle();
    h.click(&format!("skill-{long}"));
    let pane = rect_of(&h, "inspector");
    assert!((pane[2] - size::INSPECTOR_MIN).abs() < 0.5, "the inspector at its minimum: {pane:?}");

    // Open SKILL.md, Copy to…, "…" and Remove… are 340 points; the narrowest inspector has 320,
    // so Open SKILL.md goes into "…" after Reveal.
    assert!(h.rt.find("open-md").is_none(), "Open SKILL.md doesn't fit beside Copy to…");
    let keys = ["copy", "skill-more", "remove"];
    let rects: Vec<[f32; 4]> = keys.iter().map(|k| rect_of(&h, k)).collect();
    let mid = |r: &[f32; 4]| r[1] + r[3] / 2.0;
    for (k, r) in keys.iter().zip(&rects) {
        assert!((mid(r) - mid(&rects[0])).abs() < 0.5, "{k} {r:?} is on the row of copy {:?}", rects[0]);
    }
    for w in rects.windows(2) {
        assert!(w[0][0] + w[0][2] <= w[1][0] + 0.5, "in order and apart: {rects:?}");
    }
    let remove = rects[2];
    assert!(remove[0] + remove[2] <= pane[0] + pane[2] - size::PANE_EDGE + 0.5, "Remove… {remove:?} ends on the content edge of {pane:?}");

    let overflowing = |h: &Harness| -> Vec<String> {
        h.rt.elements()
            .into_iter()
            .filter(|e| e.path.contains("inspector-scroll") && !e.exiting && e.rect[2] > 0.0)
            .filter(|e| e.rect[0] < pane[0] - 0.5 || e.rect[0] + e.rect[2] > pane[0] + pane[2] + 0.5)
            .map(|e| format!("{} {:?}", e.path, e.rect))
            .collect()
    };
    let wide = overflowing(&h);
    assert!(wide.is_empty(), "wider than the inspector {pane:?}:\n{}", wide.join("\n"));

    h.click(&format!("skill-{longer}"));
    let title = h.rt.elements().into_iter().find(|e| e.text.as_deref() == Some(longer) && e.path.contains("inspector")).expect("the title");
    assert!(title.rect[3] > 30.0, "the longer title wraps: {:?}", title.rect);
    assert!(title.rect[0] + title.rect[2] <= pane[0] + pane[2] - size::PANE_EDGE + 0.5, "inside the content edge: {:?}", title.rect);
    let wide = overflowing(&h);
    assert!(wide.is_empty(), "wider than the inspector {pane:?}:\n{}", wide.join("\n"));

    h.click("skill-more");
    assert_eq!(h.item_label("skill-more", 0).as_deref(), Some("Reveal"));
    assert_eq!(h.item_label("skill-more", 1).as_deref(), Some("Open SKILL.md"), "after Reveal");
    assert!(h.item_enabled("skill-more", 1));
}

#[test]
fn open_md_stays_in_the_row_when_it_fits() {
    // A global skill with no project and no host: nowhere to copy to, so no Copy to….
    let home = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(home.path()).unwrap();
    skill(&root.join(".agents/skills"), "solo");
    let mut app = App::new(Paths::isolated(&root), Settings::default());
    app.open_scope(Scope::Global);
    let mut rt = Runtime::new(app, crate::ui::view, crate::ui::MIN_WINDOW, Env { system_dark: false, scale: 2.0 });
    rt.on_key = Some(crate::ui::on_key);
    rt.frame(0.0);
    let mut h = Harness { _home: home, root, rt };
    h.settle();
    h.click("skill-solo");
    assert!(h.rt.find("copy").is_none(), "nowhere to copy to");
    let (open, more, remove) = (rect_of(&h, "open-md"), rect_of(&h, "skill-more"), rect_of(&h, "remove"));
    assert!((open[1] - more[1]).abs() < 0.5 && (open[1] - remove[1]).abs() < 0.5, "one row: {open:?} {more:?} {remove:?}");
    h.click("skill-more");
    assert_eq!(h.item_label("skill-more", 0).as_deref(), Some("Reveal"));
    assert!(h.rt.find("skill-more-popup-item1").is_none(), "Reveal alone in the menu");
}

impl Harness {
    /// Whether item `i` of the menu keyed `menu` takes presses (a disabled item isn't focusable).
    fn item_enabled(&self, menu: &str, i: usize) -> bool {
        let key = format!("{menu}-popup-item{i}");
        self.rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key.as_str())).unwrap_or_else(|| panic!("no {key}")).focusable
    }

    fn item_label(&self, menu: &str, i: usize) -> Option<String> {
        let key = format!("{menu}-popup-item{i}");
        self.rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key.as_str())).and_then(|e| e.label)
    }
}

#[test]
fn reveal_is_in_the_menu() {
    let mut h = Harness::project();
    h.click("skill-review");
    assert!(h.rt.find("reveal").is_none(), "Reveal is no longer a button in the row");
    h.click("skill-more");
    assert!(h.rt.state.skill_menu, "the menu is open");
    assert_eq!(h.item_label("skill-more", 0).as_deref(), Some("Reveal"));
    assert!(h.item_enabled("skill-more", 0), "a skill on this machine can be revealed");
    let revealed = crate::app::reveal_log();
    h.click("skill-more-popup-item0");
    assert!(!h.rt.state.skill_menu, "choosing closes the menu");
    let folder = h.project_dir().join(".claude/skills/review");
    assert_eq!(crate::app::reveal_log()[revealed.len()..], [folder], "Reveal shows the skill's folder");
}

#[test]
fn menu_closes_on_selection_change() {
    let mut h = three_skills();
    h.rescan();
    h.click("skill-alpha");
    h.click("skill-more");
    assert!(h.rt.state.skill_menu && h.rt.find("skill-more-popup-item0").is_some(), "the menu is open");
    // While it is drawn the menu holds the arrows, as a menu does; the list stays put.
    key(&mut h, rune::Key::Down, rune::Mods::default());
    key(&mut h, rune::Key::Down, rune::Mods::default());
    assert_eq!(h.rt.state.selected.as_deref(), Some("alpha"), "the arrows move through the menu");
    assert!(h.rt.find("skill-more-popup-item0").is_some());
    // The selection moves under it (what ↓ does in the list): the menu goes, and the arrows are
    // the list's again.
    h.rt.update(|s, _| {
        s.step_selection(1, false);
    });
    h.settle();
    assert_eq!(h.rt.state.selected.as_deref(), Some("beta"));
    assert!(!h.rt.state.skill_menu, "the flag clears");
    assert!(h.rt.find("skill-more-popup-item0").is_none(), "the menu is gone");
    key(&mut h, rune::Key::Down, rune::Mods::default());
    assert_eq!(h.rt.state.selected.as_deref(), Some("gamma"), "↓ moves the selection");
    key(&mut h, rune::Key::Up, rune::Mods::default());
    assert_eq!(h.rt.state.selected.as_deref(), Some("beta"), "and keeps moving");

    // Escape closes it too, and ↓ is the list's at once.
    h.click("skill-more");
    key(&mut h, rune::Key::Escape, rune::Mods::default());
    assert!(!h.rt.state.skill_menu && h.rt.find("skill-more-popup-item0").is_none());
    key(&mut h, rune::Key::Down, rune::Mods::default());
    assert_eq!(h.rt.state.selected.as_deref(), Some("gamma"));
}

#[test]
fn an_open_skill_menu_keeps_list_keys_from_the_list() {
    let mut h = three_skills();
    h.rescan();
    h.click("skill-alpha");
    h.click("skill-more");
    key(&mut h, rune::Key::Command('a'), rune::Mods { cmd: true, ..rune::Mods::default() });
    assert!(h.rt.state.checked.is_empty(), "⌘A doesn't tick rows behind the menu");
    assert!(h.rt.state.skill_menu);
}

#[test]
fn a_skill_menu_left_open_lets_go_when_the_skill_goes() {
    let mut h = three_skills();
    h.rescan();
    h.click("skill-alpha");
    h.click("skill-more");
    assert!(h.rt.state.skill_menu);
    fs::remove_dir_all(h.project_dir().join(".claude/skills/alpha")).unwrap();
    h.rescan();
    assert!(h.rt.state.selected.is_none(), "the skill is gone");
    assert!(!h.rt.state.skill_menu, "and its menu with it");
    assert!(h.rt.find("skill-more-popup-item0").is_none());
    key(&mut h, rune::Key::Down, rune::Mods::default());
    assert!(h.rt.state.selected.is_some(), "↓ selects again");
}

#[test]
fn host_disables_local_actions() {
    let (mut h, _remote) = on_host();
    h.click("skill-far");
    h.land_remote();
    h.click("skill-more");
    assert_eq!(h.item_label("skill-more", 0).as_deref(), Some("Reveal"));
    assert!(!h.item_enabled("skill-more", 0), "Reveal stays in the menu, disabled");
    assert_eq!(h.item_label("skill-more", 1).as_deref(), Some("Open SKILL.md"));
    assert!(!h.item_enabled("skill-more", 1), "a host's SKILL.md can't be opened here");
}
