//! The skills installed in a scope: the toolbar, the filters, the list and its bulk bar.

use rune::{El, Tx, row};
use rune_kit::Kit;
use rune_kit::shell::Toolbar;

use super::{PLUS, REFRESH, SYNC, UPDATE, inspector, scope_title};
use crate::agents::{AGENTS, tilde};
use crate::app::{App, Filter};
use crate::ds::{self, ListRow, MenuAction, Page, Place, Selection, Tone, size, space};
use crate::scan::{Placement, Skill};

const FILTERS: [Filter; 4] = [Filter::All, Filter::NeedsSync, Filter::Broken, Filter::Updates];

/// `room` is what the toolbar has beside its tabs (`ui::toolbar_room`); `None` before the first
/// layout.
pub fn page(s: &App, k: &Kit, layer: &mut Vec<El>, reduced: bool, room: Option<f32>) -> (El, Toolbar, El) {
    let targets = s.targets();
    let visible = s.visible();
    let unsynced = s.filter_count(Filter::NeedsSync);
    (main(s, &visible, &targets, k, reduced), toolbar(s, unsynced, room, k, layer), inspector::installed(s, k))
}

/// Tabs, the title, New skill, Check skills.sh for updates, Rescan and Sync. When the list pane
/// is too narrow for them, the title goes, the three icon buttons fold into "…" (⌘N and ⌘R still
/// reach them through `ui::on_key`) and Sync shows only its icon.
fn toolbar(s: &App, unsynced: usize, room: Option<f32>, k: &Kit, layer: &mut Vec<El>) -> Toolbar {
    let checking = s.checking.contains(&s.scope);
    // skills.sh is this machine's: a host's skills aren't checked against it.
    let remote = s.is_remote();
    let check_label = if checking { "Checking for updates…" } else { "Check skills.sh for updates" };
    let sync_label = match unsynced {
        0 => "Sync".to_string(),
        1 => "Sync 1 skill".to_string(),
        n => format!("Sync {n} skills"),
    };
    let title = scope_title(&s.scope, "skills");
    // The wide toolbar: the title, three icon buttons and Sync, `S2` apart.
    // The title counts at most `ROW_NAME_MIN`: past that it truncates, so a long project name
    // never folds the buttons into "…" while they fit beside a shortened title.
    let icons = 3.0 * (Place::Bar.height() + space::S2);
    let wide = space::S3 + ds::toolbar_title_width(k, &title).min(size::ROW_NAME_MIN) + icons + ds::button_width(k, &sync_label, Place::Bar, true);
    let trailing = row().gap(space::S2).center_y();
    if room.is_some_and(|room| room < wide) {
        let actions = vec![
            MenuAction::new("New skill", |s: &mut App| s.open_new_skill()),
            MenuAction::new(check_label, |s: &mut App| s.check_updates()).disabled(checking || remote),
            MenuAction::new("Rescan", |s: &mut App| s.rescan()),
        ];
        let more = ds::overflow(k, "toolbar-more", Place::Bar, s.toolbar_menu, |s: &mut App| s.toolbar_menu = !s.toolbar_menu, |s: &mut App| s.toolbar_menu = false, actions, layer);
        let sync = ds::icon_button(&SYNC, sync_label, Place::Bar).on_press(|s: &mut App| s.open_sync()).build(k).key("sync");
        return super::toolbar_untitled(s, k, trailing.child(more).child(sync));
    }
    // The one primary action, unless skills are ticked: then the bulk bar's is.
    let sync = if unsynced > 0 && s.checked.is_empty() { ds::primary(sync_label, Place::Bar) } else { ds::button(sync_label, Place::Bar) };
    super::toolbar(
        s,
        k,
        title,
        trailing
            .child(ds::icon_button(&PLUS, "New skill", Place::Bar).shortcut("cmd+n").on_press(|s: &mut App| s.open_new_skill()).build(k).key("new-skill"))
            .child(ds::icon_button(&UPDATE, check_label, Place::Bar).disabled(checking || remote).on_press(|s: &mut App| s.check_updates()).build(k).key("check-updates"))
            .child(ds::icon_button(&REFRESH, "Rescan", Place::Bar).shortcut("cmd+r").on_press(|s: &mut App| s.rescan()).build(k).key("rescan"))
            .child(
                sync.icon(&SYNC)
                    .help("Link every skill from .agents into your sync targets")
                    .on_press(|s: &mut App| s.open_sync())
                    .build(k)
                    .key("sync"),
            ),
    )
}

fn main(s: &App, visible: &[usize], targets: &[usize], k: &Kit, reduced: bool) -> El {
    let filter = ds::filter(&s.filter, "Filter skills").label("Filter skills").on_edit(|s: &mut App| &mut s.filter).build(k).key("filter");
    let mut page = Page::new("installed").filter(filter).segments(filters(s, k)).selection(selection(s, visible, k));
    if s.inv.skills.is_empty() {
        let hint = match s.host() {
            Some(h) => format!("Install one from skills.sh, then copy it to {} with prism skills copy --to-host {}.", h.name, h.name),
            None => format!("Install one from skills.sh, or make one with New skill ({}).", crate::ui::key_label("N")),
        };
        page = page.empty(ds::empty(k, "No skills here yet", hint));
    } else if visible.is_empty() {
        let hint = match s.status_filter {
            Filter::All => "Try another word.",
            Filter::NeedsSync => "Every skill is linked into your sync targets.",
            Filter::Broken => "No broken links here.",
            Filter::Updates if s.checking.contains(&s.scope) => "Still checking skills.sh…",
            Filter::Updates => "Everything is up to date.",
        };
        page = page.empty(ds::empty(k, "Nothing to show", hint));
    }
    page.rows(visible.iter().map(|&i| skill_row(s, &s.inv.skills[i], targets, k, reduced))).build(k)
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

/// The status filters under the text filter.
fn filters(s: &App, k: &Kit) -> El {
    let labels: Vec<String> = FILTERS.iter().map(|&f| filter_label(s, f)).collect();
    let current = FILTERS.iter().position(|&f| f == s.status_filter);
    ds::segments(&labels, current, "Show", |s: &mut App, i: usize, _: &mut Tx| s.set_status_filter(FILTERS[i])).build(k).key("status-filter")
}

/// Above the list: a tick-all box, then either the scope's summary (and Clean up) or, with skills
/// ticked, what can be done to them all.
fn selection(s: &App, visible: &[usize], k: &Kit) -> El {
    let ticked = visible.iter().filter(|&&i| s.is_checked(&s.inv.skills[i].name)).count();
    let partial = ticked > 0 && ticked < visible.len();
    let all = !visible.is_empty() && ticked == visible.len();
    let sel = Selection::new(all, partial, !visible.is_empty(), |s: &mut App| s.toggle_all_visible())
        .label(if all { "Untick all" } else { "Tick all shown" })
        .clear_help("Clear selection (Esc)");
    let n = s.checked.len();
    if n > 0 {
        return bulk_bar(s, n, sel, k).build(k);
    }
    let total = s.inv.skills.len();
    let root = tilde(&s.inv.scope.root(&s.paths), &s.paths);
    let mut sel = sel.summary(format!("{total} skill{} in {root} · {} to pick several", if total == 1 { "" } else { "s" }, crate::ui::pick_hint()));
    let broken = crate::sync::broken_links(&s.inv).len();
    if broken > 0 {
        sel = sel.action(
            ds::quiet(format!("Clean up {broken} broken link{}", if broken == 1 { "" } else { "s" }), Place::List)
                .help("Delete links whose target is gone (Undo brings them back)")
                .on_press(|s: &mut App| s.remove_broken())
                .build(k)
                .key("clean-broken"),
        );
    }
    sel.build(k)
}

/// The selection line with `n` skills ticked: Sync, Update (when any has one), Copy to…, and
/// Remove… on the right.
fn bulk_bar(s: &App, n: usize, sel: Selection<App>, k: &Kit) -> Selection<App> {
    let with_updates: Vec<String> = s.checked.iter().filter(|x| s.has_update(x)).cloned().collect();
    let mut sel = sel
        .picked(n, |s: &mut App| s.clear_checked())
        .action(ds::primary("Sync", Place::List).icon(&SYNC).help("Sync only the selected skills").on_press(|s: &mut App| s.open_sync_checked()).build(k).key("bulk-sync"));
    if !with_updates.is_empty() {
        let count = with_updates.len();
        sel = sel.action(
            ds::button(if count == n { "Update".to_string() } else { format!("Update {count}") }, Place::List)
                .icon(&UPDATE)
                .disabled(s.updating.is_some())
                .on_press(move |s: &mut App| s.update(with_updates.clone()))
                .build(k)
                .key("bulk-update"),
        );
    }
    if s.can_copy() {
        sel = sel.action(
            ds::button("Copy to…", Place::List)
                .help("Copy the selected skills into another scope or machine")
                .on_press(|s: &mut App| s.open_copy(s.checked.clone()))
                .build(k)
                .key("bulk-copy"),
        );
    }
    sel.danger(ds::danger("Remove…", Place::List).on_press(|s: &mut App| s.confirm_remove_checked()).build(k).key("bulk-remove"))
}

/// The agents a row shows tags for: every agent that has it, `.agents` first.
fn shown_agents(skill: &Skill) -> Vec<usize> {
    (0..AGENTS.len()).filter(|&a| skill.placements[a] != Placement::Missing).collect()
}

/// A skill's sync state in a few words, its tone, and the detail for its tooltip.
pub(super) fn sync_status(skill: &Skill, targets: &[usize]) -> (String, Tone, String) {
    let missing: Vec<&str> = targets.iter().filter(|&&t| skill.placements[t] == Placement::Missing).map(|&t| AGENTS[t].name).collect();
    match skill.canonical() {
        Placement::Broken(_) => ("broken in .agents".into(), Tone::Bad, "Its .agents entry is a link to nothing".into()),
        Placement::Missing if !skill.placements.iter().any(Placement::present) => ("broken links".into(), Tone::Bad, "Every copy is a link to nothing".into()),
        Placement::Missing => ("not in .agents".into(), Tone::Warn, "Sync moves it into .agents and links it back".into()),
        _ if missing.is_empty() => ("synced".into(), Tone::Ok, "Linked into every sync target".into()),
        _ => (format!("{} to link", missing.len()), Tone::Neutral, format!("Not linked into {}", missing.join(", "))),
    }
}

fn skill_row(s: &App, skill: &Skill, targets: &[usize], k: &Kit, reduced: bool) -> El {
    let mut tags = Vec::new();
    if s.has_update(&skill.name) {
        tags.push(ds::tag(k, "update", Some(Tone::Info), "A newer version is on skills.sh").key("update-tag"));
    }
    if !s.scope.is_global() && s.global_names.contains(&skill.name) {
        tags.push(ds::tag(k, "also global", None, "A global skill has the same name; agents see both").key("shadow-tag"));
    }
    for a in shown_agents(skill) {
        let p = &skill.placements[a];
        let help = format!("{}: {}", AGENTS[a].short, if s.inv.shares_canonical[a] { "same folder as .agents" } else { p.label() });
        tags.push(ds::tag(k, AGENTS[a].short, placement_tone(p), help));
    }
    let (word, tone, why) = sync_status(skill, targets);
    let (name, tick) = (skill.name.clone(), skill.name.clone());
    let mut r = ListRow::new(format!("skill-{}", skill.name), skill.name.clone())
        .pick(s.is_checked(&skill.name), format!("Select {}", skill.name), move |s: &mut App, tx: &mut Tx| s.click_tick(&tick, tx.mods()))
        .pick_key(format!("check-{}", skill.name))
        .tags(tags)
        .trailing(ds::status(k, tone, word).key("status").help(why))
        .selected(s.selected.as_deref() == Some(skill.name.as_str()))
        .reduced(reduced);
    if !skill.description.is_empty() {
        r = r.meta(skill.description.clone());
    }
    r.build_tx(k, move |s: &mut App, tx: &mut Tx| s.click_row(&name, tx.mods()))
}

/// The tone of an agent's copy: linked, its own folder, a link elsewhere, broken; none when it
/// has none.
pub fn placement_tone(p: &Placement) -> Option<Tone> {
    match p {
        Placement::Linked => Some(Tone::Ok),
        Placement::Dir => Some(Tone::Info),
        Placement::Link(_) => Some(Tone::Warn),
        Placement::Broken(_) => Some(Tone::Bad),
        Placement::Missing => None,
    }
}
