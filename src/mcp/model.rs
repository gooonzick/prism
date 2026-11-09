//! One shape for a server across agents, so copies in different configs can be compared.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A piece of an env or header value: literal text or a reference to an environment variable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Text(String),
    Var { name: String, default: Option<String> },
}

/// An env or header value in agent-neutral form: `Bearer ${TOKEN}` is `[Text("Bearer "), Var(TOKEN)]`,
/// whatever syntax the agent wrote it in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvValue(pub Vec<Part>);

impl EnvValue {
    pub fn literal(text: &str) -> EnvValue {
        EnvValue(vec![Part::Text(text.into())])
    }

    pub fn var(name: &str) -> EnvValue {
        EnvValue(vec![Part::Var { name: name.into(), default: None }])
    }

    /// No variable references: the value itself is in the file, so it may be a secret.
    pub fn is_literal(&self) -> bool {
        self.0.iter().all(|p| matches!(p, Part::Text(_)))
    }

    /// For printing: references stay readable (`${GITLAB_TOKEN}`, `Bearer ${TOKEN}`); any other
    /// text shows as `***`, since a secret can sit around a reference too (`https://u:pw@${HOST}`).
    /// Defaults may be secrets, so they are masked.
    pub fn masked(&self) -> String {
        if self.is_literal() {
            return "***".into();
        }
        self.0
            .iter()
            .enumerate()
            .map(|(i, p)| match p {
                Part::Text(t) if i == 0 && t.starts_with("Bearer ") => if t.len() == "Bearer ".len() { t.clone() } else { "Bearer ***".into() },
                Part::Text(_) => "***".into(),
                Part::Var { name, default: None } => format!("${{{name}}}"),
                Part::Var { name, default: Some(_) } => format!("${{{name}:-***}}"),
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transport {
    Stdio { command: String, args: Vec<String>, env: BTreeMap<String, EnvValue>, cwd: Option<String> },
    /// `sse` where the agent tells SSE apart from streamable HTTP.
    Http { url: String, headers: BTreeMap<String, EnvValue>, sse: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub name: String,
    pub transport: Transport,
}

/// Where a copy lives, as Prism shows it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    Global,
    /// Claude Code's local scope: `projects[<path>].mcpServers` in `~/.claude.json`. Belongs to that project.
    ClaudeLocal(PathBuf),
    /// A config file inside the project.
    Project(PathBuf),
}

/// The fields of a copy that Prism doesn't model, written back untouched.
/// Codex TOML extras are converted to JSON values and back.
pub type Extras = serde_json::Map<String, serde_json::Value>;

/// One agent's copy of a server. (Not `Copy`: that would shadow the trait.)
#[derive(Clone, Debug, PartialEq)]
pub struct McpCopy {
    /// Index into `mcp::agents::MCP_AGENTS`.
    pub agent: usize,
    pub file: PathBuf,
    pub place: Place,
    pub server: Server,
    pub extras: Extras,
    pub enabled: bool,
    /// `Some(app name)` when an app manages this copy; Prism doesn't edit it.
    pub read_only: Option<String>,
}

/// The app that owns a server whose command lives inside an app bundle
/// (`/Applications/Pencil.app/Contents/...` → `Pencil`). The app rewrites its entry itself,
/// so Prism's edits would be lost or would break it.
pub fn read_only(server: &Server) -> Option<String> {
    let Transport::Stdio { command, .. } = &server.transport else { return None };
    let end = command.find(".app/")? + ".app".len();
    let bundle = Path::new(&command[..end]);
    bundle.file_stem().map(|s| s.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdio(command: &str) -> Server {
        Server { name: "x".into(), transport: Transport::Stdio { command: command.into(), args: vec![], env: BTreeMap::new(), cwd: None } }
    }

    #[test]
    fn app_bundle_command_is_read_only() {
        assert_eq!(read_only(&stdio("/Applications/Pencil.app/Contents/Resources/mcp-server")), Some("Pencil".into()));
        assert_eq!(read_only(&stdio("/Applications/Visual Studio Code.app/Contents/MacOS/code")), Some("Visual Studio Code".into()));
        assert_eq!(read_only(&stdio("npx")), None);
        assert_eq!(read_only(&stdio("/opt/tools/my.application/bin/x")), None);
        let http = Server { name: "x".into(), transport: Transport::Http { url: "https://a.app/mcp".into(), headers: BTreeMap::new(), sse: false } };
        assert_eq!(read_only(&http), None);
    }

    #[test]
    fn masked_hides_literals_and_keeps_references() {
        let text = |t: &str| Part::Text(t.into());
        let var = |n: &str| Part::Var { name: n.into(), default: None };
        assert_eq!(EnvValue::literal("glpat-fake").masked(), "***");
        assert_eq!(EnvValue::literal("Bearer fake").masked(), "***");
        assert_eq!(EnvValue::var("GITLAB_TOKEN").masked(), "${GITLAB_TOKEN}");
        let bearer = EnvValue(vec![text("Bearer "), Part::Var { name: "T".into(), default: Some("fake".into()) }]);
        assert!(!bearer.is_literal());
        assert_eq!(bearer.masked(), "Bearer ${T:-***}");
        assert_eq!(EnvValue(vec![text("Bearer "), var("T")]).masked(), "Bearer ${T}");
        assert_eq!(EnvValue(vec![text("https://u:s3cret@"), var("HOST"), text("/x")]).masked(), "***${HOST}***");
        assert_eq!(EnvValue(vec![text("Bearer s3cret-"), var("T")]).masked(), "Bearer ***${T}");
    }
}
