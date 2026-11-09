//! The `mcpServers` object most agents share: Claude Code, Claude Desktop, Cursor, Gemini CLI,
//! Windsurf and Kiro. They differ in reference syntax, URL keys and the `type`/`disabled` fields.

use serde_json::Value;

use super::refs::Syntax;
use super::{Entry, Obj, objects, string, strings, values};
use crate::mcp::agents::Flavor;
use crate::mcp::model::{Server, Transport};

pub(super) fn read(flavor: Flavor, servers: Option<&Value>) -> Result<Vec<Entry>, String> {
    objects(servers)?.into_iter().map(|(name, obj)| entry(flavor, name, obj)).collect()
}

fn syntax(flavor: Flavor) -> Syntax {
    match flavor {
        Flavor::ClaudeCode => Syntax::Shell,
        Flavor::Gemini => Syntax::Dollar,
        Flavor::Cursor | Flavor::Windsurf => Syntax::EnvColon,
        Flavor::Kiro => Syntax::Braced,
        Flavor::ClaudeDesktop | Flavor::Codex | Flavor::OpenCode => Syntax::None,
    }
}

/// URL keys in the order the agent prefers them, and whether each means SSE.
/// Gemini: `httpUrl` is streamable HTTP, `url` is SSE. Windsurf: `serverUrl`, `url` also accepted.
fn url_keys(flavor: Flavor) -> &'static [(&'static str, bool)] {
    match flavor {
        Flavor::Gemini => &[("httpUrl", false), ("url", true)],
        Flavor::Windsurf => &[("serverUrl", false), ("url", false)],
        _ => &[("url", false)],
    }
}

fn entry(flavor: Flavor, name: &str, obj: &Obj) -> Result<Entry, String> {
    let mut rest = obj.clone();
    let syntax = syntax(flavor);
    let what = |key: &str| format!("{name}.{key}");

    // Claude Code needs `type` to tell stdio from http/sse, so it's modelled there; other agents
    // tell by `command` vs `url` and keep `type` as written, using it only as an SSE hint.
    let modelled = flavor == Flavor::ClaudeCode;
    let kind = obj.get("type").and_then(Value::as_str).map(str::to_owned);
    if modelled && matches!(kind.as_deref(), Some("stdio" | "http" | "streamable-http" | "sse")) {
        rest.remove("type");
    }
    let wants_http = modelled && kind.as_deref().is_some_and(|k| k != "stdio");

    // Kiro and Windsurf switch an entry off with `"disabled": true`.
    let mut enabled = true;
    if matches!(flavor, Flavor::Kiro | Flavor::Windsurf)
        && let Some(Value::Bool(off)) = rest.get("disabled")
    {
        enabled = !off;
        rest.remove("disabled");
    }

    let transport = if !wants_http && rest.contains_key("command") {
        let command = string(&rest.remove("command").unwrap_or_default(), &what("command"))?;
        let args = rest.remove("args").map_or(Ok(vec![]), |v| strings(&v, &what("args")))?;
        let env = values(rest.remove("env"), syntax, &what("env"))?;
        let cwd = if flavor == Flavor::Gemini { rest.remove("cwd").map(|v| string(&v, &what("cwd"))).transpose()? } else { None };
        Transport::Stdio { command, args, env, cwd }
    } else {
        let (key, sse) = url_keys(flavor).iter().find(|(k, _)| rest.contains_key(*k)).ok_or_else(|| format!("{name}: neither a command nor a url"))?;
        let url = string(&rest.remove(*key).unwrap_or_default(), &what(key))?;
        let headers = values(rest.remove("headers"), syntax, &what("headers"))?;
        Transport::Http { url, headers, sse: *sse || kind.as_deref() == Some("sse") }
    };
    Ok((Server { name: name.into(), transport }, rest, enabled))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::read as read_file;
    use crate::mcp::model::{EnvValue, Part, Place};
    use super::*;

    fn read(flavor: Flavor, text: &str) -> Vec<Entry> {
        read_file(flavor, text, &Place::Global).unwrap()
    }

    fn http(url: &str, headers: &[(&str, EnvValue)], sse: bool) -> Transport {
        Transport::Http { url: url.into(), headers: headers.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(), sse }
    }

    fn bearer(var: &str) -> EnvValue {
        EnvValue(vec![Part::Text("Bearer ".into()), Part::Var { name: var.into(), default: None }])
    }

    #[test]
    fn cursor_reads_url_without_type_as_http() {
        let got = read(
            Flavor::Cursor,
            r#"{
              "mcpServers": {
                "context7": { "url": "https://mcp.context7.com/mcp", "headers": { "CONTEXT7_API_KEY": "${env:CONTEXT7_API_KEY}" } },
                "figma": { "command": "npx", "args": ["-y", "figma-developer-mcp", "--stdio"], "env": { "FIGMA_API_KEY": "fake-figma" } }
              }
            }"#,
        );
        assert_eq!(got[0].0.transport, http("https://mcp.context7.com/mcp", &[("CONTEXT7_API_KEY", EnvValue::var("CONTEXT7_API_KEY"))], false));
        let Transport::Stdio { command, args, env, cwd } = &got[1].0.transport else { panic!("figma is stdio") };
        assert_eq!((command.as_str(), args.len(), cwd), ("npx", 3, &None));
        assert_eq!(env["FIGMA_API_KEY"], EnvValue::literal("fake-figma"));
        assert!(got.iter().all(|(_, extras, enabled)| extras.is_empty() && *enabled));
    }

    #[test]
    fn gemini_reads_dollar_refs() {
        let got = read(
            Flavor::Gemini,
            r#"{
              "theme": "Default",
              "mcpServers": {
                "gitlab": { "command": "npx", "args": ["-y", "@zereight/mcp-gitlab"], "cwd": "/work", "timeout": 30000,
                            "env": { "GITLAB_TOKEN": "$GITLAB_TOKEN", "GITLAB_API_URL": "${GITLAB_URL}/api/v4" } },
                "events": { "url": "http://localhost:8080/sse" },
                "stream": { "httpUrl": "https://example.com/mcp", "headers": { "Authorization": "Bearer $API_TOKEN" } }
              }
            }"#,
        );
        let names: Vec<_> = got.iter().map(|(s, _, _)| s.name.as_str()).collect();
        assert_eq!(names, ["events", "gitlab", "stream"]);
        assert_eq!(got[0].0.transport, http("http://localhost:8080/sse", &[], true));
        let Transport::Stdio { env, cwd, .. } = &got[1].0.transport else { panic!("gitlab is stdio") };
        assert_eq!(env["GITLAB_TOKEN"], EnvValue::var("GITLAB_TOKEN"));
        assert_eq!(env["GITLAB_API_URL"].0[1], Part::Text("/api/v4".into()));
        assert_eq!(cwd.as_deref(), Some("/work"));
        assert_eq!(got[1].1["timeout"], 30000);
        assert_eq!(got[2].0.transport, http("https://example.com/mcp", &[("Authorization", bearer("API_TOKEN"))], false));
    }

    #[test]
    fn windsurf_reads_server_url() {
        let got = read(
            Flavor::Windsurf,
            r#"{ "mcpServers": {
                  "linear": { "serverUrl": "https://mcp.linear.app/mcp", "headers": { "Authorization": "Bearer ${env:LINEAR_TOKEN}" }, "disabled": true },
                  "old": { "url": "https://old.example.com/sse" }
            } }"#,
        );
        assert_eq!(got[0].0.transport, http("https://mcp.linear.app/mcp", &[("Authorization", bearer("LINEAR_TOKEN"))], false));
        assert!(!got[0].2 && got[0].1.is_empty());
        assert_eq!(got[1].0.transport, http("https://old.example.com/sse", &[], false));
    }

    #[test]
    fn kiro_reads_mcp_servers() {
        let got = read(
            Flavor::Kiro,
            r#"{ "mcpServers": {
                  "brave": { "command": "npx", "args": ["-y", "@brave/brave-search-mcp-server"], "env": { "BRAVE_API_KEY": "${BRAVE_API_KEY}" },
                             "disabled": false, "autoApprove": ["search"] },
                  "remote": { "url": "https://example.com/mcp", "headers": { "Authorization": "Bearer ${API_TOKEN}" } }
            } }"#,
        );
        let Transport::Stdio { env, .. } = &got[0].0.transport else { panic!("brave is stdio") };
        assert_eq!(env["BRAVE_API_KEY"], EnvValue::var("BRAVE_API_KEY"));
        assert!(got[0].2);
        assert_eq!(got[0].1.keys().collect::<Vec<_>>(), ["autoApprove"]);
        assert_eq!(got[1].0.transport, http("https://example.com/mcp", &[("Authorization", bearer("API_TOKEN"))], false));
    }

    #[test]
    fn desktop_reads_stdio() {
        let got = read(
            Flavor::ClaudeDesktop,
            r#"{ "mcpServers": { "filesystem": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "/Users/someone/Desktop"],
                                                  "env": { "TOKEN": "${NOT_A_REF}" } } },
                 "preferences": { "sidebarMode": "chat" } }"#,
        );
        let expected = Transport::Stdio {
            command: "npx".into(),
            args: vec!["-y".into(), "@modelcontextprotocol/server-filesystem".into(), "/Users/someone/Desktop".into()],
            env: BTreeMap::from([("TOKEN".to_string(), EnvValue::literal("${NOT_A_REF}"))]),
            cwd: None,
        };
        assert_eq!(got, vec![(Server { name: "filesystem".into(), transport: expected }, Default::default(), true)]);
    }
}
