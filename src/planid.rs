//! Ids for printed plans: a plan's id changes when the state it was made from changes, so
//! `--apply --plan-id` can refuse a stale plan.
//!
//! Hashes come from `fsops::content_hash` (FNV-1a), not `DefaultHasher`, whose output is not
//! stable across Rust releases: an id printed by one build must still match on the next.


use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::fsops::content_hash;

const KIND_FILE: u8 = 0;
const KIND_DIR: u8 = 1;
const KIND_LINK: u8 = 2;
const KIND_OTHER: u8 = 3;

/// Appends one length-prefixed field, so no two different field lists share bytes.
fn push(buf: &mut Vec<u8>, field: &[u8]) {
    buf.extend_from_slice(&(field.len() as u64).to_le_bytes());
    buf.extend_from_slice(field);
}

/// The id of a plan: `lines` are what the plan says, `state` is what it was made from
/// (`None` = the path did not exist).
pub fn plan_id(lines: &[String], state: &[(PathBuf, Option<u64>)]) -> String {
    let mut buf = Vec::new();
    push(&mut buf, &(lines.len() as u64).to_le_bytes());
    for line in lines {
        push(&mut buf, line.as_bytes());
    }
    push(&mut buf, &(state.len() as u64).to_le_bytes());
    for (path, hash) in state {
        push(&mut buf, path.as_os_str().as_bytes());
        match hash {
            None => push(&mut buf, &[0]),
            Some(h) => {
                push(&mut buf, &[1]);
                push(&mut buf, &h.to_le_bytes());
            }
        }
    }
    format!("{:012x}", content_hash(&buf) & 0xffff_ffff_ffff)
}

/// Hash of one file's bytes, or `None` if it cannot be read.
#[allow(dead_code)] // the MCP commands hash config files
pub fn file_hash(path: &Path) -> Option<u64> {
    fs::read(path).ok().map(|b| content_hash(&b))
}

/// Hash of a skill folder: every entry's relative path, kind, link target and file bytes,
/// sorted by path. Links below the root are not followed. A link at `path` itself is part of
/// what it names: it hashes its target and then the content it resolves to. A dangling root
/// link still hashes (its target), with no content, so it differs from a link elsewhere.
pub fn tree_hash(path: &Path) -> Option<u64> {
    let mut entries = Vec::new();
    if fs::symlink_metadata(path).ok()?.file_type().is_symlink() {
        let target = fs::read_link(path).ok()?;
        entries.push((PathBuf::new(), KIND_LINK, target.as_os_str().as_bytes().to_vec()));
        match fs::metadata(path) {
            Ok(m) if m.is_dir() => gather_children(path, Path::new(""), &mut entries)?,
            Ok(m) if m.is_file() => entries.push((PathBuf::new(), KIND_FILE, fs::read(path).ok()?)),
            _ => {}
        }
    } else {
        collect(path, Path::new(""), &mut entries)?;
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut buf = Vec::new();
    for (rel, kind, payload) in &entries {
        push(&mut buf, rel.as_os_str().as_bytes());
        push(&mut buf, &[*kind]);
        push(&mut buf, payload);
    }
    Some(content_hash(&buf))
}

/// Gathers `(relative path, kind, payload)` for `path` and, if it is a folder, everything in it.
/// Uses `symlink_metadata`, so a link is recorded as a link and never followed.
fn collect(path: &Path, rel: &Path, out: &mut Vec<(PathBuf, u8, Vec<u8>)>) -> Option<()> {
    let file_type = fs::symlink_metadata(path).ok()?.file_type();
    if file_type.is_symlink() {
        let target = fs::read_link(path).ok()?;
        out.push((rel.to_path_buf(), KIND_LINK, target.as_os_str().as_bytes().to_vec()));
    } else if file_type.is_dir() {
        out.push((rel.to_path_buf(), KIND_DIR, Vec::new()));
        gather_children(path, rel, out)?;
    } else if file_type.is_file() {
        out.push((rel.to_path_buf(), KIND_FILE, fs::read(path).ok()?));
    } else {
        out.push((rel.to_path_buf(), KIND_OTHER, Vec::new()));
    }
    Some(())
}

/// Gathers every entry directly inside `dir`, each under `rel`.
fn gather_children(dir: &Path, rel: &Path, out: &mut Vec<(PathBuf, u8, Vec<u8>)>) -> Option<()> {
    for kid in fs::read_dir(dir).ok()?.collect::<Result<Vec<_>, _>>().ok()? {
        collect(&kid.path(), &rel.join(kid.file_name()), out)?;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn state(hash: Option<u64>) -> Vec<(PathBuf, Option<u64>)> {
        vec![(PathBuf::from("/skills/a/SKILL.md"), hash)]
    }

    #[test]
    fn same_plan_same_id() {
        let lines = vec!["link 3 skills".to_string()];
        assert_eq!(plan_id(&lines, &state(Some(1))), plan_id(&lines, &state(Some(1))));
    }

    #[test]
    fn changed_file_changes_id() {
        let lines = vec!["link 3 skills".to_string()];
        assert_ne!(plan_id(&lines, &state(Some(1))), plan_id(&lines, &state(Some(2))));
        assert_ne!(plan_id(&lines, &state(Some(1))), plan_id(&lines, &state(None)));
    }

    #[test]
    fn id_is_12_hex() {
        let id = plan_id(&[], &state(None));
        assert_eq!(id.len(), 12);
        assert!(id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    }

    #[test]
    fn lines_cannot_run_together() {
        let split = vec!["ab".to_string(), "c".to_string()];
        let joined = vec!["a".to_string(), "bc".to_string()];
        assert_ne!(plan_id(&split, &[]), plan_id(&joined, &[]));
    }

    /// FNV-1a is chosen so ids survive Rust upgrades; this pins one value to prove it.
    #[test]
    fn plan_id_is_pinned() {
        assert_eq!(plan_id(&["a".to_string()], &[]), "bcc19eca8dec");
    }

    #[test]
    fn tree_hash_sees_a_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let skill = dir.path().join("skill");
        fs::create_dir(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "hi").unwrap();
        let before = tree_hash(&skill);
        fs::write(skill.join("notes.md"), "x").unwrap();
        assert_ne!(before, tree_hash(&skill));
    }

    #[test]
    fn tree_hash_sees_changed_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let skill = dir.path().join("skill");
        fs::create_dir(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "hi").unwrap();
        let before = tree_hash(&skill);
        fs::write(skill.join("SKILL.md"), "ho").unwrap();
        assert_ne!(before, tree_hash(&skill));
    }

    #[test]
    fn tree_hash_sees_edits_through_a_root_link() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        fs::write(real.join("SKILL.md"), "hi").unwrap();
        let link = dir.path().join("link");
        symlink(&real, &link).unwrap();
        let before = tree_hash(&link);
        fs::write(real.join("SKILL.md"), "ho").unwrap();
        assert_ne!(before, tree_hash(&link));
    }

    #[test]
    fn nested_links_are_hashed_by_target_and_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let skill = dir.path().join("skill");
        fs::create_dir(&skill).unwrap();
        fs::write(dir.path().join("a.md"), "a").unwrap();
        fs::write(dir.path().join("b.md"), "b").unwrap();
        let nested = skill.join("ref.md");
        symlink(dir.path().join("a.md"), &nested).unwrap();
        let before = tree_hash(&skill);

        // Editing what the nested link points at does not change the folder's hash.
        fs::write(dir.path().join("a.md"), "changed").unwrap();
        assert_eq!(before, tree_hash(&skill));

        // Repointing the nested link does.
        fs::remove_file(&nested).unwrap();
        symlink(dir.path().join("b.md"), &nested).unwrap();
        assert_ne!(before, tree_hash(&skill));
    }

    #[test]
    fn dangling_root_link_hashes_to_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let there = dir.path().join("gone");
        let elsewhere = dir.path().join("also-gone");
        let link = dir.path().join("link");
        symlink(&there, &link).unwrap();
        let hash = tree_hash(&link);
        assert!(hash.is_some());
        fs::remove_file(&link).ok();
        symlink(&elsewhere, &link).unwrap();
        assert_ne!(hash, tree_hash(&link));
    }

    #[test]
    fn tree_hash_of_missing_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(tree_hash(&dir.path().join("nope")), None);
        assert_eq!(file_hash(&dir.path().join("nope")), None);
    }
}
