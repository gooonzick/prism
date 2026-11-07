//! The app's own settings: the projects it knows and the agents a sync links into.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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

fn file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("dev.prism.skills/settings.json"))
}

/// Settings from before the rename to Prism move over on first launch.
fn migrate() {
    let (Some(new), Some(config)) = (file(), dirs::config_dir()) else { return };
    let old = config.join("skills-explorer");
    if !new.exists() && old.join("settings.json").exists() {
        let _ = std::fs::rename(&old, new.parent().unwrap());
    }
}

impl Settings {
    pub fn load() -> Settings {
        migrate();
        file().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).map(|s: Settings| Settings { persist: true, ..s })
            .unwrap_or(Settings { persist: true, ..Settings::default() })
    }

    pub fn save(&self) {
        let Some(f) = file().filter(|_| self.persist) else { return };
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
