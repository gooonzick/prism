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
    let mut ctx = crate::cli::Ctx { paths: h.rt.state.paths.clone(), settings: h.rt.state.settings.clone(), stdin: None };
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
