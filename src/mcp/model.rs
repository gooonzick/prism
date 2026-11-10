//! One shape for a server across agents, so copies in different configs can be compared.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A piece of an env or header value: literal text or a reference to an environment variable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Part {
    Text(String),
    Var { name: String, default: Option<String> },
}

/// An env or header value in agent-neutral form: `Bearer ${TOKEN}` is `[Text("Bearer "), Var(TOKEN)]`,
/// whatever syntax the agent wrote it in.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvValue(pub Vec<Part>);

impl EnvValue {
    pub fn literal(text: &str) -> EnvValue {
        EnvValue(vec![Part::Text(text.into())])
    }

    pub fn var(name: &str) -> EnvValue {
        EnvValue(vec![Part::Var { name: name.into(), default: None }])
    }

    /// A value as typed: exactly `${VAR}` or `${VAR:-default}` is a reference; anything else is
    /// literal text.
    pub fn parse(v: &str) -> EnvValue {
        let reference = v.strip_prefix("${").and_then(|r| r.strip_suffix('}')).filter(|r| !r.contains(['$', '{', '}']));
        let Some(inner) = reference else { return EnvValue::literal(v) };
        let (name, default) = match inner.split_once(":-") {
            Some((n, d)) => (n, Some(d.to_string())),
            None => (inner, None),
        };
        let valid = !name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if valid { EnvValue(vec![Part::Var { name: name.into(), default }]) } else { EnvValue::literal(v) }
    }

    /// No variable references: the value itself is in the file, so it may be a secret.
    pub fn is_literal(&self) -> bool {
        self.0.iter().all(|p| matches!(p, Part::Text(_)))
    }

    /// As written, secrets and all, with references in one syntax (`${NAME}`, `${NAME:-default}`).
    pub fn written(&self) -> String {
        self.0
            .iter()
            .map(|p| match p {
                Part::Text(t) => t.clone(),
                Part::Var { name, default: None } => format!("${{{name}}}"),
                Part::Var { name, default: Some(d) } => format!("${{{name}:-{d}}}"),
            })
            .collect()
    }

    /// The value with its secret text replaced, part by part, as [`masked`](Self::masked) prints
    /// it: references stay references (their defaults `***`), so the result's `masked()` and
    /// `written()` both give this value's `masked()`.
    pub fn masked_value(&self) -> EnvValue {
        if self.is_literal() {
            return EnvValue::literal("***");
        }
        let parts = self.0.iter().enumerate().map(|(i, p)| match p {
            Part::Text(t) if i == 0 && t.starts_with("Bearer ") => Part::Text(if t.len() == "Bearer ".len() { t.clone() } else { "Bearer ***".into() }),
            Part::Text(_) => Part::Text("***".into()),
            Part::Var { name, default } => Part::Var { name: name.clone(), default: default.as_ref().map(|_| "***".into()) },
        });
        EnvValue(parts.collect())
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transport {
    Stdio { command: String, args: Vec<String>, env: BTreeMap<String, EnvValue>, cwd: Option<String> },
    /// `sse` where the agent tells SSE apart from streamable HTTP.
    Http { url: String, headers: BTreeMap<String, EnvValue>, sse: bool },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    pub name: String,
    pub transport: Transport,
}

/// Where a copy lives, as Prism shows it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct McpCopy {
    /// Index into `mcp::agents::MCP_AGENTS`.
    pub agent: usize,
    pub file: PathBuf,
    pub place: Place,
    pub server: Server,
    /// Serialized as key names only: extras may hold secrets, and their values aren't shown.
    #[serde(with = "extra_keys")]
    pub extras: Extras,
    pub enabled: bool,
    /// `Some(app name)` when an app manages this copy; Prism doesn't edit it.
    pub read_only: Option<String>,
}

/// [`Extras`] as their key names: a list of strings out, keys with `null` values back in.
mod extra_keys {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::Extras;

    pub fn serialize<S: Serializer>(extras: &Extras, s: S) -> Result<S::Ok, S::Error> {
        extras.keys().collect::<Vec<_>>().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Extras, D::Error> {
        Ok(Vec::<String>::deserialize(d)?.into_iter().map(|k| (k, serde_json::Value::Null)).collect())
    }
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

/// A flag or query key that usually carries a secret.
fn secret_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["key", "token", "secret", "password", "auth", "bearer"].iter().any(|w| name.contains(w))
}

/// 24+ characters of `[A-Za-z0-9_-]` with a digit or a capital: an API token. Long package
/// names (`mcp-server-sequential-thinking`) are lower-case words, so they stay readable.
fn opaque_token(arg: &str) -> bool {
    arg.len() >= 24
        && !arg.starts_with('-')
        && arg.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && arg.bytes().any(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
}

/// `KEY=VALUE` with a secret-named, variable-like key (`GITHUB_TOKEN=…`, as docker `-e` takes it).
fn secret_assignment(word: &str) -> Option<&str> {
    let (key, _) = word.split_once('=')?;
    let variable = !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
    (variable && secret_name(key)).then_some(key)
}

/// Free text for printing: whatever follows `Bearer ` or `Basic ` is a credential; each word
/// that is an opaque token or a secret assignment is masked, and URLs lose their credentials.
fn masked_text(text: &str) -> String {
    let scheme = ["Bearer ", "Basic "].iter().filter_map(|s| text.find(s).map(|at| (at, s.len()))).min();
    if let Some((at, len)) = scheme {
        return format!("{}{}***", masked_text(&text[..at]), &text[at..at + len]);
    }
    text.split_inclusive(char::is_whitespace)
        .map(|piece| {
            let word = piece.trim_end();
            let gap = &piece[word.len()..];
            let shown = match secret_assignment(word) {
                Some(key) => format!("{key}=***"),
                None if opaque_token(word) => "***".into(),
                None => masked_url(word),
            };
            shown + gap
        })
        .collect()
}

/// Args for printing, with likely secrets as `***`: the value of a secret-named flag
/// (`--api-key=***`, or the arg after `--token`), secret assignments (`GITHUB_TOKEN=***`),
/// `Bearer`/`Basic` credentials, opaque tokens, and credentials in URLs.
/// Package names, paths and plain URLs stay readable.
pub fn masked_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut hide_next = false;
    for arg in args {
        if std::mem::take(&mut hide_next) {
            out.push("***".into());
            continue;
        }
        let shown = match arg.starts_with('-').then(|| arg.split_once('=')) {
            Some(Some((flag, value))) if secret_name(flag) || opaque_token(value) => format!("{flag}=***"),
            Some(Some((flag, value))) => format!("{flag}={}", masked_text(value)),
            Some(None) => {
                hide_next = secret_name(arg);
                arg.clone()
            }
            None => masked_text(arg),
        };
        out.push(shown);
    }
    out
}

/// A URL (or any text) for printing: userinfo (`https://***@host`), opaque tokens in the path
/// (`/mcp/***/sse`, as Zapier and Composio put them) and query values that are secret-named or
/// opaque tokens (`?api_key=***`, `?customerId=***`) masked, the rest as written.
pub fn masked_url(text: &str) -> String {
    let (head, fragment) = text.split_once('#').map_or((text, None), |(h, f)| (h, Some(f)));
    let (base, query) = head.split_once('?').map_or((head, None), |(b, q)| (b, Some(q)));
    let mut out = match base.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
            let authority = match authority.rfind('@') {
                Some(at) => format!("***{}", &authority[at..]),
                None => authority.to_string(),
            };
            let path: Vec<&str> = path.split('/').map(|segment| if opaque_token(segment) { "***" } else { segment }).collect();
            format!("{scheme}://{authority}{}", path.join("/"))
        }
        None => base.to_string(),
    };
    if let Some(query) = query {
        let pairs: Vec<String> = query
            .split('&')
            .map(|pair| match pair.split_once('=') {
                Some((key, value)) if secret_name(key) || opaque_token(value) => format!("{key}=***"),
                _ => pair.to_string(),
            })
            .collect();
        out.push('?');
        out.push_str(&pairs.join("&"));
    }
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
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

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn masked_args_hide_values_of_secret_flags() {
        let masked = masked_args(&strings(&["-y", "figma-developer-mcp", "--figma-api-key=fake", "-Password=fake", "--port=3000", "--stdio"]));
        assert_eq!(masked, ["-y", "figma-developer-mcp", "--figma-api-key=***", "-Password=***", "--port=3000", "--stdio"]);
    }

    #[test]
    fn masked_args_hide_the_arg_after_a_secret_flag() {
        let masked = masked_args(&strings(&["--token", "fake", "--port", "3000", "--AUTH_HEADER", "Bearer fake", "--bearer"]));
        assert_eq!(masked, ["--token", "***", "--port", "3000", "--AUTH_HEADER", "***", "--bearer"]);
    }

    #[test]
    fn masked_args_hide_opaque_tokens_and_keep_names_paths_and_urls() {
        let keep = ["@modelcontextprotocol/server-filesystem", "/Users/someone/Library/Application Support", "https://mcp.example.com/mcp", "mcp-server-sequential-thinking", "short_tok3n"];
        assert_eq!(masked_args(&strings(&keep)), keep);
        assert_eq!(masked_args(&strings(&["ghp_fake0123456789abcdefABCDEF", "0123456789abcdef0123456789"])), ["***", "***"]);
        assert_eq!(masked_args(&strings(&["--url=https://u:fake@h.example.com/x", "https://h.example.com/?token=fake"])), ["--url=https://***@h.example.com/x", "https://h.example.com/?token=***"]);
    }

    #[test]
    fn masked_args_hide_secret_env_assignments() {
        let masked = masked_args(&strings(&["run", "-e", "GITHUB_TOKEN=fake", "PORT=3000", "--env=API_KEY=fake", "ghcr.io/github/github-mcp-server"]));
        assert_eq!(masked, ["run", "-e", "GITHUB_TOKEN=***", "PORT=3000", "--env=API_KEY=***", "ghcr.io/github/github-mcp-server"]);
    }

    #[test]
    fn masked_args_hide_bearer_and_basic_credentials() {
        let masked = masked_args(&strings(&["--header", "Authorization: Bearer fake", "--header=Authorization: Basic ZmFrZQ==", "X-Mode: basic"]));
        assert_eq!(masked, ["--header", "Authorization: Bearer ***", "--header=Authorization: Basic ***", "X-Mode: basic"]);
    }

    #[test]
    fn masked_args_check_each_word_for_opaque_tokens() {
        let masked = masked_args(&strings(&["X-Api: ghp_fake0123456789abcdefABCDEF", "--header=X-Api: ghp_fake0123456789abcdefABCDEF", "run mcp-server-sequential-thinking now"]));
        assert_eq!(masked, ["X-Api: ***", "--header=X-Api: ***", "run mcp-server-sequential-thinking now"]);
    }

    #[test]
    fn masked_url_hides_userinfo_and_secret_query_values() {
        assert_eq!(masked_url("https://user:fake@host.example.com/x"), "https://***@host.example.com/x");
        assert_eq!(masked_url("https://h.example.com/x?api_key=fake&page=2&Auth=fake#frag"), "https://h.example.com/x?api_key=***&page=2&Auth=***#frag");
        assert_eq!(masked_url("https://h.example.com/a@b?q=1"), "https://h.example.com/a@b?q=1", "an @ in the path is no userinfo");
        for plain in ["https://mcp.context7.com/mcp", "npx", "/work/app", "http://127.0.0.1:3845/sse"] {
            assert_eq!(masked_url(plain), plain);
        }
    }

    #[test]
    fn masked_url_hides_tokens_in_the_path_and_in_any_query_value() {
        assert_eq!(masked_url("https://actions.zapier.com/mcp/sk-ak-AbC123xyz456DEF789ghi/sse"), "https://actions.zapier.com/mcp/***/sse");
        assert_eq!(
            masked_url("https://mcp.composio.dev/composio/server/3f2a9c1e-4b7d-4e8a-9c2f-1a2b3c4d5e6f/mcp?customerId=a1b2c3d4-e5f6-7890-abcd-ef1234567890&agent=cursor"),
            "https://mcp.composio.dev/composio/server/***/mcp?customerId=***&agent=cursor"
        );
        for plain in ["https://mcp.example.com/mcp-server-sequential-thinking/sse", "https://h.example.com/x?page=2&mode=read-only"] {
            assert_eq!(masked_url(plain), plain);
        }
    }

    #[test]
    fn masked_value_prints_and_masks_as_the_original_does() {
        let text = |t: &str| Part::Text(t.into());
        let var = |n: &str, d: Option<&str>| Part::Var { name: n.into(), default: d.map(String::from) };
        let values = [
            EnvValue::literal("fake-secret"),
            EnvValue::literal("Bearer fake-secret"),
            EnvValue(vec![var("T", None)]),
            EnvValue(vec![var("T", Some("fake-secret"))]),
            EnvValue(vec![text("Bearer "), var("T", None)]),
            EnvValue(vec![text("Bearer fake-secret-"), var("T", None)]),
            EnvValue(vec![text("https://u:fake-secret@"), var("HOST", None), text("/x")]),
        ];
        for v in values {
            let m = v.masked_value();
            assert_eq!(m.masked(), v.masked(), "{v:?}");
            assert_eq!(m.written(), v.masked(), "{v:?}");
            assert!(!m.written().contains("fake-secret"), "{v:?}");
        }
        assert_eq!(EnvValue(vec![text("Bearer "), var("T", None)]).masked_value(), EnvValue(vec![text("Bearer "), var("T", None)]));
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
