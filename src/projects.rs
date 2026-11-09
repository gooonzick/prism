//! Finding projects that already have skills, under a folder the user picks.

use std::fs;
use std::path::{Path, PathBuf};

use crate::agents::{Paths, Scope};
use crate::scan;

/// Folders never worth descending into.
const SKIP: &[&str] = &["node_modules", "target", "dist", "build", "vendor", "Library", "Applications", "Pictures", "Movies", "Music"];

/// How deep below the chosen folder projects are looked for.
const DEPTH: usize = 4;

/// Projects under `root` with at least one skill in an agent folder, with their counts.
pub fn find_projects(root: &Path, paths: &Paths) -> Vec<(PathBuf, usize)> {
    let mut out = Vec::new();
    walk(root, 0, paths, &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, depth: usize, paths: &Paths, out: &mut Vec<(PathBuf, usize)>) {
    if dir == paths.home && depth > 0 {
        return;
    }
    let n = if dir == paths.home { 0 } else { scan::scan(&Scope::Project(dir.to_path_buf()), paths).skills.len() };
    if n > 0 {
        out.push((dir.to_path_buf(), n));
    }
    if depth >= DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.filter_map(Result::ok) {
        let name = e.file_name().to_string_lossy().into_owned();
        let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
        if is_dir && !name.starts_with('.') && !SKIP.contains(&name.as_str()) {
            walk(&e.path(), depth + 1, paths, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_projects_with_skills_and_skips_the_rest() {
        let home = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        let paths = Paths::isolated(&root);
        let skill = |dir: &Path| {
            fs::create_dir_all(dir.join("x")).unwrap();
            fs::write(dir.join("x/SKILL.md"), "---\nname: x\n---\n").unwrap();
        };
        let dev = root.join("dev");
        skill(&dev.join("app/.claude/skills"));
        skill(&dev.join("group/lib/.agents/skills"));
        skill(&dev.join("app/node_modules/pkg/.claude/skills"));
        fs::create_dir_all(dev.join("empty/.claude/skills")).unwrap();

        let found = find_projects(&dev, &paths);
        assert_eq!(found, [(dev.join("app"), 1), (dev.join("group/lib"), 1)]);
    }
}
