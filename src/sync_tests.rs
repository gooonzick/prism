use super::*;
use crate::agents::index_of;
use crate::scan::scan;
use std::os::unix::fs::symlink;

fn run(scope: &Scope, paths: &Paths, actions: &[Action]) -> Result<usize, (usize, String)> {
    apply(scope, paths, actions, &mut Journal::default())
}

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
    assert_eq!(run(&Scope::Global, &f.paths, &plan.actions), Ok(3));

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

    run(&Scope::Global, &f.paths, &plan.actions).unwrap();
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
    run(&Scope::Global, &f.paths, &plan.actions).unwrap();
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
    run(&Scope::Global, &f.paths, &plan.actions).unwrap();
    assert_eq!(f.placement("alpha", "claude"), Placement::Linked);
}

#[test]
fn link_unlink_and_remove_one_skill() {
    let f = Fixture::new();
    f.skill("agents", "alpha", "A");
    let codex = index_of("codex").unwrap();

    link(&f.inv(), &f.paths, "alpha", codex, &mut Journal::default()).unwrap();
    assert_eq!(f.placement("alpha", "codex"), Placement::Linked);
    unlink(&Scope::Global, &f.paths, "alpha", codex, &mut Journal::default()).unwrap();
    assert_eq!(f.placement("alpha", "codex"), Placement::Missing);
    assert!(unlink(&Scope::Global, &f.paths, "alpha", CANONICAL, &mut Journal::default()).is_err());

    f.skill("claude", "beta", "B");
    assert!(unlink(&Scope::Global, &f.paths, "beta", index_of("claude").unwrap(), &mut Journal::default()).is_err(), "a real folder is not unlinked");

    link(&f.inv(), &f.paths, "alpha", codex, &mut Journal::default()).unwrap();
    remove(&f.inv(), &f.paths, "alpha", &mut Journal::default()).unwrap();
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
    run(&scope, &f.paths, &plan.actions).unwrap();
    assert!(project.join(".agents/skills/review/SKILL.md").is_file());
    assert_eq!(fs::read_link(project.join(".codex/skills/review")).unwrap(), PathBuf::from("../../.agents/skills/review"));
    assert!(!f.paths.home.join(".agents").exists(), "global folders are untouched");
}

#[test]
fn a_sync_can_be_undone() {
    let f = Fixture::new();
    f.skill("claude", "alpha", "A");
    let plan = plan(&f.inv(), &ids(&["claude", "codex"]), &f.paths);
    let mut j = Journal::default();
    apply(&Scope::Global, &f.paths, &plan.actions, &mut j).unwrap();
    assert_eq!(f.placement("alpha", "codex"), Placement::Linked);
    j.undo(&f.paths).unwrap();
    assert_eq!(f.placement("alpha", "agents"), Placement::Missing);
    assert_eq!(f.placement("alpha", "claude"), Placement::Dir);
    assert_eq!(f.placement("alpha", "codex"), Placement::Missing);
}

#[test]
fn broken_links_are_cleaned_up_and_come_back_on_undo() {
    let f = Fixture::new();
    fs::create_dir_all(f.dir("opencode")).unwrap();
    symlink("/nowhere/one", f.dir("opencode").join("one")).unwrap();
    symlink("/nowhere/two", f.dir("opencode").join("two")).unwrap();
    f.skill("agents", "fine", "F");
    assert_eq!(broken_links(&f.inv()).len(), 2);
    let mut j = Journal::default();
    assert_eq!(remove_broken(&f.inv(), &f.paths, &mut j), Ok(2));
    assert!(broken_links(&f.inv()).is_empty());
    assert!(f.inv().get("fine").is_some());
    j.undo(&f.paths).unwrap();
    assert_eq!(broken_links(&f.inv()).len(), 2);
}

#[test]
fn copy_to_another_scope_lands_in_its_agents_and_links() {
    let f = Fixture::new();
    f.skill("agents", "alpha", "A");
    let project = f.paths.home.join("work/app");
    fs::create_dir_all(&project).unwrap();
    let dest = Scope::Project(project.clone());
    copy_to(&f.inv(), &f.paths, "alpha", &dest, &ids(&["claude"]), &mut Journal::default()).unwrap();
    assert!(project.join(".agents/skills/alpha/SKILL.md").is_file());
    assert_eq!(fs::read_link(project.join(".claude/skills/alpha")).unwrap(), PathBuf::from("../../.agents/skills/alpha"));
    assert!(copy_to(&f.inv(), &f.paths, "alpha", &dest, &[], &mut Journal::default()).is_err(), "an existing copy is kept");
}
