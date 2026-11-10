//! Environments: this machine, or a saved host whose state the window shows. A host's state
//! comes from its own `prism wire state`, read off the UI thread; while it is in view this
//! machine's state waits in [`App::local`], and nothing in the window reads or writes this
//! machine's disk with the host's paths.

use std::collections::{HashMap, HashSet};

use rune::TextEdit;

use super::App;
use crate::agents::{AGENTS, Paths, Scope};
use crate::hosts::install::{self, Health, VERSION};
use crate::hosts::runner::Runner;
use crate::hosts::{self, Host};
use crate::mcp::agents::{Flavor, MCP_AGENTS};
use crate::mcp::scan::McpInventory;
use crate::scan::{Inventory, ScopeStats};
use crate::store::{self, LastChange, Settings};
use crate::wire::WireState;

/// The machine the window shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Env {
    Local,
    Remote { host: Host, state: RemoteState },
}

/// How far the window got with a host.
#[derive(Clone, Debug, PartialEq)]
pub enum RemoteState {
    Connecting,
    /// Reached, but its `prism` is missing, another version, or can't run there.
    NeedsInstall(Health),
    /// Why it couldn't be reached or read.
    Unreachable(String),
    Ready,
}

/// This machine's state while a host is in view.
pub struct Stash {
    pub paths: Paths,
    pub settings: Settings,
    pub scope: Scope,
    pub inv: Inventory,
    pub mcp: McpInventory,
    pub stats: HashMap<Scope, ScopeStats>,
    pub global_names: HashSet<String>,
    pub last_change: Option<LastChange>,
}

/// The Add host dialog's fields.
pub struct AddHost {
    pub name: TextEdit,
    pub ssh: TextEdit,
    pub error: Option<String>,
}

/// A host's answer to a load.
pub struct RemoteAnswer {
    /// The host it came from.
    pub host: String,
    /// Its state, or why there is none.
    pub result: Result<Box<WireState>, RemoteState>,
    /// The load installed prism first.
    pub installed: bool,
    /// Why that install failed.
    pub install_error: Option<String>,
}

/// A skill's SKILL.md body on a host: the skill's name and the body.
pub type PreviewAnswer = (String, Option<String>);

/// An inventory with nothing in it, the right shape for the view.
fn empty_inv(scope: Scope) -> Inventory {
    Inventory { scope, skills: Vec::new(), shares_canonical: vec![false; AGENTS.len()] }
}

fn empty_mcp(scope: Scope) -> McpInventory {
    McpInventory { scope, servers: Vec::new(), merges: Vec::new(), errors: Vec::new() }
}

/// `args` with `--project DIR` for a project scope.
fn scoped(mut args: Vec<String>, scope: &Scope) -> Vec<String> {
    if let Scope::Project(p) = scope {
        args.push("--project".into());
        args.push(p.display().to_string());
    }
    args
}

/// The last line of a failed command's stderr, or its exit code.
fn why(out: &crate::hosts::runner::Output, what: &str) -> String {
    out.stderr.lines().rev().map(str::trim).find(|l| !l.is_empty()).map_or_else(|| format!("{what} exited with {}", out.code), String::from)
}

/// Checks the host's prism (installing this version first when asked), then reads its state.
pub(super) fn load(r: &dyn Runner, host: &Host, scope: &Scope, install_first: bool) -> (Result<Box<WireState>, RemoteState>, Option<String>) {
    let mut install_error = None;
    if install_first {
        match install::check(r) {
            Ok((_, platform)) => install_error = install::install(r, VERSION, &platform, None, false).err(),
            Err(e) => return (Err(RemoteState::Unreachable(e)), None),
        }
    }
    let state = match install::check(r) {
        Err(e) => Err(RemoteState::Unreachable(e)),
        Ok((Health::Ready, _)) => match r.prism(&scoped(vec!["wire".into(), "state".into()], scope), None) {
            Err(e) => Err(RemoteState::Unreachable(e)),
            Ok(out) if out.code != 0 => Err(RemoteState::Unreachable(why(&out, "prism wire state"))),
            Ok(out) => serde_json::from_slice::<WireState>(&out.stdout)
                .map(Box::new)
                .map_err(|e| RemoteState::Unreachable(format!("{} sent a state this window can't read: {e}", host.name))),
        },
        Ok((health, _)) => Err(RemoteState::NeedsInstall(health)),
    };
    (state, install_error)
}

impl App {
    /// A host is in view: nothing reads or writes this machine's disk with its paths; changes
    /// run there, through its own CLI.
    pub fn is_remote(&self) -> bool {
        matches!(self.env, Env::Remote { .. })
    }

    /// The host in view.
    pub fn host(&self) -> Option<&Host> {
        match &self.env {
            Env::Remote { host, .. } => Some(host),
            Env::Local => None,
        }
    }

    /// The name the switcher shows for the machine in view.
    pub fn env_label(&self) -> String {
        self.host().map_or_else(|| hosts::copy::this_machine().to_string(), |h| h.name.clone())
    }

    /// This machine's settings, whichever machine is in view: its saved hosts live there.
    pub fn local_settings(&self) -> &Settings {
        self.local.as_ref().map_or(&self.settings, |l| &l.settings)
    }

    /// This machine's paths, whichever machine is in view.
    pub fn local_paths(&self) -> &Paths {
        self.local.as_ref().map_or(&self.paths, |l| &l.paths)
    }

    /// This machine's open scope, whichever machine is in view: skills.sh installs go there.
    pub fn local_scope(&self) -> &Scope {
        self.local.as_ref().map_or(&self.scope, |l| &l.scope)
    }

    /// Whether the agent is installed on the machine in view.
    pub fn detected(&self, agent: usize) -> bool {
        match &self.remote_detected {
            Some((skills, _)) => skills.get(agent).copied().unwrap_or(false),
            None if self.is_remote() => false,
            None => AGENTS[agent].detected(&self.paths),
        }
    }

    /// Whether the MCP agent is installed on the machine in view.
    pub fn mcp_detected(&self, agent: usize) -> bool {
        match &self.remote_detected {
            Some((_, mcp)) => mcp.get(agent).copied().unwrap_or(false),
            None if self.is_remote() => false,
            None => MCP_AGENTS[agent].detected(&self.paths),
        }
    }

    /// On a host, an installed MCP agent can take a server (its files can't be looked at here).
    pub(super) fn remote_can_hold(&self, agent: usize) -> bool {
        MCP_AGENTS[agent].flavor == Flavor::OpenCode || self.mcp_detected(agent)
    }

    /// Shows this machine (`None`) or the saved host named `to`.
    pub fn switch_env(&mut self, to: Option<&str>) {
        let Some(name) = to else {
            self.restore_local();
            return;
        };
        let Some(host) = hosts::find(&self.local_settings().hosts, name).cloned() else {
            self.notify(format!("No saved host named {name}"), String::new(), true);
            return;
        };
        if self.local.is_none() {
            let scope = self.scope.clone();
            self.local = Some(Stash {
                paths: self.paths.clone(),
                settings: std::mem::take(&mut self.settings),
                scope: scope.clone(),
                inv: std::mem::replace(&mut self.inv, empty_inv(Scope::Global)),
                mcp: std::mem::replace(&mut self.mcp, empty_mcp(Scope::Global)),
                stats: std::mem::take(&mut self.stats),
                global_names: std::mem::take(&mut self.global_names),
                last_change: self.last_change.take(),
            });
        } else {
            self.inv = empty_inv(Scope::Global);
            self.mcp = empty_mcp(Scope::Global);
            self.stats.clear();
            self.global_names.clear();
        }
        self.settings = Settings::default();
        self.scope = Scope::Global;
        self.remote_detected = None;
        // An install in flight was for the host in view before; its answer never lands here.
        self.installing_prism = false;
        self.env = Env::Remote { host, state: RemoteState::Connecting };
        self.close_everything();
        // The watcher watches this machine's folders; nothing here changes the host's view.
        self.watch();
        self.take_dirty();
        self.load_remote(Scope::Global, false);
    }

    /// Back to this machine: its stashed state, then a fresh look at its disk.
    fn restore_local(&mut self) {
        let Some(local) = self.local.take() else { return };
        self.remote_load.cancel();
        self.remote_preview.cancel();
        self.installing_prism = false;
        self.env = Env::Local;
        self.remote_detected = None;
        self.paths = local.paths;
        self.settings = local.settings;
        self.scope = local.scope;
        self.inv = local.inv;
        self.mcp = local.mcp;
        self.stats = local.stats;
        self.global_names = local.global_names;
        self.last_change = local.last_change;
        self.close_everything();
        // The CLI may have changed this machine while the host was in view.
        self.take_dirty();
        self.watch();
        self.last_change = store::load_last_change(&self.paths);
        self.reload_settings();
        self.rescan();
        self.refresh_stats();
    }

    /// Clears what the view showed of the last machine: selections, filters and dialogs.
    fn close_everything(&mut self) {
        self.reset_view();
        self.sync_plan = None;
        self.sync_host = None;
        self.sync_only = None;
        self.confirm_remove = None;
        self.copy_dialog = None;
        self.new_skill = None;
        self.found = None;
        self.mcp_form = None;
        self.mcp_dialog = None;
        self.env_menu = false;
        self.add_host = None;
        self.host_project = None;
        // Copies in flight still land: their toasts name the machine.
        self.across.close();
        // Changes asked of the last machine: those not started stay undone.
        self.drop_host_work();
        // A toast about the last machine would read as one about this one.
        self.notice = None;
        self.notice_at = None;
    }

    /// Asks the host in view for `scope`'s state, off the UI thread.
    /// While an install runs on it, nothing else is asked: a newer request would replace the
    /// install's, and its answer would never land.
    pub(super) fn load_remote(&mut self, scope: Scope, install_first: bool) {
        let Env::Remote { host, .. } = &self.env else { return };
        if self.installing_prism && !install_first {
            return;
        }
        let (host, make) = (host.clone(), self.make_runner.clone());
        self.remote_load.request((host, scope, install_first), move |(host, scope, install_first)| {
            let runner = make(&host);
            let (result, install_error) = load(runner.as_ref(), &host, &scope, install_first);
            RemoteAnswer { host: host.name, result, installed: install_first, install_error }
        });
    }

    /// Reconnects to the host in view, as from the start.
    pub fn retry_remote(&mut self) {
        self.connect_remote(self.scope.clone());
    }

    /// Asks the host in view for `scope`. A host whose state isn't in shows Connecting
    /// meanwhile; one in view keeps showing its state until the answer lands.
    pub(super) fn connect_remote(&mut self, scope: Scope) {
        if let Env::Remote { state, .. } = &mut self.env
            && *state != RemoteState::Ready
        {
            *state = RemoteState::Connecting;
        }
        self.load_remote(scope, false);
    }

    /// Puts this version of prism on the host in view, then connects.
    pub fn install_prism(&mut self) {
        if self.installing_prism {
            return;
        }
        if let Env::Remote { state, .. } = &mut self.env {
            *state = RemoteState::Connecting;
            self.installing_prism = true;
            self.load_remote(self.scope.clone(), true);
        }
    }

    /// Lands a host's answer, when it is the host in view.
    pub fn land_remote(&mut self, answer: RemoteAnswer) {
        let RemoteAnswer { host: name, result, installed, install_error } = answer;
        let Env::Remote { host, state } = &mut self.env else { return };
        if host.name != name {
            return;
        }
        let host = host.name.clone();
        // Whatever this answer was for, nothing runs on the host in view any more.
        self.installing_prism = false;
        match result {
            Ok(w) => {
                *state = RemoteState::Ready;
                let w = *w;
                if w.scope != self.scope {
                    self.reset_view();
                }
                self.paths = w.paths;
                self.settings = w.settings;
                self.scope = w.scope;
                self.inv = w.inv;
                self.mcp = w.mcp;
                self.stats = w.stats.into_iter().collect();
                if self.scope.is_global() {
                    self.global_names = self.inv.skills.iter().map(|s| s.name.clone()).collect();
                }
                self.remote_detected = Some((w.detected, w.mcp_detected));
                self.host_last_change = w.last_change;
                // Shown values were read before; any change since would not show in them.
                self.host_values.clear();
                self.mcp_revealed.clear();
                if self.selected.as_deref().is_some_and(|n| self.inv.get(n).is_none()) {
                    self.selected = None;
                }
                let inv = &self.inv;
                self.checked.retain(|n| inv.get(n).is_some());
                if self.mcp_selected.as_deref().is_some_and(|n| self.mcp.get(n).is_none()) {
                    self.mcp_selected = None;
                }
                self.load_preview();
                match install_error {
                    Some(e) => self.notify(format!("Couldn't install prism on {host}"), e, true),
                    None if installed => self.notify(format!("Installed prism {VERSION} on {host}"), String::new(), false),
                    None => {}
                }
            }
            Err(s) => {
                *state = s;
                if let Some(e) = install_error {
                    self.notify(format!("Couldn't install prism on {host}"), e, true);
                }
            }
        }
    }

    /// Asks the host in view for the selected skill's SKILL.md, off the UI thread.
    pub(super) fn load_remote_preview(&mut self) {
        self.preview = None;
        let Some(name) = self.selected.clone().filter(|n| self.inv.get(n).is_some()) else { return };
        let Some(host) = self.host().cloned() else { return };
        let (scope, make) = (self.scope.clone(), self.make_runner.clone());
        self.remote_preview.request((host, scope, name), move |(host, scope, name)| {
            let args = scoped(vec!["wire".into(), "skill-md".into(), name.clone()], &scope);
            let body = make(&host)
                .prism(&args, None)
                .ok()
                .filter(|out| out.code == 0)
                .and_then(|out| serde_json::from_slice::<serde_json::Value>(&out.stdout).ok())
                .and_then(|v| v["body"].as_str().map(String::from));
            (name, body)
        });
    }

    pub(super) fn land_remote_preview(&mut self, name: String, body: Option<String>) {
        if self.is_remote() && self.selected.as_deref() == Some(name.as_str()) {
            self.preview = body;
        }
    }

    // Saved hosts.

    pub fn open_add_host(&mut self) {
        self.env_menu = false;
        let mut name = TextEdit::new("");
        name.focused = true;
        self.add_host = Some(AddHost { name, ssh: TextEdit::new(""), error: None });
    }

    /// Saves the dialog's host on this machine and connects to it.
    pub fn save_host(&mut self) {
        let hosts = self.local_settings().hosts.clone();
        let Some(form) = &mut self.add_host else { return };
        let name = form.name.text.trim().to_string();
        let ssh = form.ssh.text.trim().to_string();
        if let Err(e) = hosts::valid_name(&name) {
            form.error = Some(e);
            return;
        }
        if !hosts::valid_target(&ssh) {
            form.error = Some("The ssh target is user@host or an alias from ~/.ssh/config, with no spaces".into());
            return;
        }
        if hosts::find(&hosts, &name).is_some() {
            form.error = Some(format!("{name} is already saved"));
            return;
        }
        self.add_host = None;
        let host = Host { name: name.clone(), ssh };
        match &mut self.local {
            Some(local) => {
                if local.settings.persist {
                    local.settings = Settings::load_from(&local.paths);
                }
                local.settings.hosts.push(host);
                local.settings.save_to(&local.paths);
            }
            None => self.edit_settings(|s| s.hosts.push(host)),
        }
        self.switch_env(Some(&name));
    }

    /// Puts `host` in view with `answer` as its state, without reaching it: for scenes.
    pub fn show_remote(&mut self, host: Host, answer: Result<Box<WireState>, RemoteState>) {
        if hosts::find(&self.settings.hosts, &host.name).is_none() {
            self.settings.hosts.push(host.clone());
        }
        let name = host.name.clone();
        self.make_runner = std::sync::Arc::new(|h: &Host| Box::new(Unreached(h.name.clone())) as Box<dyn Runner>);
        self.switch_env(Some(&name));
        self.remote_load.cancel();
        self.land_remote(RemoteAnswer { host: name, result: answer, installed: false, install_error: None });
    }
}

/// A host no scene reaches.
pub(super) struct Unreached(pub(super) String);

impl Runner for Unreached {
    fn prism(&self, _: &[String], _: Option<Vec<u8>>) -> Result<crate::hosts::runner::Output, String> {
        Err(format!("Can't reach {}: a scene reaches no host", self.0))
    }
    fn shell(&self, _: &str, _: Option<Vec<u8>>) -> Result<crate::hosts::runner::Output, String> {
        Err(format!("Can't reach {}: a scene reaches no host", self.0))
    }
}
