//! Rescanning when something the window shows changes on disk: an agent's skills folder or
//! MCP config (a CLI install, an editor, git, the agent itself), or Prism's last change and
//! settings (the CLI).

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
        // As given and with links resolved: inotify reports paths under the watched path as
        // given, FSEvents reports them resolved.
        let mut targets: Vec<PathBuf> = folders.iter().chain(&files).flat_map(|p| [p.clone(), real(p)]).collect();
        targets.dedup();
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

    /// The skills folders and the files (MCP configs, the last change, the settings) the
    /// scope's view reads.
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
        files.push(crate::store::settings_file(&self.paths));
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

    #[test]
    fn a_project_the_cli_adds_shows_up_and_survives_the_window_forgetting_another() {
        let home = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        let paths = Paths::isolated(&root);
        let (old, new) = (root.join("old-app"), root.join("new-app"));
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        let mut settings = Settings::load_from(&paths);
        settings.add_project(old.clone());
        settings.save_to(&paths);
        let mut app = App::new(paths.clone(), Settings::load_from(&paths));
        watching(&mut app);

        let mut cli = crate::cli::Ctx { paths: paths.clone(), settings: Settings::load_from(&paths), stdin: None, path_var: None, make_runner: crate::hosts::runner::no_runner() };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(crate::cli::run_with(&["projects", "add", new.to_str().unwrap()], &mut cli, &mut out, &mut err), 0);
        poll_until(&mut app, "the project the CLI added", |a| a.settings.projects.contains(&new));

        app.forget_project(&old);
        assert_eq!(Settings::load_from(&paths).projects, [new], "the CLI's project stays, the forgotten one goes");
    }

    #[test]
    fn window_settings_changes_keep_what_the_cli_saved_meanwhile() {
        let home = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        let paths = Paths::isolated(&root);
        let (a, b) = (root.join("a"), root.join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        let mut app = App::new(paths.clone(), Settings::load_from(&paths));
        // The CLI saves while the window isn't watching: the window's next change still keeps it.
        let mut cli = Settings::load_from(&paths);
        cli.add_project(a.clone());
        cli.save_to(&paths);
        app.add_project(b.clone());
        assert_eq!(Settings::load_from(&paths).projects, [a, b]);
    }

    /// Starts watching with a waker that does nothing, then gives the watch time to start.
    fn watching(app: &mut App) {
        app.watch_with(Arc::new(|| {}));
        std::thread::sleep(Duration::from_millis(300));
    }

    /// Polls until `done` holds, as the window does on each wake; fails after 5 seconds.
    fn poll_until(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
        let start = Instant::now();
        while !done(app) {
            assert!(start.elapsed() < Duration::from_secs(5), "never saw {what}");
            std::thread::sleep(Duration::from_millis(50));
            app.poll();
        }
    }

    /// Writes `text` the way agents and Prism do: a temp file next to it, renamed over it.
    fn replace(path: &std::path::Path, text: &str) {
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, text).unwrap();
        fs::rename(&tmp, path).unwrap();
    }

    fn claude_json(names: &[&str]) -> String {
        let servers: serde_json::Map<String, serde_json::Value> = names.iter().map(|n| (n.to_string(), serde_json::json!({ "command": format!("{n}-mcp") }))).collect();
        serde_json::json!({ "mcpServers": servers }).to_string()
    }

    #[test]
    fn claude_json_replaced_twice_lands_both_times() {
        let home = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        fs::create_dir_all(root.join(".claude")).unwrap();
        let file = root.join(".claude.json");
        fs::write(&file, claude_json(&["one"])).unwrap();
        let mut app = App::new(Paths::isolated(&root), Settings::default());
        assert!(app.mcp.get("one").is_some());
        watching(&mut app);

        replace(&file, &claude_json(&["one", "two"]));
        poll_until(&mut app, "the first replacement", |a| a.mcp.get("two").is_some());
        // The rename swapped the file the watch was on; the second change needs a new watch.
        replace(&file, &claude_json(&["one", "two", "three"]));
        poll_until(&mut app, "the second replacement", |a| a.mcp.get("three").is_some());
        assert!(!app.paths.state.join("last-change.json").exists(), "only the config changed");
    }

    #[test]
    fn a_project_config_edited_in_place_lands() {
        let home = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        fs::create_dir_all(root.join(".cursor")).unwrap();
        let project = root.join("app");
        let file = project.join(".cursor/mcp.json");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, r#"{"mcpServers":{"db":{"command":"db-mcp"}}}"#).unwrap();
        let mut app = App::new(Paths::isolated(&root), Settings::default());
        app.open_scope(Scope::Project(project));
        watching(&mut app);

        fs::write(&file, r#"{"mcpServers":{"db":{"command":"db-mcp"},"cache":{"command":"cache-mcp"}}}"#).unwrap();
        poll_until(&mut app, "the edit", |a| a.mcp.get("cache").is_some());
    }

    #[test]
    fn a_project_reached_through_a_link_reports_changes() {
        let home = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        fs::create_dir_all(root.join(".cursor")).unwrap();
        let real = root.join("real-app");
        fs::create_dir_all(real.join(".cursor")).unwrap();
        fs::create_dir_all(real.join(".claude/skills")).unwrap();
        let linked = root.join("linked-app");
        std::os::unix::fs::symlink(&real, &linked).unwrap();
        let mut app = App::new(Paths::isolated(&root), Settings::default());
        app.open_scope(Scope::Project(linked.clone()));
        watching(&mut app);

        fs::write(linked.join(".cursor/mcp.json"), r#"{"mcpServers":{"db":{"command":"db-mcp"}}}"#).unwrap();
        poll_until(&mut app, "the new config", |a| a.mcp.get("db").is_some());
        let dir = linked.join(".claude/skills/fresh");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "---\nname: fresh\ndescription: New\n---\n").unwrap();
        poll_until(&mut app, "the new skill", |a| a.inv.get("fresh").is_some());
    }
}
