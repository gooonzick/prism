//! The skills installed in a scope, and the selected one's placements across agents.

use rune::{Color, El, Tx, col, row, text};
use rune_kit::shell::Toolbar;
use rune_kit::{
    Button, ButtonVariant, Checkbox, ControlSize, Divider, IconButton, IconButtonVariant, Kit, Markdown, Switch, TextInput,
    glyphs,
};

use super::{REFRESH, SYNC, dot, empty, field, heading, inspector_body, mono, scroll_pane};
use crate::agents::{AGENTS, CANONICAL, Scope, tilde};
use crate::app::App;
use crate::scan::{Placement, Skill};

pub fn page(s: &App, k: &Kit, _layer: &mut Vec<El>) -> (El, Toolbar, El) {
    let targets = s.targets();
    let visible = s.visible();
    let unsynced = s.inv.skills.iter().filter(|x| !x.synced(&targets)).count();
    let title = match &s.scope {
        Scope::Global => "Global skills".to_string(),
        Scope::Project(p) => p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()),
    };
    let toolbar = Toolbar::new().title(title).trailing(
        row()
            .gap(6.0)
            .center_y()
            .child(
                IconButton::new(&REFRESH)
                    .variant(IconButtonVariant::GhostMuted)
                    .label("Rescan")
                    .help("Rescan")
                    .shortcut("⌘R")
                    .on_press(|s: &mut App| s.rescan())
                    .build(k)
                    .key("rescan"),
            )
            .child(
                Button::new(match unsynced {
                    0 => "Sync".to_string(),
                    1 => "Sync 1 skill".to_string(),
                    n => format!("Sync {n} skills"),
                })
                .icon(&SYNC)
                // The one primary action, unless skills are ticked: then the bulk bar's is.
                .variant(if unsynced == 0 || !s.checked.is_empty() { ButtonVariant::Ghost } else { ButtonVariant::Contrast })
                .control_size(ControlSize::Regular)
                .help("Link every skill from .agents into your sync targets")
                .on_press(|s: &mut App| s.open_sync())
                .build(k)
                .key("sync"),
            ),
    );

    let root = tilde(&s.inv.scope.root(&s.paths), &s.paths);
    let summary = format!(
        "{} skill{} in {root} · {}",
        s.inv.skills.len(),
        if s.inv.skills.len() == 1 { "" } else { "s" },
        if unsynced == 0 { "all in sync".to_string() } else { format!("{unsynced} not synced") }
    );
    let filter = TextInput::new(&s.filter)
        .placeholder("Filter skills")
        .leading_icon(&glyphs::SEARCH)
        .clear_button()
        .label("Filter skills")
        .width(Some(240.0))
        .on_edit(|s: &mut App| &mut s.filter)
        .build(k)
        .key("filter");
    let mut list = col().key("skills").w_full().gap(2.0);
    if s.inv.skills.is_empty() {
        list = list.child(empty(
            "No skills here yet",
            "Install one from skills.sh, or add folders with a SKILL.md to .agents/skills.",
            k,
        ));
    } else if visible.is_empty() {
        list = list.child(empty("Nothing matches", "Try another word.", k));
    }
    let header = list_header(s, &visible, summary, filter, k);
    for i in visible {
        list = list.child(skill_row(s, &s.inv.skills[i], &targets, k));
    }
    let main = col()
        .key("installed")
        .w_full()
        .h_full()
        .min_h(0.0)
        .padding(16.0)
        .gap(12.0)
        .child(header)
        .child(scroll_pane("skills-scroll", list).grow(1.0));
    (main, toolbar, details(s, k))
}

/// Above the list: a tick-all box, then either the scope's summary or, with skills ticked,
/// what can be done to them all; the filter on the right.
fn list_header(s: &App, visible: &[usize], summary: String, filter: El, k: &Kit) -> El {
    let ticked = visible.iter().filter(|&&i| s.is_checked(&s.inv.skills[i].name)).count();
    let partial = ticked > 0 && ticked < visible.len();
    let mut all = Checkbox::new(!visible.is_empty() && ticked == visible.len()).indeterminate(partial);
    if partial {
        // The kit draws its check for a mixed state unless given another mark.
        all = all.icon(&glyphs::DASH);
    }
    let all = all
        .label(if ticked == visible.len() && ticked > 0 { "Untick all" } else { "Tick all shown" })
        .hide_label()
        .disabled(visible.is_empty())
        .on_toggle(|s: &mut App| s.toggle_all_visible())
        .build(k)
        .key("check-all");
    let n = s.checked.len();
    let middle = if n == 0 {
        row().grow(1.0).min_w(0.0).child(text(summary).font(k.ty.small).color(k.c.text_muted).truncate())
    } else {
        row()
            .key("bulk-bar")
            .grow(1.0)
            .min_w(0.0)
            .gap(8.0)
            .center_y()
            .child(text(format!("{n} selected")).font(k.ty.small_medium).color(k.c.text_base))
            .child(
                Button::new(format!("Sync {n}"))
                    .icon(&SYNC)
                    .variant(ButtonVariant::Contrast)
                    .control_size(ControlSize::Small)
                    .help("Sync only the selected skills")
                    .on_press(|s: &mut App| s.open_sync_checked())
                    .build(k)
                    .key("bulk-sync"),
            )
            .child(
                Button::new("Clear")
                    .variant(ButtonVariant::GhostMuted)
                    .control_size(ControlSize::Small)
                    .help("Untick everything (Esc)")
                    .on_press(|s: &mut App| s.clear_checked())
                    .build(k)
                    .key("bulk-clear"),
            )
            .child(row().grow(1.0))
            .child(
                Button::new(format!("Remove {n}…"))
                    .variant(ButtonVariant::Danger)
                    .control_size(ControlSize::Small)
                    .on_press(|s: &mut App| s.confirm_remove_checked())
                    .build(k)
                    .key("bulk-remove"),
            )
    };
    row().w_full().h(36.0).px(12.0).gap(12.0).center_y().child(all).child(middle).child(filter)
}

/// The agents a row shows chips for: every agent that has it, `.agents` first.
fn shown_agents(skill: &Skill) -> Vec<usize> {
    (0..AGENTS.len()).filter(|&a| skill.placements[a] != Placement::Missing).collect()
}

/// A skill's sync state in a few words, its colour, and the detail for its tooltip.
fn sync_status(skill: &Skill, targets: &[usize], k: &Kit) -> (String, Color, String) {
    let missing: Vec<&str> = targets.iter().filter(|&&t| skill.placements[t] == Placement::Missing).map(|&t| AGENTS[t].name).collect();
    match skill.canonical() {
        Placement::Broken(_) => ("broken in .agents".into(), k.c.danger.icon, "Its .agents entry is a link to nothing".into()),
        Placement::Missing if !skill.placements.iter().any(Placement::present) => {
            ("broken links".into(), k.c.danger.icon, "Every copy is a link to nothing".into())
        }
        Placement::Missing => ("not in .agents".into(), k.c.warning.icon, "Sync moves it into .agents and links it back".into()),
        _ if missing.is_empty() => ("synced".into(), k.c.success.icon, "Linked into every sync target".into()),
        _ => (format!("{} to link", missing.len()), k.c.text_faint, format!("Not linked into {}", missing.join(", "))),
    }
}

fn skill_row(s: &App, skill: &Skill, targets: &[usize], k: &Kit) -> El {
    let selected = s.selected.as_deref() == Some(skill.name.as_str());
    let ticked = s.is_checked(&skill.name);
    let name = skill.name.clone();
    let tick_name = skill.name.clone();
    let tick = Checkbox::new(ticked)
        .label(format!("Select {}", skill.name))
        .hide_label()
        .on_toggle_tx(move |s: &mut App, tx: &mut Tx| s.click_tick(&tick_name, tx.mods()))
        .build(k)
        .key(format!("check-{}", skill.name));
    let mut chips = row().gap(4.0).center_y();
    for a in shown_agents(skill) {
        chips = chips.child(chip(AGENTS[a].short, &skill.placements[a], s.inv.shares_canonical[a], k));
    }
    let (status, color, why) = sync_status(skill, targets, k);
    let status = row()
        .key("status")
        .w(104.0)
        .gap(6.0)
        .center_y()
        .justify_end()
        .help(why)
        .child(dot(color))
        .child(text(status).font(k.ty.small).color(k.c.text_muted));
    let mut info = col().gap(2.0).grow(1.0).min_w(120.0).child(text(skill.name.clone()).font(k.ty.ui_medium).color(k.c.text_base).truncate());
    if !skill.description.is_empty() {
        info = info.child(text(skill.description.clone()).font(k.ty.small).color(k.c.text_muted).truncate());
    }
    let r = row()
        .key(format!("skill-{}", skill.name))
        .role("button")
        .label(skill.name.clone())
        .focusable(true)
        .w_full()
        .px(12.0)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .radius(k.radius.md)
        .on_press_tx(move |s: &mut App, tx: &mut Tx| s.click_row(&name, tx.mods()))
        .child(tick)
        .child(info)
        .child(chips)
        .child(status);
    match (selected, ticked) {
        (true, _) => r.fill(k.c.layer_02),
        (false, true) => r.fill(k.c.layer_01).hover(k.c.hover),
        (false, false) => r.hover(k.c.hover),
    }
}

fn placement_color(p: &Placement, k: &Kit) -> Option<Color> {
    match p {
        Placement::Linked => Some(k.c.success.icon),
        Placement::Dir => Some(k.c.info.icon),
        Placement::Link(_) => Some(k.c.warning.icon),
        Placement::Broken(_) => Some(k.c.danger.icon),
        Placement::Missing => None,
    }
}

/// Where an agent has the skill: a quiet label, with a dot for how.
fn chip(label: &str, p: &Placement, shared: bool, k: &Kit) -> El {
    let mut c = row()
        .h(20.0)
        .px(6.0)
        .gap(4.0)
        .center_y()
        .radius(k.radius.sm)
        .border(1.0, k.c.border_weak)
        .help(format!("{label}: {}", if shared { "same folder as .agents" } else { p.label() }));
    if let Some(color) = placement_color(p, k) {
        c = c.child(dot(color));
    }
    c.child(text(label.to_string()).font(k.ty.small).color(k.c.text_muted))
}

fn details(s: &App, k: &Kit) -> El {
    let Some(skill) = s.selected.as_deref().and_then(|n| s.inv.get(n)) else {
        return inspector_body(k).child(empty("No skill selected", "Click a skill to see where it's installed and change it.", k));
    };
    let mut body = inspector_body(k).child(text(skill.name.clone()).font(k.ty.body_medium).color(k.c.text_base).wrap());
    if !skill.description.is_empty() {
        body = body.child(text(skill.description.clone()).font(k.ty.small).color(k.c.text_muted).wrap());
    }
    if let Some(src) = &skill.source {
        body = body.child(field("Folder", mono(tilde(src, &s.paths), k), k));
    }

    // Reveal on the left, the destructive action apart from it on the right.
    let (reveal, remove) = (skill.source.clone(), skill.name.clone());
    body = body.child(
        row()
            .w_full()
            .center_y()
            .justify_between()
            .child(
                Button::new("Reveal in Finder")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Small)
                    .disabled(reveal.is_none())
                    .on_press(move |_: &mut App| {
                        if let Some(p) = &reveal {
                            let _ = std::process::Command::new("open").arg("-R").arg(p).spawn();
                        }
                    })
                    .build(k)
                    .key("reveal"),
            )
            .child(
                Button::new("Remove…")
                    .variant(ButtonVariant::Danger)
                    .control_size(ControlSize::Small)
                    .on_press(move |s: &mut App| s.confirm_remove = Some(vec![remove.clone()]))
                    .build(k)
                    .key("remove"),
            ),
    );

    // The agents that matter here: .agents, the sync targets and any that has the skill.
    let targets = s.targets();
    let (shown, hidden): (Vec<usize>, Vec<usize>) =
        (0..AGENTS.len()).partition(|&a| a == CANONICAL || targets.contains(&a) || skill.placements[a] != Placement::Missing);
    let mut agents = col().key("placements").w_full().gap(12.0).child(heading("Agents", k));
    for &a in &shown {
        agents = agents.child(placement_row(s, skill, a, k));
    }
    if !hidden.is_empty() {
        if s.show_all_agents {
            for &a in &hidden {
                agents = agents.child(placement_row(s, skill, a, k));
            }
        }
        let label = if s.show_all_agents { "Show fewer agents".to_string() } else { format!("Show {} more agents", hidden.len()) };
        // A text control, so it lines up with the agent names above it.
        agents = agents.child(
            row()
                .key("more-agents")
                .role("button")
                .label(label.clone())
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .on_press(|s: &mut App| s.show_all_agents = !s.show_all_agents)
                .child(text(label).font(k.ty.small_medium).color(k.c.text_muted)),
        );
    }
    body = body.child(agents);

    if let Some(md) = &s.preview {
        body = body.child(Divider::horizontal().build(k)).child(heading("SKILL.md", k)).child(
            Markdown::new(md.clone())
                .on_link_tx(|_: &mut App, url: &str, tx: &mut Tx| {
                    let _ = tx.open_url(url);
                })
                .build(k)
                .key("skill-md"),
        );
    }
    body
}

fn placement_row(s: &App, skill: &Skill, a: usize, k: &Kit) -> El {
    let agent = &AGENTS[a];
    let p = &skill.placements[a];
    let shared = s.inv.shares_canonical[a];
    let status = match (a == CANONICAL, p) {
        _ if shared => "same folder as .agents",
        (true, Placement::Dir | Placement::Link(_)) => "the skill's home",
        (true, Placement::Missing) => "not here yet",
        _ => p.label(),
    };
    let mut title = row().gap(6.0).center_y().child(text(agent.name).font(k.ty.ui_medium).color(k.c.text_base));
    if let Some(c) = placement_color(p, k) {
        title = title.child(dot(c));
    }
    let label = col()
        .gap(2.0)
        .grow(1.0)
        .min_w(0.0)
        .child(title)
        .child(text(format!("{} · {status}", agent.dir_label(&s.inv.scope, &s.paths))).font(k.ty.small).color(k.c.text_faint).truncate());

    let name = skill.name.clone();
    let control: El = if a == CANONICAL {
        if p.present() || matches!(p, Placement::Broken(_)) {
            row()
        } else {
            Button::new("Move into .agents")
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .help("Move the agent's copy into .agents and leave a link behind")
                .on_press(move |s: &mut App| s.replace_with_link(&name, CANONICAL))
                .build(k)
                .key("adopt")
        }
    } else if shared {
        row()
    } else if *p == Placement::Dir {
        let other = name.clone();
        row()
            .gap(4.0)
            .center_y()
            .child(
                Button::new("Use link")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Small)
                    .help("Replace this copy with a link to .agents; the copy goes to the trash")
                    .on_press(move |s: &mut App| s.replace_with_link(&name, a))
                    .build(k),
            )
            .child(
                IconButton::new(&glyphs::CLOSE)
                    .variant(IconButtonVariant::GhostMuted)
                    .label("Move this copy to the trash")
                    .help("Move this copy to the trash")
                    .on_press(move |s: &mut App| s.trash_copy(&other, a))
                    .build(k),
            )
    } else {
        let switch = Switch::new(matches!(p, Placement::Linked | Placement::Link(_)));
        // Off, the compact switch vanishes on a light pane and the standard one on a dark
        // pane; each theme gets the one that stays visible.
        let switch = if k.dark { switch } else { switch.standard() };
        switch
            .label(format!("Link {} into {}", skill.name, agent.name))
            .hide_label()
            .on_toggle(move |s: &mut App| s.toggle_link(&name, a))
            .build(k)
            .key(format!("link-{}", agent.id))
    };
    row().key(format!("agent-{}", agent.id)).w_full().gap(10.0).center_y().child(label).child(control)
}
