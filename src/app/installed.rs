//! Installed skills: picking them, linking, syncing, removing, copying and creating them.

use std::fs;

use rune::{Mods, TextEdit};

use super::{App, Filter, NewSkill};
use crate::agents::{AGENTS, CANONICAL, Scope};
use crate::scan::{Placement, Skill};
use crate::skill;
use crate::sync::{self, plural};

impl App {
    // The list.

    pub fn matches_filter(&self, s: &Skill) -> bool {
        match self.status_filter {
            Filter::All => true,
            Filter::NeedsSync => !s.synced(&self.targets()),
            Filter::Broken => s.placements.iter().any(|p| matches!(p, Placement::Broken(_))),
            Filter::Updates => self.has_update(&s.name),
        }
    }

    /// The installed skills the filters let through.
    pub fn visible(&self) -> Vec<usize> {
        let q = self.filter.text.trim().to_lowercase();
        (0..self.inv.skills.len())
            .filter(|&i| {
                let s = &self.inv.skills[i];
                (q.is_empty() || s.name.to_lowercase().contains(&q) || s.description.to_lowercase().contains(&q)) && self.matches_filter(s)
            })
            .collect()
    }

    pub fn filter_count(&self, f: Filter) -> usize {
        let targets = self.targets();
        self.inv
            .skills
            .iter()
            .filter(|s| match f {
                Filter::All => true,
                Filter::NeedsSync => !s.synced(&targets),
                Filter::Broken => s.placements.iter().any(|p| matches!(p, Placement::Broken(_))),
                Filter::Updates => self.has_update(&s.name),
            })
            .count()
    }

    pub fn set_status_filter(&mut self, f: Filter) {
        self.status_filter = f;
    }

    pub fn select(&mut self, name: &str) {
        self.selected = Some(name.to_string());
        self.load_preview();
        if !self.inspector.is_open() {
            self.inspector.toggle();
        }
    }

    pub(super) fn load_preview(&mut self) {
        self.preview = self.selected.as_deref().and_then(|n| self.inv.get(n)).and_then(|s| s.manifest()).map(|m| m.body);
    }

    /// Up and Down from the keyboard: selects the next shown row (Shift ticks the way).
    /// Returns the row's key, to scroll it into view.
    pub fn step_selection(&mut self, delta: i32, shift: bool) -> Option<String> {
        let shown: Vec<String> = self.visible().into_iter().map(|i| self.inv.skills[i].name.clone()).collect();
        if shown.is_empty() {
            return None;
        }
        let at = self.selected.as_ref().and_then(|n| shown.iter().position(|x| x == n));
        let next = match at {
            None if delta > 0 => 0,
            None => shown.len() - 1,
            Some(i) => (i as i32 + delta).clamp(0, shown.len() as i32 - 1) as usize,
        };
        let name = shown[next].clone();
        if shift {
            if self.anchor.is_none() {
                self.anchor = self.selected.clone().or_else(|| Some(name.clone()));
            }
            self.tick_range(&name);
        } else {
            self.anchor = Some(name.clone());
        }
        self.select(&name);
        Some(format!("skill-{name}"))
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
    pub fn click_row(&mut self, name: &str, mods: Mods) {
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
    pub fn click_tick(&mut self, name: &str, mods: Mods) {
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

    /// Ticks every shown skill, or clears them all when they already are.
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

    // Changes to one skill.

    /// Links or unlinks `skill` for `agent`, from its switch.
    pub fn toggle_link(&mut self, skill: &str, agent: usize) {
        let Some(s) = self.inv.get(skill) else { return };
        let mut j = self.new_journal();
        let (label, result) = match &s.placements[agent] {
            Placement::Linked | Placement::Link(_) | Placement::Broken(_) => (
                sync::label::unlink(skill, agent),
                sync::unlink(&self.scope, &self.paths, skill, agent, &mut j),
            ),
            Placement::Missing => (sync::label::link(skill, agent), sync::link(&self.inv, &self.paths, skill, agent, &mut j)),
            Placement::Dir => (
                String::new(),
                Err(format!("{} has its own folder; use \"Use link\" or move it to the trash", AGENTS[agent].name)),
            ),
        };
        let mut chars = label.chars();
        let title = chars.next().map(|c| c.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default();
        self.changed(label, j, result.map(|_| title));
    }

    /// Replaces `agent`'s own copy with a link to `.agents` (adopting it if `.agents` lacks it).
    pub fn replace_with_link(&mut self, skill: &str, agent: usize) {
        let plan = sync::plan_one(&self.inv, skill, &[agent], &self.paths);
        let mut j = self.new_journal();
        let result = match plan.conflicts.first() {
            Some(c) => Err(c.reason.clone()),
            None => sync::apply(&self.scope, &self.paths, &plan.actions, &mut j).map(|_| format!("{skill} now lives in .agents")).map_err(|(_, e)| e),
        };
        self.changed(format!("move {skill} into .agents"), j, result);
    }

    pub fn trash_copy(&mut self, skill: &str, agent: usize) {
        let mut j = self.new_journal();
        let result = sync::trash_copy(&self.scope, &self.paths, skill, agent, &mut j)
            .map(|_| format!("Moved {}'s copy of {skill} to the trash", AGENTS[agent].name));
        self.changed(format!("trash {}'s copy of {skill}", AGENTS[agent].name), j, result);
    }

    /// Removes every skill in `names`, going on past failures.
    pub fn remove(&mut self, names: &[String]) {
        self.confirm_remove = None;
        let mut j = self.new_journal();
        let mut failed = Vec::new();
        for name in names {
            if let Err(e) = sync::remove(&self.inv, &self.paths, name, &mut j) {
                failed.push(e);
            }
        }
        let removed = names.len() - failed.len();
        let result = match (failed.is_empty(), names) {
            (true, [one]) => Ok(format!("Removed {one}")),
            (true, _) => Ok(format!("Removed {removed} skills")),
            (false, _) => Err(format!("Removed {removed} of {}: {}", names.len(), failed.join("\n"))),
        };
        self.checked.retain(|n| !names.contains(n));
        self.changed(sync::label::remove(names), j, result);
    }

    pub fn remove_broken(&mut self) {
        let mut j = self.new_journal();
        let result = sync::remove_broken(&self.inv, &self.paths, &mut j).map(|n| format!("Removed {}", plural(n, "broken link", "broken links")));
        if self.status_filter == Filter::Broken {
            self.status_filter = Filter::All;
        }
        self.changed(sync::label::BROKEN.into(), j, result);
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

    pub fn toggle_target(&mut self, agent: usize) {
        let mut t = self.targets();
        if let Some(i) = t.iter().position(|&a| a == agent) {
            t.remove(i);
        } else {
            t.push(agent);
            t.sort();
        }
        self.settings.targets = Some(t.iter().map(|&a| AGENTS[a].id.to_string()).collect());
        self.settings.save_to(&self.paths);
        if self.sync_plan.is_some() {
            self.open_sync_for(self.sync_only.clone());
        }
        self.refresh_stats();
    }

    pub fn apply_sync(&mut self) {
        let Some(plan) = self.sync_plan.take() else { return };
        let only = self.sync_only.take();
        let n = plan.actions.len();
        let mut j = self.new_journal();
        let result = sync::apply(&self.scope, &self.paths, &plan.actions, &mut j)
            .map(|_| format!("Synced: {}", plural(n, "change", "changes")))
            .map_err(|(done, e)| format!("Stopped after {done} of {n}: {e}"));
        if result.is_ok() && only.is_some() {
            self.checked.clear();
        }
        self.changed(sync::label::sync(n), j, result);
    }

    // Copying to another scope.

    pub fn open_copy(&mut self, names: Vec<String>) {
        if !names.is_empty() {
            self.copy_dialog = Some(names);
        }
    }

    pub fn copy_to(&mut self, dest: Scope) {
        let Some(names) = self.copy_dialog.take() else { return };
        let targets = self.targets();
        let mut j = self.new_journal();
        let mut failed = Vec::new();
        for n in &names {
            if let Err(e) = sync::copy_to(&self.inv, &self.paths, n, &dest, &targets, &mut j) {
                failed.push(e);
            }
        }
        let to = self.scope_label(&dest);
        let copied = names.len() - failed.len();
        let result = if failed.is_empty() {
            Ok(format!("Copied {} to {to}", if names.len() == 1 { names[0].clone() } else { plural(copied, "skill", "skills") }))
        } else {
            Err(format!("Copied {copied} of {} to {to}: {}", names.len(), failed.join("\n")))
        };
        self.changed(sync::label::copy(&to), j, result);
        self.refresh_stats();
    }

    // A new skill.

    pub fn open_new_skill(&mut self) {
        let mut name = TextEdit::new("");
        name.focused = true;
        self.new_skill = Some(NewSkill { name, description: TextEdit::new(""), error: None });
    }

    /// Writes `.agents/skills/<name>/SKILL.md`, links it into the targets and opens it.
    pub fn create_skill(&mut self) {
        let Some(form) = &mut self.new_skill else { return };
        let name = form.name.text.trim().to_string();
        let description = form.description.text.clone();
        if let Err(e) = skill::valid_name(&name) {
            form.error = Some(e);
            return;
        }
        let dir = AGENTS[CANONICAL].dir(&self.scope, &self.paths).join(&name);
        if self.inv.get(&name).is_some() || fs::symlink_metadata(&dir).is_ok() {
            form.error = Some(format!("{name} already exists here"));
            return;
        }
        let body = skill::template(&name, &description);
        if let Err(e) = fs::create_dir_all(&dir).and_then(|_| fs::write(dir.join("SKILL.md"), body)) {
            form.error = Some(e.to_string());
            return;
        }
        self.new_skill = None;
        self.rescan();
        let mut j = self.new_journal();
        let plan = sync::plan_one(&self.inv, &name, &self.targets(), &self.paths);
        let _ = sync::apply(&self.scope, &self.paths, &plan.actions, &mut j);
        self.notify(format!("Created {name}"), "Its SKILL.md is open in your editor".into(), false);
        self.rescan();
        self.select(&name);
        open_path(&dir.join("SKILL.md"));
    }
}

/// Opens a file or folder with its default app.
pub fn open_path(path: &std::path::Path) {
    // Tests click the buttons that call this; they must not open windows on the machine.
    if cfg!(not(test)) {
        let _ = std::process::Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" }).arg(path).spawn();
    }
}

/// Shows a file or folder in Finder, or the desktop's file manager.
pub fn reveal_path(path: &std::path::Path) {
    if cfg!(test) {
        return;
    }
    if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
        return;
    }
    // The file manager's FileManager1 interface selects the item; without one, open its folder.
    let (path, uri) = (path.to_path_buf(), format!("array:string:file://{}", crate::fsops::uri_escape(path)));
    std::thread::spawn(move || {
        let shown = std::process::Command::new("dbus-send")
            .args(["--session", "--print-reply", "--dest=org.freedesktop.FileManager1", "--type=method_call", "/org/freedesktop/FileManager1", "org.freedesktop.FileManager1.ShowItems", &uri, "string:"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !shown {
            let _ = std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(&path)).spawn();
        }
    });
}
