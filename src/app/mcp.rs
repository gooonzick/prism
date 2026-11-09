//! MCP servers in the open scope: the list, its filter, the selection and revealed values.

use super::{App, Page};
use crate::mcp::scan::{self, McpServer, Status};

/// Which MCP servers the list shows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum McpFilter {
    #[default]
    All,
    /// Copies disagree.
    Differs,
    /// Some agent skips it in this project.
    Disabled,
    /// An app manages it.
    ReadOnly,
}

impl McpFilter {
    pub fn matches(self, s: &McpServer) -> bool {
        match self {
            McpFilter::All => true,
            McpFilter::Differs => s.status == Status::Differs,
            McpFilter::Disabled => !s.disabled_for.is_empty(),
            McpFilter::ReadOnly => s.status == Status::ReadOnly,
        }
    }
}

impl App {
    pub fn open_mcp(&mut self) {
        self.page = Page::Mcp;
        self.rescan_mcp();
    }

    pub fn open_skills(&mut self) {
        self.page = Page::Installed;
    }

    pub fn rescan_mcp(&mut self) {
        self.mcp = scan::scan(&self.scope, &self.paths);
        if self.mcp_selected.as_deref().is_some_and(|n| self.mcp.get(n).is_none()) {
            self.mcp_selected = None;
        }
        let mcp = &self.mcp;
        self.mcp_revealed.retain(|n| mcp.get(n).is_some());
    }

    pub fn select_mcp(&mut self, name: &str) {
        self.mcp_selected = Some(name.to_string());
        if !self.inspector.is_open() {
            self.inspector.toggle();
        }
    }

    /// Indices of the servers the filter lets through.
    pub fn mcp_visible(&self) -> Vec<usize> {
        (0..self.mcp.servers.len()).filter(|&i| self.mcp_filter.matches(&self.mcp.servers[i])).collect()
    }

    pub fn mcp_filter_count(&self, f: McpFilter) -> usize {
        self.mcp.servers.iter().filter(|s| f.matches(s)).count()
    }

    pub fn set_mcp_filter(&mut self, f: McpFilter) {
        self.mcp_filter = f;
    }

    /// Shows or masks the server's env, header, args and URL values in the inspector.
    pub fn toggle_reveal(&mut self, name: &str) {
        if !self.mcp_revealed.remove(name) {
            self.mcp_revealed.insert(name.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::McpFilter;
    use crate::agents::{Paths, Scope};
    use crate::app::{App, Page};
    use crate::store::Settings;

    /// Claude Code with `a` (in sync) and Cursor with a differing `b`.
    fn app() -> (tempfile::TempDir, App) {
        let home = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        fs::create_dir_all(root.join(".claude")).unwrap();
        fs::create_dir_all(root.join(".cursor")).unwrap();
        fs::write(root.join(".claude.json"), r#"{"mcpServers":{"a":{"command":"a-mcp"},"b":{"command":"b-mcp"}}}"#).unwrap();
        fs::write(root.join(".cursor/mcp.json"), r#"{"mcpServers":{"b":{"command":"b-mcp","args":["--v2"]}}}"#).unwrap();
        (home, App::new(Paths::isolated(&root), Settings::default()))
    }

    #[test]
    fn filters_count_and_narrow_the_list() {
        let (_home, mut app) = app();
        app.open_mcp();
        assert_eq!(app.page, Page::Mcp);
        assert_eq!(app.mcp_visible().len(), 2);
        assert_eq!(app.mcp_filter_count(McpFilter::Differs), 1);
        app.set_mcp_filter(McpFilter::Differs);
        let shown: Vec<&str> = app.mcp_visible().iter().map(|&i| app.mcp.servers[i].name.as_str()).collect();
        assert_eq!(shown, ["b"]);
    }

    #[test]
    fn a_new_scope_drops_the_selection_and_reveals() {
        let (home, mut app) = app();
        app.open_mcp();
        app.select_mcp("a");
        app.toggle_reveal("a");
        assert!(app.mcp_revealed.contains("a") && app.inspector.is_open());
        let project = fs::canonicalize(home.path()).unwrap().join("work");
        fs::create_dir_all(&project).unwrap();
        app.open_scope(Scope::Project(project));
        assert_eq!(app.page, Page::Mcp, "the page stays");
        assert_eq!((app.mcp_selected.as_deref(), app.mcp_revealed.len()), (None, 0));
        assert_eq!(app.mcp_filter, McpFilter::All);
    }
}
