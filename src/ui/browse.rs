//! Searching skills.sh and installing from it into the chosen scope.

use rune::{El, Tx, col, row, text};
use rune_kit::shell::Toolbar;
use rune_kit::{Badge, BadgeVariant, Button, ButtonVariant, ControlSize, Divider, Kit, Markdown, Select, Spinner, TextInput, glyphs};

use super::{empty, field, heading, inspector_body, mono, row_gap, scroll_pane};
use crate::agents::Scope;
use crate::app::App;
use crate::remote::RemoteSkill;

/// The scopes installs can go to, as labels: global first, then the projects.
fn scope_options(s: &App) -> Vec<(String, Scope)> {
    let mut v = vec![(s.scope_label(&Scope::Global), Scope::Global)];
    v.extend(s.settings.projects.iter().map(|p| (s.scope_label(&Scope::Project(p.clone())), Scope::Project(p.clone()))));
    v
}

pub fn page(s: &App, k: &Kit, layer: &mut Vec<El>) -> (El, Toolbar, El) {
    let options = scope_options(s);
    let labels: Vec<String> = options.iter().map(|(l, _)| l.clone()).collect();
    let current = s.scope_label(&s.scope);
    let scope_select = Select::new("install-scope", &labels)
        .current(Some(&current))
        .label("Install into")
        .open(s.scope_menu)
        .on_toggle(|s: &mut App| s.scope_menu = !s.scope_menu)
        .on_close(|s: &mut App| s.scope_menu = false)
        .on_choose(|s: &mut App, v: &str, _: &mut Tx| {
            s.scope_menu = false;
            if let Some((_, scope)) = scope_options(s).into_iter().find(|(l, _)| l == v) {
                s.scope = scope;
                s.selected = None;
                s.rescan();
            }
        })
        .build(k, layer)
        .key("install-scope");
    let toolbar = Toolbar::new()
        .title("skills.sh")
        .trailing(row().gap(8.0).center_y().child(text("Install into").font(k.ty.small).color(k.c.text_muted)).child(scope_select));

    let search = TextInput::new(&s.query)
        .placeholder("Search skills.sh")
        .leading_icon(&glyphs::SEARCH)
        .clear_button()
        .large()
        .width(None)
        .label("Search skills.sh")
        .on_edit(|s: &mut App| &mut s.query)
        .on_change(|s: &mut App, _: &mut Tx| s.search_changed())
        .build(k)
        .key("query");

    let mut list = col().key("results").w_full().gap(2.0);
    match &s.results {
        Err(e) => list = list.child(empty("Search failed", e, k)),
        Ok(r) if r.is_empty() && s.search.is_pending() => list = list.child(row().w_full().py(32.0).center().child(Spinner::new().build(k))),
        Ok(r) if r.is_empty() && s.query.text.trim().len() >= 2 => list = list.child(empty("No skills found", "Try another search.", k)),
        Ok(r) if r.is_empty() => {
            list = list.child(empty("Find skills on skills.sh", "Type at least two letters, e.g. \"react\" or \"testing\".", k));
        }
        Ok(r) => {
            for skill in r {
                list = list.child(result_row(s, skill, k));
            }
        }
    }

    let source = TextInput::new(&s.source)
        .placeholder("owner/repo, owner/repo@skill or a URL")
        .width(None)
        .label("Install from a source")
        .on_edit(|s: &mut App| &mut s.source)
        .on_submit(|s: &mut App, _: &mut Tx| s.install_source())
        .build(k)
        .key("source");
    let installing = s.installing.is_some();
    let from_source = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(col().grow(1.0).min_w(0.0).child(source))
        .child(
            Button::new("Install")
                .variant(ButtonVariant::Outline)
                .disabled(installing || s.source.text.trim().is_empty())
                .on_press(|s: &mut App| s.install_source())
                .build(k)
                .key("install-source"),
        );

    let main = col()
        .key("browse-page")
        .w_full()
        .h_full()
        .min_h(0.0)
        .padding(16.0)
        .gap(12.0)
        .child(search)
        .child(scroll_pane("results-scroll", list).grow(1.0))
        .child(Divider::horizontal().build(k))
        .child(heading("Or install straight from a repository", k))
        .child(from_source);
    (main, toolbar, details(s, k))
}

fn result_row(s: &App, skill: &RemoteSkill, k: &Kit) -> El {
    let selected = s.remote.as_ref().is_some_and(|r| r.id == skill.id);
    let pick = skill.clone();
    let mut info = col()
        .gap(2.0)
        .grow(1.0)
        .min_w(0.0)
        .child(text(skill.name.clone()).font(k.ty.ui_medium).color(k.c.text_base).truncate())
        .child(text(skill.source.clone()).font(k.ty.small).color(k.c.text_muted).truncate());
    if s.installed_in(&skill.name) {
        info = info.child(row().child(Badge::new("installed").variant(BadgeVariant::Accent).build(k)));
    }
    let r = row()
        .key(format!("result-{}", skill.id))
        .role("button")
        .label(skill.name.clone())
        .focusable(true)
        .w_full()
        .px(12.0)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .radius(k.radius.md)
        .on_press(move |s: &mut App| s.select_remote(pick.clone()))
        .child(info)
        .child(text(installs(skill.installs)).font(k.ty.small).color(k.c.text_faint));
    if selected { r.fill(k.c.layer_02) } else { r.hover(k.c.hover) }
}

/// `1234567` as `1.2M installs`.
pub fn installs(n: u64) -> String {
    let short = match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}K", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    };
    format!("{} install{}", short.replace(".0", ""), if n == 1 { "" } else { "s" })
}

fn details(s: &App, k: &Kit) -> El {
    let mut body = inspector_body(k);
    if let Some(job) = &s.installing {
        body = body.child(
            row_gap()
                .key("installing")
                .child(Spinner::new().build(k))
                .child(text(format!("Installing {} into {}…", job.label, s.scope_label(&job.scope))).font(k.ty.small).color(k.c.text_muted).wrap()),
        );
    }
    if let Some(log) = &s.install_log {
        let (title, out) = match log {
            Ok(o) => ("Install log", o),
            Err(e) => ("Install failed", e),
        };
        body = body.child(field(title, mono(out.trim().to_string(), k), k).key("install-log"));
    }
    let Some(r) = &s.remote else {
        if s.installing.is_none() && s.install_log.is_none() {
            body = body.child(empty("No skill selected", "Pick a search result to preview and install it.", k));
        }
        return body;
    };
    let installed = s.installed_in(&r.name);
    let url = r.url();
    body = body
        .child(text(r.name.clone()).font(k.ty.body_medium).color(k.c.text_base).wrap())
        .child(field("Source", mono(r.source.clone(), k), k))
        .child(field("Popularity", text(installs(r.installs)).font(k.ty.ui).color(k.c.text_base), k))
        .child(
            row_gap()
                .child(
                    Button::new(if installed { "Reinstall" } else { "Install" })
                        .variant(if s.installing.is_some() { ButtonVariant::Loading } else { ButtonVariant::Contrast })
                        .control_size(ControlSize::Regular)
                        .disabled(s.installing.is_some())
                        .help(format!("Install into {}", s.scope_label(&s.scope)))
                        .on_press(|s: &mut App| s.install_remote())
                        .build(k)
                        .key("install"),
                )
                .child(
                    Button::new("Open on skills.sh")
                        .variant(ButtonVariant::Ghost)
                        .on_press_tx(move |_: &mut App, tx: &mut Tx| {
                            let _ = tx.open_url(&url);
                        })
                        .build(k)
                        .key("open-web"),
                ),
        )
        .child(
            text(format!(
                "Installs into {} and links it into your sync targets.",
                crate::agents::tilde(&crate::agents::AGENTS[crate::agents::CANONICAL].dir(&s.scope, &s.paths), &s.paths)
            ))
                .font(k.ty.small)
                .color(k.c.text_faint)
                .wrap(),
        );
    body = body.child(Divider::horizontal().build(k)).child(heading("SKILL.md", k));
    body.child(match &s.remote_md {
        None => row().py(16.0).child(Spinner::new().build(k)),
        Some(Err(e)) => text(e.clone()).font(k.ty.small).color(k.c.text_faint).wrap(),
        Some(Ok(md)) => Markdown::new(crate::skill::parse(md).body)
            .on_link_tx(|_: &mut App, url: &str, tx: &mut Tx| {
                let _ = tx.open_url(url);
            })
            .build(k)
            .key("remote-md"),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn install_counts_read_short() {
        assert_eq!(super::installs(1), "1 install");
        assert_eq!(super::installs(999), "999 installs");
        assert_eq!(super::installs(1_000), "1K installs");
        assert_eq!(super::installs(876_158), "876.2K installs");
        assert_eq!(super::installs(1_256_574), "1.3M installs");
    }
}
