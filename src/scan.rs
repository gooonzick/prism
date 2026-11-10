//! What is installed in a scope: every skill folder in every agent's skills folder, and how
//! each relates to the canonical copy in `.agents/skills`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agents::{AGENTS, CANONICAL, Paths, Scope};
use crate::skill;

/// One agent's entry for a skill.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Placement {
    Missing,
    /// A real folder: a copy, or the canonical skill itself in `.agents`.
    Dir,
    /// A symlink to the canonical skill.
    Linked,
    /// A symlink somewhere else.
    Link(PathBuf),
    /// A symlink whose target is gone.
    Broken(PathBuf),
}

impl Placement {
    pub fn present(&self) -> bool {
        matches!(self, Placement::Dir | Placement::Linked | Placement::Link(_))
    }

    pub fn label(&self) -> &'static str {
        match self {
            Placement::Missing => "not linked",
            Placement::Dir => "its own copy",
            Placement::Linked => "linked",
            Placement::Link(_) => "links elsewhere",
            Placement::Broken(_) => "broken link",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    /// The folder name, which agents use as the skill's id.
    pub name: String,
    pub description: String,
    /// One per entry of [`AGENTS`].
    pub placements: Vec<Placement>,
    /// The folder the skill's contents are read from.
    pub source: Option<PathBuf>,
}

impl Skill {
    pub fn canonical(&self) -> &Placement {
        &self.placements[CANONICAL]
    }

    /// The skill is in `.agents` and every agent in `targets` links to it.
    pub fn synced(&self, targets: &[usize]) -> bool {
        self.canonical().present() && targets.iter().all(|&t| self.placements[t] == Placement::Linked)
    }

    pub fn manifest(&self) -> Option<skill::Manifest> {
        skill::read(self.source.as_deref()?)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    pub scope: Scope,
    pub skills: Vec<Skill>,
    /// Per agent: its skills folder is the canonical folder itself (a symlink to it, or the
    /// same path), so there is nothing to link and nothing that may be unlinked.
    pub shares_canonical: Vec<bool>,
}

impl Inventory {
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }
}

/// A scope's counts, for the sidebar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScopeStats {
    pub total: usize,
    pub unsynced: usize,
}

/// Skill count, and how many skills are not synced into every agent in `targets`.
pub fn stats_of(inv: &Inventory, targets: &[usize]) -> ScopeStats {
    ScopeStats { total: inv.skills.len(), unsynced: inv.skills.iter().filter(|s| !s.synced(targets)).count() }
}

pub fn scan(scope: &Scope, paths: &Paths) -> Inventory {
    let dirs: Vec<PathBuf> = AGENTS.iter().map(|a| a.dir(scope, paths)).collect();
    let canonical_real = fs::canonicalize(&dirs[CANONICAL]).ok();
    let shares_canonical: Vec<bool> = dirs
        .iter()
        .enumerate()
        .map(|(i, d)| i != CANONICAL && (d == &dirs[CANONICAL] || (canonical_real.is_some() && fs::canonicalize(d).ok() == canonical_real)))
        .collect();

    let mut names: Vec<String> = Vec::new();
    for (i, dir) in dirs.iter().enumerate() {
        if i != CANONICAL && shares_canonical[i] {
            continue;
        }
        for name in entries(dir) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names.sort_by_key(|n| n.to_lowercase());

    let skills = names
        .into_iter()
        .map(|name| {
            let canonical = dirs[CANONICAL].join(&name);
            let canonical_target = fs::canonicalize(&canonical).ok();
            let placements: Vec<Placement> = dirs
                .iter()
                .enumerate()
                .map(|(i, dir)| {
                    if i != CANONICAL && shares_canonical[i] {
                        return if canonical_target.is_some() { Placement::Linked } else { Placement::Missing };
                    }
                    let target = if i == CANONICAL { None } else { canonical_target.as_deref() };
                    placement(&dir.join(&name), target)
                })
                .collect();
            let source = placements
                .iter()
                .enumerate()
                .find(|(_, p)| p.present())
                .map(|(i, _)| dirs[i].join(&name));
            let description = source.as_deref().and_then(skill::read).and_then(|m| m.description).unwrap_or_default();
            Skill { name, description, placements, source }
        })
        .collect();
    Inventory { scope: scope.clone(), skills, shares_canonical }
}

/// Skill folder names in `dir`: folders with a manifest, and symlinks (broken ones included,
/// so they can be cleaned up). Hidden entries are skipped.
fn entries(dir: &Path) -> Vec<String> {
    let Ok(read) = fs::read_dir(dir) else { return Vec::new() };
    read.filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let path = e.path();
            let is_link = fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink());
            let is_skill = path.is_dir() && skill::manifest_path(&path).is_some();
            (is_skill || (is_link && !path.exists()) || (is_link && path.is_dir())).then_some(name)
        })
        .collect()
}

/// How `path` stands, given the canonical skill's resolved path (`None` for the canonical
/// slot itself, or when the skill isn't in `.agents`).
fn placement(path: &Path, canonical: Option<&Path>) -> Placement {
    let Ok(meta) = fs::symlink_metadata(path) else { return Placement::Missing };
    if !meta.file_type().is_symlink() {
        return if meta.is_dir() { Placement::Dir } else { Placement::Missing };
    }
    let target = fs::read_link(path).unwrap_or_default();
    match fs::canonicalize(path) {
        Err(_) => Placement::Broken(target),
        Ok(real) if Some(real.as_path()) == canonical => Placement::Linked,
        Ok(real) => Placement::Link(real),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::index_of;
    use std::os::unix::fs::symlink;

    fn write_skill(dir: &Path, name: &str, desc: &str) {
        fs::create_dir_all(dir.join(name)).unwrap();
        fs::write(dir.join(name).join("SKILL.md"), format!("---\nname: {name}\ndescription: {desc}\n---\nBody")).unwrap();
    }

    #[test]
    fn groups_one_skill_across_agents_and_tells_links_from_copies() {
        let home = tempfile::tempdir().unwrap();
        let p = Paths::isolated(home.path());
        let agents = home.path().join(".agents/skills");
        let claude = home.path().join(".claude/skills");
        let codex = home.path().join(".codex/skills");
        write_skill(&agents, "alpha", "First");
        write_skill(&codex, "beta", "Only in codex");
        fs::create_dir_all(&claude).unwrap();
        symlink("../../.agents/skills/alpha", claude.join("alpha")).unwrap();
        symlink("/nowhere/gamma", claude.join("gamma")).unwrap();
        fs::create_dir_all(codex.join(".system")).unwrap();
        fs::create_dir_all(codex.join("not-a-skill")).unwrap();

        let inv = scan(&Scope::Global, &p);
        let names: Vec<_> = inv.skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["alpha", "beta", "gamma"]);

        let (c, x) = (index_of("claude").unwrap(), index_of("codex").unwrap());
        let alpha = inv.get("alpha").unwrap();
        assert_eq!(alpha.placements[CANONICAL], Placement::Dir);
        assert_eq!(alpha.placements[c], Placement::Linked);
        assert_eq!(alpha.placements[x], Placement::Missing);
        assert_eq!(alpha.description, "First");
        assert!(alpha.synced(&[c]) && !alpha.synced(&[c, x]));

        let beta = inv.get("beta").unwrap();
        assert_eq!(beta.placements[CANONICAL], Placement::Missing);
        assert_eq!(beta.placements[x], Placement::Dir);
        assert_eq!(beta.source.as_deref(), Some(codex.join("beta").as_path()));

        assert!(matches!(inv.get("gamma").unwrap().placements[c], Placement::Broken(_)));
    }

    #[test]
    fn an_agent_folder_that_is_the_canonical_folder_is_marked_shared() {
        let home = tempfile::tempdir().unwrap();
        let p = Paths::isolated(home.path());
        write_skill(&home.path().join(".agents/skills"), "alpha", "First");
        fs::create_dir_all(home.path().join(".codex")).unwrap();
        symlink(home.path().join(".agents/skills"), home.path().join(".codex/skills")).unwrap();
        let inv = scan(&Scope::Global, &p);
        let x = index_of("codex").unwrap();
        assert!(inv.shares_canonical[x]);
        assert_eq!(inv.skills.len(), 1);
    }
}
