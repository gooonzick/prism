//! A scope's MCP servers: every detected agent's copies grouped by name, with the drift between
//! copies and the servers that look like one server under two names.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agents::{Paths, Scope};
use crate::mcp::agents::{Flavor, MCP_AGENTS, McpAgent};
use crate::mcp::formats::{self, Disabled};
use crate::mcp::model::{self, EnvValue, McpCopy, Place, Transport, masked_args, masked_url};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    InSync,
    Differs,
    /// An app manages at least one copy, so Prism leaves the server alone.
    ReadOnly,
}

/// One field where copies disagree, value by value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDiff {
    /// `transport kind`, `command`, `args`, `cwd`, `url`, `env.KEY` or `headers.KEY`.
    pub field: String,
    /// One entry per copy, in copy order: the agent and the value, masked; `None` where the copy lacks it.
    pub values: Vec<(usize, Option<String>)>,
    /// The same values as written, unmasked, in the same order. Shown only on request.
    pub written: Vec<Option<String>>,
}

impl FieldDiff {
    /// Every copy that has the field shows the same masked value: only hidden text differs.
    pub fn hidden_only(&self) -> bool {
        self.values.iter().all(|(_, v)| v.is_some()) && self.values.windows(2).all(|w| w[0].1 == w[1].1)
    }

    /// Agent and value per copy: masked, or as written when `secrets`.
    pub fn shown(&self, secrets: bool) -> impl Iterator<Item = (usize, Option<&str>)> {
        self.values.iter().zip(&self.written).map(move |((a, v), w)| (*a, if secrets { w.as_deref() } else { v.as_deref() }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    /// Sorted by agent, then place.
    pub copies: Vec<McpCopy>,
    /// In a project, every copy is global: no agent defines this name in the project itself.
    pub inherited: bool,
    pub status: Status,
    pub diffs: Vec<FieldDiff>,
    /// The agents that skip this server in the project scope.
    pub disabled_for: Vec<usize>,
}

/// Differently named servers that look like one: same URL or same package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeHint {
    pub names: Vec<String>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct McpInventory {
    pub scope: Scope,
    /// Sorted by name.
    pub servers: Vec<McpServer>,
    pub merges: Vec<MergeHint>,
    /// Config files that exist but couldn't be read; they add no copies.
    pub errors: Vec<(PathBuf, String)>,
}

impl McpInventory {
    pub fn get(&self, name: &str) -> Option<&McpServer> {
        self.servers.iter().find(|s| s.name == name)
    }
}

/// `inv` with nothing secret left in it, for sending elsewhere: env and header values become
/// their masked text (as literals), args, commands, cwds and URLs are masked, diffs carry their
/// masked values as the written ones, extras keep their keys only, and read errors their first
/// line.
pub fn masked(inv: &McpInventory) -> McpInventory {
    let values = |m: &BTreeMap<String, EnvValue>| m.iter().map(|(k, v)| (k.clone(), EnvValue::literal(&v.masked()))).collect();
    let transport = |t: &Transport| match t {
        Transport::Stdio { command, args, env, cwd } => {
            Transport::Stdio { command: masked_url(command), args: masked_args(args), env: values(env), cwd: cwd.as_deref().map(masked_url) }
        }
        Transport::Http { url, headers, sse } => Transport::Http { url: masked_url(url), headers: values(headers), sse: *sse },
    };
    let copy = |c: &McpCopy| McpCopy {
        server: model::Server { name: c.server.name.clone(), transport: transport(&c.server.transport) },
        extras: c.extras.keys().map(|k| (k.clone(), serde_json::Value::Null)).collect(),
        ..c.clone()
    };
    let diff = |d: &FieldDiff| FieldDiff { written: d.values.iter().map(|(_, v)| v.clone()).collect(), ..d.clone() };
    let servers = inv
        .servers
        .iter()
        .map(|s| McpServer { copies: s.copies.iter().map(copy).collect(), diffs: s.diffs.iter().map(diff).collect(), ..s.clone() })
        .collect();
    let errors = inv.errors.iter().map(|(f, e)| (f.clone(), safe_error(e).to_string())).collect();
    McpInventory { servers, errors, ..inv.clone() }
}

/// A read error's first line, safe to show. TOML errors go on to quote the offending line of the
/// file, which may be a secret; the first line is the position.
pub fn safe_error(message: &str) -> &str {
    message.lines().next().unwrap_or_default()
}

/// Every detected agent's servers in `scope`. In a project, an agent's global copy joins the
/// row unless that agent has its own copy of the name in the project (as each agent resolves
/// it); a row of global copies only is inherited. Missing config files are skipped; unreadable
/// ones go to `errors` and add nothing.
pub fn scan(scope: &Scope, paths: &Paths) -> McpInventory {
    let mut files = Files::default();
    let mut all = copies(scope, paths, &mut files, |_| true);
    let project = match scope {
        Scope::Project(root) => Some(root),
        Scope::Global => None,
    };
    if project.is_some() {
        let shadowed: BTreeSet<(usize, String)> = all.iter().map(|c| (c.agent, c.server.name.clone())).collect();
        // Claude Desktop has no projects, so its servers don't apply in one.
        let global = copies(&Scope::Global, paths, &mut files, |a| a.flavor != Flavor::ClaudeDesktop);
        all.extend(global.into_iter().filter(|c| !shadowed.contains(&(c.agent, c.server.name.clone()))));
    }
    let mut servers: Vec<McpServer> = by_name(all).into_iter().map(|(name, copies)| server(name, copies, project.is_some())).collect();
    if let Some(root) = project {
        let claude = claude_disabled(root, paths, &mut files);
        for s in &mut servers {
            s.disabled_for = disabled_for(s, &claude);
        }
    }
    servers.sort_by(|a, b| a.name.cmp(&b.name));
    let merges = merges(&servers, project.is_some());
    McpInventory { scope: scope.clone(), servers, merges, errors: files.errors }
}

/// Config texts read once per scan: `~/.claude.json` holds both the user and the local scope.
#[derive(Default)]
struct Files {
    /// `None`: missing or unreadable.
    texts: HashMap<PathBuf, Option<String>>,
    errors: Vec<(PathBuf, String)>,
}

impl Files {
    fn text(&mut self, path: &Path) -> Option<&str> {
        if !self.texts.contains_key(path) {
            let text = match std::fs::read_to_string(path) {
                Ok(text) => Some(text),
                Err(e) if e.kind() == ErrorKind::NotFound => None,
                Err(e) => {
                    self.error(path, e.to_string());
                    None
                }
            };
            self.texts.insert(path.into(), text);
        }
        self.texts[path].as_deref()
    }

    /// One error per file, though a file may be read for two places.
    fn error(&mut self, path: &Path, message: String) {
        if !self.errors.iter().any(|(p, _)| p == path) {
            self.errors.push((path.into(), message));
        }
    }
}

fn copies(scope: &Scope, paths: &Paths, files: &mut Files, wanted: impl Fn(&McpAgent) -> bool) -> Vec<McpCopy> {
    let mut out = Vec::new();
    for (agent, a) in MCP_AGENTS.iter().enumerate().filter(|(_, a)| wanted(a) && a.detected(paths)) {
        for (file, place) in a.files(scope, paths) {
            let Some(text) = files.text(&file) else { continue };
            match formats::read(a.flavor, text, &place) {
                Ok(entries) => out.extend(entries.into_iter().map(|(server, extras, enabled)| McpCopy {
                    agent,
                    file: file.clone(),
                    place: place.clone(),
                    read_only: model::read_only(&server),
                    server,
                    extras,
                    enabled,
                })),
                Err(e) => files.error(&file, e),
            }
        }
    }
    out
}

fn by_name(copies: Vec<McpCopy>) -> BTreeMap<String, Vec<McpCopy>> {
    let mut groups: BTreeMap<String, Vec<McpCopy>> = BTreeMap::new();
    for copy in copies {
        groups.entry(copy.server.name.clone()).or_default().push(copy);
    }
    groups
}

fn place_rank(place: &Place) -> u8 {
    match place {
        Place::Global => 0,
        Place::ClaudeLocal(_) => 1,
        Place::Project(_) => 2,
    }
}

/// `project`: the row is in a project scope, where all-global copies make it inherited.
fn server(name: String, mut copies: Vec<McpCopy>, project: bool) -> McpServer {
    copies.sort_by_key(|c| (c.agent, place_rank(&c.place)));
    let inherited = project && copies.iter().all(|c| c.place == Place::Global);
    let status = if copies.iter().any(|c| c.read_only.is_some()) {
        Status::ReadOnly
    } else if copies.windows(2).any(|w| !alike(&w[0].server.transport, &w[1].server.transport)) {
        Status::Differs
    } else {
        Status::InSync
    };
    let diffs = diffs(&copies);
    McpServer { name, copies, inherited, status, diffs, disabled_for: Vec::new() }
}

/// Equal apart from the HTTP flavour and header-name case. Agents infer SSE differently (a
/// Cursor `url` and a Claude `type: sse` are the same server) and HTTP header names are
/// case-insensitive; the model keeps both as written, comparisons ignore them.
pub(crate) fn alike(a: &Transport, b: &Transport) -> bool {
    match (a, b) {
        (Transport::Http { url: u1, headers: h1, .. }, Transport::Http { url: u2, headers: h2, .. }) => u1 == u2 && folded(h1) == folded(h2),
        _ => a == b,
    }
}

fn folded(headers: &BTreeMap<String, EnvValue>) -> BTreeMap<String, &EnvValue> {
    headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), v)).collect()
}

/// A field's value in one copy: compared as decoded, shown masked.
#[derive(PartialEq)]
enum Field<'a> {
    Text(String),
    Args(&'a [String]),
    Value(&'a EnvValue),
}

impl Field<'_> {
    fn shown(&self) -> String {
        match self {
            Field::Text(t) => masked_url(t),
            Field::Args(args) => masked_args(args).join(" "),
            Field::Value(v) => v.masked(),
        }
    }

    fn written(&self) -> String {
        match self {
            Field::Text(t) => t.clone(),
            Field::Args(args) => args.join(" "),
            Field::Value(v) => v.written(),
        }
    }
}

/// Field order in a diff; `env.*` and `headers.*` follow, by key.
const FIELDS: [&str; 5] = ["transport kind", "command", "args", "cwd", "url"];

/// A copy's fields by comparison key, each with the name to show. Header keys are compared in
/// lower case, since header names are case-insensitive.
fn fields(t: &Transport) -> BTreeMap<String, (String, Field<'_>)> {
    fn named<'a>(key: impl Into<String>, name: impl Into<String>, field: Field<'a>) -> (String, (String, Field<'a>)) {
        (key.into(), (name.into(), field))
    }
    let mut f = BTreeMap::new();
    match t {
        Transport::Stdio { command, args, env, cwd } => {
            f.insert("transport kind".into(), ("transport kind".into(), Field::Text("stdio".into())));
            f.insert("command".into(), ("command".into(), Field::Text(command.clone())));
            if !args.is_empty() {
                f.insert("args".into(), ("args".into(), Field::Args(args)));
            }
            if let Some(cwd) = cwd {
                f.insert("cwd".into(), ("cwd".into(), Field::Text(cwd.clone())));
            }
            f.extend(env.iter().map(|(k, v)| named(format!("env.{k}"), format!("env.{k}"), Field::Value(v))));
        }
        Transport::Http { url, headers, .. } => {
            f.insert("transport kind".into(), ("transport kind".into(), Field::Text("http".into())));
            f.insert("url".into(), ("url".into(), Field::Text(url.clone())));
            f.extend(headers.iter().map(|(k, v)| named(format!("headers.{}", k.to_ascii_lowercase()), format!("headers.{k}"), Field::Value(v))));
        }
    }
    f
}

fn diffs(copies: &[McpCopy]) -> Vec<FieldDiff> {
    let per_copy: Vec<_> = copies.iter().map(|c| fields(&c.server.transport)).collect();
    let keyed: BTreeSet<&String> = per_copy.iter().flat_map(|f| f.keys()).filter(|k| !FIELDS.contains(&k.as_str())).collect();
    let order = FIELDS.iter().copied().chain(keyed.into_iter().map(String::as_str));
    let mut out = Vec::new();
    for key in order {
        let values: Vec<Option<&(String, Field)>> = per_copy.iter().map(|f| f.get(key)).collect();
        if values.windows(2).all(|w| w[0].map(|(_, f)| f) == w[1].map(|(_, f)| f)) {
            continue;
        }
        // Shown as the first copy that has it spells it.
        let Some((name, _)) = values.iter().flatten().next() else { continue };
        let field = name.clone();
        let written = values.iter().map(|v| v.map(|(_, f)| f.written())).collect();
        let values = copies.iter().zip(values).map(|(c, v)| (c.agent, v.map(|(_, f)| f.shown()))).collect();
        out.push(FieldDiff { field, values, written });
    }
    out
}

/// Claude Code's disabled lists for the project, when Claude Code is installed.
fn claude_disabled(root: &Path, paths: &Paths, files: &mut Files) -> Disabled {
    let Some(claude) = MCP_AGENTS.iter().find(|a| a.flavor == Flavor::ClaudeCode && a.detected(paths)) else { return Disabled::default() };
    let scope = Scope::Project(root.into());
    let Some((file, _)) = claude.files(&scope, paths).into_iter().find(|(_, place)| matches!(place, Place::ClaudeLocal(_))) else {
        return Disabled::default();
    };
    files.text(&file).map(|text| formats::disabled_in(text, root)).unwrap_or_default()
}

/// The agents that skip `server` in the project. Claude Code: `disabledMcpServers` covers user
/// and local copies, `disabledMcpjsonServers` the `.mcp.json` ones. Others: a switched-off copy
/// in the project's own file (a global copy switched off is off everywhere, not just here).
fn disabled_for(server: &McpServer, claude: &Disabled) -> Vec<usize> {
    let listed = |list: &[String]| list.contains(&server.name);
    let mut agents: Vec<usize> = server
        .copies
        .iter()
        .filter(|c| match (MCP_AGENTS[c.agent].flavor, &c.place) {
            (Flavor::ClaudeCode, Place::Project(_)) => listed(&claude.mcp_json),
            (Flavor::ClaudeCode, _) => listed(&claude.servers),
            (_, Place::Project(_)) => !c.enabled,
            _ => false,
        })
        .map(|c| c.agent)
        .collect();
    agents.dedup();
    agents
}

/// What makes two differently named servers look like one: a key of raw values to group by,
/// the rest of the command line (copies of one package with other args still group, but say so),
/// and the reason to show, masked. Runners are normalised: `npx -y X`, `npx X`, `bunx -y X`,
/// `bunx X` and `pnpx X` all run `X`.
fn likeness(t: &Transport) -> (Vec<String>, Vec<String>, String) {
    let words = |words: &[String]| masked_args(words).join(" ");
    match t {
        Transport::Http { url, .. } => (vec!["url".into(), url.clone()], vec![], format!("same URL {}", masked_url(url))),
        Transport::Stdio { command, args, .. } => match package(command, args) {
            Some((package, rest)) => {
                (vec!["package".into(), package.into()], rest.to_vec(), format!("same package {}", words(&[package.to_string()])))
            }
            None => {
                let line: Vec<String> = std::iter::once(command).chain(args).cloned().collect();
                (std::iter::once("command".to_string()).chain(line.iter().cloned()).collect(), vec![], format!("same command {}", words(&line)))
            }
        },
    }
}

fn package<'a>(command: &str, args: &'a [String]) -> Option<(&'a str, &'a [String])> {
    let runner = Path::new(command).file_name()?.to_str()?;
    if !matches!(runner, "npx" | "bunx" | "pnpx") {
        return None;
    }
    let skip = args.iter().take_while(|a| matches!(a.as_str(), "-y" | "--yes" | "-q" | "--quiet" | "--no-install")).count();
    let (package, rest) = args[skip..].split_first()?;
    // Another flag may take a value (`--registry URL`, `-p X`), so the package is a guess; a path
    // is a local script, not a package.
    let path_like = package.starts_with(['/', '~']) || package.starts_with("./") || package.starts_with("../");
    (!package.starts_with('-') && !path_like).then_some((package.as_str(), rest))
}

/// Groups of differently named servers sharing a URL or a package, each group once.
/// Read-only servers are left out: Prism can't rename them. In a project only the project's own
/// copies count: renaming a global copy from there would edit the global configs.
fn merges(servers: &[McpServer], project: bool) -> Vec<MergeHint> {
    type Group<'a> = (String, BTreeSet<&'a str>, BTreeSet<Vec<String>>);
    let mut alike: BTreeMap<Vec<String>, Group> = BTreeMap::new();
    for s in servers.iter().filter(|s| s.status != Status::ReadOnly) {
        for c in s.copies.iter().filter(|c| !project || c.place != Place::Global) {
            let (key, rest, reason) = likeness(&c.server.transport);
            let group = alike.entry(key).or_insert_with(|| (reason, BTreeSet::new(), BTreeSet::new()));
            group.1.insert(&s.name);
            group.2.insert(rest);
        }
    }
    let mut seen = BTreeSet::new();
    let mut hints: Vec<MergeHint> = alike
        .into_values()
        .filter(|(_, names, _)| names.len() > 1 && seen.insert(names.clone()))
        .map(|(reason, names, variants)| MergeHint {
            names: names.into_iter().map(String::from).collect(),
            reason: if variants.len() > 1 { format!("{reason}, args differ") } else { reason },
        })
        .collect();
    hints.sort_by(|a, b| a.names.cmp(&b.names));
    hints
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::mcp::agents::index_of;

    fn agent(id: &str) -> usize {
        index_of(id).unwrap()
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A throwaway home with Claude Code and Cursor installed.
    fn home() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::isolated(dir.path());
        std::fs::create_dir_all(&paths.claude).unwrap();
        std::fs::create_dir_all(dir.path().join(".cursor")).unwrap();
        (dir, paths)
    }

    fn claude_json(paths: &Paths, doc: serde_json::Value) {
        write(&paths.home.join(".claude.json"), &doc.to_string());
    }

    fn cursor_json(paths: &Paths, doc: serde_json::Value) {
        write(&paths.home.join(".cursor/mcp.json"), &doc.to_string());
    }

    fn names(inv: &McpInventory) -> Vec<&str> {
        inv.servers.iter().map(|s| s.name.as_str()).collect()
    }

    fn agents_of(server: &McpServer) -> Vec<usize> {
        server.copies.iter().map(|c| c.agent).collect()
    }

    #[test]
    fn same_name_groups_across_agents() {
        let (_dir, paths) = home();
        let gitlab = json!({ "command": "npx", "args": ["-y", "@zereight/mcp-gitlab"] });
        claude_json(&paths, json!({ "mcpServers": { "solo": { "type": "http", "url": "https://solo.example.com/mcp" }, "gitlab": gitlab } }));
        cursor_json(&paths, json!({ "mcpServers": { "gitlab": gitlab } }));

        let inv = scan(&Scope::Global, &paths);
        assert_eq!(names(&inv), ["gitlab", "solo"]);
        let gitlab = inv.get("gitlab").unwrap();
        assert_eq!(agents_of(gitlab), [agent("claude"), agent("cursor")]);
        assert_eq!((gitlab.status, gitlab.inherited), (Status::InSync, false));
        assert!(gitlab.diffs.is_empty() && gitlab.disabled_for.is_empty());
        assert_eq!(gitlab.copies[1].file, paths.home.join(".cursor/mcp.json"));
        let solo = inv.get("solo").unwrap();
        assert_eq!((agents_of(solo), solo.status), (vec![agent("claude")], Status::InSync));
        assert!(inv.merges.is_empty() && inv.errors.is_empty());
    }

    #[test]
    fn undetected_agents_are_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::isolated(dir.path());
        // `~/.claude.json` without `~/.claude`: Claude Code isn't installed here.
        claude_json(&paths, json!({ "mcpServers": { "gitlab": { "command": "npx" } } }));
        assert!(scan(&Scope::Global, &paths).servers.is_empty());
    }

    #[test]
    fn different_env_value_differs_with_masked_diff() {
        let (_dir, paths) = home();
        claude_json(&paths, json!({ "mcpServers": { "gitlab": { "command": "npx", "env": { "TOKEN": "fake-a", "URL": "same" } } } }));
        cursor_json(&paths, json!({ "mcpServers": { "gitlab": { "command": "npx", "env": { "TOKEN": "fake-b", "URL": "same" } } } }));

        let gitlab = scan(&Scope::Global, &paths).get("gitlab").cloned().unwrap();
        assert_eq!(gitlab.status, Status::Differs);
        let masked = Some("***".to_string());
        assert_eq!(gitlab.diffs, [FieldDiff { field: "env.TOKEN".into(), values: vec![(agent("claude"), masked.clone()), (agent("cursor"), masked)], written: vec![Some("fake-a".into()), Some("fake-b".into())] }]);
        assert!(gitlab.diffs[0].hidden_only());
    }

    #[test]
    fn missing_header_shows_as_none() {
        let (_dir, paths) = home();
        let url = "https://mcp.context7.com/mcp";
        claude_json(&paths, json!({ "mcpServers": { "context7": { "type": "http", "url": url, "headers": { "CONTEXT7_API_KEY": "${CTX7}" } } } }));
        cursor_json(&paths, json!({ "mcpServers": { "context7": { "url": url } } }));

        let ctx7 = scan(&Scope::Global, &paths).get("context7").cloned().unwrap();
        assert_eq!(ctx7.status, Status::Differs);
        let values = vec![(agent("claude"), Some("${CTX7}".to_string())), (agent("cursor"), None)];
        assert_eq!(ctx7.diffs, [FieldDiff { field: "headers.CONTEXT7_API_KEY".into(), values, written: vec![Some("${CTX7}".into()), None] }]);
        assert!(!ctx7.diffs[0].hidden_only());
    }

    #[test]
    fn transport_kind_and_command_differences_are_listed_in_order() {
        let (_dir, paths) = home();
        claude_json(&paths, json!({ "mcpServers": { "x": { "type": "sse", "url": "http://127.0.0.1:3845/sse" } } }));
        cursor_json(&paths, json!({ "mcpServers": { "x": { "command": "uvx", "args": ["x-mcp"] } } }));

        let x = scan(&Scope::Global, &paths).get("x").cloned().unwrap();
        let fields: Vec<_> = x.diffs.iter().map(|d| d.field.as_str()).collect();
        assert_eq!(fields, ["transport kind", "command", "args", "url"]);
        assert_eq!(x.diffs[0].values, [(agent("claude"), Some("http".into())), (agent("cursor"), Some("stdio".into()))]);
        assert_eq!(x.diffs[2].values, [(agent("claude"), None), (agent("cursor"), Some("x-mcp".into()))]);
    }

    #[test]
    fn sse_and_http_flavours_are_the_same_server() {
        let (_dir, paths) = home();
        let url = "http://127.0.0.1:3845/sse";
        claude_json(&paths, json!({ "mcpServers": { "figma": { "type": "sse", "url": url } } }));
        cursor_json(&paths, json!({ "mcpServers": { "figma": { "url": url } } }));

        let figma = scan(&Scope::Global, &paths).get("figma").cloned().unwrap();
        assert_eq!((figma.status, figma.diffs.len()), (Status::InSync, 0));
        let Transport::Http { sse, .. } = &figma.copies[0].server.transport else { panic!("figma is http") };
        assert!(*sse, "the model keeps the flavour");
    }

    #[test]
    fn diffs_mask_secrets_in_args_and_urls() {
        let (_dir, paths) = home();
        claude_json(
            &paths,
            json!({ "mcpServers": {
                "figma": { "command": "npx", "args": ["-y", "figma-developer-mcp", "--figma-api-key=fake-a"] },
                "remote": { "type": "http", "url": "https://u:fake@mcp.example.com/mcp?api_key=fake-a&team=1" }
            } }),
        );
        cursor_json(
            &paths,
            json!({ "mcpServers": {
                "figma": { "command": "npx", "args": ["-y", "figma-developer-mcp", "--figma-api-key=fake-b"] },
                "remote": { "url": "https://mcp.example.com/mcp?api_key=fake-b&team=1" }
            } }),
        );

        let inv = scan(&Scope::Global, &paths);
        let figma = inv.get("figma").unwrap();
        let shown = Some("-y figma-developer-mcp --figma-api-key=***".to_string());
        assert_eq!(figma.diffs, [FieldDiff { field: "args".into(), values: vec![(agent("claude"), shown.clone()), (agent("cursor"), shown)], written: vec![Some("-y figma-developer-mcp --figma-api-key=fake-a".into()), Some("-y figma-developer-mcp --figma-api-key=fake-b".into())] }]);
        assert!(figma.diffs[0].hidden_only());
        let revealed: Vec<_> = figma.diffs[0].shown(true).map(|(_, v)| v.unwrap()).collect();
        assert_eq!(revealed, ["-y figma-developer-mcp --figma-api-key=fake-a", "-y figma-developer-mcp --figma-api-key=fake-b"]);
        let remote = inv.get("remote").unwrap();
        let values = vec![
            (agent("claude"), Some("https://***@mcp.example.com/mcp?api_key=***&team=1".to_string())),
            (agent("cursor"), Some("https://mcp.example.com/mcp?api_key=***&team=1".to_string())),
        ];
        assert_eq!(remote.diffs, [FieldDiff { field: "url".into(), values, written: vec![Some("https://u:fake@mcp.example.com/mcp?api_key=fake-a&team=1".into()), Some("https://mcp.example.com/mcp?api_key=fake-b&team=1".into())] }]);
        assert!(!remote.diffs[0].hidden_only());
    }

    #[test]
    fn refs_in_different_syntax_are_equal() {
        let (_dir, paths) = home();
        claude_json(
            &paths,
            json!({ "mcpServers": {
                "gitlab": { "command": "npx", "env": { "T": "${T}" } },
                "linear": { "type": "http", "url": "https://mcp.linear.app/mcp", "headers": { "Authorization": "Bearer ${LINEAR}" } }
            } }),
        );
        cursor_json(
            &paths,
            json!({ "mcpServers": {
                "gitlab": { "command": "npx", "env": { "T": "${env:T}" } },
                "linear": { "url": "https://mcp.linear.app/mcp", "headers": { "Authorization": "Bearer ${env:LINEAR}" } }
            } }),
        );

        let inv = scan(&Scope::Global, &paths);
        for name in ["gitlab", "linear"] {
            let server = inv.get(name).unwrap();
            assert_eq!((server.copies.len(), server.status), (2, Status::InSync), "{name}");
            assert!(server.diffs.is_empty(), "{name}");
        }
    }

    #[test]
    fn npx_and_bunx_same_package_suggest_merge() {
        let (_dir, paths) = home();
        let pencil = json!({ "command": "/Applications/Pencil.app/Contents/Resources/mcp-server" });
        claude_json(
            &paths,
            json!({ "mcpServers": {
                "figma": { "command": "npx", "args": ["-y", "figma-developer-mcp", "--stdio"] },
                "figma-other": { "command": "npx", "args": ["-y", "figma-developer-mcp", "--port", "1"] },
                "ctx": { "type": "http", "url": "https://mcp.context7.com/mcp" },
                "pencil": pencil
            } }),
        );
        cursor_json(
            &paths,
            json!({ "mcpServers": {
                "figma-dev": { "command": "bunx", "args": ["figma-developer-mcp", "--stdio"] },
                "context7": { "url": "https://mcp.context7.com/mcp" },
                "pencil-app": pencil
            } }),
        );
        write(&paths.config.join("opencode/opencode.json"), r#"{ "mcp": { "figma-pn": { "type": "local", "command": ["pnpx", "figma-developer-mcp", "--stdio"] } } }"#);

        let inv = scan(&Scope::Global, &paths);
        assert_eq!(inv.get("pencil").unwrap().status, Status::ReadOnly);
        let hint = |names: &[&str], reason: &str| MergeHint { names: names.iter().map(|n| n.to_string()).collect(), reason: reason.into() };
        assert_eq!(
            inv.merges,
            [
                hint(&["context7", "ctx"], "same URL https://mcp.context7.com/mcp"),
                hint(&["figma", "figma-dev", "figma-other", "figma-pn"], "same package figma-developer-mcp, args differ")
            ],
            "read-only copies don't count; other args still merge, said so"
        );
    }

    #[test]
    fn same_package_with_other_args_still_merges_without_leaking_keys() {
        let (_dir, paths) = home();
        claude_json(&paths, json!({ "mcpServers": { "a": { "command": "bunx", "args": ["-y", "figma-developer-mcp", "--figma-api-key=AAA", "--stdio"] } } }));
        cursor_json(&paths, json!({ "mcpServers": { "b": { "command": "npx", "args": ["-y", "figma-developer-mcp", "--figma-api-key=BBB", "--stdio"] } } }));
        let inv = scan(&Scope::Global, &paths);
        assert_eq!(inv.merges.len(), 1);
        let reason = &inv.merges[0].reason;
        assert_eq!(reason, "same package figma-developer-mcp, args differ");
        assert!(!reason.contains("AAA") && !reason.contains("BBB"));
    }

    #[test]
    fn package_hints_skip_runner_flags_and_give_up_on_unclear_ones() {
        let (_dir, paths) = home();
        let hints = |a: serde_json::Value, b: serde_json::Value| {
            claude_json(&paths, json!({ "mcpServers": { "a": a } }));
            cursor_json(&paths, json!({ "mcpServers": { "b": b } }));
            scan(&Scope::Global, &paths).merges.into_iter().map(|h| h.reason).collect::<Vec<_>>()
        };
        let run = |cmd: &str, args: &[&str]| json!({ "command": cmd, "args": args });
        assert!(hints(run("npx", &["--registry", "https://x", "pkgA"]), run("npx", &["--registry", "https://y", "pkgB"])).is_empty());
        assert_eq!(hints(run("pnpx", &["-y", "pkg", "--a", "1"]), run("npx", &["pkg", "--a", "2"])), ["same package pkg, args differ"]);
        assert!(hints(run("bunx", &["-y", "./local.js", "--x"]), run("bunx", &["-y", "./local.js", "--y"])).is_empty());
        assert_eq!(hints(run("bunx", &["-y", "./local.js", "--x"]), run("bunx", &["-y", "./local.js", "--x"])), ["same command bunx -y ./local.js --x"]);
    }

    #[test]
    fn opencode_lists_servers_from_both_global_files() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::isolated(dir.path());
        write(&paths.config.join("opencode/opencode.json"), r#"{"mcp":{"only-json":{"type":"remote","url":"https://a.example/mcp"}}}"#);
        write(&paths.config.join("opencode/opencode.jsonc"), r#"{"mcp":{"only-jsonc":{"type":"remote","url":"https://b.example/mcp"}}}"#);
        let inv = scan(&Scope::Global, &paths);
        assert_eq!(names(&inv), ["only-json", "only-jsonc"]);
    }

    #[test]
    fn project_merge_hints_skip_inherited_servers() {
        let (dir, paths) = home();
        let root = dir.path().join("app");
        std::fs::create_dir_all(&root).unwrap();
        let ctx7 = json!({ "type": "http", "url": "https://mcp.context7.com/mcp" });
        claude_json(
            &paths,
            json!({
                "mcpServers": { "ctx": ctx7, "ctx-old": ctx7 },
                "projects": { root.to_string_lossy(): { "mcpServers": { "context7": ctx7 } } }
            }),
        );
        write(&root.join(".cursor/mcp.json"), &json!({ "mcpServers": { "c7": { "url": "https://mcp.context7.com/mcp" } } }).to_string());

        let names = |inv: McpInventory| inv.merges.into_iter().map(|h| h.names).collect::<Vec<_>>();
        assert_eq!(names(scan(&Scope::Global, &paths)), [["ctx", "ctx-old"]]);
        let project = scan(&Scope::Project(root), &paths);
        assert!(project.get("ctx").unwrap().inherited);
        assert_eq!(names(project), [["c7", "context7"]], "renaming an inherited server would edit the global configs");
    }

    #[test]
    fn merge_reasons_mask_secrets() {
        let (_dir, paths) = home();
        let url = "https://x.example/mcp?api_key=abc123";
        let cmd = json!({ "command": "uvx", "args": ["x-mcp", "--token", "abc123"] });
        claude_json(&paths, json!({ "mcpServers": { "a": { "type": "http", "url": url }, "c": cmd } }));
        cursor_json(&paths, json!({ "mcpServers": { "b": { "url": url }, "d": cmd } }));

        let inv = scan(&Scope::Global, &paths);
        let reasons: Vec<_> = inv.merges.iter().map(|h| h.reason.as_str()).collect();
        assert_eq!(reasons, ["same URL https://x.example/mcp?api_key=***", "same command uvx x-mcp --token ***"]);
        assert!(reasons.iter().all(|r| !r.contains("abc123")));
    }

    #[test]
    fn merge_reasons_mask_tokens_in_url_paths() {
        let (_dir, paths) = home();
        let url = "https://actions.zapier.com/mcp/sk-ak-AbC123xyz456DEF789ghi/sse";
        claude_json(&paths, json!({ "mcpServers": { "zapier": { "type": "sse", "url": url } } }));
        cursor_json(&paths, json!({ "mcpServers": { "zap": { "url": url } } }));
        let inv = scan(&Scope::Global, &paths);
        let reasons: Vec<_> = inv.merges.iter().map(|h| h.reason.as_str()).collect();
        assert_eq!(reasons, ["same URL https://actions.zapier.com/mcp/***/sse"]);
    }

    #[test]
    fn grouping_uses_raw_values_not_masked_ones() {
        let (_dir, paths) = home();
        claude_json(&paths, json!({ "mcpServers": { "a": { "type": "http", "url": "https://x.example/mcp?api_key=one" } } }));
        cursor_json(&paths, json!({ "mcpServers": { "b": { "url": "https://x.example/mcp?api_key=two" } } }));
        assert!(scan(&Scope::Global, &paths).merges.is_empty());
    }

    #[test]
    fn project_copies_shadow_global_ones_per_agent() {
        let (dir, paths) = home();
        let root = dir.path().join("app");
        let gitlab = json!({ "command": "npx", "args": ["-y", "@zereight/mcp-gitlab"] });
        claude_json(&paths, json!({ "mcpServers": { "gitlab": gitlab }, "projects": { root.to_string_lossy(): { "disabledMcpServers": ["gitlab"] } } }));
        cursor_json(&paths, json!({ "mcpServers": { "gitlab": { "command": "npx", "args": ["-y", "@zereight/mcp-gitlab", "--old"] } } }));
        write(&root.join(".cursor/mcp.json"), &json!({ "mcpServers": { "gitlab": gitlab } }).to_string());

        let inv = scan(&Scope::Project(root.clone()), &paths);
        assert_eq!(names(&inv), ["gitlab"]);
        let row = inv.get("gitlab").unwrap();
        let places: Vec<_> = row.copies.iter().map(|c| (c.agent, c.place.clone())).collect();
        assert_eq!(places, [(agent("claude"), Place::Global), (agent("cursor"), Place::Project(root))], "Cursor's project copy hides its global one");
        assert!(!row.inherited);
        assert_eq!((row.status, row.disabled_for.clone()), (Status::InSync, vec![agent("claude")]));
    }

    #[test]
    fn desktop_servers_stay_out_of_projects() {
        let (dir, paths) = home();
        let desktop = &MCP_AGENTS[agent("claude-desktop")];
        let file = desktop.files(&Scope::Global, &paths).remove(0).0;
        write(&file, &json!({ "mcpServers": { "fs": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem"] } } }).to_string());
        assert_eq!(names(&scan(&Scope::Global, &paths)), ["fs"]);
        let root = dir.path().join("app");
        std::fs::create_dir_all(&root).unwrap();
        assert!(scan(&Scope::Project(root), &paths).servers.is_empty());
    }

    #[test]
    fn header_names_compare_case_insensitively() {
        let (_dir, paths) = home();
        let url = "https://mcp.example.com/mcp";
        claude_json(&paths, json!({ "mcpServers": {
            "same": { "type": "http", "url": url, "headers": { "Authorization": "Bearer ${T}" } },
            "other": { "type": "http", "url": "https://other.example.com/mcp", "headers": { "X-Key": "${A}" } }
        } }));
        cursor_json(&paths, json!({ "mcpServers": {
            "same": { "url": url, "headers": { "authorization": "Bearer ${env:T}" } },
            "other": { "url": "https://other.example.com/mcp", "headers": { "x-key": "${env:B}" } }
        } }));

        let inv = scan(&Scope::Global, &paths);
        let same = inv.get("same").unwrap();
        assert_eq!((same.status, same.diffs.len()), (Status::InSync, 0));
        let Transport::Http { headers, .. } = &same.copies[1].server.transport else { panic!("same is http") };
        assert!(headers.contains_key("authorization"), "the model keeps the case as written");
        let values = vec![(agent("claude"), Some("${A}".to_string())), (agent("cursor"), Some("${B}".to_string()))];
        assert_eq!(inv.get("other").unwrap().diffs, [FieldDiff { field: "headers.X-Key".into(), values, written: vec![Some("${A}".into()), Some("${B}".into())] }]);
    }

    #[test]
    fn project_scope_shows_global_as_inherited() {
        let (dir, paths) = home();
        let root = dir.path().join("work/app");
        std::fs::create_dir_all(&root).unwrap();
        claude_json(
            &paths,
            json!({
                "mcpServers": { "gitlab": { "command": "npx", "args": ["-y", "@zereight/mcp-gitlab"] }, "context7": { "type": "http", "url": "https://mcp.context7.com/mcp" } },
                "projects": { root.to_string_lossy(): { "mcpServers": { "context7": { "type": "http", "url": "https://mcp.context7.com/mcp", "headers": { "K": "${K}" } } } } }
            }),
        );
        write(&root.join(".cursor/mcp.json"), &json!({ "mcpServers": { "linear": { "url": "https://mcp.linear.app/mcp" } } }).to_string());

        let inv = scan(&Scope::Project(root.clone()), &paths);
        assert_eq!(inv.scope, Scope::Project(root.clone()));
        assert_eq!(names(&inv), ["context7", "gitlab", "linear"]);
        let ctx7 = inv.get("context7").unwrap();
        assert!(!ctx7.inherited, "the project's own context7 hides the global one");
        assert_eq!(ctx7.copies.iter().map(|c| &c.place).collect::<Vec<_>>(), [&Place::ClaudeLocal(root.clone())]);
        let gitlab = inv.get("gitlab").unwrap();
        assert!(gitlab.inherited);
        assert_eq!((gitlab.copies[0].place.clone(), gitlab.status), (Place::Global, Status::InSync));
        let linear = inv.get("linear").unwrap();
        assert_eq!((linear.inherited, agents_of(linear), linear.copies[0].place.clone()), (false, vec![agent("cursor")], Place::Project(root)));
    }

    #[test]
    fn claude_disabled_list_marks_disabled_for() {
        let (dir, paths) = home();
        let root = dir.path().join("app");
        std::fs::create_dir_all(dir.path().join(".kiro")).unwrap();
        claude_json(
            &paths,
            json!({
                "mcpServers": { "gitlab": { "command": "npx", "args": ["-y", "@zereight/mcp-gitlab"] } },
                "projects": { root.to_string_lossy(): { "disabledMcpServers": ["gitlab", "team"], "disabledMcpjsonServers": ["shared"] } }
            }),
        );
        let team = json!({ "command": "uvx", "args": ["team-mcp"] });
        write(&root.join(".mcp.json"), &json!({ "mcpServers": { "shared": { "command": "uvx", "args": ["shared-mcp"] }, "team": team } }).to_string());
        write(&root.join(".kiro/settings/mcp.json"), &json!({ "mcpServers": { "team": { "command": "uvx", "args": ["team-mcp"], "disabled": true } } }).to_string());

        let inv = scan(&Scope::Project(root), &paths);
        let disabled = |name: &str| inv.get(name).unwrap().disabled_for.clone();
        assert_eq!(disabled("gitlab"), [agent("claude")], "disabledMcpServers covers user-scope servers");
        assert_eq!(disabled("shared"), [agent("claude")], "disabledMcpjsonServers covers .mcp.json servers");
        assert_eq!(disabled("team"), [agent("kiro")], "disabledMcpServers doesn't reach .mcp.json; Kiro's own switch does");
        assert_eq!(agents_of(inv.get("team").unwrap()), [agent("claude"), agent("kiro")]);
        assert!(scan(&Scope::Global, &paths).get("gitlab").unwrap().disabled_for.is_empty());
    }

    #[test]
    fn unreadable_file_goes_to_errors() {
        let (_dir, paths) = home();
        std::fs::create_dir_all(&paths.codex).unwrap();
        claude_json(&paths, json!({ "mcpServers": { "gitlab": { "command": "npx" } } }));
        write(&paths.home.join(".cursor/mcp.json"), "{ \"mcpServers\": { oops");

        let inv = scan(&Scope::Global, &paths);
        assert_eq!(names(&inv), ["gitlab"]);
        assert_eq!(agents_of(inv.get("gitlab").unwrap()), [agent("claude")]);
        assert_eq!(inv.errors.len(), 1, "a missing Codex config is no error: {:?}", inv.errors);
        assert_eq!(inv.errors[0].0, paths.home.join(".cursor/mcp.json"));
        assert!(!inv.errors[0].1.is_empty());
    }
}
