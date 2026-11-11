//! Searching skills.sh and installing from it into the chosen scope.

use rune::{El, Tx, col, row};
use rune_kit::shell::Toolbar;
use rune_kit::{Button, ButtonVariant, Checkbox, Kit, Select, TextInput};

use crate::agents::Scope;
use crate::app::{App, RepoListing};
use crate::ds::{self, Card, Inspector, ListRow, Page, Place, pair_row, space, stacked_row, text};
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
    let toolbar = super::toolbar_untabbed(k, "skills.sh", row().gap(space::S2).center_y().child(text::label(k, "Install into")).child(scope_select));
    (main(s, k), toolbar, details(s, k))
}

fn main(s: &App, k: &Kit) -> El {
    let search = ds::filter(&s.query, "Search skills.sh")
        .label("Search skills.sh")
        .on_edit(|s: &mut App| &mut s.query)
        .on_change(|s: &mut App, _: &mut Tx| s.search_changed())
        .build(k)
        .key("query");
    let mut page = Page::new("browse-page").filter(search);
    if let Some(note) = super::hosts::browse_note(s, k) {
        page = page.notice(note);
    }
    match &s.results {
        Err(e) => page = page.empty(ds::empty(k, "Search failed", e.clone())),
        Ok(r) if r.is_empty() && s.search.is_pending() => {
            page = page.empty(row().w_full().py(space::S6).center().child(ds::loading(k, "Searching skills.sh…")));
        }
        Ok(r) if r.is_empty() && s.query.text.trim().len() >= 2 => page = page.empty(ds::empty(k, "No skills found", "Try another search.")),
        Ok(r) if r.is_empty() => {
            page = page.empty(ds::empty(k, "Find skills on skills.sh", "Type at least two letters, e.g. \"react\" or \"testing\"."));
        }
        Ok(r) => page = page.rows(r.iter().map(|skill| result_row(s, skill, k))),
    }
    page.footer(from_source(s, k)).build(k)
}

/// "Or install straight from a repository": the source, then Show skills and Install all under
/// it, so the input keeps its width in the narrowest list.
fn from_source(s: &App, k: &Kit) -> El {
    let source = TextInput::new(&s.source)
        .placeholder("owner/repo, owner/repo@skill or a URL")
        .width(None)
        .label("Install from a source")
        .on_edit(|s: &mut App| &mut s.source)
        .on_submit(|s: &mut App, _: &mut Tx| s.install_source())
        .build(k)
        .key("source");
    let installing = s.installing.is_some();
    let buttons = row()
        .gap(space::S2)
        .center_y()
        .child(
            ds::button("Show skills", Place::List)
                .help("List the skills in this repository to pick from")
                .disabled(s.source.text.trim().is_empty())
                .on_press(|s: &mut App| s.list_repo())
                .build(k)
                .key("list-source"),
        )
        .child(
            ds::quiet("Install all", Place::List)
                .help("Install every skill in it (or the one named after @)")
                .disabled(installing || s.source.text.trim().is_empty())
                .on_press(|s: &mut App| s.install_source())
                .build(k)
                .key("install-source"),
        );
    col()
        .key("from-source")
        .w_full()
        .gap(space::S2)
        .child(text::label(k, "Or install straight from a repository"))
        .child(source)
        .child(buttons)
}

fn result_row(s: &App, skill: &RemoteSkill, k: &Kit) -> El {
    let pick = skill.clone();
    let mut tags = Vec::new();
    if s.installed_in(&skill.name) {
        tags.push(ds::tag(k, "installed", None, "Already in this scope").key("installed-tag"));
    }
    ListRow::new(format!("result-{}", skill.id), skill.name.clone())
        .meta(skill.source.clone())
        .tags(tags)
        .trailing(text::label(k, installs(skill.installs)))
        .selected(s.remote.as_ref().is_some_and(|r| r.id == skill.id))
        .build(k, move |s: &mut App| s.select_remote(pick.clone()))
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

/// What is under way (installing) and, when `hint` is given, the note under the actions.
fn status(s: &App, k: &Kit, hint: Option<El>) -> Option<El> {
    let busy = s.installing.as_ref().map(|job| ds::loading(k, format!("Installing {} into {}…", job.label, s.install_label(&job.scope))).key("installing"));
    if busy.is_none() && hint.is_none() {
        return None;
    }
    Some(col().w_full().gap(space::S2).children(busy).children(hint))
}

/// The last install's output, or why it failed.
fn log_card(s: &App, k: &Kit) -> Option<El> {
    let (title, out) = match s.install_log.as_ref()? {
        Ok(o) => ("Install log", o),
        Err(e) => ("Install failed", e),
    };
    Card::new("install-log").label(title).row(text::code(k, out.trim().to_string())).build(k)
}

/// The main action of the inspector: loading while an install runs.
fn install_button(s: &App, label: String) -> Button {
    let b = ds::primary(label, Place::Inspector).disabled(s.installing.is_some()).help(format!("Install into {}", s.install_label(s.local_scope())));
    if s.installing.is_some() { b.variant(ButtonVariant::Loading) } else { b }
}

fn details(s: &App, k: &Kit) -> El {
    if let Some(repo) = &s.repo {
        return repo_listing(s, repo, k);
    }
    let Some(r) = &s.remote else {
        if s.installing.is_none() && s.install_log.is_none() {
            return Inspector::empty(k, "No skill selected", "Pick a search result to preview and install it.");
        }
        let mut ins = Inspector::new();
        if let Some(n) = status(s, k, None) {
            ins = ins.notice(n);
        }
        return ins.card(log_card(s, k)).build(k);
    };
    let installed = s.installed_in(&r.name);
    let url = r.url();
    let hint = text::hint(
        k,
        format!(
            "Installs into {} and links it into your sync targets.",
            crate::agents::tilde(&crate::agents::AGENTS[crate::agents::CANONICAL].dir(s.local_scope(), s.local_paths()), s.local_paths())
        ),
    );
    let install = install_button(s, if installed { "Reinstall" } else { "Install" }.to_string()).on_press(|s: &mut App| s.install_remote()).build(k).key("install");
    let open = ds::button("Open on skills.sh", Place::Inspector)
        .on_press_tx(move |_: &mut App, tx: &mut Tx| {
            let _ = tx.open_url(&url);
        })
        .build(k)
        .key("open-web");
    let mut ins = Inspector::new().title(r.name.clone()).actions(vec![install, open], None);
    if let Some(n) = status(s, k, Some(hint)) {
        ins = ins.notice(n);
    }
    let details = Card::new("remote-details")
        .label("Details")
        .row(stacked_row(k, "Source", text::code(k, r.source.clone())))
        .row(pair_row(k, "Popularity", text::value(k, installs(r.installs))))
        .build(k);
    let md = Card::new("remote-md-card")
        .label("SKILL.md")
        .row(match &s.remote_md {
            None => ds::loading(k, "Loading SKILL.md…"),
            Some(Err(e)) => text::hint(k, e.clone()),
            Some(Ok(md)) => ds::markdown::<App>(k, crate::skill::parse(md).body).key("remote-md"),
        })
        .build(k);
    ins.card(log_card(s, k)).card(details).card(md).build(k)
}

/// A repository's skills with a tick each, and one button to install the ticked ones.
fn repo_listing(s: &App, repo: &RepoListing, k: &Kit) -> El {
    let mut ins = Inspector::new().title(repo.source.clone());
    let skills = match &repo.skills {
        None => return ins.notice(ds::loading(k, "Reading the repository…")).build(k),
        Some(Err(e)) => return ins.description(e.clone()).build(k),
        Some(Ok(v)) => v,
    };
    let picked = skills.iter().filter(|r| r.2).count();
    let label = match picked {
        0 => "Pick skills to install".to_string(),
        1 => "Install 1 skill".to_string(),
        n => format!("Install {n} skills"),
    };
    let install = install_button(s, label).disabled(picked == 0 || s.installing.is_some()).on_press(|s: &mut App| s.install_repo_ticked()).build(k).key("install-ticked");
    let close = ds::quiet("Close", Place::Inspector).on_press(|s: &mut App| s.repo = None).build(k).key("repo-close");
    ins = ins.actions(vec![install, close], None);
    if let Some(n) = status(s, k, None) {
        ins = ins.notice(n);
    }
    let rows = skills.iter().enumerate().map(|(i, (name, desc, on))| {
        let mut cb = Checkbox::new(*on).label(if s.installed_in(name) { format!("{name} (installed)") } else { name.clone() }).on_toggle(move |s: &mut App| s.toggle_repo_skill(i));
        if !desc.is_empty() {
            cb = cb.description(desc.clone());
        }
        cb.build(k).key(format!("repo-skill-{i}"))
    });
    ins.card(log_card(s, k)).card(Card::new("repo-skills").rows(rows).build(k)).build(k)
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
