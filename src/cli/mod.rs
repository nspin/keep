use std::io::Write;

use anyhow::Result;
use git2::{FileMode, Repository};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

use crate::{sha256sum, Database, FilesystemSubstance, ShallowDifferenceSide, Snapshot, Substance};

mod args;

use args::{Args, Command};

pub fn cli_main() -> Result<()> {
    let args = Args::get()?;
    args.apply_verbosity();
    args.run_command()
}

impl Args {
    fn database(&self) -> Result<Database> {
        let git_dir = self.git_dir.as_ref().unwrap();
        Ok(Database::new(Repository::open_bare(git_dir)?))
    }

    fn substance(&self) -> Result<FilesystemSubstance> {
        let substance_dir = self.substance_dir.as_ref().unwrap();
        Ok(FilesystemSubstance::new(substance_dir))
    }

    fn apply_verbosity(&self) {
        const HACK_VERBOSITY: u64 = 2;
        let level_filter = match HACK_VERBOSITY + self.verbosity {
            0 => log::LevelFilter::Error,
            1 => log::LevelFilter::Warn,
            2 => log::LevelFilter::Info,
            3 => log::LevelFilter::Debug,
            _ => log::LevelFilter::Trace,
        };
        env_logger::builder().filter(None, level_filter).init();
    }

    fn run_command(&self) -> Result<()> {
        match &self.command {
            Command::Snapshot {
                subject,
                relative_path,
                force,
                remove_after,
                snapshot_dir,
            } => {
                let db = self.database()?;
                let substance = self.substance()?;
                let snapshot = Snapshot::new(&snapshot_dir);
                log::info!(
                    "taking snapshot of {} to {}",
                    subject.display(),
                    snapshot.path().display()
                );
                snapshot.take(&subject)?;
                log::info!("planting snapshot");
                let (mode, tree) = db.plant_snapshot(&snapshot)?;
                log::info!("planted: {:06o},{}", u32::from(mode), tree);
                log::info!("storing snapshot");
                db.store_snapshot(&substance, tree, &subject)?;
                // log::info!("adding snapshot to index at {}", relative_path);
                // db.add_to_index(mode, tree, relative_path)?;
                let parent = db.repository().head()?.peel_to_commit()?;
                let big_tree = parent.tree_id();
                log::info!(
                    "adding snapshot to HEAD^{{tree}} ({}) at {}",
                    big_tree,
                    relative_path
                );
                let new_big_tree = db.append(big_tree, &relative_path, mode, tree, *force)?;
                let commit =
                    db.commit_simple("x", &db.repository().find_tree(new_big_tree)?, &parent)?;
                log::info!("new commit is {}. merging --ff-only into HEAD", commit);
                db.safe_merge(commit)?;
                if *remove_after {
                    snapshot.remove()?;
                }
            }
            Command::Mount { mountpoint, tree, uid, gid } => {
                let db = self.database()?;
                let substance = self.substance()?;
                let tree = db.resolve_treeish(&tree)?;
                db.mount(tree, &mountpoint, substance, *uid, *gid)?;
            }
            Command::Diff { tree_a, tree_b } => {
                let db = self.database()?;
                let tree_a = db.resolve_treeish(&tree_a)?;
                let tree_b = db.resolve_treeish(&tree_b)?;
                let mut stdout = StandardStream::stdout(ColorChoice::Always);
                db.shallow_diff(tree_a, tree_b, |difference| {
                    let color = match difference.side {
                        ShallowDifferenceSide::A => Color::Red,
                        ShallowDifferenceSide::B => Color::Green,
                    };
                    stdout.set_color(ColorSpec::new().set_fg(Some(color)))?;
                    writeln!(&mut stdout, "{}", difference)?;
                    Ok(())
                })?;
                stdout.reset()?;
            }
            Command::Check { tree } => {
                let db = self.database()?;
                let tree = db.resolve_treeish(&tree)?;
                db.check(tree)?;
            }
            Command::UniqueBlobs { tree } => {
                let db = self.database()?;
                let tree = db.resolve_treeish(&tree)?;
                db.unique_shadows(tree, |path, blob| {
                    println!("{} {}", blob.content_hash(), path);
                    Ok(())
                })?;
            }
            Command::CheckBlobs { tree, deep } => {
                let db = self.database()?;
                let substance = self.substance()?;
                let tree = db.resolve_treeish(&tree)?;
                db.unique_shadows(tree, |path, blob| {
                    // TODO check size
                    if !substance.have_blob(blob.content_hash()) {
                        println!("missing blob: {} {}", blob.content_hash(), path);
                    }
                    if *deep {
                        if !substance.check_blob(blob.content_hash()).is_ok() {
                            println!("invalid blob: {} {}", blob.content_hash(), path);
                        }
                    }
                    Ok(())
                })?;
            }
            Command::Sha256Sum { path } => {
                let blob = sha256sum(path)?;
                println!("{} *{}", blob, path.display());
            }
            Command::TakeSnapshot { subject, out } => {
                let snapshot = Snapshot::new(out);
                snapshot.take(&subject)?;
            }
            Command::PlantSnapshot { snapshot } => {
                let db = self.database()?;
                let snapshot = Snapshot::new(snapshot);
                let (mode, tree) = db.plant_snapshot(&snapshot)?;
                println!("{:06o},{}", u32::from(mode), tree)
            }
            Command::StoreSnapshot { tree, subject } => {
                let db = self.database()?;
                let substance = self.substance()?;
                let tree = db.resolve_treeish(&tree)?;
                db.store_snapshot(&substance, tree, &subject)?;
            }
            Command::Append {
                big_tree,
                relative_path,
                mode,
                object,
                force,
            } => {
                let db = self.database()?;
                let big_tree = db.resolve_treeish(&big_tree)?;
                assert_eq!(mode, &format!("{:06o}", u32::from(FileMode::Tree)));
                let mode = FileMode::Tree;
                let object = db.resolve_treeish(&object)?;
                let new_tree = db.append(big_tree, &relative_path, mode, object, *force)?;
                println!("{}", new_tree)
            }
            Command::Remove {
                big_tree,
                relative_path,
            } => {
                let db = self.database()?;
                let big_tree = db.resolve_treeish(&big_tree)?;
                let new_tree = db.remove(big_tree, &relative_path)?;
                println!("{}", new_tree)
            }
            Command::AddToIndex {
                mode,
                tree,
                relative_path,
            } => {
                let db = self.database()?;
                let tree = db.resolve_treeish(&tree)?;
                assert_eq!(mode, &format!("{:06o}", u32::from(FileMode::Tree)));
                db.add_to_index(FileMode::Tree, tree, relative_path)?;
            }
        }
        Ok(())
    }
}
