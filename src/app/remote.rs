//! skills.sh from the app: searching, previewing, listing a repo, installing and updating.

use super::{App, Install, Page};
use crate::agents::{AGENTS, CANONICAL, Scope};
use crate::remote::{self, RemoteSkill};
use crate::sync;

/// A skill in a repository listing: name, description, ticked.
pub type RepoSkill = (String, String, bool);

/// A repository's skills, as `skills add -l` lists them, for picking which to install.
pub struct RepoListing {
    pub source: String,
    /// Name, description, ticked. `None` while it loads.
    pub skills: Option<Result<Vec<RepoSkill>, String>>,
}

/// An update in flight: the scope and the skills.
#[derive(Clone, Debug, PartialEq)]
pub struct UpdateJob {
    pub scope: Scope,
    pub skills: Vec<String>,
}

impl App {
    pub fn open_browse(&mut self) {
        self.page = Page::Browse;
        self.query.focused = true;
    }

    pub fn search_changed(&mut self) {
        let q = self.query.text.clone();
        self.search.request(q, |q| remote::search(&q));
    }

    pub fn select_remote(&mut self, skill: RemoteSkill) {
        self.repo = None;
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

    // Installing.

    pub fn install_remote(&mut self) {
        let Some(r) = self.remote.clone() else { return };
        self.start_install(r.source.clone(), vec![r.skill_arg().to_string()], r.name.clone());
    }

    /// Installs straight from the source field: the skill it names, or all of the repo.
    pub fn install_source(&mut self) {
        let Some((source, skill)) = remote::parse_source(&self.source.text) else { return };
        let label = skill.clone().map_or_else(|| source.clone(), |s| format!("{source}@{s}"));
        self.start_install(source, skill.into_iter().collect(), label);
    }

    /// Lists the skills in the source field's repository, to pick from.
    pub fn list_repo(&mut self) {
        let Some((source, _)) = remote::parse_source(&self.source.text) else { return };
        self.repo = Some(RepoListing { source: source.clone(), skills: None });
        self.remote = None;
        if !self.inspector.is_open() {
            self.inspector.toggle();
        }
        self.repo_fetch.request(source, |src| {
            let r = remote::list_repo(&src);
            (src, r)
        });
    }

    pub(super) fn land_repo(&mut self, source: String, result: Result<Vec<(String, String)>, String>) {
        if let Some(repo) = &mut self.repo
            && repo.source == source
        {
            repo.skills = Some(result.map(|v| v.into_iter().map(|(n, d)| (n, d, false)).collect()));
        }
    }

    pub fn toggle_repo_skill(&mut self, i: usize) {
        if let Some(RepoListing { skills: Some(Ok(v)), .. }) = &mut self.repo
            && let Some(row) = v.get_mut(i)
        {
            row.2 = !row.2;
        }
    }

    pub fn install_repo_ticked(&mut self) {
        let Some(RepoListing { source, skills: Some(Ok(v)) }) = &self.repo else { return };
        let picked: Vec<String> = v.iter().filter(|r| r.2).map(|r| r.0.clone()).collect();
        if picked.is_empty() {
            return;
        }
        let label = if picked.len() == 1 { format!("{source}@{}", picked[0]) } else { format!("{} skills from {source}", picked.len()) };
        let source = source.clone();
        self.start_install(source, picked, label);
    }

    fn start_install(&mut self, source: String, skills: Vec<String>, label: String) {
        if self.installing.is_some() {
            return;
        }
        let (scope, paths) = (self.scope.clone(), self.paths.clone());
        self.install_log = None;
        self.installing = Some(Install { label, scope: scope.clone() });
        self.install.request((source, skills), move |(source, skills)| {
            let r = remote::install(&source, &skills, &scope, &paths);
            (skills, r)
        });
    }

    pub(super) fn finish_install(&mut self, skills: Vec<String>, result: Result<String, String>) {
        let Some(job) = self.installing.take() else { return };
        let ok = result.is_ok();
        self.install_log = Some(result);
        if !ok {
            self.notify(format!("Installing {} failed", job.label), "The log is in the details pane".into(), true);
            return;
        }
        let detail = match sync::link_installed(&job.scope, &self.paths, &self.targets(), &skills) {
            Ok(n) => format!("Into {} · {} link{}", self.scope_label(&job.scope), n, if n == 1 { "" } else { "s" }),
            Err(e) => e,
        };
        self.notify(format!("Installed {}", job.label), detail, false);
        if job.scope == self.scope {
            self.rescan();
        }
        self.refresh_stats();
    }

    // Updates.

    pub fn has_update(&self, name: &str) -> bool {
        self.updates.get(&self.scope).is_some_and(|v| v.iter().any(|n| n == name))
    }


    /// Checks the current scope for updates once a session, in the background. Without Node
    /// there is nothing to check with; that only gets reported when asked for.
    pub(super) fn check_updates_once(&mut self) {
        if self.waker.is_some() && !self.checked_once.contains(&self.scope) && remote::runner().is_some() {
            self.check_updates();
        }
    }

    pub fn check_updates(&mut self) {
        let (scope, paths) = (self.scope.clone(), self.paths.clone());
        self.checked_once.insert(scope.clone());
        self.checking.insert(scope.clone());
        self.update_check.request(scope, move |scope| {
            let r = remote::check_updates(&scope, &paths);
            (scope, r)
        });
    }

    pub(super) fn land_update_check(&mut self, scope: Scope, result: Result<Vec<String>, String>) {
        self.checking.remove(&scope);
        match result {
            Ok(names) => {
                self.updates.insert(scope, names);
            }
            Err(e) if scope == self.scope => self.notify("Couldn't check for updates".into(), e, true),
            Err(_) => {}
        }
    }

    /// Updates `names` (or every skill with an update when empty) in the current scope.
    pub fn update(&mut self, names: Vec<String>) {
        if self.updating.is_some() {
            return;
        }
        let names = if names.is_empty() { self.updates.get(&self.scope).cloned().unwrap_or_default() } else { names };
        if names.is_empty() {
            return;
        }
        let job = UpdateJob { scope: self.scope.clone(), skills: names };
        let paths = self.paths.clone();
        self.updating = Some(job.clone());
        self.update_run.request(job, move |job| {
            let r = remote::update(&job.skills, &job.scope, &paths);
            (job, r)
        });
    }

    pub(super) fn finish_update(&mut self, job: UpdateJob, result: Result<String, String>) {
        self.updating = None;
        match result {
            Ok(_) => {
                if let Some(v) = self.updates.get_mut(&job.scope) {
                    v.retain(|n| !job.skills.contains(n));
                }
                let what = if job.skills.len() == 1 { job.skills[0].clone() } else { format!("{} skills", job.skills.len()) };
                self.notify(format!("Updated {what}"), String::new(), false);
            }
            Err(e) => self.notify("Update failed".into(), e.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").to_string(), true),
        }
        if job.scope == self.scope {
            self.rescan();
        }
    }
}
