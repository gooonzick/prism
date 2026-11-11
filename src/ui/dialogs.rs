//! Modal dialogs: the sync preview (this machine's plan, or a host's), the remove confirmation,
//! Copy to…, New skill and the projects found in a folder. All of them sit in the `ds` dialog
//! frame; picks and fields go in cards, plans are `Plan`s.

use rune::{El, Tx, col, icon, row};
use rune_kit::{ButtonVariant, Checkbox, Kit, TextArea, TextInput, glyphs};

use crate::agents::{AGENTS, CANONICAL};
use crate::app::{App, Planned, Target};
use crate::ds::{self, Card, DialogFrame, DialogKind, Notice, Place, Plan, Tone, color, field, item_row, loading, pressable, size, space, text};
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

/// Cancel in a dialog's footer.
fn cancel(key: &str, k: &Kit, on: impl Fn(&mut App) + 'static) -> El {
    ds::button("Cancel", Place::Bar).on_press(on).build(k).key(key.to_string())
}

/// The apply button of a plan: loading while it is applied, disabled when there is nothing to do.
fn apply_button(applying: bool, n: usize) -> rune_kit::Button {
    match (applying, n) {
        (true, _) => ds::primary("Applying…", Place::Bar).variant(ButtonVariant::Loading).disabled(true),
        (false, 0) => ds::primary("Nothing to do", Place::Bar).disabled(true),
        (false, n) => ds::primary(format!("Apply {n} change{}", if n == 1 { "" } else { "s" }), Place::Bar),
    }
}

/// The plan's skipped skills, each as "skill: why".
fn skipped(view: &PlanView) -> Vec<String> {
    view.skipped.iter().map(|(skill, reason)| format!("{skill}: {reason}")).collect()
}

pub fn sync(s: &App, k: &Kit, layer: &mut Vec<El>) {
    if !s.sync_open() {
        return;
    }
    let targets = s.targets();
    let mut agents = Card::new("sync-targets").label("Link into");
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
        agents = agents.row(cb.build(k).key(format!("target-{}", agent.id)));
    }

    let mut plan = Plan::new("sync");
    // `settled`: the plan is in, so an empty one means there is nothing to do.
    let (view, applying, settled) = match (&s.sync_plan, &s.sync_host) {
        (Some(p), _) => {
            let linkable = targets.iter().filter(|&&t| !s.inv.shares_canonical[t]).count();
            let view = PlanView::from((p, &s.paths));
            let groups = group_actions(&p.actions, linkable);
            plan = plan.heading(changes_heading(view.lines.len(), groups.len()));
            for (skill, parts) in groups {
                plan = plan.group(skill, parts);
            }
            (view, false, true)
        }
        (None, Some(host)) => {
            // A host's plan comes as lines, not actions to group: they show as the host words them.
            if let Some(notice) = &host.notice {
                plan = plan.notice(Notice::new(Tone::Warn, notice.clone()).build(k).key("sync-notice"));
            }
            let view = match &host.view {
                None => {
                    plan = plan.waiting(format!("Asking {} for the plan…", s.env_label()));
                    PlanView::default()
                }
                Some(Err(e)) => {
                    plan = plan.failed(e.clone());
                    PlanView::default()
                }
                Some(Ok(view)) => view.clone(),
            };
            for line in &view.lines {
                plan = plan.line(line.clone());
            }
            let n = view.lines.len();
            plan = plan.heading(if n == 0 { "Changes".to_string() } else { format!("{n} change{}", if n == 1 { "" } else { "s" }) });
            (view, host.applying, matches!(host.view, Some(Ok(_))))
        }
        (None, None) => return,
    };
    if settled && view.lines.is_empty() && view.skipped.is_empty() {
        plan = plan.note("Everything is in sync.");
    }
    let plan = plan.skipped(skipped(&view));

    let apply = apply_button(applying, view.lines.len()).on_press(|s: &mut App| s.apply_sync()).build(k).key("sync-apply");
    let picks = col().w(size::DIALOG_PICKS).children(agents.build(k));
    let changes = col().grow(1.0).min_w(0.0).gap(space::S4).children(plan.build(k));
    let title = match s.sync_only.as_deref() {
        Some([one]) => format!("Sync {one}"),
        Some(names) => format!("Sync {} selected skills", names.len()),
        None => format!("Sync {}", s.scope_label(&s.inv.scope)),
    };
    DialogFrame::new("sync", title, DialogKind::Wide)
        .body(text::body(k, SYNC_NOTE))
        .body(row().w_full().gap(space::S4).items_start().child(picks).child(changes))
        .footer(cancel("sync-cancel", k, |s| s.close_sync()), apply)
        .build(k, |s: &mut App| s.close_sync(), layer);
}

pub fn remove(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(names) = &s.confirm_remove else { return };
    let skills: Vec<_> = names.iter().filter_map(|n| s.inv.get(n)).collect();
    let mut scope = s.scope_label(&s.inv.scope);
    if let Some(host) = s.host() {
        scope.push_str(&format!(" on {}", host.name));
    }
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
    let confirm = ds::danger(if targets.len() == 1 { "Remove".to_string() } else { format!("Remove {}", targets.len()) }, Place::Bar)
        .on_press(move |s: &mut App| s.remove(&targets))
        .build(k)
        .key("remove-confirm");
    DialogFrame::new("remove", title, DialogKind::Confirm)
        .body(text::body(k, message))
        .footer(cancel("remove-cancel", k, |s| s.confirm_remove = None), confirm)
        .build(k, |s: &mut App| s.confirm_remove = None, layer);
}

const SYNC_NOTE: &str = "Skills live once in .agents/skills and every agent gets a link. A skill only one agent has moves into .agents first. Folders that differ are skipped, and replaced copies go to the trash.";

/// One place a copy can go, a card row: its icon, its name and what is there now.
fn scope_row(key: String, label: String, global: bool, detail: String, k: &Kit, reduced: bool, pick: impl Fn(&mut App) + 'static) -> El {
    let mark = icon(if global { &super::HOME } else { &glyphs::FOLDER }, size::ICON).color(color::text_muted(k));
    let line = row().w_full().gap(space::S3).center_y().child(mark).child(item_row(text::name(k, label.clone()), Some(text::hint(k, detail)), None).grow(1.0).min_w(0.0));
    pressable(k, line, label, reduced, pick).key(key)
}

/// "N skills there", or the picked skills it already has, which a copy skips.
fn scope_detail(total: usize, clashes: &[&String]) -> String {
    if clashes.is_empty() {
        format!("{total} skill{} there", if total == 1 { "" } else { "s" })
    } else {
        let which = if clashes.len() == 1 { "that one is" } else { "those are" };
        format!("already has {}; {which} skipped", clashes.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", "))
    }
}

/// Where to copy the picked skills: a card of the other scopes of the machine in view, then a
/// card per other machine with its Global and projects, each labelled with the machine's name.
pub fn copy(s: &App, k: &Kit, layer: &mut Vec<El>, reduced: bool) {
    let Some(names) = &s.copy_dialog else { return };
    let what = match names.as_slice() {
        [one] => one.clone(),
        many => format!("{} skills", many.len()),
    };
    if let Some(target) = &s.across.skills {
        return copy_plan(s, &what, target, k, layer);
    }
    let machines = &s.across.machines;
    let mut here = Card::new("copy-scopes");
    let scopes = s.other_scopes();
    if !machines.is_empty() && !scopes.is_empty() {
        here = here.label(s.env_label());
    }
    for (i, scope) in scopes.into_iter().enumerate() {
        let stats = s.stats.get(&scope).copied().unwrap_or_default();
        let clashes: Vec<&String> = names.iter().filter(|n| s.skill_in(&scope, n)).collect();
        let dest = scope.clone();
        here = here.row(scope_row(format!("copy-to-{i}"), s.scope_label(&scope), scope.is_global(), scope_detail(stats.total, &clashes), k, reduced, move |s: &mut App| s.copy_to(dest.clone())));
    }
    let mut frame = DialogFrame::new("copy", format!("Copy {what} to…"), DialogKind::Confirm).body(text::body(k, "It lands in that scope's .agents/skills and is linked into your sync targets there."));
    if let Some(card) = here.build(k) {
        frame = frame.body(card);
    }
    for m in machines {
        let mut card = Card::new(format!("copy-scopes-{}", m.key())).label(m.label.clone());
        match &m.view {
            None => card = card.row(loading(k, format!("Asking {} for its projects…", m.label))),
            Some(Err(e)) => card = card.row(Notice::new(Tone::Bad, e.clone()).build(k)),
            Some(Ok(view)) => {
                for (i, scope) in view.scopes().into_iter().enumerate() {
                    let total = view.stats.get(&scope).map_or(0, |st| st.total);
                    // Its Global's skills are known; a project's would take another look there.
                    let clashes: Vec<&String> = if scope.is_global() { names.iter().filter(|n| view.global_names.contains(*n)).collect() } else { Vec::new() };
                    let (label, machine) = (m.label.clone(), view.label(&scope));
                    let dest = scope.clone();
                    card = card.row(scope_row(format!("copy-to-{}-{i}", m.key()), machine, scope.is_global(), scope_detail(total, &clashes), k, reduced, move |s: &mut App| s.copy_to_machine(&label, dest.clone())));
                }
            }
        }
        if let Some(card) = card.build(k) {
            frame = frame.body(card);
        }
    }
    frame.footer_cancel(cancel("copy-cancel", k, |s| s.close_copy())).build(k, |s: &mut App| s.close_copy(), layer);
}

/// The other machine's plan for the pick, then Apply: lines, the skills it skips, and why.
fn copy_plan(s: &App, what: &str, t: &Target, k: &Kit, layer: &mut Vec<El>) {
    let dest = s.machine(&t.machine).and_then(|m| m.view.as_ref()).and_then(|v| v.as_ref().ok()).map_or_else(|| s.scope_label(&t.dest), |v| v.label(&t.dest));
    let mut plan = Plan::new("copy");
    if let Some(notice) = &t.notice {
        plan = plan.notice(Notice::new(Tone::Warn, notice.clone()).build(k).key("copy-notice"));
    }
    let view = match &t.plan {
        Planned::Asking => {
            plan = plan.waiting(format!("Asking {} for the plan…", t.machine));
            PlanView::default()
        }
        Planned::Waiting(why) => {
            plan = plan.note(why.clone());
            PlanView::default()
        }
        Planned::Done(Err(e)) => {
            plan = plan.failed(e.clone());
            PlanView::default()
        }
        Planned::Done(Ok(view)) => {
            if view.lines.is_empty() {
                plan = plan.note("Nothing to copy.");
            }
            view.clone()
        }
    };
    for line in &view.lines {
        plan = plan.line(line.clone());
    }
    let plan = plan.skipped(skipped(&view));
    let apply = apply_button(t.applying, view.lines.len()).on_press(|s: &mut App| s.apply_copy_across()).build(k).key("copy-apply");
    let what_there = format!("It lands in .agents/skills there and is linked into the sync targets of {0}; a skill {0} already has is skipped, never overwritten.", t.machine);
    let mut frame = DialogFrame::new("copy", format!("Copy {what} to {dest} on {}", t.machine), DialogKind::Form).body(text::body(k, what_there));
    for block in plan.build(k) {
        frame = frame.body(block);
    }
    frame.footer(cancel("copy-cancel", k, |s| s.close_copy()), apply).build(k, |s: &mut App| s.close_copy(), layer);
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
    let dir = crate::agents::tilde(&crate::agents::AGENTS[CANONICAL].dir(&s.scope, &s.paths), &s.paths);
    // A host's file opens in no editor here.
    let what = match s.host() {
        Some(h) => format!("Creates {dir}/<name>/SKILL.md on {} and links it into the sync targets there.", h.name),
        None => format!("Creates {dir}/<name>/SKILL.md, links it into your sync targets and opens it in your editor."),
    };
    let fields = Card::new("new-skill-fields").row(field(k, "Name", name)).row(field(k, "When to use it", description));
    let create = ds::primary("Create skill", Place::Bar).on_press(|s: &mut App| s.create_skill()).build(k).key("new-create");
    DialogFrame::new("new-skill", format!("New skill in {}", s.scope_label(&s.scope)), DialogKind::Form)
        .body(text::body(k, what))
        .body(fields.build(k).expect("two fields"))
        .error(form.error.clone())
        .footer(cancel("new-cancel", k, |s| s.new_skill = None), create)
        .build(k, |s: &mut App| s.new_skill = None, layer);
}

/// Projects found under a folder, ticked to add.
pub fn found(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(found) = &s.found else { return };
    let mut list = Card::new("found-list");
    for (i, (path, n, on)) in found.projects.iter().enumerate() {
        let parent = path.parent().map(|p| crate::agents::tilde(p, &s.paths)).unwrap_or_default();
        let title = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        list = list.row(
            Checkbox::new(*on)
                .label(title)
                .description(format!("{parent} · {n} skill{}", if *n == 1 { "" } else { "s" }))
                .on_toggle(move |s: &mut App| s.toggle_found(i))
                .build(k)
                .key(format!("found-{i}")),
        );
    }
    let picked = found.projects.iter().filter(|p| p.2).count();
    let add = ds::primary(if picked == 1 { "Add 1 project".to_string() } else { format!("Add {picked} projects") }, Place::Bar)
        .disabled(picked == 0)
        .on_press(|s: &mut App| s.add_found())
        .build(k)
        .key("found-add");
    let mut frame = DialogFrame::new("found", format!("Projects with skills in {}", crate::agents::tilde(&found.root, &s.paths)), DialogKind::Form)
        .body(text::body(k, "That folder has no skills of its own; pick the projects inside it to add."));
    if let Some(card) = list.build(k) {
        frame = frame.body(card);
    }
    frame.footer(cancel("found-cancel", k, |s| s.found = None), add).build(k, |s: &mut App| s.found = None, layer);
}
