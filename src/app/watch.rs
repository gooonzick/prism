//! Rescanning when something the window shows changes on disk: an agent's skills folder or
//! MCP config (a CLI install, an editor, git, the agent itself), or Prism's last change (the CLI).

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use notify::{RecursiveMode, Watcher};

use super::App;
use crate::agents::{AGENTS, Scope};
use crate::mcp::agents::{Flavor, MCP_AGENTS};

impl App {
    /// Watches what the current scope shows, replacing the previous watch. Things that don't
    /// exist yet are watched through their nearest existing parent, so creating them is seen too.
    /// Home itself is never watched as a folder: that would stream every change under it.
    pub(super) fn watch(&mut self) {
        self.watcher = None;
        let Some(waker) = self.waker.clone() else { return };
        let dirty = self.dirty.clone();
        let (folders, files) = self.watch_targets();
        let targets: Vec<PathBuf> = folders.iter().chain(&files).map(|p| real(p)).collect();
        let Ok(mut watcher) = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            // A watched parent folder reports its other entries too; only ours count.
            let relevant = event.is_ok_and(|e| {
                !e.kind.is_access() && (e.paths.is_empty() || e.paths.iter().any(|p| targets.iter().any(|t| t.starts_with(p) || p.starts_with(t))))
            });
            if relevant && !dirty.swap(true, Ordering::AcqRel) {
                waker();
            }
        }) else {
            return;
        };
        let home = &self.paths.home;
        let mut watched: Vec<PathBuf> = Vec::new();
        let mut add = |path: &Path, mode: RecursiveMode| {
            if !watched.iter().any(|w| w == path) && watcher.watch(path, mode).is_ok() {
                watched.push(path.to_path_buf());
            }
        };
        for dir in &folders {
            match dir.ancestors().find(|p| p.is_dir()) {
                Some(p) if p == dir => add(p, RecursiveMode::Recursive),
                Some(p) if p != home && !home.starts_with(p) => add(p, RecursiveMode::NonRecursive),
                _ => {}
            }
        }
        for file in &files {
            let parent = file.parent().unwrap_or(file);
            if parent == home || home.starts_with(parent) {
                // In home (`~/.claude.json`): the file itself, when it's there.
                if file.is_file() {
                    add(file, RecursiveMode::NonRecursive);
                }
                continue;
            }
            if let Some(p) = parent.ancestors().find(|p| p.is_dir())
                && p != home
                && !home.starts_with(p)
            {
                add(p, RecursiveMode::NonRecursive);
            }
        }
        self.watcher = Some(watcher);
    }

    /// The skills folders and the files (MCP configs, the last change) the scope's view reads.
    fn watch_targets(&self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let folders = AGENTS.iter().map(|a| a.dir(&self.scope, &self.paths)).collect();
        let mut scopes = vec![self.scope.clone()];
        // A project shows the global servers too.
        if !self.scope.is_global() {
            scopes.push(Scope::Global);
        }
        let mut files = Vec::new();
        for scope in &scopes {
            for agent in MCP_AGENTS {
                files.extend(agent.files(scope, &self.paths).into_iter().map(|(f, _)| f));
                // OpenCode lists only the files it has; the one a new server goes to may appear.
                if agent.flavor == Flavor::OpenCode {
                    files.push(agent.new_server_file(scope, &self.paths).0);
                }
            }
        }
        files.push(crate::store::last_change_file(&self.paths));
        files.sort();
        files.dedup();
        (folders, files)
    }

    /// Watches for tests, with their own waker; `set_waker` would also go to the network.
    #[cfg(test)]
    pub(crate) fn watch_with(&mut self, waker: rune::Waker) {
        self.waker = Some(waker);
        self.watch();
    }

    /// A change landed since the last look.
    pub(super) fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }
}

/// `path` as the file system reports it in events: its existing part with links resolved
/// (`/var` is `/private/var` on macOS), the rest as given.
fn real(path: &Path) -> PathBuf {
    for base in path.ancestors() {
        if let Ok(resolved) = std::fs::canonicalize(base) {
            return resolved.join(path.strip_prefix(base).unwrap_or(Path::new("")));
        }
    }
    path.to_path_buf()
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

    #[test]
    fn a_change_saved_by_the_cli_reloads_last_change() {
        let home = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        let paths = Paths::isolated(&root);
        fs::create_dir_all(&paths.state).unwrap();
        let mut app = App::new(paths.clone(), Settings::default());
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = wakes.clone();
        app.watch_with(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        std::thread::sleep(Duration::from_millis(300));

        let change = crate::store::LastChange { label: "add fresh".into(), journal: crate::fsops::Journal::default() };
        crate::store::save_last_change(&paths, Some(&change));

        let start = Instant::now();
        while wakes.load(Ordering::SeqCst) == 0 {
            assert!(start.elapsed() < Duration::from_secs(5), "no change was reported");
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(app.poll());
        assert_eq!(app.last_change.map(|c| c.label).as_deref(), Some("add fresh"));
    }
}
