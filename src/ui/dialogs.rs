//! Modal dialogs: the sync preview and the remove confirmation.

use rune::{El, ScrollSpec, col, row, text};
use rune_kit::{Button, ButtonVariant, Checkbox, Dialog, Kit};

use super::{dot, heading};
use crate::agents::{AGENTS, CANONICAL};
use crate::app::App;

pub fn sync(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(plan) = &s.sync_plan else { return };
    let targets = s.targets();
    let mut agents = col().key("sync-targets").gap(6.0).child(heading("Link into", k));
    for a in (0..AGENTS.len()).filter(|&a| a != CANONICAL) {
        let agent = &AGENTS[a];
        let mut cb = Checkbox::new(targets.contains(&a))
            .label(agent.name)
            .description(agent.dir_label(&s.inv.scope, &s.paths))
            .on_toggle(move |s: &mut App| s.toggle_target(a));
        if s.inv.shares_canonical[a] {
            cb = cb.description("same folder as .agents").disabled(true);
        } else if !agent.detected(&s.paths) {
            cb = cb.label(format!("{} (not found)", agent.name));
        }
        agents = agents.child(cb.build(k).key(format!("target-{}", agent.id)));
    }

    let mut changes = col().key("sync-changes").gap(6.0).w_full();
    if plan.is_empty() {
        changes = changes.child(text("Everything is in sync.").font(k.ty.small).color(k.c.text_muted));
    }
    for a in &plan.actions {
        changes = changes.child(row().gap(8.0).center_y().child(dot(k.c.success.icon)).child(text(a.describe()).font(k.ty.small).color(k.c.text_base).wrap()));
    }
    for c in &plan.conflicts {
        changes = changes.child(
            row()
                .gap(8.0)
                .center_y()
                .child(dot(k.c.warning.icon))
                .child(text(format!("Skipped {}: {}", c.skill, c.reason)).font(k.ty.small).color(k.c.text_muted).wrap()),
        );
    }

    let n = plan.actions.len();
    let buttons = row()
        .w_full()
        .gap(8.0)
        .justify_end()
        .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.sync_plan = None).build(k).key("sync-cancel"))
        .child(
            Button::new(if n == 0 { "Nothing to do".to_string() } else { format!("Apply {n} change{}", if n == 1 { "" } else { "s" }) })
                .variant(ButtonVariant::Contrast)
                .disabled(n == 0)
                .on_press(|s: &mut App| s.apply_sync())
                .build(k)
                .key("sync-apply"),
        );
    let body = row()
        .w_full()
        .gap(20.0)
        .child(col().w(250.0).child(agents))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(6.0)
                .child(heading("Changes", k))
                .child(col().w_full().h(260.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(changes)),
        );
    Dialog::new("sync-dialog", format!("Sync {}", s.scope_label(&s.inv.scope)))
        .width(760.0)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.sync_plan = None)
        .child(dialog_body().child(note(SYNC_NOTE, k)).child(body).child(buttons))
        .build(k, layer);
}

pub fn remove(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(name) = &s.confirm_remove else { return };
    let Some(skill) = s.inv.get(name) else { return };
    let places: Vec<&str> = (0..AGENTS.len()).filter(|&a| skill.placements[a].present()).map(|a| AGENTS[a].name).collect();
    let target = name.clone();
    let message = format!(
        "Removes it from {} in {}. Links are deleted; folders move to the trash.",
        if places.is_empty() { "every agent".to_string() } else { places.join(", ") },
        s.scope_label(&s.inv.scope)
    );
    let buttons = row()
        .w_full()
        .gap(8.0)
        .justify_end()
        .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.confirm_remove = None).build(k).key("remove-cancel"))
        .child(Button::new("Remove").variant(ButtonVariant::Danger).on_press(move |s: &mut App| s.remove(&target)).build(k).key("remove-confirm"));
    Dialog::new("remove-dialog", format!("Remove {name}?"))
        .width(440.0)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.confirm_remove = None)
        .child(dialog_body().child(note(&message, k)).child(buttons))
        .build(k, layer);
}

const SYNC_NOTE: &str = "Skills live once in .agents/skills and every agent gets a link. A skill only one agent has moves into .agents first. Folders that differ are skipped, and replaced copies go to the trash.";

fn dialog_body() -> El {
    col().w_full().px(16.0).pb(16.0).gap(16.0)
}

fn note(t: &str, k: &Kit) -> El {
    text(t.to_string()).font(k.ty.ui).color(k.c.text_muted).wrap()
}
