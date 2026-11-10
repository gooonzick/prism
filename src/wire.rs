//! The window's state as one JSON document, so a window can show another machine: `prism wire
//! state` prints it there, the window reads it here. Secrets stay where they are: MCP values are
//! masked and saved hosts are left out.

use serde::{Deserialize, Serialize};

use crate::agents::{self, AGENTS, Paths, Scope};
use crate::mcp::agents::MCP_AGENTS;
use crate::mcp::scan::{self as mcp_scan, McpInventory};
use crate::scan::{self, Inventory, ScopeStats};
use crate::store::{self, Settings};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireState {
    /// The version of the prism that wrote it.
    pub version: String,
    pub paths: Paths,
    /// With no hosts: those are this machine's.
    pub settings: Settings,
    pub scope: Scope,
    pub inv: Inventory,
    /// Masked.
    pub mcp: McpInventory,
    /// Global first, then each project in the settings.
    pub stats: Vec<(Scope, ScopeStats)>,
    /// The label of the change `prism undo` would take back.
    pub last_change: Option<String>,
    /// Per entry of [`AGENTS`]: installed.
    pub detected: Vec<bool>,
    /// Per entry of [`MCP_AGENTS`]: installed.
    pub mcp_detected: Vec<bool>,
}

/// Everything the window shows for `scope`.
pub fn state(scope: &Scope, paths: &Paths, settings: &Settings) -> WireState {
    let inv = scan::scan(scope, paths);
    let targets = agents::target_indices(settings.targets.as_deref(), paths);
    let scopes = std::iter::once(Scope::Global).chain(settings.projects.iter().cloned().map(Scope::Project));
    let stats = scopes
        .map(|s| {
            let stats = if &s == scope { scan::stats_of(&inv, &targets) } else { scan::stats_of(&scan::scan(&s, paths), &targets) };
            (s, stats)
        })
        .collect();
    WireState {
        version: env!("CARGO_PKG_VERSION").into(),
        paths: paths.clone(),
        settings: Settings { hosts: Vec::new(), persist: false, ..settings.clone() },
        scope: scope.clone(),
        mcp: mcp_scan::masked(&mcp_scan::scan(scope, paths)),
        inv,
        stats,
        last_change: store::load_last_change(paths).map(|c| c.label),
        detected: AGENTS.iter().map(|a| a.detected(paths)).collect(),
        mcp_detected: MCP_AGENTS.iter().map(|a| a.detected(paths)).collect(),
    }
}
