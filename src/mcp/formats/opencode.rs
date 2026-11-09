//! OpenCode: servers under `mcp`, `{type: local, command: [..], environment}` or
//! `{type: remote, url, headers}`, references as `{env:VAR}`.

use serde_json::Value;

use super::refs::Syntax;
use super::{Entry, Obj, objects, string, strings, values};
use crate::mcp::model::{Server, Transport};

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
