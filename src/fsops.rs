//! Filesystem changes that can be taken back. Every link, unlink and move a sync, remove or
//! copy makes goes through a [`Journal`], and [`Journal::undo`] reverses them in order.
//! Nothing here deletes data: replaced folders and files go to the trash.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::agents::Paths;

#[derive(Clone, Debug, PartialEq)]
enum Step {
    /// A symlink was made at this path.
    Linked(PathBuf),
    /// A symlink was removed; it pointed at `target` (as written, relative or not).
    Unlinked { at: PathBuf, target: PathBuf },
    /// A folder or file was moved, to the trash or into `.agents`.
    Moved { from: PathBuf, to: PathBuf },
    /// A folder was copied here.
    Copied(PathBuf),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Journal {
    steps: Vec<Step>,
}

impl Journal {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// A relative symlink at `at` to `target`, as the `skills` CLI writes them.
    pub fn link(&mut self, at: &Path, target: &Path) -> io::Result<()> {
        let parent = at.parent().ok_or_else(|| io::Error::other("link has no parent folder"))?;
        fs::create_dir_all(parent)?;
        let from = fs::canonicalize(parent)?;
        let to = match (target.parent().map(fs::canonicalize), target.file_name()) {
            (Some(Ok(dir)), Some(name)) => dir.join(name),
            _ => target.to_path_buf(),
        };
        self.symlink(&relative(&from, &to), at)
    }

    /// A symlink at `at` holding `target` exactly as given.
    pub fn symlink(&mut self, target: &Path, at: &Path) -> io::Result<()> {
        if let Some(parent) = at.parent() {
            fs::create_dir_all(parent)?;
        }
        std::os::unix::fs::symlink(target, at)?;
        self.steps.push(Step::Linked(at.to_path_buf()));
        Ok(())
    }

    pub fn remove_link(&mut self, at: &Path) -> io::Result<()> {
        let target = fs::read_link(at)?;
        fs::remove_file(at)?;
        self.steps.push(Step::Unlinked { at: at.to_path_buf(), target });
        Ok(())
    }

    pub fn move_to(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        move_path(from, to)?;
        self.steps.push(Step::Moved { from: from.to_path_buf(), to: to.to_path_buf() });
        Ok(())
    }

    pub fn copy(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        if fs::symlink_metadata(to).is_ok() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} already exists", to.display())));
        }
        copy_dir(from, to)?;
        self.steps.push(Step::Copied(to.to_path_buf()));
        Ok(())
    }

    /// Removes a symlink, or moves a folder or file to the trash.
    pub fn discard(&mut self, path: &Path, paths: &Paths) -> io::Result<()> {
        let meta = fs::symlink_metadata(path)?;
        if meta.file_type().is_symlink() {
            return self.remove_link(path);
        }
        let dst = trash_slot(path, paths)?;
        self.move_to(path, &dst)
    }

    /// Reverses every step, last first. Stops at the first step that can't be reversed (the
    /// path was changed since), leaving the rest as they are.
    pub fn undo(self, paths: &Paths) -> Result<usize, String> {
        let n = self.steps.len();
        for (done, step) in self.steps.into_iter().rev().enumerate() {
            let result = match &step {
                Step::Linked(at) => match fs::symlink_metadata(at) {
                    Ok(m) if m.file_type().is_symlink() => fs::remove_file(at),
                    Ok(_) => Err(io::Error::other(format!("{} is no longer a link", at.display()))),
                    Err(_) => Ok(()),
                },
                Step::Unlinked { at, target } => match fs::symlink_metadata(at) {
                    Ok(_) => Err(io::Error::other(format!("{} exists again", at.display()))),
                    Err(_) => std::os::unix::fs::symlink(target, at),
                },
                Step::Moved { from, to } => match fs::symlink_metadata(from) {
                    Ok(_) => Err(io::Error::other(format!("{} exists again", from.display()))),
                    Err(_) => move_path(to, from),
                },
                Step::Copied(at) => trash_slot(at, paths).and_then(|dst| move_path(at, &dst)),
            };
            result.map_err(|e| format!("Undid {done} of {n} steps: {e}"))?;
        }
        Ok(n)
    }
}

/// A free path in the trash for `path`'s name.
fn trash_slot(path: &Path, paths: &Paths) -> io::Result<PathBuf> {
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
    Ok(dst)
}

fn trash_dir(paths: &Paths) -> PathBuf {
    if cfg!(target_os = "macos") {
        paths.home.join(".Trash")
    } else {
        std::env::var_os("XDG_DATA_HOME").map_or_else(|| paths.home.join(".local/share"), PathBuf::from).join("Trash/files")
    }
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

fn move_path(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        // Another volume: copy, then remove the original.
        Err(e) if e.raw_os_error() == Some(18) => {
            if fs::symlink_metadata(src)?.is_dir() {
                copy_dir(src, dst)?;
                fs::remove_dir_all(src)
            } else {
                fs::copy(src, dst)?;
                fs::remove_file(src)
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_reverses_links_moves_and_copies() {
        let home = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        let paths = Paths::isolated(&root);
        let (a, b, c) = (root.join("a"), root.join("b"), root.join("c"));
        fs::create_dir_all(&a).unwrap();
        fs::write(a.join("SKILL.md"), "x").unwrap();
        fs::create_dir_all(root.join("links")).unwrap();
        std::os::unix::fs::symlink("../a", root.join("links/old")).unwrap();

        let mut j = Journal::default();
        j.copy(&a, &c).unwrap();
        j.move_to(&a, &b).unwrap();
        j.link(&root.join("links/new"), &b).unwrap();
        j.remove_link(&root.join("links/old")).unwrap();
        assert_eq!(fs::read_link(root.join("links/new")).unwrap(), PathBuf::from("../b"));

        assert_eq!(j.undo(&paths), Ok(4));
        assert!(a.join("SKILL.md").is_file() && !b.exists());
        assert!(fs::symlink_metadata(root.join("links/new")).is_err());
        assert_eq!(fs::read_link(root.join("links/old")).unwrap(), PathBuf::from("../a"));
        assert!(!c.exists(), "the copy went to the trash");
    }

    #[test]
    fn discard_trashes_folders_and_undo_brings_them_back() {
        let home = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        let paths = Paths::isolated(&root);
        let skill = root.join("skills/x");
        fs::create_dir_all(&skill).unwrap();
        let mut j = Journal::default();
        j.discard(&skill, &paths).unwrap();
        assert!(!skill.exists());
        j.undo(&paths).unwrap();
        assert!(skill.is_dir());
    }

    #[test]
    fn relative_paths() {
        assert_eq!(relative(Path::new("/h/.claude/skills"), Path::new("/h/.agents/skills/a")), PathBuf::from("../../.agents/skills/a"));
        assert_eq!(relative(Path::new("/h/a"), Path::new("/h/a/b")), PathBuf::from("b"));
    }
}
