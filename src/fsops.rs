//! Filesystem changes that can be taken back. Every link, unlink and move a sync, remove or
//! copy makes goes through a [`Journal`], and [`Journal::undo`] reverses them in order.
//! Nothing here deletes data: replaced folders and files go to the trash.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agents::Paths;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Step {
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
    /// A file was rewritten. `backup` is the original (`None`: it didn't exist), `hash_after`
    /// the hash of what was written, so undo can tell if anything else touched it since.
    Rewrote { path: PathBuf, backup: Option<PathBuf>, hash_after: u64 },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Journal {
    pub(crate) steps: Vec<Step>,
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

    /// Replaces `path` with `contents`, atomically (a temp file next to it, then a rename). The
    /// first time this journal touches `path` its original goes to `<state>/backups/`.
    #[allow(dead_code)] // the MCP commands write configs through it
    pub fn write_file(&mut self, path: &Path, contents: &[u8], paths: &Paths) -> io::Result<()> {
        let earlier = self.steps.iter().position(|s| matches!(s, Step::Rewrote { path: p, .. } if p == path));
        let original = match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let backup = match (&original, earlier) {
            (Some(bytes), None) => Some(back_up(path, bytes, paths)?),
            _ => None,
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_atomic(path, contents)?;
        let hash_after = content_hash(contents);
        match earlier {
            // A second write to the same file: undo goes back to the first original.
            Some(i) => {
                if let Step::Rewrote { hash_after: h, .. } = &mut self.steps[i] {
                    *h = hash_after;
                }
            }
            None => self.steps.push(Step::Rewrote { path: path.to_path_buf(), backup, hash_after }),
        }
        Ok(())
    }

    /// Reverses every step, last first. Stops at the first step that can't be reversed (the
    /// path was changed since), leaving the rest as they are: the error carries the steps not
    /// yet undone, as a journal, so they can be tried again.
    pub fn undo(mut self, paths: &Paths) -> Result<usize, (String, Journal)> {
        let n = self.steps.len();
        for done in 0..n {
            let step = self.steps.last().cloned().expect("a step is left");
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
                Step::Rewrote { path, backup, hash_after } => match fs::read(path) {
                    Ok(now) if content_hash(&now) == *hash_after => match backup {
                        Some(b) => fs::read(b).and_then(|bytes| write_atomic(path, &bytes)),
                        // The file the write made; a link that led to it stays.
                        None => fs::remove_file(through_links(path)),
                    },
                    _ => Err(io::Error::other(format!("{} changed since", path.display()))),
                },
            };
            result.map_err(|e| (format!("Undid {done} of {n} steps: {e}"), self.clone()))?;
            self.steps.pop();
        }
        Ok(n)
    }
}

/// FNV-1a, 64 bit. Fixed on purpose: hashes are saved on disk, and `DefaultHasher` may change
/// between Rust releases.
pub fn content_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3))
}

/// Writes `contents` to `path` through a temp file next to it, so a reader never sees half a
/// file. The temp file is created with the file's mode (0600 for a new one) from the start:
/// configs hold secrets. It never outlives an error. A linked file (dotfiles) is written where
/// the link leads, so the link stays; the temp name has the pid, so the window and the CLI
/// writing at once don't take each other's.
pub(crate) fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let path = &through_links(path);
    let mode = fs::metadata(path).map_or(0o600, |m| m.permissions().mode() & 0o7777);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".prism-tmp-{}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    let _ = fs::remove_file(&tmp); // left over from a crash
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&tmp)
        .and_then(|mut f| {
            io::Write::write_all(&mut f, contents)?;
            // The umask may have taken bits off; the original mode is what we want.
            f.set_permissions(fs::Permissions::from_mode(mode))
        })
        .and_then(|()| fs::rename(&tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

/// Where `path` leads after every link on it (a relative link counts from its folder), or
/// `path` itself when it is no link. A dangling link leads to the file it would make.
fn through_links(path: &Path) -> PathBuf {
    let mut at = path.to_path_buf();
    // As many hops as the kernel follows before it gives up on a loop.
    for _ in 0..40 {
        match fs::read_link(&at) {
            Ok(to) => at = at.parent().map_or(to.clone(), |dir| dir.join(&to)),
            Err(_) => break,
        }
    }
    at
}

/// Copies a config's original bytes into the private backups folder. Returns the copy.
fn back_up(path: &Path, bytes: &[u8], paths: &Paths) -> io::Result<PathBuf> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let dir = paths.state.join("backups");
    fs::create_dir_all(&dir)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    for n in 0.. {
        let to = dir.join(format!("{n}-{name}"));
        match fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&to) {
            Ok(mut f) => {
                io::Write::write_all(&mut f, bytes)?;
                return Ok(to);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    unreachable!()
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

    fn setup() -> (tempfile::TempDir, PathBuf, Paths) {
        let home = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        let paths = Paths::isolated(&root);
        (home, root, paths)
    }

    #[test]
    fn rewrite_backs_up_and_undo_restores() {
        use std::os::unix::fs::PermissionsExt;
        let (_h, root, paths) = setup();
        let a = root.join("a.json");
        fs::write(&a, br#"{"x":1}"#).unwrap();
        let mut j = Journal::default();
        j.write_file(&a, br#"{"x":2}"#, &paths).unwrap();
        assert_eq!(fs::read(&a).unwrap(), br#"{"x":2}"#);
        assert_eq!(fs::read_dir(&root).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().contains("prism-tmp")).count(), 0);
        let Step::Rewrote { backup: Some(backup), .. } = &j.steps[0] else { panic!("{:?}", j.steps) };
        assert_eq!(fs::read(backup).unwrap(), br#"{"x":1}"#);
        assert_eq!(fs::metadata(backup).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(backup.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        j.undo(&paths).unwrap();
        assert_eq!(fs::read(&a).unwrap(), br#"{"x":1}"#);
    }

    #[test]
    fn rewriting_twice_undoes_to_the_original() {
        let (_h, root, paths) = setup();
        let a = root.join("a.json");
        fs::write(&a, "0").unwrap();
        let mut j = Journal::default();
        j.write_file(&a, b"1", &paths).unwrap();
        j.write_file(&a, b"2", &paths).unwrap();
        j.undo(&paths).unwrap();
        assert_eq!(fs::read(&a).unwrap(), b"0");
    }

    #[test]
    fn rewrite_keeps_the_file_mode() {
        use std::os::unix::fs::PermissionsExt;
        let (_h, root, paths) = setup();
        let a = root.join("a.json");
        fs::write(&a, "0").unwrap();
        fs::set_permissions(&a, fs::Permissions::from_mode(0o640)).unwrap();
        Journal::default().write_file(&a, b"1", &paths).unwrap();
        assert_eq!(fs::metadata(&a).unwrap().permissions().mode() & 0o777, 0o640);
    }

    #[test]
    fn rewrite_of_new_file_undo_deletes_it() {
        let (_h, root, paths) = setup();
        let a = root.join("sub/new.json");
        let mut j = Journal::default();
        j.write_file(&a, b"{}", &paths).unwrap();
        assert!(a.is_file());
        j.undo(&paths).unwrap();
        assert!(!a.exists());
    }

    #[test]
    fn undo_refuses_after_outside_edit() {
        let (_h, root, paths) = setup();
        let a = root.join("a.json");
        fs::write(&a, "1").unwrap();
        let mut j = Journal::default();
        j.write_file(&a, "2".as_bytes(), &paths).unwrap();
        fs::write(&a, "by hand").unwrap();
        let (err, _) = j.undo(&paths).unwrap_err();
        assert!(err.contains("changed since"), "{err}");
        assert_eq!(fs::read(&a).unwrap(), b"by hand");
    }

    #[test]
    fn a_failed_write_leaves_no_temp_file() {
        let (_h, root, _paths) = setup();
        // A non-empty folder where the file should go: the temp file is written, the rename fails.
        let a = root.join("a.json");
        fs::create_dir_all(a.join("inner")).unwrap();
        assert!(write_atomic(&a, b"1").is_err());
        assert!(!root.join(format!("a.json.prism-tmp-{}", std::process::id())).exists());
    }

    #[test]
    fn a_failed_journal_write_records_nothing() {
        let (_h, root, paths) = setup();
        let a = root.join("a.json");
        fs::create_dir_all(a.join("inner")).unwrap();
        let mut j = Journal::default();
        assert!(j.write_file(&a, b"1", &paths).is_err());
        assert!(j.is_empty());
    }

    #[test]
    fn new_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let (_h, root, paths) = setup();
        let a = root.join("new.json");
        Journal::default().write_file(&a, b"1", &paths).unwrap();
        assert_eq!(fs::metadata(&a).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn a_linked_config_stays_a_link_through_write_and_undo() {
        let (_h, root, paths) = setup();
        let target = root.join("dotfiles/cursor-mcp.json");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "0").unwrap();
        let link = root.join(".cursor/mcp.json");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink("../dotfiles/cursor-mcp.json", &link).unwrap();

        let mut j = Journal::default();
        j.write_file(&link, b"1", &paths).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the write kept the link");
        assert_eq!(fs::read(&target).unwrap(), b"1");
        assert_eq!(fs::read_dir(target.parent().unwrap()).unwrap().count(), 1, "no temp file left next to the target");

        j.undo(&paths).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the undo kept the link");
        assert_eq!(fs::read(&target).unwrap(), b"0");
    }

    #[test]
    fn another_process_temp_file_is_left_alone() {
        let (_h, root, _paths) = setup();
        let a = root.join("a.json");
        // The window and the CLI may write the same file at once; each has its own temp file.
        let theirs = root.join("a.json.prism-tmp");
        fs::write(&theirs, "theirs").unwrap();
        write_atomic(&a, b"1").unwrap();
        assert_eq!(fs::read(&theirs).unwrap(), b"theirs");
        assert_eq!(fs::read(&a).unwrap(), b"1");
    }

    #[test]
    fn content_hash_is_fnv1a() {
        assert_eq!(content_hash(b""), 0xcbf29ce484222325);
        assert_eq!(content_hash(b"a"), 0xaf63dc4c8601ec8c);
    }

    #[test]
    fn a_failed_undo_returns_the_steps_left() {
        let (_h, root, paths) = setup();
        let (a, b) = (root.join("a.json"), root.join("b.json"));
        fs::write(&a, "0").unwrap();
        let mut j = Journal::default();
        j.write_file(&a, b"1", &paths).unwrap();
        j.write_file(&b, b"1", &paths).unwrap();
        fs::write(&a, "hand").unwrap();
        let (err, left) = j.undo(&paths).unwrap_err();
        assert!(err.contains("changed since"), "{err}");
        assert!(!b.exists(), "the later step was undone");
        assert!(matches!(left.steps.as_slice(), [Step::Rewrote { path, .. }] if path == &a));
    }
}
