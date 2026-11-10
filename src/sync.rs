//! Keeping agents' skills folders in step with `.agents/skills`.
//!
//! A sync moves a skill that only exists in one agent's folder into `.agents/skills` (and
//! leaves a symlink behind), then links it into every target agent. Nothing is overwritten:
//! a folder that differs from the canonical copy is reported as a conflict, and replaced
//! copies go to the trash, never straight to deletion.

use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::agents::{AGENTS, CANONICAL, Paths, Scope};
use crate::fsops::Journal;
use crate::scan::{Inventory, Placement, Skill};

/// `1 change`, `3 changes`.
pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// How the window and the CLI name a change, so Undo reads the same in both.
pub mod label {
    use super::{AGENTS, Scope, plural};
    use crate::agents::{Paths, tilde};

    pub fn sync(changes: usize) -> String {
        format!("sync ({})", plural(changes, "change", "changes"))
    }

    pub fn link(skill: &str, agent: usize) -> String {
        format!("link {skill} into {}", AGENTS[agent].name)
    }

    pub fn unlink(skill: &str, agent: usize) -> String {
        format!("unlink {skill} from {}", AGENTS[agent].name)
    }

    pub fn remove(names: &[String]) -> String {
        match names {
            [one] => format!("remove {one}"),
            _ => format!("remove {}", plural(names.len(), "skill", "skills")),
        }
    }

    pub const BROKEN: &str = "clean up broken links";

    pub fn copy(to: &str) -> String {
        format!("copy to {to}")
    }

    /// A skill copied here from another machine, `from` naming it.
    pub fn receive(names: &[String], from: &str) -> String {
        match names {
            [one] => format!("copy {one} from {from}"),
            _ => format!("copy {} from {from}", plural(names.len(), "skill", "skills")),
        }
    }

    pub fn trash_copy(skill: &str, agent: usize) -> String {
        format!("trash {}'s copy of {skill}", AGENTS[agent].name)
    }

    /// A scope as the window titles it: `Global (~)`, or the project's path with `~`.
    pub fn scope(scope: &Scope, paths: &Paths) -> String {
        match scope {
            Scope::Global => "Global (~)".into(),
            Scope::Project(p) => tilde(p, paths),
        }
    }
}

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

#[cfg(test)]
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

/// Applies actions in order, recording each change in `j`, stopping at the first failure.
/// Returns how many succeeded.
pub fn apply(scope: &Scope, paths: &Paths, actions: &[Action], j: &mut Journal) -> Result<usize, (usize, String)> {
    for (done, action) in actions.iter().enumerate() {
        apply_one(scope, paths, action, j).map_err(|e| (done, format!("{}: {e}", action.describe())))?;
    }
    Ok(actions.len())
}

fn apply_one(scope: &Scope, paths: &Paths, action: &Action, j: &mut Journal) -> io::Result<()> {
    let path = |agent: usize, skill: &str| AGENTS[agent].dir(scope, paths).join(skill);
    match action {
        Action::Adopt { skill, from } => {
            let (src, dst) = (path(*from, skill), path(CANONICAL, skill));
            if fs::symlink_metadata(&dst).is_ok() {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} already exists", dst.display())));
            }
            j.move_to(&src, &dst)?;
            j.link(&src, &dst)
        }
        Action::AdoptLink { skill, from } => {
            let real = fs::canonicalize(path(*from, skill))?;
            let dst = path(CANONICAL, skill);
            j.symlink(&real, &dst)?;
            // The agent's own link now goes through .agents like the rest.
            j.remove_link(&path(*from, skill))?;
            j.link(&path(*from, skill), &dst)
        }
        Action::Link { skill, agent } => j.link(&path(*agent, skill), &path(CANONICAL, skill)),
        Action::Replace { skill, agent } => {
            let at = path(*agent, skill);
            j.discard(&at, paths)?;
            j.link(&at, &path(CANONICAL, skill))
        }
    }
}

/// Links freshly installed skills (all of the scope's when `skills` is empty or `*`) into
/// `targets`. Installs are not undoable, so the journal is thrown away. Returns how many
/// links it made.
pub fn link_installed(scope: &Scope, paths: &Paths, targets: &[usize], skills: &[String]) -> Result<usize, String> {
    let inv = crate::scan::scan(scope, paths);
    let names: Vec<String> = if skills.is_empty() || skills.iter().any(|s| s == "*") {
        inv.skills.iter().filter(|s| s.canonical().present()).map(|s| s.name.clone()).collect()
    } else {
        skills.to_vec()
    };
    let plan = plan_many(&inv, &names, targets, paths);
    let links: Vec<Action> = plan.actions.into_iter().filter(|a| matches!(a, Action::Link { .. } | Action::Replace { .. })).collect();
    apply(scope, paths, &links, &mut Journal::default()).map_err(|(_, e)| e)
}

/// Links `skill` into `agent`, moving it into `.agents` first if it isn't there yet.
pub fn link(inv: &Inventory, paths: &Paths, skill: &str, agent: usize, j: &mut Journal) -> Result<(), String> {
    let plan = plan_one(inv, skill, &[agent], paths);
    if let Some(c) = plan.conflicts.first() {
        return Err(c.reason.clone());
    }
    apply(&inv.scope, paths, &plan.actions, j).map(|_| ()).map_err(|(_, e)| e)
}

/// Removes `agent`'s symlink to `skill`. Real folders are left alone: see [`trash_copy`].
pub fn unlink(scope: &Scope, paths: &Paths, skill: &str, agent: usize, j: &mut Journal) -> Result<(), String> {
    if agent == CANONICAL {
        return Err("The .agents copy is the skill itself; remove the skill instead".into());
    }
    let at = AGENTS[agent].dir(scope, paths).join(skill);
    let meta = fs::symlink_metadata(&at).map_err(|e| format!("{}: {e}", at.display()))?;
    if !meta.file_type().is_symlink() {
        return Err(format!("{} is a folder, not a link", at.display()));
    }
    j.remove_link(&at).map_err(|e| format!("{}: {e}", at.display()))
}

/// Moves `agent`'s own folder for `skill` to the trash.
pub fn trash_copy(scope: &Scope, paths: &Paths, skill: &str, agent: usize, j: &mut Journal) -> Result<(), String> {
    let at = AGENTS[agent].dir(scope, paths).join(skill);
    j.discard(&at, paths).map_err(|e| format!("{}: {e}", at.display()))
}

/// Removes the skill from the scope: every link goes, every folder goes to the trash.
pub fn remove(inv: &Inventory, paths: &Paths, skill: &str, j: &mut Journal) -> Result<(), String> {
    let s = inv.get(skill).ok_or_else(|| format!("{skill} is not installed here"))?;
    // Links first, so none is left pointing at a trashed folder.
    let mut order: Vec<usize> = (0..AGENTS.len()).filter(|&a| !inv.shares_canonical[a]).collect();
    order.sort_by_key(|&a| matches!(s.placements[a], Placement::Dir));
    for a in order {
        if s.placements[a] == Placement::Missing {
            continue;
        }
        let at = AGENTS[a].dir(&inv.scope, paths).join(skill);
        j.discard(&at, paths).map_err(|e| format!("{}: {e}", at.display()))?;
    }
    Ok(())
}

/// Every broken link in the scope, as (skill, agent).
pub fn broken_links(inv: &Inventory) -> Vec<(String, usize)> {
    inv.skills
        .iter()
        .flat_map(|s| (0..AGENTS.len()).filter(|&a| matches!(s.placements[a], Placement::Broken(_))).map(move |a| (s.name.clone(), a)))
        .filter(|(_, a)| !inv.shares_canonical[*a])
        .collect()
}

/// Removes every broken link in the scope. Returns how many went.
pub fn remove_broken(inv: &Inventory, paths: &Paths, j: &mut Journal) -> Result<usize, String> {
    let broken = broken_links(inv);
    for (skill, a) in &broken {
        let at = AGENTS[*a].dir(&inv.scope, paths).join(skill);
        j.remove_link(&at).map_err(|e| format!("{}: {e}", at.display()))?;
    }
    Ok(broken.len())
}

/// Copies `skill` from `inv`'s scope into `dest`'s `.agents/skills` and links it into
/// `targets` there. A skill already in `dest` is left alone.
pub fn copy_to(inv: &Inventory, paths: &Paths, skill: &str, dest: &Scope, targets: &[usize], j: &mut Journal) -> Result<(), String> {
    let s = inv.get(skill).ok_or_else(|| format!("{skill} is not installed here"))?;
    let src = s.source.as_ref().and_then(|p| fs::canonicalize(p).ok()).ok_or_else(|| format!("{skill} has no readable folder"))?;
    let dst = AGENTS[CANONICAL].dir(dest, paths).join(skill);
    if fs::symlink_metadata(&dst).is_ok() {
        return Err(format!("{skill} is already in {}", dst.display()));
    }
    j.copy(&src, &dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    link_arrived(paths, skill, dest, targets, j)
}

/// Links `skill`, just put into `dest`'s `.agents/skills`, into `targets` there.
fn link_arrived(paths: &Paths, skill: &str, dest: &Scope, targets: &[usize], j: &mut Journal) -> Result<(), String> {
    let there = crate::scan::scan(dest, paths);
    let plan = plan_one(&there, skill, targets, paths);
    let links: Vec<Action> = plan.actions.into_iter().filter(|a| matches!(a, Action::Link { .. })).collect();
    apply(dest, paths, &links, j).map(|_| ()).map_err(|(_, e)| e)
}

/// Why `name` can't be a skill folder's name: it must be one path component and not hidden.
pub fn check_folder_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.contains(['/', '\0']) || name.starts_with('.') {
        return Err(format!("{name:?} is not a skill name: it must be a folder name, not a path or a hidden name"));
    }
    Ok(())
}

/// What another machine sends in one tar.
pub struct Incoming<'a> {
    /// Each skill's name and the sender's [`tree_hash`](crate::planid::tree_hash) of its folder.
    pub sent: &'a [(String, u64)],
    /// `true`: each skill is a top-level folder named after it (`pdf/SKILL.md`). `false`: the
    /// tar holds the one skill's folder contents (`./SKILL.md`).
    pub named: bool,
    /// The skills to place: the others (already here) are checked and left out.
    pub place: &'a [String],
}

/// Puts skills that another machine sent as one tar into `dest`'s `.agents/skills`, then links
/// them into `targets` there like [`copy_to`]. The tar comes over the network, so it is saved
/// and listed first: a member with an absolute path or a `..`, or (named) outside the sent
/// skills' folders, refuses the whole archive. It unpacks into a staging folder, and nothing is
/// placed unless every sent folder is there and has the sender's hash (a cut-off or altered
/// stream is refused). The staging folder and the saved tar always go. Returns how many skills
/// were placed; on failure, how many were before it (undo takes them back).
pub fn receive_many(paths: &Paths, dest: &Scope, incoming: &Incoming, tar: &mut dyn Read, targets: &[usize], j: &mut Journal) -> Result<usize, (usize, String)> {
    let names: Vec<&str> = incoming.sent.iter().map(|(n, _)| n.as_str()).collect();
    for (i, name) in names.iter().enumerate() {
        check_folder_name(name).map_err(|e| (0, e))?;
        if names[..i].contains(name) {
            return Err((0, format!("{name} is sent twice")));
        }
    }
    if names.is_empty() || (!incoming.named && names.len() != 1) {
        return Err((0, "a tar of one folder's contents holds one skill".into()));
    }
    let store = AGENTS[CANONICAL].dir(dest, paths);
    for name in incoming.place {
        if !names.contains(&name.as_str()) {
            return Err((0, format!("{name} was not sent")));
        }
        if fs::symlink_metadata(store.join(name)).is_ok() {
            return Err((0, format!("{name} is already in {}", store.join(name).display())));
        }
    }
    fs::create_dir_all(&store).map_err(|e| (0, format!("{}: {e}", store.display())))?;
    // Hidden names: a scan of the store never shows them as skills.
    let incoming_name = format!(".prism-incoming-{}-{}", std::process::id(), names[0]);
    let (archive, staging) = (store.join(format!("{incoming_name}.tar")), store.join(&incoming_name));
    let top = if incoming.named { Some(names.as_slice()) } else { None };
    let into = if incoming.named { staging.clone() } else { staging.join(names[0]) };
    let unpacked = unpack(tar, &archive, &staging, &into, top).and_then(|()| {
        for (name, hash) in incoming.sent {
            let folder = staging.join(name);
            // A link where a folder should be would place a link to anywhere.
            let real = fs::symlink_metadata(&folder).is_ok_and(|m| m.is_dir());
            if !real || crate::planid::tree_hash(&folder) != Some(*hash) {
                return Err(format!("{name} arrived incomplete or changed; nothing was placed"));
            }
        }
        Ok(())
    });
    let _ = fs::remove_file(&archive);
    let mut placed = 0;
    let result = unpacked.map_err(|e| (0, e)).and_then(|()| {
        for name in incoming.place {
            let dst = store.join(name);
            j.place(&staging.join(name), &dst).map_err(|e| (placed, format!("{}: {e}", dst.display())))?;
            placed += 1;
        }
        Ok(())
    });
    let _ = fs::remove_dir_all(&staging);
    result?;
    // Every skill is placed by now: a link that fails leaves all of them for undo to take back.
    for name in incoming.place {
        link_arrived(paths, name, dest, targets, j).map_err(|e| (placed, e))?;
    }
    Ok(placed)
}

/// Saves `tar` to `archive`, checks every member stays inside (and, given `top`, inside one of
/// those top-level folders), and unpacks it into `into`, made inside a fresh `staging`.
fn unpack(tar: &mut dyn Read, archive: &Path, staging: &Path, into: &Path, top: Option<&[&str]>) -> Result<(), String> {
    let saved = fs::File::create(archive).and_then(|mut f| io::copy(tar, &mut f));
    saved.map_err(|e| format!("could not save the skill's archive: {e}"))?;
    let listed = run_tar(Command::new("tar").arg("-tf").arg(archive))?;
    let outside = |m: &&str| {
        let path = Path::new(m);
        let first = path.components().find(|c| c != &Component::CurDir);
        let stray = match (top, first) {
            (Some(names), Some(Component::Normal(n))) => !names.iter().any(|t| n == std::ffi::OsStr::new(t)),
            _ => false,
        };
        m.starts_with('/') || path.components().any(|c| c == Component::ParentDir) || stray
    };
    if let Some(member) = String::from_utf8_lossy(&listed).lines().find(outside) {
        return Err(format!("the skill's archive reaches outside its folder ({member}); nothing was unpacked"));
    }
    let _ = fs::remove_dir_all(staging);
    fs::create_dir(staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    if into != staging {
        fs::create_dir(into).map_err(|e| format!("{}: {e}", into.display()))?;
    }
    run_tar(Command::new("tar").arg("-xf").arg(archive).arg("-C").arg(into).arg("--no-same-owner")).map(|_| ())
}

/// Runs `tar` and returns what it printed, or why it failed.
fn run_tar(cmd: &mut Command) -> Result<Vec<u8>, String> {
    let out = cmd.output().map_err(|e| format!("could not run tar: {e}"))?;
    if !out.status.success() {
        return Err(format!("tar could not read the skill's archive: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out.stdout)
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
