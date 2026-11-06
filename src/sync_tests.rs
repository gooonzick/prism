use super::*;
use crate::agents::index_of;
use crate::scan::scan;
use std::os::unix::fs::symlink;

struct Fixture {
    home: tempfile::TempDir,
    paths: Paths,
}

impl Fixture {
    fn new() -> Fixture {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::isolated(&fs::canonicalize(home.path()).unwrap());
        Fixture { home, paths }
    }

    fn dir(&self, agent: &str) -> PathBuf {
        AGENTS[index_of(agent).unwrap()].dir(&Scope::Global, &self.paths)
    }

    fn skill(&self, agent: &str, name: &str, body: &str) {
        let d = self.dir(agent).join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("SKILL.md"), format!("---\nname: {name}\ndescription: d\n---\n{body}")).unwrap();
    }

    fn inv(&self) -> Inventory {
        scan(&Scope::Global, &self.paths)
    }

    fn placement(&self, name: &str, agent: &str) -> Placement {
        self.inv().get(name).unwrap().placements[index_of(agent).unwrap()].clone()
    }
}

fn ids(names: &[&str]) -> Vec<usize> {
    names.iter().map(|n| index_of(n).unwrap()).collect()
}

#[test]
fn sync_adopts_a_lone_copy_and_links_it_everywhere() {
    let f = Fixture::new();
    f.skill("claude", "alpha", "A");
    let targets = ids(&["claude", "codex", "opencode"]);

    let plan = plan(&f.inv(), &targets, &f.paths);
    assert_eq!(
        plan.actions,
        [
            Action::Adopt { skill: "alpha".into(), from: index_of("claude").unwrap() },
            Action::Link { skill: "alpha".into(), agent: index_of("codex").unwrap() },
            Action::Link { skill: "alpha".into(), agent: index_of("opencode").unwrap() },
        ]
    );
    assert_eq!(apply(&Scope::Global, &f.paths, &plan.actions), Ok(3));

    assert_eq!(f.placement("alpha", "agents"), Placement::Dir);
    for agent in ["claude", "codex", "opencode"] {
        assert_eq!(f.placement("alpha", agent), Placement::Linked, "{agent}");
    }
    // Links are relative, as the skills CLI writes them.
    assert_eq!(fs::read_link(f.dir("claude").join("alpha")).unwrap(), PathBuf::from("../../.agents/skills/alpha"));
    assert_eq!(fs::read_link(f.dir("opencode").join("alpha")).unwrap(), PathBuf::from("../../../.agents/skills/alpha"));
    assert!(super::plan(&f.inv(), &targets, &f.paths).is_empty(), "a second sync has nothing to do");
}

#[test]
fn identical_copies_become_links_and_different_ones_are_conflicts() {
    let f = Fixture::new();
    f.skill("agents", "same", "S");
    f.skill("codex", "same", "S");
    f.skill("agents", "diff", "original");
    f.skill("codex", "diff", "edited");

    let plan = plan(&f.inv(), &ids(&["codex"]), &f.paths);
    assert_eq!(plan.actions, [Action::Replace { skill: "same".into(), agent: index_of("codex").unwrap() }]);
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].skill, "diff");

    apply(&Scope::Global, &f.paths, &plan.actions).unwrap();
    assert_eq!(f.placement("same", "codex"), Placement::Linked);
    assert!(f.paths.home.join(".Trash/same/SKILL.md").is_file() || !cfg!(target_os = "macos"), "the copy went to the trash");
    assert_eq!(f.placement("diff", "codex"), Placement::Dir, "a conflict is left alone");
}

#[test]
fn different_copies_in_two_agents_are_not_adopted() {
    let f = Fixture::new();
    f.skill("claude", "x", "one");
    f.skill("codex", "x", "two");
    let plan = plan(&f.inv(), &ids(&["claude", "codex"]), &f.paths);
    assert!(plan.actions.is_empty());
    assert_eq!(plan.conflicts.len(), 1);
}

#[test]
fn a_link_to_an_outside_folder_is_adopted_as_a_link() {
    let f = Fixture::new();
    let outside = f.home.path().join("plugins/prompting");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("SKILL.md"), "---\nname: prompting\n---\n").unwrap();
    fs::create_dir_all(f.dir("opencode")).unwrap();
    symlink(&outside, f.dir("opencode").join("prompting")).unwrap();

    let plan = plan(&f.inv(), &ids(&["opencode", "claude"]), &f.paths);
    assert_eq!(plan.actions[0], Action::AdoptLink { skill: "prompting".into(), from: index_of("opencode").unwrap() });
    apply(&Scope::Global, &f.paths, &plan.actions).unwrap();
    assert_eq!(fs::canonicalize(f.dir("agents").join("prompting")).unwrap(), fs::canonicalize(&outside).unwrap());
    assert_eq!(f.placement("prompting", "opencode"), Placement::Linked);
    assert_eq!(f.placement("prompting", "claude"), Placement::Linked);
    assert!(outside.join("SKILL.md").is_file(), "the outside folder is untouched");
}

#[test]
fn broken_links_are_replaced() {
    let f = Fixture::new();
    f.skill("agents", "alpha", "A");
    fs::create_dir_all(f.dir("claude")).unwrap();
    symlink("/nowhere/alpha", f.dir("claude").join("alpha")).unwrap();
    let plan = plan(&f.inv(), &ids(&["claude"]), &f.paths);
    assert_eq!(plan.actions, [Action::Replace { skill: "alpha".into(), agent: index_of("claude").unwrap() }]);
    apply(&Scope::Global, &f.paths, &plan.actions).unwrap();
    assert_eq!(f.placement("alpha", "claude"), Placement::Linked);
}

#[test]
fn link_unlink_and_remove_one_skill() {
    let f = Fixture::new();
    f.skill("agents", "alpha", "A");
    let codex = index_of("codex").unwrap();

    link(&f.inv(), &f.paths, "alpha", codex).unwrap();
    assert_eq!(f.placement("alpha", "codex"), Placement::Linked);
    unlink(&Scope::Global, &f.paths, "alpha", codex).unwrap();
    assert_eq!(f.placement("alpha", "codex"), Placement::Missing);
    assert!(unlink(&Scope::Global, &f.paths, "alpha", CANONICAL).is_err());

    f.skill("claude", "beta", "B");
    assert!(unlink(&Scope::Global, &f.paths, "beta", index_of("claude").unwrap()).is_err(), "a real folder is not unlinked");

    link(&f.inv(), &f.paths, "alpha", codex).unwrap();
    remove(&f.inv(), &f.paths, "alpha").unwrap();
    assert!(f.inv().get("alpha").is_none());
}

#[test]
fn projects_sync_inside_the_project() {
    let f = Fixture::new();
    let project = f.paths.home.join("work/app");
    fs::create_dir_all(project.join(".claude/skills/review")).unwrap();
    fs::write(project.join(".claude/skills/review/SKILL.md"), "---\nname: review\n---\n").unwrap();
    let scope = Scope::Project(project.clone());
    let inv = scan(&scope, &f.paths);
    let plan = plan(&inv, &ids(&["claude", "codex"]), &f.paths);
    apply(&scope, &f.paths, &plan.actions).unwrap();
    assert!(project.join(".agents/skills/review/SKILL.md").is_file());
    assert_eq!(fs::read_link(project.join(".codex/skills/review")).unwrap(), PathBuf::from("../../.agents/skills/review"));
    assert!(!f.paths.home.join(".agents").exists(), "global folders are untouched");
}

#[test]
fn relative_paths() {
    assert_eq!(relative(Path::new("/h/.claude/skills"), Path::new("/h/.agents/skills/a")), PathBuf::from("../../.agents/skills/a"));
    assert_eq!(relative(Path::new("/h/a"), Path::new("/h/a/b")), PathBuf::from("b"));
}
