//! Keeping agents' skills folders in step with `.agents/skills`.
//!
//! A sync moves a skill that only exists in one agent's folder into `.agents/skills` (and
//! leaves a symlink behind), then links it into every target agent. Nothing is overwritten:
//! a folder that differs from the canonical copy is reported as a conflict, and replaced
//! copies go to the trash, never straight to deletion.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::agents::{AGENTS, CANONICAL, Paths, Scope};
use crate::scan::{Inventory, Placement, Skill};

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Move the folder from agent `from` into `.agents/skills` and link it back.
    Adopt { skill: String, from: usize },
    /// Point `.agents/skills/<skill>` at where agent `from`'s link points.
    AdoptLink { skill: String, from: usize },
    /// Create a symlink in `agent`'s folder.
    Link { skill: String, agent: usize },
    /// Trash `agent`'s identical copy (or remove its broken link) and link instead.
    Replace { skill: String, agent: usize },
}

impl Action {
    pub fn describe(&self) -> String {
        match self {
            Action::Adopt { skill, from } => format!("Move {skill} from {} into .agents and link it back", AGENTS[*from].name),
            Action::AdoptLink { skill, from } => format!("Link .agents/{skill} to the folder {} links to", AGENTS[*from].name),
            Action::Link { skill, agent } => format!("Link {skill} into {}", AGENTS[*agent].name),
            Action::Replace { skill, agent } => format!("Replace {}'s copy of {skill} with a link", AGENTS[*agent].name),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Conflict {
    pub skill: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub actions: Vec<Action>,
    pub conflicts: Vec<Conflict>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty() && self.conflicts.is_empty()
    }
}

/// What a sync of `inv` into `targets` (agent indices) would do.
pub fn plan(inv: &Inventory, targets: &[usize], paths: &Paths) -> Plan {
    let mut plan = Plan::default();
    for skill in &inv.skills {
        plan_skill(inv, skill, targets, paths, &mut plan);
    }
    plan
}

/// The plan for one skill alone.
pub fn plan_one(inv: &Inventory, name: &str, targets: &[usize], paths: &Paths) -> Plan {
    plan_many(inv, &[name.to_string()], targets, paths)
}

/// The plan for the named skills only, in inventory order.
pub fn plan_many(inv: &Inventory, names: &[String], targets: &[usize], paths: &Paths) -> Plan {
    let mut plan = Plan::default();
    for skill in inv.skills.iter().filter(|s| names.contains(&s.name)) {
        plan_skill(inv, skill, targets, paths, &mut plan);
    }
    plan
}

fn plan_skill(inv: &Inventory, skill: &Skill, targets: &[usize], paths: &Paths, plan: &mut Plan) {
    let scope = &inv.scope;
    let path = |agent: usize| AGENTS[agent].dir(scope, paths).join(&skill.name);
    let name = skill.name.clone();
    let conflict = |plan: &mut Plan, reason: String| plan.conflicts.push(Conflict { skill: name.clone(), reason });

    // Where the canonical contents will be, and which agent they came from.
    let (content, adopted_from): (PathBuf, Option<usize>) = match skill.canonical() {
        Placement::Dir | Placement::Link(_) | Placement::Linked => (path(CANONICAL), None),
        Placement::Broken(_) => {
            conflict(plan, format!(".agents/skills/{} is a broken link", skill.name));
            return;
        }
        Placement::Missing => {
            let copies: Vec<usize> = (1..AGENTS.len()).filter(|&a| skill.placements[a] == Placement::Dir && !inv.shares_canonical[a]).collect();
            if let Some(&first) = copies.first() {
                if let Some(&other) = copies[1..].iter().find(|&&a| !dirs_equal(&path(first), &path(a))) {
                    conflict(plan, format!("{} and {} hold different copies", AGENTS[first].name, AGENTS[other].name));
                    return;
                }
                plan.actions.push(Action::Adopt { skill: name.clone(), from: first });
                (path(first), Some(first))
            } else if let Some(from) = (1..AGENTS.len()).find(|&a| matches!(skill.placements[a], Placement::Link(_))) {
                plan.actions.push(Action::AdoptLink { skill: name.clone(), from });
                let Placement::Link(real) = &skill.placements[from] else { unreachable!() };
                (real.clone(), None)
            } else {
                return;
            }
        }
    };
    let content_real = fs::canonicalize(&content).ok();

    for &t in targets {
        if t == CANONICAL || inv.shares_canonical[t] || Some(t) == adopted_from {
            continue;
        }
        match &skill.placements[t] {
            Placement::Linked => {}
            Placement::Missing => plan.actions.push(Action::Link { skill: name.clone(), agent: t }),
            Placement::Broken(_) => plan.actions.push(Action::Replace { skill: name.clone(), agent: t }),
            Placement::Link(real) if Some(real) == content_real.as_ref() => {}
            Placement::Link(real) => conflict(plan, format!("{} links to {}", AGENTS[t].name, real.display())),
            Placement::Dir if dirs_equal(&path(t), &content) => plan.actions.push(Action::Replace { skill: name.clone(), agent: t }),
            Placement::Dir => conflict(plan, format!("{} has its own, different copy", AGENTS[t].name)),
        }
    }
}

/// Applies actions in order, stopping at the first failure. Returns how many succeeded.
pub fn apply(scope: &Scope, paths: &Paths, actions: &[Action]) -> Result<usize, (usize, String)> {
    for (done, action) in actions.iter().enumerate() {
        apply_one(scope, paths, action).map_err(|e| (done, format!("{}: {e}", action.describe())))?;
    }
    Ok(actions.len())
}

fn apply_one(scope: &Scope, paths: &Paths, action: &Action) -> io::Result<()> {
    let path = |agent: usize, skill: &str| AGENTS[agent].dir(scope, paths).join(skill);
    match action {
        Action::Adopt { skill, from } => {
            let (src, dst) = (path(*from, skill), path(CANONICAL, skill));
            if fs::symlink_metadata(&dst).is_ok() {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} already exists", dst.display())));
            }
            fs::create_dir_all(dst.parent().unwrap())?;
            move_dir(&src, &dst)?;
            make_link(&src, &dst)
        }
        Action::AdoptLink { skill, from } => {
            let real = fs::canonicalize(path(*from, skill))?;
            let dst = path(CANONICAL, skill);
            fs::create_dir_all(dst.parent().unwrap())?;
            std::os::unix::fs::symlink(&real, &dst)?;
            // The agent's own link now goes through .agents like the rest.
            fs::remove_file(path(*from, skill))?;
            make_link(&path(*from, skill), &dst)
        }
        Action::Link { skill, agent } => make_link(&path(*agent, skill), &path(CANONICAL, skill)),
        Action::Replace { skill, agent } => {
            let at = path(*agent, skill);
            discard(&at, paths)?;
            make_link(&at, &path(CANONICAL, skill))
        }
    }
}

/// Links `skill` into `agent`, moving it into `.agents` first if it isn't there yet.
pub fn link(inv: &Inventory, paths: &Paths, skill: &str, agent: usize) -> Result<(), String> {
    let plan = plan_one(inv, skill, &[agent], paths);
    if let Some(c) = plan.conflicts.first() {
        return Err(c.reason.clone());
    }
    apply(&inv.scope, paths, &plan.actions).map(|_| ()).map_err(|(_, e)| e)
}

/// Removes `agent`'s symlink to `skill`. Real folders are left alone: see [`trash_copy`].
pub fn unlink(scope: &Scope, paths: &Paths, skill: &str, agent: usize) -> Result<(), String> {
    if agent == CANONICAL {
        return Err("The .agents copy is the skill itself; remove the skill instead".into());
    }
    let at = AGENTS[agent].dir(scope, paths).join(skill);
    let meta = fs::symlink_metadata(&at).map_err(|e| format!("{}: {e}", at.display()))?;
    if !meta.file_type().is_symlink() {
        return Err(format!("{} is a folder, not a link", at.display()));
    }
    fs::remove_file(&at).map_err(|e| format!("{}: {e}", at.display()))
}

/// Moves `agent`'s own folder for `skill` to the trash.
pub fn trash_copy(scope: &Scope, paths: &Paths, skill: &str, agent: usize) -> Result<(), String> {
    let at = AGENTS[agent].dir(scope, paths).join(skill);
    discard(&at, paths).map_err(|e| format!("{}: {e}", at.display()))
}

/// Removes the skill from the scope: every link goes, every folder goes to the trash.
pub fn remove(inv: &Inventory, paths: &Paths, skill: &str) -> Result<(), String> {
    let s = inv.get(skill).ok_or_else(|| format!("{skill} is not installed here"))?;
    // Links first, so none is left pointing at a trashed folder.
    let mut order: Vec<usize> = (0..AGENTS.len()).filter(|&a| !inv.shares_canonical[a]).collect();
    order.sort_by_key(|&a| matches!(s.placements[a], Placement::Dir));
    for a in order {
        if s.placements[a] == Placement::Missing {
            continue;
        }
        let at = AGENTS[a].dir(&inv.scope, paths).join(skill);
        discard(&at, paths).map_err(|e| format!("{}: {e}", at.display()))?;
    }
    Ok(())
}

/// Removes a symlink, or moves a folder to the trash.
fn discard(path: &Path, paths: &Paths) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || meta.is_file() {
        return fs::remove_file(path);
    }
    let trash = trash_dir(paths);
    fs::create_dir_all(&trash)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let mut dst = trash.join(&name);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut n = 0;
    while fs::symlink_metadata(&dst).is_ok() {
        n += 1;
        dst = trash.join(format!("{name} {stamp}-{n}"));
    }
    move_dir(path, &dst)
}

fn trash_dir(paths: &Paths) -> PathBuf {
    if cfg!(target_os = "macos") {
        paths.home.join(".Trash")
    } else {
        std::env::var_os("XDG_DATA_HOME").map_or_else(|| paths.home.join(".local/share"), PathBuf::from).join("Trash/files")
    }
}

/// A relative symlink at `at` to `target`, as the `skills` CLI writes them.
fn make_link(at: &Path, target: &Path) -> io::Result<()> {
    let parent = at.parent().ok_or_else(|| io::Error::other("link has no parent folder"))?;
    fs::create_dir_all(parent)?;
    let from = fs::canonicalize(parent)?;
    let to = match (target.parent().map(fs::canonicalize), target.file_name()) {
        (Some(Ok(dir)), Some(name)) => dir.join(name),
        _ => target.to_path_buf(),
    };
    std::os::unix::fs::symlink(relative(&from, &to), at)
}

/// `to` written relative to the folder `from`. Both are absolute and normalised.
pub fn relative(from: &Path, to: &Path) -> PathBuf {
    let a: Vec<Component> = from.components().collect();
    let b: Vec<Component> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut out = PathBuf::new();
    for _ in common..a.len() {
        out.push("..");
    }
    for c in &b[common..] {
        out.push(c);
    }
    out
}

fn move_dir(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        // Another volume: copy, then remove the original.
        Err(e) if e.raw_os_error() == Some(18) => {
            copy_dir(src, dst)?;
            fs::remove_dir_all(src)
        }
        Err(e) => Err(e),
    }
}

pub fn copy_dir(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(&from)?, &to)?;
        } else if ty.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// The two folders hold the same files with the same bytes (`.DS_Store` aside).
pub fn dirs_equal(a: &Path, b: &Path) -> bool {
    fn list(dir: &Path) -> Option<Vec<(String, PathBuf)>> {
        let mut v: Vec<_> = fs::read_dir(dir)
            .ok()?
            .filter_map(Result::ok)
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
            .filter(|(n, _)| n != ".DS_Store")
            .collect();
        v.sort();
        Some(v)
    }
    if fs::canonicalize(a).ok().is_some_and(|x| fs::canonicalize(b).ok() == Some(x)) {
        return true;
    }
    let (Some(la), Some(lb)) = (list(a), list(b)) else { return false };
    la.len() == lb.len()
        && la.iter().zip(&lb).all(|((na, pa), (nb, pb))| {
            na == nb
                && match (pa.is_dir(), pb.is_dir()) {
                    (true, true) => dirs_equal(pa, pb),
                    (false, false) => fs::read(pa).ok().is_some_and(|x| fs::read(pb).ok() == Some(x)),
                    _ => false,
                }
        })
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
