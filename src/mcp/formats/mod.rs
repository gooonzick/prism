//! Each agent's config dialect, read into the shared model.
//!
//! Reading only parses; nothing here rewrites a file. JSON and JSONC go through `jsonc-parser`
//! and TOML through `toml_edit`, the libraries the writers use to keep files byte for byte.

mod claude;
mod codex;
mod mcp_servers;
mod opencode;
mod refs;

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

use crate::mcp::agents::Flavor;
use crate::mcp::model::{EnvValue, Extras, Place, Server};
use refs::Syntax;

/// One server entry of a config file: the server, the fields Prism doesn't model, and whether
/// the entry itself is switched on.
pub type Entry = (Server, Extras, bool);

/// Servers in `text`, a config file of `flavor`. `place` picks the section where one file holds
/// several (Claude Code's `~/.claude.json`). A file that doesn't parse is an error, never an empty
/// list, so nothing later mistakes it for "no servers" and writes over it.
pub fn read(flavor: Flavor, text: &str, place: &Place) -> Result<Vec<Entry>, String> {
    match flavor {
        Flavor::Codex => codex::read(text),
        Flavor::OpenCode => opencode::read(&parse_json(text)?),
        Flavor::ClaudeCode => claude::read(&parse_json(text)?, place),
        _ => mcp_servers::read(flavor, parse_json(text)?.get("mcpServers")),
    }
}

/// Servers Claude Code skips in `project`, read from the text of `~/.claude.json`.
pub fn disabled_in(text: &str, project: &Path) -> Vec<String> {
    claude::disabled_in(text, project)
}

/// A JSON or JSONC document. An empty file reads as `{}`.
fn parse_json(text: &str) -> Result<Value, String> {
    if text.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    let value: Value = jsonc_parser::parse_to_serde_value(text, &Default::default()).map_err(|e| e.to_string())?;
    if value.is_object() { Ok(value) } else { Err("the top level is not an object".into()) }
}

/// A JSON object: one server entry.
type Obj = Map<String, Value>;

/// The entries of a servers object, sorted by name (serde_json doesn't keep file order).
fn objects(servers: Option<&Value>) -> Result<Vec<(&String, &Obj)>, String> {
    let Some(servers) = servers else { return Ok(vec![]) };
    let servers = servers.as_object().ok_or("the servers section is not an object")?;
    servers.iter().map(|(name, v)| Ok((name, v.as_object().ok_or_else(|| format!("{name}: not an object"))?))).collect()
}

fn string(v: &Value, what: &str) -> Result<String, String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(_) | Value::Bool(_) => Ok(v.to_string()),
        _ => Err(format!("{what}: expected a string")),
    }
}

fn strings(v: &Value, what: &str) -> Result<Vec<String>, String> {
    v.as_array().ok_or_else(|| format!("{what}: expected a list"))?.iter().map(|x| string(x, what)).collect()
}

/// An env or headers object, each value read in the agent's reference syntax.
fn values(v: Option<Value>, syntax: Syntax, what: &str) -> Result<BTreeMap<String, EnvValue>, String> {
    let Some(v) = v else { return Ok(BTreeMap::new()) };
    let Value::Object(map) = v else { return Err(format!("{what}: expected an object")) };
    map.iter().map(|(k, v)| Ok((k.clone(), refs::decode(&string(v, what)?, syntax)))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreadable_config_is_reported_not_rewritten() {
        for flavor in [Flavor::ClaudeCode, Flavor::Cursor, Flavor::OpenCode, Flavor::Codex] {
            assert!(read(flavor, "{ \"mcpServers\": { oops", &Place::Global).is_err(), "{flavor:?}");
        }
        assert!(read(Flavor::Cursor, "[1, 2]", &Place::Global).is_err());
        assert!(read(Flavor::Cursor, "{\"mcpServers\": {\"x\": 5}}", &Place::Global).is_err());
        assert!(read(Flavor::Codex, "[mcp_servers.x]\ncommand = ", &Place::Global).is_err());
        assert_eq!(read(Flavor::Kiro, "", &Place::Global), Ok(vec![]));
    }

    #[test]
    fn unknown_fields_land_in_extras() {
        let codex = "[mcp_servers.x]\ncommand = \"x\"\nstartup_timeout_sec = 20\nenabled_tools = [\"a\"]\n";
        let (_, extras, _) = &read(Flavor::Codex, codex, &Place::Global).unwrap()[0];
        assert_eq!(Value::Object(extras.clone()), serde_json::json!({ "startup_timeout_sec": 20, "enabled_tools": ["a"] }));

        let claude = r#"{ "mcpServers": { "x": { "type": "http", "url": "https://x.example.com", "oauth": { "clientId": "abc" } },
                                           "w": { "type": "ws", "url": "wss://w.example.com" } } }"#;
        let got = read(Flavor::ClaudeCode, claude, &Place::Global).unwrap();
        let w = &got[0];
        assert_eq!(Value::Object(w.1.clone()), serde_json::json!({ "type": "ws" }), "a type Prism can't model stays as written");
        let x = &got[1];
        assert_eq!(Value::Object(x.1.clone()), serde_json::json!({ "oauth": { "clientId": "abc" } }), "type is modelled, not extra");

        let cursor = r#"{ "mcpServers": { "x": { "type": "stdio", "command": "x", "envFile": ".env" } } }"#;
        let (_, extras, _) = &read(Flavor::Cursor, cursor, &Place::Global).unwrap()[0];
        assert_eq!(Value::Object(extras.clone()), serde_json::json!({ "type": "stdio", "envFile": ".env" }));
    }
}
