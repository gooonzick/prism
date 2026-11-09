//! Ids for printed plans: a plan's id changes when the state it was made from changes, so
//! `--apply --plan-id` can refuse a stale plan.
//!
//! Hashes come from `fsops::content_hash` (FNV-1a), not `DefaultHasher`, whose output is not
//! stable across Rust releases: an id printed by one build must still match on the next.

// Nothing calls this yet; the CLI's plan step does. Drop this once it does.
#![allow(dead_code)]

use std::fs;
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
        push(&mut buf, path.as_os_str().as_encoded_bytes());
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
pub fn file_hash(path: &Path) -> Option<u64> {
    fs::read(path).ok().map(|b| content_hash(&b))
}

/// Hash of a skill folder: every entry's relative path, kind, link target and file bytes,
/// sorted by path. Links are not followed. A link at `path` itself hashes its target.
pub fn tree_hash(path: &Path) -> Option<u64> {
    let mut entries = Vec::new();
    collect(path, Path::new(""), &mut entries)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut buf = Vec::new();
    for (rel, kind, payload) in &entries {
        push(&mut buf, rel.as_os_str().as_encoded_bytes());
        push(&mut buf, &[*kind]);
        push(&mut buf, payload);
    }
    Some(content_hash(&buf))
}

/// Gathers `(relative path, kind, payload)` for `path` and, if it is a folder, everything in it.
fn collect(path: &Path, rel: &Path, out: &mut Vec<(PathBuf, u8, Vec<u8>)>) -> Option<()> {
    let file_type = fs::symlink_metadata(path).ok()?.file_type();
    if file_type.is_symlink() {
        let target = fs::read_link(path).ok()?;
        out.push((rel.to_path_buf(), KIND_LINK, target.as_os_str().as_encoded_bytes().to_vec()));
    } else if file_type.is_dir() {
        out.push((rel.to_path_buf(), KIND_DIR, Vec::new()));
        for kid in fs::read_dir(path).ok()?.collect::<Result<Vec<_>, _>>().ok()? {
            collect(&kid.path(), &rel.join(kid.file_name()), out)?;
        }
    } else if file_type.is_file() {
        out.push((rel.to_path_buf(), KIND_FILE, fs::read(path).ok()?));
    } else {
        out.push((rel.to_path_buf(), KIND_OTHER, Vec::new()));
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn tree_hash_sees_a_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let skill = dir.path().join("skill");
        std::fs::create_dir(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "hi").unwrap();
        let before = tree_hash(&skill);
        std::fs::write(skill.join("notes.md"), "x").unwrap();
        assert_ne!(before, tree_hash(&skill));
    }

    #[test]
    fn tree_hash_of_missing_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(tree_hash(&dir.path().join("nope")), None);
        assert_eq!(file_hash(&dir.path().join("nope")), None);
    }
}
