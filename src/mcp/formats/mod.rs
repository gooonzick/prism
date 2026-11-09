//! Each agent's config dialect, read into the shared model.
//!
//! Reading only parses. Writing edits the text in place: JSON and JSONC through `jsonc-parser`'s
//! CST, TOML through `toml_edit`, so everything but the touched entry stays byte for byte.

mod claude;
mod codex;
mod json;
mod mcp_servers;
mod opencode;
mod refs;
#[cfg(test)]
mod write_tests;

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

use crate::mcp::agents::{Flavor, MCP_AGENTS};
use crate::mcp::model::{EnvValue, Extras, Place, Server, Transport};
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

pub use claude::Disabled;

/// Servers Claude Code skips in `project`, read from the text of `~/.claude.json`.
pub fn disabled_in(text: &str, project: &Path) -> Disabled {
    claude::disabled_in(text, project)
}

/// `text` with `server` written into it: the entry edited in place when it exists (only the
/// modelled fields that differ; `extras` ignored), else appended with `extras` at the end of the
/// servers object. Empty `text` makes a new file. `Err` when the agent can't express a value, and
/// when the result wouldn't read back as `server`, so a wrong write never reaches the disk.
#[allow(dead_code)] // the add, copy and edit commands use it
pub fn upsert(flavor: Flavor, text: &str, place: &Place, server: &Server, extras: &Extras) -> Result<String, String> {
    let before = read(flavor, text, place)?;
    let out = match flavor {
        Flavor::Codex => codex::upsert(text, server, extras)?,
        Flavor::OpenCode => json::edit(text, |root| opencode::upsert(root, server, extras))?,
        Flavor::ClaudeCode => json::edit(text, |root| {
            json::upsert_at(root, &claude::servers_path(root, place), &server.name, |now| mcp_servers::wants(flavor, server, now), extras)
        })?,
        _ => json::edit(text, |root| json::upsert_at(root, &mcp_servers::path(), &server.name, |now| mcp_servers::wants(flavor, server, now), extras))?,
    };
    let after = read(flavor, &out, place)?;
    if !after.iter().any(|(s, _, _)| same_server(flavor, s, server)) {
        return Err(format!("{} would read {} back differently", agent(flavor), server.name));
    }
    untouched(flavor, &before, &after, &server.name, &server.name)?;
    Ok(out)
}

/// A last check before anything reaches the disk: every entry but `from` (now `to`) reads back
/// exactly as before, extras and on/off included.
fn untouched(flavor: Flavor, before: &[Entry], after: &[Entry], from: &str, to: &str) -> Result<(), String> {
    let others = |entries: &[Entry], skip: &str| {
        let mut rest: Vec<Entry> = entries.iter().filter(|(s, _, _)| s.name != skip).cloned().collect();
        rest.sort_by(|a, b| a.0.name.cmp(&b.0.name));
        rest
    };
    if others(before, from) == others(after, to) { Ok(()) } else { Err(format!("{}: the edit would change other servers too", agent(flavor))) }
}

/// OpenCode doesn't tell SSE from streamable HTTP (it tries both), so `sse` doesn't survive there.
fn same_server(flavor: Flavor, got: &Server, wanted: &Server) -> bool {
    let mut wanted = wanted.clone();
    if let (Flavor::OpenCode, Transport::Http { sse, .. }) = (flavor, &mut wanted.transport) {
        *sse = false;
    }
    *got == wanted
}

/// `text` without the server `name`; unchanged when it isn't there.
#[allow(dead_code)] // the remove command uses it
pub fn remove(flavor: Flavor, text: &str, place: &Place, name: &str) -> Result<String, String> {
    let before = read(flavor, text, place)?;
    if !before.iter().any(|(s, _, _)| s.name == name) {
        return Ok(text.into());
    }
    let out = match flavor {
        Flavor::Codex => codex::remove(text, name)?,
        Flavor::OpenCode => json::edit(text, |root| json::remove_at(root, &opencode::servers_path(root), name))?,
        Flavor::ClaudeCode => json::edit(text, |root| json::remove_at(root, &claude::servers_path(root, place), name))?,
        _ => json::edit(text, |root| json::remove_at(root, &mcp_servers::path(), name))?,
    };
    let after = read(flavor, &out, place)?;
    if after.iter().any(|(s, _, _)| s.name == name) {
        return Err(format!("{}: {name} would still be there", agent(flavor)));
    }
    untouched(flavor, &before, &after, name, name)?;
    Ok(out)
}

/// `text` with the server `from` renamed to `to`, in the same position. Never over another server.
#[allow(dead_code)] // the merge command uses it
pub fn rename(flavor: Flavor, text: &str, place: &Place, from: &str, to: &str) -> Result<String, String> {
    let before = read(flavor, text, place)?;
    let Some((old, extras, enabled)) = before.iter().find(|(s, _, _)| s.name == from) else { return Err(format!("no server {from}")) };
    if from == to {
        return Ok(text.into());
    }
    let out = match flavor {
        Flavor::Codex => codex::rename(text, from, to)?,
        Flavor::OpenCode => json::edit(text, |root| json::rename_at(root, &opencode::servers_path(root), from, to))?,
        Flavor::ClaudeCode => json::edit(text, |root| json::rename_at(root, &claude::servers_path(root, place), from, to))?,
        _ => json::edit(text, |root| json::rename_at(root, &mcp_servers::path(), from, to))?,
    };
    let after = read(flavor, &out, place)?;
    let moved = (Server { name: to.into(), transport: old.transport.clone() }, extras.clone(), *enabled);
    if !after.contains(&moved) {
        return Err(format!("{} would read {to} back differently", agent(flavor)));
    }
    untouched(flavor, &before, &after, from, to)?;
    Ok(out)
}

/// `text` with the server `name` switched off (or back on) in `project`. `place` is where the copy
/// lives. Claude Code keeps the switch in `~/.claude.json` whatever the copy's file, so `text` is
/// that file: `disabledMcpjsonServers` for a `.mcp.json` copy (`Place::Project`), else
/// `disabledMcpServers`. OpenCode and Codex switch the entry itself, only in the project's own
/// file (`text` is that file). Other agents have no per-project switch.
#[allow(dead_code)] // the enable and disable commands use it
pub fn set_disabled(flavor: Flavor, text: &str, project: &Path, place: &Place, name: &str, disabled: bool) -> Result<String, String> {
    let project_file = matches!(place, Place::Project(_));
    match flavor {
        Flavor::ClaudeCode => {
            read(flavor, text, &Place::Global)?;
            json::edit(text, |root| claude::set_disabled(root, project, project_file, name, disabled))
        }
        Flavor::OpenCode | Flavor::Codex if !project_file => Err(format!("{} can only switch off a server in the project's own config", agent(flavor))),
        Flavor::OpenCode => {
            read(flavor, text, place)?;
            json::edit(text, |root| opencode::set_enabled(root, name, !disabled))
        }
        Flavor::Codex => {
            read(flavor, text, place)?;
            codex::set_enabled(text, name, !disabled)
        }
        _ => Err(format!("{} can't switch a server off for one project", agent(flavor))),
    }
}

/// The agent's name for messages.
fn agent(flavor: Flavor) -> &'static str {
    MCP_AGENTS.iter().find(|a| a.flavor == flavor).map_or("this agent", |a| a.name)
}

/// What a writer wants a key of an entry to hold. Keys it doesn't list stay as they are.
#[derive(Clone, Debug)]
enum Want {
    /// Not the writer's to change: a key that's an extra here, or one that already reads right.
    Keep,
    Absent,
    Str(String),
    /// A list of strings; an empty list means no key.
    List(Vec<String>),
    /// An object of strings as `(key, text)`; empty means no key. Text already there that reads
    /// (in `Syntax`) as the same value counts as the same, so `$T` stays `$T` in Gemini.
    Map(Vec<(String, String)>, Syntax),
}

/// An entry's keys in the order a new entry writes them.
type Wants = Vec<(&'static str, Want)>;

impl Want {
    /// Whether `now` already reads as what's wanted, so nothing needs writing.
    fn holds(&self, now: Option<&Value>) -> bool {
        match (self, now) {
            (Want::Keep, _) | (Want::Absent, None) => true,
            (Want::Absent, Some(_)) => false,
            (Want::Str(s), Some(v)) => string(v, "").is_ok_and(|v| v == *s),
            (Want::List(items), None) => items.is_empty(),
            (Want::List(items), Some(v)) => strings(v, "").is_ok_and(|v| v == *items),
            (Want::Map(pairs, _), None) => pairs.is_empty(),
            (Want::Map(pairs, syntax), Some(Value::Object(map))) => {
                map.len() == pairs.len() && pairs.iter().all(|(k, text)| map.get(k).is_some_and(|v| same_text(v, text, *syntax)))
            }
            (Want::Str(_) | Want::Map(..), _) => false,
        }
    }
}

/// Whether `now` reads as the same value as `text`, both in `syntax`.
fn same_text(now: &Value, text: &str, syntax: Syntax) -> bool {
    string(now, "").is_ok_and(|now| refs::decode(&now, syntax) == refs::decode(text, syntax))
}

/// Env or header values in the agent's syntax, as `(key, text)`.
fn encoded(flavor: Flavor, what: &str, values: &BTreeMap<String, EnvValue>, syntax: Syntax) -> Result<Vec<(String, String)>, String> {
    values.iter().map(|(k, v)| refs::encode(v, syntax).map(|text| (k.clone(), text)).ok_or_else(|| format!("{} can't express {what} {k}", agent(flavor)))).collect()
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
