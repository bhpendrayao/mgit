# mgit

A minimal version control system written from scratch in Rust that stores data in
**Git's real on-disk format**. Repositories created by `mgit` can be read and
verified by the official `git` tool.

```console
$ mgit init
$ mgit commit -m "Initial commit"
[main (root-commit) 59b9bfc] Initial commit

$ git --git-dir=.mgit log --oneline     # real Git reads mgit's history
59b9bfc Initial commit
$ git --git-dir=.mgit fsck --full       # and validates every object
```

## Features

| Command | Description |
|---|---|
| `mgit init` | Create an empty repository |
| `mgit hash-object [-w] <file>` | Hash a file as a blob (optionally store it) |
| `mgit cat-file -p\|-t\|-s <obj>` | Print an object's content, type, or size |
| `mgit ls-tree [--name-only] <obj>` | List a tree (or a commit's tree) |
| `mgit write-tree` | Snapshot the working directory as a tree |
| `mgit commit -m <msg>` | Record a snapshot with history |
| `mgit log` | Show commit history |
| `mgit status` | Show new, modified, and deleted files |
| `mgit checkout [--force] <branch\|commit>` | Switch to a branch or restore a past commit |

Objects can be referenced by full hash, abbreviated hash (4+ characters), a branch name, or `HEAD`.

## How it works

Git is a **content-addressable filesystem**. Everything is an object identified
by the SHA-1 hash of its contents:

- **Blob**: raw file contents.
- **Tree**: a directory listing; each entry is `<mode> <name>\0<20-byte hash>`,
  sorted in Git's order (directories compare as if they end in `/`).
- **Commit**: a tree hash, parent commit(s), author, and message.

Each object is stored as `<type> <size>\0<data>`, zlib-compressed, at
`.mgit/objects/<first 2 hex chars>/<remaining 38>`. Branches are files under
`refs/heads/` holding a commit hash, and `HEAD` points at the current branch.

### Checkout

`checkout` compares three snapshots: the current commit, the target commit,
and the files on disk. It deletes files that exist only in the current commit,
rewrites files whose content differs from the target (restoring executable
bits), and leaves untracked files alone. Before touching anything, it refuses
to run if uncommitted changes would be lost, unless `--force` is given.

Checking out a branch name keeps HEAD attached (`ref: refs/heads/main`);
checking out a commit hash puts HEAD in **detached** mode, pointing directly
at the commit, just like Git.

`mgit` uses `.mgit/` instead of `.git/` so it can safely run inside an existing
Git repository without touching it.

## Project structure

```
src/
├── main.rs      # CLI definition (clap)
├── commands.rs  # command implementations
├── object.rs    # hashing, compression, object read/write, commit parsing
├── tree.rs      # tree format, directory snapshots, working-tree scanning
└── repo.rs      # repository discovery, refs, HEAD, hash-prefix resolution
```

## Build and run

```bash
cargo build --release
./target/release/mgit --help

# or install it on your PATH
cargo install --path .
```

Set your identity with `MGIT_AUTHOR_NAME` and `MGIT_AUTHOR_EMAIL`.

## Testing

```bash
cargo test
```

Unit tests check that hashes match real Git byte-for-byte (for example, the
well-known empty tree hash `4b825dc6…`). Integration tests in `tests/cli.rs`
run the compiled binary end to end, covering checkout, detached HEAD, and the
safety check that protects uncommitted work. CI runs formatting, Clippy, and tests on
Linux, macOS, and Windows.

## Roadmap

- [x] `checkout` to restore a previous commit
- [ ] Branches (`branch`, `switch`)
- [ ] A staging area (`add`) using Git's index format
- [ ] `diff` between commits
- [ ] `.mgitignore` support

## License

MIT
