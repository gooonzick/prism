//! The app's state and what its controls do. The view (`ui`) only reads this; each
//! submodule owns one capability's state changes.

mod across;
mod discover;
mod env;
mod installed;
mod mcp;
mod mcp_edit;
mod remote;
mod remote_actions;
mod watch;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use rune::{Latest, TextEdit, Waker};
use rune_kit::shell::PaneState;

use crate::agents::{self, AGENTS, CANONICAL, Paths, Scope};
use crate::fsops::Journal;
use crate::hosts::runner::{self as host_runner, MakeRunner};
use crate::mcp::scan::{self as mcp_scan, McpInventory, McpServer};
use crate::remote::RemoteSkill;
use crate::scan::{self, Inventory, ScopeStats};
use crate::store::{self, LastChange, Settings};
use crate::sync::Plan;

pub use across::{Across, Planned, Target};
pub use env::{AddHost, Env, PreviewAnswer, RemoteAnswer, RemoteState, Stash};
pub use installed::{open_path, reveal_path};
pub use mcp::McpFilter;
pub use mcp_edit::{McpChange, McpDialog, McpForm};
pub use remote::{RepoListing, UpdateJob};
pub use remote_actions::{HostAnswer, HostJob, HostPlan, HostProject};

#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    /// The skills installed in [`App::scope`].
    Installed,
    /// Searching skills.sh.
    Browse,
    /// The MCP servers in [`App::scope`].
    Mcp,
}

/// Which installed skills the list shows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Filter {
    #[default]
    All,
    /// Not synced into every target.
    NeedsSync,
    /// With a broken link somewhere.
    Broken,
    /// With a newer version on its source.
    Updates,
}

pub struct Notice {
    pub title: String,
    pub detail: String,
    pub error: bool,
    /// Offers to undo the change it reports.
    pub undo: bool,
    /// The machine that change was made on: "This Mac" or a host's name. The toast's Undo acts
    /// there, whichever machine is in view; `None` means the one in view.
    pub machine: Option<String>,
    /// That change, as `undo` names it: the toast's Undo takes it back only while it is still
    /// that machine's last change.
    pub label: Option<String>,
}

pub struct Install {
    pub label: String,
    pub scope: Scope,
}

pub type InstallResult = (Vec<String>, Result<String, String>);

/// A repository's skills (name, description), or why they couldn't be read.
pub type RepoAnswer = (String, Result<Vec<(String, String)>, String>);

/// The new-skill dialog's fields.
pub struct NewSkill {
    pub name: TextEdit,
    pub description: TextEdit,
    pub error: Option<String>,
}

/// Projects found under a folder, waiting to be picked.
pub struct Found {
    pub root: PathBuf,
    /// Folder, skill count, ticked.
    pub projects: Vec<(PathBuf, usize, bool)>,
}

pub struct App {
    pub paths: Paths,
    pub settings: Settings,
    pub sidebar: PaneState,
    pub inspector: PaneState,
    pub page: Page,
    pub scope: Scope,
    pub inv: Inventory,
    pub filter: TextEdit,
    pub status_filter: Filter,
    pub selected: Option<String>,
    /// The selected installed skill's `SKILL.md` body.
    pub preview: Option<String>,
    /// Skills ticked for a bulk sync or remove, in inventory order.
    pub checked: Vec<String>,
    /// The row a Shift-click extends from: the last one clicked without Shift.
    pub anchor: Option<String>,
    /// The inspector lists every agent, not only the relevant ones.
    pub show_all_agents: bool,
    /// The open sync dialog's plan.
    pub sync_plan: Option<Plan>,
    /// The open sync dialog's plan, made by the host in view.
    pub sync_host: Option<HostPlan>,
    /// The skills the open sync dialog is for; `None` syncs everything.
    pub sync_only: Option<Vec<String>>,
    /// The skills a "Remove" is waiting to be confirmed for.
    pub confirm_remove: Option<Vec<String>>,
    /// The skills the "Copy to" dialog is open for.
    pub copy_dialog: Option<Vec<String>>,
    pub new_skill: Option<NewSkill>,
    pub notice: Option<Notice>,
    /// When the notice went up, on the app clock.
    pub notice_at: Option<f64>,
    /// The last change, to undo: what it was, and its steps.
    pub last_change: Option<LastChange>,
    /// Skill count and unsynced count per scope.
    pub stats: HashMap<Scope, ScopeStats>,
    /// Names of the global skills, to mark project skills that shadow one.
    pub global_names: HashSet<String>,

    // MCP servers.
    pub mcp: McpInventory,
    pub mcp_selected: Option<String>,
    pub mcp_filter: McpFilter,
    /// Servers whose values the inspector shows unmasked.
    pub mcp_revealed: HashSet<String>,
    /// The open Add or Edit dialog.
    pub mcp_form: Option<McpForm>,
    /// The open plan dialog.
    pub mcp_dialog: Option<McpDialog>,

    // Environments.
    /// This machine, or the host in view.
    pub env: Env,
    /// This machine's state while a host is in view.
    pub local: Option<Stash>,
    /// The host's installed agents (skills, MCP), from its state.
    pub remote_detected: Option<(Vec<bool>, Vec<bool>)>,
    /// How the window reaches a host; tests hand out `Local` runners.
    pub make_runner: MakeRunner,
    pub remote_load: Latest<RemoteAnswer>,
    pub remote_preview: Latest<PreviewAnswer>,
    /// The connect in flight installs prism first.
    pub installing_prism: bool,
    /// The environment menu is open.
    pub env_menu: bool,
    pub add_host: Option<AddHost>,
    /// Changes waiting to run on the host in view, oldest first.
    pub host_jobs: VecDeque<HostJob>,
    /// The one running now, and the host it runs on: maybe one no longer in view.
    host_running: Option<(String, remote_actions::Work, remote_actions::Then)>,
    pub host_act: Latest<HostAnswer>,
    /// The label of the change `undo` would take back on the host in view.
    pub host_last_change: Option<String>,
    /// Servers on the host in view whose values are shown, unmasked.
    pub host_values: HashMap<String, McpServer>,
    /// Add project… on a host: the typed path.
    pub host_project: Option<HostProject>,
    /// Copies to another machine.
    pub across: Across,

    // Watching the scope's folders.
    waker: Option<Waker>,
    watcher: Option<notify::RecommendedWatcher>,
    dirty: Arc<AtomicBool>,

    // Updates from skills.sh.
    pub updates: HashMap<Scope, Vec<String>>,
    pub update_check: Latest<(Scope, Result<Vec<String>, String>)>,
    pub checking: HashSet<Scope>,
    pub checked_once: HashSet<Scope>,
    pub update_run: Latest<(UpdateJob, Result<String, String>)>,
    pub updating: Option<UpdateJob>,

    // Discovering projects.
    pub discover: Latest<(PathBuf, Vec<(PathBuf, usize)>)>,
    pub discovering: bool,
    pub found: Option<Found>,

    /// The menu's "Install Command Line Tool…": (title, detail, failed).
    pub cli_tool: Latest<(String, String, bool)>,

    // skills.sh.
    /// The install-scope menu is open.
    pub scope_menu: bool,
    pub query: TextEdit,
    pub search: Latest<Result<Vec<RemoteSkill>, String>>,
    pub results: Result<Vec<RemoteSkill>, String>,
    pub remote: Option<RemoteSkill>,
    pub remote_fetch: Latest<(String, Result<String, String>)>,
    pub remote_md: Option<Result<String, String>>,
    pub source: TextEdit,
    pub repo_fetch: Latest<RepoAnswer>,
    pub repo: Option<RepoListing>,
    pub install: Latest<InstallResult>,
    pub installing: Option<Install>,
    pub install_log: Option<Result<String, String>>,
}

impl App {
    pub fn new(paths: Paths, settings: Settings) -> App {
        let scope = Scope::Global;
        let inv = scan::scan(&scope, &paths);
        let mcp = mcp_scan::scan(&scope, &paths);
        let last_change = store::load_last_change(&paths);
        let mut app = App {
            paths,
            settings,
            sidebar: PaneState::new(248.0).range(180.0, 420.0),
            inspector: PaneState::new(380.0).range(300.0, 640.0).collapsed(),
            page: Page::Installed,
            scope,
            inv,
            filter: TextEdit::new(""),
            status_filter: Filter::All,
            selected: None,
            preview: None,
            checked: Vec::new(),
            anchor: None,
            show_all_agents: false,
            sync_plan: None,
            sync_host: None,
            sync_only: None,
            confirm_remove: None,
            copy_dialog: None,
            new_skill: None,
            notice: None,
            notice_at: None,
            last_change,
            stats: HashMap::new(),
            global_names: HashSet::new(),
            mcp,
            mcp_selected: None,
            mcp_filter: McpFilter::All,
            mcp_revealed: HashSet::new(),
            mcp_form: None,
            mcp_dialog: None,
            env: Env::Local,
            local: None,
            remote_detected: None,
            make_runner: host_runner::make_runner(),
            remote_load: Latest::default(),
            remote_preview: Latest::default(),
            installing_prism: false,
            env_menu: false,
            add_host: None,
            host_jobs: VecDeque::new(),
            host_running: None,
            host_act: Latest::default(),
            host_last_change: None,
            host_values: HashMap::new(),
            host_project: None,
            across: Across::default(),
            waker: None,
            watcher: None,
            dirty: Arc::new(AtomicBool::new(false)),
            updates: HashMap::new(),
            update_check: Latest::default(),
            checking: HashSet::new(),
            checked_once: HashSet::new(),
            update_run: Latest::default(),
            updating: None,
            discover: Latest::default(),
            discovering: false,
            found: None,
            cli_tool: Latest::default(),
            scope_menu: false,
            query: TextEdit::new(""),
            search: Latest::default().debounce(Duration::from_millis(250)),
            results: Ok(Vec::new()),
            remote: None,
            remote_fetch: Latest::default(),
            remote_md: None,
            source: TextEdit::new(""),
            repo_fetch: Latest::default(),
            repo: None,
            install: Latest::default(),
            installing: None,
            install_log: None,
        };
        // A project passed on the command line opens first.
        if let Some(dir) = rune::extra_args().iter().map(PathBuf::from).find(|p| p.is_dir()) {
            let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
            app.add_project(dir);
        }
        app.refresh_stats();
        app
    }

    /// Starts the background work a window needs: answers wake it, and the scope's folders
    /// are watched. Tests never call this, so they touch neither the network nor FSEvents.
    pub fn set_waker(&mut self, waker: Waker) {
        for w in [&self.search as &dyn SetWaker, &self.remote_fetch, &self.install, &self.update_check, &self.update_run, &self.discover, &self.repo_fetch, &self.cli_tool, &self.remote_load, &self.remote_preview, &self.host_act] {
            w.set(waker.clone());
        }
        self.across.set_waker(waker.clone());
        self.waker = Some(waker);
        self.watch();
        self.check_updates_once();
    }

    /// Lands background answers. True when something visible changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        // A host in view has no watcher; this machine is looked at again on the way back.
        if !self.is_remote() && self.take_dirty() {
            // Watch first: a file replaced by a rename, or a folder that appeared, needs a new
            // watch, and the rescan after it sees anything that landed in between.
            self.watch();
            // The CLI may have made or undone a change, or changed the projects or targets.
            self.last_change = store::load_last_change(&self.paths);
            let settings_changed = self.reload_settings();
            self.rescan();
            if settings_changed {
                self.refresh_stats();
            }
            changed = true;
        }
        if let Some(r) = self.search.take() {
            self.results = r;
            changed = true;
        }
        if let Some((id, md)) = self.remote_fetch.take()
            && self.remote.as_ref().is_some_and(|r| r.id == id)
        {
            self.remote_md = Some(md);
            changed = true;
        }
        if let Some((skills, result)) = self.install.take() {
            self.finish_install(skills, result);
            changed = true;
        }
        if let Some((scope, result)) = self.update_check.take() {
            self.land_update_check(scope, result);
            changed = true;
        }
        if let Some((job, result)) = self.update_run.take() {
            self.finish_update(job, result);
            changed = true;
        }
        if let Some((root, found)) = self.discover.take() {
            self.land_discovery(root, found);
            changed = true;
        }
        if let Some((source, result)) = self.repo_fetch.take() {
            self.land_repo(source, result);
            changed = true;
        }
        if let Some((title, detail, failed)) = self.cli_tool.take() {
            self.notify(self.here(title), detail, failed);
            changed = true;
        }
        if let Some(answer) = self.remote_load.take() {
            self.land_remote(answer);
            changed = true;
        }
        if let Some((name, body)) = self.remote_preview.take() {
            self.land_remote_preview(name, body);
            changed = true;
        }
        if let Some(answer) = self.host_act.take() {
            self.land_host(answer);
            changed = true;
        }
        changed |= self.poll_across();
        changed
    }

    /// The first half of `prism setup`, off the main thread: it asks the login shell for its
    /// `PATH`, since an app opened from the Finder doesn't get the one with `~/.local/bin`.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn install_cli_tool(&mut self) {
        let paths = self.paths.clone();
        self.cli_tool.request((), move |()| {
            use crate::cli::setup;
            let path = setup::login_path().or_else(|| std::env::var_os("PATH"));
            let r = setup::cli_part(&paths.home, path.as_deref());
            let at = r.path.as_deref().map(|p| crate::agents::tilde(p, &paths)).unwrap_or_default();
            match r.status {
                setup::Status::Unchanged => ("The command line tool is installed".into(), at, false),
                s if s.ok() => ("Installed the command line tool".into(), at, false),
                _ => ("Command line tool not installed".into(), r.message, true),
            }
        });
    }

    // Scopes and projects.

    /// Opens `scope` on the page that's open (skills or MCP); from skills.sh, on its skills.
    pub fn open_scope(&mut self, scope: Scope) {
        if self.page == Page::Browse {
            self.page = Page::Installed;
        }
        if self.is_remote() {
            // The scope changes when the host's state for it lands.
            self.connect_remote(scope);
            return;
        }
        if scope != self.scope {
            self.scope = scope;
            self.reset_view();
            self.watch();
            self.check_updates_once();
        }
        self.rescan();
    }

    /// Clears the selections and filters, for another scope or machine.
    fn reset_view(&mut self) {
        self.selected = None;
        self.preview = None;
        self.checked.clear();
        self.anchor = None;
        self.status_filter = Filter::All;
        self.filter = TextEdit::new("");
        self.mcp_selected = None;
        self.mcp_filter = McpFilter::All;
        self.mcp_revealed.clear();
        self.host_values.clear();
    }

    /// The settings as on disk, where the CLI may have changed them. Settings built in memory
    /// (tests) have no disk to come from. True when they changed.
    fn reload_settings(&mut self) -> bool {
        if !self.settings.persist {
            return false;
        }
        let disk = Settings::load_from(&self.paths);
        let changed = disk != self.settings;
        self.settings = disk;
        changed
    }

    /// Changes the settings as they are on disk now and saves them, so a change the CLI made
    /// since the window last looked isn't written over.
    pub(super) fn edit_settings<R>(&mut self, change: impl FnOnce(&mut Settings) -> R) -> R {
        let _ = self.reload_settings();
        let r = change(&mut self.settings);
        self.settings.save_to(&self.paths);
        r
    }

    pub fn add_project(&mut self, dir: PathBuf) {
        if self.is_remote() {
            return self.host_add_project(dir);
        }
        self.edit_settings(|s| s.add_project(dir.clone()));
        self.open_scope(Scope::Project(dir));
        self.refresh_stats();
    }

    pub fn forget_project(&mut self, dir: &PathBuf) {
        if self.is_remote() {
            return self.host_forget_project(dir);
        }
        self.edit_settings(|s| s.projects.retain(|p| p != dir));
        self.stats.remove(&Scope::Project(dir.clone()));
        if self.scope == Scope::Project(dir.clone()) {
            self.open_scope(Scope::Global);
        }
    }

    /// Reads the scope again: from disk, or from the host in view.
    pub fn rescan(&mut self) {
        if self.is_remote() {
            self.load_remote(self.scope.clone(), false);
            return;
        }
        self.inv = scan::scan(&self.scope, &self.paths);
        if self.selected.as_deref().is_some_and(|n| self.inv.get(n).is_none()) {
            self.selected = None;
        }
        let inv = &self.inv;
        self.checked.retain(|n| inv.get(n).is_some());
        let stats = self.stats_of(&self.inv);
        self.stats.insert(self.scope.clone(), stats);
        if self.scope.is_global() {
            self.global_names = self.inv.skills.iter().map(|s| s.name.clone()).collect();
        }
        self.load_preview();
        self.rescan_mcp();
    }

    fn stats_of(&self, inv: &Inventory) -> ScopeStats {
        scan::stats_of(inv, &self.targets())
    }

    /// Scans every known scope for the sidebar's counts.
    pub fn refresh_stats(&mut self) {
        // A host's counts come with its state.
        if self.is_remote() {
            return;
        }
        let mut scopes = vec![Scope::Global];
        scopes.extend(self.settings.projects.iter().cloned().map(Scope::Project));
        for scope in scopes {
            let inv = if scope == self.scope { self.inv.clone() } else { scan::scan(&scope, &self.paths) };
            if scope.is_global() {
                self.global_names = inv.skills.iter().map(|s| s.name.clone()).collect();
            }
            let stats = self.stats_of(&inv);
            self.stats.insert(scope, stats);
        }
    }

    /// Agent indices a sync links into: the chosen ones, or those installed on the machine in view.
    pub fn targets(&self) -> Vec<usize> {
        match self.settings.targets.as_deref() {
            Some(ids) => agents::target_indices(Some(ids), &self.paths),
            None => (0..AGENTS.len()).filter(|&a| a != CANONICAL && self.detected(a)).collect(),
        }
    }

    pub fn scope_label(&self, scope: &Scope) -> String {
        crate::sync::label::scope(scope, &self.paths)
    }

    /// Every scope but the current one: where skills can be copied to.
    pub fn other_scopes(&self) -> Vec<Scope> {
        let mut v = vec![Scope::Global];
        v.extend(self.settings.projects.iter().cloned().map(Scope::Project));
        v.retain(|s| s != &self.scope);
        v
    }

    // Notices and undo.

    pub fn notify(&mut self, title: String, detail: String, error: bool) {
        self.notice = Some(Notice { title, detail, error, undo: false, machine: None, label: None });
        self.notice_at = None;
    }

    /// The toast up now offers to undo the change `label` on `machine`.
    pub(super) fn offer_undo(&mut self, machine: String, label: Option<String>) {
        if let Some(n) = &mut self.notice {
            n.undo = true;
            n.machine = Some(machine);
            n.label = label;
        }
    }

    /// A journal for a new change. The last change's config backups are not needed any more,
    /// so they go now, before this change makes its own.
    pub fn new_journal(&self) -> Journal {
        store::begin_change(&self.paths);
        Journal::default()
    }

    /// Reports a change made through `j`, keeping it for Undo when it changed anything.
    fn changed(&mut self, label: String, j: Journal, result: Result<String, String>) {
        let undoable = !j.is_empty();
        if undoable {
            self.last_change = Some(LastChange { label: label.clone(), journal: j });
            self.save_last_change();
        }
        match result {
            Ok(title) => self.notify(title, String::new(), false),
            Err(e) => {
                let e = if undoable { format!("{e}\nUndo takes back what was done.") } else { e };
                self.notify("Couldn't finish that".into(), e, true);
            }
        }
        if undoable {
            self.offer_undo(crate::hosts::copy::this_machine().to_string(), Some(label));
        }
        self.rescan();
    }

    fn save_last_change(&self) {
        store::save_last_change(&self.paths, self.last_change.as_ref());
    }

    /// Takes back the last change of the machine in view.
    pub fn undo(&mut self) {
        if self.is_remote() {
            return self.host_undo(None);
        }
        self.undo_here(None);
    }

    /// Takes back this machine's last change, in view; with `expect`, only if it is still that
    /// change (a toast's Undo).
    pub(super) fn undo_here(&mut self, expect: Option<String>) {
        // From disk: the CLI may have undone it, or made a newer change, since this window did.
        self.last_change = store::load_last_change(&self.paths);
        if let Some(want) = expect.filter(|want| self.last_change.as_ref().is_none_or(|c| c.label != *want)) {
            return self.notify("Couldn't undo".into(), format!("{want} is no longer the last change; nothing was undone"), true);
        }
        let Some(LastChange { label, journal }) = self.last_change.take() else { return };
        match journal.undo(&self.paths) {
            Ok(_) => {
                store::drop_backups(&self.paths);
                self.notify(format!("Undid: {label}"), String::new(), false);
            }
            Err((e, left)) => {
                self.notify(format!("Couldn't fully undo: {label}"), e, true);
                self.last_change = Some(LastChange { label, journal: left });
            }
        }
        self.save_last_change();
        self.rescan();
        self.refresh_stats();
    }
}

/// [`Latest::set_waker`] over answers of any type.
trait SetWaker {
    fn set(&self, waker: Waker);
}

impl<T: Send + 'static> SetWaker for Latest<T> {
    fn set(&self, waker: Waker) {
        self.set_waker(waker);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_failure_points_at_undo() {
        let home = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(home.path()).unwrap();
        let mut app = App::new(Paths::isolated(&root), Settings::default());
        let mut j = app.new_journal();
        j.write_file(&root.join("a.json"), b"1", &app.paths).unwrap();
        app.changed("edit".into(), j, Err("Stopped after 1 of 2 files: can't write b.json".into()));
        let notice = app.notice.as_ref().unwrap();
        assert!(notice.undo && notice.error);
        assert!(notice.detail.ends_with("Undo takes back what was done."), "{}", notice.detail);

        app.changed("edit".into(), Journal::default(), Err("can't write a.json".into()));
        assert_eq!(app.notice.as_ref().unwrap().detail, "can't write a.json", "nothing to undo, no hint");
    }
}
