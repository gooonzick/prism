//! A scope's MCP servers: every detected agent's copies grouped by name, with the drift between
//! copies and the servers that look like one server under two names.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::agents::{Paths, Scope};
use crate::mcp::agents::{Flavor, MCP_AGENTS};
use crate::mcp::formats::{self, Disabled};
use crate::mcp::model::{self, EnvValue, McpCopy, Place, Transport, masked_args, masked_url};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    InSync,
    Differs,
    /// An app manages at least one copy, so Prism leaves the server alone.
    ReadOnly,
}

/// One field where copies disagree, value by value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldDiff {
    /// `transport kind`, `command`, `args`, `cwd`, `url`, `env.KEY` or `headers.KEY`.
    pub field: String,
    /// One entry per copy, in copy order: the agent and the value, masked; `None` where the copy lacks it.
    pub values: Vec<(usize, Option<String>)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct McpServer {
    pub name: String,
    /// Sorted by agent, then place.
    pub copies: Vec<McpCopy>,
    /// A global server shown in a project that doesn't define one by this name.
    pub inherited: bool,
    pub status: Status,
    pub diffs: Vec<FieldDiff>,
    /// The agents that skip this server in the project scope.
    pub disabled_for: Vec<usize>,
}

/// Differently named servers that look like one: same URL or same package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeHint {
    pub names: Vec<String>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq)]
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

/// Every detected agent's servers in `scope`. A project also shows the global servers it
/// doesn't define itself, as inherited. Missing config files are skipped; unreadable ones go to
/// `errors` and add nothing.
pub fn scan(scope: &Scope, paths: &Paths) -> McpInventory {
    let mut files = Files::default();
    let own = by_name(copies(scope, paths, &mut files));
    let mut servers: Vec<McpServer> = Vec::new();
    if let Scope::Project(root) = scope {
        let global = by_name(copies(&Scope::Global, paths, &mut files));
        servers.extend(global.into_iter().filter(|(name, _)| !own.contains_key(name)).map(|(name, copies)| server(name, copies, true)));
        servers.extend(own.into_iter().map(|(name, copies)| server(name, copies, false)));
        let claude = claude_disabled(root, paths, &mut files);
        for s in &mut servers {
            s.disabled_for = disabled_for(s, &claude);
        }
    } else {
        servers.extend(own.into_iter().map(|(name, copies)| server(name, copies, false)));
    }
    servers.sort_by(|a, b| a.name.cmp(&b.name));
    let merges = merges(&servers);
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

fn copies(scope: &Scope, paths: &Paths, files: &mut Files) -> Vec<McpCopy> {
    let mut out = Vec::new();
    for (agent, a) in MCP_AGENTS.iter().enumerate().filter(|(_, a)| a.detected(paths)) {
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

fn server(name: String, mut copies: Vec<McpCopy>, inherited: bool) -> McpServer {
    copies.sort_by_key(|c| (c.agent, place_rank(&c.place)));
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

/// Equal apart from the HTTP flavour: agents infer SSE differently (a Cursor `url` and a Claude
/// `type: sse` are the same server), so the flavour stays in the model but not in comparisons.
fn alike(a: &Transport, b: &Transport) -> bool {
    match (a, b) {
        (Transport::Http { url: u1, headers: h1, .. }, Transport::Http { url: u2, headers: h2, .. }) => u1 == u2 && h1 == h2,
        _ => a == b,
    }
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
}

/// Field order in a diff; `env.*` and `headers.*` follow, by key.
const FIELDS: [&str; 5] = ["transport kind", "command", "args", "cwd", "url"];

fn fields(t: &Transport) -> BTreeMap<String, Field<'_>> {
    let mut f = BTreeMap::new();
    match t {
        Transport::Stdio { command, args, env, cwd } => {
            f.insert("transport kind".into(), Field::Text("stdio".into()));
            f.insert("command".into(), Field::Text(command.clone()));
            if !args.is_empty() {
                f.insert("args".into(), Field::Args(args));
            }
            if let Some(cwd) = cwd {
                f.insert("cwd".into(), Field::Text(cwd.clone()));
            }
            f.extend(env.iter().map(|(k, v)| (format!("env.{k}"), Field::Value(v))));
        }
        Transport::Http { url, headers, .. } => {
            f.insert("transport kind".into(), Field::Text("http".into()));
            f.insert("url".into(), Field::Text(url.clone()));
            f.extend(headers.iter().map(|(k, v)| (format!("headers.{k}"), Field::Value(v))));
        }
    }
    f
}

fn diffs(copies: &[McpCopy]) -> Vec<FieldDiff> {
    let per_copy: Vec<_> = copies.iter().map(|c| fields(&c.server.transport)).collect();
    let keyed: BTreeSet<&String> = per_copy.iter().flat_map(|f| f.keys()).filter(|k| !FIELDS.contains(&k.as_str())).collect();
    let order = FIELDS.iter().copied().chain(keyed.into_iter().map(String::as_str));
    let mut out = Vec::new();
    for field in order {
        let values: Vec<Option<&Field>> = per_copy.iter().map(|f| f.get(field)).collect();
        if values.windows(2).all(|w| w[0] == w[1]) {
            continue;
        }
        let values = copies.iter().zip(values).map(|(c, v)| (c.agent, v.map(Field::shown))).collect();
        out.push(FieldDiff { field: field.into(), values });
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

/// What makes two differently named servers look like one, as a key and the reason to show.
/// Runners are normalised: `npx -y X`, `npx X`, `bunx -y X`, `bunx X` and `pnpx X` all run `X`.
fn likeness(t: &Transport) -> (String, Vec<String>) {
    match t {
        Transport::Http { url, .. } => (format!("same URL {url}"), vec![]),
        Transport::Stdio { command, args, .. } => match package(command, args) {
            Some((package, rest)) => (format!("same package {package}"), rest.to_vec()),
            None => (format!("same command {}", std::iter::once(command.clone()).chain(masked_args(args)).collect::<Vec<_>>().join(" ")), args.clone()),
        },
    }
}

fn package<'a>(command: &str, args: &'a [String]) -> Option<(&'a str, &'a [String])> {
    let runner = Path::new(command).file_name()?.to_str()?;
    let args = match runner {
        "npx" | "bunx" if args.first().is_some_and(|a| a == "-y" || a == "--yes") => &args[1..],
        "npx" | "bunx" | "pnpx" => args,
        _ => return None,
    };
    let (package, rest) = args.split_first()?;
    Some((package, rest))
}

/// Groups of differently named servers sharing a URL or a package, each group once.
/// Read-only servers are left out: Prism can't rename them. So are inherited ones: renaming
/// them from a project would edit the global configs.
fn merges(servers: &[McpServer]) -> Vec<MergeHint> {
    let mut alike: BTreeMap<(String, Vec<String>), BTreeSet<&str>> = BTreeMap::new();
    for s in servers.iter().filter(|s| s.status != Status::ReadOnly && !s.inherited) {
        for c in &s.copies {
            alike.entry(likeness(&c.server.transport)).or_default().insert(&s.name);
        }
    }
    let mut seen = BTreeSet::new();
    let mut hints: Vec<MergeHint> = alike
        .into_iter()
        .filter(|(_, names)| names.len() > 1 && seen.insert(names.clone()))
        .map(|((reason, _), names)| MergeHint { names: names.into_iter().map(String::from).collect(), reason })
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
        assert_eq!(gitlab.diffs, [FieldDiff { field: "env.TOKEN".into(), values: vec![(agent("claude"), masked.clone()), (agent("cursor"), masked)] }]);
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
        assert_eq!(ctx7.diffs, [FieldDiff { field: "headers.CONTEXT7_API_KEY".into(), values }]);
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
        assert_eq!(figma.diffs, [FieldDiff { field: "args".into(), values: vec![(agent("claude"), shown.clone()), (agent("cursor"), shown)] }]);
        let remote = inv.get("remote").unwrap();
        let values = vec![
            (agent("claude"), Some("https://***@mcp.example.com/mcp?api_key=***&team=1".to_string())),
            (agent("cursor"), Some("https://mcp.example.com/mcp?api_key=***&team=1".to_string())),
        ];
        assert_eq!(remote.diffs, [FieldDiff { field: "url".into(), values }]);
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
            [hint(&["context7", "ctx"], "same URL https://mcp.context7.com/mcp"), hint(&["figma", "figma-dev", "figma-pn"], "same package figma-developer-mcp")],
            "read-only copies and other args don't count"
        );
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
