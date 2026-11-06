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
    /// The open sync dialog's plan.
    pub sync_plan: Option<Plan>,
    /// The skill a "Remove" is waiting to be confirmed for.
    pub confirm_remove: Option<String>,
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
            sync_plan: None,
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
            self.open_sync();
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

    pub fn remove(&mut self, skill: &str) {
        self.confirm_remove = None;
        let result = sync::remove(&self.inv, &self.paths, skill);
        self.report(result, Some(format!("Removed {skill}; its folders are in the trash")));
        self.rescan();
    }

    pub fn open_sync(&mut self) {
        self.rescan();
        self.sync_plan = Some(sync::plan(&self.inv, &self.targets(), &self.paths));
    }

    pub fn apply_sync(&mut self) {
        let Some(plan) = self.sync_plan.take() else { return };
        let n = plan.actions.len();
        let result = sync::apply(&self.scope, &self.paths, &plan.actions);
        match result {
            Ok(_) => self.notify(format!("Synced: {n} change{}", if n == 1 { "" } else { "s" }), String::new(), false),
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
