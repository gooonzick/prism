//! Which agents hold MCP servers, and in which files.
//!
//! Every `agent: usize` in `mcp` indexes `MCP_AGENTS`, not `agents::AGENTS`: Claude Desktop has
//! MCP servers but no skills, and the skills-only agents have no MCP config Prism reads.

use std::path::PathBuf;

use crate::agents::{self, Paths, Scope};
use crate::mcp::model::Place;

/// The config dialect an agent writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    ClaudeCode,
    ClaudeDesktop,
    Codex,
    Cursor,
    OpenCode,
    Gemini,
    Windsurf,
    Kiro,
}

pub struct McpAgent {
    /// Same id as in `agents::AGENTS` where the agent is in both.
    pub id: &'static str,
    pub name: &'static str,
    pub flavor: Flavor,
}

pub static MCP_AGENTS: &[McpAgent] = &[
    McpAgent { id: "claude", name: "Claude Code", flavor: Flavor::ClaudeCode },
    McpAgent { id: "claude-desktop", name: "Claude Desktop", flavor: Flavor::ClaudeDesktop },
    McpAgent { id: "codex", name: "Codex", flavor: Flavor::Codex },
    McpAgent { id: "cursor", name: "Cursor", flavor: Flavor::Cursor },
    McpAgent { id: "opencode", name: "OpenCode", flavor: Flavor::OpenCode },
    McpAgent { id: "gemini", name: "Gemini CLI", flavor: Flavor::Gemini },
    McpAgent { id: "windsurf", name: "Windsurf", flavor: Flavor::Windsurf },
    McpAgent { id: "kiro", name: "Kiro", flavor: Flavor::Kiro },
];

pub fn index_of(id: &str) -> Option<usize> {
    MCP_AGENTS.iter().position(|a| a.id == id)
}

/// Claude Code's own state file: `~/.claude.json`, or `$CLAUDE_CONFIG_DIR/.claude.json` when that
/// is set. `Paths` only keeps the resolved folder, so a `CLAUDE_CONFIG_DIR` pointing at `~/.claude`
/// itself reads as unset.
fn claude_json(paths: &Paths) -> PathBuf {
    if paths.claude == paths.home.join(".claude") { paths.home.join(".claude.json") } else { paths.claude.join(".claude.json") }
}

/// Claude Desktop's folder: macOS keeps it under Application Support, Linux under the XDG config.
fn desktop_dir(paths: &Paths) -> PathBuf {
    if cfg!(target_os = "macos") { paths.home.join("Library/Application Support/Claude") } else { paths.config.join("Claude") }
}

/// `name.jsonc` when it exists, else `name.json`: OpenCode reads either.
fn json_or_jsonc(dir: PathBuf, name: &str) -> PathBuf {
    let jsonc = dir.join(format!("{name}.jsonc"));
    if jsonc.is_file() { jsonc } else { dir.join(format!("{name}.json")) }
}

impl McpAgent {
    /// The config files this agent reads in `scope`, existing or not, with the place each stands for.
    pub fn files(&self, scope: &Scope, paths: &Paths) -> Vec<(PathBuf, Place)> {
        let global = |p: PathBuf| vec![(p, Place::Global)];
        let Scope::Project(root) = scope else {
            return match self.flavor {
                Flavor::ClaudeCode => global(claude_json(paths)),
                Flavor::ClaudeDesktop => global(desktop_dir(paths).join("claude_desktop_config.json")),
                Flavor::Codex => global(paths.codex.join("config.toml")),
                Flavor::Cursor => global(paths.home.join(".cursor/mcp.json")),
                Flavor::OpenCode => global(json_or_jsonc(paths.config.join("opencode"), "opencode")),
                Flavor::Gemini => global(paths.home.join(".gemini/settings.json")),
                Flavor::Windsurf => global(paths.home.join(".codeium/windsurf/mcp_config.json")),
                Flavor::Kiro => global(paths.home.join(".kiro/settings/mcp.json")),
            };
        };
        let project = |rel: &str| vec![(root.join(rel), Place::Project(root.clone()))];
        match self.flavor {
            Flavor::ClaudeCode => vec![(claude_json(paths), Place::ClaudeLocal(root.clone())), (root.join(".mcp.json"), Place::Project(root.clone()))],
            Flavor::ClaudeDesktop | Flavor::Windsurf => vec![],
            // Codex reads it only in projects the user trusted.
            Flavor::Codex => project(".codex/config.toml"),
            Flavor::Cursor => project(".cursor/mcp.json"),
            Flavor::OpenCode => vec![(json_or_jsonc(root.clone(), "opencode"), Place::Project(root.clone()))],
            Flavor::Gemini => project(".gemini/settings.json"),
            Flavor::Kiro => project(".kiro/settings/mcp.json"),
        }
    }

    /// Installed, by the same markers as for skills; Claude Desktop by its config folder.
    pub fn detected(&self, paths: &Paths) -> bool {
        match self.flavor {
            Flavor::ClaudeDesktop => desktop_dir(paths).is_dir(),
            Flavor::ClaudeCode | Flavor::Codex | Flavor::Cursor | Flavor::OpenCode | Flavor::Gemini | Flavor::Windsurf | Flavor::Kiro => {
                agents::index_of(self.id).is_some_and(|i| agents::AGENTS[i].detected(paths))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn files(id: &str, scope: &Scope, paths: &Paths) -> Vec<(PathBuf, Place)> {
        MCP_AGENTS[index_of(id).unwrap()].files(scope, paths)
    }

    #[test]
    fn ids_are_unique_and_shared_ones_match_the_skills_table() {
        for (i, a) in MCP_AGENTS.iter().enumerate() {
            assert_eq!(index_of(a.id), Some(i), "{} is listed twice", a.id);
            if let Some(j) = agents::index_of(a.id) {
                assert_eq!(agents::AGENTS[j].name, a.name);
            }
        }
    }

    #[test]
    fn global_files_follow_each_agents_home() {
        let p = Paths::isolated(Path::new("/h"));
        let one = |id| files(id, &Scope::Global, &p).remove(0);
        assert_eq!(one("claude"), (PathBuf::from("/h/.claude.json"), Place::Global));
        assert_eq!(one("codex").0, PathBuf::from("/h/.codex/config.toml"));
        assert_eq!(one("opencode").0, PathBuf::from("/h/.config/opencode/opencode.json"));
        assert_eq!(one("windsurf").0, PathBuf::from("/h/.codeium/windsurf/mcp_config.json"));
        assert_eq!(one("kiro").0, PathBuf::from("/h/.kiro/settings/mcp.json"));
        let mut moved = p.clone();
        moved.claude = "/elsewhere/claude".into();
        assert_eq!(files("claude", &Scope::Global, &moved)[0].0, PathBuf::from("/elsewhere/claude/.claude.json"));
    }

    #[test]
    fn claude_project_scope_reads_local_and_mcp_json() {
        let p = Paths::isolated(Path::new("/h"));
        let root = PathBuf::from("/work/app");
        assert_eq!(
            files("claude", &Scope::Project(root.clone()), &p),
            vec![(PathBuf::from("/h/.claude.json"), Place::ClaudeLocal(root.clone())), (root.join(".mcp.json"), Place::Project(root.clone()))]
        );
        assert_eq!(files("codex", &Scope::Project(root.clone()), &p), vec![(root.join(".codex/config.toml"), Place::Project(root.clone()))]);
        assert!(files("windsurf", &Scope::Project(root.clone()), &p).is_empty());
        assert!(files("claude-desktop", &Scope::Project(root), &p).is_empty());
    }

    #[test]
    fn opencode_prefers_jsonc_when_it_exists() {
        let home = tempfile::tempdir().unwrap();
        let p = Paths::isolated(home.path());
        let dir = p.config.join("opencode");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(files("opencode", &Scope::Global, &p)[0].0, dir.join("opencode.json"));
        std::fs::write(dir.join("opencode.jsonc"), "{}").unwrap();
        assert_eq!(files("opencode", &Scope::Global, &p)[0].0, dir.join("opencode.jsonc"));
    }

    #[test]
    fn detection_uses_the_skills_markers_and_the_desktop_folder() {
        let home = tempfile::tempdir().unwrap();
        let p = Paths::isolated(home.path());
        let desktop = &MCP_AGENTS[index_of("claude-desktop").unwrap()];
        let cursor = &MCP_AGENTS[index_of("cursor").unwrap()];
        assert!(!desktop.detected(&p) && !cursor.detected(&p));
        std::fs::create_dir_all(desktop_dir(&p)).unwrap();
        std::fs::create_dir_all(home.path().join(".cursor")).unwrap();
        assert!(desktop.detected(&p) && cursor.detected(&p));
    }
}
