//! Codex: `[mcp_servers.<name>]` tables in `config.toml`. Codex has no reference syntax inside
//! values; it names variables in dedicated keys instead (`env_vars`, `bearer_token_env_var`,
//! `env_http_headers`), which read here as references.

use serde_json::Value;
use toml_edit::{DocumentMut, Item};

use super::refs::Syntax;
use super::{Entry, string, strings, values};
use crate::mcp::model::{EnvValue, Part, Server, Transport};

pub(super) fn read(text: &str) -> Result<Vec<Entry>, String> {
    let doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    let Some(servers) = doc.get("mcp_servers") else { return Ok(vec![]) };
    let servers = servers.as_table_like().ok_or("mcp_servers: expected a table")?;
    servers.iter().map(|(name, item)| entry(name, item)).collect()
}

fn entry(name: &str, item: &Item) -> Result<Entry, String> {
    let Value::Object(mut rest) = json(item) else { return Err(format!("{name}: expected a table")) };
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
