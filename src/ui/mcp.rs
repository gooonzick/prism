//! The MCP servers in a scope: the list with its status filters and merge hints, and the
//! inspector with a server's values (masked until revealed), its copies and their drift.

use rune::{El, Tx, col, row};
use rune_kit::{Kit, glyphs};
use rune_kit::shell::Toolbar;

use super::{PLUS, REFRESH, scope_title};
use crate::agents::{self, Scope, tilde};
use crate::app::{App, McpFilter};
use crate::ds::{self, Card, Inspector, ListRow, MenuAction, Notice, Page, Place, Tone, item_row, size, pair_row, space, stacked_row, stacked_row_with, text, value_row};
use crate::mcp::agents::MCP_AGENTS;
use crate::mcp::model::{EnvValue, McpCopy, Place as CopyPlace, Transport, masked_args, masked_url};
use crate::mcp::scan::{McpServer, Status, safe_error};

/// `room` is what the toolbar has beside its tabs (`ui::toolbar_room`); `None` before the first
/// layout. `drawn` says the window has room for the inspector (`ui::inspector_pane`); without
/// it there is no inspector, nor its "…" menu.
pub fn page(s: &App, k: &Kit, layer: &mut Vec<El>, reduced: bool, room: Option<f32>, drawn: bool) -> (El, Toolbar, Option<El>) {
    (main(s, k, reduced), toolbar(s, k, room), drawn.then(|| inspector(s, k, layer)))
}

/// Tabs, the title, Rescan and Add server. When the list pane is too narrow for them, the title
/// goes and Add server shows only its icon.
fn toolbar(s: &App, k: &Kit, room: Option<f32>) -> Toolbar {
    let title = scope_title(&s.scope, "MCP servers");
    let rescan = ds::icon_button(&REFRESH, "Rescan", Place::Bar).shortcut("cmd+r").on_press(|s: &mut App| s.rescan()).build(k).key("rescan");
    let trailing = row().gap(space::S2).center_y().child(rescan);
    if !super::toolbar_fits(k, room, &title, 1, "Add server") {
        let add = ds::icon_button(&PLUS, "Add server", Place::Bar).shortcut("cmd+n").on_press(|s: &mut App| s.open_mcp_add()).build(k).key("mcp-add");
        return super::toolbar_untitled(s, k, trailing.child(add));
    }
    let add = ds::primary("Add server", Place::Bar)
        .icon(&PLUS)
        .help("Add an MCP server to your agents")
        .shortcut("cmd+n")
        .on_press(|s: &mut App| s.open_mcp_add())
        .build(k)
        .key("mcp-add");
    super::toolbar(s, k, title, trailing.child(add))
}

/// The filters a scope offers: nothing is disabled globally, so that filter is for projects.
fn filters(scope: &Scope) -> Vec<McpFilter> {
    let mut f = vec![McpFilter::All, McpFilter::Differs];
    if !scope.is_global() {
        f.push(McpFilter::Disabled);
    }
    f.push(McpFilter::ReadOnly);
    f
}

fn filter_label(s: &App, f: McpFilter) -> String {
    let n = s.mcp_filter_count(f);
    match f {
        McpFilter::All => format!("All {n}"),
        McpFilter::Differs => format!("Differs {n}"),
        McpFilter::Disabled => format!("Disabled {n}"),
        McpFilter::ReadOnly => format!("Read-only {n}"),
    }
}

fn main(s: &App, k: &Kit, reduced: bool) -> El {
    let shown = filters(&s.scope);
    let labels: Vec<String> = shown.iter().map(|&f| filter_label(s, f)).collect();
    let current = shown.iter().position(|&f| f == s.mcp_filter);
    let picks = shown.clone();
    let segments = ds::segments(&labels, current, "Show", move |s: &mut App, i: usize, _: &mut Tx| s.set_mcp_filter(picks[i])).build(k).key("mcp-filter");

    let total = s.mcp.servers.len();
    let root = tilde(&s.mcp.scope.root(&s.paths), &s.paths);
    let mut page = Page::new("mcp").segments(segments).count(format!("{total} server{} for {root}", if total == 1 { "" } else { "s" }));
    for (i, (file, e)) in s.mcp.errors.iter().enumerate() {
        let line = format!("Couldn't read {}: {}", tilde(file, &s.paths), safe_error(e));
        page = page.notice(Notice::new(Tone::Warn, line).build(k).key(format!("mcp-error-{i}")));
    }
    for (i, m) in s.mcp.merges.iter().enumerate() {
        let mut notice = Notice::new(Tone::Info, format!("Maybe one server: {} ({})", m.names.join(", "), m.reason));
        // The core merges only servers this scope owns and Prism may change.
        let mergeable = m.names.iter().all(|n| s.mcp.get(n).is_some_and(|x| !x.inherited && x.status != Status::ReadOnly));
        if mergeable {
            let names = m.names.clone();
            notice = notice.action(
                ds::button("Merge…", Place::List)
                    .help("Rename the copies to one name")
                    .on_press(move |s: &mut App| s.open_mcp_merge(names.clone()))
                    .build(k)
                    .key(format!("mcp-merge-{i}")),
            );
        }
        page = page.notice(notice.build(k).key(format!("mcp-merge-notice-{i}")));
    }

    let visible = s.mcp_visible();
    if s.mcp.servers.is_empty() {
        page = page.empty(ds::empty(k, "No MCP servers here", "Servers your agents' configs list for this scope show up here."));
    } else if visible.is_empty() {
        let hint = match s.mcp_filter {
            McpFilter::All => "",
            McpFilter::Differs => "Every server's copies agree.",
            McpFilter::Disabled => "No server is switched off in this project.",
            McpFilter::ReadOnly => "No app manages a server here.",
        };
        page = page.empty(ds::empty(k, "Nothing to show", hint));
    }
    page.rows(visible.iter().map(|&i| server_row(s, &s.mcp.servers[i], k, reduced))).build(k)
}

/// The short name a chip shows: the skills table's, where the agent is in it.
fn short(agent: usize) -> &'static str {
    let a = &MCP_AGENTS[agent];
    match agents::index_of(a.id) {
        Some(i) => agents::AGENTS[i].short,
        None if a.id == "claude-desktop" => "Desktop",
        None => a.name,
    }
}

/// Each agent once, in copy order (one agent can hold a name in two places).
fn agents_of(server: &McpServer) -> Vec<usize> {
    let mut agents: Vec<usize> = server.copies.iter().map(|c| c.agent).collect();
    agents.dedup();
    agents
}

/// A server's state in a word, its tone, and the detail for its tooltip.
pub fn status(server: &McpServer) -> (&'static str, Tone, String) {
    match server.status {
        Status::InSync => ("in sync", Tone::Ok, "Every copy has the same transport and values".into()),
        Status::Differs => ("differs", Tone::Warn, "The copies disagree; the inspector shows how".into()),
        Status::ReadOnly => {
            let apps: Vec<&str> = server.copies.iter().filter_map(|c| c.read_only.as_deref()).collect();
            ("read-only", Tone::Neutral, format!("Managed by {}", apps.join(", ")))
        }
    }
}

/// The command line or URL, masked: what a row shows under the name.
fn summary(t: &Transport) -> String {
    match t {
        Transport::Stdio { command, args, .. } => std::iter::once(masked_url(command)).chain(masked_args(args)).collect::<Vec<_>>().join(" "),
        Transport::Http { url, .. } => masked_url(url),
    }
}

fn server_row(s: &App, server: &McpServer, k: &Kit, reduced: bool) -> El {
    let mut tags = Vec::new();
    if server.inherited {
        tags.push(ds::tag(k, "global", None, "A global server; it applies in this project too").key("global-tag"));
    }
    if !server.disabled_for.is_empty() {
        let who: Vec<&str> = server.disabled_for.iter().map(|&a| MCP_AGENTS[a].name).collect();
        tags.push(ds::tag(k, "disabled", Some(Tone::Warn), format!("Switched off here for {}", who.join(", "))).key("disabled-tag"));
    }
    for a in agents_of(server) {
        let copies: Vec<&McpCopy> = server.copies.iter().filter(|c| c.agent == a).collect();
        let files: Vec<String> = copies.iter().map(|c| tilde(&c.file, &s.paths)).collect();
        let off = server.disabled_for.contains(&a);
        let tone = if off { Some(Tone::Warn) } else { None };
        let help = format!("{}: {}{}", MCP_AGENTS[a].name, files.join(", "), if off { " (off here)" } else { "" });
        tags.push(ds::tag(k, short(a), tone, help).key(format!("agent-{}", MCP_AGENTS[a].id)));
    }
    let (word, tone, why) = status(server);
    let name = server.name.clone();
    ListRow::new(format!("mcp-row-{}", server.name), server.name.clone())
        .meta(summary(&server.copies[0].server.transport))
        .tags(tags)
        .trailing(ds::status(k, tone, word).key("status").help(why))
        .selected(s.mcp_selected.as_deref() == Some(server.name.as_str()))
        // A global server seen from a project: there, but not the project's own.
        .dim(server.inherited)
        .reduced(reduced)
        .build(k, move |s: &mut App| s.select_mcp(&name))
}

fn place_label(p: &CopyPlace) -> &'static str {
    match p {
        CopyPlace::Global => "global",
        CopyPlace::ClaudeLocal(_) => "local",
        CopyPlace::Project(_) => "project",
    }
}

fn inspector(s: &App, k: &Kit, layer: &mut Vec<El>) -> El {
    let Some(server) = s.mcp_selected.as_deref().and_then(|n| s.mcp.get(n)) else {
        return Inspector::empty(k, "No server selected", "Click a server to see its values and where each agent keeps it.");
    };
    let revealed = s.mcp_revealed.contains(&server.name);
    let (word, tone, why) = status(server);
    let mut about = vec![match server.status {
        Status::InSync if server.copies.len() > 1 => "Every copy has the same transport and values".to_string(),
        Status::InSync => "One copy".to_string(),
        Status::Differs => differ_line(server),
        Status::ReadOnly => format!("{why}; Prism leaves it alone"),
    }];
    if server.inherited {
        about.push("A global server, used in this project".into());
    }
    if !server.disabled_for.is_empty() {
        let who: Vec<&str> = server.disabled_for.iter().map(|&a| MCP_AGENTS[a].name).collect();
        about.push(format!("Switched off here for {}", who.join(", ")));
    }
    let mut ins = Inspector::new().title(server.name.clone()).status(ds::status(k, tone, word).key("mcp-status")).description(about.join(". "));
    if let Some((left, right)) = actions(s, server, k, layer) {
        ins = ins.actions(left, right);
    }
    // On a host, shown values are the server as written there, fetched for this one only.
    let shown = s.shown_server(server);
    let (env_card, headers_card) = secrets(shown, revealed, k);
    ins.card(configuration(shown, revealed, k)).card(env_card).card(headers_card).card(copies(s, server, k)).card(diffs(shown, revealed, k)).build(k)
}

/// "Copies differ in command and args", naming up to two fields.
fn differ_line(server: &McpServer) -> String {
    let fields: Vec<&str> = server.diffs.iter().map(|d| d.field.as_str()).collect();
    match fields.as_slice() {
        [] => "The copies differ".into(),
        [one] => format!("Copies differ in {one}"),
        [a, b] => format!("Copies differ in {a} and {b}"),
        more => format!("Copies differ in {} fields", more.len()),
    }
}

/// The transport and its command or URL, as the first copy has them; Show values on the card.
fn configuration(server: &McpServer, revealed: bool, k: &Kit) -> Option<El> {
    let first = &server.copies[0];
    let name = server.name.clone();
    let reveal = ds::reveal(revealed, Place::Inspector)
        .help("Env and header values, args and the URL may hold secrets")
        .on_press(move |s: &mut App| s.toggle_reveal(&name))
        .build(k)
        .key("mcp-reveal");
    let mut card = Card::new("mcp-values").label("Configuration").trailing(reveal);
    if server.status == Status::Differs {
        card = card.row(text::hint(k, format!("As {} has it", MCP_AGENTS[first.agent].name)));
    }
    let t = &first.server.transport;
    let kind = match t {
        Transport::Stdio { .. } => "stdio",
        Transport::Http { sse: true, .. } => "SSE",
        Transport::Http { .. } => "HTTP",
    };
    card = card.row(pair_row(k, "Transport", text::value(k, kind)));
    let shown = |v: &String| if revealed { v.clone() } else { masked_url(v) };
    match t {
        Transport::Stdio { command, args, cwd, .. } => {
            let line: Vec<String> = if revealed {
                std::iter::once(command.clone()).chain(args.iter().cloned()).collect()
            } else {
                std::iter::once(masked_url(command)).chain(masked_args(args)).collect()
            };
            card = card.row(stacked_row(k, "Command", text::command(k, line.join(" ")).key("mcp-command")));
            if let Some(cwd) = cwd {
                card = card.row(stacked_row(k, "Working folder", text::code(k, shown(cwd))));
            }
        }
        Transport::Http { url, .. } => {
            card = card.row(stacked_row(k, "URL", text::code(k, shown(url)).key("mcp-url")));
        }
    }
    card.build(k)
}

/// The Environment and Headers cards: a row per key, its value masked, or as written once
/// revealed. Each is left out when there is nothing in it.
fn secrets(server: &McpServer, revealed: bool, k: &Kit) -> (Option<El>, Option<El>) {
    let card = |key: &str, label: &str, items: Vec<(&String, &EnvValue)>| {
        let rows = items.into_iter().map(|(name, v)| {
            let value = if revealed { v.written() } else { v.masked() };
            value_row(text::code(k, name.clone()), name, text::code(k, value.clone()), &value).key(format!("{key}-{name}"))
        });
        Card::new(key).label(label).rows(rows).build(k)
    };
    match &server.copies[0].server.transport {
        Transport::Stdio { env, .. } => (card("mcp-env", "Environment", env.iter().collect()), None),
        Transport::Http { headers, .. } => (None, card("mcp-headers", "Headers", headers.iter().collect())),
    }
}

/// Copy to… and Edit… on the left, then "…" with Disable here / Enable here; Remove… on the
/// right. An app's server has none; a global server seen from a project offers only Edit… and
/// the switch.
fn actions(s: &App, server: &McpServer, k: &Kit, layer: &mut Vec<El>) -> Option<(Vec<El>, Option<El>)> {
    if server.status == Status::ReadOnly {
        return None;
    }
    let (copy, edit, switch, remove) = (server.name.clone(), server.name.clone(), server.name.clone(), server.name.clone());
    let mut left = Vec::new();
    // A global server seen from a project is copied from the global scope.
    if !server.inherited {
        left.push(ds::button("Copy to…", Place::Inspector).help("Write it to more agents").on_press(move |s: &mut App| s.open_mcp_copy(&copy)).build(k).key("mcp-copy"));
    }
    left.push(
        ds::button("Edit…", Place::Inspector)
            .help(if server.inherited { "Edits the global server" } else { "Change its command, URL or values in every copy" })
            .on_press(move |s: &mut App| s.open_mcp_edit(&edit))
            .build(k)
            .key("mcp-edit"),
    );
    // One item whose label follows the state; a global scope has no project to switch it off in.
    let (label, help) = if server.disabled_for.is_empty() {
        ("Disable here", "Switch it off for this project only")
    } else {
        ("Enable here", "Switch it back on for this project")
    };
    let items = vec![MenuAction::new(label, move |s: &mut App| s.toggle_mcp_disabled(&switch)).description(help).disabled(s.scope.is_global())];
    left.push(ds::overflow::<App>(k, "mcp-more", Place::Inspector, s.mcp_menu, |s: &mut App| s.mcp_menu = !s.mcp_menu, |s: &mut App| s.mcp_menu = false, items, layer));
    let remove = (!server.inherited)
        .then(|| ds::danger("Remove…", Place::Inspector).on_press(move |s: &mut App| s.confirm_mcp_remove(&remove)).build(k).key("mcp-remove"));
    Some((left, remove))
}

/// Where each agent keeps its copy.
fn copies(s: &App, server: &McpServer, k: &Kit) -> Option<El> {
    let rows = server.copies.iter().enumerate().map(|(i, c)| {
        let off = server.disabled_for.contains(&c.agent) || !c.enabled;
        let mut name = row().gap(space::S2).center_y().min_w(0.0).child(text::name(k, MCP_AGENTS[c.agent].name).shrink());
        if off {
            name = name.child(ds::dot(k, Tone::Warn));
        }
        // An app manages this copy: Prism leaves it alone.
        if c.read_only.is_some() {
            name = name.child(rune::icon(&glyphs::LOCK, size::ICON).color(ds::color::text_muted(k)).key(format!("mcp-copy-{i}-lock")));
        }
        let mut detail = tilde(&c.file, &s.paths);
        if let Some(app) = &c.read_only {
            detail.push_str(&format!(" · managed by {app}"));
        }
        if off {
            detail.push_str(" · off here");
        }
        item_row(name, Some(text::hint(k, detail)), Some(text::label(k, place_label(&c.place)))).key(format!("copy-{i}"))
    });
    Card::new("mcp-copies").label("Agents").rows(rows).build(k)
}

/// Field by field, what each copy has where they disagree: masked, or as written once revealed.
fn diffs(server: &McpServer, revealed: bool, k: &Kit) -> Option<El> {
    let rows = server.diffs.iter().map(|d| {
        let mut values = col().w_full().gap(space::S1);
        for (agent, value) in d.shown(revealed) {
            values = values.child(match value {
                Some(v) => value_row(text::label(k, short(agent)), short(agent), text::code(k, v.to_string()), v),
                None => pair_row(k, short(agent), text::hint(k, "missing")),
            });
        }
        if d.hidden_only() && !revealed {
            values = values.child(text::hint(k, "Only hidden values differ; Show values compares them"));
        }
        stacked_row_with(text::code(k, d.field.clone()), values)
    });
    Card::new("mcp-diffs").label("Differences").rows(rows).build(k)
}
