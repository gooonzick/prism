//! The inspector for an installed skill: what it is, its actions, and where each agent has it.

use rune::{El, col, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Divider, IconButton, IconButtonVariant, Kit, Switch, glyphs};

use super::installed::placement_color;
use super::{UPDATE, dot, empty, inspector_body, inspector_title, mono, prop, section, skill_md};
use crate::agents::{AGENTS, CANONICAL, tilde};
use crate::app::{App, open_path, reveal_path};
use crate::scan::{Placement, Skill};
use crate::skill::manifest_path;

pub fn installed(s: &App, k: &Kit) -> El {
    let Some(skill) = s.selected.as_deref().and_then(|n| s.inv.get(n)) else {
        return inspector_body(k).child(empty("No skill selected", "Click a skill, or move through the list with ↑ and ↓.", k));
    };
    let mut head = col().w_full().gap(4.0).child(inspector_title(skill.name.clone(), k));
    if !skill.description.is_empty() {
        head = head.child(text(skill.description.clone()).font(k.ty.caption()).color(k.c.text_muted).wrap());
    }
    let mut body = inspector_body(k).child(head);
    if s.has_update(&skill.name) {
        body = body.child(update_banner(s, &skill.name, k));
    }
    if let Some(src) = &skill.source {
        body = body.child(prop("Folder", mono(tilde(src, &s.paths), k).color(k.c.text_base), k));
    }
    body = body.child(actions(s, skill, k)).child(Divider::horizontal().build(k)).child(agents(s, skill, k));
    if let Some(md) = &s.preview {
        body = body.child(Divider::horizontal().build(k)).child(section("SKILL.md", None, k)).child(skill_md(md.clone(), k).key("skill-md"));
    }
    body
}

fn update_banner(s: &App, name: &str, k: &Kit) -> El {
    let target = name.to_string();
    row()
        .key("update-banner")
        .w_full()
        .px(12.0)
        .py(10.0)
        .gap(10.0)
        .center_y()
        .radius(k.radius.md)
        .border(1.0, k.c.border_weak)
        .child(dot(k.c.info.icon))
        .child(text("A newer version is on skills.sh").font(k.ty.small).color(k.c.text_base).grow(1.0).min_w(0.0).wrap())
        .child(
            Button::new(if s.updating.is_some() { "Updating…" } else { "Update" })
                .icon(&UPDATE)
                .variant(if s.updating.is_some() { ButtonVariant::Loading } else { ButtonVariant::Contrast })
                .control_size(ControlSize::Small)
                .disabled(s.updating.is_some())
                .on_press(move |s: &mut App| s.update(vec![target.clone()]))
                .build(k)
                .key("update"),
        )
}

/// Open, reveal and copy on the left; the destructive action apart from them on the right.
fn actions(s: &App, skill: &Skill, k: &Kit) -> El {
    // A host's files aren't on this machine: nothing here opens or reveals them.
    let remote = s.is_remote();
    let small = |label: &str| Button::new(label).variant(ButtonVariant::Outline).control_size(ControlSize::Small);
    // Looking for the file reads the disk: a host's folder isn't on this machine.
    let manifest = if remote { None } else { skill.source.as_deref().and_then(manifest_path) };
    let folder = skill.source.clone();
    let (copy, remove) = (skill.name.clone(), skill.name.clone());
    let mut left = row()
        .gap(6.0)
        .center_y()
        .child(
            small("Open SKILL.md")
                .disabled(remote || manifest.is_none())
                .on_press(move |_: &mut App| {
                    if let Some(p) = &manifest {
                        open_path(p);
                    }
                })
                .build(k)
                .key("open-md"),
        )
        .child(
            small("Reveal")
                .help(if cfg!(target_os = "macos") { "Show the folder in Finder" } else { "Show the folder in the file manager" })
                .disabled(remote || folder.is_none())
                .on_press(move |_: &mut App| {
                    if let Some(p) = &folder {
                        reveal_path(p);
                    }
                })
                .build(k)
                .key("reveal"),
        );
    if s.can_copy() {
        left = left.child(
            small("Copy to…")
                .help("Copy this skill into another scope or machine")
                .on_press(move |s: &mut App| s.open_copy(vec![copy.clone()]))
                .build(k)
                .key("copy"),
        );
    }
    row().w_full().gap(8.0).center_y().child(left).child(row().grow(1.0)).child(
        Button::new("Remove…")
            .variant(ButtonVariant::Danger)
            .control_size(ControlSize::Small)
            .on_press(move |s: &mut App| s.confirm_remove = Some(vec![remove.clone()]))
            .build(k)
            .key("remove"),
    )
}

/// `.agents`, the sync targets and any agent that has the skill; the rest fold away.
fn agents(s: &App, skill: &Skill, k: &Kit) -> El {
    let targets = s.targets();
    let (shown, hidden): (Vec<usize>, Vec<usize>) =
        (0..AGENTS.len()).partition(|&a| a == CANONICAL || targets.contains(&a) || skill.placements[a] != Placement::Missing);
    let mut agents = col().key("placements").w_full().gap(10.0).child(section("Agents", None, k));
    for &a in &shown {
        agents = agents.child(placement_row(s, skill, a, k));
    }
    if hidden.is_empty() {
        return agents;
    }
    if s.show_all_agents {
        for &a in &hidden {
            agents = agents.child(placement_row(s, skill, a, k));
        }
    }
    let label = if s.show_all_agents { "Show fewer agents".to_string() } else { format!("Show {} more agents", hidden.len()) };
    // A text control, so it lines up with the agent names above it.
    agents.child(
        row()
            .key("more-agents")
            .role("button")
            .label(label.clone())
            .focusable(true)
            .cursor(rune::Cursor::Pointer)
            .on_press(|s: &mut App| s.show_all_agents = !s.show_all_agents)
            .child(text(label).font(k.ty.small_medium).color(k.c.text_muted)),
    )
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
    let mut title = row().gap(6.0).center_y().child(text(agent.name).font(k.ty.ui).color(k.c.text_base));
    if let Some(c) = placement_color(p, k) {
        title = title.child(dot(c));
    }
    let label = col()
        .gap(1.0)
        .grow(1.0)
        .min_w(0.0)
        .child(title)
        .child(text(format!("{} · {status}", agent.dir_label(&s.inv.scope, &s.paths))).font(k.ty.caption()).color(k.c.text_faint).truncate());

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
