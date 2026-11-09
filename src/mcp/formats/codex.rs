//! Codex: `[mcp_servers.<name>]` tables in `config.toml`. Codex has no reference syntax inside
//! values; it names variables in dedicated keys instead (`env_vars`, `bearer_token_env_var`,
//! `env_http_headers`), which read here as references.

use std::collections::BTreeSet;

use serde_json::Value;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Key, Table, TableLike};

use super::refs::{self, Syntax};
use super::{Entry, Obj, Want, Wants, string, strings, values};
use crate::mcp::model::{EnvValue, Extras, Part, Server, Transport};

fn parse(text: &str) -> Result<DocumentMut, String> {
    text.parse().map_err(|e: toml_edit::TomlError| e.to_string())
}

pub(super) fn read(text: &str) -> Result<Vec<Entry>, String> {
    let doc = parse(text)?;
    let Some(servers) = doc.get("mcp_servers") else { return Ok(vec![]) };
    let servers = servers.as_table_like().ok_or("mcp_servers: expected a table")?;
    servers.iter().map(|(name, item)| entry(name, json(item))).collect()
}

fn entry(name: &str, table: Value) -> Result<Entry, String> {
    let Value::Object(mut rest) = table else { return Err(format!("{name}: expected a table")) };
    let what = |key: &str| format!("{name}.{key}");
    let mut enabled = true;
    if let Some(Value::Bool(on)) = rest.get("enabled") {
        enabled = *on;
        rest.remove("enabled");
    }
    let transport = if rest.contains_key("command") {
        let command = string(&rest.remove("command").unwrap_or_default(), &what("command"))?;
        let args = rest.remove("args").map_or(Ok(vec![]), |v| strings(&v, &what("args")))?;
        let mut env = values(rest.remove("env"), Syntax::None, &what("env"))?;
        // Names only; the `{ name, source }` form stays as written, since the model has no `source`.
        if rest.get("env_vars").and_then(Value::as_array).is_some_and(|a| a.iter().all(Value::is_string)) {
            for var in strings(&rest.remove("env_vars").unwrap_or_default(), &what("env_vars"))? {
                env.entry(var.clone()).or_insert_with(|| EnvValue::var(&var));
            }
        }
        let cwd = rest.remove("cwd").map(|v| string(&v, &what("cwd"))).transpose()?;
        Transport::Stdio { command, args, env, cwd }
    } else {
        let url = string(&rest.remove("url").ok_or_else(|| format!("{name}: neither a command nor a url"))?, &what("url"))?;
        let mut headers = values(rest.remove("http_headers"), Syntax::None, &what("http_headers"))?;
        if let Some(map) = rest.remove("env_http_headers") {
            let Value::Object(map) = map else { return Err(format!("{}: expected a table", what("env_http_headers"))) };
            for (header, var) in map {
                headers.insert(header, EnvValue::var(&string(&var, &what("env_http_headers"))?));
            }
        }
        if let Some(var) = rest.remove("bearer_token_env_var") {
            let var = string(&var, &what("bearer_token_env_var"))?;
            headers.insert("Authorization".into(), EnvValue(vec![Part::Text("Bearer ".into()), Part::Var { name: var, default: None }]));
        }
        Transport::Http { url, headers, sse: false }
    };
    Ok((Server { name: name.into(), transport }, rest, enabled))
}

/// The `mcp_servers` table, created (implicit, so no bare header) when the file has none.
fn servers_mut(doc: &mut DocumentMut) -> Result<&mut dyn TableLike, String> {
    let item = doc.entry("mcp_servers").or_insert_with(|| {
        let mut table = Table::new();
        table.set_implicit(true);
        Item::Table(table)
    });
    item.as_table_like_mut().ok_or_else(|| "mcp_servers: expected a table".into())
}

pub(super) fn upsert(text: &str, server: &Server, extras: &Extras) -> Result<String, String> {
    let mut doc = parse(text)?;
    let servers = servers_mut(&mut doc)?;
    match servers.get_mut(&server.name) {
        Some(item) => {
            let Value::Object(now) = json(item) else { return Err(format!("{}: expected a table", server.name)) };
            let wants = wants(server, Some(&now))?;
            let entry = item.as_table_like_mut().ok_or_else(|| format!("{}: expected a table", server.name))?;
            sync(entry, &now, &wants);
        }
        None => {
            let table = fresh(&wants(server, None)?, extras);
            servers.insert(&server.name, Item::Table(table));
        }
    }
    Ok(doc.to_string())
}

/// Removes the server's table with its sub-tables and the comment written above it.
pub(super) fn remove(text: &str, name: &str) -> Result<String, String> {
    let mut doc = parse(text)?;
    if let Some(servers) = doc.get_mut("mcp_servers").and_then(Item::as_table_like_mut) {
        servers.remove(name);
    }
    Ok(doc.to_string())
}

/// Renames by taking the entries from `from` on out and putting them back in order, the renamed
/// one under the new key with the old key's formatting. Tables keep their place in the file.
pub(super) fn rename(text: &str, from: &str, to: &str) -> Result<String, String> {
    let mut doc = parse(text)?;
    let servers = doc.get_mut("mcp_servers").ok_or_else(|| format!("no server {from}"))?;
    let keys: Vec<String> = servers.as_table_like().map(|t| t.iter().map(|(k, _)| k.to_string()).collect()).unwrap_or_default();
    if keys.iter().any(|k| k == to) {
        return Err(format!("{to} already exists"));
    }
    let at = keys.iter().position(|k| k == from).ok_or_else(|| format!("no server {from}"))?;
    let renamed = |key: &Key| Key::new(to).with_leaf_decor(key.leaf_decor().clone()).with_dotted_decor(key.dotted_decor().clone());
    if let Some(table) = servers.as_table_mut() {
        let tail: Vec<(Key, Item)> = keys[at..].iter().filter_map(|k| table.remove_entry(k)).collect();
        for (i, (key, item)) in tail.into_iter().enumerate() {
            table.insert_formatted(&if i == 0 { renamed(&key) } else { key }, item);
        }
    } else if let Some(table) = servers.as_inline_table_mut() {
        let tail: Vec<(Key, toml_edit::Value)> = keys[at..].iter().filter_map(|k| table.remove_entry(k)).collect();
        for (i, (key, value)) in tail.into_iter().enumerate() {
            table.insert_formatted(&if i == 0 { renamed(&key) } else { key }, value);
        }
    } else {
        return Err("mcp_servers: expected a table".into());
    }
    Ok(doc.to_string())
}

/// `enabled` on the server's table. A server that is on and never had the key gets none.
pub(super) fn set_enabled(text: &str, name: &str, on: bool) -> Result<String, String> {
    let mut doc = parse(text)?;
    let entry = doc.get_mut("mcp_servers").and_then(Item::as_table_like_mut).and_then(|s| s.get_mut(name)).and_then(Item::as_table_like_mut);
    let entry = entry.ok_or_else(|| format!("no server {name}"))?;
    let now = entry.get("enabled").and_then(Item::as_bool);
    if now.is_some_and(|b| b != on) || (now.is_none() && !on) {
        set(entry, "enabled", on.into());
    }
    Ok(doc.to_string())
}

/// Codex has no references inside values. An env reference is `env_vars` (only under its own
/// name); a header reference is `bearer_token_env_var` (`Authorization: Bearer ${T}`) or
/// `env_http_headers` (the whole value). Anything else, and SSE, Codex can't express.
fn wants(server: &Server, now: Option<&Obj>) -> Result<Wants, String> {
    let was_stdio = now.and_then(|obj| entry(&server.name, Value::Object(obj.clone())).ok()).map(|(s, _, _)| matches!(s.transport, Transport::Stdio { .. }));
    let stdio = matches!(server.transport, Transport::Stdio { .. });
    let other = || if was_stdio.is_some_and(|w| w != stdio) { Want::Absent } else { Want::Keep };
    let literal = |v: &EnvValue| refs::encode(v, Syntax::None);
    let whole_var = |v: &EnvValue| match v.0.as_slice() {
        [Part::Var { name, default: None }] => Some(name.clone()),
        _ => None,
    };
    Ok(match &server.transport {
        Transport::Stdio { command, args, env, cwd } => {
            let (mut fixed, mut names) = (vec![], vec![]);
            for (k, v) in env {
                match (literal(v), whole_var(v)) {
                    (Some(text), _) => fixed.push((k.clone(), text)),
                    (None, Some(name)) if name == *k => names.push(name),
                    _ => return Err(format!("Codex can't express env {k}")),
                }
            }
            let env_vars = match now.and_then(|obj| obj.get("env_vars")) {
                Some(Value::Array(listed)) if listed.iter().all(Value::is_string) => {
                    let listed: BTreeSet<&str> = listed.iter().filter_map(Value::as_str).collect();
                    if listed == names.iter().map(String::as_str).collect() { Want::Keep } else { Want::List(names) }
                }
                // The `{ name, source }` form: an extra, kept as written; it can't take more names.
                Some(_) if names.is_empty() => Want::Keep,
                Some(_) => return Err(format!("Codex can't express env {}", names[0])),
                None => Want::List(names),
            };
            vec![
                ("command", Want::Str(command.clone())),
                ("args", Want::List(args.clone())),
                ("env", Want::Map(fixed, Syntax::None)),
                ("env_vars", env_vars),
                ("cwd", cwd.clone().map_or(Want::Absent, Want::Str)),
                ("url", other()),
                ("bearer_token_env_var", other()),
                ("http_headers", other()),
                ("env_http_headers", other()),
            ]
        }
        Transport::Http { sse: true, .. } => return Err("Codex can't express sse".into()),
        Transport::Http { url, headers, .. } => {
            let (mut bearer, mut fixed, mut vars) = (Want::Absent, vec![], vec![]);
            for (k, v) in headers {
                match (v.0.as_slice(), literal(v), whole_var(v)) {
                    ([Part::Text(scheme), Part::Var { name, default: None }], _, _) if k == "Authorization" && scheme == "Bearer " => bearer = Want::Str(name.clone()),
                    (_, Some(text), _) => fixed.push((k.clone(), text)),
                    (_, None, Some(name)) => vars.push((k.clone(), name)),
                    _ => return Err(format!("Codex can't express header {k}")),
                }
            }
            vec![
                ("url", Want::Str(url.clone())),
                ("bearer_token_env_var", bearer),
                ("http_headers", Want::Map(fixed, Syntax::None)),
                ("env_http_headers", Want::Map(vars, Syntax::None)),
                ("command", other()),
                ("args", other()),
                ("env", other()),
                ("env_vars", other()),
                ("cwd", other()),
            ]
        }
    })
}

/// Brings an existing table to `wants`, touching only what reads differently.
fn sync(entry: &mut dyn TableLike, now: &Obj, wants: &Wants) {
    for (key, want) in wants {
        if want.holds(now.get(*key)) {
            continue;
        }
        match want {
            Want::Keep => {}
            Want::Absent => {
                entry.remove(key);
            }
            Want::List(items) if items.is_empty() => {
                entry.remove(key);
            }
            Want::Map(pairs, _) if pairs.is_empty() => {
                entry.remove(key);
            }
            Want::Str(s) => set(entry, key, s.as_str().into()),
            Want::List(items) => match entry.get_mut(key).and_then(Item::as_array_mut) {
                Some(array) => sync_list(array, items),
                None => set(entry, key, list(items)),
            },
            Want::Map(pairs, _) => match entry.get_mut(key).and_then(Item::as_table_like_mut) {
                Some(table) => sync_map(table, pairs),
                None => set(entry, key, inline(pairs)),
            },
        }
    }
}

/// Puts `value` at `key`, keeping the spacing and comment around the old value.
fn set(entry: &mut dyn TableLike, key: &str, mut value: toml_edit::Value) {
    match entry.get_mut(key).and_then(Item::as_value_mut) {
        Some(old) => {
            *value.decor_mut() = old.decor().clone();
            *old = value;
        }
        None => {
            entry.insert(key, Item::Value(value));
        }
    }
}

fn same(v: &toml_edit::Value, text: &str) -> bool {
    string(&value(v), "").is_ok_and(|v| v == text)
}

fn sync_list(array: &mut Array, items: &[String]) {
    for (i, item) in items.iter().enumerate() {
        match array.get(i) {
            Some(v) if same(v, item) => {}
            Some(_) => {
                array.replace(i, item.as_str());
            }
            None => array.push(item.as_str()),
        }
    }
    while array.len() > items.len() {
        array.remove(array.len() - 1);
    }
}

/// An `env`-like table, inline (`env = { .. }`) or its own (`[mcp_servers.x.env]`).
fn sync_map(table: &mut dyn TableLike, pairs: &[(String, String)]) {
    let stale: Vec<String> = table.iter().map(|(k, _)| k.to_string()).filter(|k| !pairs.iter().any(|(p, _)| p == k)).collect();
    for k in stale {
        table.remove(&k);
    }
    for (k, text) in pairs {
        if !table.get(k).and_then(Item::as_value).is_some_and(|v| same(v, text)) {
            set(table, k, text.as_str().into());
        }
    }
}

/// A new server table: the wanted keys in order, then the extras it doesn't already have.
fn fresh(wants: &Wants, extras: &Extras) -> Table {
    let mut table = Table::new();
    for (key, want) in wants {
        let value = match want {
            Want::Str(s) => s.as_str().into(),
            Want::List(items) if !items.is_empty() => list(items),
            Want::Map(pairs, _) if !pairs.is_empty() => inline(pairs),
            _ => continue,
        };
        table.insert(key, Item::Value(value));
    }
    for (k, v) in extras {
        if let (false, Some(v)) = (table.contains_key(k), toml(v)) {
            table.insert(k, Item::Value(v));
        }
    }
    table
}

fn list(items: &[String]) -> toml_edit::Value {
    toml_edit::Value::Array(items.iter().map(String::as_str).collect())
}

fn inline(pairs: &[(String, String)]) -> toml_edit::Value {
    toml_edit::Value::InlineTable(pairs.iter().map(|(k, v)| (k.as_str(), toml_edit::Value::from(v.as_str()))).collect::<InlineTable>())
}

/// A JSON extra back as TOML. TOML has no null, so a null is left out.
fn toml(v: &Value) -> Option<toml_edit::Value> {
    Some(match v {
        Value::Null => return None,
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.into(),
            None => n.as_f64()?.into(),
        },
        Value::String(s) => s.as_str().into(),
        Value::Array(a) => toml_edit::Value::Array(a.iter().filter_map(toml).collect()),
        Value::Object(o) => toml_edit::Value::InlineTable(o.iter().filter_map(|(k, v)| Some((k.as_str(), toml(v)?))).collect::<InlineTable>()),
    })
}

/// A TOML item as JSON, so Codex extras share the `Extras` type. Datetimes become strings.
fn json(item: &Item) -> Value {
    match item {
        Item::None => Value::Null,
        Item::Value(v) => value(v),
        Item::Table(t) => Value::Object(t.iter().map(|(k, i)| (k.to_string(), json(i))).collect()),
        Item::ArrayOfTables(a) => Value::Array(a.iter().map(|t| Value::Object(t.iter().map(|(k, i)| (k.to_string(), json(i))).collect())).collect()),
    }
}

fn value(v: &toml_edit::Value) -> Value {
    use toml_edit::Value as T;
    match v {
        T::String(s) => Value::String(s.value().clone()),
        T::Integer(i) => (*i.value()).into(),
        T::Float(f) => serde_json::Number::from_f64(*f.value()).map_or(Value::Null, Value::Number),
        T::Boolean(b) => (*b.value()).into(),
        T::Datetime(d) => Value::String(d.value().to_string()),
        T::Array(a) => Value::Array(a.iter().map(value).collect()),
        T::InlineTable(t) => Value::Object(t.iter().map(|(k, v)| (k.to_string(), value(v))).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::read as read_file;
    use crate::mcp::agents::Flavor;
    use crate::mcp::model::{EnvValue, Part, Place, Transport};

    const CONFIG: &str = r#"
model = "gpt-5"

# GitLab, token from the shell
[mcp_servers.gitlab]
command = "npx"
args = ["-y", "@zereight/mcp-gitlab"]
env_vars = ["GITLAB_TOKEN"]
startup_timeout_sec = 20

[mcp_servers.gitlab.env]
GITLAB_API_URL = "https://gitlab.example.com/api/v4"
LITERAL = "${NOT_A_REF}"

[mcp_servers.context7]
url = "https://mcp.context7.com/mcp"
bearer_token_env_var = "CTX7_TOKEN"
http_headers = { "X-Static" = "fake-static" }
env_http_headers = { "X-Team" = "TEAM_ID" }
enabled = false
tools = { search = { approval_mode = "approve" } }
"#;

    #[test]
    fn codex_reads_env_vars_and_bearer_as_references() {
        let got = read_file(Flavor::Codex, CONFIG, &Place::Global).unwrap();
        let names: Vec<_> = got.iter().map(|(s, _, _)| s.name.as_str()).collect();
        assert_eq!(names, ["gitlab", "context7"]);

        let Transport::Stdio { command, args, env, cwd } = &got[0].0.transport else { panic!("gitlab is stdio") };
        assert_eq!((command.as_str(), args.len(), cwd), ("npx", 2, &None));
        assert_eq!(env["GITLAB_TOKEN"], EnvValue::var("GITLAB_TOKEN"));
        assert_eq!(env["GITLAB_API_URL"], EnvValue::literal("https://gitlab.example.com/api/v4"));
        assert_eq!(env["LITERAL"], EnvValue::literal("${NOT_A_REF}"));
        assert!(got[0].2);

        let Transport::Http { url, headers, sse } = &got[1].0.transport else { panic!("context7 is http") };
        assert_eq!((url.as_str(), *sse), ("https://mcp.context7.com/mcp", false));
        assert_eq!(headers["Authorization"].0, vec![Part::Text("Bearer ".into()), Part::Var { name: "CTX7_TOKEN".into(), default: None }]);
        assert_eq!(headers["X-Team"], EnvValue::var("TEAM_ID"));
        assert_eq!(headers["X-Static"], EnvValue::literal("fake-static"));
        assert!(!got[1].2);
        assert_eq!(got[1].1["tools"]["search"]["approval_mode"], "approve");
    }
}
