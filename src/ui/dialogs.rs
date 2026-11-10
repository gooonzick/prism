//! Modal dialogs: the sync preview and the remove confirmation.

use rune::{El, ScrollSpec, Tx, col, icon, row, text};
use rune_kit::{Button, ButtonVariant, Checkbox, Dialog, Kit, TextArea, TextInput, glyphs};

use super::heading;
use crate::agents::{AGENTS, CANONICAL};
use crate::app::App;
use crate::plan_view::PlanView;
use crate::sync::Action;

/// The plan's actions per skill, in order, each as a short phrase; links to several agents
/// collapse into one phrase.
fn group_actions(actions: &[Action], targets: usize) -> Vec<(String, Vec<String>)> {
    let mut groups: Vec<(String, Vec<String>, Vec<&str>)> = Vec::new();
    for a in actions {
        let (skill, phrase, link) = match a {
            Action::Adopt { skill, from } => (skill, Some(format!("move into .agents from {}", AGENTS[*from].name)), None),
            Action::AdoptLink { skill, from } => (skill, Some(format!("point .agents where {} links", AGENTS[*from].name)), None),
            Action::Link { skill, agent } => (skill, None, Some(AGENTS[*agent].name)),
            Action::Replace { skill, agent } => (skill, Some(format!("replace {}'s copy with a link", AGENTS[*agent].name)), None),
        };
        if groups.last().is_none_or(|g| &g.0 != skill) {
            groups.push((skill.clone(), Vec::new(), Vec::new()));
        }
        let g = groups.last_mut().unwrap();
        g.1.extend(phrase);
        g.2.extend(link);
    }
    groups
        .into_iter()
        .map(|(skill, mut phrases, links)| {
            match links.len() {
                0 => {}
                n if n == targets && n > 2 => phrases.push(format!("link into all {n} agents")),
                _ => phrases.push(format!("link into {}", links.join(", "))),
            }
            (skill, phrases)
        })
        .collect()
}

fn changes_heading(changes: usize, skills: usize) -> String {
    match (changes, skills) {
        (0, _) => "Changes".into(),
        (c, 1) => format!("{c} change{} to 1 skill", if c == 1 { "" } else { "s" }),
        (c, n) => format!("{c} changes to {n} skills"),
    }
}

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
        } else if !s.detected(a) {
            cb = cb.label(format!("{} (not found)", agent.name));
        }
        agents = agents.child(cb.build(k).key(format!("target-{}", agent.id)));
    }

    let linkable = targets.iter().filter(|&&t| !s.inv.shares_canonical[t]).count();
    let view = PlanView::from((plan, &s.paths));
    let groups = group_actions(&plan.actions, linkable);
    let mut changes = col().key("sync-changes").gap(10.0).w_full();
    if view.lines.is_empty() && view.skipped.is_empty() {
        changes = changes.child(text("Everything is in sync.").font(k.ty.small).color(k.c.text_muted));
    }
    for (skill, parts) in &groups {
        changes = changes.child(
            col()
                .w_full()
                .gap(2.0)
                .child(text(skill.clone()).font(k.ty.small_medium).color(k.c.text_base))
                .child(text(parts.join(" · ")).font(k.ty.small).color(k.c.text_muted).wrap()),
        );
    }
    if !view.skipped.is_empty() {
        changes = changes.child(heading(&format!("Skipped ({})", view.skipped.len()), k));
        for (skill, reason) in &view.skipped {
            changes = changes.child(
                row()
                    .w_full()
                    .gap(8.0)
                    .items_start()
                    .child(icon(&glyphs::CIRCLE_X, 14.0).color(k.c.warning.icon))
                    .child(
                        col()
                            .grow(1.0)
                            .min_w(0.0)
                            .gap(2.0)
                            .child(text(skill.clone()).font(k.ty.small_medium).color(k.c.text_base))
                            .child(text(reason.clone()).font(k.ty.small).color(k.c.text_muted).wrap()),
                    ),
            );
        }
    }

    let n = view.lines.len();
    let buttons = row()
        .w_full()
        .gap(8.0)
        .justify_end()
        .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.close_sync()).build(k).key("sync-cancel"))
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
                .child(heading(&changes_heading(view.lines.len(), groups.len()), k))
                .child(col().w_full().h(260.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(changes)),
        );
    let title = match s.sync_only.as_deref() {
        Some([one]) => format!("Sync {one}"),
        Some(names) => format!("Sync {} selected skills", names.len()),
        None => format!("Sync {}", s.scope_label(&s.inv.scope)),
    };
    Dialog::new("sync-dialog", title)
        .width(760.0)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.close_sync())
        .child(dialog_body().child(note(SYNC_NOTE, k)).child(body).child(buttons))
        .build(k, layer);
}

pub fn remove(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(names) = &s.confirm_remove else { return };
    let skills: Vec<_> = names.iter().filter_map(|n| s.inv.get(n)).collect();
    let scope = s.scope_label(&s.inv.scope);
    let (title, message) = match skills.as_slice() {
        [] => return,
        [skill] => {
            let places: Vec<&str> = (0..AGENTS.len()).filter(|&a| skill.placements[a].present()).map(|a| AGENTS[a].name).collect();
            let from = if places.is_empty() { "every agent".to_string() } else { places.join(", ") };
            (format!("Remove {}?", skill.name), format!("Removes it from {from} in {scope}. Links are deleted; folders move to the trash."))
        }
        many => {
            const SHOWN: usize = 8;
            let mut list = many.iter().take(SHOWN).map(|x| x.name.as_str()).collect::<Vec<_>>().join(", ");
            if many.len() > SHOWN {
                list.push_str(&format!(" and {} more", many.len() - SHOWN));
            }
            (
                format!("Remove {} skills?", many.len()),
                format!("Removes {list} from every agent in {scope}. Links are deleted; folders move to the trash."),
            )
        }
    };
    let targets: Vec<String> = skills.iter().map(|x| x.name.clone()).collect();
    let buttons = row()
        .w_full()
        .gap(8.0)
        .justify_end()
        .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.confirm_remove = None).build(k).key("remove-cancel"))
        .child(
            Button::new(if targets.len() == 1 { "Remove".to_string() } else { format!("Remove {}", targets.len()) })
                .variant(ButtonVariant::Danger)
                .on_press(move |s: &mut App| s.remove(&targets))
                .build(k)
                .key("remove-confirm"),
        );
    Dialog::new("remove-dialog", title)
        .width(460.0)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.confirm_remove = None)
        .child(dialog_body().child(note(&message, k)).child(buttons))
        .build(k, layer);
}

const SYNC_NOTE: &str = "Skills live once in .agents/skills and every agent gets a link. A skill only one agent has moves into .agents first. Folders that differ are skipped, and replaced copies go to the trash.";

pub(super) fn dialog_body() -> El {
    col().w_full().px(16.0).pb(16.0).gap(16.0)
}

pub(super) fn note(t: &str, k: &Kit) -> El {
    text(t.to_string()).font(k.ty.ui).color(k.c.text_muted).wrap()
}

/// Where to copy the picked skills: one row per other scope.
pub fn copy(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(names) = &s.copy_dialog else { return };
    let what = match names.as_slice() {
        [one] => one.clone(),
        many => format!("{} skills", many.len()),
    };
    let mut scopes = col().key("copy-scopes").w_full().gap(4.0);
    for (i, scope) in s.other_scopes().into_iter().enumerate() {
        let stats = s.stats.get(&scope).copied().unwrap_or_default();
        let clashes: Vec<&String> = names.iter().filter(|n| s.skill_in(&scope, n)).collect();
        let detail = if clashes.is_empty() {
            format!("{} skill{} there", stats.total, if stats.total == 1 { "" } else { "s" })
        } else {
            format!("already has {}; that one is skipped", clashes.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", "))
        };
        let dest = scope.clone();
        scopes = scopes.child(
            row()
                .key(format!("copy-to-{i}"))
                .role("button")
                .label(s.scope_label(&scope))
                .focusable(true)
                .w_full()
                .px(12.0)
                .py(8.0)
                .gap(10.0)
                .center_y()
                .radius(k.radius.md)
                .hover(k.c.hover)
                .on_press(move |s: &mut App| s.copy_to(dest.clone()))
                .child(icon(if scope.is_global() { &super::HOME } else { &glyphs::FOLDER }, 16.0).color(k.c.icon_muted))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(text(s.scope_label(&scope)).font(k.ty.ui_medium).color(k.c.text_base).truncate())
                        .child(text(detail).font(k.ty.small).color(k.c.text_faint).truncate()),
                ),
        );
    }
    let buttons = row()
        .w_full()
        .justify_end()
        .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.copy_dialog = None).build(k).key("copy-cancel"));
    Dialog::new("copy-dialog", format!("Copy {what} to…"))
        .width(460.0)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.copy_dialog = None)
        .child(dialog_body().child(note("It lands in that scope's .agents/skills and is linked into your sync targets there.", k)).child(scopes).child(buttons))
        .build(k, layer);
}

/// A name and a description, written into a new SKILL.md.
pub fn new_skill(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(form) = &s.new_skill else { return };
    let name = TextInput::new(&form.name)
        .label("Name")
        .placeholder("pdf-tools")
        .width(None)
        .invalid(form.error.is_some())
        .on_edit(|s: &mut App| &mut s.new_skill.as_mut().expect("the dialog is open").name)
        .on_submit(|s: &mut App, _: &mut Tx| s.create_skill())
        .build(k)
        .key("new-name");
    let description = TextArea::new(&form.description)
        .label("When to use it")
        .placeholder("Use when the user works with PDF files: extracting text, filling forms, merging…")
        .width(None)
        .rows(3)
        .on_edit(|s: &mut App| &mut s.new_skill.as_mut().expect("the dialog is open").description)
        .build(k)
        .key("new-description");
    let mut body = dialog_body()
        .child(note(&format!("Creates {}/<name>/SKILL.md, links it into your sync targets and opens it in your editor.", crate::agents::tilde(&crate::agents::AGENTS[CANONICAL].dir(&s.scope, &s.paths), &s.paths)), k))
        .child(field_block("Name", name, k))
        .child(field_block("When to use it", description, k));
    if let Some(e) = &form.error {
        body = body.child(
            row()
                .gap(6.0)
                .center_y()
                .child(icon(&glyphs::CIRCLE_X, 14.0).color(k.c.danger.icon))
                .child(text(e.clone()).font(k.ty.small).color(k.c.danger.icon).wrap()),
        );
    }
    let buttons = row()
        .w_full()
        .gap(8.0)
        .justify_end()
        .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.new_skill = None).build(k).key("new-cancel"))
        .child(Button::new("Create skill").variant(ButtonVariant::Contrast).on_press(|s: &mut App| s.create_skill()).build(k).key("new-create"));
    Dialog::new("new-skill-dialog", format!("New skill in {}", s.scope_label(&s.scope)))
        .width(480.0)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.new_skill = None)
        .child(body.child(buttons))
        .build(k, layer);
}

pub(super) fn field_block(label: &str, input: El, k: &Kit) -> El {
    col().w_full().gap(6.0).child(text(label.to_string()).font(k.ty.small_medium).color(k.c.text_base)).child(input)
}

/// Projects found under a folder, ticked to add.
pub fn found(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(found) = &s.found else { return };
    let mut list = col().key("found-list").w_full().gap(6.0);
    for (i, (path, n, on)) in found.projects.iter().enumerate() {
        let parent = path.parent().map(|p| crate::agents::tilde(p, &s.paths)).unwrap_or_default();
        let title = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        list = list.child(
            Checkbox::new(*on)
                .label(title)
                .description(format!("{parent} · {n} skill{}", if *n == 1 { "" } else { "s" }))
                .on_toggle(move |s: &mut App| s.toggle_found(i))
                .build(k)
                .key(format!("found-{i}")),
        );
    }
    let picked = found.projects.iter().filter(|p| p.2).count();
    let buttons = row()
        .w_full()
        .gap(8.0)
        .justify_end()
        .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.found = None).build(k).key("found-cancel"))
        .child(
            Button::new(if picked == 1 { "Add 1 project".to_string() } else { format!("Add {picked} projects") })
                .variant(ButtonVariant::Contrast)
                .disabled(picked == 0)
                .on_press(|s: &mut App| s.add_found())
                .build(k)
                .key("found-add"),
        );
    Dialog::new("found-dialog", format!("Projects with skills in {}", crate::agents::tilde(&found.root, &s.paths)))
        .width(520.0)
        .fit()
        .open(true)
        .on_close(|s: &mut App| s.found = None)
        .child(
            dialog_body()
                .child(note("That folder has no skills of its own; pick the projects inside it to add.", k))
                .child(col().w_full().h(320.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(list))
                .child(buttons),
        )
        .build(k, layer);
}
