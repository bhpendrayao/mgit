//! Locating the repository and managing refs (branches and HEAD).

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// mgit uses `.mgit` instead of `.git` so it can safely run inside a
/// real Git repository. The on-disk format is still Git-compatible:
/// `git --git-dir=.mgit log` works on an mgit repository.
pub const GIT_DIR: &str = ".mgit";

pub struct Repo {
    pub git_dir: PathBuf,
    pub work_dir: PathBuf,
}

impl Repo {
    /// Walk up from the current directory until a `.mgit` folder is found.
    pub fn discover() -> Result<Self> {
        let mut dir = std::env::current_dir()?;
        loop {
            let candidate = dir.join(GIT_DIR);
            if candidate.is_dir() {
                return Ok(Self {
                    git_dir: candidate,
                    work_dir: dir,
                });
            }
            if !dir.pop() {
                bail!("not an mgit repository (run `mgit init` first)");
            }
        }
    }

    /// The ref HEAD points to, e.g. `refs/heads/main`.
    pub fn head_ref(&self) -> Result<String> {
        let head = fs::read_to_string(self.git_dir.join("HEAD"))?;
        head.trim()
            .strip_prefix("ref: ")
            .map(str::to_string)
            .context("detached HEAD is not supported")
    }

    pub fn branch_name(&self) -> Result<String> {
        let r = self.head_ref()?;
        Ok(r.trim_start_matches("refs/heads/").to_string())
    }

    /// The commit HEAD points to, or `None` before the first commit.
    pub fn head_commit(&self) -> Result<Option<String>> {
        let path = self.git_dir.join(self.head_ref()?);
        match fs::read_to_string(path) {
            Ok(s) => Ok(Some(s.trim().to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn update_head(&self, commit: &str) -> Result<()> {
        let path = self.git_dir.join(self.head_ref()?);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, format!("{commit}\n"))?;
        Ok(())
    }

    /// Accept `HEAD`, a full hash, or an abbreviated hash (4+ chars).
    pub fn resolve(&self, name: &str) -> Result<String> {
        if name == "HEAD" {
            return self.head_commit()?.context("HEAD has no commits yet");
        }
        if name.len() < 4 || !name.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("'{name}' is not a valid object name");
        }
        let name = name.to_ascii_lowercase();
        let dir = self.git_dir.join("objects").join(&name[..2]);
        let rest = &name[2..];

        let matches: Vec<String> = match fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|f| f.starts_with(rest))
                .map(|f| format!("{}{f}", &name[..2]))
                .collect(),
            Err(_) => Vec::new(),
        };
        match matches.len() {
            0 => bail!("object '{name}' not found"),
            1 => Ok(matches.into_iter().next().unwrap()),
            n => bail!("'{name}' is ambiguous ({n} matches)"),
        }
    }
}

pub fn init(path: &Path) -> Result<PathBuf> {
    let git_dir = path.join(GIT_DIR);
    if git_dir.exists() {
        bail!("repository already exists at {}", git_dir.display());
    }
    fs::create_dir_all(git_dir.join("objects"))?;
    fs::create_dir_all(git_dir.join("refs").join("heads"))?;
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n")?;
    Ok(fs::canonicalize(git_dir)?)
}
