//! Where each coding agent keeps its skills, globally (under the home folder) and per project.
//!
//! `.agents/skills` is the canonical store: a skill lives there once, and every other agent's
//! folder holds a symlink to it. That is the layout `skills` (skills.sh) installs, so skills
//! added by its CLI and skills synced here look the same.

use std::path::{Path, PathBuf};

/// Home and config folders, read once from the environment so tests can point them elsewhere.
#[derive(Clone, Debug, PartialEq)]
pub struct Paths {
    pub home: PathBuf,
    /// `$XDG_CONFIG_HOME`, else `~/.config`.
    pub config: PathBuf,
    /// `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
    pub claude: PathBuf,
    /// `$CODEX_HOME`, else `~/.codex`.
    pub codex: PathBuf,
}

impl Paths {
    pub fn from_env() -> Paths {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        Paths::with_home(home)
    }

    pub fn with_home(home: PathBuf) -> Paths {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        Paths {
            config: var("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")),
            claude: var("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home.join(".claude")),
            codex: var("CODEX_HOME").unwrap_or_else(|| home.join(".codex")),
            home,
        }
    }

    /// Paths rooted entirely in `home`, ignoring the environment.
    #[cfg(test)]
    pub fn isolated(home: &Path) -> Paths {
        Paths { home: home.into(), config: home.join(".config"), claude: home.join(".claude"), codex: home.join(".codex") }
    }
}

/// Global skills (the home folder) or one project's.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    Global,
    Project(PathBuf),
}

impl Scope {
    pub fn is_global(&self) -> bool {
        matches!(self, Scope::Global)
    }

    /// The folder commands run in: home for global, the project folder otherwise.
    pub fn root(&self, paths: &Paths) -> PathBuf {
        match self {
            Scope::Global => paths.home.clone(),
            Scope::Project(p) => p.clone(),
        }
    }

    pub fn title(&self) -> String {
        match self {
            Scope::Global => "Global".into(),
            Scope::Project(p) => p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()),
        }
    }
}

#[derive(Clone, Copy)]
enum Base {
    Home,
    Config,
    Claude,
    Codex,
}

pub struct Agent {
    /// Stable id, saved in settings.
    pub id: &'static str,
    pub name: &'static str,
    /// For narrow places: the status chips.
    pub short: &'static str,
    /// Relative to a project root.
    project: &'static str,
    /// Relative to `global_base`.
    global: (Base, &'static str),
    /// A folder whose presence means the agent is installed, relative to `global_base`.
    marker: (Base, &'static str),
}

/// The canonical store. Always index 0.
pub const CANONICAL: usize = 0;

pub static AGENTS: &[Agent] = &[
    Agent { id: "agents", name: ".agents", short: ".agents", project: ".agents/skills", global: (Base::Home, ".agents/skills"), marker: (Base::Home, ".agents") },
    Agent { id: "claude", name: "Claude Code", short: "Claude", project: ".claude/skills", global: (Base::Claude, "skills"), marker: (Base::Claude, "") },
    Agent { id: "codex", name: "Codex", short: "Codex", project: ".codex/skills", global: (Base::Codex, "skills"), marker: (Base::Codex, "") },
    Agent { id: "opencode", name: "OpenCode", short: "OpenCode", project: ".opencode/skills", global: (Base::Config, "opencode/skills"), marker: (Base::Config, "opencode") },
    Agent { id: "cursor", name: "Cursor", short: "Cursor", project: ".cursor/skills", global: (Base::Home, ".cursor/skills"), marker: (Base::Home, ".cursor") },
    Agent { id: "gemini", name: "Gemini CLI", short: "Gemini", project: ".gemini/skills", global: (Base::Home, ".gemini/skills"), marker: (Base::Home, ".gemini") },
    Agent { id: "copilot", name: "GitHub Copilot", short: "Copilot", project: ".github/skills", global: (Base::Home, ".copilot/skills"), marker: (Base::Home, ".copilot") },
    Agent { id: "windsurf", name: "Windsurf", short: "Windsurf", project: ".windsurf/skills", global: (Base::Home, ".codeium/windsurf/skills"), marker: (Base::Home, ".codeium/windsurf") },
    Agent { id: "goose", name: "Goose", short: "Goose", project: ".goose/skills", global: (Base::Config, "goose/skills"), marker: (Base::Config, "goose") },
    Agent { id: "kiro", name: "Kiro", short: "Kiro", project: ".kiro/skills", global: (Base::Home, ".kiro/skills"), marker: (Base::Home, ".kiro") },
    Agent { id: "roo", name: "Roo Code", short: "Roo", project: ".roo/skills", global: (Base::Home, ".roo/skills"), marker: (Base::Home, ".roo") },
];

fn base(paths: &Paths, b: Base) -> &Path {
    match b {
        Base::Home => &paths.home,
        Base::Config => &paths.config,
        Base::Claude => &paths.claude,
        Base::Codex => &paths.codex,
    }
}

impl Agent {
    /// The agent's skills folder in `scope`.
    pub fn dir(&self, scope: &Scope, paths: &Paths) -> PathBuf {
        match scope {
            Scope::Global => base(paths, self.global.0).join(self.global.1),
            Scope::Project(root) => root.join(self.project),
        }
    }

    /// The agent's skills folder as a short label: `~/.claude/skills`, `.codex/skills`.
    pub fn dir_label(&self, scope: &Scope, paths: &Paths) -> String {
        match scope {
            Scope::Global => tilde(&self.dir(scope, paths), paths),
            Scope::Project(_) => self.project.into(),
        }
    }

    /// Whether the agent looks installed on this machine.
    pub fn detected(&self, paths: &Paths) -> bool {
        base(paths, self.marker.0).join(self.marker.1).is_dir()
    }
}

pub fn index_of(id: &str) -> Option<usize> {
    AGENTS.iter().position(|a| a.id == id)
}

/// `path` with the home folder written as `~`.
pub fn tilde(path: &Path, paths: &Paths) -> String {
    match path.strip_prefix(&paths.home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// Agents to sync into by default: the installed ones, except the canonical store.
pub fn default_targets(paths: &Paths) -> Vec<String> {
    AGENTS.iter().skip(1).filter(|a| a.detected(paths)).map(|a| a.id.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_dirs_follow_each_agents_home() {
        let p = Paths::isolated(Path::new("/h"));
        let dir = |id| AGENTS[index_of(id).unwrap()].dir(&Scope::Global, &p);
        assert_eq!(dir("agents"), PathBuf::from("/h/.agents/skills"));
        assert_eq!(dir("claude"), PathBuf::from("/h/.claude/skills"));
        assert_eq!(dir("codex"), PathBuf::from("/h/.codex/skills"));
        assert_eq!(dir("opencode"), PathBuf::from("/h/.config/opencode/skills"));
    }

    #[test]
    fn project_dirs_sit_under_the_project() {
        let p = Paths::isolated(Path::new("/h"));
        let scope = Scope::Project("/work/app".into());
        assert_eq!(AGENTS[CANONICAL].dir(&scope, &p), PathBuf::from("/work/app/.agents/skills"));
        assert_eq!(AGENTS[index_of("opencode").unwrap()].dir(&scope, &p), PathBuf::from("/work/app/.opencode/skills"));
        assert_eq!(scope.title(), "app");
    }

    #[test]
    fn ids_are_unique_and_canonical_is_first() {
        assert_eq!(AGENTS[CANONICAL].id, "agents");
        for (i, a) in AGENTS.iter().enumerate() {
            assert_eq!(index_of(a.id), Some(i), "{} is listed twice", a.id);
        }
    }
}
