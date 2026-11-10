//! Searching skills.sh and installing from it into the chosen scope.

use rune::{El, Tx, col, row, text};
use rune_kit::shell::Toolbar;
use rune_kit::{Badge, BadgeVariant, Button, ButtonVariant, Checkbox, ControlSize, Divider, Kit, Select, Spinner, TextInput, glyphs};

use super::{code_box, empty, heading, inspector_body, inspector_title, mono, prop, row_gap, scroll_pane, section, skill_md};
use crate::agents::Scope;
use crate::app::{App, RepoListing};
use crate::remote::RemoteSkill;

/// The scopes installs can go to, as labels: global first, then the projects. They are this
/// machine's, whichever machine is in view.
fn scope_options(s: &App) -> Vec<(String, Scope)> {
    let mut v = vec![(s.install_label(&Scope::Global), Scope::Global)];
    v.extend(s.local_settings().projects.iter().map(|p| (s.install_label(&Scope::Project(p.clone())), Scope::Project(p.clone()))));
    v
}

pub fn page(s: &App, k: &Kit, layer: &mut Vec<El>) -> (El, Toolbar, El) {
    let options = scope_options(s);
    let labels: Vec<String> = options.iter().map(|(l, _)| l.clone()).collect();
    let current = s.install_label(s.local_scope());
    let scope_select = Select::new("install-scope", &labels)
        .current(Some(&current))
        .label("Install into")
        .open(s.scope_menu)
        .on_toggle(|s: &mut App| s.scope_menu = !s.scope_menu)
        .on_close(|s: &mut App| s.scope_menu = false)
        .on_choose(|s: &mut App, v: &str, _: &mut Tx| {
            s.scope_menu = false;
            if let Some((_, scope)) = scope_options(s).into_iter().find(|(l, _)| l == v) {
                s.set_install_scope(scope);
            }
        })
        .build(k, layer)
        .key("install-scope");
    let toolbar = super::toolbar_untabbed(k, "skills.sh", row().gap(8.0).center_y().child(text("Install into").font(k.ty.small).color(k.c.text_muted)).child(scope_select));

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
            Button::new("Show skills")
                .variant(ButtonVariant::Outline)
                .help("List the skills in this repository to pick from")
                .disabled(s.source.text.trim().is_empty())
                .on_press(|s: &mut App| s.list_repo())
                .build(k)
                .key("list-source"),
        )
        .child(
            Button::new("Install all")
                .variant(ButtonVariant::Ghost)
                .help("Install every skill in it (or the one named after @)")
                .disabled(installing || s.source.text.trim().is_empty())
                .on_press(|s: &mut App| s.install_source())
                .build(k)
                .key("install-source"),
        );

    let mut main = col().key("browse-page").w_full().h_full().min_h(0.0).padding(16.0).gap(12.0);
    if let Some(note) = super::hosts::browse_note(s, k) {
        main = main.child(note);
    }
    let main = main
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
        info = info.child(row().child(Badge::new("installed").variant(BadgeVariant::Subtle).build(k)));
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
                .child(text(format!("Installing {} into {}…", job.label, s.install_label(&job.scope))).font(k.ty.small).color(k.c.text_muted).wrap()),
        );
    }
    if let Some(log) = &s.install_log {
        let (title, out) = match log {
            Ok(o) => ("Install log", o),
            Err(e) => ("Install failed", e),
        };
        body = body.child(prop(title, code_box(mono(out.trim().to_string(), k).color(k.c.text_base), k), k).key("install-log"));
    }
    if let Some(repo) = &s.repo {
        return body.child(repo_listing(s, repo, k));
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
        .child(inspector_title(r.name.clone(), k))
        .child(prop("Source", mono(r.source.clone(), k).color(k.c.text_base), k))
        .child(prop("Popularity", text(installs(r.installs)).font(k.ty.ui).color(k.c.text_base), k))
        .child(
            row_gap()
                .child(
                    Button::new(if installed { "Reinstall" } else { "Install" })
                        .variant(if s.installing.is_some() { ButtonVariant::Loading } else { ButtonVariant::Contrast })
                        .control_size(ControlSize::Regular)
                        .disabled(s.installing.is_some())
                        .help(format!("Install into {}", s.install_label(s.local_scope())))
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
                crate::agents::tilde(&crate::agents::AGENTS[crate::agents::CANONICAL].dir(s.local_scope(), s.local_paths()), s.local_paths())
            ))
                .font(k.ty.caption())
                .color(k.c.text_faint)
                .wrap(),
        );
    body = body.child(Divider::horizontal().build(k)).child(section("SKILL.md", None, k));
    body.child(match &s.remote_md {
        None => row().py(16.0).child(Spinner::new().build(k)),
        Some(Err(e)) => text(e.clone()).font(k.ty.small).color(k.c.text_faint).wrap(),
        Some(Ok(md)) => skill_md(crate::skill::parse(md).body, k).key("remote-md"),
    })
}


/// A repository's skills with a tick each, and one button to install the ticked ones.
fn repo_listing(s: &App, repo: &RepoListing, k: &Kit) -> El {
    let mut body = col()
        .key("repo")
        .w_full()
        .gap(12.0)
        .child(inspector_title(repo.source.clone(), k));
    let skills = match &repo.skills {
        None => return body.child(row_gap().child(Spinner::new().build(k)).child(text("Reading the repository…").font(k.ty.small).color(k.c.text_muted))),
        Some(Err(e)) => return body.child(text(e.clone()).font(k.ty.small).color(k.c.text_muted).wrap()),
        Some(Ok(v)) => v,
    };
    let picked = skills.iter().filter(|r| r.2).count();
    body = body.child(
        row_gap()
            .child(
                Button::new(match picked {
                    0 => "Pick skills to install".to_string(),
                    1 => "Install 1 skill".to_string(),
                    n => format!("Install {n} skills"),
                })
                .variant(if s.installing.is_some() { ButtonVariant::Loading } else { ButtonVariant::Contrast })
                .control_size(ControlSize::Regular)
                .disabled(picked == 0 || s.installing.is_some())
                .help(format!("Install into {}", s.install_label(s.local_scope())))
                .on_press(|s: &mut App| s.install_repo_ticked())
                .build(k)
                .key("install-ticked"),
            )
            .child(Button::new("Close").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.repo = None).build(k).key("repo-close")),
    );
    let mut list = col().w_full().gap(10.0);
    for (i, (name, desc, on)) in skills.iter().enumerate() {
        let mut cb = Checkbox::new(*on).label(name.clone()).on_toggle(move |s: &mut App| s.toggle_repo_skill(i));
        if !desc.is_empty() {
            cb = cb.description(desc.clone());
        }
        if s.installed_in(name) {
            cb = cb.label(format!("{name} (installed)"));
        }
        list = list.child(cb.build(k).key(format!("repo-skill-{i}")));
    }
    body.child(list)
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
