use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use clap::{ArgAction, Parser, Subcommand};

use crate::ShadowPath;

#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(name = "keep", arg_required_else_help = false)]
pub struct Args {
    #[arg(long, env = "GIT_DIR")]
    pub git_dir: Option<PathBuf>,
    #[arg(long, env = "SUBSTANCE_DIR")]
    pub substance_dir: Option<PathBuf>,
    #[arg(
        short = 'v',
        action = ArgAction::Count,
        help = "Sets the verbosity level (supply more than once for increased verbosity)"
    )]
    pub verbosity: u8,
    #[arg(long = "ro", help = "Constrains execution to read-only operations.")]
    pub read_only: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Command {
    Snapshot {
        subject: PathBuf,
        relative_path: ShadowPath,
        #[arg(long, short = 'f', help = "Replace RELATIVE_PATH if it exists.")]
        force: bool,
        #[arg(long = "rm", help = "Remove snapshot afterwards if success.")]
        remove_after: bool,
        #[arg(long, short = 'd', default_value = "tmp.snapshot")]
        snapshot_dir: PathBuf,
    },
    Mount {
        mountpoint: PathBuf,
        #[arg(default_value = "HEAD")]
        tree: String,
        #[arg(long, short = 'u', default_value = "0")]
        uid: u32,
        #[arg(long, short = 'g', default_value = "0")]
        gid: u32,
    },
    #[command(about = "Default: HEAD _ or HEAD^ HEAD.")]
    Diff {
        tree_a: Option<String>,
        tree_b: Option<String>,
    },
    Check {
        #[arg(default_value = "HEAD")]
        tree: String,
    },
    UniqueBlobs {
        #[arg(default_value = "HEAD")]
        tree: String,
    },
    CheckBlobs {
        #[arg(default_value = "HEAD")]
        tree: String,
        #[arg(long)]
        deep: bool,
    },
    #[command(name = "sha256sum")]
    Sha256Sum {
        path: PathBuf,
    },
    TakeSnapshot {
        subject: PathBuf,
        out: PathBuf,
    },
    PlantSnapshot {
        snapshot: PathBuf,
    },
    StoreSnapshot {
        tree: String,
        subject: PathBuf,
    },
    Append {
        mode: String,
        object: String,
        relative_path: ShadowPath,
        #[arg(default_value = "HEAD")]
        big_tree: String,
        #[arg(long, short = 'f', help = "Replace RELATIVE_PATH if it exists.")]
        force: bool,
    },
    Remove {
        relative_path: ShadowPath,
        #[arg(default_value = "HEAD")]
        big_tree: String,
    },
    AddToIndex {
        mode: String,
        tree: String,
        relative_path: ShadowPath,
    },
}

impl Args {
    pub fn get() -> Result<Self> {
        Self::parse().finish()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn get_from<I, T>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        Self::try_parse_from(args)?.finish()
    }

    fn finish(self) -> Result<Self> {
        let (needs_git_dir, needs_substance_dir) = match self.command {
            Command::Snapshot { .. }
            | Command::Mount { .. }
            | Command::CheckBlobs { .. }
            | Command::StoreSnapshot { .. } => (true, true),
            Command::Diff { .. }
            | Command::Check { .. }
            | Command::UniqueBlobs { .. }
            | Command::PlantSnapshot { .. }
            | Command::Append { .. }
            | Command::Remove { .. }
            | Command::AddToIndex { .. } => (true, false),
            Command::Sha256Sum { .. } | Command::TakeSnapshot { .. } => (false, false),
        };
        if needs_git_dir && self.git_dir.is_none() {
            return Err(anyhow!("missing '--git-dir'"));
        }
        if needs_substance_dir && self.substance_dir.is_none() {
            return Err(anyhow!("missing '--substance-dir'"));
        }

        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse() {
        Args::get_from(vec![
            "",
            "--git-dir",
            "x/y",
            "--substance-dir",
            "y/x",
            "mount",
            "a/b/c",
        ])
        .unwrap();
    }
}
