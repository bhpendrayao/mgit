use crate::object::{Commit, Kind, Object};
use crate::repo::{self, Repo};
use crate::tree;
use anyhow::{bail, Result};
use chrono::DateTime;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn init() -> Result<()> {
    let path = repo::init(Path::new("."))?;
    println!("Initialized empty mgit repository in {}", path.display());
    Ok(())
}

pub fn hash_object(file: &Path, write: bool) -> Result<()> {
    let blob = Object::new(Kind::Blob, std::fs::read(file)?);
    let hash = if write {
        blob.write(&Repo::discover()?.git_dir)?
    } else {
        blob.hash()
    };
    println!("{hash}");
    Ok(())
}

pub enum CatMode {
    Pretty,
    Type,
    Size,
}

pub fn cat_file(name: &str, mode: CatMode) -> Result<()> {
    let repo = Repo::discover()?;
    let hash = repo.resolve(name)?;
    let obj = Object::read(&repo.git_dir, &hash)?;
    match mode {
        CatMode::Type => println!("{}", obj.kind),
        CatMode::Size => println!("{}", obj.data.len()),
        CatMode::Pretty => match obj.kind {
            Kind::Tree => print_tree(&obj.data, false)?,
            Kind::Blob | Kind::Commit => std::io::stdout().write_all(&obj.data)?,
        },
    }
    Ok(())
}

pub fn ls_tree(name: &str, name_only: bool) -> Result<()> {
    let repo = Repo::discover()?;
    let mut obj = Object::read(&repo.git_dir, &repo.resolve(name)?)?;
    // Like Git, accept a commit and show its root tree.
    if obj.kind == Kind::Commit {
        let commit = Commit::parse(&obj.data)?;
        obj = Object::read(&repo.git_dir, &commit.tree)?;
    }
    if obj.kind != Kind::Tree {
        bail!("'{name}' is not a tree");
    }
    print_tree(&obj.data, name_only)
}

fn print_tree(data: &[u8], name_only: bool) -> Result<()> {
    for e in tree::parse(data)? {
        if name_only {
            println!("{}", e.name);
        } else {
            let kind = if e.is_dir() { "tree" } else { "blob" };
            println!("{:0>6} {kind} {}\t{}", e.mode, e.hash, e.name);
        }
    }
    Ok(())
}

pub fn write_tree() -> Result<()> {
    let repo = Repo::discover()?;
    println!("{}", snapshot(&repo)?);
    Ok(())
}

/// Snapshot the working directory, returning the root tree hash.
fn snapshot(repo: &Repo) -> Result<String> {
    match tree::write_dir(&repo.git_dir, &repo.work_dir)? {
        Some(hash) => Ok(hash),
        None => bail!("nothing to snapshot: the working directory is empty"),
    }
}

pub fn commit(message: &str) -> Result<()> {
    let repo = Repo::discover()?;
    let tree_hash = snapshot(&repo)?;
    let parent = repo.head_commit()?;

    if let Some(p) = &parent {
        let prev = Commit::parse(&Object::read(&repo.git_dir, p)?.data)?;
        if prev.tree == tree_hash {
            println!("nothing to commit, working tree clean");
            return Ok(());
        }
    }

    let name = std::env::var("MGIT_AUTHOR_NAME").unwrap_or_else(|_| "mgit user".into());
    let email = std::env::var("MGIT_AUTHOR_EMAIL").unwrap_or_else(|_| "user@example.com".into());
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let identity = format!("{name} <{email}> {now} +0000");

    let mut body = format!("tree {tree_hash}\n");
    if let Some(p) = &parent {
        body.push_str(&format!("parent {p}\n"));
    }
    body.push_str(&format!("author {identity}\ncommitter {identity}\n\n"));
    body.push_str(message.trim_end());
    body.push('\n');

    let hash = Object::new(Kind::Commit, body.into_bytes()).write(&repo.git_dir)?;
    repo.update_head(&hash)?;

    let root = if parent.is_none() {
        " (root-commit)"
    } else {
        ""
    };
    println!(
        "[{}{root} {}] {}",
        repo.branch_name()?,
        &hash[..7],
        first_line(message)
    );
    Ok(())
}

pub fn log() -> Result<()> {
    let repo = Repo::discover()?;
    let mut current = match repo.head_commit()? {
        Some(h) => h,
        None => bail!(
            "your current branch '{}' has no commits yet",
            repo.branch_name()?
        ),
    };
    loop {
        let commit = Commit::parse(&Object::read(&repo.git_dir, &current)?.data)?;
        let (who, date) = split_identity(&commit.author);

        println!("\x1b[33mcommit {current}\x1b[0m");
        println!("Author: {who}");
        println!("Date:   {date}\n");
        for line in commit.message.lines() {
            println!("    {line}");
        }
        println!();

        // Follow the first parent (mgit has no merges).
        match commit.parents.into_iter().next() {
            Some(p) => current = p,
            None => break,
        }
    }
    Ok(())
}

pub fn status() -> Result<()> {
    let repo = Repo::discover()?;
    println!("On branch {}", repo.branch_name()?);

    let committed = match repo.head_commit()? {
        Some(h) => {
            let c = Commit::parse(&Object::read(&repo.git_dir, &h)?.data)?;
            tree::flatten(&repo.git_dir, &c.tree)?
        }
        None => {
            println!("\nNo commits yet");
            Default::default()
        }
    };
    let working = tree::scan_worktree(&repo.work_dir)?;

    let mut changes = Vec::new();
    for (path, hash) in &working {
        match committed.get(path) {
            None => changes.push(("\x1b[32mnew file:", path)),
            Some(old) if old != hash => changes.push(("\x1b[33mmodified:", path)),
            _ => {}
        }
    }
    for path in committed.keys() {
        if !working.contains_key(path) {
            changes.push(("\x1b[31mdeleted: ", path));
        }
    }

    if changes.is_empty() {
        println!("nothing to commit, working tree clean");
    } else {
        println!("\nChanges since last commit:");
        for (label, path) in changes {
            println!("  {label}   {path}\x1b[0m");
        }
    }
    Ok(())
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

/// Split `Name <email> 1700000000 +0000` into identity and a readable date.
fn split_identity(raw: &str) -> (String, String) {
    let mut parts = raw.rsplitn(3, ' ');
    let tz = parts.next().unwrap_or("");
    let ts = parts.next().unwrap_or("");
    let who = parts.next().unwrap_or(raw).to_string();
    let date = ts
        .parse::<i64>()
        .ok()
        .and_then(|t| DateTime::from_timestamp(t, 0))
        .map(|d| format!("{} {tz}", d.format("%a %b %-d %H:%M:%S %Y")))
        .unwrap_or_else(|| ts.to_string());
    (who, date)
}
