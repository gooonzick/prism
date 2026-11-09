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
    let mut ctx = crate::cli::Ctx { paths: h.rt.state.paths.clone(), settings: h.rt.state.settings.clone(), stdin: None, path_var: None };
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
