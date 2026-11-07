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
    /// A folder or file was moved into `.agents`.
    Moved { from: PathBuf, to: PathBuf },
    /// A folder or file went to the trash. `info` is its freedesktop `.trashinfo`, off macOS.
    Trashed { from: PathBuf, to: PathBuf, info: Option<PathBuf> },
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
        let (to, info) = trash(path, paths)?;
        self.steps.push(Step::Trashed { from: path.to_path_buf(), to, info });
        Ok(())
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
                Step::Trashed { from, to, info } => match fs::symlink_metadata(from) {
                    Ok(_) => Err(io::Error::other(format!("{} exists again", from.display()))),
                    Err(_) => move_path(to, from).map(|()| {
                        if let Some(info) = info {
                            let _ = fs::remove_file(info);
                        }
                    }),
                },
                Step::Copied(at) => trash(at, paths).map(|_| ()),
            };
            result.map_err(|e| format!("Undid {done} of {n} steps: {e}"))?;
        }
        Ok(n)
    }
}

/// Moves `path` to the trash. Returns where it went and, in a freedesktop trash, its info file.
fn trash(path: &Path, paths: &Paths) -> io::Result<(PathBuf, Option<PathBuf>)> {
    if cfg!(target_os = "macos") {
        let dst = free_slot(&paths.trash, path, |_| true)?;
        move_path(path, &dst)?;
        Ok((dst, None))
    } else {
        trash_freedesktop(path, &paths.trash).map(|(dst, info)| (dst, Some(info)))
    }
}

/// The trash file managers on Linux use: the item in `files/`, and in `info/` a `.trashinfo`
/// with its original path, so it can be put back from there too.
fn trash_freedesktop(path: &Path, trash: &Path) -> io::Result<(PathBuf, PathBuf)> {
    let (files, infos) = (trash.join("files"), trash.join("info"));
    fs::create_dir_all(&infos)?;
    let info_for = |dst: &Path| infos.join(format!("{}.trashinfo", dst.file_name().unwrap_or_default().to_string_lossy()));
    let dst = free_slot(&files, path, |dst| fs::symlink_metadata(info_for(dst)).is_err())?;
    let info = info_for(&dst);
    let original = std::path::absolute(path)?;
    let body = format!("[Trash Info]\nPath={}\nDeletionDate={}\n", uri_escape(&original), local_now());
    // The info file comes first and is created exclusively: it reserves the name.
    fs::OpenOptions::new().write(true).create_new(true).open(&info).and_then(|mut f| io::Write::write_all(&mut f, body.as_bytes()))?;
    if let Err(e) = move_path(path, &dst) {
        let _ = fs::remove_file(&info);
        return Err(e);
    }
    Ok((dst, info))
}

/// A path in `dir` for `path`'s name that is free, and that `ok` accepts.
fn free_slot(dir: &Path, path: &Path, ok: impl Fn(&Path) -> bool) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let mut dst = dir.join(&name);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut n = 0;
    while fs::symlink_metadata(&dst).is_ok() || !ok(&dst) {
        n += 1;
        dst = dir.join(format!("{name} {stamp}-{n}"));
    }
    Ok(dst)
}

/// A path as a URI path: unreserved bytes and `/` as they are, the rest percent-encoded.
pub fn uri_escape(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::new();
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The local time as a `.trashinfo` wants it: `2026-11-07T14:05:09`.
fn local_now() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as libc::time_t;
    // SAFETY: `localtime_r` only writes the `tm` it is given.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        tm
    };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
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
    fn the_freedesktop_trash_keeps_where_things_came_from() {
        let home = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        let trash = root.join(".local/share/Trash");
        let skill = root.join("my skills/x");
        fs::create_dir_all(&skill).unwrap();
        let (dst, info) = trash_freedesktop(&skill, &trash).unwrap();
        assert_eq!(dst, trash.join("files/x"));
        assert!(dst.is_dir() && !skill.exists());
        let text = fs::read_to_string(&info).unwrap();
        assert!(text.starts_with("[Trash Info]\n"));
        assert!(text.contains(&format!("Path={}/my%20skills/x\n", uri_escape(&root))), "{text}");
        assert!(text.contains("DeletionDate=20"), "{text}");

        fs::create_dir_all(&skill).unwrap();
        let (second, info2) = trash_freedesktop(&skill, &trash).unwrap();
        assert_ne!(second, dst, "a second x gets its own name");
        assert_eq!(info2.file_name().unwrap().to_string_lossy(), format!("{}.trashinfo", second.file_name().unwrap().to_string_lossy()));
    }

    #[test]
    fn relative_paths() {
        assert_eq!(relative(Path::new("/h/.claude/skills"), Path::new("/h/.agents/skills/a")), PathBuf::from("../../.agents/skills/a"));
        assert_eq!(relative(Path::new("/h/a"), Path::new("/h/a/b")), PathBuf::from("b"));
    }
}
