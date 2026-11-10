//! MCP dialogs: Add (and Edit, which reuses its fields) and the plan dialog that copy, edit,
//! remove, disable/enable and merge show before writing. Values are masked unless the user
//! shows one field.

use rune::{Color, El, ScrollSpec, Tx, col, icon, row, text};
use rune_kit::{Button, ButtonVariant, Checkbox, ControlSize, Dialog, IconButton, IconButtonVariant, Kit, RadioItem, TextInput, glyphs};

use super::heading;
use crate::agents::tilde;
use crate::app::{App, McpChange, McpForm};
use crate::mcp::agents::{Flavor, MCP_AGENTS};
use crate::mcp::plan::ClaudeScope;
use crate::plan_view::PlanView;
use crate::mcp::scan::Status;

fn form_mut(s: &mut App) -> &mut McpForm {
    s.mcp_form.as_mut().expect("the MCP form is open")
}

fn body() -> El {
    col().w_full().px(16.0).pb(16.0).gap(16.0)
}

fn note(t: impl Into<String>, k: &Kit) -> El {
    text(t.into()).font(k.ty.small).color(k.c.text_muted).wrap()
}

/// A coloured mark before a line of small text: an error, a skip or a heads-up.
fn flagged(mark: El, line: String, k: &Kit) -> El {
    row().w_full().gap(6.0).items_start().child(mark).child(text(line).font(k.ty.small).color(k.c.text_muted).grow(1.0).min_w(0.0).wrap())
}

fn cross(color: Color) -> El {
    icon(&glyphs::CIRCLE_X, 14.0).color(color)
}

/// A dot in a 14-point box, to line up with [`cross`].
fn spot(color: Color) -> El {
    row().size(14.0, 16.0).center().child(super::dot(color))
}

fn block(label: &str, content: El, k: &Kit) -> El {
    col().w_full().gap(6.0).child(text(label.to_string()).font(k.ty.small_medium).color(k.c.text_base)).child(content)
}

/// Two segments, keyed, like the page tabs: a command or a URL.
fn kind_switch(http: bool, k: &Kit) -> El {
    let c = &k.c;
    let seg = |key: &str, label: &str, on: bool, pick: fn(&mut App)| {
        let seg = row()
            .key(key.to_string())
            .role("radio")
            .label(label.to_string())
            .focusable(true)
            .focus_selected(on)
            .focus_ring(2.0, 1.0, c.border_focus)
            .h(28.0)
            .px(12.0)
            .center()
            .radius(6.0)
            .cursor(rune::Cursor::Pointer)
            .on_press(pick)
            .child(text(label.to_string()).font(rune::TextStyle { line_height: 13.0, ..k.ty.ui }).color(if on { c.text_base } else { c.text_muted }).a11y_hidden());
        if on { seg.fill(c.bg_base).box_shadow(rune::Shadow { spread: 0.5, color: c.border_strong, ..Default::default() }) } else { seg.hover(c.hover) }
    };
    row()
        .key("mcp-add-kind")
        .role("radiogroup")
        .label("Runs as")
        .focus_group()
        .h(28.0)
        .center_y()
        .radius(6.0)
        .fill(c.layer_01)
        .box_shadow(rune::Shadow { spread: 0.5, color: c.border_base, ..Default::default() })
        .child(seg("mcp-add-kind-stdio", "Command", !http, |s| s.set_mcp_kind(false)))
        .child(seg("mcp-add-kind-http", "URL", http, |s| s.set_mcp_kind(true)))
}

/// An eye that shows or masks a field.
fn eye(shown: bool, key: String, on: impl Fn(&mut App) + 'static, k: &Kit) -> El {
    let label = if shown { "Hide value" } else { "Show value" };
    IconButton::new(if shown { &glyphs::EYE_OFF } else { &glyphs::EYE }).variant(IconButtonVariant::GhostMuted).label(label).help(label).on_press(on).build(k).key(key)
}

/// The command line or the URL; a prefilled one with a likely secret is masked until shown.
fn line_field(f: &McpForm, k: &Kit) -> El {
    let (edit, key, placeholder, label): (_, _, _, _) = if f.http {
        (&f.url, "mcp-add-url", "https://mcp.example.com/mcp", "URL")
    } else {
        (&f.command, "mcp-add-command", "npx -y @scope/server-mcp --flag \"a value\"", "Command and arguments")
    };
    let http = f.http;
    let mut input = TextInput::new(edit)
        .label(label)
        .placeholder(placeholder)
        .width(None)
        .invalid(f.error.is_some())
        .on_edit(move |s: &mut App| if http { &mut form_mut(s).url } else { &mut form_mut(s).command })
        .on_submit(|s: &mut App, _: &mut Tx| s.save_mcp_form());
    if f.line_secret {
        input = input.secure().revealed(f.line_shown);
    }
    let mut line = row().w_full().gap(4.0).center_y().child(input.build(k).key(key).grow(1.0).min_w(0.0));
    if f.line_secret {
        line = line.child(eye(f.line_shown, "mcp-add-line-show".into(), |s: &mut App| s.toggle_mcp_line_shown(), k));
    }
    block(label, line, k)
}

/// Env or header rows: a key, a masked value with its eye, and a remove button.
fn pairs(f: &McpForm, k: &Kit) -> El {
    let header = f.http;
    let what = if header { "header" } else { "env" };
    let mut rows = col().key(format!("mcp-add-{what}s")).w_full().gap(6.0);
    for (i, p) in f.pairs(header).iter().enumerate() {
        let key = TextInput::new(&p.key)
            .label(if header { "Header name" } else { "Variable name" })
            .placeholder(if header { "Authorization" } else { "API_TOKEN" })
            .width(Some(170.0))
            .on_edit(move |s: &mut App| &mut form_mut(s).pairs_edit(header, i).key)
            .build(k)
            .key(format!("mcp-add-{what}-{i}-key"));
        let value = TextInput::new(&p.value)
            .label("Value")
            .placeholder("value or ${VAR}")
            .width(None)
            .secure()
            .revealed(p.shown)
            .on_edit(move |s: &mut App| &mut form_mut(s).pairs_edit(header, i).value)
            .build(k)
            .key(format!("mcp-add-{what}-{i}-value"));
        rows = rows.child(
            row()
                .w_full()
                .gap(4.0)
                .center_y()
                .child(key)
                .child(text("=").font(k.ty.mono).color(k.c.text_faint))
                .child(value.grow(1.0).min_w(0.0))
                .child(eye(p.shown, format!("mcp-add-{what}-{i}-show"), move |s: &mut App| s.toggle_mcp_pair_shown(header, i), k))
                .child(
                    IconButton::new(&glyphs::CLOSE)
                        .variant(IconButtonVariant::GhostMuted)
                        .label("Remove row")
                        .help("Remove row")
                        .on_press(move |s: &mut App| s.remove_mcp_pair(header, i))
                        .build(k)
                        .key(format!("mcp-add-{what}-{i}-remove")),
                ),
        );
    }
    let add = Button::new(if header { "Add header" } else { "Add variable" })
        .icon(&super::PLUS)
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press(move |s: &mut App| s.add_mcp_pair(header))
        .build(k)
        .key(format!("mcp-add-{what}-add"));
    let hint = note("Type ${VAR} to read a value from the environment instead of storing it in the config.", k);
    // Past four rows the list scrolls, so the dialog keeps its buttons on screen.
    let list = col().w_full().max_h(150.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(rows);
    let section = col().w_full().gap(6.0).child(list).child(row().w_full().gap(8.0).center_y().child(add)).child(hint);
    block(if header { "Headers" } else { "Environment" }, section, k)
}

/// A config file as short as a checkbox line holds: relative to the project inside one, else
/// from `~`, and only its last two parts when that is still long.
fn file_label(s: &App, file: &std::path::Path) -> String {
    let root = s.scope.root(&s.paths);
    if !s.scope.is_global()
        && let Ok(rel) = file.strip_prefix(&root)
    {
        return rel.display().to_string();
    }
    let full = tilde(file, &s.paths);
    if full.chars().count() <= 34 {
        return full;
    }
    let parts: Vec<_> = file.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    format!("…/{}", parts[parts.len().saturating_sub(2)..].join("/"))
}

/// Every MCP agent, ticked or not; the ones that can't take the server here are off, saying why.
fn agents(s: &App, f: &McpForm, k: &Kit) -> El {
    let boxes: Vec<El> = (0..MCP_AGENTS.len())
        .map(|a| {
            let agent = &MCP_AGENTS[a];
            let (detected, holds) = (agent.detected(&s.paths), s.mcp_can_hold(a));
            let detail = match (detected, holds) {
                (false, _) => "not found".to_string(),
                (true, false) => "no project config".to_string(),
                (true, true) if agent.flavor == Flavor::ClaudeCode && f.claude_scope == ClaudeScope::Project => ".mcp.json".to_string(),
                (true, true) => file_label(s, &agent.new_server_file(&s.scope, &s.paths).0),
            };
            Checkbox::new(f.agents.contains(&a))
                .label(agent.name)
                .description(detail)
                .disabled(!detected || !holds)
                .on_toggle(move |s: &mut App| s.toggle_mcp_form_agent(a))
                .build(k)
                .key(format!("mcp-add-agent-{}", agent.id))
        })
        .collect();
    let half = boxes.len().div_ceil(2);
    let mut left = col().grow(1.0).min_w(0.0).gap(6.0);
    let mut right = col().grow(1.0).min_w(0.0).gap(6.0);
    for (i, b) in boxes.into_iter().enumerate() {
        if i < half {
            left = left.child(b);
        } else {
            right = right.child(b);
        }
    }
    block("Agents", row().key("mcp-add-agents").w_full().gap(16.0).items_start().child(left).child(right), k)
}

/// Where Claude Code keeps a project server: on this machine, or in the repository.
fn claude_scope(f: &McpForm, k: &Kit) -> El {
    let item = |label: &str, detail: &str, scope: ClaudeScope, key: &str| {
        RadioItem::new(label, f.claude_scope == scope).description(detail).on_select(move |s: &mut App| s.set_claude_scope(scope)).build(k).key(key.to_string())
    };
    block(
        "Claude Code keeps it for",
        row()
            .key("mcp-add-scope")
            .role("radiogroup")
            .label("Claude Code keeps it for")
            .focus_group()
            .w_full()
            .gap(16.0)
            .items_start()
            .child(item("Only me", "In ~/.claude.json; tokens stay off the repository", ClaudeScope::Local, "mcp-add-scope-local").grow(1.0).min_w(0.0))
            .child(item("Team (.mcp.json)", "In the repository; Claude Code asks to approve it", ClaudeScope::Project, "mcp-add-scope-project").grow(1.0).min_w(0.0)),
        k,
    )
}

/// Add a server, or edit one: the same fields; editing keeps the name and the agents.
pub fn form(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(f) = &s.mcp_form else { return };
    let editing = f.editing.is_some();
    let mut b = body();
    if editing {
        b = b.child(note("Changes go to every copy; each keeps the fields you don't change. The plan comes next.", k));
    } else {
        let name = TextInput::new(&f.name)
            .label("Name")
            .placeholder("github")
            .width(None)
            .invalid(f.error.is_some())
            .on_edit(|s: &mut App| &mut form_mut(s).name)
            .on_submit(|s: &mut App, _: &mut Tx| s.save_mcp_form())
            .build(k)
            .key("mcp-add-name");
        b = b.child(row().w_full().gap(12.0).items_end().child(block("Name", name, k).grow(1.0).min_w(0.0)).child(kind_switch(f.http, k)));
    }
    b = b.child(line_field(f, k)).child(pairs(f, k));
    if !editing {
        b = b.child(agents(s, f, k));
        if !s.scope.is_global() {
            b = b.child(claude_scope(f, k));
        }
    }
    if let Some(e) = &f.error {
        b = b.child(flagged(cross(k.c.danger.icon), e.clone(), k).key("mcp-add-error"));
    }
    let buttons = row()
        .w_full()
        .gap(8.0)
        .justify_end()
        .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.close_mcp_form()).build(k).key("mcp-add-cancel"))
        .child(Button::new(if editing { "Review changes" } else { "Add server" }).variant(ButtonVariant::Contrast).on_press(|s: &mut App| s.save_mcp_form()).build(k).key("mcp-add-save"));
    let title = match &f.editing {
        Some((name, _)) => format!("Edit {name}"),
        None => format!("Add an MCP server to {}", s.scope_label(&s.scope)),
    };
    Dialog::new("mcp-form", title)
        .width(620.0)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.close_mcp_form())
        .child(b.child(buttons))
        .build(k, layer);
}

/// What the user picks before the plan: a copy's source and targets, what to remove from,
/// the merged name. `None` for edit and disable/enable.
fn picks(s: &App, change: &McpChange, k: &Kit) -> Option<El> {
    let server = s.mcp.get(change.name())?;
    let mut c = col().key("mcp-picks").w_full().gap(12.0);
    match change {
        McpChange::Copy { to, from, .. } => {
            if server.status == Status::Differs {
                let mut sources = col().gap(10.0).role("radiogroup").label("Copy from").focus_group();
                let mut seen = Vec::new();
                for copy in &server.copies {
                    let a = copy.agent;
                    if seen.contains(&a) {
                        continue;
                    }
                    seen.push(a);
                    let agent = &MCP_AGENTS[a];
                    sources = sources.child(
                        RadioItem::new(agent.name, *from == Some(a))
                            .description(file_label(s, &copy.file))
                            .on_select(move |s: &mut App| s.set_mcp_copy_from(a))
                            .build(k)
                            .key(format!("mcp-copy-from-{}", agent.id)),
                    );
                }
                c = c.child(heading("Copy from", k)).child(note(if server.inherited { "The copies differ; new copies get the one you pick." } else { "The copies differ; the one you pick also overwrites the others." }, k)).child(sources);
            }
            let mut targets = col().gap(6.0);
            let free: Vec<usize> = (0..MCP_AGENTS.len()).filter(|&a| MCP_AGENTS[a].detected(&s.paths) && s.mcp_can_hold(a) && !server.copies.iter().any(|x| x.agent == a)).collect();
            if free.is_empty() {
                targets = targets.child(note("Every installed agent has it.", k));
            }
            for a in free {
                targets = targets.child(
                    Checkbox::new(to.contains(&a))
                        .label(MCP_AGENTS[a].name)
                        .on_toggle(move |s: &mut App| s.toggle_mcp_copy_to(a))
                        .build(k)
                        .key(format!("mcp-copy-to-{}", MCP_AGENTS[a].id)),
                );
            }
            c = c.child(heading("Copy to", k)).child(targets);
        }
        McpChange::Remove { from, .. } => {
            let mut boxes = col().gap(6.0);
            for a in s.mcp_own_agents(server) {
                boxes = boxes.child(
                    Checkbox::new(from.contains(&a))
                        .label(MCP_AGENTS[a].name)
                        .on_toggle(move |s: &mut App| s.toggle_mcp_remove_from(a))
                        .build(k)
                        .key(format!("mcp-remove-from-{}", MCP_AGENTS[a].id)),
                );
            }
            c = c.child(heading("Remove from", k)).child(boxes);
        }
        McpChange::Merge { names, into } => {
            let input = TextInput::new(into)
                .label("Name")
                .width(None)
                .on_edit(|s: &mut App| match s.mcp_dialog.as_mut().map(|d| &mut d.change) {
                    Some(McpChange::Merge { into, .. }) => &mut **into,
                    _ => unreachable!("the merge dialog is open"),
                })
                .on_change(|s: &mut App, _: &mut Tx| s.replan_mcp())
                .build(k)
                .key("mcp-merge-into");
            c = c.child(heading("One name for", k)).child(note(names.join(", "), k)).child(block("Name", input, k));
        }
        McpChange::Edit { .. } | McpChange::Switch { .. } => return None,
    }
    Some(c)
}

/// The plan, as the CLI prints it: its lines, the skipped agents and the heads-ups.
fn plan_view(s: &App, k: &Kit) -> (El, usize) {
    let d = s.mcp_dialog.as_ref().expect("the plan dialog is open");
    let mut c = col().key("mcp-plan").w_full().gap(8.0);
    let plan = match &d.plan {
        None => return (c.child(heading("Changes", k)).child(note("Pick the copy to copy from.", k)), 0),
        Some(Err(e)) => return (c.child(heading("Changes", k)).child(flagged(cross(k.c.danger.icon), e.clone(), k)), 0),
        Some(Ok(plan)) => plan,
    };
    let view = PlanView::from((plan, &s.paths));
    // Lines for several agents share one, so the count is the plan's actions, not the lines.
    let n = plan.actions.len();
    c = c.child(heading(&if n == 0 { "Changes".to_string() } else { format!("{n} change{}", if n == 1 { "" } else { "s" }) }, k));
    let lines = view.lines;
    if lines.is_empty() {
        c = c.child(note("Nothing to do.", k));
    }
    for (i, line) in lines.into_iter().enumerate() {
        c = c.child(text(line).key(format!("mcp-plan-{i}")).font(k.ty.ui).color(k.c.text_base).wrap());
    }
    if !view.skipped.is_empty() {
        c = c.child(col().pt(6.0).child(heading(&format!("Skipped ({})", view.skipped.len()), k)));
        for (who, why) in &view.skipped {
            c = c.child(flagged(cross(k.c.warning.icon), format!("{who}: {why}"), k));
        }
    }
    if !view.warnings.is_empty() {
        c = c.child(col().pt(6.0).child(heading("Heads-up", k)));
        for w in view.warnings {
            c = c.child(flagged(spot(k.c.info.icon), w, k));
        }
    }
    (c, n)
}

fn plan_title(change: &McpChange) -> String {
    match change {
        McpChange::Copy { name, .. } => format!("Copy {name} to more agents"),
        McpChange::Edit { name, .. } => format!("Edit {name}"),
        McpChange::Remove { name, .. } => format!("Remove {name}"),
        McpChange::Switch { name, disabled: true } => format!("Disable {name} in this project"),
        McpChange::Switch { name, disabled: false } => format!("Enable {name} in this project"),
        McpChange::Merge { names, .. } => format!("Merge {}", names.join(", ")),
    }
}

/// Copy, edit, remove, disable/enable and merge: the plan first, then Apply.
pub fn plan(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(d) = &s.mcp_dialog else { return };
    let (changes, n) = plan_view(s, k);
    let changes = col().w_full().h(240.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(changes);
    let mut b = body();
    if let Some(notice) = &d.notice {
        b = b.child(flagged(spot(k.c.warning.icon), notice.clone(), k).key("mcp-plan-notice"));
    }
    b = b.child(match picks(s, &d.change, k) {
        Some(p) => row().w_full().gap(20.0).items_start().child(col().w(240.0).child(p)).child(col().grow(1.0).min_w(0.0).child(changes)),
        None => changes,
    });
    let danger = matches!(d.change, McpChange::Remove { .. });
    let buttons = row()
        .w_full()
        .gap(8.0)
        .justify_end()
        .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.close_mcp_dialog()).build(k).key("mcp-cancel"))
        .child(
            Button::new(if n == 0 { "Nothing to do".to_string() } else { format!("Apply {n} change{}", if n == 1 { "" } else { "s" }) })
                .variant(if danger { ButtonVariant::Danger } else { ButtonVariant::Contrast })
                .disabled(n == 0)
                .on_press(|s: &mut App| s.apply_mcp_plan())
                .build(k)
                .key("mcp-apply"),
        );
    Dialog::new("mcp-plan", plan_title(&d.change))
        .width(if picks_shown(&d.change) { 760.0 } else { 560.0 })
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.close_mcp_dialog())
        .child(b.child(buttons))
        .build(k, layer);
}

fn picks_shown(change: &McpChange) -> bool {
    !matches!(change, McpChange::Edit { .. } | McpChange::Switch { .. })
}
