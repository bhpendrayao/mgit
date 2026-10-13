//! Git's object database: every piece of content (file, directory, commit)
//! is stored as `<type> <size>\0<data>`, zlib-compressed, under a path
//! derived from the SHA-1 hash of that uncompressed byte string.

use anyhow::{bail, Context, Result};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use sha1::{Digest, Sha1};
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Blob,
    Tree,
    Commit,
}

impl Kind {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "blob" => Ok(Kind::Blob),
            "tree" => Ok(Kind::Tree),
            "commit" => Ok(Kind::Commit),
            other => bail!("unsupported object type '{other}'"),
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Kind::Blob => "blob",
            Kind::Tree => "tree",
            Kind::Commit => "commit",
        })
    }
}

#[derive(Debug, Clone)]
pub struct Object {
    pub kind: Kind,
    pub data: Vec<u8>,
}

impl Object {
    pub fn new(kind: Kind, data: Vec<u8>) -> Self {
        Self { kind, data }
    }

    /// The exact bytes Git hashes and stores: header + content.
    fn serialize(&self) -> Vec<u8> {
        let mut out = format!("{} {}\0", self.kind, self.data.len()).into_bytes();
        out.extend_from_slice(&self.data);
        out
    }

    pub fn hash(&self) -> String {
        format!("{:x}", Sha1::digest(self.serialize()))
    }

    /// Store the object in `.mgit/objects` and return its hash.
    /// Objects are immutable, so an existing file is never rewritten.
    pub fn write(&self, git_dir: &Path) -> Result<String> {
        let hash = self.hash();
        let path = object_path(git_dir, &hash);
        if path.exists() {
            return Ok(hash);
        }
        fs::create_dir_all(path.parent().unwrap())?;

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&self.serialize())?;
        let compressed = encoder.finish()?;

        // Write to a temp file first, then rename, so a crash never leaves
        // a half-written object behind.
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, compressed)?;
        fs::rename(&tmp, &path)?;
        Ok(hash)
    }

    pub fn read(git_dir: &Path, hash: &str) -> Result<Self> {
        let path = object_path(git_dir, hash);
        let compressed = fs::read(path).with_context(|| format!("object {hash} not found"))?;

        let mut raw = Vec::new();
        ZlibDecoder::new(&compressed[..])
            .read_to_end(&mut raw)
            .with_context(|| format!("object {hash} is corrupt"))?;

        let nul = raw
            .iter()
            .position(|&b| b == 0)
            .context("object header missing NUL byte")?;
        let header = std::str::from_utf8(&raw[..nul])?;
        let (kind, size) = header.split_once(' ').context("malformed object header")?;
        let kind = Kind::parse(kind)?;
        let size: usize = size.parse().context("invalid object size")?;

        let data = raw[nul + 1..].to_vec();
        if data.len() != size {
            bail!("object {hash}: expected {size} bytes, found {}", data.len());
        }
        Ok(Self { kind, data })
    }
}

/// Objects live at `objects/ab/cdef...`: the first two hex chars form a
/// directory so no single folder gets too large.
pub fn object_path(git_dir: &Path, hash: &str) -> PathBuf {
    git_dir.join("objects").join(&hash[..2]).join(&hash[2..])
}

/// Hash a blob without storing it (used by `status`).
pub fn blob_hash(data: &[u8]) -> String {
    Object::new(Kind::Blob, data.to_vec()).hash()
}

/// A parsed commit object.
#[derive(Debug)]
pub struct Commit {
    pub tree: String,
    pub parents: Vec<String>,
    pub author: String,
    pub message: String,
}

impl Commit {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(data).context("commit is not valid UTF-8")?;
        let (headers, message) = text.split_once("\n\n").unwrap_or((text, ""));

        let mut tree = None;
        let mut parents = Vec::new();
        let mut author = String::new();
        for line in headers.lines() {
            match line.split_once(' ') {
                Some(("tree", v)) => tree = Some(v.to_string()),
                Some(("parent", v)) => parents.push(v.to_string()),
                Some(("author", v)) => author = v.to_string(),
                _ => {}
            }
        }
        Ok(Self {
            tree: tree.context("commit has no tree")?,
            parents,
            author,
            message: message.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_hash_matches_real_git() {
        // `echo "hello world" | git hash-object --stdin`
        assert_eq!(
            blob_hash(b"hello world\n"),
            "3b18e512dba79e4c8300dd08aeb37f8e728b8dad"
        );
    }

    #[test]
    fn write_then_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let obj = Object::new(Kind::Blob, b"some content".to_vec());
        let hash = obj.write(dir.path()).unwrap();
        let back = Object::read(dir.path(), &hash).unwrap();
        assert_eq!(back.kind, Kind::Blob);
        assert_eq!(back.data, b"some content");
    }

    #[test]
    fn parses_commit() {
        let raw = b"tree abc\nparent def\nauthor A <a@b.c> 0 +0000\n\nhello\n";
        let c = Commit::parse(raw).unwrap();
        assert_eq!(c.tree, "abc");
        assert_eq!(c.parents, vec!["def"]);
        assert_eq!(c.message, "hello\n");
    }
}
