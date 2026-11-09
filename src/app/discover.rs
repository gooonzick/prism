//! Finding projects that already have skills, under a folder the user picks.

use std::path::PathBuf;

use super::{App, Found};

impl App {
    pub fn discover_in(&mut self, root: PathBuf) {
        let paths = self.paths.clone();
        self.discovering = true;
        self.found = None;
        self.discover.request(root, move |root| {
            let found = crate::projects::find_projects(&root, &paths);
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
