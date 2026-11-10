//! Add project…: the folder the user picks, or the projects with skills inside it.

use std::path::PathBuf;

use super::{App, Found};
use crate::agents::Scope;

impl App {
    /// Adds `dir` when it has skills of its own, offers the projects with skills found inside it
    /// when it has none, and adds it as is when nothing inside has skills either. A folder already
    /// listed just opens.
    pub fn add_folder(&mut self, dir: PathBuf) {
        if self.is_remote() {
            return;
        }
        let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
        if self.settings.projects.contains(&dir) {
            self.open_scope(Scope::Project(dir));
            return;
        }
        if self.discovering {
            return;
        }
        let paths = self.paths.clone();
        self.discovering = true;
        self.found = None;
        self.discover.request(dir, move |root| {
            let found = crate::projects::find_projects(&root, &paths);
            (root, found)
        });
    }

    pub(super) fn land_discovery(&mut self, root: PathBuf, found: Vec<(PathBuf, usize)>) {
        self.discovering = false;
        if self.is_remote() {
            return;
        }
        // The folder has skills of its own: it is the project.
        if found.iter().any(|(p, _)| *p == root) {
            self.add_project(root);
            return;
        }
        let known = &self.settings.projects;
        let projects: Vec<(PathBuf, usize, bool)> = found.iter().filter(|(p, _)| !known.contains(p)).map(|(p, n)| (p.clone(), *n, true)).collect();
        if !projects.is_empty() {
            self.found = Some(Found { root, projects });
            return;
        }
        let where_ = crate::agents::tilde(&root, &self.paths);
        if !found.is_empty() {
            self.notify("Nothing new to add".into(), format!("Every project with skills in {where_} is already listed"), false);
        } else if root == self.paths.home {
            // Home is the Global scope, never a project.
            self.notify("No projects with skills found".into(), format!("Nothing in {where_} has skills; pick a project folder instead"), false);
        } else {
            self.add_project(root);
        }
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
        if self.is_remote() {
            return;
        }
        let Some(found) = self.found.take() else { return };
        let picked: Vec<PathBuf> = found.projects.into_iter().filter(|p| p.2).map(|p| p.0).collect();
        self.edit_settings(|s| {
            for p in &picked {
                s.add_project(p.clone());
            }
        });
        self.refresh_stats();
        let n = picked.len();
        self.notify(format!("Added {n} project{}", if n == 1 { "" } else { "s" }), String::new(), false);
    }
}
