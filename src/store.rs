//! The app's own settings: the projects it knows and the agents a sync links into.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agents::Paths;
use crate::fsops::Journal;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub projects: Vec<PathBuf>,
    /// Agent ids. `None` until the user picks: then the installed agents are used.
    #[serde(default)]
    pub targets: Option<Vec<String>>,
    /// Loaded from disk, so saved back there. Settings built in tests stay in memory.
    #[serde(skip)]
    pub persist: bool,
}

fn file(paths: &Paths) -> PathBuf {
    paths.state.join("settings.json")
}

/// Settings from before the rename to Prism move over on first launch.
fn migrate(paths: &Paths) {
    let new = file(paths);
    let Some(config) = paths.state.parent() else { return };
    let old = config.join("skills-explorer");
    if !new.exists() && old.join("settings.json").exists() {
        let _ = std::fs::rename(&old, &paths.state);
    }
}

/// The last change, kept on disk so the CLI and the window undo the same thing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LastChange {
    pub label: String,
    pub journal: Journal,
}

fn last_change_file(paths: &Paths) -> PathBuf {
    paths.state.join("last-change.json")
}

/// Saves `change`, or forgets the last change when it is `None`.
pub fn save_last_change(paths: &Paths, change: Option<&LastChange>) {
    let f = last_change_file(paths);
    match change.and_then(|c| serde_json::to_vec_pretty(c).ok()) {
        Some(json) => {
            let _ = std::fs::create_dir_all(&paths.state);
            let _ = std::fs::write(f, json);
        }
        None => {
            let _ = std::fs::remove_file(f);
        }
    }
}

pub fn load_last_change(paths: &Paths) -> Option<LastChange> {
    serde_json::from_slice(&std::fs::read(last_change_file(paths)).ok()?).ok()
}

/// Starts a new change: the backups of the previous one are no longer needed.
pub fn begin_change(paths: &Paths) {
    let dir: &Path = &paths.state.join("backups");
    let _ = std::fs::remove_dir_all(dir);
}

impl Settings {
    pub fn load_from(paths: &Paths) -> Settings {
        migrate(paths);
        std::fs::read(file(paths)).ok().and_then(|b| serde_json::from_slice(&b).ok()).map(|s: Settings| Settings { persist: true, ..s })
            .unwrap_or(Settings { persist: true, ..Settings::default() })
    }

    pub fn save_to(&self, paths: &Paths) {
        if !self.persist {
            return;
        }
        let f = file(paths);
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(f, json);
        }
    }

    pub fn add_project(&mut self, path: PathBuf) -> bool {
        if self.projects.contains(&path) {
            return false;
        }
        self.projects.push(path);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsops::Step;

    #[test]
    fn last_change_round_trips_through_disk() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::isolated(home.path());
        assert_eq!(load_last_change(&paths), None);
        let change = LastChange {
            label: "Sync".into(),
            journal: Journal {
                steps: vec![
                    Step::Linked("/h/.claude/skills/a".into()),
                    Step::Rewrote { path: "/h/.claude.json".into(), backup: Some("/s/backups/0-.claude.json".into()), hash_after: u64::MAX },
                    Step::Rewrote { path: "/h/new.json".into(), backup: None, hash_after: 7 },
                ],
            },
        };
        save_last_change(&paths, Some(&change));
        assert_eq!(load_last_change(&paths), Some(change));
        save_last_change(&paths, None);
        assert_eq!(load_last_change(&paths), None);
    }
}
