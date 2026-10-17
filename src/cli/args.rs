use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::string::ToString;

use anyhow::{anyhow, Result};
use clap::{Arg, ArgAction, ArgMatches, Command as ClapCommand};

use crate::ShadowPath;

const ENV_GIT_DIR: &str = "GIT_DIR";
const ENV_SUBSTANCE_DIR: &str = "SUBSTANCE_DIR";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub git_dir: Option<PathBuf>,
    pub substance_dir: Option<PathBuf>,
    pub read_only: bool,
    pub verbosity: u64,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Snapshot {
        subject: PathBuf,
        relative_path: ShadowPath,
        force: bool,
        remove_after: bool,
        snapshot_dir: PathBuf,
    },
    Mount {
        mountpoint: PathBuf,
        tree: String,
        uid: u32,
        gid: u32,
    },
    Diff {
        tree_a: String,
        tree_b: String,
    },
    Check {
        tree: String,
    },
    UniqueBlobs {
        tree: String,
    },
    CheckBlobs {
        tree: String,
        deep: bool,
    },
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
        big_tree: String,
        relative_path: ShadowPath,
        mode: String,
        object: String,
        force: bool,
    },
    Remove {
        big_tree: String,
        relative_path: ShadowPath,
    },
    AddToIndex {
        mode: String,
        tree: String,
        relative_path: ShadowPath,
    },
}

fn app() -> ClapCommand {
    ClapCommand::new("keep")
        .subcommand_required(true)
        .arg(
            Arg::new("git-dir")
                .long("git-dir")
                .value_name("GIT_DIR")
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("substance-dir")
                .long("substance-dir")
                .value_name("SUBSTANCE_DIR")
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("v")
                .short('v')
                .action(ArgAction::Count)
                .help("Sets the verbosity level (supply more than once for increased verbosity)"),
        )
        .arg(
            Arg::new("read-only")
                .long("ro")
                .action(ArgAction::SetTrue)
                .help("Constrains execution to read-only operations."),
        )
        .subcommand(
            ClapCommand::new("snapshot")
                .arg(
                    Arg::new("force")
                        .long("force")
                        .short('f')
                        .action(ArgAction::SetTrue)
                        .help("Replace RELATIVE_PATH if it exists."),
                )
                .arg(
                    Arg::new("remove_after")
                        .long("rm")
                        .action(ArgAction::SetTrue)
                        .help("Remove snapshot afterwards if success."),
                )
                .arg(
                    Arg::new("snapshot_dir")
                        .long("snapshot-dir")
                        .short('d')
                        .value_name("SNAPSHOT_DIR")
                        .default_value("tmp.snapshot")
                        .action(ArgAction::Set),
                )
                .arg(Arg::new("SUBJECT").required(true).index(1))
                .arg(Arg::new("RELATIVE_PATH").required(true).index(2)),
        )
        .subcommand(
            ClapCommand::new("mount")
                .arg(Arg::new("MOUNTPOINT").required(true).index(1))
                .arg(Arg::new("TREE").default_value("HEAD").index(2))
                .arg(
                    Arg::new("uid")
                        .long("uid")
                        .short('u')
                        .value_name("UID")
                        .default_value("0")
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("gid")
                        .long("gid")
                        .short('g')
                        .value_name("GID")
                        .default_value("0")
                        .action(ArgAction::Set),
                ),
        )
        .subcommand(
            ClapCommand::new("diff")
                .arg(Arg::new("TREE_A").index(1))
                .arg(Arg::new("TREE_B").index(2))
                .about("Default: HEAD _ or HEAD^ HEAD."),
        )
        .subcommand(
            ClapCommand::new("check").arg(Arg::new("TREE").default_value("HEAD").index(1)),
        )
        .subcommand(
            ClapCommand::new("unique-blobs").arg(Arg::new("TREE").default_value("HEAD").index(1)),
        )
        .subcommand(
            ClapCommand::new("check-blobs")
                .arg(Arg::new("TREE").default_value("HEAD").index(1))
                .arg(Arg::new("deep").long("deep").action(ArgAction::SetTrue)),
        )
        .subcommand(ClapCommand::new("sha256sum").arg(Arg::new("PATH").required(true).index(1)))
        .subcommand(
            ClapCommand::new("take-snapshot")
                .arg(Arg::new("SUBJECT").required(true).index(1))
                .arg(Arg::new("OUT").required(true).index(2)),
        )
        .subcommand(
            ClapCommand::new("plant-snapshot")
                .arg(Arg::new("SNAPSHOT").required(true).index(1)),
        )
        .subcommand(
            ClapCommand::new("store-snapshot")
                .arg(Arg::new("TREE").required(true).index(1))
                .arg(Arg::new("SUBJECT").required(true).index(2)),
        )
        .subcommand(
            ClapCommand::new("append")
                .arg(
                    Arg::new("force")
                        .long("force")
                        .short('f')
                        .action(ArgAction::SetTrue)
                        .help("Replace RELATIVE_PATH if it exists."),
                )
                .arg(Arg::new("MODE").required(true).index(1))
                .arg(Arg::new("OBJECT").required(true).index(2))
                .arg(Arg::new("RELATIVE_PATH").required(true).index(3))
                .arg(Arg::new("BIG_TREE").default_value("HEAD").index(4)),
        )
        .subcommand(
            ClapCommand::new("remove")
                .arg(Arg::new("RELATIVE_PATH").required(true).index(1))
                .arg(Arg::new("BIG_TREE").default_value("HEAD").index(2)),
        )
        .subcommand(
            ClapCommand::new("add-to-index")
                .arg(Arg::new("MODE").required(true).index(1))
                .arg(Arg::new("TREE").required(true).index(2))
                .arg(Arg::new("RELATIVE_PATH").required(true).index(3)),
        )
}

impl Args {
    pub fn get() -> Result<Self> {
        Self::match_(app().get_matches())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn get_from<I, T>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        Self::match_(app().try_get_matches_from(args)?)
    }

    fn match_(matches: ArgMatches) -> Result<Self> {
        let git_dir = value(&matches, "git-dir")
            .map(PathBuf::from)
            .or_else(|| path_from_env(ENV_GIT_DIR));
        let substance_dir = value(&matches, "substance-dir")
            .map(PathBuf::from)
            .or_else(|| path_from_env(ENV_SUBSTANCE_DIR));
        let read_only = matches.get_flag("read-only");
        let verbosity = u64::from(matches.get_count("v"));

        let ensure_git_dir = || {
            if git_dir.is_none() {
                Err(anyhow!("missing '--git-dir'"))
            } else {
                Ok(())
            }
        };

        let ensure_substance_dir = || {
            if substance_dir.is_none() {
                Err(anyhow!("missing '--substance-dir'"))
            } else {
                Ok(())
            }
        };

        let command = if let Some(submatches) = matches.subcommand_matches("snapshot") {
            ensure_git_dir()?;
            ensure_substance_dir()?;
            Command::Snapshot {
                subject: value(submatches, "SUBJECT").unwrap().parse()?,
                relative_path: value(submatches, "RELATIVE_PATH").unwrap().parse()?,
                force: submatches.get_flag("force"),
                remove_after: submatches.get_flag("remove_after"),
                snapshot_dir: value(submatches, "snapshot_dir").unwrap().parse()?,
            }
        } else if let Some(submatches) = matches.subcommand_matches("mount") {
            ensure_git_dir()?;
            ensure_substance_dir()?;
            Command::Mount {
                mountpoint: value(submatches, "MOUNTPOINT").unwrap().parse()?,
                tree: value(submatches, "TREE").unwrap().to_string(),
                uid: value(submatches, "uid").unwrap().parse()?,
                gid: value(submatches, "gid").unwrap().parse()?,
            }
        } else if let Some(submatches) = matches.subcommand_matches("diff") {
            ensure_git_dir()?;
            let (tree_a, tree_b) =
                match (value(submatches, "TREE_A"), value(submatches, "TREE_B")) {
                    (None, None) => ("HEAD^", "HEAD"),
                    (Some(tree_a), None) => ("HEAD", tree_a),
                    (Some(tree_a), Some(tree_b)) => (tree_a, tree_b),
                    _ => panic!(),
                };
            Command::Diff {
                tree_a: tree_a.to_string(),
                tree_b: tree_b.to_string(),
            }
        } else if let Some(submatches) = matches.subcommand_matches("check") {
            ensure_git_dir()?;
            Command::Check {
                tree: value(submatches, "TREE").unwrap().to_string(),
            }
        } else if let Some(submatches) = matches.subcommand_matches("unique-blobs") {
            ensure_git_dir()?;
            Command::UniqueBlobs {
                tree: value(submatches, "TREE").unwrap().to_string(),
            }
        } else if let Some(submatches) = matches.subcommand_matches("check-blobs") {
            ensure_git_dir()?;
            ensure_substance_dir()?;
            Command::CheckBlobs {
                tree: value(submatches, "TREE").unwrap().to_string(),
                deep: submatches.get_flag("deep"),
            }
        } else if let Some(submatches) = matches.subcommand_matches("sha256sum") {
            Command::Sha256Sum {
                path: value(submatches, "PATH").unwrap().parse()?,
            }
        } else if let Some(submatches) = matches.subcommand_matches("take-snapshot") {
            Command::TakeSnapshot {
                subject: value(submatches, "SUBJECT").unwrap().parse()?,
                out: value(submatches, "OUT").unwrap().parse()?,
            }
        } else if let Some(submatches) = matches.subcommand_matches("plant-snapshot") {
            ensure_git_dir()?;
            Command::PlantSnapshot {
                snapshot: value(submatches, "SNAPSHOT").unwrap().parse()?,
            }
        } else if let Some(submatches) = matches.subcommand_matches("store-snapshot") {
            ensure_git_dir()?;
            ensure_substance_dir()?;
            Command::StoreSnapshot {
                tree: value(submatches, "TREE").unwrap().parse()?,
                subject: value(submatches, "SUBJECT").unwrap().parse()?,
            }
        } else if let Some(submatches) = matches.subcommand_matches("append") {
            ensure_git_dir()?;
            Command::Append {
                big_tree: value(submatches, "BIG_TREE").unwrap().parse()?,
                relative_path: value(submatches, "RELATIVE_PATH").unwrap().parse()?,
                mode: value(submatches, "MODE").unwrap().parse()?,
                object: value(submatches, "OBJECT").unwrap().parse()?,
                force: submatches.get_flag("force"),
            }
        } else if let Some(submatches) = matches.subcommand_matches("remove") {
            ensure_git_dir()?;
            Command::Remove {
                big_tree: value(submatches, "BIG_TREE").unwrap().parse()?,
                relative_path: value(submatches, "RELATIVE_PATH").unwrap().parse()?,
            }
        } else if let Some(submatches) = matches.subcommand_matches("add-to-index") {
            ensure_git_dir()?;
            Command::AddToIndex {
                mode: value(submatches, "MODE").unwrap().parse()?,
                tree: value(submatches, "TREE").unwrap().parse()?,
                relative_path: value(submatches, "RELATIVE_PATH").unwrap().parse()?,
            }
        } else {
            panic!()
        };

        Ok(Args {
            git_dir,
            substance_dir,
            read_only,
            verbosity,
            command,
        })
    }
}

fn value<'a>(matches: &'a ArgMatches, id: &str) -> Option<&'a str> {
    matches.get_one::<String>(id).map(String::as_str)
}

fn path_from_env(var: &str) -> Option<PathBuf> {
    env::var_os(var).map(|s| <OsString as AsRef<Path>>::as_ref(&s).to_path_buf())
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
