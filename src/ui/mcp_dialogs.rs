//! MCP dialogs: Add (and Edit, which reuses its fields) and the plan dialog that copy, edit,
//! remove, disable/enable and merge show before writing. Values are masked unless the user
//! shows one field. Both sit in the `ds` dialog frame; fields and picks go in cards, plans are
//! `Plan`s.

use rune::{El, Tx, col, row};
use rune_kit::{ButtonVariant, Checkbox, Kit, Placement, RadioItem, Select, TextInput, glyphs};

use crate::agents::tilde;
use crate::app::{App, McpChange, McpDialog, McpForm, Planned, Target};
use crate::ds::{self, Card, DialogFrame, DialogKind, Notice, Place, Plan, Tone, field, field_pair, loading, plain_row, size, space, text};
use crate::mcp::agents::{Flavor, MCP_AGENTS};
use crate::mcp::plan::ClaudeScope;
use crate::mcp::scan::Status;
use crate::plan_view::PlanView;

fn form_mut(s: &mut App) -> &mut McpForm {
    s.mcp_form.as_mut().expect("the MCP form is open")
}

/// Cancel in a dialog's footer.
fn cancel(key: &str, k: &Kit, on: impl Fn(&mut App) + 'static) -> El {
    ds::button("Cancel", Place::Bar).on_press(on).build(k).key(key.to_string())
}

/// A control with a hint under it.
fn with_hint(k: &Kit, control: El, hint: impl Into<String>) -> El {
    col().w_full().gap(space::S1).child(control).child(text::hint(k, hint))
}

/// The name and, beside it, whether the server runs as a command or at a URL.
fn server_card(f: &McpForm, k: &Kit) -> El {
    let name = TextInput::new(&f.name)
        .label("Name")
        .placeholder("github")
        .width(None)
        .invalid(f.error.is_some())
        .on_edit(|s: &mut App| &mut form_mut(s).name)
        .on_submit(|s: &mut App, _: &mut Tx| s.save_mcp_form())
        .build(k)
        .key("mcp-add-name");
    let labels = ["Command".to_string(), "URL".to_string()];
    let kind = ds::segments(&labels, Some(usize::from(f.http)), "Runs as", |s: &mut App, i: usize, _: &mut Tx| s.set_mcp_kind(i == 1)).build(k).key("mcp-add-kind");
    Card::new("mcp-add-server").label("Server").row(field_pair(field(k, "Name", name), field(k, "Runs as", kind), None)).build(k).expect("one row")
}

/// The command line or the URL; a prefilled one with a likely secret is masked until shown.
fn line_card(f: &McpForm, k: &Kit) -> El {
    let (edit, key, placeholder, label) = if f.http {
        (&f.url, "mcp-add-url", "https://mcp.example.com/mcp", "URL")
    } else {
        (&f.command, "mcp-add-command", "npx -y @scope/server-mcp --flag \"a value\"", "Command and arguments")
    };
    fn on_edit(s: &mut App) -> &mut rune::TextEdit {
        let f = form_mut(s);
        if f.http { &mut f.url } else { &mut f.command }
    }
    let input = if f.line_secret {
        ds::secret_input(edit, on_edit, f.line_shown, |s: &mut App| s.toggle_mcp_line_shown())
            .label(label)
            .placeholder(placeholder)
            .invalid(f.error.is_some())
            .on_submit(|s: &mut App, _: &mut Tx| s.save_mcp_form())
            .key(key)
            .eye_key("mcp-add-line-show")
            .build(k)
    } else {
        TextInput::new(edit)
            .label(label)
            .placeholder(placeholder)
            .width(None)
            .invalid(f.error.is_some())
            .on_edit(on_edit)
            .on_submit(|s: &mut App, _: &mut Tx| s.save_mcp_form())
            .build(k)
            .key(key)
    };
    Card::new("mcp-add-line").row(field(k, label, input)).build(k).expect("one row")
}

/// Env or header rows: a key, a masked value with its eye, and a remove button; then Add and
/// the `${VAR}` hint.
fn pairs_card(f: &McpForm, k: &Kit) -> El {
    let header = f.http;
    let what = if header { "header" } else { "env" };
    let mut card = Card::new(format!("mcp-add-{what}-card")).label(if header { "Headers" } else { "Environment" });
    let pairs = f.pairs(header);
    if !pairs.is_empty() {
        let mut rows = col().key(format!("mcp-add-{what}s")).w_full().gap(space::S2);
        for (i, p) in pairs.iter().enumerate() {
            let key = TextInput::new(&p.key)
                .label(if header { "Header name" } else { "Variable name" })
                .placeholder(if header { "Authorization" } else { "API_TOKEN" })
                .width(Some(170.0))
                .on_edit(move |s: &mut App| &mut form_mut(s).pairs_edit(header, i).key)
                .build(k)
                .key(format!("mcp-add-{what}-{i}-key"));
            let value = ds::secret_input(&p.value, move |s: &mut App| &mut form_mut(s).pairs_edit(header, i).value, p.shown, move |s: &mut App| s.toggle_mcp_pair_shown(header, i))
                .label("Value")
                .placeholder("value or ${VAR}")
                .key(format!("mcp-add-{what}-{i}-value"))
                .eye_key(format!("mcp-add-{what}-{i}-show"))
                .build(k);
            let remove = ds::icon_button(&glyphs::CLOSE, "Remove row", Place::Inspector)
                .on_press(move |s: &mut App| s.remove_mcp_pair(header, i))
                .build(k)
                .key(format!("mcp-add-{what}-{i}-remove"));
            rows = rows.child(row().w_full().gap(space::S1).center_y().child(key).child(text::hint(k, "=")).child(value.grow(1.0).min_w(0.0)).child(remove));
        }
        // The rows don't scroll on their own: the dialog body does. A field's focus ring is
        // drawn just outside the field, so the rows keep that much room around them and sit
        // that much further out, so the fields still line up with the rest of the form.
        let ring = size::FOCUS_RING;
        card = card.row(col().w_full().mx(-ring).my(-ring).child(rows.padding(ring)));
    }
    let add = ds::quiet(if header { "Add header" } else { "Add variable" }, Place::Inspector)
        .icon(&super::PLUS)
        .on_press(move |s: &mut App| s.add_mcp_pair(header))
        .build(k)
        .key(format!("mcp-add-{what}-add"));
    card.row(plain_row(with_hint(k, row().child(add), "Type ${VAR} to read a value from the environment instead of storing it in the config.")))
        .build(k)
        .expect("the add row")
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

/// Every MCP agent, ticked or not, in two columns; the ones that can't take the server here are
/// off, saying why.
fn agents_card(s: &App, f: &McpForm, k: &Kit) -> El {
    let boxes: Vec<El> = (0..MCP_AGENTS.len())
        .map(|a| {
            let agent = &MCP_AGENTS[a];
            let (detected, holds) = (s.mcp_detected(a), s.mcp_can_hold(a));
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
    let mut left = col().grow(1.0).min_w(0.0).gap(space::S2);
    let mut right = col().grow(1.0).min_w(0.0).gap(space::S2);
    for (i, b) in boxes.into_iter().enumerate() {
        if i < half {
            left = left.child(b);
        } else {
            right = right.child(b);
        }
    }
    let columns = row().key("mcp-add-agents").w_full().gap(space::S4).items_start().child(left).child(right);
    Card::new("mcp-add-agent-card").label("Agents").row(columns).build(k).expect("one row")
}

/// Where Claude Code keeps a project server: on this machine, or in the repository.
fn claude_scope_card(f: &McpForm, k: &Kit) -> El {
    let item = |label: &str, detail: &str, scope: ClaudeScope, key: &str| {
        RadioItem::new(label, f.claude_scope == scope).description(detail).on_select(move |s: &mut App| s.set_claude_scope(scope)).build(k).key(key.to_string())
    };
    let choices = row()
        .key("mcp-add-scope")
        .role("radiogroup")
        .label("Claude Code keeps it for")
        .focus_group()
        .w_full()
        .gap(space::S4)
        .items_start()
        .child(item("Only me", "In ~/.claude.json; tokens stay off the repository", ClaudeScope::Local, "mcp-add-scope-local").grow(1.0).min_w(0.0))
        .child(item("Team (.mcp.json)", "In the repository; Claude Code asks to approve it", ClaudeScope::Project, "mcp-add-scope-project").grow(1.0).min_w(0.0));
    Card::new("mcp-add-scope-card").label("Claude Code keeps it for").row(choices).build(k).expect("one row")
}

/// Add a server, or edit one: the same fields; editing keeps the name and the agents.
pub fn form(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(f) = &s.mcp_form else { return };
    let editing = f.editing.is_some();
    let title = match &f.editing {
        Some((name, _)) => format!("Edit {name}"),
        None => format!("Add an MCP server to {}", s.scope_label(&s.scope)),
    };
    let mut frame = DialogFrame::new("mcp-form", title, DialogKind::Form);
    frame = if editing {
        frame.body(text::body(k, "Changes go to every copy; each keeps the fields you don't change. The plan comes next."))
    } else {
        frame.body(server_card(f, k))
    };
    frame = frame.body(line_card(f, k)).body(pairs_card(f, k));
    if !editing {
        frame = frame.body(agents_card(s, f, k));
        if !s.scope.is_global() {
            frame = frame.body(claude_scope_card(f, k));
        }
    }
    let save = ds::primary(if editing { "Review changes" } else { "Add server" }, Place::Bar).on_press(|s: &mut App| s.save_mcp_form()).build(k).key("mcp-add-save");
    frame
        .error(f.error.clone())
        .error_key("mcp-add-error")
        .footer(cancel("mcp-add-cancel", k, |s| s.close_mcp_form()), save)
        .build(k, |s: &mut App| s.close_mcp_form(), layer);
}

/// The machine a copy goes to: this one, or another environment. Its menu goes in `menus`,
/// which the frame draws over the dialog.
fn machine_select(s: &App, across: Option<&Target>, k: &Kit, menus: &mut Vec<El>) -> El {
    let here = s.env_label();
    let mut labels = vec![here.clone()];
    labels.extend(s.across.machines.iter().map(|m| m.label.clone()));
    let current = across.map_or(here, |t| t.machine.clone());
    let select = Select::new("mcp-copy-machine", &labels)
        .current(Some(&current))
        .label("Machine")
        .placement(Placement::BottomStart)
        .disabled(across.is_some_and(|t| t.applying))
        .open(s.across.machine_menu)
        .on_toggle(|s: &mut App| s.across.machine_menu = !s.across.machine_menu)
        .on_close(|s: &mut App| s.across.machine_menu = false)
        .on_choose(|s: &mut App, v: &str, _: &mut Tx| s.set_mcp_copy_machine(v))
        .build(k, menus)
        .w_full();
    field(k, "Machine", select)
}

/// On another machine: rows for the "Copy to" card (where there: Global or one of its
/// projects), and a card of its installed agents once they are in.
fn across_picks(s: &App, t: &Target, k: &Kit, menus: &mut Vec<El>) -> (Vec<El>, Option<El>) {
    let view = match s.machine(&t.machine).and_then(|m| m.view.as_ref()) {
        None => return (vec![loading(k, format!("Asking {} for its agents…", t.machine))], None),
        Some(Err(e)) => return (vec![Notice::new(Tone::Bad, e.clone()).build(k)], None),
        Some(Ok(view)) => view,
    };
    let mut rows = Vec::new();
    if !view.projects.is_empty() {
        let scopes: Vec<(String, crate::agents::Scope)> = view.scopes().into_iter().map(|sc| (view.label(&sc), sc)).collect();
        let labels: Vec<String> = scopes.iter().map(|(l, _)| l.clone()).collect();
        let select = Select::new("mcp-copy-scope", &labels)
            .current(Some(&view.label(&t.dest)))
            .label("Scope")
            .placement(Placement::BottomStart)
            .disabled(t.applying)
            .open(s.across.scope_menu)
            .on_toggle(|s: &mut App| s.across.scope_menu = !s.across.scope_menu)
            .on_close(|s: &mut App| s.across.scope_menu = false)
            .on_choose(move |s: &mut App, v: &str, _: &mut Tx| {
                if let Some((_, scope)) = scopes.iter().find(|(l, _)| l == v) {
                    s.set_mcp_copy_scope(scope.clone());
                }
            })
            .build(k, menus)
            .w_full();
        rows.push(field(k, "Scope", select));
    }
    let agents = view.agents();
    let mut card = Card::new("mcp-copy-agents").label("Agents");
    if agents.is_empty() {
        card = card.row(text::hint(k, format!("No agent that takes MCP servers is installed on {}.", t.machine)));
    }
    for a in agents {
        let ticked = t.agents.as_ref().is_some_and(|on| on.contains(&a));
        card = card.row(
            Checkbox::new(ticked)
                .label(MCP_AGENTS[a].name)
                .disabled(t.applying)
                .on_toggle(move |s: &mut App| s.toggle_mcp_copy_to(a))
                .build(k)
                .key(format!("mcp-copy-to-{}", MCP_AGENTS[a].id)),
        );
    }
    (rows, card.build(k))
}

/// What the user picks before the plan, as cards: a copy's source and targets, what to remove
/// from, the merged name. `None` for edit and disable/enable.
fn picks(s: &App, d: &McpDialog, k: &Kit, menus: &mut Vec<El>) -> Option<Vec<El>> {
    let change = &d.change;
    let server = s.mcp.get(change.name())?;
    let mut cards = Vec::new();
    match change {
        McpChange::Copy { to, from, .. } => {
            if server.status == Status::Differs {
                let hint = if server.inherited { "The copies differ; new copies get the one you pick." } else { "The copies differ; the one you pick also overwrites the others." };
                let mut sources = Card::new("mcp-copy-from").label("Copy from").row(text::hint(k, hint));
                let mut seen = Vec::new();
                for copy in &server.copies {
                    let a = copy.agent;
                    if seen.contains(&a) {
                        continue;
                    }
                    seen.push(a);
                    let agent = &MCP_AGENTS[a];
                    sources = sources.row(
                        RadioItem::new(agent.name, *from == Some(a))
                            .description(file_label(s, &copy.file))
                            .on_select(move |s: &mut App| s.set_mcp_copy_from(a))
                            .build(k)
                            .key(format!("mcp-copy-from-{}", agent.id)),
                    );
                }
                cards.extend(sources.build(k).map(|c| c.role("radiogroup").label("Copy from").focus_group()));
            }
            let mut targets = Card::new("mcp-copy-targets").label("Copy to");
            if !s.across.machines.is_empty() {
                targets = targets.row(machine_select(s, d.across.as_ref(), k, menus));
            }
            if let Some(t) = &d.across {
                let (rows, agents) = across_picks(s, t, k, menus);
                cards.extend(targets.rows(rows).build(k));
                cards.extend(agents);
                return Some(cards);
            }
            let free: Vec<usize> = (0..MCP_AGENTS.len()).filter(|&a| s.mcp_detected(a) && s.mcp_can_hold(a) && !server.copies.iter().any(|x| x.agent == a)).collect();
            if free.is_empty() {
                targets = targets.row(text::hint(k, "Every installed agent has it."));
            }
            for a in free {
                targets = targets.row(
                    Checkbox::new(to.contains(&a))
                        .label(MCP_AGENTS[a].name)
                        .on_toggle(move |s: &mut App| s.toggle_mcp_copy_to(a))
                        .build(k)
                        .key(format!("mcp-copy-to-{}", MCP_AGENTS[a].id)),
                );
            }
            cards.extend(targets.build(k));
        }
        McpChange::Remove { from, .. } => {
            let mut boxes = Card::new("mcp-remove-picks").label("Remove from");
            for a in s.mcp_own_agents(server) {
                boxes = boxes.row(
                    Checkbox::new(from.contains(&a))
                        .label(MCP_AGENTS[a].name)
                        .on_toggle(move |s: &mut App| s.toggle_mcp_remove_from(a))
                        .build(k)
                        .key(format!("mcp-remove-from-{}", MCP_AGENTS[a].id)),
                );
            }
            cards.extend(boxes.build(k));
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
            let card = Card::new("mcp-merge-picks").label("One name for").row(text::hint(k, names.join(", "))).row(field(k, "Name", input));
            cards.extend(card.build(k));
        }
        McpChange::Edit { .. } | McpChange::Switch { .. } => return None,
    }
    Some(cards)
}

/// "3 changes", or "Changes" when there are none.
fn changes_heading(n: usize) -> String {
    if n == 0 { "Changes".to_string() } else { format!("{n} change{}", if n == 1 { "" } else { "s" }) }
}

/// The plan, as the CLI prints it: its lines, the skipped agents and the heads-ups. On a host,
/// the plan the host made, once it is in. Also the count the apply button names.
fn plan_of(s: &App, d: &McpDialog, k: &Kit) -> (Plan, usize) {
    let mut plan = Plan::new("mcp-plan");
    if let Some(notice) = d.across.as_ref().and_then(|t| t.notice.as_ref()).or(d.notice.as_ref()) {
        plan = plan.notice(Notice::new(Tone::Warn, notice.clone()).build(k).key("mcp-plan-notice"));
    }
    if let Some(t) = &d.across {
        return match &t.plan {
            Planned::Asking => (plan.heading("Changes").waiting(format!("Asking {} for the plan…", t.machine)), 0),
            Planned::Waiting(why) => (plan.heading("Changes").note(why.clone()), 0),
            Planned::Done(Err(e)) => (plan.failed(e.clone()), 0),
            // The machine's lines join agents, as a host's do.
            Planned::Done(Ok(view)) => (lines(plan, view.clone(), view.lines.len()), view.lines.len()),
        };
    }
    match (&d.plan, &d.host_plan) {
        (_, Some(host)) => match &host.view {
            None => (plan.heading("Changes").waiting(format!("Asking {} for the plan…", s.env_label())), 0),
            Some(Err(e)) => (plan.failed(e.clone()), 0),
            // The host's lines already join agents, so they are the count.
            Some(Ok(view)) => (lines(plan, view.clone(), view.lines.len()), view.lines.len()),
        },
        (None, None) => (plan.heading("Changes").note("Pick the copy to copy from."), 0),
        (Some(Err(e)), None) => (plan.failed(e.clone()), 0),
        // Lines for several agents share one, so the count is the plan's actions, not the lines.
        (Some(Ok(p)), None) => (lines(plan, PlanView::from((p, &s.paths)), p.actions.len()), p.actions.len()),
    }
}

/// A plan's lines under their count, then what it skips and the heads-ups.
fn lines(plan: Plan, view: PlanView, n: usize) -> Plan {
    let mut plan = plan.heading(changes_heading(n));
    if view.lines.is_empty() {
        plan = plan.note("Nothing to do.");
    }
    for line in view.lines {
        plan = plan.line(line);
    }
    plan.skipped(view.skipped.iter().map(|(who, why)| format!("{who}: {why}")).collect()).heads_up(view.warnings)
}

fn plan_title(d: &McpDialog) -> String {
    match &d.change {
        McpChange::Copy { name, .. } if d.across.is_some() => format!("Copy {name} to {}", d.across.as_ref().map_or("", |t| t.machine.as_str())),
        McpChange::Copy { name, .. } => format!("Copy {name} to more agents"),
        McpChange::Edit { name, .. } => format!("Edit {name}"),
        McpChange::Remove { name, .. } => format!("Remove {name}"),
        McpChange::Switch { name, disabled: true } => format!("Disable {name} in this project"),
        McpChange::Switch { name, disabled: false } => format!("Enable {name} in this project"),
        McpChange::Merge { names, .. } => format!("Merge {}", names.join(", ")),
    }
}

/// Copy, edit, remove, disable/enable and merge: the plan first, then Apply. Copy and merge put
/// their picks on the left of the plan (Wide); remove puts them above it (Form).
pub fn plan(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(d) = &s.mcp_dialog else { return };
    let mut menus = Vec::new();
    let picks = picks(s, d, k, &mut menus);
    let (plan, n) = plan_of(s, d, k);
    let changes = col().key("mcp-plan").gap(space::S4).children(plan.build(k));
    let beside = matches!(d.change, McpChange::Copy { .. } | McpChange::Merge { .. });
    let (kind, body) = match picks {
        Some(p) if beside => {
            let left = col().key("mcp-picks").w(size::DIALOG_PICKS).gap(space::S4).children(p);
            (DialogKind::Wide, vec![row().w_full().gap(space::S4).items_start().child(left).child(changes.grow(1.0).min_w(0.0))])
        }
        Some(p) => (DialogKind::Form, vec![col().key("mcp-picks").w_full().gap(space::S4).children(p), changes.w_full()]),
        None => (DialogKind::Form, vec![changes.w_full()]),
    };
    let danger = matches!(d.change, McpChange::Remove { .. });
    let applying = d.host_plan.as_ref().is_some_and(|p| p.applying) || d.across.as_ref().is_some_and(|t| t.applying);
    let apply = match (applying, n) {
        (true, _) => ds::primary("Applying…", Place::Bar).variant(ButtonVariant::Loading).disabled(true),
        (false, n) => {
            let label = if n == 0 { "Nothing to do".to_string() } else { format!("Apply {n} change{}", if n == 1 { "" } else { "s" }) };
            if danger { ds::danger(label, Place::Bar) } else { ds::primary(label, Place::Bar) }.disabled(n == 0)
        }
    };
    let mut frame = DialogFrame::new("mcp-plan", plan_title(d), kind);
    for b in body {
        frame = frame.body(b);
    }
    frame
        .footer(cancel("mcp-cancel", k, |s| s.close_mcp_dialog()), apply.on_press(|s: &mut App| s.apply_mcp_plan()).build(k).key("mcp-apply"))
        .menus(menus)
        .build(k, |s: &mut App| s.close_mcp_dialog(), layer);
}
