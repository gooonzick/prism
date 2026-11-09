use super::*;

fn ctx(home: &std::path::Path) -> Ctx {
    let paths = Paths::isolated(home);
    let settings = Settings::load_from(&paths);
    Ctx { paths, settings, stdin: None }
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

#[test]
fn install_bad_source_is_usage_error() {
    let home = tempfile::tempdir().unwrap();
    let (code, out, err) = go(&["skills", "install", "not a source!"], &mut ctx(home.path()));
    // Exit 2 and not 1: a runner was never looked for, let alone started.
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
fn env_from_stdin_is_read_once() {
    let home = empty_mcp_home();
    let mut c = ctx(home.path());
    c.stdin = Some(Box::new(std::io::Cursor::new("FIXTURE_STDIN_SECRET\n")));
    let (code, out, err) = go(&["mcp", "add", "plata", "--command", "npx", "--env", "TOKEN=-", "--to", "cursor"], &mut c);
    assert_eq!(code, 0, "{out}{err}");
    let written = std::fs::read_to_string(home.path().join(".cursor/mcp.json")).unwrap();
    assert!(written.contains("\"FIXTURE_STDIN_SECRET\""), "{written}");
    let home = empty_mcp_home();
    let mut c = ctx(home.path());
    c.stdin = Some(Box::new(std::io::Cursor::new("FIXTURE_STDIN_SECRET\n")));
    let (code, out, err) = go(&["mcp", "add", "plata", "--command", "npx", "--env", "A=-", "--env", "B=-", "--to", "cursor"], &mut c);
    assert_eq!(code, 2, "{out}{err}");
    assert!(!home.path().join(".cursor/mcp.json").exists());
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
