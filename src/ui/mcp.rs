//! The MCP servers in a scope: the list with its status filters and merge hints, and the
//! inspector with a server's values (masked until revealed), its copies and their drift.

use rune::{Color, El, Tx, col, row, text};
use rune_kit::shell::Toolbar;
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, SegmentedControl, glyphs};

use super::installed::tag;
use super::{PLUS, REFRESH, dot, empty, field, heading, inspector_body, mono, page_tabs, scope_title, scroll_pane};
use crate::agents::{self, Scope, tilde};
use crate::app::{App, McpFilter};
use crate::mcp::agents::MCP_AGENTS;
use crate::mcp::model::{EnvValue, McpCopy, Place, Transport, masked_args, masked_url};
use crate::mcp::scan::{McpServer, Status, safe_error};

pub fn page(s: &App, k: &Kit, _layer: &mut Vec<El>) -> (El, Toolbar, El) {
    (main(s, k), toolbar(s, k), inspector(s, k))
}

fn toolbar(s: &App, k: &Kit) -> Toolbar {
    Toolbar::new().leading(page_tabs(s, k)).title(scope_title(&s.scope, "MCP servers")).trailing(
        row()
            .gap(6.0)
            .center_y()
            .child(
                IconButton::new(&REFRESH)
                    .variant(IconButtonVariant::GhostMuted)
                    .label("Rescan")
                    .help("Rescan")
                    .shortcut("cmd+r")
                    .on_press(|s: &mut App| s.rescan())
                    .build(k)
                    .key("rescan"),
            )
            .child(
                Button::new("Add server")
                    .icon(&PLUS)
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Regular)
                    .help("Add an MCP server to your agents")
                    .shortcut("cmd+n")
                    .on_press(|s: &mut App| s.open_mcp_add())
                    .build(k)
                    .key("mcp-add"),
            ),
    )
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

fn main(s: &App, k: &Kit) -> El {
    let shown = filters(&s.scope);
    let labels: Vec<String> = shown.iter().map(|&f| filter_label(s, f)).collect();
    let current = shown.iter().position(|&f| f == s.mcp_filter);
    let picks = shown.clone();
    let segments = SegmentedControl::new(&labels, current)
        .label("Show")
        .fit()
        .on_pick(move |s: &mut App, i: usize, _: &mut Tx| s.set_mcp_filter(picks[i]))
        .build(k)
        .key("mcp-filter");

    let visible = s.mcp_visible();
    let mut list = col().key("mcp-servers").w_full().gap(2.0);
    if s.mcp.servers.is_empty() {
        list = list.child(empty("No MCP servers here", "Servers your agents' configs list for this scope show up here.", k));
    } else if visible.is_empty() {
        let hint = match s.mcp_filter {
            McpFilter::All => "",
            McpFilter::Differs => "Every server's copies agree.",
            McpFilter::Disabled => "No server is switched off in this project.",
            McpFilter::ReadOnly => "No app manages a server here.",
        };
        list = list.child(empty("Nothing to show", hint, k));
    }
    for &i in &visible {
        list = list.child(server_row(s, &s.mcp.servers[i], k));
    }
    if !s.mcp.merges.is_empty() {
        let mut hints = col().key("mcp-merges").w_full().pt(10.0).px(12.0).gap(6.0);
        for (i, m) in s.mcp.merges.iter().enumerate() {
            let mut line = notice_line(k.c.info.icon, format!("Maybe one server: {} — {}", m.names.join(", "), m.reason), k);
            // The core merges only servers this scope owns and Prism may change.
            let mergeable = m.names.iter().all(|n| s.mcp.get(n).is_some_and(|x| !x.inherited && x.status != Status::ReadOnly));
            if mergeable {
                let names = m.names.clone();
                line = line.child(
                    Button::new("Merge…")
                        .variant(ButtonVariant::Outline)
                        .control_size(ControlSize::Small)
                        .help("Rename the copies to one name")
                        .on_press(move |s: &mut App| s.open_mcp_merge(names.clone()))
                        .build(k)
                        .key(format!("mcp-merge-{i}")),
                );
            }
            hints = hints.child(line);
        }
        list = list.child(hints);
    }

    let total = s.mcp.servers.len();
    let root = tilde(&s.mcp.scope.root(&s.paths), &s.paths);
    let summary = format!("{total} server{} for {root}", if total == 1 { "" } else { "s" });
    let header = row().w_full().h(36.0).px(12.0).center_y().child(text(summary).font(k.ty.small).color(k.c.text_faint).truncate());
    let mut page = col().key("mcp").w_full().h_full().min_h(0.0).padding(16.0).gap(10.0);
    if !s.mcp.errors.is_empty() {
        let mut errors = col().key("mcp-errors").w_full().px(12.0).gap(4.0);
        for (file, e) in &s.mcp.errors {
            errors = errors.child(notice_line(k.c.warning.icon, format!("Couldn't read {}: {}", tilde(file, &s.paths), safe_error(e)), k));
        }
        page = page.child(errors);
    }
    page.child(row().w_full().child(segments)).child(header).child(scroll_pane("mcp-scroll", list).grow(1.0))
}

/// A quiet line of small text after a coloured dot.
fn notice_line(color: Color, line: String, k: &Kit) -> El {
    row().w_full().gap(8.0).center_y().child(dot(color)).child(text(line).font(k.ty.small).color(k.c.text_muted).grow(1.0).min_w(0.0).wrap())
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

fn status(server: &McpServer, k: &Kit) -> (&'static str, Color, String) {
    match server.status {
        Status::InSync => ("in sync", k.c.success.icon, "Every copy has the same transport and values".into()),
        Status::Differs => ("differs", k.c.warning.icon, "The copies disagree; the inspector shows how".into()),
        Status::ReadOnly => {
            let apps: Vec<&str> = server.copies.iter().filter_map(|c| c.read_only.as_deref()).collect();
            ("read-only", k.c.text_faint, format!("Managed by {}", apps.join(", ")))
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

fn server_row(s: &App, server: &McpServer, k: &Kit) -> El {
    let selected = s.mcp_selected.as_deref() == Some(server.name.as_str());
    let name = server.name.clone();
    let mut tags = row().key("tags").gap(4.0).center_y();
    if server.inherited {
        tags = tags.child(tag("global", None, "A global server; it applies in this project too", k).key("global-tag"));
    }
    if !server.disabled_for.is_empty() {
        let who: Vec<&str> = server.disabled_for.iter().map(|&a| MCP_AGENTS[a].name).collect();
        tags = tags.child(tag("disabled", Some(k.c.warning.icon), &format!("Switched off here for {}", who.join(", ")), k).key("disabled-tag"));
    }
    let mut chips = row().key("agents").gap(4.0).center_y();
    for a in agents_of(server) {
        let copies: Vec<&McpCopy> = server.copies.iter().filter(|c| c.agent == a).collect();
        let files: Vec<String> = copies.iter().map(|c| tilde(&c.file, &s.paths)).collect();
        let off = server.disabled_for.contains(&a);
        let color = if off { Some(k.c.warning.icon) } else { None };
        let help = format!("{}: {}{}", MCP_AGENTS[a].name, files.join(", "), if off { " (off here)" } else { "" });
        chips = chips.child(tag(short(a), color, &help, k).key(format!("agent-{}", MCP_AGENTS[a].id)));
    }
    let (word, color, why) = status(server, k);
    let status = row()
        .key("status")
        .w(88.0)
        .gap(6.0)
        .center_y()
        .justify_end()
        .help(why)
        .child(dot(color))
        .child(text(word).font(k.ty.small).color(k.c.text_muted));
    let info = col()
        .gap(2.0)
        .grow(1.0)
        .min_w(120.0)
        .child(text(server.name.clone()).font(k.ty.ui_medium).color(k.c.text_base).truncate())
        .child(text(summary(&server.copies[0].server.transport)).font(k.ty.small).color(k.c.text_muted).truncate());
    let r = row()
        .key(format!("mcp-row-{}", server.name))
        .role("button")
        .label(server.name.clone())
        .focusable(true)
        .w_full()
        .px(12.0)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .radius(k.radius.md)
        .on_press(move |s: &mut App| s.select_mcp(&name))
        .child(info)
        .child(tags)
        .child(chips)
        .child(status);
    let r = if selected { r.fill(k.c.layer_02) } else { r.hover(k.c.hover) };
    // A global server seen from a project: there, but not the project's own.
    if server.inherited { r.dim(0.6) } else { r }
}

fn place_label(p: &Place) -> &'static str {
    match p {
        Place::Global => "global",
        Place::ClaudeLocal(_) => "local",
        Place::Project(_) => "project",
    }
}

fn inspector(s: &App, k: &Kit) -> El {
    let Some(server) = s.mcp_selected.as_deref().and_then(|n| s.mcp.get(n)) else {
        return inspector_body(k).child(empty("No server selected", "Click a server to see its values and where each agent keeps it.", k));
    };
    let revealed = s.mcp_revealed.contains(&server.name);
    let (word, color, why) = status(server, k);
    let mut about = vec![match server.status {
        Status::InSync if server.copies.len() > 1 => "Every copy has the same transport and values".to_string(),
        Status::InSync => "One copy".to_string(),
        Status::Differs => "The copies differ; the differences are below".to_string(),
        Status::ReadOnly => format!("{why}; Prism leaves it alone"),
    }];
    if server.inherited {
        about.push("A global server, used in this project".into());
    }
    if !server.disabled_for.is_empty() {
        let who: Vec<&str> = server.disabled_for.iter().map(|&a| MCP_AGENTS[a].name).collect();
        about.push(format!("Switched off here for {}", who.join(", ")));
    }
    let title = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(text(server.name.clone()).font(k.ty.body_medium).color(k.c.text_base).grow(1.0).min_w(0.0).wrap())
        .child(row().gap(6.0).center_y().child(dot(color)).child(text(word).font(k.ty.small).color(k.c.text_muted)));
    let mut body = inspector_body(k).child(title).child(text(about.join(". ")).font(k.ty.small).color(k.c.text_muted).wrap());
    if let Some(actions) = actions(s, server, k) {
        body = body.child(actions);
    }

    let first = &server.copies[0];
    let t = &first.server.transport;
    let mut values = col().key("mcp-values").w_full().gap(12.0);
    let reveal_label = if revealed { "Hide values" } else { "Show values" };
    let name = server.name.clone();
    let reveal = Button::new(reveal_label)
        .icon(if revealed { &glyphs::EYE_OFF } else { &glyphs::EYE })
        .variant(ButtonVariant::Outline)
        .control_size(ControlSize::Small)
        .help("Env and header values, args and the URL may hold secrets")
        .on_press(move |s: &mut App| s.toggle_reveal(&name))
        .build(k)
        .key("mcp-reveal");
    let mut kind = text(match t {
        Transport::Stdio { .. } => "stdio",
        Transport::Http { sse: true, .. } => "SSE",
        Transport::Http { .. } => "HTTP",
    }
    .to_string())
    .font(k.ty.ui)
    .color(k.c.text_base);
    if server.status == Status::Differs {
        kind = kind.help(format!("As {} has it; the differences are below", MCP_AGENTS[first.agent].name));
    }
    values = values.child(row().w_full().gap(8.0).center_y().child(field("Transport", kind, k).grow(1.0)).child(reveal));
    match t {
        Transport::Stdio { command, args, env, cwd } => {
            let line: Vec<String> = if revealed {
                std::iter::once(command.clone()).chain(args.iter().cloned()).collect()
            } else {
                std::iter::once(masked_url(command)).chain(masked_args(args)).collect()
            };
            values = values.child(field("Command", mono(line.join(" "), k).key("mcp-command"), k));
            if let Some(cwd) = cwd {
                values = values.child(field("Working folder", mono(if revealed { cwd.clone() } else { masked_url(cwd) }, k), k));
            }
            if !env.is_empty() {
                values = values.child(field("Environment", pairs(env.iter(), revealed, k), k));
            }
        }
        Transport::Http { url, headers, .. } => {
            values = values.child(field("URL", mono(if revealed { url.clone() } else { masked_url(url) }, k).key("mcp-url"), k));
            if !headers.is_empty() {
                values = values.child(field("Headers", pairs(headers.iter(), revealed, k), k));
            }
        }
    }
    body = body.child(values).child(copies(s, server, k));
    if !server.diffs.is_empty() {
        body = body.child(diffs(server, k));
    }
    body
}

/// Copy, edit and the project switch on the left; Remove apart on the right. An app's server
/// has none; a global server seen from a project offers only Edit and the switch.
fn actions(s: &App, server: &McpServer, k: &Kit) -> Option<El> {
    if server.status == Status::ReadOnly {
        return None;
    }
    let small = |label: &str| Button::new(label).variant(ButtonVariant::Outline).control_size(ControlSize::Small);
    let (copy, edit, switch, remove) = (server.name.clone(), server.name.clone(), server.name.clone(), server.name.clone());
    let mut left = row().gap(6.0).center_y();
    // A global server seen from a project is copied from the global scope.
    if !server.inherited {
        left = left.child(small("Copy to…").help("Write it to more agents").on_press(move |s: &mut App| s.open_mcp_copy(&copy)).build(k).key("mcp-copy"));
    }
    left = left.child(
            small("Edit…")
                .help(if server.inherited { "Edits the global server" } else { "Change its command, URL or values in every copy" })
                .on_press(move |s: &mut App| s.open_mcp_edit(&edit))
                .build(k)
                .key("mcp-edit"),
        );
    if !s.scope.is_global() {
        let (label, key, help) = if server.disabled_for.is_empty() {
            ("Disable here", "mcp-disable", "Switch it off for this project only")
        } else {
            ("Enable here", "mcp-enable", "Switch it back on for this project")
        };
        left = left.child(small(label).help(help).on_press(move |s: &mut App| s.toggle_mcp_disabled(&switch)).build(k).key(key));
    }
    let mut actions = row().key("mcp-actions").w_full().gap(8.0).center_y().child(left).child(row().grow(1.0));
    if !server.inherited {
        actions = actions.child(
            Button::new("Remove…")
                .variant(ButtonVariant::Danger)
                .control_size(ControlSize::Small)
                .on_press(move |s: &mut App| s.confirm_mcp_remove(&remove))
                .build(k)
                .key("mcp-remove"),
        );
    }
    Some(actions)
}

/// Env or header keys with their values: masked, or as written once revealed.
fn pairs<'a>(items: impl Iterator<Item = (&'a String, &'a EnvValue)>, revealed: bool, k: &Kit) -> El {
    let mut c = col().w_full().gap(4.0);
    for (key, v) in items {
        let value = if revealed { v.written() } else { v.masked() };
        c = c.child(
            row()
                .w_full()
                .gap(8.0)
                .child(text(key.clone()).font(k.ty.mono).color(k.c.text_base).shrink().min_w(0.0).truncate())
                .child(text("=").font(k.ty.mono).color(k.c.text_faint))
                .child(mono(value, k).grow(1.0).min_w(0.0)),
        );
    }
    c
}

/// Where each agent keeps its copy.
fn copies(s: &App, server: &McpServer, k: &Kit) -> El {
    let mut list = col().key("mcp-copies").w_full().gap(12.0).child(heading("Agents", k));
    for (i, c) in server.copies.iter().enumerate() {
        let off = server.disabled_for.contains(&c.agent) || !c.enabled;
        let mut title = row().gap(6.0).center_y().child(text(MCP_AGENTS[c.agent].name).font(k.ty.ui_medium).color(k.c.text_base));
        if off {
            title = title.child(dot(k.c.warning.icon));
        }
        if c.read_only.is_some() {
            title = title.child(rune::icon(&glyphs::LOCK, 12.0).color(k.c.icon_muted));
        }
        let mut detail = format!("{} · {}", place_label(&c.place), tilde(&c.file, &s.paths));
        if let Some(app) = &c.read_only {
            detail.push_str(&format!(" · managed by {app}"));
        }
        if off {
            detail.push_str(" · off here");
        }
        list = list.child(
            col()
                .key(format!("copy-{i}"))
                .w_full()
                .gap(2.0)
                .child(title)
                .child(text(detail).font(k.ty.small).color(k.c.text_faint).wrap_anywhere()),
        );
    }
    list
}

/// Field by field, what each copy has where they disagree (values come masked).
fn diffs(server: &McpServer, k: &Kit) -> El {
    let mut table = col().key("mcp-diffs").w_full().gap(10.0).child(heading("Differences", k));
    for d in &server.diffs {
        let mut group = col().w_full().gap(3.0).child(text(d.field.clone()).font(k.ty.small_medium).color(k.c.text_base));
        for (agent, value) in &d.values {
            let shown = match value {
                Some(v) => mono(v.clone(), k).grow(1.0).min_w(0.0),
                None => text("missing").font(k.ty.small).color(k.c.text_faint),
            };
            group = group.child(row().w_full().gap(8.0).center_y().child(text(short(*agent)).font(k.ty.small).color(k.c.text_muted).w(72.0)).child(shown));
        }
        table = table.child(group);
    }
    table
}
