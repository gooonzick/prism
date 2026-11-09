use super::*;

fn ctx(home: &std::path::Path) -> Ctx {
    let paths = Paths::isolated(home);
    let settings = Settings::load_from(&paths);
    Ctx { paths, settings }
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
