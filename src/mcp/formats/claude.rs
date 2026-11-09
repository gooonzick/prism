//! Claude Code: `~/.claude.json` holds both the user scope (top-level `mcpServers`) and every
//! project's local scope (`projects[<abs path>].mcpServers`); `.mcp.json` holds the project scope.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use super::{Entry, mcp_servers, parse_json};
use crate::mcp::agents::Flavor;
use crate::mcp::model::Place;

/// The `projects` entry for `path`. Claude Code keys it by the path its process saw, which is
/// usually the resolved one, so the canonical form is tried first, then `path` as given.
fn project<'a>(doc: &'a Value, path: &Path) -> Option<&'a Value> {
    let projects = doc.get("projects")?;
    let canonical = std::fs::canonicalize(path).ok();
    canonical.as_deref().into_iter().chain([path]).find_map(|p| projects.get(p.to_string_lossy().as_ref()))
}

pub(super) fn read(doc: &Value, place: &Place) -> Result<Vec<Entry>, String> {
    let servers = match place {
        Place::ClaudeLocal(path) => project(doc, path).and_then(|p| p.get("mcpServers")),
        Place::Global | Place::Project(_) => doc.get("mcpServers"),
    };
    mcp_servers::read(Flavor::ClaudeCode, servers)
}

/// Servers Claude Code skips in a project, kept apart because each list covers different copies.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Disabled {
    /// `disabledMcpServers`: user-scope and local-scope servers.
    pub servers: Vec<String>,
    /// `disabledMcpjsonServers`: `.mcp.json` servers.
    pub mcp_json: Vec<String>,
}

/// Claude Code's disabled lists for `project_path`, from the text of `~/.claude.json`.
/// Unreadable text has none.
pub(super) fn disabled_in(text: &str, project_path: &Path) -> Disabled {
    let Ok(doc) = parse_json(text) else { return Disabled::default() };
    let Some(entry) = project(&doc, project_path) else { return Disabled::default() };
    let names = |key: &str| -> Vec<String> {
        let list = entry.get(key).and_then(Value::as_array).into_iter().flatten();
        list.filter_map(|n| n.as_str().map(str::to_owned)).collect::<BTreeSet<_>>().into_iter().collect()
    };
    Disabled { servers: names("disabledMcpServers"), mcp_json: names("disabledMcpjsonServers") }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::read as read_file;
    use super::*;
    use crate::mcp::model::{EnvValue, Part, Transport};

    const CLAUDE_JSON: &str = r#"{
      "numStartups": 12,
      "mcpServers": {
        "gitlab": { "type": "stdio", "command": "npx", "args": ["-y", "@zereight/mcp-gitlab"],
                    "env": { "GITLAB_PERSONAL_ACCESS_TOKEN": "${GITLAB_TOKEN}", "GITLAB_API_URL": "${GITLAB_URL:-https://gitlab.example.com}/api/v4", "READ_ONLY": "true" } },
        "context7": { "type": "http", "url": "https://mcp.context7.com/mcp", "headers": { "CONTEXT7_API_KEY": "fake-ctx7-key" } }
      },
      "projects": {
        "/work/app": {
          "allowedTools": [],
          "mcpServers": { "figma": { "type": "sse", "url": "http://127.0.0.1:3845/sse" } },
          "disabledMcpServers": ["gitlab"],
          "disabledMcpjsonServers": ["shared", "gitlab"]
        }
      }
    }"#;

    const MCP_JSON: &str = r#"{ "mcpServers": { "shared": { "command": "uvx", "args": ["shared-mcp"] } } }"#;

    #[test]
    fn claude_reads_user_local_and_project_scopes() {
        let user = read_file(Flavor::ClaudeCode, CLAUDE_JSON, &Place::Global).unwrap();
        assert_eq!(user.iter().map(|(s, _, _)| s.name.as_str()).collect::<Vec<_>>(), ["context7", "gitlab"]);
        let Transport::Http { url, headers, sse } = &user[0].0.transport else { panic!("context7 is http") };
        assert_eq!((url.as_str(), *sse), ("https://mcp.context7.com/mcp", false));
        assert_eq!(headers["CONTEXT7_API_KEY"], EnvValue::literal("fake-ctx7-key"));
        let Transport::Stdio { command, env, .. } = &user[1].0.transport else { panic!("gitlab is stdio") };
        assert_eq!(command, "npx");
        assert_eq!(env["GITLAB_PERSONAL_ACCESS_TOKEN"], EnvValue::var("GITLAB_TOKEN"));
        assert_eq!(env["GITLAB_API_URL"].0[0], Part::Var { name: "GITLAB_URL".into(), default: Some("https://gitlab.example.com".into()) });
        assert!(env["READ_ONLY"].is_literal());

        let local = read_file(Flavor::ClaudeCode, CLAUDE_JSON, &Place::ClaudeLocal("/work/app".into())).unwrap();
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].0.transport, Transport::Http { url: "http://127.0.0.1:3845/sse".into(), headers: Default::default(), sse: true });
        assert!(read_file(Flavor::ClaudeCode, CLAUDE_JSON, &Place::ClaudeLocal("/work/other".into())).unwrap().is_empty());

        let project = read_file(Flavor::ClaudeCode, MCP_JSON, &Place::Project("/work/app".into())).unwrap();
        assert!(matches!(&project[0].0.transport, Transport::Stdio { command, .. } if command == "uvx"));

        let disabled = disabled_in(CLAUDE_JSON, &PathBuf::from("/work/app"));
        assert_eq!(disabled.servers, ["gitlab"], "disabledMcpServers: user and local servers");
        assert_eq!(disabled.mcp_json, ["gitlab", "shared"], "disabledMcpjsonServers: .mcp.json servers");
        assert_eq!(disabled_in(CLAUDE_JSON, &PathBuf::from("/work/other")), Disabled::default());
    }

    /// Claude Code keys `projects` by the path its process saw; a symlinked checkout still finds it.
    #[cfg(unix)]
    #[test]
    fn local_scope_is_found_by_the_canonical_project_path() {
        let home = tempfile::tempdir().unwrap();
        let real = home.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = home.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let key = std::fs::canonicalize(&real).unwrap();
        let doc = serde_json::json!({ "projects": { key.to_string_lossy(): {
            "mcpServers": { "figma": { "type": "sse", "url": "http://127.0.0.1:3845/sse" } },
            "disabledMcpServers": ["figma"]
        } } })
        .to_string();
        assert_eq!(read_file(Flavor::ClaudeCode, &doc, &Place::ClaudeLocal(link.clone())).unwrap().len(), 1);
        assert_eq!(disabled_in(&doc, &link).servers, ["figma"]);
    }
}
