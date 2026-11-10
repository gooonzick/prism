//! The window: a sidebar of scopes, the main pane for the open page, and an inspector with the
//! selected skill's details.

mod browse;
mod dialogs;
mod hosts;
mod inspector;
mod installed;
mod mcp;
mod mcp_dialogs;

use rune::{Chrome, Color, Cx, El, Icon, KeyEvent, ScrollSpec, Spring, TextStyle, Tx, col, el, row, text};
use rune_kit::shell::{self, PaneMotion, Sidebar, SidebarRow, SidebarSection, SidebarToggle, SplitView, Toolbar};
use rune_kit::{ControlSize, Kit, Markdown, MarkdownStyle, Toast, ToastRegion, glyphs};

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
    let (main, toolbar, inspector) = match (s.page.clone(), hosts::pending(s, &k)) {
        // A host whose state isn't in yet: what is happening with it, in place of the page.
        (Page::Installed | Page::Mcp, Some(pane)) => (pane, Toolbar::new().leading(page_tabs(s, &k)).title(s.env_label()), inspector_body(&k)),
        (Page::Installed, None) => installed::page(s, &k, &mut layer),
        (Page::Mcp, None) => mcp::page(s, &k, &mut layer),
        (Page::Browse, _) => browse::page(s, &k, &mut layer),
    };
    dialogs::sync(s, &k, &mut layer);
    dialogs::remove(s, &k, &mut layer);
    dialogs::copy(s, &k, &mut layer);
    dialogs::new_skill(s, &k, &mut layer);
    dialogs::found(s, &k, &mut layer);
    mcp_dialogs::form(s, &k, &mut layer);
    mcp_dialogs::plan(s, &k, &mut layer);
    hosts::add_host(s, &k, &mut layer);
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
        .sidebar(&s.sidebar, |s: &mut App| &mut s.sidebar, sidebar(s, &k, &mut layer))
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

fn sidebar(s: &App, k: &Kit, layer: &mut Vec<El>) -> El {
    let installed = s.page != Page::Browse;
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
        let row = counted(SidebarRow::new(Scope::Project(p.clone()).title()), &Scope::Project(p.clone()))
            .icon(&glyphs::FOLDER)
            .subtitle({
                let n = s.stats.get(&Scope::Project(p.clone())).map_or(0, |x| x.total);
                format!("{} · {n} skill{}", tilde(p.parent().unwrap_or(p), &s.paths), if n == 1 { "" } else { "s" })
            })
            .height(40.0)
            .selected(installed && s.scope == Scope::Project(p.clone()))
            .on_select(move |s: &mut App, _| s.open_scope(Scope::Project(open.clone())));
        // A host's projects stay listed there until window actions reach hosts.
        let row = if s.is_remote() { row } else { row.on_close(move |s: &mut App, _| s.forget_project(&forget)) };
        row.build(k).key(format!("project-{i}"))
    });
    let add = SidebarRow::new(if s.discovering { "Looking for projects…" } else { "Add project…" })
        .icon(&PLUS)
        .height(32.0)
        .on_select(|s: &mut App, tx: &mut Tx| {
            if s.discovering {
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
    let mut section = SidebarSection::new("Projects").key("projects").count(s.settings.projects.len()).rows(projects);
    // Projects on a host are added there for now: no folder picker reaches it.
    if !s.is_remote() {
        section = section.row(add);
    }
    Sidebar::new()
        .label("Scopes")
        .inset(0.0)
        .header_gap(4.0)
        .header_padding(0.0, 12.0)
        .row_gap(2.0)
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

/// Skills or MCP servers of the open scope, in the toolbar: segments as the kit draws them,
/// keyed so a test (or a scene) can pick one.
pub fn page_tabs(s: &App, k: &Kit) -> El {
    let c = &k.c;
    let tab = |key: &str, label: &str, on: bool, open: fn(&mut App)| {
        let seg = row()
            .key(key.to_string())
            .role("radio")
            .label(label.to_string())
            .focusable(true)
            .focus_selected(on)
            .focus_ring(2.0, 1.0, c.border_focus)
            .h(28.0)
            .px(12.0)
            .center()
            .radius(6.0)
            .cursor(rune::Cursor::Pointer)
            .on_press(open)
            .child(text(label.to_string()).font(rune::TextStyle { line_height: 13.0, ..k.ty.ui }).color(if on { c.text_base } else { c.text_muted }).a11y_hidden());
        if on {
            seg.fill(c.bg_base).box_shadow(rune::Shadow { spread: 0.5, color: c.border_strong, ..Default::default() })
        } else {
            seg.hover(c.hover)
        }
    };
    row()
        .key("page-tabs")
        .role("radiogroup")
        .label("Show")
        .focus_group()
        .h(28.0)
        .center_y()
        .radius(6.0)
        .fill(c.layer_01)
        .box_shadow(rune::Shadow { spread: 0.5, color: c.border_base, ..Default::default() })
        .child(tab("tab-skills", "Skills", s.page == Page::Installed, |s| s.open_skills()))
        .child(tab("tab-mcp", "MCP servers", s.page == Page::Mcp, |s| s.open_mcp()))
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
    if ev.is_command('n') && s.page == Page::Mcp {
        // An open form or plan keeps what the user has typed or picked.
        if s.mcp_form.is_none() && s.mcp_dialog.is_none() {
            s.open_mcp_add();
        }
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
/// `key` with the platform's command modifier, as shortcut hints show it: "⌘N", or "Ctrl+N".
pub fn key_label(key: &str) -> String {
    if cfg!(target_os = "macos") { format!("⌘{key}") } else { format!("Ctrl+{key}") }
}

/// How to pick several rows with the mouse.
pub fn pick_hint() -> &'static str {
    if cfg!(target_os = "macos") { "⌘- or ⇧-click" } else { "Ctrl- or Shift-click" }
}

// The inspector's type ladder: title (15 semibold), section title (13 semibold), value (13 or
// 12 mono, text colour), label (11, muted), path and note (11, faint).

/// An inspector's title: a step above its section titles.
pub fn inspector_title(t: impl Into<String>, k: &Kit) -> El {
    text(t.into()).font(TextStyle::new(15.0, 600.0).lh(20.0)).color(k.c.text_base).wrap()
}

/// A section's title row in an inspector, with an optional control on the right.
pub fn section(title: &str, trailing: Option<El>, k: &Kit) -> El {
    let mut head = row().w_full().gap(8.0).center_y().child(text(title.to_string()).font(TextStyle::new(13.0, 600.0).lh(20.0)).color(k.c.text_base).grow(1.0).min_w(0.0));
    if let Some(t) = trailing {
        head = head.child(t);
    }
    head
}

/// A labelled value: the label quiet, the value in the text colour.
pub fn prop(label: &str, value: El, k: &Kit) -> El {
    col().w_full().gap(4.0).child(text(label.to_string()).font(k.ty.caption()).color(k.c.text_muted)).child(value)
}

/// A boxed block of monospace text.
pub fn code_box(inner: El, k: &Kit) -> El {
    col().w_full().px(10.0).py(7.0).radius(k.radius.md).fill(k.c.layer_01).border(1.0, k.c.border_muted).child(inner)
}

/// Monospace words that wrap only between words, so `--stdio` never splits; a word longer than
/// the line still breaks.
pub fn words(line: &str, k: &Kit) -> El {
    let mut flow = row().w_full().flex_wrap().gaps(0.0, 7.0);
    for w in line.split(' ').filter(|w| !w.is_empty()) {
        flow = flow.child(text(w.to_string()).font(k.ty.mono).color(k.c.text_base).wrap_anywhere().shrink().min_w(0.0).selectable(true).selection_color(k.selection()));
    }
    flow
}

/// A SKILL.md body in an inspector: body text at UI size and headings below the inspector's
/// own titles, so the file never outranks the pane around it.
pub fn skill_md(md: String, k: &Kit) -> El {
    let style = MarkdownStyle::from_kit(k)
        .body_font(k.ty.ui)
        .heading_font(1, TextStyle::new(13.0, 600.0).lh(20.0))
        .heading_font(2, TextStyle::new(13.0, 600.0).lh(20.0))
        .heading_font(3, TextStyle::new(13.0, 500.0).lh(20.0))
        .code_block_font(k.ty.mono);
    Markdown::new(md)
        .style(style)
        .on_link_tx(|_: &mut App, url: &str, tx: &mut Tx| {
            let _ = tx.open_url(url);
        })
        .build(k)
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
