//! The window: a sidebar of scopes, the main pane for the open page, and an inspector with the
//! selected skill's details.

mod browse;
mod dialogs;
mod inspector;
mod installed;

use rune::{Chrome, Color, Cx, El, Icon, KeyEvent, ScrollSpec, Spring, Tx, col, el, row, text};
use rune_kit::shell::{self, PaneMotion, Sidebar, SidebarRow, SidebarSection, SidebarToggle, SplitView};
use rune_kit::{ControlSize, Kit, Toast, ToastRegion, glyphs};

use crate::agents::{Scope, tilde};
use crate::app::{App, Page};

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
    let (main, toolbar, inspector) = match s.page {
        Page::Installed => installed::page(s, &k, &mut layer),
        Page::Browse => browse::page(s, &k, &mut layer),
    };
    dialogs::sync(s, &k, &mut layer);
    dialogs::remove(s, &k, &mut layer);
    dialogs::copy(s, &k, &mut layer);
    dialogs::new_skill(s, &k, &mut layer);
    dialogs::found(s, &k, &mut layer);
    if let Some(n) = &s.notice {
        let mut toast = Toast::new("notice", n.title.clone()).closable().on_close(|s: &mut App| s.notice = None);
        if !n.detail.is_empty() {
            toast = toast.description(n.detail.clone());
        }
        if n.undo {
            toast = toast.action(&k, "Undo", false, |s: &mut App, _: &mut Tx| s.undo());
        }
        toast = toast.icon(&k, if n.error { &glyphs::CIRCLE_X } else { &glyphs::CHECK });
        layer.push(ToastRegion::new().toast(toast.build(&k)).build(&k));
    }
    let split = SplitView::new(main)
        .sidebar(&s.sidebar, |s: &mut App| &mut s.sidebar, sidebar(s, &k))
        .inspector(&s.inspector, |s: &mut App| &mut s.inspector, scroll_pane("inspector-scroll", inspector))
        .toolbar(toolbar)
        .titlebar_inset(cx.titlebar_inset())
        .frame_inset(8.0)
        .toolbar_in_panel()
        .sidebar_padding(18.0, 10.0)
        .sidebar_toggle(SidebarToggle::new().size(ControlSize::Large).past_lights(16.0).edge(18.0).gap(4.0).in_header())
        .pane_motion(PaneMotion::new().spring(Spring::new(0.3, 0.0)).scale_with_width(480.0, 0.15, 0.3).fade_tail(0.15))
        .reduced_motion(cx.is_reduced_motion())
        .build(&k);
    rune_kit::root(split, layer)
}

fn sidebar(s: &App, k: &Kit) -> El {
    let installed = s.page == Page::Installed;
    // The badge counts what needs attention: skills not linked into every target.
    let counted = |row: SidebarRow, scope: &Scope| {
        let stats = s.stats.get(scope).copied().unwrap_or_default();
        if stats.unsynced > 0 { row.badge(stats.unsynced.to_string()) } else { row }
    };
    let global = counted(SidebarRow::new("Global").icon(&HOME), &Scope::Global)
        .height(32.0)
        .selected(installed && s.scope == Scope::Global)
        .on_select(|s: &mut App, _| s.open_scope(Scope::Global))
        .build(k)
        .key("scope-global");
    let browse = SidebarRow::new("skills.sh")
        .icon(&GLOBE)
        .height(32.0)
        .selected(s.page == Page::Browse)
        .on_select(|s: &mut App, _| s.open_browse())
        .build(k)
        .key("browse");
    let projects = s.settings.projects.iter().enumerate().map(|(i, p)| {
        let (open, forget) = (p.clone(), p.clone());
        counted(SidebarRow::new(Scope::Project(p.clone()).title()), &Scope::Project(p.clone()))
            .icon(&glyphs::FOLDER)
            .subtitle({
                let n = s.stats.get(&Scope::Project(p.clone())).map_or(0, |x| x.total);
                format!("{} · {n} skill{}", tilde(p.parent().unwrap_or(p), &s.paths), if n == 1 { "" } else { "s" })
            })
            .height(40.0)
            .selected(installed && s.scope == Scope::Project(p.clone()))
            .on_select(move |s: &mut App, _| s.open_scope(Scope::Project(open.clone())))
            .on_close(move |s: &mut App, _| s.forget_project(&forget))
            .build(k)
            .key(format!("project-{i}"))
    });
    let add = SidebarRow::new("Add project…")
        .icon(&PLUS)
        .height(32.0)
        .on_select(|_: &mut App, tx: &mut Tx| {
            let options = rune::FileDialogOptions { title: "Choose a project folder".into(), ..Default::default() };
            tx.open_folder(options, |s: &mut App, result, _| match result {
                Ok(Some(dir)) => s.add_project(dir),
                Ok(None) => {}
                Err(e) => s.notify("Couldn't open the folder picker".into(), e.to_string(), true),
            });
        })
        .build(k)
        .key("add-project");
    let find = SidebarRow::new(if s.discovering { "Finding projects…" } else { "Find projects…" })
        .icon(&glyphs::SEARCH)
        .height(32.0)
        .on_select(|s: &mut App, tx: &mut Tx| {
            if s.discovering {
                return;
            }
            let dev = s.paths.home.join("Development");
            let options = rune::FileDialogOptions {
                title: "Look for projects with skills in…".into(),
                directory: Some(if dev.is_dir() { dev } else { s.paths.home.clone() }),
                ..Default::default()
            };
            tx.open_folder(options, |s: &mut App, result, _| match result {
                Ok(Some(dir)) => s.discover_in(dir),
                Ok(None) => {}
                Err(e) => s.notify("Couldn't open the folder picker".into(), e.to_string(), true),
            });
        })
        .build(k)
        .key("find-projects");
    Sidebar::new()
        .label("Scopes")
        .inset(0.0)
        .header_gap(4.0)
        .header_padding(0.0, 12.0)
        .row_gap(2.0)
        .header(global)
        .header(browse)
        .section(SidebarSection::new("Projects").key("projects").count(s.settings.projects.len()).rows(projects).row(add).row(find))
        .build(k)
}

/// A pane's content in a vertical scroll view that fills the pane.
pub fn scroll_pane(key: &str, content: El) -> El {
    col().key(key.to_string()).w_full().h_full().min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(content)
}

/// A small heading over a group in a pane.
pub fn heading(label: &str, k: &Kit) -> El {
    text(label.to_string()).font(k.ty.small_medium).color(k.c.text_faint)
}

pub fn dot(color: Color) -> El {
    el().size(7.0, 7.0).radius(3.5).fill(color)
}

pub fn empty(title: &str, detail: &str, k: &Kit) -> El {
    col()
        .w_full()
        .py(48.0)
        .gap(6.0)
        .items_center()
        .child(text(title.to_string()).font(k.ty.ui_medium).color(k.c.text_muted))
        .child(text(detail.to_string()).font(k.ty.small).color(k.c.text_faint).wrap())
}

pub fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if shell::is_sidebar_toggle(ev) {
        s.sidebar.toggle();
        return true;
    }
    if ev.key == rune::Key::Escape && !s.checked.is_empty() && s.sync_plan.is_none() && s.confirm_remove.is_none() {
        s.clear_checked();
        return true;
    }
    if ev.is_command('a') && s.page == Page::Installed && !s.filter.focused {
        s.toggle_all_visible();
        return true;
    }
    if ev.is_command('z') && s.last_change.is_some() {
        s.undo();
        return true;
    }
    if ev.is_command('n') && s.page == Page::Installed {
        s.open_new_skill();
        return true;
    }
    let in_list = s.page == Page::Installed && !s.filter.focused && s.sync_plan.is_none() && s.confirm_remove.is_none();
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

/// The row of a key-value field in the inspector.
pub fn field(label: &str, value: El, k: &Kit) -> El {
    col().gap(3.0).w_full().child(heading(label, k)).child(value)
}

pub fn mono(t: impl Into<String>, k: &Kit) -> El {
    text(t.into()).font(k.ty.mono).color(k.c.text_muted).wrap_anywhere().selectable(true).selection_color(k.selection())
}

pub fn inspector_body(k: &Kit) -> El {
    let _ = k;
    col().key("inspector").w_full().padding(16.0).gap(16.0)
}

pub fn row_gap() -> El {
    row().gap(8.0).center_y()
}
