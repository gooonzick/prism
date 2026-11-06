//! The skills installed in a scope: the toolbar, the filters, the list and its bulk bar.

use rune::{Color, El, Tx, col, row, text};
use rune_kit::shell::Toolbar;
use rune_kit::{Button, ButtonVariant, Checkbox, ControlSize, IconButton, IconButtonVariant, Kit, SegmentedControl, TextInput, glyphs};

use super::{PLUS, REFRESH, SYNC, UPDATE, dot, empty, inspector, scroll_pane};
use crate::agents::{AGENTS, Scope, tilde};
use crate::app::{App, Filter};
use crate::scan::{Placement, Skill};

const FILTERS: [Filter; 4] = [Filter::All, Filter::NeedsSync, Filter::Broken, Filter::Updates];

pub fn page(s: &App, k: &Kit, _layer: &mut Vec<El>) -> (El, Toolbar, El) {
    let targets = s.targets();
    let visible = s.visible();
    let unsynced = s.filter_count(Filter::NeedsSync);
    (main(s, &visible, &targets, k), toolbar(s, unsynced, k), inspector::installed(s, k))
}

fn toolbar(s: &App, unsynced: usize, k: &Kit) -> Toolbar {
    let title = match &s.scope {
        Scope::Global => "Global skills".to_string(),
        Scope::Project(p) => p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()),
    };
    let checking = s.checking.contains(&s.scope);
    let check_label = if checking { "Checking for updates…" } else { "Check skills.sh for updates" };
    Toolbar::new().title(title).trailing(
        row()
            .gap(6.0)
            .center_y()
            .child(
                IconButton::new(&PLUS)
                    .variant(IconButtonVariant::GhostMuted)
                    .label("New skill")
                    .help("New skill")
                    .shortcut("⌘N")
                    .on_press(|s: &mut App| s.open_new_skill())
                    .build(k)
                    .key("new-skill"),
            )
            .child(
                IconButton::new(&UPDATE)
                    .variant(IconButtonVariant::GhostMuted)
                    .label(check_label)
                    .help(check_label)
                    .disabled(checking)
                    .on_press(|s: &mut App| s.check_updates())
                    .build(k)
                    .key("check-updates"),
            )
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
    )
}

fn main(s: &App, visible: &[usize], targets: &[usize], k: &Kit) -> El {
    let filter = TextInput::new(&s.filter)
        .placeholder("Filter skills")
        .leading_icon(&glyphs::SEARCH)
        .clear_button()
        .label("Filter skills")
        .width(None)
        .on_edit(|s: &mut App| &mut s.filter)
        .build(k)
        .key("filter");
    let mut list = col().key("skills").w_full().gap(2.0);
    if s.inv.skills.is_empty() {
        list = list.child(empty("No skills here yet", "Install one from skills.sh, or make one with New skill (⌘N).", k));
    } else if visible.is_empty() {
        let hint = match s.status_filter {
            Filter::All => "Try another word.",
            Filter::NeedsSync => "Every skill is linked into your sync targets.",
            Filter::Broken => "No broken links here.",
            Filter::Updates if s.checking.contains(&s.scope) => "Still checking skills.sh…",
            Filter::Updates => "Everything is up to date.",
        };
        list = list.child(empty("Nothing to show", hint, k));
    }
    for &i in visible {
        list = list.child(skill_row(s, &s.inv.skills[i], targets, k));
    }
    col()
        .key("installed")
        .w_full()
        .h_full()
        .min_h(0.0)
        .padding(16.0)
        .gap(10.0)
        .child(filters(s, filter, k))
        .child(list_header(s, visible, k))
        .child(scroll_pane("skills-scroll", list).grow(1.0))
}

fn filter_label(s: &App, f: Filter) -> String {
    let n = s.filter_count(f);
    match f {
        Filter::All => format!("All {n}"),
        Filter::NeedsSync => format!("Unsynced {n}"),
        Filter::Broken => format!("Broken {n}"),
        Filter::Updates => format!("Updates {n}"),
    }
}

/// The text filter, then the status filters under it.
fn filters(s: &App, filter: El, k: &Kit) -> El {
    let labels: Vec<String> = FILTERS.iter().map(|&f| filter_label(s, f)).collect();
    let current = FILTERS.iter().position(|&f| f == s.status_filter);
    let segments = SegmentedControl::new(&labels, current)
        .label("Show")
        .fit()
        .on_pick(|s: &mut App, i: usize, _: &mut Tx| s.set_status_filter(FILTERS[i]))
        .build(k)
        .key("status-filter");
    // The search on its own line keeps both readable however narrow the pane gets.
    col().w_full().gap(10.0).child(filter).child(row().w_full().child(segments))
}

/// Above the list: a tick-all box, then either the scope's summary or, with skills ticked,
/// what can be done to them all.
fn list_header(s: &App, visible: &[usize], k: &Kit) -> El {
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
        let total = s.inv.skills.len();
        let root = tilde(&s.inv.scope.root(&s.paths), &s.paths);
        let summary = format!("{total} skill{} in {root} · ⌘- or ⇧-click to pick several", if total == 1 { "" } else { "s" });
        let mut r = row().grow(1.0).min_w(0.0).gap(12.0).center_y().child(text(summary).font(k.ty.small).color(k.c.text_faint).grow(1.0).min_w(0.0).truncate());
        let broken = crate::sync::broken_links(&s.inv).len();
        if broken > 0 {
            r = r.child(
                Button::new(format!("Clean up {broken} broken link{}", if broken == 1 { "" } else { "s" }))
                    .variant(ButtonVariant::GhostMuted)
                    .control_size(ControlSize::Small)
                    .help("Delete links whose target is gone (Undo brings them back)")
                    .on_press(|s: &mut App| s.remove_broken())
                    .build(k)
                    .key("clean-broken"),
            );
        }
        r
    } else {
        bulk_bar(s, n, k)
    };
    row().w_full().h(36.0).px(12.0).gap(12.0).center_y().child(all).child(middle)
}

fn bulk_bar(s: &App, n: usize, k: &Kit) -> El {
    let with_updates: Vec<String> = s.checked.iter().filter(|x| s.has_update(x)).cloned().collect();
    let mut bar = row()
        .key("bulk-bar")
        .grow(1.0)
        .min_w(0.0)
        .gap(8.0)
        .center_y()
        .child(
            row()
                .key("bulk-clear")
                .role("button")
                .label("Clear selection")
                .help("Clear selection (Esc)")
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .gap(4.0)
                .center_y()
                .on_press(|s: &mut App| s.clear_checked())
                .child(text(format!("{n} selected")).font(k.ty.small_medium).color(k.c.text_base))
                .child(rune::icon(&glyphs::CLOSE, 12.0).color(k.c.icon_muted)),
        )
        .child(
            Button::new("Sync")
                .icon(&SYNC)
                .variant(ButtonVariant::Contrast)
                .control_size(ControlSize::Small)
                .help("Sync only the selected skills")
                .on_press(|s: &mut App| s.open_sync_checked())
                .build(k)
                .key("bulk-sync"),
        );
    if !with_updates.is_empty() {
        let count = with_updates.len();
        bar = bar.child(
            Button::new(if count == n { "Update".to_string() } else { format!("Update {count}") })
                .icon(&UPDATE)
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .disabled(s.updating.is_some())
                .on_press(move |s: &mut App| s.update(with_updates.clone()))
                .build(k)
                .key("bulk-update"),
        );
    }
    if !s.other_scopes().is_empty() {
        bar = bar.child(
            Button::new("Copy to…")
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .help("Copy the selected skills into another scope")
                .on_press(|s: &mut App| s.open_copy(s.checked.clone()))
                .build(k)
                .key("bulk-copy"),
        );
    }
    bar.child(row().grow(1.0)).child(
        Button::new("Remove…")
            .variant(ButtonVariant::Danger)
            .control_size(ControlSize::Small)
            .on_press(|s: &mut App| s.confirm_remove_checked())
            .build(k)
            .key("bulk-remove"),
    )
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
    if s.has_update(&skill.name) {
        chips = chips.child(tag("update", Some(k.c.info.icon), "A newer version is on skills.sh", k).key("update-tag"));
    }
    if !s.scope.is_global() && s.global_names.contains(&skill.name) {
        chips = chips.child(tag("also global", None, "A global skill has the same name; agents see both", k).key("shadow-tag"));
    }
    for a in shown_agents(skill) {
        let p = &skill.placements[a];
        let help = format!("{}: {}", AGENTS[a].short, if s.inv.shares_canonical[a] { "same folder as .agents" } else { p.label() });
        chips = chips.child(tag(AGENTS[a].short, placement_color(p, k), &help, k));
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

pub fn placement_color(p: &Placement, k: &Kit) -> Option<Color> {
    match p {
        Placement::Linked => Some(k.c.success.icon),
        Placement::Dir => Some(k.c.info.icon),
        Placement::Link(_) => Some(k.c.warning.icon),
        Placement::Broken(_) => Some(k.c.danger.icon),
        Placement::Missing => None,
    }
}

/// A quiet label, with a dot when it has a state to show.
fn tag(label: &str, color: Option<Color>, help: &str, k: &Kit) -> El {
    let mut c = row().h(20.0).px(6.0).gap(4.0).center_y().radius(k.radius.sm).border(1.0, k.c.border_weak).help(help.to_string());
    if let Some(color) = color {
        c = c.child(dot(color));
    }
    c.child(text(label.to_string()).font(k.ty.small).color(k.c.text_muted))
}
