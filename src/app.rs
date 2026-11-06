//! The app's state and what its controls do. The view (`ui`) only reads this.

use std::path::PathBuf;
use std::time::Duration;

use rune::{Latest, TextEdit, Waker};
use rune_kit::shell::PaneState;

use crate::agents::{self, AGENTS, CANONICAL, Paths, Scope};
use crate::remote::{self, RemoteSkill};
use crate::scan::{self, Inventory, Placement};
use crate::store::Settings;
use crate::sync::{self, Plan};

#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    /// The skills installed in [`App::scope`].
    Installed,
    /// Searching skills.sh.
    Browse,
}

pub struct Notice {
    pub title: String,
    pub detail: String,
    pub error: bool,
}

pub struct Install {
    pub label: String,
    pub scope: Scope,
}

pub type InstallResult = (Option<String>, Result<String, String>);

pub struct App {
    pub paths: Paths,
    pub settings: Settings,
    pub sidebar: PaneState,
    pub inspector: PaneState,
    pub page: Page,
    pub scope: Scope,
    pub inv: Inventory,
    pub filter: TextEdit,
    pub selected: Option<String>,
    /// The selected installed skill's `SKILL.md` body.
    pub preview: Option<String>,
    /// Skills ticked for a bulk sync or remove, in inventory order.
    pub checked: Vec<String>,
    /// The inspector lists every agent, not only the relevant ones.
    pub show_all_agents: bool,
    /// The row a Shift-click extends from: the last one clicked without Shift.
    pub anchor: Option<String>,
    /// The open sync dialog's plan.
    pub sync_plan: Option<Plan>,
    /// The skills the open sync dialog is for; `None` syncs everything.
    pub sync_only: Option<Vec<String>>,
    /// The skills a "Remove" is waiting to be confirmed for.
    pub confirm_remove: Option<Vec<String>>,
    pub notice: Option<Notice>,
    /// When the notice went up, on the app clock.
    pub notice_at: Option<f64>,
    /// The install-scope menu is open.
    pub scope_menu: bool,

    pub query: TextEdit,
    pub search: Latest<Result<Vec<RemoteSkill>, String>>,
    pub results: Result<Vec<RemoteSkill>, String>,
    pub remote: Option<RemoteSkill>,
    pub remote_fetch: Latest<(String, Result<String, String>)>,
    pub remote_md: Option<Result<String, String>>,
    pub source: TextEdit,
    pub install: Latest<InstallResult>,
    pub installing: Option<Install>,
    pub install_log: Option<Result<String, String>>,
}

impl App {
    pub fn new(paths: Paths, settings: Settings) -> App {
        let scope = Scope::Global;
        let inv = scan::scan(&scope, &paths);
        let mut app = App {
            paths,
            settings,
            sidebar: PaneState::new(248.0).range(180.0, 420.0),
            inspector: PaneState::new(380.0).range(300.0, 640.0).collapsed(),
            page: Page::Installed,
            scope,
            inv,
            filter: TextEdit::new(""),
            selected: None,
            preview: None,
            checked: Vec::new(),
            anchor: None,
            show_all_agents: false,
            sync_plan: None,
            sync_only: None,
            confirm_remove: None,
            notice: None,
            notice_at: None,
            scope_menu: false,
            query: TextEdit::new(""),
            search: Latest::default().debounce(Duration::from_millis(250)),
            results: Ok(Vec::new()),
            remote: None,
            remote_fetch: Latest::default(),
            remote_md: None,
            source: TextEdit::new(""),
            install: Latest::default(),
            installing: None,
            install_log: None,
        };
        // Projects passed on the command line open first.
        if let Some(dir) = rune::extra_args().iter().map(PathBuf::from).find(|p| p.is_dir()) {
            let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
            app.add_project(dir);
        }
        app
    }

    pub fn set_waker(&self, waker: Waker) {
        self.search.set_waker(waker.clone());
        self.remote_fetch.set_waker(waker.clone());
        self.install.set_waker(waker);
    }

    /// Lands background answers. True when something visible changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(r) = self.search.take() {
            self.results = r;
            changed = true;
        }
        if let Some((id, md)) = self.remote_fetch.take()
            && self.remote.as_ref().is_some_and(|r| r.id == id) {
                self.remote_md = Some(md);
                changed = true;
            }
        if let Some((skill, result)) = self.install.take() {
            self.finish_install(skill, result);
            changed = true;
        }
        changed
    }

    // Scopes and projects.

    pub fn open_scope(&mut self, scope: Scope) {
        self.page = Page::Installed;
        if scope != self.scope {
            self.scope = scope;
            self.selected = None;
            self.preview = None;
            self.checked.clear();
            self.anchor = None;
            self.filter = TextEdit::new("");
        }
        self.rescan();
    }

    pub fn add_project(&mut self, dir: PathBuf) {
        if self.settings.add_project(dir.clone()) {
            self.settings.save();
        }
        self.open_scope(Scope::Project(dir));
    }

    pub fn forget_project(&mut self, dir: &PathBuf) {
        self.settings.projects.retain(|p| p != dir);
        self.settings.save();
        if self.scope == Scope::Project(dir.clone()) {
            self.open_scope(Scope::Global);
        }
    }

    pub fn rescan(&mut self) {
        self.inv = scan::scan(&self.scope, &self.paths);
        if self.selected.as_deref().is_some_and(|n| self.inv.get(n).is_none()) {
            self.selected = None;
        }
        let inv = &self.inv;
        self.checked.retain(|n| inv.get(n).is_some());
        self.load_preview();
    }

    // Installed skills.

    /// Agent indices a sync links into.
    pub fn targets(&self) -> Vec<usize> {
        let ids = self.settings.targets.clone().unwrap_or_else(|| agents::default_targets(&self.paths));
        ids.iter().filter_map(|id| agents::index_of(id)).filter(|&i| i != CANONICAL).collect()
    }

    pub fn toggle_target(&mut self, agent: usize) {
        let mut t = self.targets();
        if let Some(i) = t.iter().position(|&a| a == agent) {
            t.remove(i);
        } else {
            t.push(agent);
            t.sort();
        }
        self.settings.targets = Some(t.iter().map(|&a| AGENTS[a].id.to_string()).collect());
        self.settings.save();
        if self.sync_plan.is_some() {
            self.open_sync_for(self.sync_only.clone());
        }
    }

    /// The installed skills matching the filter.
    pub fn visible(&self) -> Vec<usize> {
        let q = self.filter.text.trim().to_lowercase();
        (0..self.inv.skills.len())
            .filter(|&i| {
                let s = &self.inv.skills[i];
                q.is_empty() || s.name.to_lowercase().contains(&q) || s.description.to_lowercase().contains(&q)
            })
            .collect()
    }

    pub fn select(&mut self, name: &str) {
        self.selected = Some(name.to_string());
        self.load_preview();
        if !self.inspector.is_open() {
            self.inspector.toggle();
        }
    }

    fn load_preview(&mut self) {
        self.preview = self.selected.as_deref().and_then(|n| self.inv.get(n)).and_then(|s| s.manifest()).map(|m| m.body);
    }

    /// Links or unlinks `skill` for `agent`, from its switch.
    pub fn toggle_link(&mut self, skill: &str, agent: usize) {
        let Some(s) = self.inv.get(skill) else { return };
        let result = match &s.placements[agent] {
            Placement::Linked | Placement::Link(_) | Placement::Broken(_) => sync::unlink(&self.scope, &self.paths, skill, agent),
            Placement::Missing => sync::link(&self.inv, &self.paths, skill, agent),
            Placement::Dir => Err(format!("{} has its own folder; use \"Replace with link\" or move it to the trash", AGENTS[agent].name)),
        };
        self.report(result, None);
        self.rescan();
    }

    /// Replaces `agent`'s own copy with a link to `.agents` (adopting it if `.agents` lacks it).
    pub fn replace_with_link(&mut self, skill: &str, agent: usize) {
        let plan = sync::plan_one(&self.inv, skill, &[agent], &self.paths);
        let result = match plan.conflicts.first() {
            Some(c) => Err(c.reason.clone()),
            None => sync::apply(&self.scope, &self.paths, &plan.actions).map(|_| ()).map_err(|(_, e)| e),
        };
        self.report(result, Some(format!("{skill} now links to .agents")));
        self.rescan();
    }

    pub fn trash_copy(&mut self, skill: &str, agent: usize) {
        let result = sync::trash_copy(&self.scope, &self.paths, skill, agent);
        self.report(result, Some(format!("Moved {}'s copy of {skill} to the trash", AGENTS[agent].name)));
        self.rescan();
    }

    /// Removes every skill in `names`, going on past failures.
    pub fn remove(&mut self, names: &[String]) {
        self.confirm_remove = None;
        let mut failed = Vec::new();
        for name in names {
            if let Err(e) = sync::remove(&self.inv, &self.paths, name) {
                failed.push(e);
            }
        }
        let removed = names.len() - failed.len();
        match (failed.is_empty(), names) {
            (true, [one]) => self.notify(format!("Removed {one}"), "Its folders are in the trash".into(), false),
            (true, _) => self.notify(format!("Removed {removed} skills"), "Their folders are in the trash".into(), false),
            (false, _) => self.notify(format!("Removed {removed} of {}", names.len()), failed.join("\n"), true),
        }
        self.checked.retain(|n| !names.contains(n));
        self.rescan();
    }

    // Multi-select.

    pub fn is_checked(&self, name: &str) -> bool {
        self.checked.iter().any(|n| n == name)
    }

    pub fn toggle_checked(&mut self, name: &str) {
        if let Some(i) = self.checked.iter().position(|n| n == name) {
            self.checked.remove(i);
        } else {
            self.tick([name.to_string()]);
        }
    }

    fn tick(&mut self, names: impl IntoIterator<Item = String>) {
        for n in names {
            if !self.is_checked(&n) {
                self.checked.push(n);
            }
        }
        let order: Vec<&str> = self.inv.skills.iter().map(|s| s.name.as_str()).collect();
        self.checked.sort_by_key(|n| order.iter().position(|o| o == n));
    }

    /// A click on a row: Cmd toggles its tick, Shift ticks every shown row from the anchor
    /// to it, and a plain click opens it in the inspector.
    pub fn click_row(&mut self, name: &str, mods: rune::Mods) {
        if mods.shift {
            self.tick_range(name);
        } else if mods.cmd {
            self.toggle_checked(name);
            self.anchor = Some(name.to_string());
        } else {
            self.anchor = Some(name.to_string());
            self.select(name);
        }
    }

    /// A click on a row's checkbox: Shift ticks the range, otherwise it toggles.
    pub fn click_tick(&mut self, name: &str, mods: rune::Mods) {
        if mods.shift {
            self.tick_range(name);
        } else {
            self.toggle_checked(name);
            self.anchor = Some(name.to_string());
        }
    }

    /// Ticks the shown rows between the anchor and `name`, both included. Without an anchor
    /// still on screen, just `name`.
    fn tick_range(&mut self, name: &str) {
        let shown: Vec<String> = self.visible().into_iter().map(|i| self.inv.skills[i].name.clone()).collect();
        let Some(to) = shown.iter().position(|n| n == name) else { return };
        let from = self.anchor.as_ref().and_then(|a| shown.iter().position(|n| n == a)).unwrap_or(to);
        let (lo, hi) = (from.min(to), from.max(to));
        self.tick(shown[lo..=hi].iter().cloned());
        if self.anchor.is_none() {
            self.anchor = Some(name.to_string());
        }
    }

    /// Ticks every skill the filter shows, or clears them all when they already are.
    pub fn toggle_all_visible(&mut self) {
        let visible: Vec<String> = self.visible().into_iter().map(|i| self.inv.skills[i].name.clone()).collect();
        if !visible.is_empty() && visible.iter().all(|n| self.is_checked(n)) {
            self.checked.retain(|n| !visible.contains(n));
        } else {
            self.tick(visible);
        }
    }

    pub fn clear_checked(&mut self) {
        self.checked.clear();
    }

    pub fn confirm_remove_checked(&mut self) {
        if !self.checked.is_empty() {
            self.confirm_remove = Some(self.checked.clone());
        }
    }

    // Sync.

    pub fn open_sync(&mut self) {
        self.open_sync_for(None);
    }

    pub fn open_sync_checked(&mut self) {
        if !self.checked.is_empty() {
            self.open_sync_for(Some(self.checked.clone()));
        }
    }

    fn open_sync_for(&mut self, only: Option<Vec<String>>) {
        self.rescan();
        let targets = self.targets();
        self.sync_plan = Some(match &only {
            Some(names) => sync::plan_many(&self.inv, names, &targets, &self.paths),
            None => sync::plan(&self.inv, &targets, &self.paths),
        });
        self.sync_only = only;
    }

    pub fn close_sync(&mut self) {
        self.sync_plan = None;
        self.sync_only = None;
    }

    pub fn apply_sync(&mut self) {
        let Some(plan) = self.sync_plan.take() else { return };
        let only = self.sync_only.take();
        let n = plan.actions.len();
        let result = sync::apply(&self.scope, &self.paths, &plan.actions);
        match result {
            Ok(_) => {
                self.notify(format!("Synced: {n} change{}", if n == 1 { "" } else { "s" }), String::new(), false);
                if only.is_some() {
                    self.checked.clear();
                }
            }
            Err((done, e)) => self.notify(format!("Sync stopped after {done} of {n}"), e, true),
        }
        self.rescan();
    }

    fn report(&mut self, result: Result<(), String>, ok: Option<String>) {
        match result {
            Ok(()) => {
                if let Some(t) = ok {
                    self.notify(t, String::new(), false);
                }
            }
            Err(e) => self.notify("Couldn't do that".into(), e, true),
        }
    }

    pub fn notify(&mut self, title: String, detail: String, error: bool) {
        self.notice = Some(Notice { title, detail, error });
        self.notice_at = None;
    }

    // skills.sh.

    pub fn open_browse(&mut self) {
        self.page = Page::Browse;
        self.query.focused = true;
    }

    pub fn search_changed(&mut self) {
        let q = self.query.text.clone();
        self.search.request(q, |q| remote::search(&q));
    }

    pub fn select_remote(&mut self, skill: RemoteSkill) {
        self.remote_md = None;
        self.install_log = None;
        let s = skill.clone();
        self.remote_fetch.request(s, |s| (s.id.clone(), remote::fetch_manifest(&s)));
        self.remote = Some(skill);
        if !self.inspector.is_open() {
            self.inspector.toggle();
        }
    }

    /// Whether `name` is already in `scope`'s `.agents`.
    pub fn installed_in(&self, name: &str) -> bool {
        AGENTS[CANONICAL].dir(&self.scope, &self.paths).join(name).exists()
    }

    pub fn install_remote(&mut self) {
        let Some(r) = self.remote.clone() else { return };
        self.start_install(r.source.clone(), Some(r.skill_arg().to_string()), r.name.clone());
    }

    pub fn install_source(&mut self) {
        let Some((source, skill)) = remote::parse_source(&self.source.text) else { return };
        let label = skill.clone().map_or_else(|| source.clone(), |s| format!("{source}@{s}"));
        self.start_install(source, skill, label);
    }

    fn start_install(&mut self, source: String, skill: Option<String>, label: String) {
        if self.installing.is_some() {
            return;
        }
        let (scope, paths) = (self.scope.clone(), self.paths.clone());
        self.install_log = None;
        self.installing = Some(Install { label, scope: scope.clone() });
        self.install.request((source, skill), move |(source, skill)| {
            let r = remote::install(&source, skill.as_deref(), &scope, &paths);
            (skill, r)
        });
    }

    fn finish_install(&mut self, skill: Option<String>, result: Result<String, String>) {
        let Some(job) = self.installing.take() else { return };
        let ok = result.is_ok();
        self.install_log = Some(result);
        if !ok {
            self.notify(format!("Installing {} failed", job.label), "See the log in the details pane".into(), true);
            return;
        }
        // Link what was installed into the target agents.
        let inv = scan::scan(&job.scope, &self.paths);
        let names: Vec<String> = match skill.as_deref() {
            Some(s) if s != "*" => vec![s.to_string()],
            _ => inv.skills.iter().filter(|s| s.canonical().present()).map(|s| s.name.clone()).collect(),
        };
        let mut plan = Plan::default();
        for n in &names {
            let p = sync::plan_one(&inv, n, &self.targets(), &self.paths);
            plan.actions.extend(p.actions.into_iter().filter(|a| matches!(a, sync::Action::Link { .. } | sync::Action::Replace { .. })));
        }
        let linked = sync::apply(&job.scope, &self.paths, &plan.actions);
        let detail = match linked {
            Ok(n) => format!("Into {} · {n} link{}", self.scope_label(&job.scope), if n == 1 { "" } else { "s" }),
            Err((_, e)) => e,
        };
        self.notify(format!("Installed {}", job.label), detail, false);
        if job.scope == self.scope {
            self.rescan();
        }
    }

    pub fn scope_label(&self, scope: &Scope) -> String {
        match scope {
            Scope::Global => "Global (~)".into(),
            Scope::Project(p) => agents::tilde(p, &self.paths),
        }
    }
}
