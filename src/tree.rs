//! Tree objects represent directories. Each entry is stored as
//! `<mode> <name>\0<20 raw SHA-1 bytes>`, sorted the way Git sorts them.

use crate::object::{blob_hash, Kind, Object};
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub const MODE_FILE: &str = "100644";
pub const MODE_EXEC: &str = "100755";
pub const MODE_DIR: &str = "40000";

/// Names never included in a snapshot.
const IGNORED: &[&str] = &[".git", ".mgit", "target"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub mode: String,
    pub name: String,
    pub hash: String,
}

impl TreeEntry {
    pub fn is_dir(&self) -> bool {
        self.mode == MODE_DIR
    }
}

pub fn parse(data: &[u8]) -> Result<Vec<TreeEntry>> {
    let mut entries = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        let space = rest
            .iter()
            .position(|&b| b == b' ')
            .context("bad tree entry")?;
        let nul = rest
            .iter()
            .position(|&b| b == 0)
            .context("bad tree entry")?;
        if nul + 21 > rest.len() {
            bail!("truncated tree entry");
        }
        entries.push(TreeEntry {
            mode: String::from_utf8(rest[..space].to_vec())?,
            name: String::from_utf8(rest[space + 1..nul].to_vec())?,
            hash: hex::encode(&rest[nul + 1..nul + 21]),
        });
        rest = &rest[nul + 21..];
    }
    Ok(entries)
}

pub fn serialize(entries: &mut [TreeEntry]) -> Result<Vec<u8>> {
    // Git compares directory names as if they ended in '/'.
    entries.sort_by_key(sort_key);
    let mut out = Vec::new();
    for e in entries.iter() {
        out.extend_from_slice(format!("{} {}\0", e.mode, e.name).as_bytes());
        out.extend_from_slice(&hex::decode(&e.hash)?);
    }
    Ok(out)
}

fn sort_key(e: &TreeEntry) -> Vec<u8> {
    let mut key = e.name.as_bytes().to_vec();
    if e.is_dir() {
        key.push(b'/');
    }
    key
}

/// Recursively snapshot `dir` into tree and blob objects.
/// Returns `None` for directories with nothing to track (Git ignores
/// empty directories too).
pub fn write_dir(git_dir: &Path, dir: &Path) -> Result<Option<String>> {
    let mut entries = Vec::new();
    for item in fs::read_dir(dir)? {
        let item = item?;
        let name = item.file_name().to_string_lossy().into_owned();
        if IGNORED.contains(&name.as_str()) {
            continue;
        }
        let path = item.path();
        let meta = fs::symlink_metadata(&path)?;

        if meta.is_dir() {
            if let Some(hash) = write_dir(git_dir, &path)? {
                entries.push(TreeEntry {
                    mode: MODE_DIR.into(),
                    name,
                    hash,
                });
            }
        } else if meta.is_file() {
            let blob = Object::new(Kind::Blob, fs::read(&path)?);
            let hash = blob.write(git_dir)?;
            entries.push(TreeEntry {
                mode: file_mode(&meta).into(),
                name,
                hash,
            });
        }
        // Symlinks and other special files are skipped for simplicity.
    }

    if entries.is_empty() {
        return Ok(None);
    }
    let tree = Object::new(Kind::Tree, serialize(&mut entries)?);
    Ok(Some(tree.write(git_dir)?))
}

#[cfg(unix)]
fn file_mode(meta: &fs::Metadata) -> &'static str {
    use std::os::unix::fs::PermissionsExt;
    if meta.permissions().mode() & 0o111 != 0 {
        MODE_EXEC
    } else {
        MODE_FILE
    }
}

#[cfg(not(unix))]
fn file_mode(_meta: &fs::Metadata) -> &'static str {
    MODE_FILE
}

/// A file recorded in a commit: its blob hash and mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub hash: String,
    pub mode: String,
}

/// Flatten a stored tree into `path -> file entry`.
pub fn flatten(git_dir: &Path, tree_hash: &str) -> Result<BTreeMap<String, FileEntry>> {
    let mut out = BTreeMap::new();
    flatten_into(git_dir, tree_hash, "", &mut out)?;
    Ok(out)
}

fn flatten_into(
    git_dir: &Path,
    tree_hash: &str,
    prefix: &str,
    out: &mut BTreeMap<String, FileEntry>,
) -> Result<()> {
    let obj = Object::read(git_dir, tree_hash)?;
    for e in parse(&obj.data)? {
        let path = format!("{prefix}{}", e.name);
        if e.is_dir() {
            flatten_into(git_dir, &e.hash, &format!("{path}/"), out)?;
        } else {
            out.insert(
                path,
                FileEntry {
                    hash: e.hash,
                    mode: e.mode,
                },
            );
        }
    }
    Ok(())
}

/// Flatten the working directory into `path -> blob hash` without
/// writing anything to the object store.
pub fn scan_worktree(dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    scan_into(dir, "", &mut out)?;
    Ok(out)
}

fn scan_into(dir: &Path, prefix: &str, out: &mut BTreeMap<String, String>) -> Result<()> {
    for item in fs::read_dir(dir)? {
        let item = item?;
        let name = item.file_name().to_string_lossy().into_owned();
        if IGNORED.contains(&name.as_str()) {
            continue;
        }
        let path = item.path();
        let meta = fs::symlink_metadata(&path)?;
        let rel = format!("{prefix}{name}");
        if meta.is_dir() {
            scan_into(&path, &format!("{rel}/"), out)?;
        } else if meta.is_file() {
            out.insert(rel, blob_hash(&fs::read(&path)?));
        }
    }
    Ok(())
}

/// Apply a stored mode to a file on disk (no-op on non-Unix systems).
#[cfg(unix)]
pub fn set_mode(path: &Path, mode: &str) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let bits = if mode == MODE_EXEC { 0o755 } else { 0o644 };
    fs::set_permissions(path, fs::Permissions::from_mode(bits))?;
    Ok(())
}

#[cfg(not(unix))]
pub fn set_mode(_path: &Path, _mode: &str) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tree_hash_matches_git() {
        let tree = Object::new(Kind::Tree, serialize(&mut []).unwrap());
        assert_eq!(tree.hash(), "4b825dc642cb6eb9a060e54bf8d69288fbee4904");
    }

    #[test]
    fn serialize_parse_roundtrip() {
        let mut entries = vec![
            TreeEntry {
                mode: MODE_FILE.into(),
                name: "b.txt".into(),
                hash: "a".repeat(40),
            },
            TreeEntry {
                mode: MODE_DIR.into(),
                name: "a".into(),
                hash: "b".repeat(40),
            },
        ];
        let bytes = serialize(&mut entries).unwrap();
        let parsed = parse(&bytes).unwrap();
        assert_eq!(parsed, entries); // `entries` is now sorted
        assert_eq!(parsed[0].name, "a");
    }
}
