//! The inspector for an installed skill: what it is, its actions, and where each agent has it.

use rune::{El, row};
use rune_kit::{Kit, glyphs};

use super::UPDATE;
use super::installed::placement_tone;
use crate::agents::{AGENTS, CANONICAL, tilde};
use crate::app::{App, open_path, reveal_path};
use crate::ds::{self, Card, Inspector, MenuAction, Notice, Place, Tone, item_row, plain_row, size, space, text};
use crate::scan::{Placement, Skill};
use crate::skill::manifest_path;

pub fn installed(s: &App, k: &Kit, layer: &mut Vec<El>) -> El {
    let Some(skill) = s.selected.as_deref().and_then(|n| s.inv.get(n)) else {
        return Inspector::empty(k, "No skill selected", "Click a skill, or move through the list with ↑ and ↓.");
    };
    let mut ins = Inspector::new().title(skill.name.clone());
    if !skill.description.is_empty() {
        ins = ins.description(skill.description.clone());
    }
    let (left, right) = actions(s, skill, k, layer);
    ins = ins.actions(left, Some(right));
    if s.has_update(&skill.name) {
        ins = ins.notice(update_notice(s, &skill.name, k));
    }
    let folder = skill.source.as_ref().and_then(|src| Card::new("folder").label("Folder").row(text::path(k, tilde(src, &s.paths))).build(k));
    let md = s.preview.as_ref().and_then(|md| Card::new("skill-md-card").label("SKILL.md").row(ds::markdown::<App>(k, md.clone()).key("skill-md")).build(k));
    ins.card(folder).card(agents(s, skill, k)).card(md).build(k)
}

fn update_notice(s: &App, name: &str, k: &Kit) -> El {
    let target = name.to_string();
    let updating = s.updating.is_some();
    // Not `primary`: the toolbar's Sync is the view's next step.
    let update = ds::button(if updating { "Updating…" } else { "Update" }, Place::Inspector)
        .icon(&UPDATE)
        .disabled(updating)
        .on_press(move |s: &mut App| s.update(vec![target.clone()]))
        .build(k)
        .key("update");
    Notice::new(Tone::Info, "A newer version is on skills.sh").action(update).build(k).key("update-banner")
}

/// Whether the action row fits on one line in the narrowest inspector with Open SKILL.md
/// visible (Copy to… when `copy`), measured as the kit lays the buttons out.
fn row_fits(k: &Kit, copy: bool) -> bool {
    let place = Place::Inspector;
    let mut buttons = vec![ds::button_width(k, "Open SKILL.md", place, false)];
    if copy {
        buttons.push(ds::button_width(k, "Copy to…", place, false));
    }
    // The overflow button, then Remove… pinned right; one gap before each control after the first.
    let width = buttons.iter().sum::<f32>() + place.height() + ds::button_width(k, "Remove…", place, false) + (buttons.len() + 1) as f32 * space::S2;
    width <= size::INSPECTOR_MIN - 2.0 * size::PANE_EDGE
}

/// Open SKILL.md and Copy to… on the left, then "…" with Reveal; Remove… on the right. When
/// the row would not fit the narrowest inspector, Open SKILL.md goes into "…" after Reveal.
fn actions(s: &App, skill: &Skill, k: &Kit, layer: &mut Vec<El>) -> (Vec<El>, El) {
    // A host's files aren't on this machine: nothing here opens or reveals them.
    let remote = s.is_remote();
    // Looking for the file reads the disk: a host's folder isn't on this machine.
    let manifest = if remote { None } else { skill.source.as_deref().and_then(manifest_path) };
    let folder = skill.source.clone();
    let can_copy = s.can_copy();
    let open_visible = row_fits(k, can_copy);
    let open_disabled = remote || manifest.is_none();
    let open = move |_: &mut App| {
        if let Some(p) = &manifest {
            open_path(p);
        }
    };

    let mut items = vec![
        MenuAction::new("Reveal", move |_: &mut App| {
            if let Some(p) = &folder {
                reveal_path(p);
            }
        })
        .description(if cfg!(target_os = "macos") { "Show the folder in Finder" } else { "Show the folder in the file manager" })
        .disabled(remote || skill.source.is_none()),
    ];
    let mut left = Vec::new();
    if open_visible {
        left.push(ds::button("Open SKILL.md", Place::Inspector).disabled(open_disabled).on_press(open).build(k).key("open-md"));
    } else {
        items.push(MenuAction::new("Open SKILL.md", open).disabled(open_disabled));
    }
    if can_copy {
        let copy = skill.name.clone();
        left.push(
            ds::button("Copy to…", Place::Inspector)
                .help("Copy this skill into another scope or machine")
                .on_press(move |s: &mut App| s.open_copy(vec![copy.clone()]))
                .build(k)
                .key("copy"),
        );
    }
    left.push(ds::overflow::<App>(k, "skill-more", Place::Inspector, s.skill_menu, |s: &mut App| s.skill_menu = !s.skill_menu, |s: &mut App| s.skill_menu = false, items, layer));
    let remove = skill.name.clone();
    let remove = ds::danger("Remove…", Place::Inspector).on_press(move |s: &mut App| s.confirm_remove = Some(vec![remove.clone()])).build(k).key("remove");
    (left, remove)
}

/// `.agents`, the sync targets and any agent that has the skill; the rest fold away.
fn agents(s: &App, skill: &Skill, k: &Kit) -> Option<El> {
    let targets = s.targets();
    let (shown, hidden): (Vec<usize>, Vec<usize>) =
        (0..AGENTS.len()).partition(|&a| a == CANONICAL || targets.contains(&a) || skill.placements[a] != Placement::Missing);
    let mut card = Card::new("placements").label("Agents").rows(shown.iter().map(|&a| placement_row(s, skill, a, k)));
    if hidden.is_empty() {
        return card.build(k);
    }
    if s.show_all_agents {
        card = card.rows(hidden.iter().map(|&a| placement_row(s, skill, a, k)));
    }
    let label = if s.show_all_agents { "Show fewer agents".to_string() } else { format!("Show {} more agents", hidden.len()) };
    let more = ds::quiet(label, Place::Inspector).on_press(|s: &mut App| s.show_all_agents = !s.show_all_agents).build(k).key("more-agents");
    card.row(plain_row(row().child(more))).build(k)
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
    let mut name = row().gap(space::S2).center_y().min_w(0.0).child(text::name(k, agent.name).shrink());
    if let Some(tone) = placement_tone(p) {
        name = name.child(ds::dot(k, tone));
    }
    let hint = text::hint(k, format!("{} · {status}", agent.dir_label(&s.inv.scope, &s.paths)));

    let skill_name = skill.name.clone();
    let control: Option<El> = if a == CANONICAL {
        if p.present() || matches!(p, Placement::Broken(_)) {
            None
        } else {
            Some(
                ds::button("Move into .agents", Place::Inspector)
                    .help("Move the agent's copy into .agents and leave a link behind")
                    .on_press(move |s: &mut App| s.replace_with_link(&skill_name, CANONICAL))
                    .build(k)
                    .key("adopt"),
            )
        }
    } else if shared {
        None
    } else if *p == Placement::Dir {
        let other = skill_name.clone();
        Some(
            row()
                .gap(space::S1)
                .center_y()
                .child(
                    ds::button("Use link", Place::Inspector)
                        .help("Replace this copy with a link to .agents; the copy goes to the trash")
                        .on_press(move |s: &mut App| s.replace_with_link(&skill_name, a))
                        .build(k)
                        .key(format!("use-link-{}", agent.id)),
                )
                .child(
                    ds::icon_button(&glyphs::CLOSE, "Move this copy to the trash", Place::Inspector)
                        .on_press(move |s: &mut App| s.trash_copy(&other, a))
                        .build(k)
                        .key(format!("trash-{}", agent.id)),
                ),
        )
    } else {
        Some(
            ds::switch(k, matches!(p, Placement::Linked | Placement::Link(_)))
                .label(format!("Link {} into {}", skill.name, agent.name))
                .hide_label()
                .on_toggle(move |s: &mut App| s.toggle_link(&skill_name, a))
                .build(k)
                .key(format!("link-{}", agent.id)),
        )
    };
    item_row(name, Some(hint), control).key(format!("agent-{}", agent.id))
}
