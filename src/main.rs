mod commands;
mod object;
mod repo;
mod tree;

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "mgit",
    version,
    about = "A minimal, Git-compatible version control system"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create an empty repository in the current directory
    Init,
    /// Compute a file's object hash, optionally storing it
    HashObject {
        /// Write the object into the object database
        #[arg(short = 'w')]
        write: bool,
        file: PathBuf,
    },
    /// Show the content, type, or size of a stored object
    CatFile {
        #[command(flatten)]
        mode: CatModeArgs,
        /// Object hash (full or abbreviated) or HEAD
        object: String,
    },
    /// List the contents of a tree (or a commit's tree)
    LsTree {
        #[arg(long)]
        name_only: bool,
        object: String,
    },
    /// Snapshot the working directory as a tree object
    WriteTree,
    /// Record a snapshot of the working directory
    Commit {
        #[arg(short, long)]
        message: String,
    },
    /// Show commit history
    Log,
    /// Show changes since the last commit
    Status,
    /// Switch to a branch or restore the files of a past commit
    Checkout {
        /// Branch name, commit hash (full or abbreviated), or HEAD
        target: String,
        /// Discard local changes that would be overwritten
        #[arg(short, long)]
        force: bool,
    },
}

#[derive(Args)]
#[group(required = true, multiple = false)]
struct CatModeArgs {
    /// Pretty-print the object's content
    #[arg(short = 'p')]
    pretty: bool,
    /// Show the object's type
    #[arg(short = 't')]
    kind: bool,
    /// Show the object's size in bytes
    #[arg(short = 's')]
    size: bool,
}

fn main() {
    if let Err(err) = run(Cli::parse()) {
        eprintln!("fatal: {err:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Init => commands::init(),
        Command::HashObject { write, file } => commands::hash_object(&file, write),
        Command::CatFile { mode, object } => {
            let mode = if mode.pretty {
                commands::CatMode::Pretty
            } else if mode.kind {
                commands::CatMode::Type
            } else {
                commands::CatMode::Size
            };
            commands::cat_file(&object, mode)
        }
        Command::LsTree { name_only, object } => commands::ls_tree(&object, name_only),
        Command::WriteTree => commands::write_tree(),
        Command::Commit { message } => commands::commit(&message),
        Command::Log => commands::log(),
        Command::Status => commands::status(),
        Command::Checkout { target, force } => commands::checkout(&target, force),
    }
}
