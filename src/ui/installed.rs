//! The skills installed in a scope, and the selected one's placements across agents.

use rune::{Color, El, Tx, col, row, text};
use rune_kit::shell::Toolbar;
use rune_kit::{
    Badge, BadgeVariant, Button, ButtonVariant, ControlSize, Divider, IconButton, IconButtonVariant, Kit, Markdown, Switch, TextInput, glyphs,
};

use super::{REFRESH, SYNC, dot, empty, field, heading, inspector_body, mono, row_gap, scroll_pane};
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
                Button::new(if unsynced == 0 { "Sync".to_string() } else { format!("Sync {unsynced}") })
                    .icon(&SYNC)
                    .variant(if unsynced == 0 { ButtonVariant::Ghost } else { ButtonVariant::Contrast })
                    .control_size(ControlSize::Regular)
                    .help("Link every skill from .agents into the selected agents")
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
        .child(
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(text(summary).font(k.ty.small).color(k.c.text_muted).grow(1.0).truncate())
                .child(filter),
        )
        .child(scroll_pane("skills-scroll", list).grow(1.0));
    (main, toolbar, details(s, k))
}

/// The agents a row shows chips for: `.agents` and every agent that has it.
fn shown_agents(skill: &Skill) -> Vec<usize> {
    (0..AGENTS.len()).filter(|&a| a == CANONICAL || skill.placements[a] != Placement::Missing).collect()
}

fn skill_row(s: &App, skill: &Skill, targets: &[usize], k: &Kit) -> El {
    let selected = s.selected.as_deref() == Some(skill.name.as_str());
    let name = skill.name.clone();
    let mut chips = row().gap(4.0).center_y();
    for a in shown_agents(skill) {
        chips = chips.child(chip(AGENTS[a].short, &skill.placements[a], s.inv.shares_canonical[a], k));
    }
    let missing: Vec<&str> = targets.iter().filter(|&&t| skill.placements[t] == Placement::Missing).map(|&t| AGENTS[t].name).collect();
    if !missing.is_empty() {
        chips = chips.child(
            row()
                .h(20.0)
                .px(6.0)
                .center_y()
                .radius(k.radius.sm)
                .border(1.0, k.c.border_weak)
                .help(format!("Not in {}", missing.join(", ")))
                .child(text(format!("+{} missing", missing.len())).font(k.ty.small).color(k.c.text_faint)),
        );
    }
    let mut info = col().gap(2.0).grow(1.0).min_w(120.0).child(text(skill.name.clone()).font(k.ty.ui_medium).color(k.c.text_base).truncate());
    if !skill.description.is_empty() {
        info = info.child(text(skill.description.clone()).font(k.ty.small).color(k.c.text_muted).truncate());
    }
    let mut r = row()
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
        .on_press(move |s: &mut App| s.select(&name))
        .child(info)
        .child(chips);
    r = if selected { r.fill(k.c.layer_02) } else { r.hover(k.c.hover) };
    r
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

fn chip(label: &str, p: &Placement, shared: bool, k: &Kit) -> El {
    let present = p.present() || matches!(p, Placement::Broken(_));
    let mut c = row()
        .h(20.0)
        .px(6.0)
        .gap(4.0)
        .center_y()
        .radius(k.radius.sm)
        .border(1.0, if present { k.c.border_base } else { k.c.border_weak })
        .help(format!("{label}: {}", if shared { "same folder as .agents" } else { p.label() }));
    if let Some(color) = placement_color(p, k) {
        c = c.child(dot(color));
    }
    c.child(text(label.to_string()).font(k.ty.small).color(if present { k.c.text_base } else { k.c.text_faint }))
}

fn details(s: &App, k: &Kit) -> El {
    let Some(skill) = s.selected.as_deref().and_then(|n| s.inv.get(n)) else {
        return inspector_body(k).child(empty("No skill selected", "Pick one to see where it's installed.", k));
    };
    let mut body = inspector_body(k).child(text(skill.name.clone()).font(k.ty.body_medium).color(k.c.text_base).wrap());
    if !skill.description.is_empty() {
        body = body.child(text(skill.description.clone()).font(k.ty.small).color(k.c.text_muted).wrap());
    }
    if let Some(src) = &skill.source {
        body = body.child(field("Folder", mono(tilde(src, &s.paths), k), k));
    }

    let (reveal, remove) = (skill.source.clone(), skill.name.clone());
    body = body.child(
        row_gap()
            .child(
                Button::new("Reveal in Finder")
                    .variant(ButtonVariant::Outline)
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
                    .on_press(move |s: &mut App| s.confirm_remove = Some(remove.clone()))
                    .build(k)
                    .key("remove"),
            ),
    );
    let mut agents = col().key("placements").w_full().gap(10.0).child(heading("Agents", k));
    for a in 0..AGENTS.len() {
        agents = agents.child(placement_row(s, skill, a, k));
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
    let mut label = col()
        .gap(1.0)
        .grow(1.0)
        .min_w(0.0)
        .child(
            row()
                .gap(6.0)
                .center_y()
                .child(text(agent.name).font(k.ty.ui_medium).color(k.c.text_base))
                .child(match placement_color(p, k) {
                    Some(c) => dot(c),
                    None => row(),
                }),
        )
        .child(text(agent.dir_label(&s.inv.scope, &s.paths)).font(k.ty.small).color(k.c.text_faint).truncate());
    let status = if shared { "same folder as .agents".to_string() } else { p.label().to_string() };
    label = label.child(text(status).font(k.ty.small).color(k.c.text_muted));

    let name = skill.name.clone();
    let control: El = if a == CANONICAL {
        if p.present() {
            Badge::new("source").variant(BadgeVariant::Subtle).build(k)
        } else {
            Button::new("Move here")
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .help("Move the agent's copy into .agents and link it back")
                .on_press(move |s: &mut App| s.replace_with_link(&name, CANONICAL))
                .build(k)
        }
    } else if shared {
        row()
    } else if *p == Placement::Dir {
        let other = name.clone();
        row()
            .gap(4.0)
            .child(
                Button::new("Link")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Small)
                    .help("Replace this copy with a link to .agents (the copy goes to the trash)")
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
        Switch::new(matches!(p, Placement::Linked | Placement::Link(_)))
            .label(format!("Link {} into {}", skill.name, agent.name))
            .hide_label()
            .on_toggle(move |s: &mut App| s.toggle_link(&name, a))
            .build(k)
            .key(format!("link-{}", agent.id))
    };
    row().key(format!("agent-{}", agent.id)).w_full().gap(10.0).center_y().child(label).child(control)
}
