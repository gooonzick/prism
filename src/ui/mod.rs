//! The window: a sidebar of scopes, the main pane for the open page, and an inspector with the
//! selected skill's details.

mod browse;
mod dialogs;
mod hosts;
mod inspector;
mod installed;
mod mcp;
mod mcp_dialogs;

use rune::{Chrome, Cx, El, Icon, KeyEvent, ScrollSpec, Tx, col, row};
use rune_kit::shell::{self, PaneState, Sidebar, SidebarRow, SidebarSection, SidebarToggle, SplitView, Toolbar};
use rune_kit::{Button, Kit, Toast, ToastRegion, glyphs};

use crate::agents::{Scope, tilde};
use crate::app::{App, Page};
use crate::ds::{self, LoadingButton, Place, motion, size, space};

pub static HOME: Icon = Icon {
    name: "se-home",
    view: 16.0,
    body: r#"<path d="M3 7L8 3L13 7V12.5H9.5V9.5H6.5V12.5H3Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>"#,
};
pub static PLUS: Icon = Icon {
    name: "se-plus",
    view: 16.0,
    body: r#"<path d="M8 3.5V12.5M3.5 8H12.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#,
};
pub static GLOBE: Icon = Icon {
    name: "se-globe",
    view: 16.0,
    body: r#"<circle cx="8" cy="8" r="5.5" stroke="currentColor" stroke-width="1.2" fill="none"/><path d="M2.5 8H13.5M8 2.5C9.6 4.2 10.3 6 10.3 8C10.3 10 9.6 11.8 8 13.5C6.4 11.8 5.7 10 5.7 8C5.7 6 6.4 4.2 8 2.5Z" stroke="currentColor" stroke-width="1.2" fill="none"/>"#,
};
pub static REFRESH: Icon = Icon {
    name: "se-refresh",
    view: 16.0,
    body: r#"<path d="M12.5 8A4.5 4.5 0 1 1 11.2 4.8M12.5 3V5.5H10" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};
pub static SYNC: Icon = Icon {
    name: "se-sync",
    view: 16.0,
    body: r#"<path d="M3 6.5H12L9.5 4M13 9.5H4L6.5 12" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

pub static UPDATE: Icon = Icon {
    name: "se-update",
    view: 16.0,
    body: r#"<path d="M8 10.5V3.5M5 6.5L8 3.5L11 6.5M3.5 12.5H12.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

pub fn chrome() -> Chrome {
    Chrome { full_size_content: true, traffic_lights: Some((16.0, 25.0)), drag_regions: true }
}

pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let k = Kit::neutral(cx.system_dark());
    let mut layer = Vec::new();
    if s.gallery {
        let g = crate::ds::gallery::<App>(&k, &mut layer);
        return rune_kit::root(g, layer);
    }
    // The inspector as it will be drawn: its width decides which actions fit in its row. With no
    // room for it, its content (and its "…" menu, whatever its flag says) isn't built.
    let pane = inspector_pane(s, cx);
    let (main, toolbar, inspector) = match (s.page.clone(), hosts::pending(s, &k)) {
        // A host whose state isn't in yet: what is happening with it, in place of the page.
        (Page::Installed | Page::Mcp, Some(pending)) => (pending, toolbar(s, &k, s.env_label(), row()), Some(ds::Inspector::new().build(&k))),
        (Page::Installed, None) => installed::page(s, &k, &mut layer, cx.is_reduced_motion(), toolbar_room(cx), pane.map(|p| p.width())),
        (Page::Mcp, None) => mcp::page(s, &k, &mut layer, cx.is_reduced_motion(), toolbar_room(cx), pane.is_some()),
        (Page::Browse, _) => {
            let (main, toolbar, details) = browse::page(s, &k, &mut layer, cx.layout_with("main-pane", |r| r[2]));
            (main, toolbar, Some(details))
        }
    };
    dialogs::sync(s, &k, &mut layer);
    dialogs::remove(s, &k, &mut layer);
    dialogs::copy(s, &k, &mut layer, cx.is_reduced_motion());
    dialogs::new_skill(s, &k, &mut layer);
    dialogs::found(s, &k, &mut layer);
    mcp_dialogs::form(s, &k, &mut layer);
    mcp_dialogs::plan(s, &k, &mut layer);
    hosts::add_host(s, &k, &mut layer);
    hosts::add_project(s, &k, &mut layer);
    if let Some(n) = &s.notice {
        let mut toast = Toast::new("notice", n.title.clone()).closable().on_close(|s: &mut App| s.notice = None);
        if !n.detail.is_empty() {
            toast = toast.description(n.detail.clone());
        }
        if n.undo {
            // On the machine the toast names; ⌘Z means the one in view.
            toast = toast.action(&k, "Undo", false, |s: &mut App, _: &mut Tx| s.undo_toast());
        }
        toast = toast.icon(&k, if n.error { &glyphs::CIRCLE_X } else { &glyphs::CHECK });
        layer.push(ToastRegion::new().toast(toast.build(&k)).build(&k));
    }
    // Key handling has no layout of its own: the list pane's edge goes into `App::seen` (no view
    // reads it, so the report runs no view).
    let main = main.on_layout_tx(|s: &mut App, r, tx: &mut Tx| {
        s.seen.pane_end = Some(r[0] + r[2]);
        tx.live_only();
    });
    let mut split = SplitView::new(main).sidebar(&s.sidebar, |s: &mut App| &mut s.sidebar, sidebar(s, &k, &mut layer));
    if let (Some(pane), Some(inspector)) = (pane, inspector) {
        split = split.inspector(&pane, |s: &mut App| &mut s.inspector, scroll_pane("inspector-scroll", inspector));
    }
    let split = split
        .toolbar(toolbar)
        .titlebar_inset(cx.titlebar_inset())
        .frame_inset(space::S2)
        .toolbar_in_panel()
        .sidebar_padding(space::S2, space::S2)
        .sidebar_toggle(SidebarToggle::new().size(Place::Bar.control()).past_lights(space::S4).edge(space::S4).gap(space::S1).in_header())
        .pane_motion(motion::panes())
        .reduced_motion(cx.is_reduced_motion())
        .build(&k);
    // The window's width, for `App::inspector_drawn`.
    rune_kit::root(split, layer).on_layout_tx(|s: &mut App, r, tx: &mut Tx| {
        s.seen.window = Some(r[2]);
        tx.live_only();
    })
}

/// The inspector pane as it is drawn in this window (`App::inspector_pane`), or `None` when the
/// window has no room for it.
pub fn inspector_pane(s: &App, cx: &Cx<App>) -> Option<PaneState> {
    s.inspector_pane(cx.layout_with("kit-root", |r| r[2]))
}

fn sidebar(s: &App, k: &Kit, layer: &mut Vec<El>) -> El {
    let installed = s.page != Page::Browse;
    // The badge counts what needs attention: skills not linked into every target.
    let counted = |row: SidebarRow, scope: &Scope| {
        let stats = s.stats.get(scope).copied().unwrap_or_default();
        if stats.unsynced > 0 { row.badge(stats.unsynced.to_string()) } else { row }
    };
    let global = counted(SidebarRow::new("Global").icon(&HOME), &Scope::Global)
        .height(size::ROW_1)
        .selected(installed && s.scope == Scope::Global)
        .on_select(|s: &mut App, _| s.open_scope(Scope::Global))
        .build(k)
        .key("scope-global");
    let browse = SidebarRow::new("skills.sh")
        .icon(&GLOBE)
        .height(size::ROW_1)
        .selected(s.page == Page::Browse)
        .on_select(|s: &mut App, _| s.open_browse())
        .build(k)
        .key("browse");
    let projects = s.settings.projects.iter().enumerate().map(|(i, p)| {
        let (open, forget) = (p.clone(), p.clone());
        let row = counted(SidebarRow::new(Scope::Project(p.clone()).title()), &Scope::Project(p.clone()))
            .icon(&glyphs::FOLDER)
            .subtitle({
                let n = s.stats.get(&Scope::Project(p.clone())).map_or(0, |x| x.total);
                format!("{} · {n} skill{}", tilde(p.parent().unwrap_or(p), &s.paths), if n == 1 { "" } else { "s" })
            })
            .height(size::ROW_2)
            .selected(installed && s.scope == Scope::Project(p.clone()))
            .on_select(move |s: &mut App, _| s.open_scope(Scope::Project(open.clone())))
            .on_close(move |s: &mut App, _| s.forget_project(&forget));
        row.build(k).key(format!("project-{i}"))
    });
    let add = SidebarRow::new(if s.discovering { "Looking for projects…" } else { "Add project…" })
        .icon(&PLUS)
        .height(size::ROW_1)
        .on_select(|s: &mut App, tx: &mut Tx| {
            if s.discovering {
                return;
            }
            // No folder picker reaches a host's disk: its projects are typed paths.
            if s.is_remote() {
                s.open_host_project();
                return;
            }
            let dev = s.paths.home.join("Development");
            let options = rune::FileDialogOptions {
                title: "Choose a project, or a folder that holds projects".into(),
                directory: Some(if dev.is_dir() { dev } else { s.paths.home.clone() }),
                ..Default::default()
            };
            tx.open_folder(options, |s: &mut App, result, _| match result {
                Ok(Some(dir)) => s.add_folder(dir),
                Ok(None) => {}
                Err(e) => s.notify("Couldn't open the folder picker".into(), e.to_string(), true),
            });
        })
        .build(k)
        .key("add-project");
    let section = SidebarSection::new("Projects").key("projects").count(s.settings.projects.len()).rows(projects).row(add);
    Sidebar::new()
        .label("Scopes")
        .inset(0.0)
        .header_gap(space::S1)
        .header_padding(0.0, space::S3)
        .row_gap(space::ROW_GAP)
        .header(hosts::switcher(s, k, layer))
        .header(global)
        .header(browse)
        .section(section)
        .build(k)
}

/// The toolbar title for a scope's page: "Global skills", or the project folder's name.
pub fn scope_title(scope: &Scope, what: &str) -> String {
    match scope {
        Scope::Global => format!("Global {what}"),
        Scope::Project(p) => p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()),
    }
}

/// Skills or MCP servers of the open scope, in the toolbar: `ds::tabs`, keyed so a test (or a
/// scene) can pick one. Their end goes into `App::seen` for key handling.
fn page_tabs(s: &App, k: &Kit) -> El {
    let current = if s.page == Page::Mcp { 1 } else { 0 };
    let pick = |s: &mut App, i, _: &mut Tx| if i == 1 { s.open_mcp() } else { s.open_skills() };
    ds::tabs(k, "Show", &[("tab-skills", "Skills"), ("tab-mcp", "MCP servers")], current, pick).key("page-tabs").on_layout_tx(|s: &mut App, r, tx: &mut Tx| {
        s.seen.tabs_end = Some(r[0] + r[2]);
        tx.live_only();
    })
}

/// The toolbar of a page with tabs: the page tabs, then the page's title (it truncates first),
/// on the leading side; `trailing` (the page's buttons) on the other.
pub fn toolbar(s: &App, k: &Kit, title: impl Into<String>, trailing: El) -> Toolbar {
    // One leading child, so the tabs and the title sit `S3` apart rather than at the kit's slot gap.
    let leading = row().gap(space::S3).center_y().shrink().min_w(0.0).child(page_tabs(s, k)).child(ds::toolbar_title(k, title).shrink().min_w(0.0));
    Toolbar::new().leading(leading).trailing(trailing)
}

/// The toolbar of a page with tabs and no title: a narrow list pane, where the title has no room
/// left beside the page's buttons.
pub fn toolbar_untitled(s: &App, k: &Kit, trailing: El) -> Toolbar {
    Toolbar::new().leading(page_tabs(s, k)).trailing(trailing)
}

/// What the toolbar of a page with tabs has left for the page's title and buttons
/// (`room_beside_tabs`), as the view reads the layout. `None` before the first layout.
fn toolbar_room(cx: &Cx<App>) -> Option<f32> {
    room_beside_tabs(cx.layout_with("main-pane", |r| r[0] + r[2]), cx.layout_with("page-tabs", |r| r[0] + r[2]))
}

/// What the toolbar of a page with tabs has left for the page's title and buttons: from the
/// tabs' end to the list pane's, less the kit band's end padding (`S3`) and the `S2` gaps either
/// side of its spacer. `None` before the first layout. The pane's width does not depend on what
/// the toolbar holds, so a page that folds its buttons by this never flips back and forth.
fn room_beside_tabs(pane_end: Option<f32>, tabs_end: Option<f32>) -> Option<f32> {
    Some(pane_end? - tabs_end? - space::S3 - 2.0 * space::S2)
}

/// Whether controls `widths` wide fit side by side in `room`, `S2` apart: the toolbars, the
/// skill inspector's action row and the skills.sh footer measure their rows with it before
/// layout (`ds::button_width`).
pub fn row_fits(widths: &[f32], room: f32) -> bool {
    let gaps = widths.len().saturating_sub(1) as f32 * space::S2;
    widths.iter().sum::<f32>() + gaps <= room
}

/// Whether a page's wide toolbar fits the `room` beside its tabs (`toolbar_room`; `None` before
/// the first layout fits): the title `S3` after the tabs, then `icons` icon buttons and one text
/// button with an icon labelled `button`, `S2` apart. The title counts at most `ROW_NAME_MIN`:
/// past that it truncates, so a long project name never folds the buttons while they fit beside
/// a shortened title.
pub fn toolbar_fits(k: &Kit, room: Option<f32>, title: &str, icons: usize, button: &str) -> bool {
    let title = space::S3 + ds::toolbar_title_width(k, title).min(size::ROW_NAME_MIN);
    let mut controls = vec![Place::Bar.height(); icons];
    controls.push(ds::button_width(k, button, Place::Bar, true));
    room.is_none_or(|room| row_fits(&controls, room - title))
}

/// The Skills toolbar's "…" menu is drawn: open, on the Skills page with no host's pane in its
/// place, in a toolbar too narrow for its buttons (`installed::toolbar_folds`) as layout last
/// reported it (`App::seen`). Only then does it hold the keys.
fn toolbar_menu_shown(s: &App) -> bool {
    // The kit's type is the same in both themes, so either measures the toolbar.
    let k = Kit::neutral(false);
    s.toolbar_menu && s.page == Page::Installed && !s.host_pending() && installed::toolbar_folds(s, &k, room_beside_tabs(s.seen.pane_end, s.seen.tabs_end))
}

/// Cancel in a dialog's footer.
pub fn cancel(key: &str, k: &Kit, on: impl Fn(&mut App) + 'static) -> El {
    ds::button("Cancel", Place::Bar).on_press(on).build(k).key(key.to_string())
}

/// "1 change", "3 changes".
pub fn changes(n: usize) -> String {
    format!("{n} change{}", if n == 1 { "" } else { "s" })
}

/// A plan's changes card label: "3 changes", or "Changes" when there are none.
pub fn changes_heading(n: usize) -> String {
    if n == 0 { "Changes".to_string() } else { changes(n) }
}

/// The apply button of a plan of `n` changes, `danger` when it removes: loading while it is
/// applied, disabled when there is nothing to do.
pub fn apply_button(applying: bool, n: usize, danger: bool) -> Button {
    let label = if n == 0 { "Nothing to do".to_string() } else { format!("Apply {}", changes(n)) };
    match (applying, danger) {
        (true, _) => ds::primary("Applying…", Place::Bar).loading(true),
        (false, true) => ds::danger(label, Place::Bar).disabled(n == 0),
        (false, false) => ds::primary(label, Place::Bar).disabled(n == 0),
    }
}

/// The toolbar of a page without tabs (skills.sh): the title on the leading side, `trailing` on
/// the other.
pub fn toolbar_untabbed(k: &Kit, title: impl Into<String>, trailing: El) -> Toolbar {
    // The title truncates before the trailing controls give way.
    let leading = row().shrink().min_w(0.0).child(ds::toolbar_title(k, title).shrink().min_w(0.0));
    Toolbar::new().leading(leading).trailing(trailing)
}

/// A pane's content in a vertical scroll view that fills the pane.
pub fn scroll_pane(key: &str, content: El) -> El {
    col().key(key.to_string()).w_full().h_full().min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(content)
}

pub fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if shell::is_sidebar_toggle(ev) {
        s.sidebar.toggle();
        // The list's width changes, and with it whether the Skills toolbar folds into "…".
        s.toolbar_menu = false;
        // The inspector's width may change too, and with it what its "…" holds.
        s.skill_menu = false;
        s.mcp_menu = false;
        return true;
    }
    // An open "…" takes the keys: arrows move through it, Escape closes it. Only while it is
    // drawn, so a flag left behind never swallows a key.
    if s.skill_menu_shown() || s.mcp_menu_shown() || toolbar_menu_shown(s) {
        return false;
    }
    if ev.key == rune::Key::Escape && !s.checked.is_empty() && !s.sync_open() && s.confirm_remove.is_none() {
        s.clear_checked();
        return true;
    }
    if ev.is_command('a') && s.page == Page::Installed && !s.filter.focused {
        s.toggle_all_visible();
        return true;
    }
    // The machine in view: a host's last change is undone there.
    if ev.is_command('z') && s.can_undo() {
        s.undo();
        return true;
    }
    if ev.is_command('n') && s.page == Page::Installed {
        s.open_new_skill();
        return true;
    }
    if ev.is_command('n') && s.page == Page::Mcp {
        // An open form or plan keeps what the user has typed or picked.
        if s.mcp_form.is_none() && s.mcp_dialog.is_none() {
            s.open_mcp_add();
        }
        return true;
    }
    let in_list = s.page == Page::Installed && !s.filter.focused && !s.sync_open() && s.confirm_remove.is_none();
    if in_list && matches!(ev.key, rune::Key::Up | rune::Key::Down) && !ev.mods.cmd {
        let delta = if ev.key == rune::Key::Down { 1 } else { -1 };
        if let Some(key) = s.step_selection(delta, ev.mods.shift) {
            tx.scroll_into_view(&key);
        }
        return true;
    }
    if in_list
        && ev.key == rune::Key::Text(" ".into())
        && !ev.mods.cmd
        && let Some(name) = s.selected.clone()
    {
        s.toggle_checked(&name);
        return true;
    }
    if ev.is_command('r') {
        s.rescan();
        return true;
    }
    if ev.is_command('k') {
        s.open_browse();
        return true;
    }
    if ev.is_command('f') && s.page == Page::Installed {
        s.filter.focused = true;
        return true;
    }
    false
}

/// `key` with the platform's command modifier, as shortcut hints show it: "⌘N", or "Ctrl+N".
pub fn key_label(key: &str) -> String {
    if cfg!(target_os = "macos") { format!("⌘{key}") } else { format!("Ctrl+{key}") }
}

/// How to pick several rows with the mouse.
pub fn pick_hint() -> &'static str {
    if cfg!(target_os = "macos") { "⌘- or ⇧-click" } else { "Ctrl- or Shift-click" }
}
