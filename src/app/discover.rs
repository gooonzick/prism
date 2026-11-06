//! Finding projects that already have skills, under a folder the user picks.

use std::fs;
use std::path::{Path, PathBuf};

use super::{App, Found};
use crate::agents::{Paths, Scope};
use crate::scan;

/// Folders never worth descending into.
const SKIP: &[&str] = &["node_modules", "target", "dist", "build", "vendor", "Library", "Applications", "Pictures", "Movies", "Music"];

/// How deep below the chosen folder projects are looked for.
const DEPTH: usize = 4;

/// Projects under `root` with at least one skill in an agent folder, with their counts.
pub fn find_projects(root: &Path, paths: &Paths) -> Vec<(PathBuf, usize)> {
    let mut out = Vec::new();
    walk(root, 0, paths, &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, depth: usize, paths: &Paths, out: &mut Vec<(PathBuf, usize)>) {
    if dir == paths.home && depth > 0 {
        return;
    }
    let n = if dir == paths.home { 0 } else { scan::scan(&Scope::Project(dir.to_path_buf()), paths).skills.len() };
    if n > 0 {
        out.push((dir.to_path_buf(), n));
    }
    if depth >= DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.filter_map(Result::ok) {
        let name = e.file_name().to_string_lossy().into_owned();
        let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
        if is_dir && !name.starts_with('.') && !SKIP.contains(&name.as_str()) {
            walk(&e.path(), depth + 1, paths, out);
        }
    }
}

impl App {
    pub fn discover_in(&mut self, root: PathBuf) {
        let paths = self.paths.clone();
        self.discovering = true;
        self.found = None;
        self.discover.request(root, move |root| {
            let found = find_projects(&root, &paths);
            (root, found)
        });
    }

    pub(super) fn land_discovery(&mut self, root: PathBuf, found: Vec<(PathBuf, usize)>) {
        self.discovering = false;
        let known = &self.settings.projects;
        let projects: Vec<(PathBuf, usize, bool)> = found.into_iter().filter(|(p, _)| !known.contains(p)).map(|(p, n)| (p, n, true)).collect();
        if projects.is_empty() {
            self.notify("No new projects found".into(), format!("Nothing under {} has skills that isn't already listed", crate::agents::tilde(&root, &self.paths)), false);
            return;
        }
        self.found = Some(Found { root, projects });
    }

    /// Lands a discovery of `a` and `b` (one skill each), as a finished search would.
    #[cfg(test)]
    pub fn land_discovery_for_test(&mut self, a: PathBuf, b: PathBuf) {
        let root = a.parent().unwrap().to_path_buf();
        self.land_discovery(root, vec![(a, 1), (b, 1)]);
    }

    pub fn toggle_found(&mut self, i: usize) {
        if let Some(f) = &mut self.found
            && let Some(p) = f.projects.get_mut(i)
        {
            p.2 = !p.2;
        }
    }

    pub fn add_found(&mut self) {
        let Some(found) = self.found.take() else { return };
        let picked: Vec<PathBuf> = found.projects.into_iter().filter(|p| p.2).map(|p| p.0).collect();
        for p in &picked {
            self.settings.add_project(p.clone());
        }
        self.settings.save();
        self.refresh_stats();
        let n = picked.len();
        self.notify(format!("Added {n} project{}", if n == 1 { "" } else { "s" }), String::new(), false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_projects_with_skills_and_skips_the_rest() {
        let home = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        let paths = Paths::isolated(&root);
        let skill = |dir: &Path| {
            fs::create_dir_all(dir.join("x")).unwrap();
            fs::write(dir.join("x/SKILL.md"), "---\nname: x\n---\n").unwrap();
        };
        let dev = root.join("dev");
        skill(&dev.join("app/.claude/skills"));
        skill(&dev.join("group/lib/.agents/skills"));
        skill(&dev.join("app/node_modules/pkg/.claude/skills"));
        fs::create_dir_all(dev.join("empty/.claude/skills")).unwrap();

        let found = find_projects(&dev, &paths);
        assert_eq!(found, [(dev.join("app"), 1), (dev.join("group/lib"), 1)]);
    }
}
