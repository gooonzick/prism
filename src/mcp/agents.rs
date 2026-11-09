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
    #[allow(dead_code)] // the add/copy commands print it
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

#[allow(dead_code)] // the write commands resolve `--agent` with it
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

/// OpenCode reads every config file it finds and merges them: `opencode.json` and `opencode.jsonc`
/// in each of its folders. Project folders are the project root and `.opencode/`.
fn opencode_candidates(scope: &Scope, paths: &Paths) -> Vec<PathBuf> {
    let dirs = match scope {
        Scope::Global => vec![paths.config.join("opencode")],
        Scope::Project(root) => vec![root.clone(), root.join(".opencode")],
    };
    dirs.into_iter().flat_map(|d| [d.join("opencode.json"), d.join("opencode.jsonc")]).collect()
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
                Flavor::OpenCode => opencode_candidates(scope, paths).into_iter().filter(|p| p.is_file()).map(|p| (p, Place::Global)).collect(),
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
            Flavor::OpenCode => opencode_candidates(scope, paths).into_iter().filter(|p| p.is_file()).map(|p| (p, Place::Project(root.clone()))).collect(),
            Flavor::Gemini => project(".gemini/settings.json"),
            Flavor::Kiro => project(".kiro/settings/mcp.json"),
        }
    }

    /// Where a new server goes in `scope`. Mostly the one file the agent reads. OpenCode reads
    /// several: the first that already has an `mcp` key, else an existing `.jsonc`, else an
    /// existing `.json`, else a new `opencode.json` in the scope's primary folder.
    #[allow(dead_code)] // the add and copy commands use it
    pub fn new_server_file(&self, scope: &Scope, paths: &Paths) -> (PathBuf, Place) {
        let files = self.files(scope, paths);
        if self.flavor != Flavor::OpenCode {
            return files.into_iter().next().unwrap_or_else(|| (paths.home.clone(), Place::Global));
        }
        let has_mcp = |p: &PathBuf| {
            std::fs::read_to_string(p).ok().and_then(|t| jsonc_parser::parse_to_serde_value(&t, &Default::default()).ok()).is_some_and(|v: serde_json::Value| v.get("mcp").is_some())
        };
        let ext = |e: &str| files.iter().find(|(p, _)| p.extension().is_some_and(|x| x == e));
        if let Some(found) = files.iter().find(|(p, _)| has_mcp(p)).or_else(|| ext("jsonc")).or_else(|| ext("json")) {
            return found.clone();
        }
        match scope {
            Scope::Global => (paths.config.join("opencode/opencode.json"), Place::Global),
            Scope::Project(root) => (root.join("opencode.json"), Place::Project(root.clone())),
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
        let opencode = &MCP_AGENTS[index_of("opencode").unwrap()];
        assert_eq!(opencode.new_server_file(&Scope::Global, &p).0, PathBuf::from("/h/.config/opencode/opencode.json"));
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
    fn opencode_reads_every_existing_file() {
        let home = tempfile::tempdir().unwrap();
        let p = Paths::isolated(home.path());
        let dir = p.config.join("opencode");
        std::fs::create_dir_all(&dir).unwrap();
        let new_file = |scope: &Scope| MCP_AGENTS[index_of("opencode").unwrap()].new_server_file(scope, &p).0;
        assert!(files("opencode", &Scope::Global, &p).is_empty());
        assert_eq!(new_file(&Scope::Global), dir.join("opencode.json"));
        std::fs::write(dir.join("opencode.json"), "{}").unwrap();
        std::fs::write(dir.join("opencode.jsonc"), "{}").unwrap();
        let read: Vec<_> = files("opencode", &Scope::Global, &p).into_iter().map(|f| f.0).collect();
        assert_eq!(read, [dir.join("opencode.json"), dir.join("opencode.jsonc")]);
        assert_eq!(new_file(&Scope::Global), dir.join("opencode.jsonc"), "an existing .jsonc comes first");
        std::fs::write(dir.join("opencode.json"), r#"{"mcp":{}}"#).unwrap();
        assert_eq!(new_file(&Scope::Global), dir.join("opencode.json"), "a file with an mcp key wins");
        let root = home.path().join("app");
        std::fs::create_dir_all(root.join(".opencode")).unwrap();
        std::fs::write(root.join(".opencode/opencode.json"), "{}").unwrap();
        std::fs::write(root.join("opencode.jsonc"), "{}").unwrap();
        let project: Vec<_> = files("opencode", &Scope::Project(root.clone()), &p).into_iter().map(|f| f.0).collect();
        assert_eq!(project, [root.join("opencode.jsonc"), root.join(".opencode/opencode.json")]);
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
