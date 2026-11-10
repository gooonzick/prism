use super::*;

fn ctx(home: &std::path::Path) -> Ctx {
    let paths = Paths::isolated(home);
    let settings = Settings::load_from(&paths);
    Ctx { paths, settings, stdin: None, path_var: None, make_runner: crate::hosts::runner::no_runner() }
}

fn go(args: &[&str], ctx: &mut Ctx) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(args, ctx, &mut out, &mut err);
    (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
}

#[test]
fn a_folder_is_not_a_subcommand() {
    assert_eq!(run(vec!["/tmp".into()]), None);
    assert_eq!(run(vec!["--snap".into(), "x.png".into()]), None);
    assert_eq!(run(vec![]), None);
}

#[test]
fn help_and_version_route_to_the_cli() {
    // Clap answers these before any settings or home are read.
    for args in [&["--help"][..], &["-h"], &["help"], &["--version"], &["-V"], &["help", "mcp"]] {
        let args: Vec<std::ffi::OsString> = args.iter().map(|a| a.into()).collect();
        assert_eq!(run(args.clone()), Some(0), "{args:?}");
    }
}

#[test]
fn global_flags_before_a_subcommand_route_to_the_cli() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().to_str().unwrap();
    let with_project = format!("--project={project}");
    for args in [&["--json", "skills", "--help"][..], &["--project", project, "mcp", "--help"], &[&with_project, "--json", "mcp", "list", "--help"]] {
        let argv: Vec<std::ffi::OsString> = args.iter().map(|a| a.into()).collect();
        assert_eq!(run(argv), Some(0), "{args:?}");
    }
    // The window's own flags, a folder, or global flags with no subcommand still open the window.
    for args in [&["--json"][..], &["--project", project], &["--snap", "x.png", "skills"], &[project, "--json"]] {
        let argv: Vec<std::ffi::OsString> = args.iter().map(|a| a.into()).collect();
        assert_eq!(run(argv), None, "{args:?}");
    }
}

#[test]
fn agents_lists_detected_agents_as_json() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join(".claude")).unwrap();
    std::fs::create_dir(home.path().join(".codex")).unwrap();
    let (code, out, _) = go(&["agents", "--json"], &mut ctx(home.path()));
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let find = |id: &str| v.as_array().unwrap().iter().find(|a| a["id"] == id).unwrap().clone();
    assert_eq!(v.as_array().unwrap().len(), crate::agents::AGENTS.len());
    assert_eq!(find("claude")["detected"], true);
    assert_eq!(find("claude")["target"], true);
    assert_eq!(find("claude")["name"], "Claude Code");
    assert_eq!(find("codex")["detected"], true);
    assert_eq!(find("cursor")["detected"], false);
    assert_eq!(find("cursor")["target"], false);
}

#[test]
fn explicit_targets_override_detection() {
    let home = tempfile::tempdir().unwrap();
    let mut c = ctx(home.path());
    c.settings.targets = Some(vec!["cursor".into()]);
    let (_, out, _) = go(&["agents", "--json"], &mut c);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let cursor = v.as_array().unwrap().iter().find(|a| a["id"] == "cursor").unwrap();
    assert_eq!(cursor["target"], true);
}

#[test]
fn agents_prints_a_table_for_humans() {
    let home = tempfile::tempdir().unwrap();
    let (code, out, _) = go(&["agents"], &mut ctx(home.path()));
    assert_eq!(code, 0);
    assert!(out.contains("Claude Code"));
}

#[test]
fn unknown_flag_is_usage_error() {
    let home = tempfile::tempdir().unwrap();
    let (code, out, err) = go(&["agents", "--bogus"], &mut ctx(home.path()));
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert!(!err.is_empty());
}

#[test]
fn cli_undo_reverts_last_change() {
    let home = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(home.path()).unwrap();
    let mut c = ctx(&root);
    let (src, at) = (root.join("skill"), root.join("links/skill"));
    std::fs::create_dir_all(&src).unwrap();
    let mut journal = crate::fsops::Journal::default();
    journal.link(&at, &src).unwrap();
    store::save_last_change(&c.paths, Some(&store::LastChange { label: "Link skill".into(), journal }));

    let (code, out, _) = go(&["undo"], &mut c);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "Undid: Link skill");
    assert!(std::fs::symlink_metadata(&at).is_err());
    assert_eq!(store::load_last_change(&c.paths), None);

    let (code, out, _) = go(&["undo"], &mut c);
    assert_eq!(code, 0);
    assert!(out.contains("Nothing to undo"));
}

#[test]
fn cli_undo_fails_when_the_change_was_edited_since() {
    let home = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(home.path()).unwrap();
    let mut c = ctx(&root);
    let f = root.join("a.json");
    let mut journal = crate::fsops::Journal::default();
    journal.write_file(&f, b"1", &c.paths).unwrap();
    std::fs::write(&f, "hand").unwrap();
    store::save_last_change(&c.paths, Some(&store::LastChange { label: "Edit".into(), journal }));
    let (code, _, err) = go(&["undo"], &mut c);
    assert_eq!(code, 1);
    assert!(err.contains("changed since"), "{err}");
}

#[test]
fn a_stopped_undo_stays_the_last_change_until_it_can_finish() {
    let home = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(home.path()).unwrap();
    let mut c = ctx(&root);
    let f = root.join("a.json");
    std::fs::write(&f, "0").unwrap();
    let mut journal = crate::fsops::Journal::default();
    journal.write_file(&f, b"1", &c.paths).unwrap();
    std::fs::write(&f, "hand").unwrap();
    store::save_last_change(&c.paths, Some(&store::LastChange { label: "Edit".into(), journal }));

    assert_eq!(go(&["undo"], &mut c).0, 1);
    let left = store::load_last_change(&c.paths).expect("kept");
    assert!(matches!(left.journal.steps.as_slice(), [crate::fsops::Step::Rewrote { .. }]));

    std::fs::write(&f, "1").unwrap();
    assert_eq!(go(&["undo"], &mut c).0, 0);
    assert_eq!(std::fs::read(&f).unwrap(), b"0");
    assert_eq!(store::load_last_change(&c.paths), None);
}

#[test]
fn a_full_undo_leaves_no_backups() {
    let home = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(home.path()).unwrap();
    let mut c = ctx(&root);
    let f = root.join("a.json");
    std::fs::write(&f, "token").unwrap();
    let mut journal = crate::fsops::Journal::default();
    journal.write_file(&f, b"1", &c.paths).unwrap();
    let backups = c.paths.state.join("backups");
    assert!(backups.exists());
    store::save_last_change(&c.paths, Some(&store::LastChange { label: "Edit".into(), journal }));

    assert_eq!(go(&["undo"], &mut c).0, 0);
    assert_eq!(std::fs::read(&f).unwrap(), b"token");
    assert!(!backups.exists(), "no copy of the original stays behind");
}

#[test]
fn a_stopped_undo_keeps_its_backups() {
    let home = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(home.path()).unwrap();
    let mut c = ctx(&root);
    let f = root.join("a.json");
    std::fs::write(&f, "0").unwrap();
    let mut journal = crate::fsops::Journal::default();
    journal.write_file(&f, b"1", &c.paths).unwrap();
    std::fs::write(&f, "hand").unwrap();
    store::save_last_change(&c.paths, Some(&store::LastChange { label: "Edit".into(), journal }));
    assert_eq!(go(&["undo"], &mut c).0, 1);
    assert!(c.paths.state.join("backups").exists(), "the retry needs them");
}

fn write_skill(dir: &std::path::Path, name: &str, desc: &str) {
    std::fs::create_dir_all(dir.join(name)).unwrap();
    std::fs::write(dir.join(name).join("SKILL.md"), format!("---\nname: {name}\ndescription: {desc}\n---\nline one\nline two\n")).unwrap();
}

fn json(out: &str) -> serde_json::Value {
    serde_json::from_str(out).unwrap()
}

#[test]
fn skills_list_json_shows_placements() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    write_skill(&h.join(".claude/skills"), "solo", "Only in Claude");
    write_skill(&h.join(".agents/skills"), "shared", "Linked around");
    std::fs::create_dir_all(h.join(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(h.join(".agents/skills/shared"), h.join(".claude/skills/shared")).unwrap();
    let mut c = ctx(h);
    c.settings.targets = Some(vec!["claude".into()]);
    let (code, out, _) = go(&["skills", "list", "--json"], &mut c);
    assert_eq!(code, 0);
    let v = json(&out);
    let find = |n: &str| v.as_array().unwrap().iter().find(|s| s["name"] == n).unwrap().clone();
    let solo = find("solo");
    assert_eq!(solo["placements"]["claude"], "dir");
    assert_eq!(solo["placements"]["agents"], "missing");
    assert_eq!(solo["synced"], false);
    assert_eq!(solo["description"], "Only in Claude");
    let shared = find("shared");
    assert_eq!(shared["placements"]["agents"], "dir");
    assert_eq!(shared["placements"]["claude"], "linked");
    assert_eq!(shared["synced"], true);
}

#[test]
fn skills_show_reports_body_lines() {
    let home = tempfile::tempdir().unwrap();
    write_skill(&home.path().join(".claude/skills"), "solo", "d");
    let (code, out, _) = go(&["skills", "show", "solo", "--json"], &mut ctx(home.path()));
    assert_eq!(code, 0);
    let v = json(&out);
    assert_eq!(v["name"], "solo");
    assert_eq!(v["body_lines"], 2);
    assert_eq!(v["placements"]["claude"], "dir");
}

#[test]
fn skills_show_unknown_is_exit_1() {
    let home = tempfile::tempdir().unwrap();
    let (code, _, err) = go(&["skills", "show", "nope"], &mut ctx(home.path()));
    assert_eq!(code, 1);
    assert!(err.contains("no skill named nope in Global"), "{err}");
}

#[test]
fn project_flag_scopes_skills_without_registration() {
    let home = tempfile::tempdir().unwrap();
    let proj = home.path().join("app");
    write_skill(&proj.join(".claude/skills"), "local", "d");
    let (code, out, _) = go(&["skills", "list", "--json", "--project", proj.to_str().unwrap()], &mut ctx(home.path()));
    assert_eq!(code, 0);
    assert_eq!(json(&out).as_array().unwrap().len(), 1);
    let (code, _, err) = go(&["skills", "list", "--project", home.path().join("missing").to_str().unwrap()], &mut ctx(home.path()));
    assert_eq!(code, 2);
    assert!(err.contains("not a folder"), "{err}");
}

#[test]
fn projects_add_persists_to_state_dir() {
    let home = tempfile::tempdir().unwrap();
    let proj = home.path().join("app");
    std::fs::create_dir_all(&proj).unwrap();
    let mut c = ctx(home.path());
    let (code, _, _) = go(&["projects", "add", proj.to_str().unwrap()], &mut c);
    assert_eq!(code, 0);
    let saved = Settings::load_from(&c.paths);
    assert_eq!(saved.projects, vec![std::fs::canonicalize(&proj).unwrap()]);
    let (_, out, _) = go(&["projects", "list", "--json"], &mut c);
    assert_eq!(json(&out).as_array().unwrap().len(), 1);
    let (code, _, _) = go(&["projects", "remove", proj.to_str().unwrap()], &mut c);
    assert_eq!(code, 0);
    assert!(Settings::load_from(&c.paths).projects.is_empty());
}

#[test]
fn projects_find_lists_without_adding() {
    let home = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(home.path()).unwrap();
    write_skill(&root.join("dev/app/.claude/skills"), "x", "d");
    let mut c = ctx(&root);
    let (code, out, _) = go(&["projects", "find", root.join("dev").to_str().unwrap(), "--json"], &mut c);
    assert_eq!(code, 0);
    let v = json(&out);
    assert_eq!(v[0]["path"], root.join("dev/app").to_str().unwrap());
    assert_eq!(v[0]["skills"], 1);
    assert!(Settings::load_from(&c.paths).projects.is_empty());
}

#[test]
fn targets_set_saves_and_rejects_unknown_agent() {
    let home = tempfile::tempdir().unwrap();
    let mut c = ctx(home.path());
    let (code, _, _) = go(&["targets", "set", "claude", "codex"], &mut c);
    assert_eq!(code, 0);
    assert_eq!(Settings::load_from(&c.paths).targets, Some(vec!["claude".to_string(), "codex".to_string()]));
    let (code, _, err) = go(&["targets", "set", "nope"], &mut c);
    assert_eq!(code, 2);
    assert!(err.contains("nope") && err.contains("claude"), "{err}");
    assert_eq!(Settings::load_from(&c.paths).targets, Some(vec!["claude".to_string(), "codex".to_string()]));
}

/// A skill that exists only in Claude's folder, with Codex as the one sync target.
fn unsynced(home: &std::path::Path) -> Ctx {
    write_skill(&home.join(".claude/skills"), "review", "Reviews");
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    let mut c = ctx(home);
    c.settings.targets = Some(vec!["claude".into(), "codex".into()]);
    c
}

#[test]
fn sync_without_apply_changes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let mut c = unsynced(home.path());
    let (code, out, _) = go(&["skills", "sync", "--json"], &mut c);
    assert_eq!(code, 0);
    let v = json(&out);
    assert_eq!(v["applied"], false);
    assert_eq!(v["plan_id"].as_str().unwrap().len(), 12);
    assert!(!v["actions"].as_array().unwrap().is_empty());
    assert!(v["conflicts"].as_array().unwrap().is_empty());
    assert!(!home.path().join(".agents/skills/review").exists());
    assert_eq!(store::load_last_change(&c.paths), None);
}

#[test]
fn sync_apply_links_and_records_last_change() {
    let home = tempfile::tempdir().unwrap();
    let mut c = unsynced(home.path());
    let (_, out, _) = go(&["skills", "sync", "--json"], &mut c);
    let plan = json(&out);
    let id = plan["plan_id"].as_str().unwrap();
    let n = plan["actions"].as_array().unwrap().len();
    let (code, out, _) = go(&["skills", "sync", "--apply", "--plan-id", id, "--json"], &mut c);
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    assert_eq!(v["applied"], true);
    assert_eq!(v["done"], n);
    assert!(home.path().join(".agents/skills/review/SKILL.md").is_file());
    assert!(std::fs::symlink_metadata(home.path().join(".codex/skills/review")).unwrap().file_type().is_symlink());
    let last = store::load_last_change(&c.paths).expect("last change");
    assert_eq!(last.label, format!("sync ({n} changes)"));
    let (code, _, _) = go(&["undo"], &mut c);
    assert_eq!(code, 0);
    assert!(!home.path().join(".agents/skills/review").exists());
    assert!(home.path().join(".claude/skills/review/SKILL.md").is_file());
}

#[test]
fn apply_with_stale_plan_id_exits_4() {
    let home = tempfile::tempdir().unwrap();
    let mut c = unsynced(home.path());
    let (_, out, _) = go(&["skills", "sync", "--json"], &mut c);
    let old = json(&out)["plan_id"].as_str().unwrap().to_string();
    std::fs::write(home.path().join(".claude/skills/review/extra.md"), "new").unwrap();
    let (code, out, _) = go(&["skills", "sync", "--apply", "--plan-id", &old, "--json"], &mut c);
    assert_eq!(code, 4);
    let v = json(&out);
    assert_eq!(v["applied"], false);
    assert_ne!(v["plan_id"].as_str().unwrap(), old);
    assert!(!home.path().join(".agents/skills/review").exists());
    assert_eq!(store::load_last_change(&c.paths), None);
    let (code, human, _) = go(&["skills", "sync", "--apply", "--plan-id", &old], &mut c);
    assert_eq!(code, 4);
    assert!(human.contains("changed"), "{human}");
}

#[test]
fn plan_id_without_apply_is_usage() {
    let home = tempfile::tempdir().unwrap();
    let (code, _, err) = go(&["skills", "sync", "--plan-id", "abc"], &mut unsynced(home.path()));
    assert_eq!(code, 2);
    assert!(err.contains("--apply"), "{err}");
}

#[test]
fn a_refused_skills_plan_prints_json_error() {
    let home = tempfile::tempdir().unwrap();
    let (code, out, err) = go(&["skills", "sync", "nosuch", "--json"], &mut unsynced(home.path()));
    assert_eq!(code, 1);
    assert!(err.contains("no skill named nosuch"), "{err}");
    assert!(json(&out)["error"].as_str().unwrap().contains("no skill named nosuch"), "{out}");
}

#[test]
fn a_partial_apply_points_at_undo() {
    use std::os::unix::fs::PermissionsExt;
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    write_skill(&h.join(".claude/skills"), "a", "d");
    write_skill(&h.join(".codex/skills"), "b", "d");
    let mut c = ctx(h);
    let locked = h.join(".codex/skills");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let (code, _, err) = go(&["skills", "remove", "a", "b", "--apply"], &mut c);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(code, 1);
    assert!(err.contains("Stopped after 1 of 2"), "{err}");
    assert!(err.contains("prism undo"), "{err}");
}

#[test]
fn remove_needs_apply() {
    let home = tempfile::tempdir().unwrap();
    let mut c = unsynced(home.path());
    let (code, out, _) = go(&["skills", "remove", "review", "--json"], &mut c);
    assert_eq!(code, 0);
    assert_eq!(json(&out)["applied"], false);
    assert!(home.path().join(".claude/skills/review").exists());
    let (code, out, _) = go(&["skills", "remove", "review", "--apply", "--json"], &mut c);
    assert_eq!(code, 0, "{out}");
    assert!(!home.path().join(".claude/skills/review").exists());
    assert_eq!(store::load_last_change(&c.paths).unwrap().label, "remove review");
    assert_eq!(go(&["undo"], &mut c).0, 0);
    assert!(home.path().join(".claude/skills/review/SKILL.md").is_file());
}

#[test]
fn link_and_unlink_plan_one_line_each() {
    let home = tempfile::tempdir().unwrap();
    let mut c = unsynced(home.path());
    assert_eq!(go(&["skills", "sync", "--apply"], &mut c).0, 0);
    let (code, out, _) = go(&["skills", "unlink", "review", "--agent", "codex", "--json"], &mut c);
    assert_eq!(code, 0);
    assert_eq!(json(&out)["actions"], serde_json::json!(["Unlink review from Codex"]));
    let id = json(&out)["plan_id"].as_str().unwrap().to_string();
    assert!(std::fs::symlink_metadata(home.path().join(".codex/skills/review")).is_ok());
    assert_eq!(go(&["skills", "unlink", "review", "--agent", "codex", "--apply", "--plan-id", &id], &mut c).0, 0);
    assert!(std::fs::symlink_metadata(home.path().join(".codex/skills/review")).is_err());
    assert_eq!(store::load_last_change(&c.paths).unwrap().label, "unlink review from Codex");

    let (_, out, _) = go(&["skills", "link", "review", "--agent", "codex", "--json"], &mut c);
    assert_eq!(json(&out)["actions"], serde_json::json!(["Link review into Codex"]));
    assert_eq!(go(&["skills", "link", "review", "--agent", "codex", "--apply"], &mut c).0, 0);
    assert!(std::fs::symlink_metadata(home.path().join(".codex/skills/review")).unwrap().file_type().is_symlink());
    let (code, _, err) = go(&["skills", "link", "review", "--agent", "nope"], &mut c);
    assert_eq!(code, 2);
    assert!(err.contains("nope"), "{err}");
}

#[test]
fn a_plan_with_only_conflicts_applies_nothing() {
    let home = tempfile::tempdir().unwrap();
    let mut c = unsynced(home.path());
    write_skill(&home.path().join(".agents/skills"), "review", "Canonical");
    write_skill(&home.path().join(".codex/skills"), "review", "Different");
    let (code, out, _) = go(&["skills", "sync", "--apply", "--json"], &mut c);
    assert_eq!(code, 0);
    let v = json(&out);
    assert_eq!(v["applied"], false);
    assert_eq!(v["conflicts"][0]["skill"], "review");
    assert!(v["conflicts"].as_array().unwrap().iter().any(|c| c["reason"].as_str().unwrap().contains("Codex")));
    assert_eq!(store::load_last_change(&c.paths), None);
}

#[test]
fn copy_to_global_and_clean_broken() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let proj = h.join("app");
    write_skill(&proj.join(".agents/skills"), "local", "d");
    std::fs::create_dir_all(h.join(".codex")).unwrap();
    let mut c = ctx(h);
    c.settings.targets = Some(vec!["codex".into()]);
    let p = proj.to_str().unwrap();
    let (code, out, _) = go(&["skills", "copy", "local", "--to-global", "--project", p, "--json"], &mut c);
    assert_eq!(code, 0);
    assert_eq!(json(&out)["applied"], false);
    assert!(!h.join(".agents/skills/local").exists());
    assert_eq!(go(&["skills", "copy", "local", "--to-global", "--project", p, "--apply"], &mut c).0, 0);
    assert!(h.join(".agents/skills/local/SKILL.md").is_file());
    assert!(std::fs::symlink_metadata(h.join(".codex/skills/local")).is_ok());
    assert_eq!(go(&["skills", "copy", "local", "--project", p], &mut c).0, 2);

    std::fs::create_dir_all(h.join(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(h.join("gone"), h.join(".claude/skills/ghost")).unwrap();
    let (_, out, _) = go(&["skills", "clean-broken", "--json"], &mut c);
    assert_eq!(json(&out)["actions"].as_array().unwrap().len(), 1);
    assert!(std::fs::symlink_metadata(h.join(".claude/skills/ghost")).is_ok());
    assert_eq!(go(&["skills", "clean-broken", "--apply"], &mut c).0, 0);
    assert!(std::fs::symlink_metadata(h.join(".claude/skills/ghost")).is_err());
    assert_eq!(store::load_last_change(&c.paths).unwrap().label, "clean up broken links");
}

#[test]
fn new_skill_is_immediate() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    let mut c = ctx(home.path());
    c.settings.targets = Some(vec!["codex".into()]);
    let (code, _, _) = go(&["skills", "new", "pdf-tools", "--description", "Use when: PDFs"], &mut c);
    assert_eq!(code, 0);
    let m = crate::skill::read(&home.path().join(".agents/skills/pdf-tools")).unwrap();
    assert_eq!(m.name.as_deref(), Some("pdf-tools"));
    assert_eq!(m.description.as_deref(), Some("Use when: PDFs"));
    assert!(std::fs::symlink_metadata(home.path().join(".codex/skills/pdf-tools")).unwrap().file_type().is_symlink());
    assert_eq!(go(&["skills", "new", "pdf-tools"], &mut c).0, 1);
    let (code, _, err) = go(&["skills", "new", "Bad Name"], &mut c);
    assert_eq!(code, 2);
    assert!(err.contains("lowercase"), "{err}");
}

#[test]
fn a_partial_apply_saves_what_was_done_and_undo_restores_it() {
    use std::os::unix::fs::PermissionsExt;
    // A read-only folder doesn't stop root, so there is no failure to provoke.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    write_skill(&h.join(".claude/skills"), "a", "d");
    write_skill(&h.join(".codex/skills"), "b", "d");
    let mut c = ctx(h);
    c.settings.targets = Some(vec!["claude".into(), "codex".into()]);
    // b cannot leave its folder, so removing it fails after a is gone.
    struct Unlock(std::path::PathBuf);
    impl Drop for Unlock {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }
    let _unlock = Unlock(h.join(".codex/skills"));
    std::fs::set_permissions(&_unlock.0, std::fs::Permissions::from_mode(0o555)).unwrap();
    let (code, out, _) = go(&["skills", "remove", "a", "b", "--apply", "--json"], &mut c);
    std::fs::set_permissions(&_unlock.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(code, 1, "{out}");
    let v = json(&out);
    assert_eq!(v["applied"], false);
    assert_eq!(v["done"], 1);
    assert!(v["error"].as_str().is_some());
    assert!(!h.join(".claude/skills/a").exists());
    assert!(h.join(".codex/skills/b").exists());
    assert_eq!(store::load_last_change(&c.paths).unwrap().label, "remove 2 skills");
    assert_eq!(go(&["undo"], &mut c).0, 0);
    assert!(h.join(".claude/skills/a/SKILL.md").is_file());
}

/// `fine` only in Claude's folder (two actions: adopt, link into Codex), `clash` in conflict.
fn mixed(h: &std::path::Path) -> Ctx {
    write_skill(&h.join(".claude/skills"), "fine", "d");
    write_skill(&h.join(".agents/skills"), "clash", "Canonical");
    write_skill(&h.join(".codex/skills"), "clash", "Different");
    let mut c = ctx(h);
    c.settings.targets = Some(vec!["codex".into()]);
    c
}

#[test]
fn applying_a_plan_with_actions_and_conflicts_does_both() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let mut c = mixed(h);
    assert!(std::fs::symlink_metadata(h.join(".codex/skills/fine")).is_err());
    let (code, out, _) = go(&["skills", "sync", "--apply", "--json"], &mut c);
    assert_eq!(code, 0, "{out}");
    let v = json(&out);
    assert_eq!(v["applied"], true);
    assert_eq!(v["done"], 2);
    assert_eq!(v["actions"].as_array().unwrap().len(), 2);
    assert!(std::fs::symlink_metadata(h.join(".codex/skills/fine")).unwrap().file_type().is_symlink());
    assert_eq!(v["conflicts"][0]["skill"], "clash");
    assert!(h.join(".codex/skills/clash").is_dir());

    let home = tempfile::tempdir().unwrap();
    let mut c = mixed(home.path());
    let (code, human, _) = go(&["skills", "sync", "--apply"], &mut c);
    assert_eq!(code, 0);
    assert!(human.contains("Done: 2 changes") && human.contains("clash"), "{human}");
}

#[test]
fn changing_targets_changes_the_copy_plan_id() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let proj = h.join("app");
    write_skill(&proj.join(".agents/skills"), "local", "d");
    std::fs::create_dir_all(h.join(".codex")).unwrap();
    let mut c = ctx(h);
    c.settings.targets = Some(vec!["codex".into()]);
    let args = ["skills", "copy", "local", "--to-global", "--project", proj.to_str().unwrap(), "--json"];
    let before = json(&go(&args, &mut c).1)["plan_id"].as_str().unwrap().to_string();
    c.settings.targets = Some(vec!["codex".into(), "claude".into()]);
    let after = json(&go(&args, &mut c).1)["plan_id"].as_str().unwrap().to_string();
    assert_ne!(before, after);
    let (code, _, _) = go(&["skills", "copy", "local", "--to-global", "--project", proj.to_str().unwrap(), "--apply", "--plan-id", &before], &mut c);
    assert_eq!(code, 4);
}

/// A skill folder with a nested file, a hidden file and a relative symlink, as another
/// machine would send it.
fn sent_skill(h: &std::path::Path) -> std::path::PathBuf {
    let src = h.join("elsewhere/pdf");
    write_skill(&h.join("elsewhere"), "pdf", "Reads PDFs");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("sub/a.md"), "nested").unwrap();
    std::fs::write(src.join(".hidden"), "dot").unwrap();
    std::os::unix::fs::symlink("sub/a.md", src.join("link")).unwrap();
    src
}

/// The tar the sender streams: `tar -C <dir> -cf - .`.
fn tar_of(dir: &std::path::Path) -> Vec<u8> {
    let out = std::process::Command::new("tar").env("COPYFILE_DISABLE", "1").arg("-C").arg(dir).args(["-cf", "-", "."]).output().unwrap();
    assert!(out.status.success());
    out.stdout
}

/// A ctx for home `h` linking into Codex, with `pdf`'s tar ready on stdin.
fn receiving(h: &std::path::Path) -> Ctx {
    std::fs::create_dir_all(h.join(".codex")).unwrap();
    let mut c = ctx(h);
    c.settings.targets = Some(vec!["codex".into()]);
    c
}

fn receive_args<'a>(hash: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["skills", "receive", "pdf", "--to-global", "--source-hash", hash, "--from", "laptop", "--json"];
    args.extend_from_slice(extra);
    args
}

/// Plans a receive of `pdf` with `hash` as the sender's and applies it with the tar of `src`
/// on stdin.
fn receive_pdf_as(c: &mut Ctx, src: &std::path::Path, hash: &str) -> (i32, String, String) {
    let id = json(&go(&receive_args(hash, &[]), c).1)["plan_id"].as_str().unwrap().to_string();
    c.stdin = Some(Box::new(std::io::Cursor::new(tar_of(src))));
    go(&receive_args(hash, &["--apply", "--plan-id", &id]), c)
}

/// The same, with `src`'s own hash, as `wire skill-hash` gives it.
fn receive_pdf(c: &mut Ctx, src: &std::path::Path) -> (i32, String, String) {
    let hash = crate::planid::tree_hash(src).unwrap().to_string();
    receive_pdf_as(c, src, &hash)
}

fn leftovers(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with(".prism-incoming")).collect()
}

#[test]
fn receive_keeps_tree_and_refuses_clash() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let src = sent_skill(h);
    let mut c = receiving(h);

    let (code, out, _) = go(&receive_args("7", &[]), &mut c);
    assert_eq!(code, 0);
    let plan = json(&out);
    assert_eq!(plan["actions"], serde_json::json!(["Copy pdf from laptop to Global (~) (link into Codex)"]));
    assert!(!h.join(".agents/skills/pdf").exists(), "a plan changes nothing");

    let (code, out, err) = receive_pdf(&mut c, &src);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(json(&out)["applied"], true);
    let got = h.join(".agents/skills/pdf");
    assert_eq!(std::fs::read_to_string(got.join("SKILL.md")).unwrap(), std::fs::read_to_string(src.join("SKILL.md")).unwrap());
    assert_eq!(std::fs::read_to_string(got.join("sub/a.md")).unwrap(), "nested");
    assert_eq!(std::fs::read_to_string(got.join(".hidden")).unwrap(), "dot");
    assert_eq!(std::fs::read_link(got.join("link")).unwrap(), std::path::Path::new("sub/a.md"), "relative links arrive as links");
    assert_eq!(crate::planid::tree_hash(&got), crate::planid::tree_hash(&src));
    assert!(std::fs::symlink_metadata(h.join(".codex/skills/pdf")).unwrap().file_type().is_symlink());
    assert!(leftovers(&h.join(".agents/skills")).is_empty());
    assert_eq!(store::load_last_change(&c.paths).unwrap().label, "copy pdf from laptop");

    // The same name again is a conflict, and the first copy stays as it is.
    std::fs::write(src.join("SKILL.md"), "changed").unwrap();
    let plan = json(&go(&receive_args("8", &[]), &mut c).1);
    assert_eq!(plan["actions"], serde_json::json!([]));
    assert_eq!(plan["conflicts"], serde_json::json!([{ "skill": "pdf", "reason": "already in Global (~)" }]));
    let id = plan["plan_id"].as_str().unwrap().to_string();
    c.stdin = Some(Box::new(std::io::Cursor::new(tar_of(&src))));
    assert_eq!(go(&receive_args("8", &["--apply", "--plan-id", &id]), &mut c).0, 0);
    assert_ne!(std::fs::read_to_string(got.join("SKILL.md")).unwrap(), "changed");
    assert!(leftovers(&h.join(".agents/skills")).is_empty());
}

#[test]
fn receive_refuses_what_does_not_match_the_senders_hash() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let src = sent_skill(h);
    let mut c = receiving(h);
    let hash = crate::planid::tree_hash(&src).unwrap().to_string();
    std::fs::write(src.join("sub/a.md"), "tampered").unwrap();
    let (code, out, _) = receive_pdf_as(&mut c, &src, &hash);
    assert_eq!(code, 1);
    assert_eq!(json(&out)["error"], "pdf arrived incomplete or changed; nothing was placed");
    assert!(std::fs::symlink_metadata(h.join(".agents/skills/pdf")).is_err());
    assert!(std::fs::symlink_metadata(h.join(".codex/skills/pdf")).is_err());
    assert!(leftovers(&h.join(".agents/skills")).is_empty());
    assert_eq!(store::load_last_change(&c.paths), None);
}

#[test]
fn a_linked_skill_round_trips_with_the_hash_wire_gives() {
    // On the sending machine .agents/skills/pdf is a link to a folder elsewhere.
    let sender = tempfile::tempdir().unwrap();
    let s = sender.path();
    let folder = sent_skill(s);
    std::fs::create_dir_all(s.join(".agents/skills")).unwrap();
    std::os::unix::fs::symlink(&folder, s.join(".agents/skills/pdf")).unwrap();
    let mut from = ctx(s);
    let (code, out, _) = go(&["wire", "skill-hash", "pdf"], &mut from);
    assert_eq!(code, 0);
    let hash = json(&out)["hash"].as_u64().unwrap().to_string();

    let receiver = tempfile::tempdir().unwrap();
    let mut c = receiving(receiver.path());
    let (code, out, err) = receive_pdf_as(&mut c, &s.join(".agents/skills/pdf"), &hash);
    assert_eq!(code, 0, "{out}{err}");
    let got = crate::planid::tree_hash(&receiver.path().join(".agents/skills/pdf")).unwrap();
    assert_eq!(got.to_string(), hash);
}

#[test]
fn receive_refuses_a_name_that_is_a_path() {
    let home = tempfile::tempdir().unwrap();
    let mut c = receiving(home.path());
    for name in ["../evil", "a/b", ".", "..", ""] {
        let (code, _, err) = go(&["skills", "receive", name, "--to-global", "--source-hash", "1", "--from", "laptop"], &mut c);
        assert_eq!(code, 2, "{name}: {err}");
    }
    assert!(!home.path().join(".agents").exists());
}

#[test]
fn receive_plan_id_covers_hash() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let src = sent_skill(h);
    let mut c = receiving(h);
    let id = |c: &mut Ctx, hash: &str| json(&go(&receive_args(hash, &[]), c).1)["plan_id"].as_str().unwrap().to_string();
    let (one, two) = (id(&mut c, "1"), id(&mut c, "2"));
    assert_ne!(one, two);
    c.stdin = Some(Box::new(std::io::Cursor::new(tar_of(&src))));
    let (code, out, _) = go(&receive_args("2", &["--apply", "--plan-id", &one]), &mut c);
    assert_eq!(code, 4);
    assert_eq!(json(&out)["plan_changed"], true);
    assert!(!h.join(".agents/skills/pdf").exists());
}

#[test]
fn receive_then_undo_removes_it() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let src = sent_skill(h);
    let mut c = receiving(h);
    assert_eq!(receive_pdf(&mut c, &src).0, 0);
    assert!(h.join(".agents/skills/pdf/SKILL.md").is_file());
    let (code, out, _) = go(&["undo"], &mut c);
    assert_eq!(code, 0, "{out}");
    assert!(std::fs::symlink_metadata(h.join(".agents/skills/pdf")).is_err());
    assert!(std::fs::symlink_metadata(h.join(".codex/skills/pdf")).is_err());
}

#[test]
fn skills_trash_moves_copy() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    write_skill(&h.join(".claude/skills"), "own", "d");
    let mut c = ctx(h);
    let (code, out, _) = go(&["skills", "trash", "own", "--agent", "claude", "--json"], &mut c);
    assert_eq!(code, 0);
    let plan = json(&out);
    assert_eq!(plan["actions"], serde_json::json!(["Move Claude Code's copy of own to the trash"]));
    assert!(h.join(".claude/skills/own/SKILL.md").is_file(), "a plan changes nothing");
    let id = plan["plan_id"].as_str().unwrap().to_string();
    let (code, out, _) = go(&["skills", "trash", "own", "--agent", "claude", "--apply", "--plan-id", &id], &mut c);
    assert_eq!(code, 0, "{out}");
    assert!(std::fs::symlink_metadata(h.join(".claude/skills/own")).is_err());
    assert_eq!(store::load_last_change(&c.paths).unwrap().label, "trash Claude Code's copy of own");
    assert_eq!(go(&["undo"], &mut c).0, 0);
    assert!(h.join(".claude/skills/own/SKILL.md").is_file());

    // A link is not a copy: it is unlinked, not trashed.
    std::fs::create_dir_all(h.join(".codex/skills")).unwrap();
    std::os::unix::fs::symlink(h.join(".claude/skills/own"), h.join(".codex/skills/own")).unwrap();
    let plan = json(&go(&["skills", "trash", "own", "--agent", "codex", "--json"], &mut c).1);
    assert_eq!(plan["actions"], serde_json::json!([]));
    assert_eq!(plan["conflicts"][0]["reason"], "Codex has a link, not its own copy; unlink it instead");
}

#[test]
fn install_bad_source_is_usage_error() {
    let home = tempfile::tempdir().unwrap();
    let (code, out, err) = go(&["skills", "install", "not a source!"], &mut ctx(home.path()));
    // Exit 2 and not 1: a runner was never looked for, let alone started.
    assert_eq!(code, 2, "{out}{err}");
    assert!(err.contains("not a source!"), "{err}");
}

#[test]
fn repo_bad_source_is_usage_error() {
    let home = tempfile::tempdir().unwrap();
    let (code, out, err) = go(&["skills", "repo", "not a source!"], &mut ctx(home.path()));
    // Rejected as the window rejects it, before any runner is looked for.
    assert_eq!(code, 2, "{out}{err}");
    assert!(err.contains("not a source!"), "{err}");
}

#[test]
fn install_rejects_unknown_target_agent() {
    let home = tempfile::tempdir().unwrap();
    let (code, _, err) = go(&["skills", "install", "owner/repo", "--to", "nope"], &mut ctx(home.path()));
    assert_eq!(code, 2);
    assert!(err.contains("nope") && err.contains("claude"), "{err}");
}

#[test]
#[ignore = "needs network and Node"]
fn search_finds_find_skills() {
    let home = tempfile::tempdir().unwrap();
    let (code, out, err) = go(&["skills", "search", "find-skills", "--json"], &mut ctx(home.path()));
    assert_eq!(code, 0, "{err}");
    assert!(json(&out).as_array().unwrap().iter().any(|r| r["skill"] == "find-skills"), "{out}");
}

#[test]
#[ignore = "needs network and Node"]
fn install_links_into_targets() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    std::fs::create_dir_all(h.join(".codex")).unwrap();
    let mut c = ctx(h);
    c.settings.targets = Some(vec!["codex".into()]);
    let (code, out, err) = go(&["skills", "install", "vercel-labs/skills@find-skills", "--json"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(json(&out)["installed"][0], "find-skills");
    assert!(std::fs::symlink_metadata(h.join(".codex/skills/find-skills")).unwrap().file_type().is_symlink());
}

/// A home with Cursor and Codex holding `gl` (the first with a literal env value and a header
/// secret, the second differing in args), plus a broken Codex-style file for the error path.
fn mcp_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".cursor")).unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::write(
        home.path().join(".cursor/mcp.json"),
        r#"{"mcpServers":{
            "gl":{"command":"npx","args":["-y","gl-mcp","--api-key=FIXTURE_ARG_SECRET"],"env":{"GITLAB_TOKEN":"FIXTURE_ENV_SECRET","HOST":"${env:GITLAB_HOST}"},"extraSecret":"FIXTURE_EXTRA_SECRET"},
            "web":{"url":"https://u:FIXTURE_URL_SECRET@mcp.example.com/mcp","headers":{"Authorization":"Bearer FIXTURE_HEADER_SECRET"}}}}"#,
    )
    .unwrap();
    std::fs::write(
        home.path().join(".codex/config.toml"),
        "[mcp_servers.gl]\ncommand = \"npx\"\nargs = [\"-y\", \"gl-mcp\"]\n[mcp_servers.gl.env]\nGITLAB_TOKEN = \"FIXTURE_ENV_SECRET\"\n",
    )
    .unwrap();
    home
}

const FIXTURES: [&str; 5] = ["FIXTURE_ENV_SECRET", "FIXTURE_HEADER_SECRET", "FIXTURE_ARG_SECRET", "FIXTURE_URL_SECRET", "FIXTURE_EXTRA_SECRET"];

#[test]
fn mcp_list_json_masks_env_and_headers() {
    let home = mcp_home();
    let (code, out, _) = go(&["mcp", "list", "--json"], &mut ctx(home.path()));
    assert_eq!(code, 0);
    for secret in FIXTURES {
        assert!(!out.contains(secret), "{secret} leaked: {out}");
    }
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let gl = v["servers"].as_array().unwrap().iter().find(|s| s["name"] == "gl").unwrap();
    assert_eq!(gl["agents"], serde_json::json!(["codex", "cursor"]));
    assert_eq!(gl["status"], "differs");
    assert_eq!(gl["inherited"], false);
    assert_eq!(gl["transport"], "stdio");
    assert_eq!(gl["disabled_for"], serde_json::json!([]));
    let web = v["servers"].as_array().unwrap().iter().find(|s| s["name"] == "web").unwrap();
    assert_eq!(web["transport"], "http");
    assert_eq!(web["status"], "in_sync");
    assert!(v["merges"].as_array().unwrap().is_empty() && v["errors"].as_array().unwrap().is_empty());
}

#[test]
fn mcp_list_human_has_one_line_per_server() {
    let home = mcp_home();
    let (code, out, _) = go(&["mcp", "list"], &mut ctx(home.path()));
    assert_eq!(code, 0);
    let line = out.lines().find(|l| l.starts_with("gl")).unwrap();
    assert!(line.contains("codex") && line.contains("cursor") && line.contains("differs"), "{line}");
    assert!(out.lines().any(|l| l.starts_with("web") && l.contains("in sync")));
    assert!(FIXTURES.iter().all(|s| !out.contains(s)));
}

#[test]
fn mcp_show_masks_by_default() {
    let home = mcp_home();
    for json in [true, false] {
        let args: &[&str] = if json { &["mcp", "show", "gl", "--json"] } else { &["mcp", "show", "gl"] };
        let (code, out, _) = go(args, &mut ctx(home.path()));
        assert_eq!(code, 0);
        for secret in FIXTURES {
            assert!(!out.contains(secret), "{secret} leaked: {out}");
        }
        assert!(out.contains("${GITLAB_HOST}") && out.contains("--api-key=***"), "{out}");
    }
    let (_, out, _) = go(&["mcp", "show", "gl", "--json"], &mut ctx(home.path()));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["name"], "gl");
    let cursor = v["copies"].as_array().unwrap().iter().find(|c| c["agent"] == "cursor").unwrap();
    assert_eq!(cursor["place"], "global");
    assert_eq!(cursor["enabled"], true);
    assert!(cursor["read_only"].is_null());
    assert_eq!(cursor["file"], "~/.cursor/mcp.json");
    assert_eq!(cursor["transport"]["env"]["GITLAB_TOKEN"], "***");
    assert_eq!(cursor["extra_keys"], serde_json::json!(["extraSecret"]));
    assert!(v["diffs"].as_array().unwrap().iter().any(|d| d["field"] == "args"));
}

#[test]
fn mcp_show_masks_headers_and_url() {
    let home = mcp_home();
    let (_, out, _) = go(&["mcp", "show", "web", "--json"], &mut ctx(home.path()));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let t = &v["copies"][0]["transport"];
    assert_eq!(t["type"], "http");
    assert_eq!(t["headers"]["Authorization"], "***");
    assert_eq!(t["url"], "https://***@mcp.example.com/mcp");
}

#[test]
fn tokens_in_url_paths_are_masked_in_show_and_in_merge_hints() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".cursor")).unwrap();
    let url = "https://actions.zapier.com/mcp/sk-ak-FIXTURE123PATH456SECRET/sse";
    std::fs::write(home.path().join(".cursor/mcp.json"), format!(r#"{{"mcpServers":{{"zapier":{{"url":"{url}"}},"zap":{{"url":"{url}"}}}}}}"#)).unwrap();
    let mut c = ctx(home.path());
    for args in [&["mcp", "list"][..], &["mcp", "list", "--json"], &["mcp", "show", "zapier"], &["mcp", "show", "zapier", "--json"]] {
        let (code, out, _) = go(args, &mut c);
        assert_eq!(code, 0, "{args:?}");
        assert!(!out.contains("FIXTURE123PATH456SECRET"), "{args:?}: {out}");
        assert!(out.contains("https://actions.zapier.com/mcp/***/sse"), "{args:?}: {out}");
    }
}

#[test]
fn mcp_show_secrets_reveals_values() {
    let home = mcp_home();
    let (code, out, _) = go(&["mcp", "show", "gl", "web", "--show-secrets", "--json"], &mut ctx(home.path()));
    assert_eq!(code, 2, "one name only");
    assert!(out.is_empty());
    let (code, out, _) = go(&["mcp", "show", "gl", "--show-secrets"], &mut ctx(home.path()));
    assert_eq!(code, 0);
    for secret in ["FIXTURE_ENV_SECRET", "FIXTURE_ARG_SECRET"] {
        assert!(out.contains(secret), "{secret} missing: {out}");
    }
    assert!(out.contains("${GITLAB_HOST}"));
    assert!(!out.contains("FIXTURE_EXTRA_SECRET"), "extras are never printed");
    let (_, out, _) = go(&["mcp", "show", "web", "--show-secrets", "--json"], &mut ctx(home.path()));
    assert!(out.contains("Bearer FIXTURE_HEADER_SECRET") && out.contains("FIXTURE_URL_SECRET"));
}

#[test]
fn mcp_show_says_when_only_hidden_values_differ() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".cursor")).unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::write(home.path().join(".cursor/mcp.json"), r#"{"mcpServers":{"f":{"command":"npx","args":["f-mcp","--api-key=FIXTURE_A1"]}}}"#).unwrap();
    std::fs::write(home.path().join(".codex/config.toml"), "[mcp_servers.f]\ncommand = \"npx\"\nargs = [\"f-mcp\", \"--api-key=FIXTURE_B2\"]\n").unwrap();
    let (code, out, _) = go(&["mcp", "show", "f"], &mut ctx(home.path()));
    assert_eq!(code, 0);
    assert!(out.contains("only hidden values differ") && !out.contains("FIXTURE_"), "{out}");
    let (_, out, _) = go(&["mcp", "show", "f", "--json"], &mut ctx(home.path()));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["diffs"][0]["hidden_only"], true);
    let (_, out, _) = go(&["mcp", "show", "f", "--show-secrets"], &mut ctx(home.path()));
    assert!(out.contains("f-mcp --api-key=FIXTURE_A1") && out.contains("f-mcp --api-key=FIXTURE_B2"), "{out}");
    assert!(!out.contains("only hidden values differ"));
}

#[test]
fn mcp_show_unknown_is_exit_1() {
    let home = mcp_home();
    let (code, out, err) = go(&["mcp", "show", "nope"], &mut ctx(home.path()));
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(err.contains("nope"));
}

#[test]
fn mcp_list_reports_unreadable_files_without_their_content() {
    let home = mcp_home();
    std::fs::write(home.path().join(".codex/config.toml"), "[mcp_servers.gl\ncommand = \"FIXTURE_ENV_SECRET\"\n").unwrap();
    let (code, out, _) = go(&["mcp", "list", "--json"], &mut ctx(home.path()));
    assert_eq!(code, 0);
    assert!(!out.contains("FIXTURE_ENV_SECRET"), "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let e = &v["errors"][0];
    assert_eq!(e["file"], "~/.codex/config.toml");
    assert!(e["error"].as_str().unwrap().contains("line 1"), "{e}");
    let (_, human, _) = go(&["mcp", "list"], &mut ctx(home.path()));
    assert!(human.contains("~/.codex/config.toml") && !human.contains("FIXTURE_ENV_SECRET"));
}

#[test]
fn mcp_list_in_a_project_marks_inherited_rows() {
    let home = mcp_home();
    let project = tempfile::tempdir().unwrap();
    let (code, out, _) = go(&["mcp", "list", "--json", "--project", project.path().to_str().unwrap()], &mut ctx(home.path()));
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["servers"].as_array().unwrap().iter().all(|s| s["inherited"] == true));
    let (_, human, _) = go(&["mcp", "list", "--project", project.path().to_str().unwrap()], &mut ctx(home.path()));
    assert!(human.contains("(global)"));
}

#[test]
fn mcp_list_names_an_agent_once_per_row() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(project.path()).unwrap();
    let entry = r#"{"command":"npx","args":["x-mcp"]}"#;
    std::fs::write(home.path().join(".claude.json"), format!(r#"{{"projects":{{{}:{{"mcpServers":{{"x":{entry}}}}}}}}}"#, serde_json::to_string(&root).unwrap())).unwrap();
    std::fs::write(root.join(".mcp.json"), format!(r#"{{"mcpServers":{{"x":{entry}}}}}"#)).unwrap();
    let (code, out, _) = go(&["mcp", "list", "--project", root.to_str().unwrap()], &mut ctx(home.path()));
    assert_eq!(code, 0);
    let line = out.lines().find(|l| l.starts_with('x')).unwrap();
    assert_eq!(line.matches("claude").count(), 1, "{line}");
}

/// A home where Cursor and Codex exist but hold no servers yet.
fn empty_mcp_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".cursor")).unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    home
}

fn agents_of(out: &str, name: &str) -> Vec<String> {
    let v = json(out);
    let row = v["servers"].as_array().unwrap().iter().find(|s| s["name"] == name).unwrap().clone();
    row["agents"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect()
}

#[test]
fn a_linked_cursor_config_stays_a_link_through_add_and_undo() {
    let home = empty_mcp_home();
    let root = std::fs::canonicalize(home.path()).unwrap();
    let dotfiles = root.join("dotfiles/cursor-mcp.json");
    std::fs::create_dir_all(dotfiles.parent().unwrap()).unwrap();
    std::fs::write(&dotfiles, r#"{"mcpServers":{}}"#).unwrap();
    let link = root.join(".cursor/mcp.json");
    std::os::unix::fs::symlink(&dotfiles, &link).unwrap();
    let mut c = ctx(&root);
    let is_link = || std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink();

    let (code, out, err) = go(&["mcp", "add", "plata", "--command", "npx", "--to", "cursor"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    assert!(is_link(), "the add kept the link");
    assert!(std::fs::read_to_string(&dotfiles).unwrap().contains("plata"));

    assert_eq!(go(&["undo"], &mut c).0, 0);
    assert!(is_link(), "the undo kept the link");
    assert_eq!(std::fs::read_to_string(&dotfiles).unwrap(), r#"{"mcpServers":{}}"#);
}

#[test]
fn add_then_list_shows_server_in_both_agents() {
    let home = empty_mcp_home();
    let mut c = ctx(home.path());
    let (code, out, err) = go(&["mcp", "add", "plata", "--command", "npx", "--arg", "-y", "--arg", "plata-mcp", "--to", "cursor", "codex"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("Add plata"), "{out}");
    let (_, list, _) = go(&["mcp", "list", "--json"], &mut c);
    let mut agents = agents_of(&list, "plata");
    agents.sort();
    assert_eq!(agents, ["codex", "cursor"]);
    let (_, undo, _) = go(&["undo"], &mut c);
    assert!(undo.contains("add plata"), "{undo}");
}

#[test]
fn env_from_file_is_read_and_masked() {
    let home = empty_mcp_home();
    let secret = home.path().join("token.txt");
    std::fs::write(&secret, "FIXTURE_FILE_SECRET\n").unwrap();
    let mut c = ctx(home.path());
    let arg = format!("TOKEN=@{}", secret.display());
    let (code, out, err) = go(&["mcp", "add", "plata", "--command", "npx", "--env", &arg, "--env", "HOST=${PLATA_HOST}", "--to", "cursor"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    assert!(!out.contains("FIXTURE_FILE_SECRET") && !err.contains("FIXTURE_FILE_SECRET"));
    let written: serde_json::Value = json(&std::fs::read_to_string(home.path().join(".cursor/mcp.json")).unwrap());
    assert_eq!(written["mcpServers"]["plata"]["env"]["TOKEN"], "FIXTURE_FILE_SECRET");
    assert_eq!(written["mcpServers"]["plata"]["env"]["HOST"], "${env:PLATA_HOST}");
    let (_, shown, _) = go(&["mcp", "show", "plata"], &mut c);
    assert!(shown.contains("TOKEN = ***") && shown.contains("${PLATA_HOST}") && !shown.contains("FIXTURE_FILE_SECRET"), "{shown}");
}

#[test]
fn env_from_stdin_takes_all_of_it_or_several_values_apart_by_nul() {
    let home = empty_mcp_home();
    let mut c = ctx(home.path());
    c.stdin = Some(Box::new(std::io::Cursor::new("FIXTURE_STDIN_SECRET\n")));
    let (code, out, err) = go(&["mcp", "add", "plata", "--command", "npx", "--env", "TOKEN=-", "--to", "cursor"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    let written = std::fs::read_to_string(home.path().join(".cursor/mcp.json")).unwrap();
    assert!(written.contains("\"FIXTURE_STDIN_SECRET\""), "{written}");

    // Two `KEY=-` and one value: refused, naming no value.
    let home = empty_mcp_home();
    let mut c = ctx(home.path());
    c.stdin = Some(Box::new(std::io::Cursor::new("FIXTURE_STDIN_SECRET\n")));
    let (code, out, err) = go(&["mcp", "add", "plata", "--command", "npx", "--env", "A=-", "--env", "B=-", "--to", "cursor"], &mut c);
    assert_eq!(code, 2, "{out}{err}");
    assert!(err.contains("stdin holds 1 values for 2") && !err.contains("FIXTURE"), "{err}");
    assert!(!home.path().join(".cursor/mcp.json").exists());

    // Two values apart by a NUL, each read as typed: a reference stays one.
    c.stdin = Some(Box::new(std::io::Cursor::new("FIXTURE_A_SECRET\0${PLATA_HOST}\n")));
    let (code, out, err) = go(&["mcp", "add", "plata", "--command", "npx", "--env", "A=-", "--env", "B=-", "--to", "cursor"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    let written = json(&std::fs::read_to_string(home.path().join(".cursor/mcp.json")).unwrap());
    assert_eq!(written["mcpServers"]["plata"]["env"]["A"], "FIXTURE_A_SECRET");
    assert_eq!(written["mcpServers"]["plata"]["env"]["B"], "${env:PLATA_HOST}");
}

#[test]
fn edit_clears_the_args_and_renames_a_header_by_case() {
    let home = mcp_home();
    let mut c = ctx(home.path());
    let (code, out, err) = go(&["mcp", "edit", "gl", "--no-args", "--apply"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    let cursor = |h: &std::path::Path| json(&std::fs::read_to_string(h.join(".cursor/mcp.json")).unwrap());
    assert!(cursor(home.path())["mcpServers"]["gl"].get("args").is_none_or(|a| a.as_array().is_some_and(|a| a.is_empty())), "{}", cursor(home.path()));

    // The unset goes first, so the header renamed by case stays.
    c.stdin = Some(Box::new(std::io::Cursor::new("Bearer FIXTURE_NEW_HEADER")));
    let (code, out, err) = go(&["mcp", "edit", "web", "--unset-header", "Authorization", "--header", "authorization=-", "--apply"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    let headers = cursor(home.path())["mcpServers"]["web"]["headers"].clone();
    assert_eq!(headers, serde_json::json!({ "authorization": "Bearer FIXTURE_NEW_HEADER" }));
}

#[test]
fn definition_on_stdin_adds_and_edits_with_nothing_in_argv() {
    use crate::mcp::model::{EnvValue, Transport};
    let home = empty_mcp_home();
    let mut c = ctx(home.path());
    let url = "https://mcp.example.com/mcp?key=FIXTURE_URL_TOKEN";
    let t = Transport::Http { url: url.into(), headers: [("X-Key".to_string(), EnvValue::literal("FIXTURE_HEADER_TOKEN"))].into(), sse: false };
    c.stdin = Some(Box::new(std::io::Cursor::new(serde_json::to_vec(&t).unwrap())));
    let (code, out, err) = go(&["mcp", "add", "web", "--definition", "-", "--to", "cursor", "--json"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    let cursor = |h: &std::path::Path| json(&std::fs::read_to_string(h.join(".cursor/mcp.json")).unwrap());
    assert_eq!(cursor(home.path())["mcpServers"]["web"]["url"], url);
    assert_eq!(cursor(home.path())["mcpServers"]["web"]["headers"]["X-Key"], "FIXTURE_HEADER_TOKEN");

    // An edit takes what differs from the copy: here, the URL only.
    let edited = Transport::Http { url: "https://mcp.example.com/mcp?key=FIXTURE_NEW_TOKEN".into(), headers: [("X-Key".to_string(), EnvValue::literal("FIXTURE_HEADER_TOKEN"))].into(), sse: false };
    let body = serde_json::to_vec(&edited).unwrap();
    c.stdin = Some(Box::new(std::io::Cursor::new(body.clone())));
    let (code, out, _) = go(&["mcp", "edit", "web", "--definition", "-", "--json"], &mut c);
    assert_eq!(code, 0, "{out}");
    let plan = json(&out);
    assert_eq!(plan["actions"].as_array().unwrap().len(), 1, "{out}");
    assert!(!out.contains("FIXTURE"), "the plan prints no value: {out}");
    c.stdin = Some(Box::new(std::io::Cursor::new(body)));
    let (code, out, err) = go(&["mcp", "edit", "web", "--definition", "-", "--apply", "--plan-id", plan["plan_id"].as_str().unwrap(), "--json"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(cursor(home.path())["mcpServers"]["web"]["url"], "https://mcp.example.com/mcp?key=FIXTURE_NEW_TOKEN");

    // A definition that can't be read is named, not quoted.
    c.stdin = Some(Box::new(std::io::Cursor::new("{FIXTURE_BROKEN")));
    let (code, _, err) = go(&["mcp", "edit", "web", "--definition", "-"], &mut c);
    assert_eq!(code, 2);
    assert!(err.contains("can't be read") && !err.contains("FIXTURE"), "{err}");
    let (code, _, _) = go(&["mcp", "edit", "web", "--definition", "-", "--url", "https://x"], &mut c);
    assert_eq!(code, 2, "--definition goes alone");
}

#[test]
fn sync_to_agents_alone_only_moves_into_agents() {
    let home = tempfile::tempdir().unwrap();
    let mut c = unsynced(home.path());
    let (code, out, err) = go(&["skills", "sync", "review", "--to", "agents", "--apply", "--json"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(json(&out)["done"], 1, "{out}");
    assert!(home.path().join(".agents/skills/review/SKILL.md").is_file());
    assert!(std::fs::symlink_metadata(home.path().join(".claude/skills/review")).unwrap().file_type().is_symlink());
    assert!(!home.path().join(".codex/skills/review").exists(), "no target was linked");
}

#[test]
fn unreadable_env_file_names_the_path_only() {
    let home = empty_mcp_home();
    let (code, out, err) = go(&["mcp", "add", "plata", "--command", "npx", "--env", "TOKEN=@/nonexistent/FIXTURE_PATH", "--to", "cursor"], &mut ctx(home.path()));
    assert_eq!(code, 2, "{out}{err}");
    assert!(err.contains("/nonexistent/FIXTURE_PATH"));
}

#[test]
fn copy_without_apply_changes_nothing() {
    let home = mcp_home();
    let before = std::fs::read_to_string(home.path().join(".codex/config.toml")).unwrap();
    let mut c = ctx(home.path());
    let (code, out, err) = go(&["mcp", "copy", "web", "--to", "codex", "--json"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    let v = json(&out);
    assert_eq!(v["applied"], false);
    assert!(!v["actions"].as_array().unwrap().is_empty());
    assert!(v["skipped"].is_array() && v["warnings"].is_array());
    assert_eq!(std::fs::read_to_string(home.path().join(".codex/config.toml")).unwrap(), before);
    assert!(!out.contains("FIXTURE_"), "{out}");
    let id = v["plan_id"].as_str().unwrap();
    let (code, out, err) = go(&["mcp", "copy", "web", "--to", "codex", "--apply", "--plan-id", id, "--json"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(json(&out)["applied"], true);
    assert!(std::fs::read_to_string(home.path().join(".codex/config.toml")).unwrap().contains("[mcp_servers.web]"));
    let (_, undo, _) = go(&["undo"], &mut c);
    assert!(undo.contains("copy web"), "{undo}");
    assert_eq!(std::fs::read_to_string(home.path().join(".codex/config.toml")).unwrap(), before);
}

#[test]
fn plan_id_without_apply_is_usage_error() {
    let home = mcp_home();
    let (code, _, _) = go(&["mcp", "remove", "gl", "--plan-id", "abc"], &mut ctx(home.path()));
    assert_eq!(code, 2);
}

#[test]
fn stale_plan_id_exits_4() {
    let home = mcp_home();
    let mut c = ctx(home.path());
    let (_, out, _) = go(&["mcp", "remove", "gl", "--json"], &mut c);
    let old = json(&out)["plan_id"].as_str().unwrap().to_string();
    let cursor = home.path().join(".cursor/mcp.json");
    let text = std::fs::read_to_string(&cursor).unwrap().replace("gl-mcp", "gl-mcp2");
    std::fs::write(&cursor, &text).unwrap();
    let (code, out, _) = go(&["mcp", "remove", "gl", "--apply", "--plan-id", &old, "--json"], &mut c);
    assert_eq!(code, 4, "{out}");
    let v = json(&out);
    assert_eq!(v["applied"], false);
    assert_eq!(v["plan_changed"], true);
    assert_ne!(v["plan_id"], old.as_str());
    assert_eq!(std::fs::read_to_string(&cursor).unwrap(), text);
}

#[test]
fn remove_edit_and_merge_apply_and_label_the_change() {
    let home = mcp_home();
    let mut c = ctx(home.path());
    let (code, out, err) = go(&["mcp", "edit", "gl", "--env", "HOST=${GL_HOST}", "--unset-env", "GITLAB_TOKEN", "--apply"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    let (_, undo, _) = go(&["undo"], &mut c);
    assert!(undo.contains("edit gl"), "{undo}");
    let (code, out, err) = go(&["mcp", "remove", "gl", "--from", "codex", "--apply"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    assert!(!std::fs::read_to_string(home.path().join(".codex/config.toml")).unwrap().contains("mcp_servers.gl"));
    let (_, undo, _) = go(&["undo"], &mut c);
    assert!(undo.contains("remove gl"), "{undo}");
}

#[test]
fn a_refused_mcp_plan_prints_json_error() {
    let home = empty_mcp_home();
    let (code, out, err) = go(&["mcp", "remove", "nosuch", "--json"], &mut ctx(home.path()));
    assert_eq!(code, 1);
    assert!(err.contains("no server nosuch"), "{err}");
    assert_eq!(json(&out)["error"], "no server nosuch");
}

#[test]
fn adding_a_server_every_agent_has_is_nothing_to_do() {
    let home = empty_mcp_home();
    let mut c = ctx(home.path());
    let add = ["mcp", "add", "plata", "--command", "npx", "--to", "cursor", "codex"];
    assert_eq!(go(&add, &mut c).0, 0);
    let (code, out, err) = go(&add, &mut c);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.starts_with("Nothing to do"), "{out}");
    // An agent that isn't installed is a real skip: the add failed.
    let (code, out, _) = go(&["mcp", "add", "plata", "--command", "npx", "--to", "cursor", "gemini"], &mut c);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("isn't installed"), "{out}");
}

#[test]
fn copy_help_says_when_from_is_needed() {
    let home = empty_mcp_home();
    let (code, out, _) = go(&["mcp", "copy", "--help"], &mut ctx(home.path()));
    assert_eq!(code, 0);
    assert!(out.contains("Needed when the copies differ"), "{out}");
}

#[test]
fn unknown_agent_is_usage_error_listing_ids() {
    let home = empty_mcp_home();
    let (code, _, err) = go(&["mcp", "add", "x", "--command", "npx", "--to", "nope"], &mut ctx(home.path()));
    assert_eq!(code, 2);
    assert!(err.contains("nope") && err.contains("cursor") && err.contains("codex"), "{err}");
}

#[test]
fn disable_without_project_is_usage_error() {
    let home = mcp_home();
    let (code, _, err) = go(&["mcp", "disable", "gl"], &mut ctx(home.path()));
    assert_eq!(code, 2, "{err}");
    let (code, _, _) = go(&["mcp", "enable", "gl", "--apply"], &mut ctx(home.path()));
    assert_eq!(code, 2);
}

#[test]
fn errors_never_echo_values() {
    let home = mcp_home();
    let all = |code: i32, out: String, err: String| {
        assert_ne!(code, 0, "{out}{err}");
        assert!(!out.contains("FIXTURE_SECRET_V") && !err.contains("FIXTURE_SECRET_V"), "{out}{err}");
    };
    let cases: [&[&str]; 6] = [
        &["mcp", "add", "n", "--command", "npx", "--env", "TOKEN=FIXTURE_SECRET_V", "--to", "nope"],
        &["mcp", "add", "n", "--command", "npx", "--env", "FIXTURE_SECRET_V", "--to", "cursor"],
        &["mcp", "add", "n", "--command", "npx", "--env", "=FIXTURE_SECRET_V", "--to", "cursor"],
        &["mcp", "add", "n", "--command", "npx", "--url", "https://x", "--env", "T=FIXTURE_SECRET_V", "--to", "cursor"],
        &["mcp", "add", "n", "--url", "https://x", "--header", "Authorization=FIXTURE_SECRET_V", "--header", "NOEQUALS", "--to", "cursor"],
        &["mcp", "edit", "missing", "--env", "T=FIXTURE_SECRET_V", "--apply"],
    ];
    for args in cases {
        let (code, out, err) = go(args, &mut ctx(home.path()));
        all(code, out, err);
    }
    // The failure inside the core (an existing server) must not echo either.
    let (code, out, err) = go(&["mcp", "add", "gl", "--command", "npx", "--env", "T=FIXTURE_SECRET_V", "--to", "cursor", "--json"], &mut ctx(home.path()));
    assert!(!out.contains("FIXTURE_SECRET_V") && !err.contains("FIXTURE_SECRET_V"), "{code} {out}{err}");
}

// prism doctor

fn skill_dir(root: &std::path::Path, name: &str, manifest: &str) {
    let d = root.join(".agents/skills").join(name);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("SKILL.md"), manifest).unwrap();
}

#[test]
fn clean_home_exits_0() {
    let home = tempfile::tempdir().unwrap();
    skill_dir(home.path(), "good", "---\nname: good\ndescription: fine\n---\nbody\n");
    let (code, out, _) = go(&["doctor"], &mut ctx(home.path()));
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.trim(), "No problems found.");
    let (code, out, _) = go(&["doctor", "--json"], &mut ctx(home.path()));
    assert_eq!(code, 0);
    assert_eq!(json(&out), serde_json::json!({ "problems": [] }));
}

#[test]
fn broken_link_and_drift_exit_3() {
    let home = mcp_home();
    let h = home.path();
    skill_dir(h, "nameless", "---\ndescription: x\n---\n");
    skill_dir(h, "mute", "---\nname: mute\n---\n");
    std::fs::create_dir_all(h.join(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(h.join(".agents/skills/ghost"), h.join(".claude/skills/ghost")).unwrap();
    std::fs::write(h.join(".codex/bad.toml"), "").unwrap();

    let (code, out, _) = go(&["doctor", "--json"], &mut ctx(h));
    assert_eq!(code, 3, "{out}");
    for secret in FIXTURES {
        assert!(!out.contains(secret), "{secret} leaked: {out}");
    }
    let v = json(&out);
    let ps = v["problems"].as_array().unwrap();
    let find = |kind: &str, subject: &str| ps.iter().find(|p| p["kind"] == kind && p["subject"] == subject).unwrap_or_else(|| panic!("{kind} {subject}: {out}")).clone();
    assert!(ps.iter().all(|p| p["scope"] == "global"));
    let link = find("broken_link", "ghost");
    let detail = link["detail"].as_str().unwrap();
    assert!(detail.contains("Claude Code") && detail.contains("~/.agents/skills/ghost"), "{detail}");
    assert!(find("invalid_skill", "nameless")["detail"].as_str().unwrap().contains("name"));
    assert!(find("invalid_skill", "mute")["detail"].as_str().unwrap().contains("description"));
    let drift = find("mcp_differs", "gl");
    assert!(drift["detail"].as_str().unwrap().contains("args"), "{drift}");

    let (code, out, _) = go(&["doctor"], &mut ctx(h));
    assert_eq!(code, 3);
    assert!(out.contains("global") && out.contains("ghost") && out.contains("gl"), "{out}");
}

#[test]
fn unreadable_mcp_file_is_a_problem_with_a_tilde_subject() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".cursor")).unwrap();
    std::fs::write(home.path().join(".cursor/mcp.json"), "{ FIXTURE_FILE_TEXT").unwrap();
    let (code, out, _) = go(&["doctor", "--json"], &mut ctx(home.path()));
    assert_eq!(code, 3, "{out}");
    let p = &json(&out)["problems"][0];
    assert_eq!(p["kind"], "mcp_unreadable");
    assert_eq!(p["subject"], "~/.cursor/mcp.json");
    assert!(!out.contains("FIXTURE_FILE_TEXT"));
}

#[test]
fn doctor_all_checks_projects_and_flags_missing_ones() {
    let home = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(home.path()).unwrap();
    let proj = root.join("app");
    std::fs::create_dir_all(proj.join(".agents/skills/odd")).unwrap();
    std::fs::write(proj.join(".agents/skills/odd/SKILL.md"), "---\nname: odd\n---\n").unwrap();
    let mut c = ctx(&root);
    c.settings.projects = vec![proj.clone(), root.join("gone")];

    let (code, out, _) = go(&["doctor", "--json"], &mut c);
    assert_eq!(code, 0, "global alone is clean: {out}");
    let (code, out, _) = go(&["doctor", "--all", "--json"], &mut c);
    assert_eq!(code, 3, "{out}");
    let v = json(&out);
    let ps = v["problems"].as_array().unwrap();
    assert!(ps.iter().any(|p| p["kind"] == "invalid_skill" && p["subject"] == "odd" && p["scope"] == "~/app"), "{out}");
    assert!(ps.iter().any(|p| p["kind"] == "missing_project" && p["scope"] == "~/gone"), "{out}");

    let (code, out, _) = go(&["doctor", "--project", proj.to_str().unwrap(), "--json"], &mut c);
    assert_eq!(code, 3);
    assert_eq!(json(&out)["problems"].as_array().unwrap().len(), 1);
}

#[test]
fn doctor_writes_nothing() {
    let home = mcp_home();
    let snap = |p: &std::path::Path| -> Vec<(String, Vec<u8>)> {
        let mut v = Vec::new();
        let mut stack = vec![p.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let (path, t) = (e.path(), e.file_type().unwrap());
                if t.is_dir() { stack.push(path) } else { v.push((path.display().to_string(), std::fs::read(&path).unwrap_or_default())) }
            }
        }
        v.sort();
        v
    };
    let before = snap(home.path());
    go(&["doctor", "--all"], &mut ctx(home.path()));
    assert_eq!(snap(home.path()), before);
}

// Task 12 follow-ups

#[test]
fn merge_through_clap_labels_and_undoes_byte_for_byte() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".cursor")).unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    let cursor = home.path().join(".cursor/mcp.json");
    let codex = home.path().join(".codex/config.toml");
    std::fs::write(&cursor, r#"{"mcpServers":{"a":{"url":"https://m.example.com/mcp"}}}"#).unwrap();
    std::fs::write(&codex, "[mcp_servers.b]\nurl = \"https://m.example.com/mcp\"\n").unwrap();
    let (cb, xb) = (std::fs::read(&cursor).unwrap(), std::fs::read(&codex).unwrap());
    let mut c = ctx(home.path());
    let (code, out, err) = go(&["mcp", "merge", "a", "b", "--into", "c", "--apply"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    assert_ne!(std::fs::read(&cursor).unwrap(), cb);
    assert!(std::fs::read_to_string(&cursor).unwrap().contains("\"c\""));
    assert!(std::fs::read_to_string(&codex).unwrap().contains("mcp_servers.c"));
    assert_eq!(store::load_last_change(&c.paths).unwrap().label, "merge into c");
    let (code, undo, _) = go(&["undo"], &mut c);
    assert_eq!(code, 0);
    assert!(undo.contains("merge into c"), "{undo}");
    assert_eq!((std::fs::read(&cursor).unwrap(), std::fs::read(&codex).unwrap()), (cb, xb));
}

#[test]
fn disable_in_a_project_applies_and_labels() {
    let home = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(home.path()).unwrap();
    let proj = root.join("app");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(root.join(".claude.json"), r#"{"mcpServers":{"gl":{"command":"npx","args":["gl-mcp"]}}}"#).unwrap();
    let mut c = ctx(&root);
    let (code, out, err) = go(&["mcp", "disable", "gl", "--project", proj.to_str().unwrap(), "--apply"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join(".claude.json")).unwrap()).unwrap();
    assert!(doc["projects"][proj.to_str().unwrap()]["disabledMcpServers"].as_array().unwrap().iter().any(|n| n == "gl"), "{doc}");
    assert_eq!(store::load_last_change(&c.paths).unwrap().label, "disable gl");
}

// prism doctor, fix round 1

fn settings_with(c: &mut Ctx, projects: &[&std::path::Path]) {
    c.settings.projects = projects.iter().map(|p| p.to_path_buf()).collect();
}

#[test]
fn all_reports_an_unreadable_global_file_once() {
    let home = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(home.path()).unwrap();
    std::fs::create_dir_all(root.join(".cursor")).unwrap();
    std::fs::write(root.join(".cursor/mcp.json"), "{ broken").unwrap();
    let (a, b) = (root.join("a"), root.join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let mut c = ctx(&root);
    settings_with(&mut c, &[&a, &b]);
    let (code, out, _) = go(&["doctor", "--all", "--json"], &mut c);
    assert_eq!(code, 3, "{out}");
    let v = json(&out);
    let hits: Vec<_> = v["problems"].as_array().unwrap().iter().filter(|p| p["kind"] == "mcp_unreadable").collect();
    assert_eq!(hits.len(), 1, "{out}");
    assert_eq!(hits[0]["scope"], "global");
    // A single project still shows the global file it reads.
    let (_, out, _) = go(&["doctor", "--project", a.to_str().unwrap(), "--json"], &mut c);
    assert_eq!(json(&out)["problems"].as_array().unwrap().iter().filter(|p| p["kind"] == "mcp_unreadable").count(), 1, "{out}");
}

#[test]
fn inherited_drift_is_global_under_all_and_prefixed_under_one_project() {
    let home = mcp_home();
    let root = std::fs::canonicalize(home.path()).unwrap();
    let proj = root.join("app");
    std::fs::create_dir_all(&proj).unwrap();
    let mut c = ctx(&root);
    settings_with(&mut c, &[&proj]);

    let (_, out, _) = go(&["doctor", "--all", "--json"], &mut c);
    let v = json(&out);
    let gl: Vec<_> = v["problems"].as_array().unwrap().iter().filter(|p| p["kind"] == "mcp_differs" && p["subject"] == "gl").collect();
    assert_eq!(gl.len(), 1, "{out}");
    assert_eq!(gl[0]["scope"], "global");
    assert!(!gl[0]["detail"].as_str().unwrap().starts_with("global: "));

    let (code, out, _) = go(&["doctor", "--project", proj.to_str().unwrap(), "--json"], &mut c);
    assert_eq!(code, 3);
    let v = json(&out);
    let gl = v["problems"].as_array().unwrap().iter().find(|p| p["kind"] == "mcp_differs" && p["subject"] == "gl").unwrap().clone();
    assert!(gl["detail"].as_str().unwrap().starts_with("global: args"), "{gl}");
    assert_eq!(gl["scope"], "~/app");
}

#[test]
fn a_mixed_row_is_reported_unprefixed_in_both_modes() {
    let home = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(home.path()).unwrap();
    std::fs::create_dir_all(root.join(".codex")).unwrap();
    std::fs::write(root.join(".codex/config.toml"), "[mcp_servers.gl]\ncommand = \"npx\"\n").unwrap();
    let proj = root.join("app");
    std::fs::create_dir_all(proj.join(".cursor")).unwrap();
    std::fs::create_dir_all(root.join(".cursor")).unwrap();
    std::fs::write(proj.join(".cursor/mcp.json"), r#"{"mcpServers":{"gl":{"command":"uvx"}}}"#).unwrap();
    let mut c = ctx(&root);
    settings_with(&mut c, &[&proj]);
    for args in [&["doctor", "--all", "--json"][..], &["doctor", "--project", proj.to_str().unwrap(), "--json"][..]] {
        let (code, out, _) = go(args, &mut c);
        assert_eq!(code, 3, "{out}");
        let v = json(&out);
        let gl = v["problems"].as_array().unwrap().iter().find(|p| p["kind"] == "mcp_differs" && p["subject"] == "gl").unwrap_or_else(|| panic!("{out}")).clone();
        assert!(!gl["detail"].as_str().unwrap().starts_with("global: "), "{gl}");
        assert_eq!(gl["scope"], "~/app");
    }
}

/// A home with Claude Code and Codex installed and `~/.local/bin` on `PATH`.
fn setup_home(h: &std::path::Path) -> Ctx {
    std::fs::create_dir_all(h.join(".claude")).unwrap();
    std::fs::create_dir_all(h.join(".codex")).unwrap();
    let mut c = ctx(h);
    c.path_var = Some(std::env::join_paths([h.join(".local/bin"), "/usr/bin".into()]).unwrap());
    c
}

fn this_binary() -> std::path::PathBuf {
    std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap()
}

fn installed_skill(h: &std::path::Path) -> String {
    std::fs::read_to_string(h.join(".agents/skills/prism/SKILL.md")).unwrap()
}

#[test]
fn setup_installs_skill_and_links_it() {
    let home = tempfile::tempdir().unwrap();
    let h = &std::fs::canonicalize(home.path()).unwrap();
    let mut c = setup_home(h);

    let (code, out, err) = go(&["setup", "--json"], &mut c);
    let v = json(&out);
    assert_eq!(v["skill"]["status"], "installed", "{out}{err}");
    assert_eq!(v["skill"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["skill"]["linked"], 2);
    let text = installed_skill(h);
    let m = crate::skill::parse(&text);
    assert_eq!(m.name.as_deref(), Some("prism"));
    assert_eq!(crate::skill::metadata(&text, "prism-version").as_deref(), Some(env!("CARGO_PKG_VERSION")));
    for agent in [".claude/skills/prism", ".codex/skills/prism"] {
        assert!(std::fs::symlink_metadata(h.join(agent)).unwrap().file_type().is_symlink(), "{agent}");
        assert_eq!(std::fs::canonicalize(h.join(agent)).unwrap(), h.join(".agents/skills/prism"));
    }
    assert_eq!(store::load_last_change(&c.paths).unwrap().label, "install prism skill");

    let link = h.join(".local/bin/prism");
    if cfg!(target_os = "macos") {
        assert_eq!(code, 0, "{out}{err}");
        assert_eq!(v["cli"]["status"], "installed");
        assert_eq!(v["cli"]["path"], link.display().to_string());
        assert_eq!(std::fs::read_link(&link).unwrap(), this_binary());
    } else {
        // Linux only checks: the tarball's installer puts prism on PATH, here nothing did.
        assert_eq!(code, 1, "{out}");
        assert_eq!(v["cli"]["status"], "failed");
        assert!(std::fs::symlink_metadata(&link).is_err());
    }

    // Again: nothing left to do, and the last change is still the install.
    let (_, out, _) = go(&["setup", "--json"], &mut c);
    let v = json(&out);
    assert_eq!(v["skill"]["status"], "unchanged", "{out}");
    assert_eq!(v["skill"]["linked"], 0);
    if cfg!(target_os = "macos") {
        assert_eq!(v["cli"]["status"], "unchanged");
    }
    assert_eq!(store::load_last_change(&c.paths).unwrap().label, "install prism skill");

    // Undo takes the links back; the skill stays in .agents.
    assert_eq!(go(&["undo"], &mut c).0, 0);
    assert!(std::fs::symlink_metadata(h.join(".claude/skills/prism")).is_err());
    assert!(h.join(".agents/skills/prism/SKILL.md").is_file());
}

#[test]
fn setup_upgrades_older_skill_only() {
    let ours = env!("CARGO_PKG_VERSION");
    // (installed SKILL.md, expected status, rewritten)
    let cases = [
        ("---\nname: prism\ndescription: old\nmetadata:\n  prism-version: \"0.0.1\"\n---\nold\n", "updated", true),
        ("---\nname: prism\ndescription: old\nmetadata:\n  prism-version: not-a-version\n---\nold\n", "updated", true),
        ("---\nname: prism\ndescription: new\nmetadata:\n  prism-version: \"999.0.0\"\n---\nnewer\n", "unchanged", false),
        ("---\nname: prism\ndescription: mine\n---\nmy own prism skill\n", "skipped", false),
        ("---\nname: prisma\ndescription: theirs\nmetadata:\n  prism-version: \"0.0.1\"\n---\nnot ours\n", "skipped", false),
    ];
    for (before, status, rewritten) in cases {
        let home = tempfile::tempdir().unwrap();
        let h = &std::fs::canonicalize(home.path()).unwrap();
        let mut c = setup_home(h);
        std::fs::create_dir_all(h.join(".agents/skills/prism")).unwrap();
        std::fs::write(h.join(".agents/skills/prism/SKILL.md"), before).unwrap();

        let (code, out, _) = go(&["setup", "--json"], &mut c);
        let v = json(&out);
        assert_eq!(v["skill"]["status"], status, "{before}\n{out}");
        let after = installed_skill(h);
        if rewritten {
            assert_eq!(crate::skill::metadata(&after, "prism-version").as_deref(), Some(ours), "{before}");
        } else {
            assert_eq!(after, before);
        }
        if status == "skipped" {
            // Someone else's skill: not linked anywhere, and setup says it didn't finish.
            assert_eq!(code, 1, "{out}");
            assert!(v["skill"]["message"].as_str().unwrap().contains("prism"), "{out}");
            assert!(std::fs::symlink_metadata(h.join(".claude/skills/prism")).is_err());
        } else {
            assert!(h.join(".claude/skills/prism").exists(), "{before}");
        }
    }
}

#[test]
fn setup_reports_path_problem_without_failing_skill_part() {
    let home = tempfile::tempdir().unwrap();
    let h = &std::fs::canonicalize(home.path()).unwrap();
    let mut c = setup_home(h);
    // Someone else's `prism` program sits where the link would go.
    let theirs = h.join(".local/bin/prism");
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::write(&theirs, "#!/bin/sh\n").unwrap();

    let (code, out, _) = go(&["setup", "--json"], &mut c);
    assert_eq!(code, 1, "{out}");
    let v = json(&out);
    assert_eq!(v["cli"]["status"], "failed");
    let message = v["cli"]["message"].as_str().unwrap();
    assert!(message.contains(&theirs.display().to_string()), "{message}");
    assert_eq!(std::fs::read_to_string(&theirs).unwrap(), "#!/bin/sh\n", "left alone");
    assert_eq!(v["skill"]["status"], "installed");
    assert!(h.join(".claude/skills/prism/SKILL.md").is_file());

    // The same, for people.
    let (code, out, err) = go(&["setup"], &mut c);
    assert_eq!(code, 1);
    assert!(format!("{out}{err}").contains(&theirs.display().to_string()), "{out}{err}");
}

#[test]
fn setup_works_on_the_global_setup_only() {
    let home = tempfile::tempdir().unwrap();
    let h = &std::fs::canonicalize(home.path()).unwrap();
    let mut c = setup_home(h);
    let (code, _, err) = go(&["setup", "--project", &h.display().to_string()], &mut c);
    assert_eq!(code, 2);
    assert!(err.contains("global"), "{err}");
    assert!(!h.join(".agents/skills/prism").exists());
}

#[test]
fn hosts_add_list_remove() {
    let home = tempfile::tempdir().unwrap();
    let mut c = ctx(home.path());
    let (code, out, _) = go(&["hosts", "add", "laptop", "--ssh", "george@laptop", "--json"], &mut c);
    assert_eq!(code, 0);
    assert_eq!(json(&out), serde_json::json!({ "name": "laptop", "ssh": "george@laptop", "added": true }));
    // The settings file round-trips.
    assert_eq!(Settings::load_from(&c.paths).hosts, vec![crate::hosts::Host { name: "laptop".into(), ssh: "george@laptop".into() }]);
    let (code, out, _) = go(&["hosts", "list", "--json"], &mut c);
    assert_eq!(code, 0);
    assert_eq!(json(&out), serde_json::json!([{ "name": "laptop", "ssh": "george@laptop" }]));
    let (code, out, _) = go(&["hosts", "remove", "laptop", "--json"], &mut c);
    assert_eq!(code, 0);
    assert_eq!(json(&out), serde_json::json!({ "name": "laptop", "removed": true }));
    let (_, out, _) = go(&["hosts", "list", "--json"], &mut c);
    assert_eq!(json(&out), serde_json::json!([]));
    assert!(Settings::load_from(&c.paths).hosts.is_empty());
}

#[test]
fn hosts_add_rejects_bad_names() {
    let home = tempfile::tempdir().unwrap();
    let mut c = ctx(home.path());
    let long = "a".repeat(33);
    for name in ["bad name", "local", "this", "", long.as_str()] {
        let (code, _, err) = go(&["hosts", "add", name, "--ssh", "me@box"], &mut c);
        assert_eq!(code, 2, "{name:?}");
        assert!(!err.is_empty(), "{name:?}");
    }
    assert!(c.settings.hosts.is_empty());
    assert!(Settings::load_from(&c.paths).hosts.is_empty());
}

#[test]
fn hosts_add_twice_is_usage_error() {
    let home = tempfile::tempdir().unwrap();
    let mut c = ctx(home.path());
    assert_eq!(go(&["hosts", "add", "box", "--ssh", "me@box"], &mut c).0, 0);
    let (code, _, err) = go(&["hosts", "add", "box", "--ssh", "me@other"], &mut c);
    assert_eq!(code, 2);
    assert!(err.contains("already"), "{err}");
    assert_eq!(c.settings.hosts[0].ssh, "me@box");
}

#[test]
fn hosts_reject_project_flag() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (code, _, _) = go(&["--project", dir.path().to_str().unwrap(), "hosts", "list"], &mut ctx(home.path()));
    assert_eq!(code, 2);
}

// prism --host, hosts check and install

use crate::hosts::install::VERSION;
use crate::hosts::runner::{self, Output, Runner};
use std::sync::{Arc, Mutex};

/// A host that answers the check script with `uname` and `prism <version>` and logs every call.
struct FakeHost {
    version: Option<String>,
    log: Arc<Mutex<Vec<String>>>,
}

impl Runner for FakeHost {
    fn prism(&self, args: &[String], stdin: Option<Vec<u8>>) -> Result<Output, String> {
        self.log.lock().unwrap().push(format!("prism {} stdin={}", args.join(" "), stdin.is_some()));
        Ok(Output { code: 0, stdout: b"remote out\n".to_vec(), stderr: String::new() })
    }
    fn shell(&self, script: &str, _: Option<Vec<u8>>) -> Result<Output, String> {
        self.log.lock().unwrap().push(format!("shell {script}"));
        let mut stdout = String::from("Linux x86_64\n");
        if let Some(v) = &self.version {
            stdout.push_str(&format!("prism {v}\n"));
        }
        Ok(Output { code: 0, stdout: stdout.into_bytes(), stderr: String::new() })
    }
}

/// `ctx` whose runners are `FakeHost`s reporting `version`; the log also names each host asked for.
fn fake_hosts(home: &std::path::Path, version: Option<&str>) -> (Ctx, Arc<Mutex<Vec<String>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut c = ctx(home);
    c.settings.hosts.push(crate::hosts::Host { name: "x".into(), ssh: "me@x".into() });
    let (shared, version) = (log.clone(), version.map(String::from));
    c.make_runner = Arc::new(move |h: &crate::hosts::Host| {
        shared.lock().unwrap().push(format!("runner {} {}", h.name, h.ssh));
        Box::new(FakeHost { version: version.clone(), log: shared.clone() }) as Box<dyn Runner>
    });
    (c, log)
}

/// A `prism` in the remote home's `~/.local/bin` that reports this version, for the check.
fn remote_with_prism() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let remote = tempfile::tempdir().unwrap();
    let bin = remote.path().join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("prism"), format!("#!/bin/sh\necho \"prism {VERSION}\"\n")).unwrap();
    std::fs::set_permissions(bin.join("prism"), std::fs::Permissions::from_mode(0o755)).unwrap();
    remote
}

fn on_local_host(home: &std::path::Path, remote: &std::path::Path) -> Ctx {
    let mut c = ctx(home);
    c.settings.hosts.push(crate::hosts::Host { name: "x".into(), ssh: "me@x".into() });
    c.make_runner = runner::local_runners(&[("x", remote)]);
    c
}

#[test]
fn host_flag_routes_to_the_cli() {
    for args in [&["--host", "x", "skills", "--help"][..], &["--host=x", "--json", "doctor", "--help"]] {
        let argv: Vec<std::ffi::OsString> = args.iter().map(|a| a.into()).collect();
        assert_eq!(run(argv), Some(0), "{args:?}");
    }
    let argv: Vec<std::ffi::OsString> = ["--host", "x"].iter().map(|a| a.into()).collect();
    assert_eq!(run(argv), None);
}

#[test]
fn version_mismatch_refuses_commands() {
    let home = tempfile::tempdir().unwrap();
    let (mut c, log) = fake_hosts(home.path(), Some("0.0.0-other"));
    let (code, out, err) = go(&["--host", "x", "skills", "list"], &mut c);
    assert_eq!(code, 1, "{out}{err}");
    assert_eq!(err.trim(), format!("prism on x is version 0.0.0-other, not {VERSION}; run prism hosts install x"));
    let log = log.lock().unwrap().clone();
    assert_eq!(log.len(), 2, "{log:?}");
    assert_eq!(log[0], "runner x me@x");
    assert!(log[1].starts_with("shell uname -sm\n"), "{log:?}");

    let (mut c, log) = fake_hosts(home.path(), None);
    let (code, out, _) = go(&["--json", "--host", "x", "skills", "list"], &mut c);
    assert_eq!(code, 1);
    assert_eq!(json(&out)["error"], "prism on x is missing; run prism hosts install x");
    assert!(log.lock().unwrap().iter().all(|l| !l.starts_with("prism ")));
}

#[test]
fn host_flag_forwards_and_keeps_exit_code() {
    let remote = remote_with_prism();
    let r = remote.path();
    std::fs::create_dir_all(r.join(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(r.join(".agents/skills/ghost"), r.join(".claude/skills/ghost")).unwrap();
    let (_, direct, _) = go(&["doctor", "--json"], &mut ctx(r));

    let home = tempfile::tempdir().unwrap();
    for args in [&["--host", "x", "doctor", "--json"][..], &["doctor", "--json", "--host=x"]] {
        let (code, out, err) = go(args, &mut on_local_host(home.path(), r));
        assert_eq!(code, 3, "{args:?}: {out}{err}");
        assert_eq!(out, direct, "{args:?}");
        assert!(json(&out)["problems"].as_array().unwrap().iter().any(|p| p["kind"] == "broken_link" && p["subject"] == "ghost"), "{out}");
    }
    // Nothing happened here.
    assert!(!home.path().join(".claude").exists());
}

#[test]
fn host_flag_forwards_stdin_only_for_dash_values() {
    let remote = remote_with_prism();
    std::fs::create_dir_all(remote.path().join(".cursor")).unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut c = on_local_host(home.path(), remote.path());
    c.stdin = Some(Box::new(std::io::Cursor::new("FIXTURE_STDIN_SECRET\n")));
    let (code, out, err) = go(&["--host", "x", "mcp", "add", "plata", "--command", "npx", "--env", "TOKEN=-", "--to", "cursor"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    let written = std::fs::read_to_string(remote.path().join(".cursor/mcp.json")).unwrap();
    assert!(written.contains("\"FIXTURE_STDIN_SECRET\""), "{written}");

    let (mut c, log) = fake_hosts(home.path(), Some(VERSION));
    c.stdin = Some(Box::new(std::io::Cursor::new("unused")));
    let (code, out, _) = go(&["--host", "x", "agents", "--json"], &mut c);
    assert_eq!((code, out.as_str()), (0, "remote out\n"));
    assert_eq!(log.lock().unwrap().last().unwrap(), "prism agents --json stdin=false");
}

#[test]
fn host_flag_does_not_read_stdin_for_a_dash_after_double_dash() {
    let home = tempfile::tempdir().unwrap();
    let (mut c, log) = fake_hosts(home.path(), Some(VERSION));
    c.stdin = Some(Box::new(std::io::Cursor::new("unused")));
    let (code, _, err) = go(&["--host", "x", "mcp", "add", "--command", "cmd", "--to", "claude", "--", "-"], &mut c);
    assert_eq!(code, 0, "{err}");
    assert_eq!(log.lock().unwrap().last().unwrap(), "prism mcp add --command cmd --to claude -- - stdin=false");
    assert!(c.stdin.is_some(), "stdin was not touched");
}

#[test]
fn reserved_host_names_run_here_in_process() {
    let home = tempfile::tempdir().unwrap();
    let (_, plain, _) = go(&["agents", "--json"], &mut ctx(home.path()));
    for name in ["local", "this"] {
        // `ctx` hands out no runner: asking for one (a remote check) would panic.
        let (code, out, err) = go(&["--host", name, "agents", "--json"], &mut ctx(home.path()));
        assert_eq!((code, out.as_str(), err.as_str()), (0, plain.as_str(), ""), "{name}");
    }
}

#[test]
fn host_flag_takes_an_unsaved_ssh_target() {
    let home = tempfile::tempdir().unwrap();
    let (mut c, log) = fake_hosts(home.path(), Some(VERSION));
    let (code, _, _) = go(&["--host", "me@elsewhere", "agents"], &mut c);
    assert_eq!(code, 0);
    assert_eq!(log.lock().unwrap()[0], "runner me@elsewhere me@elsewhere");
}

/// A host that can't be reached.
struct Gone;

impl Runner for Gone {
    fn prism(&self, _: &[String], _: Option<Vec<u8>>) -> Result<Output, String> {
        Err("Can't reach x: timed out".into())
    }
    fn shell(&self, _: &str, _: Option<Vec<u8>>) -> Result<Output, String> {
        Err("Can't reach x: timed out".into())
    }
}

#[test]
fn an_unreachable_host_exits_1() {
    let home = tempfile::tempdir().unwrap();
    let mut c = ctx(home.path());
    c.make_runner = Arc::new(|_: &crate::hosts::Host| Box::new(Gone) as Box<dyn Runner>);
    let (code, _, err) = go(&["--host", "x", "agents"], &mut c);
    assert_eq!(code, 1);
    assert_eq!(err.trim(), "Can't reach x: timed out");
    let (code, out, _) = go(&["hosts", "check", "x", "--json"], &mut c);
    assert_eq!(code, 1);
    assert_eq!(json(&out)["reachable"], false);
}

#[test]
fn hosts_check_reports_health() {
    let home = tempfile::tempdir().unwrap();
    let (mut c, _) = fake_hosts(home.path(), Some(VERSION));
    let (code, out, _) = go(&["hosts", "check", "x", "--json"], &mut c);
    assert_eq!(code, 0, "{out}");
    let want = serde_json::json!({ "name": "x", "reachable": true, "health": "ready", "remote_version": VERSION, "os": "linux", "arch": "x86_64", "why": null });
    assert_eq!(json(&out), want);

    let (mut c, _) = fake_hosts(home.path(), Some("0.0.0-other"));
    let (code, out, _) = go(&["hosts", "check", "x", "--json"], &mut c);
    assert_eq!(code, 3);
    assert_eq!(json(&out)["health"], "other_version");
    assert_eq!(json(&out)["remote_version"], "0.0.0-other");

    let (mut c, _) = fake_hosts(home.path(), None);
    let (code, out, _) = go(&["hosts", "check", "x"], &mut c);
    assert_eq!(code, 3);
    assert!(out.contains("prism on x is missing"), "{out}");
}

#[test]
fn hosts_install_uploads_a_binary() {
    let remote = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    // The upload is a script that prints this version, so the check after it says ready.
    let binary = home.path().join("prism-build");
    std::fs::write(&binary, format!("#!/bin/sh\necho \"prism {VERSION}\"\n")).unwrap();
    let mut c = on_local_host(home.path(), remote.path());
    let (code, out, err) = go(&["hosts", "install", "x", "--binary", binary.to_str().unwrap(), "--json"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(json(&out), serde_json::json!({ "name": "x", "installed": true, "version": VERSION }));
    assert!(remote.path().join(".local/bin/prism").exists());

    // An upload that doesn't report this version is refused before it replaces anything.
    std::fs::write(&binary, "#!/bin/sh\necho \"prism 0.0.0-other\"\n").unwrap();
    let (code, _, err) = go(&["hosts", "install", "x", "--binary", binary.to_str().unwrap()], &mut c);
    assert_eq!(code, 1);
    assert!(err.contains("not prism"), "{err}");
    assert!(std::fs::read_to_string(remote.path().join(".local/bin/prism")).unwrap().contains(VERSION));
}

#[test]
fn hosts_install_leaves_another_program_unless_forced() {
    use std::os::unix::fs::PermissionsExt;
    let remote = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let theirs = remote.path().join(".local/bin/prism");
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::write(&theirs, "#!/bin/sh\necho 'Prism image tools 4.1'\n").unwrap();
    std::fs::set_permissions(&theirs, std::fs::Permissions::from_mode(0o755)).unwrap();
    let binary = home.path().join("prism-build");
    std::fs::write(&binary, format!("#!/bin/sh\necho \"prism {VERSION}\"\n")).unwrap();
    let mut c = on_local_host(home.path(), remote.path());
    let install = ["hosts", "install", "x", "--binary", binary.to_str().unwrap()];
    let (code, _, err) = go(&install, &mut c);
    assert_eq!(code, 1);
    assert_eq!(err.trim(), "Can't install prism on x: ~/.local/bin/prism is another program; move it away, or replace it with prism hosts install --force");
    assert!(std::fs::read_to_string(&theirs).unwrap().contains("image tools"), "left alone");

    let (code, out, err) = go(&[&install[..], &["--force"]].concat(), &mut c);
    assert_eq!(code, 0, "{out}{err}");
    assert!(std::fs::read_to_string(&theirs).unwrap().contains(VERSION));
}

/// `mcp_home()` plus skills in every placement, a registered project, a saved host and a last
/// change: everything the window's state holds.
fn wire_home() -> (tempfile::TempDir, Ctx) {
    let home = mcp_home();
    let root = std::fs::canonicalize(home.path()).unwrap();
    let agents = root.join(".agents/skills");
    let claude = root.join(".claude/skills");
    let codex = root.join(".codex/skills");
    write_skill(&agents, "alpha", "In .agents");
    write_skill(&root.join("elsewhere"), "beta", "Linked from elsewhere");
    std::fs::create_dir_all(&claude).unwrap();
    std::fs::create_dir_all(&codex).unwrap();
    std::os::unix::fs::symlink(agents.join("alpha"), claude.join("alpha")).unwrap();
    std::os::unix::fs::symlink(root.join("elsewhere/beta"), codex.join("beta")).unwrap();
    std::os::unix::fs::symlink("/nowhere/gamma", claude.join("gamma")).unwrap();
    let project = root.join("work/app");
    write_skill(&project.join(".agents/skills"), "local", "Project skill");
    let mut c = ctx(&root);
    c.settings.projects = vec![project];
    c.settings.hosts = vec![crate::hosts::Host { name: "box".into(), ssh: "me@box".into() }];
    store::save_last_change(&c.paths, Some(&store::LastChange { label: "Sync 2 skills".into(), journal: Default::default() }));
    (home, c)
}

#[test]
fn wire_state_round_trips() {
    use crate::scan::Placement;
    let (_home, mut c) = wire_home();
    let (code, out, err) = go(&["wire", "state"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    let got: crate::wire::WireState = serde_json::from_str(&out).unwrap();
    let want = crate::wire::state(&Scope::Global, &c.paths, &c.settings);
    assert_eq!(got.version, want.version);
    assert_eq!(got.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(got.paths, want.paths);
    assert_eq!(got.settings, want.settings);
    assert!(got.settings.hosts.is_empty(), "hosts stay on this machine");
    assert_eq!(got.settings.projects, c.settings.projects);
    assert_eq!(got.scope, want.scope);
    assert_eq!(got.inv, want.inv);
    assert_eq!(got.mcp, want.mcp);
    assert_eq!(got.stats, want.stats);
    assert_eq!(got.last_change, want.last_change);
    assert_eq!(got.last_change.as_deref(), Some("Sync 2 skills"));
    assert_eq!(got.detected, want.detected);
    assert_eq!(got.mcp_detected, want.mcp_detected);

    // The fixture covers every placement, and both scopes' counts.
    let placements: Vec<&Placement> = got.inv.skills.iter().flat_map(|s| &s.placements).collect();
    assert!(placements.contains(&&Placement::Dir) && placements.contains(&&Placement::Linked) && placements.contains(&&Placement::Missing));
    assert!(placements.iter().any(|p| matches!(p, Placement::Link(_))) && placements.iter().any(|p| matches!(p, Placement::Broken(_))));
    assert!(got.inv.get("alpha").unwrap().source.is_some());
    assert_eq!(got.stats.len(), 2);
    assert_eq!(got.stats[0].0, Scope::Global);
    assert_eq!(got.stats[0].1.total, 3);
    assert_eq!(got.stats[1].1.total, 1);
    assert!(got.mcp.get("gl").is_some() && got.mcp.get("web").is_some());
    assert_eq!(got.detected.len(), crate::agents::AGENTS.len());
    assert_eq!(got.mcp_detected.len(), crate::mcp::agents::MCP_AGENTS.len());
}

#[test]
fn wire_state_masks_everything() {
    let home = mcp_home();
    let (code, out, _) = go(&["wire", "state"], &mut ctx(home.path()));
    assert_eq!(code, 0);
    for secret in FIXTURES {
        assert!(!out.contains(secret), "{secret} leaked: {out}");
    }
    let state: crate::wire::WireState = serde_json::from_str(&out).unwrap();
    let gl = state.mcp.get("gl").unwrap();
    assert!(!gl.diffs.is_empty(), "the args differ, so their written values are in the state");
    let cursor = gl.copies.iter().find(|c| c.extras.contains_key("extraSecret")).unwrap();
    assert_eq!(cursor.extras["extraSecret"], serde_json::Value::Null, "extras travel as names only");
}

#[test]
fn wire_server_is_unmasked() {
    let home = mcp_home();
    let mut c = ctx(home.path());
    let (code, out, _) = go(&["wire", "server", "gl"], &mut c);
    assert_eq!(code, 0);
    assert!(out.contains("FIXTURE_ENV_SECRET") && out.contains("FIXTURE_ARG_SECRET"), "{out}");
    assert!(!out.contains("FIXTURE_EXTRA_SECRET"), "extras travel as names only");
    let server: crate::mcp::scan::McpServer = serde_json::from_str(&out).unwrap();
    assert_eq!(server.name, "gl");
    let (code, _, err) = go(&["wire", "server", "nope"], &mut c);
    assert_eq!(code, 1);
    assert!(err.contains("nope"), "{err}");
}

#[test]
fn wire_skill_md_and_hash_read_the_skill_folder() {
    let (_home, mut c) = wire_home();
    let (code, out, _) = go(&["wire", "skill-md", "alpha"], &mut c);
    assert_eq!(code, 0);
    assert_eq!(json(&out), serde_json::json!({ "body": "line one\nline two" }));
    let (code, out, _) = go(&["wire", "skill-hash", "alpha"], &mut c);
    assert_eq!(code, 0);
    let hash = crate::planid::tree_hash(&c.paths.home.join(".agents/skills/alpha")).unwrap();
    assert_eq!(json(&out), serde_json::json!({ "hash": hash }));
    let project = c.settings.projects[0].to_str().unwrap().to_string();
    let (code, out, _) = go(&["--project", &project, "wire", "skill-md", "local"], &mut c);
    assert_eq!(code, 0);
    assert_eq!(json(&out)["body"], "line one\nline two");
    let (code, _, err) = go(&["wire", "skill-md", "nope"], &mut c);
    assert_eq!(code, 1);
    assert!(err.contains("nope"), "{err}");
}

#[test]
fn wire_is_hidden_from_help() {
    let home = tempfile::tempdir().unwrap();
    let (code, out, _) = go(&["--help"], &mut ctx(home.path()));
    assert_eq!(code, 0);
    assert!(out.contains("skills") && !out.contains("wire"), "{out}");
    assert_eq!(run(vec!["wire".into(), "--help".into()]), Some(0), "wire still routes to the CLI");
}

#[test]
fn wire_masked_values_show_as_local() {
    use crate::mcp::model::{Transport, masked_args, masked_url};
    let home = mcp_home();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(
        home.path().join(".claude.json"),
        r#"{"mcpServers":{
            "def":{"command":"npx","args":["def-mcp","--token","FIXTURE_ARG_SECRET"],"cwd":"/work","env":{"KEY":"${KEY:-FIXTURE_ENV_SECRET}","HOST":"${GITLAB_HOST}"}},
            "bearer":{"type":"http","url":"https://u:FIXTURE_URL_SECRET@mcp.example.com/mcp","headers":{"Authorization":"Bearer ${TOKEN}","X-Mixed":"Bearer FIXTURE_HEADER_SECRET-${T}"}}}}"#,
    )
    .unwrap();
    let paths = Paths::isolated(home.path());
    let local = crate::mcp::scan::scan(&Scope::Global, &paths);
    let remote = crate::mcp::scan::masked(&local);
    let serialized = serde_json::to_string(&remote).unwrap();
    for secret in FIXTURES {
        assert!(!serialized.contains(secret), "{secret} leaked: {serialized}");
    }
    let copies = local.servers.iter().flat_map(|s| &s.copies).zip(remote.servers.iter().flat_map(|s| &s.copies));
    let mut checked = 0;
    for (here, there) in copies {
        match (&here.server.transport, &there.server.transport) {
            (Transport::Stdio { command: c1, args: a1, env: e1, cwd: d1 }, Transport::Stdio { command: c2, args: a2, env: e2, cwd: d2 }) => {
                assert_eq!(masked_url(c1), masked_url(c2));
                assert_eq!(masked_args(a1), masked_args(a2));
                assert_eq!(d1.as_deref().map(masked_url), d2.as_deref().map(masked_url));
                assert_eq!(e1.keys().collect::<Vec<_>>(), e2.keys().collect::<Vec<_>>());
                assert!(e1.iter().zip(e2).all(|((_, v1), (_, v2))| v1.masked() == v2.masked()), "{e1:?} vs {e2:?}");
            }
            (Transport::Http { url: u1, headers: h1, .. }, Transport::Http { url: u2, headers: h2, .. }) => {
                assert_eq!(masked_url(u1), masked_url(u2));
                assert_eq!(h1.keys().collect::<Vec<_>>(), h2.keys().collect::<Vec<_>>());
                assert!(h1.iter().zip(h2).all(|((_, v1), (_, v2))| v1.masked() == v2.masked()), "{h1:?} vs {h2:?}");
            }
            (a, b) => panic!("transport kind changed: {a:?} vs {b:?}"),
        }
        checked += 1;
    }
    assert_eq!(checked, local.servers.iter().map(|s| s.copies.len()).sum::<usize>());
    // References survive masking: the window shows them as here.
    let def = &remote.get("def").unwrap().copies[0].server.transport;
    let Transport::Stdio { env, .. } = def else { panic!("{def:?}") };
    assert_eq!(env["HOST"].masked(), "${GITLAB_HOST}");
    assert_eq!(env["KEY"].masked(), "${KEY:-***}");
    let bearer = &remote.get("bearer").unwrap().copies[0].server.transport;
    let Transport::Http { headers, .. } = bearer else { panic!("{bearer:?}") };
    assert_eq!(headers["Authorization"].masked(), "Bearer ${TOKEN}");
    assert_eq!(headers["X-Mixed"].masked(), "Bearer ***${T}");
}

// Copies to another machine: home A is this machine ("this"), home B the host "laptop".

/// Every path under `root` with its kind and bytes (link targets for links), to tell that a
/// command left a home as it was.
fn snapshot(root: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut todo = vec![root.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            let bytes = if meta.file_type().is_symlink() {
                std::fs::read_link(&path).unwrap().into_os_string().into_encoded_bytes()
            } else if meta.is_dir() {
                todo.push(path.clone());
                b"/".to_vec()
            } else {
                std::fs::read(&path).unwrap()
            };
            out.push((path, bytes));
        }
    }
    out.sort();
    out
}

/// A host home whose `prism` reports `version`, with Claude Code installed.
fn host_home(version: &str) -> tempfile::TempDir {
    let remote = remote_with_prism();
    std::fs::write(remote.path().join(".local/bin/prism"), format!("#!/bin/sh\necho \"prism {version}\"\n")).unwrap();
    std::fs::create_dir_all(remote.path().join(".claude")).unwrap();
    remote
}

/// A ctx on home `a` whose "this" is `a` and whose saved host "laptop" is `b`.
fn copying(a: &std::path::Path, b: &std::path::Path) -> Ctx {
    let mut c = ctx(a);
    c.settings.hosts.push(crate::hosts::Host { name: "laptop".into(), ssh: "me@laptop".into() });
    c.make_runner = runner::local_runners(&[("this", a), ("laptop", b)]);
    c
}

#[test]
fn skill_copy_to_host_plan_then_apply() {
    let (here, there) = (tempfile::tempdir().unwrap(), host_home(VERSION));
    let (a, b) = (here.path(), there.path());
    let src = sent_skill(a);
    std::fs::create_dir_all(a.join(".agents/skills")).unwrap();
    std::fs::rename(&src, a.join(".agents/skills/pdf")).unwrap();
    std::fs::remove_dir(a.join("elsewhere")).unwrap();
    let before = snapshot(a);
    let mut c = copying(a, b);

    let (code, out, err) = go(&["skills", "copy", "pdf", "--to-host", "laptop", "--to-global", "--json"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    let plan = json(&out);
    let this = crate::hosts::copy::this_machine();
    assert_eq!(plan["actions"], serde_json::json!([format!("Copy pdf from {this} to Global (~) (link into Claude Code) on laptop")]));
    assert_eq!(plan["applied"], false);
    assert!(!b.join(".agents/skills/pdf").exists(), "a plan changes nothing");

    let (_, text, _) = go(&["skills", "copy", "pdf", "--to-host", "laptop", "--to-global"], &mut c);
    assert!(text.contains("on laptop") && text.contains("--apply --plan-id"), "{text}");

    let id = plan["plan_id"].as_str().unwrap();
    let (code, out, err) = go(&["skills", "copy", "pdf", "--to-host", "laptop", "--to-global", "--apply", "--plan-id", id, "--json"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(json(&out)["applied"], true);
    let got = b.join(".agents/skills/pdf");
    assert_eq!(crate::planid::tree_hash(&got), crate::planid::tree_hash(&a.join(".agents/skills/pdf")));
    assert_eq!(std::fs::read_link(got.join("link")).unwrap(), std::path::Path::new("sub/a.md"));
    assert!(std::fs::symlink_metadata(b.join(".claude/skills/pdf")).unwrap().file_type().is_symlink());
    assert_eq!(store::load_last_change(&Paths::isolated(b)).unwrap().label, format!("copy pdf from {this}"));
    assert_eq!(snapshot(a), before, "this machine is unchanged");

    // Again: the host has it now, and the old id is stale.
    let (code, out, _) = go(&["skills", "copy", "pdf", "--to-host", "laptop", "--to-global", "--apply", "--plan-id", id, "--json"], &mut c);
    assert_eq!(code, 4, "{out}");
    let again = json(&out);
    assert_eq!(again["plan_changed"], true);
    assert_eq!(again["conflicts"], serde_json::json!([{ "skill": "pdf", "reason": "already in Global (~) on laptop" }]));
}

#[test]
fn copy_skill_runs_between_any_two_machines() {
    use crate::hosts::copy::copy_skills;
    use crate::hosts::runner::Local;
    let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::create_dir_all(two.path().join(".claude")).unwrap();
    write_skill(&one.path().join(".agents/skills"), "pdf", "Reads PDFs");
    let from = Local { name: "box".into(), home: one.path().into() };
    let to = Local { name: "laptop".into(), home: two.path().into() };
    let dir = std::fs::canonicalize(one.path().join(".agents/skills/pdf")).unwrap();
    let skills = [("pdf".to_string(), dir)];
    let plan = copy_skills(&from, "box", &Scope::Global, &skills, &to, "laptop", &Scope::Global, None).unwrap();
    assert_eq!(plan["actions"], serde_json::json!(["Copy pdf from box to Global (~) (link into Claude Code) on laptop"]));
    let id = plan["plan_id"].as_str().unwrap();
    let done = copy_skills(&from, "box", &Scope::Global, &skills, &to, "laptop", &Scope::Global, Some(id)).unwrap();
    assert_eq!(done["applied"], true);
    assert!(two.path().join(".agents/skills/pdf/SKILL.md").is_file());
}

/// Home A with Claude Code holding `node-srv`: a Homebrew command, a secret and a reference.
fn server_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(
        home.path().join(".claude.json"),
        r#"{"mcpServers":{"node-srv":{"command":"/opt/homebrew/bin/node","args":["server.js"],"env":{"API_KEY":"FIXTURE_COPY_SECRET","TOKEN":"${TOKEN}"}}}}"#,
    )
    .unwrap();
    home
}

#[test]
fn mcp_copy_to_host_warns_on_paths() {
    let (here, there) = (server_home(), host_home(VERSION));
    let (a, b) = (here.path(), there.path());
    let before = snapshot(a);
    let mut c = copying(a, b);
    let this = crate::hosts::copy::this_machine();

    let (code, out, err) = go(&["mcp", "copy", "node-srv", "--to-host", "laptop", "--to", "claude", "--json"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    let plan = json(&out);
    assert_eq!(plan["actions"], serde_json::json!(["Add node-srv to Claude Code on laptop"]));
    let warnings: Vec<&str> = plan["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).collect();
    let portable: Vec<&str> = warnings.iter().copied().filter(|w| w.ends_with("check it exists on laptop") || w.ends_with("set on laptop")).collect();
    assert_eq!(portable, [format!("command points at /opt/homebrew/bin/node on {this}; check it exists on laptop"), "env.TOKEN needs ${TOKEN} set on laptop".to_string()]);
    assert!(!b.join(".claude.json").exists(), "a plan changes nothing");

    let id = plan["plan_id"].as_str().unwrap().to_string();
    let (code, applied, err2) = go(&["mcp", "copy", "node-srv", "--to-host", "laptop", "--to", "claude", "--apply", "--plan-id", &id, "--json"], &mut c);
    assert_eq!(code, 0, "{applied}{err2}");
    assert_eq!(json(&applied)["applied"], true);
    let written: serde_json::Value = json(&std::fs::read_to_string(b.join(".claude.json")).unwrap());
    let srv = &written["mcpServers"]["node-srv"];
    assert_eq!(srv["command"], "/opt/homebrew/bin/node");
    assert_eq!(srv["env"]["API_KEY"], "FIXTURE_COPY_SECRET");
    assert_eq!(srv["env"]["TOKEN"], "${TOKEN}");
    for text in [&out, &err, &applied, &err2] {
        assert!(!text.contains("FIXTURE_COPY_SECRET"), "{text}");
    }
    assert_eq!(store::load_last_change(&Paths::isolated(b)).unwrap().label, format!("copy node-srv from {this}"));
    assert_eq!(snapshot(a), before, "this machine is unchanged");
}

#[test]
fn mcp_receive_plan_id_covers_the_definition() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    let mut c = ctx(home.path());
    let args = ["mcp", "receive", "s", "--to", "claude", "--from", "box", "--target", "laptop", "--source-home", "/home/me", "--json"];
    let mut id = |command: &str| {
        let server = crate::mcp::model::Server {
            name: "s".into(),
            transport: crate::mcp::model::Transport::Stdio { command: command.into(), args: vec![], env: Default::default(), cwd: None },
        };
        c.stdin = Some(Box::new(std::io::Cursor::new(serde_json::to_vec(&server).unwrap())));
        let (code, out, err) = go(&args, &mut c);
        assert_eq!(code, 0, "{out}{err}");
        json(&out)["plan_id"].as_str().unwrap().to_string()
    };
    assert_ne!(id("one"), id("two"));
    assert_eq!(id("one"), id("one"));
}

#[test]
fn copy_to_unready_host_refuses() {
    let (here, there) = (server_home(), host_home("0.0.0-other"));
    let (a, b) = (here.path(), there.path());
    write_skill(&a.join(".agents/skills"), "pdf", "Reads PDFs");
    let (before_a, before_b) = (snapshot(a), snapshot(b));
    let mut c = copying(a, b);
    let refusal = format!("prism on laptop is version 0.0.0-other, not {VERSION}; run prism hosts install laptop");
    for args in [
        &["skills", "copy", "pdf", "--to-host", "laptop", "--to-global"][..],
        &["mcp", "copy", "node-srv", "--to-host", "laptop", "--to", "claude"],
    ] {
        let (code, out, err) = go(args, &mut c);
        assert_eq!(code, 1, "{args:?}: {out}{err}");
        assert_eq!(err.trim(), refusal, "{args:?}");
    }
    assert_eq!(snapshot(a), before_a);
    assert_eq!(snapshot(b), before_b);
}

#[test]
fn several_skills_to_a_host_are_one_change_there() {
    let (here, there) = (tempfile::tempdir().unwrap(), host_home(VERSION));
    let (a, b) = (here.path(), there.path());
    write_skill(&a.join(".agents/skills"), "alpha", "First");
    // A skill linked from a folder named otherwise: it travels under its own name.
    let sent = sent_skill(a);
    std::fs::rename(&sent, a.join("elsewhere/pdf-src")).unwrap();
    std::os::unix::fs::symlink(a.join("elsewhere/pdf-src"), a.join(".agents/skills/pdf")).unwrap();
    let mut c = copying(a, b);

    let args = ["skills", "copy", "alpha", "pdf", "--to-host", "laptop", "--to-global", "--json"];
    let (code, out, err) = go(&args, &mut c);
    assert_eq!(code, 0, "{out}{err}");
    let plan = json(&out);
    assert_eq!(plan["actions"].as_array().unwrap().len(), 2, "{plan}");
    let id = plan["plan_id"].as_str().unwrap().to_string();
    let mut apply = args.to_vec();
    apply.extend(["--apply", "--plan-id", &id]);
    let (code, out, err) = go(&apply, &mut c);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(json(&out)["done"], 2);
    for name in ["alpha", "pdf"] {
        let got = b.join(".agents/skills").join(name);
        assert!(std::fs::symlink_metadata(&got).unwrap().is_dir(), "{name} arrives as a folder");
        assert_eq!(crate::planid::tree_hash(&got), crate::planid::tree_hash(&std::fs::canonicalize(a.join(".agents/skills").join(name)).unwrap()), "{name}");
        assert!(std::fs::symlink_metadata(b.join(".claude/skills").join(name)).unwrap().file_type().is_symlink());
    }
    assert_eq!(store::load_last_change(&Paths::isolated(b)).unwrap().label, format!("copy 2 skills from {}", crate::hosts::copy::this_machine()));

    // One undo on the host takes both back.
    let (code, out, _) = go(&["undo"], &mut ctx(b));
    assert_eq!(code, 0, "{out}");
    for name in ["alpha", "pdf"] {
        assert!(std::fs::symlink_metadata(b.join(".agents/skills").join(name)).is_err(), "{name}");
        assert!(std::fs::symlink_metadata(b.join(".claude/skills").join(name)).is_err(), "{name}");
    }
}

/// `skills receive --skill …` on `c` with `tar` on stdin, planned then applied.
fn receive_named(c: &mut Ctx, skills: &[String], tar: Vec<u8>) -> (i32, String, String) {
    let mut args = vec!["skills", "receive", "--to-global", "--from", "laptop", "--json"];
    for s in skills {
        args.extend(["--skill", s.as_str()]);
    }
    let (code, out, err) = go(&args, c);
    assert_eq!(code, 0, "{out}{err}");
    let id = json(&out)["plan_id"].as_str().unwrap().to_string();
    args.extend(["--apply", "--plan-id", &id]);
    c.stdin = Some(Box::new(std::io::Cursor::new(tar)));
    go(&args, c)
}

/// One tar of `root`'s `names`, each under its name, as the sender packs it.
fn named_tar(root: &std::path::Path, names: &[&str]) -> Vec<u8> {
    let mut cmd = std::process::Command::new("tar");
    cmd.env("COPYFILE_DISABLE", "1").arg("-C").arg(root).arg("-cf").arg("-");
    cmd.args(names.iter().map(|n| format!("./{n}")));
    let out = cmd.output().unwrap();
    assert!(out.status.success());
    out.stdout
}

#[test]
fn one_tampered_skill_of_two_places_nothing() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let src = h.join("from");
    write_skill(&src, "alpha", "First");
    write_skill(&src, "beta", "Second");
    let hash = |n: &str| crate::planid::tree_hash(&src.join(n)).unwrap();
    let skills = [format!("alpha={}", hash("alpha")), format!("beta={}", hash("beta"))];
    std::fs::write(src.join("beta/SKILL.md"), "tampered").unwrap();
    let mut c = receiving(h);
    let (code, out, _) = receive_named(&mut c, &skills, named_tar(&src, &["alpha", "beta"]));
    assert_eq!(code, 1, "{out}");
    assert_eq!(json(&out)["error"], "beta arrived incomplete or changed; nothing was placed");
    for name in ["alpha", "beta"] {
        assert!(std::fs::symlink_metadata(h.join(".agents/skills").join(name)).is_err(), "{name}");
        assert!(std::fs::symlink_metadata(h.join(".codex/skills").join(name)).is_err(), "{name}");
    }
    assert!(leftovers(&h.join(".agents/skills")).is_empty());
    assert_eq!(store::load_last_change(&c.paths), None);
}

#[test]
fn a_folder_nobody_declared_refuses_the_archive() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let src = h.join("from");
    write_skill(&src, "alpha", "First");
    write_skill(&src, "stowaway", "Not declared");
    let skills = [format!("alpha={}", crate::planid::tree_hash(&src.join("alpha")).unwrap())];
    let mut c = receiving(h);
    let (code, out, _) = receive_named(&mut c, &skills, named_tar(&src, &["alpha", "stowaway"]));
    assert_eq!(code, 1, "{out}");
    assert!(json(&out)["error"].as_str().unwrap().contains("reaches outside"), "{out}");
    assert!(std::fs::symlink_metadata(h.join(".agents/skills/alpha")).is_err());
    assert!(std::fs::symlink_metadata(h.join(".agents/skills/stowaway")).is_err());
    assert!(leftovers(&h.join(".agents/skills")).is_empty());
}

#[test]
fn mcp_copy_to_a_host_project() {
    let (here, there) = (server_home(), host_home(VERSION));
    let (a, b) = (here.path(), there.path());
    let project = std::fs::canonicalize(b).unwrap().join("work/app");
    std::fs::create_dir_all(&project).unwrap();
    let mut c = copying(a, b);
    let dir = project.to_str().unwrap();
    let args = ["mcp", "copy", "node-srv", "--to-host", "laptop", "--to", "claude", "--to-project", dir, "--claude-scope", "project", "--json"];
    let (code, out, err) = go(&args, &mut c);
    assert_eq!(code, 0, "{out}{err}");
    let id = json(&out)["plan_id"].as_str().unwrap().to_string();
    let mut apply = args.to_vec();
    apply.extend(["--apply", "--plan-id", &id]);
    let (code, out, err) = go(&apply, &mut c);
    assert_eq!(code, 0, "{out}{err}");
    assert!(!out.contains("FIXTURE_COPY_SECRET"), "{out}");
    let written: serde_json::Value = json(&std::fs::read_to_string(project.join(".mcp.json")).unwrap());
    assert_eq!(written["mcpServers"]["node-srv"]["env"]["API_KEY"], "FIXTURE_COPY_SECRET");
    let global = std::fs::read_to_string(b.join(".claude.json")).unwrap_or_default();
    assert!(!global.contains("node-srv"), "{global}");
    let (_, listed, _) = go(&["mcp", "list", "--json"], &mut ctx(b));
    assert!(agents_of_opt(&listed, "node-srv").is_none(), "{listed}");

    // --to-project is a place on the host: it needs --to-host.
    let (code, _, _) = go(&["mcp", "copy", "node-srv", "--to", "claude", "--to-project", dir], &mut c);
    assert_eq!(code, 2);
}

fn agents_of_opt(out: &str, name: &str) -> Option<serde_json::Value> {
    json(out)["servers"].as_array().unwrap().iter().find(|s| s["name"] == name).cloned()
}

#[test]
fn reserved_names_are_not_other_machines() {
    let home = tempfile::tempdir().unwrap();
    write_skill(&home.path().join(".agents/skills"), "pdf", "Reads PDFs");
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(home.path().join(".claude.json"), r#"{"mcpServers":{"srv":{"command":"srv"}}}"#).unwrap();
    // Any runner asked for is a bug: each of these is refused before it reaches one.
    let mut c = ctx(home.path());
    for name in ["this", "local"] {
        for args in [
            &["hosts", "check", name][..],
            &["hosts", "install", name],
            &["skills", "copy", "pdf", "--to-host", name, "--to-global"],
            &["mcp", "copy", "srv", "--to", "cursor", "--to-host", name],
        ] {
            let (code, out, err) = go(args, &mut c);
            assert_eq!(code, 2, "{args:?}: {out}{err}");
            assert!(err.contains(&format!("{name} is this machine")), "{args:?}: {err}");
        }
    }
}

#[test]
fn relative_folders_for_another_machine_are_refused() {
    let home = tempfile::tempdir().unwrap();
    write_skill(&home.path().join(".agents/skills"), "pdf", "Reads PDFs");
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(home.path().join(".claude.json"), r#"{"mcpServers":{"srv":{"command":"srv"}}}"#).unwrap();
    let mut c = ctx(home.path());
    c.settings.hosts.push(crate::hosts::Host { name: "x".into(), ssh: "me@x".into() });
    // No runner is reached: the paths are refused first.
    for args in [
        &["--host", "x", "--project", "site", "skills", "list"][..],
        &["--host=x", "skills", "list", "--project=./site"],
        &["--host", "x", "skills", "copy", "pdf", "--to-project", "site"],
        &["skills", "copy", "pdf", "--to-host", "x", "--to-project", "site"],
        &["mcp", "copy", "srv", "--to", "claude", "--to-host", "x", "--to-project", "../site"],
    ] {
        let (code, out, err) = go(args, &mut c);
        assert_eq!(code, 2, "{args:?}: {out}{err}");
        assert!(err.contains("full path") && err.contains("site"), "{args:?}: {err}");
    }
    // On this machine ("this" too) a relative folder is the working folder's.
    let (_, _, err) = go(&["--host", "this", "--project", "no-such-site", "skills", "list"], &mut c);
    assert_eq!(err.trim(), "no-such-site is not a folder");
}
