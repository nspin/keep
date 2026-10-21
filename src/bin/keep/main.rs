use std::io::Write;

use anyhow::Result;
use git2::{FileMode, Repository};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

use keep::{
    Database, Entry, FilesystemSubstance, ShallowDifferenceSide, Snapshot, decode_entry_filemode,
};

mod args;
mod commands;

use args::{Args, Command};

pub fn main() -> Result<()> {
    let args = Args::get()?;
    args.apply_verbosity();
    args.run()
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
        const HACK_VERBOSITY: u8 = 2;
        let level_filter = match HACK_VERBOSITY + self.verbosity {
            0 => log::LevelFilter::Error,
            1 => log::LevelFilter::Warn,
            2 => log::LevelFilter::Info,
            3 => log::LevelFilter::Debug,
            _ => log::LevelFilter::Trace,
        };
        env_logger::builder()
            .filter(None, level_filter)
            .format_target(false)
            .init();
    }

    fn run(&self) -> Result<()> {
        match &self.command {
            Command::Snapshot {
                subject,
                relative_path,
                force,
            } => {
                let db = self.database()?;
                let substance = self.substance()?;
                log::info!("taking snapshot of {}", subject.display());
                let snapshot = Snapshot::take(subject)?;
                let entry = db.plant_snapshot(&snapshot)?;
                log::info!("snapshot: {}", entry);
                log::info!("storing snapshot");
                db.store_snapshot(&substance, &entry, subject)?;
                let parent = db.repository().head()?.peel_to_commit()?;
                let big_tree = parent.tree_id();
                log::info!(
                    "adding snapshot to HEAD^{{tree}} ({}) at {}",
                    big_tree,
                    relative_path
                );
                let new_big_tree = db.append(big_tree, relative_path, &entry, *force)?;
                let commit =
                    db.commit_simple("x", &db.repository().find_tree(new_big_tree)?, &parent)?;
                log::info!("new commit is {}. merging --ff-only into HEAD", commit);
                db.safe_merge(commit)?;
            }
            Command::TakeSnapshot { subject } => {
                let db = self.database()?;
                let snapshot = Snapshot::take(subject)?;
                let entry = db.plant_snapshot(&snapshot)?;
                println!("{entry}")
            }
            Command::StoreSnapshot {
                mode,
                object,
                subject,
            } => {
                let db = self.database()?;
                let substance = self.substance()?;
                let entry = parse_entry(&db, mode, object)?;
                db.store_snapshot(&substance, &entry, subject)?;
            }
            Command::Append {
                big_tree,
                relative_path,
                mode,
                object,
                force,
            } => {
                let db = self.database()?;
                let big_tree = db.resolve_treeish(big_tree)?;
                let entry = parse_entry(&db, mode, object)?;
                let new_tree = db.append(big_tree, relative_path, &entry, *force)?;
                println!("{}", new_tree)
            }
            Command::Remove {
                big_tree,
                relative_path,
            } => {
                let db = self.database()?;
                let big_tree = db.resolve_treeish(big_tree)?;
                let new_tree = db.remove(big_tree, relative_path)?;
                println!("{}", new_tree)
            }
            Command::Mount {
                mountpoint,
                tree,
                uid,
                gid,
            } => {
                let db = self.database()?;
                let substance = self.substance()?;
                let tree = db.resolve_treeish(tree)?;
                db.mount(tree, mountpoint, substance, *uid, *gid)?;
            }
            Command::Check { mode, object } => {
                let db = self.database()?;
                let entry = parse_entry(&db, mode, object)?;
                db.check(&entry)?;
            }
            Command::CheckContent { mode, object, deep } => {
                let mut db = self.database()?;
                let mut substance = self.substance()?;
                let entry = parse_entry(&db, mode, object)?;
                commands::check_content::run(&mut db, &mut substance, &entry, *deep)?;
            }
            Command::UniqueShadows { mode, object } => {
                let db = self.database()?;
                let entry = parse_entry(&db, mode, object)?;
                db.unique_shadows(&entry, |path, shadow| {
                    println!("{} {}", shadow.content_hash(), path);
                    Ok(())
                })?;
            }
            Command::Diff { tree_a, tree_b } => {
                let (tree_a, tree_b) = match (tree_a.as_deref(), tree_b.as_deref()) {
                    (None, None) => ("HEAD^", "HEAD"),
                    (Some(tree_a), None) => ("HEAD", tree_a),
                    (Some(tree_a), Some(tree_b)) => (tree_a, tree_b),
                    _ => panic!(),
                };
                let db = self.database()?;
                let tree_a = db.resolve_treeish(tree_a)?;
                let tree_b = db.resolve_treeish(tree_b)?;
                let mut stdout = StandardStream::stdout(ColorChoice::Always);
                db.shallow_diff(tree_a, tree_b, |difference| {
                    let color = match difference.side() {
                        ShallowDifferenceSide::A => Color::Red,
                        ShallowDifferenceSide::B => Color::Green,
                    };
                    stdout.set_color(ColorSpec::new().set_fg(Some(color)))?;
                    writeln!(&mut stdout, "{}", difference)?;
                    Ok(())
                })?;
                stdout.reset()?;
            }
        }
        Ok(())
    }
}

fn parse_entry(db: &Database, mode: &str, oid: &str) -> Result<Entry> {
    let mode = decode_entry_filemode(i32::from_str_radix(mode, 8).unwrap()).unwrap();
    let oid = match mode {
        FileMode::Tree => db.resolve_treeish(oid)?,
        _ => db.resolve_blobish(oid)?,
    };
    Ok(Entry { mode, oid })
}
