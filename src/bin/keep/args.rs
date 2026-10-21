use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use clap::{ArgAction, Parser, Subcommand};

use keep::ShadowPath;

#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(name = "keep", arg_required_else_help = false)]
pub struct Args {
    #[arg(long, short = 'd', env = "KEEP_GIT_DIR")]
    pub git_dir: Option<PathBuf>,
    #[arg(long, short = 's', env = "KEEP_SUBSTANCE_DIR")]
    pub substance_dir: Option<PathBuf>,
    #[arg(
        long = "ro",
        short = 'r',
        help = "Constrains execution to read-only operations."
    )]
    pub read_only: bool,
    #[arg(
        short = 'v',
        action = ArgAction::Count,
        help = "Sets the verbosity level (supply more than once for increased verbosity)"
    )]
    pub verbosity: u8,
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
    },
    TakeSnapshot {
        subject: PathBuf,
    },
    StoreSnapshot {
        #[arg(long, short = 'm', default_value = "040000")]
        mode: String,
        object: String,
        subject: PathBuf,
    },
    Append {
        #[arg(long, short = 'm', default_value = "040000")]
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
    Mount {
        mountpoint: PathBuf,
        #[arg(default_value = "HEAD")]
        tree: String,
        #[arg(long, short = 'u', default_value = "0")]
        uid: u32,
        #[arg(long, short = 'g', default_value = "0")]
        gid: u32,
    },
    Check {
        #[arg(long, short = 'm', default_value = "040000")]
        mode: String,
        #[arg(default_value = "HEAD")]
        object: String,
    },
    CheckContent {
        #[arg(long, short = 'm', default_value = "040000")]
        mode: String,
        #[arg(default_value = "HEAD")]
        object: String,
        #[arg(long)]
        deep: bool,
    },
    UniqueShadows {
        #[arg(long, short = 'm', default_value = "040000")]
        mode: String,
        #[arg(default_value = "HEAD")]
        object: String,
    },
    #[command(about = "Default: HEAD _ or HEAD^ HEAD.")]
    Diff {
        tree_a: Option<String>,
        tree_b: Option<String>,
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
            | Command::StoreSnapshot { .. }
            | Command::Mount { .. }
            | Command::CheckContent { .. } => (true, true),
            Command::TakeSnapshot { .. }
            | Command::Append { .. }
            | Command::Remove { .. }
            | Command::Check { .. }
            | Command::UniqueShadows { .. }
            | Command::Diff { .. } => (true, false),
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
