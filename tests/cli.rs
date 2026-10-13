//! End-to-end tests that run the compiled `mgit` binary in a temp directory.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn mgit(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mgit"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to run mgit")
}

/// Run a command that must succeed and return its stdout.
fn ok(dir: &Path, args: &[&str]) -> String {
    let out = mgit(dir, args);
    assert!(
        out.status.success(),
        "mgit {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn read(dir: &Path, file: &str) -> String {
    fs::read_to_string(dir.join(file)).unwrap()
}

/// Make two commits and return the first commit's hash.
fn two_commit_repo(dir: &Path) -> String {
    ok(dir, &["init"]);
    fs::write(dir.join("a.txt"), "version 1").unwrap();
    ok(dir, &["commit", "-m", "first"]);
    let first = ok(dir, &["cat-file", "-p", "HEAD"]); // sanity check
    assert!(first.starts_with("tree "));
    let first_hash = fs::read_to_string(dir.join(".mgit/refs/heads/main"))
        .unwrap()
        .trim()
        .to_string();

    fs::write(dir.join("a.txt"), "version 2").unwrap();
    fs::create_dir_all(dir.join("docs")).unwrap();
    fs::write(dir.join("docs/new.md"), "added later").unwrap();
    ok(dir, &["commit", "-m", "second"]);
    first_hash
}

#[test]
fn checkout_restores_old_commit_and_returns_to_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let first = two_commit_repo(dir);

    // Go back in time using an abbreviated hash.
    let out = ok(dir, &["checkout", &first[..7]]);
    assert!(out.contains("detached HEAD"));
    assert_eq!(read(dir, "a.txt"), "version 1");
    assert!(!dir.join("docs").exists(), "file and empty dir removed");

    // Return to the branch tip.
    ok(dir, &["checkout", "main"]);
    assert_eq!(read(dir, "a.txt"), "version 2");
    assert_eq!(read(dir, "docs/new.md"), "added later");
    assert!(ok(dir, &["status"]).contains("On branch main"));
}

#[test]
fn checkout_refuses_to_destroy_uncommitted_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let first = two_commit_repo(dir);

    fs::write(dir.join("a.txt"), "unsaved work").unwrap();
    let out = mgit(dir, &["checkout", &first]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("a.txt"));
    assert_eq!(read(dir, "a.txt"), "unsaved work", "file must be untouched");

    // --force discards the change.
    ok(dir, &["checkout", "--force", &first]);
    assert_eq!(read(dir, "a.txt"), "version 1");
}

#[test]
fn untracked_files_survive_checkout() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let first = two_commit_repo(dir);

    fs::write(dir.join("scratch.txt"), "my notes").unwrap();
    ok(dir, &["checkout", &first]);
    assert_eq!(read(dir, "scratch.txt"), "my notes");
}
