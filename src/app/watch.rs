//! Rescanning when an agent's skills folder changes on disk (a CLI install, an editor, git).

use std::sync::atomic::Ordering;

use notify::{RecursiveMode, Watcher};

use super::App;
use crate::agents::AGENTS;

impl App {
    /// Watches the current scope's skills folders, replacing the previous watch. Folders that
    /// don't exist yet are watched through their parent, so creating one is seen too.
    pub(super) fn watch(&mut self) {
        self.watcher = None;
        let Some(waker) = self.waker.clone() else { return };
        let dirty = self.dirty.clone();
        let Ok(mut watcher) = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let relevant = event.is_ok_and(|e| !e.kind.is_access());
            if relevant && !dirty.swap(true, Ordering::AcqRel) {
                waker();
            }
        }) else {
            return;
        };
        let mut watched = Vec::new();
        for agent in AGENTS {
            let dir = agent.dir(&self.scope, &self.paths);
            let (path, mode) = match dir.ancestors().find(|p| p.is_dir()) {
                Some(p) if p == dir => (p.to_path_buf(), RecursiveMode::Recursive),
                // Only the nearest existing parent, not all of home.
                Some(p) if p != self.paths.home && !self.paths.home.starts_with(p) => (p.to_path_buf(), RecursiveMode::NonRecursive),
                _ => continue,
            };
            if !watched.contains(&path) && watcher.watch(&path, mode).is_ok() {
                watched.push(path);
            }
        }
        self.watcher = Some(watcher);
    }

    /// A change landed since the last look.
    pub(super) fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use crate::agents::{Paths, Scope};
    use crate::app::App;
    use crate::store::Settings;

    #[test]
    fn a_skill_added_on_disk_shows_up_without_a_rescan() {
        let home = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        let project = root.join("app");
        fs::create_dir_all(project.join(".claude/skills")).unwrap();
        let mut app = App::new(Paths::isolated(&root), Settings::default());
        app.open_scope(Scope::Project(project.clone()));

        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = wakes.clone();
        app.waker = Some(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        app.watch();
        std::thread::sleep(Duration::from_millis(300));

        let dir = project.join(".claude/skills/fresh");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "---\nname: fresh\ndescription: New\n---\n").unwrap();

        let start = Instant::now();
        while wakes.load(Ordering::SeqCst) == 0 {
            assert!(start.elapsed() < Duration::from_secs(5), "no change was reported");
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(app.poll(), "the change lands on the next poll");
        assert!(app.inv.get("fresh").is_some());
    }
}
