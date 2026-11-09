//! OpenCode: servers under `mcp`, `{type: local, command: [..], environment}` or
//! `{type: remote, url, headers}`, references as `{env:VAR}`.

use jsonc_parser::cst::CstObject;
use serde_json::Value;

use super::refs::Syntax;
use super::{Entry, Obj, Want, Wants, encoded, json, objects, string, strings, values};
use crate::mcp::agents::Flavor;
use crate::mcp::model::{Extras, Server, Transport};

/// Servers sit right under `mcp` (the shape configs have today) or under `mcp.servers`, the newer
/// layout where `mcp` also holds settings such as `timeout` and entries switch off with `disabled`.
pub(super) fn read(doc: &Value) -> Result<Vec<Entry>, String> {
    let Some(mcp) = doc.get("mcp") else { return Ok(vec![]) };
    let nested = mcp.get("servers").filter(|s| s.is_object() && s.get("type").is_none());
    let servers = match nested {
        Some(servers) => objects(Some(servers))?,
        // Only objects are servers; anything else under `mcp` is a setting.
        None => mcp.as_object().ok_or("mcp: expected an object")?.iter().filter_map(|(k, v)| Some((k, v.as_object()?))).collect(),
    };
    servers.into_iter().map(|(name, obj)| entry(name, obj)).collect()
}

fn entry(name: &str, obj: &Obj) -> Result<Entry, String> {
    let mut rest = obj.clone();
    let what = |key: &str| format!("{name}.{key}");
    let mut enabled = true;
    for (key, on_when) in [("enabled", true), ("disabled", false)] {
        if let Some(Value::Bool(b)) = rest.get(key) {
            enabled = *b == on_when;
            rest.remove(key);
        }
    }
    let kind = rest.remove("type").map(|v| string(&v, &what("type"))).transpose()?;
    let local = match kind.as_deref() {
        Some("local") => true,
        Some("remote") => false,
        Some(_) => return Err(format!("{name}: unknown type")),
        None => rest.contains_key("command"),
    };
    let transport = if local {
        let mut command = rest.remove("command").map_or(Ok(vec![]), |v| strings(&v, &what("command")))?.into_iter();
        let program = command.next().ok_or_else(|| format!("{name}: empty command"))?;
        let env = values(rest.remove("environment"), Syntax::Brace, &what("environment"))?;
        let cwd = rest.remove("cwd").map(|v| string(&v, &what("cwd"))).transpose()?;
        Transport::Stdio { command: program, args: command.collect(), env, cwd }
    } else {
        let url = string(&rest.remove("url").ok_or_else(|| format!("{name}: no url"))?, &what("url"))?;
        let headers = values(rest.remove("headers"), Syntax::Brace, &what("headers"))?;
        Transport::Http { url, headers, sse: false }
    };
    Ok((Server { name: name.into(), transport }, rest, enabled))
}

/// Where the servers sit, as the reader finds them: `mcp.servers` when the file uses that layout,
/// else right under `mcp` (also for a file with no servers yet).
pub(super) fn servers_path(root: &CstObject) -> Vec<String> {
    let mcp = root.get("mcp").and_then(|m| m.object_value());
    let servers = mcp.and_then(|m| m.get("servers")).and_then(|s| s.object_value());
    if servers.is_some_and(|s| s.get("type").is_none()) { vec!["mcp".into(), "servers".into()] } else { vec!["mcp".into()] }
}

pub(super) fn upsert(root: &CstObject, server: &Server, extras: &Extras) -> Result<(), String> {
    json::upsert_at(root, &servers_path(root), &server.name, |now| wants(server, now), extras)
}

/// `enabled` (or `disabled`, in the `mcp.servers` layout) on the entry. Leaves no key behind for
/// a server that is on and never had one.
pub(super) fn set_enabled(root: &CstObject, name: &str, on: bool) -> Result<(), String> {
    let path = servers_path(root);
    let entry = json::object_at(root, &path)?.and_then(|s| s.get(name)).and_then(|p| p.object_value()).ok_or_else(|| format!("no server {name}"))?;
    let mut found = false;
    for (key, value) in [("enabled", on), ("disabled", !on)] {
        if let Some(prop) = entry.get(key) {
            found = true;
            json::set_bool(&prop, value);
        }
    }
    if !found && !on {
        let (key, value) = if path.len() == 2 { ("disabled", true) } else { ("enabled", false) };
        entry.append(key, value.into());
    }
    Ok(())
}

/// OpenCode has one remote type for SSE and streamable HTTP, so `sse` isn't written.
fn wants(server: &Server, now: Option<&Obj>) -> Result<Wants, String> {
    let was_stdio = now.and_then(|obj| entry(&server.name, obj).ok()).map(|(s, _, _)| matches!(s.transport, Transport::Stdio { .. }));
    let stdio = matches!(server.transport, Transport::Stdio { .. });
    let other = || if was_stdio.is_some_and(|w| w != stdio) { Want::Absent } else { Want::Keep };
    let kind = |k: &str| if was_stdio == Some(stdio) { Want::Keep } else { Want::Str(k.into()) };
    Ok(match &server.transport {
        Transport::Stdio { command, args, env, cwd } => vec![
            ("type", kind("local")),
            ("command", Want::List([command.clone()].into_iter().chain(args.iter().cloned()).collect())),
            ("environment", Want::Map(encoded(Flavor::OpenCode, "env", env, Syntax::Brace)?, Syntax::Brace)),
            ("cwd", cwd.clone().map_or(Want::Absent, Want::Str)),
            ("url", other()),
            ("headers", other()),
        ],
        Transport::Http { url, headers, .. } => vec![
            ("type", kind("remote")),
            ("url", Want::Str(url.clone())),
            ("headers", Want::Map(encoded(Flavor::OpenCode, "header", headers, Syntax::Brace)?, Syntax::Brace)),
            ("command", other()),
            ("environment", other()),
            ("cwd", other()),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::super::read as read_file;
    use crate::mcp::agents::Flavor;
    use crate::mcp::model::{EnvValue, Part, Place, Transport};

    #[test]
    fn opencode_reads_local_command_array_and_env_refs() {
        let text = r#"{
          // comments are fine: this is JSONC
          "$schema": "https://opencode.ai/config.json",
          "mcp": {
            "gitlab": {
              "type": "local",
              "command": ["npx", "-y", "@zereight/mcp-gitlab"],
              "environment": { "GITLAB_TOKEN": "{env:TOKEN}", "GITLAB_API_URL": "https://gitlab.example.com/api/v4" },
              "enabled": false,
            },
            "context7": { "type": "remote", "url": "https://mcp.context7.com/mcp", "headers": { "Authorization": "Bearer {env:CTX7}" }, "timeout": 5000 },
          },
        }"#;
        let got = read_file(Flavor::OpenCode, text, &Place::Global).unwrap();
        let (ctx7, extras, enabled) = &got[0];
        let Transport::Http { url, headers, sse } = &ctx7.transport else { panic!("context7 is remote") };
        assert_eq!((url.as_str(), *sse, *enabled), ("https://mcp.context7.com/mcp", false, true));
        assert_eq!(headers["Authorization"].0, vec![Part::Text("Bearer ".into()), Part::Var { name: "CTX7".into(), default: None }]);
        assert_eq!(extras["timeout"], 5000);
        let (gitlab, extras, enabled) = &got[1];
        let Transport::Stdio { command, args, env, .. } = &gitlab.transport else { panic!("gitlab is local") };
        assert_eq!((command.as_str(), args.as_slice()), ("npx", ["-y".to_string(), "@zereight/mcp-gitlab".to_string()].as_slice()));
        assert_eq!(env["GITLAB_TOKEN"], EnvValue::var("TOKEN"));
        assert!(env["GITLAB_API_URL"].is_literal());
        assert!(!enabled && extras.is_empty());
    }

    #[test]
    fn opencode_reads_servers_under_mcp_servers_with_disabled() {
        let text = r#"{ "mcp": { "timeout": 10000, "servers": {
            "pw": { "type": "local", "command": ["npx", "@playwright/mcp"], "cwd": "/work", "disabled": true }
        } } }"#;
        let got = read_file(Flavor::OpenCode, text, &Place::Global).unwrap();
        assert_eq!(got.len(), 1);
        let Transport::Stdio { cwd, .. } = &got[0].0.transport else { panic!("pw is local") };
        assert_eq!(cwd.as_deref(), Some("/work"));
        assert!(!got[0].2 && got[0].1.is_empty());
    }
}
