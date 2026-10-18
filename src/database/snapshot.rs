use std::path::Path;

use anyhow::Result;
use git2::{FileMode, Oid};

use crate::{Database, ShadowTreeEntryName, Snapshot, Substance};

impl Database {
    pub fn plant_snapshot(&self, value: &Snapshot) -> Result<(FileMode, Oid)> {
        self.plant_snapshot_inner(value, self.empty_blob_oid()?)
    }

    fn plant_snapshot_inner(
        &self,
        value: &Snapshot,
        empty_blob_oid: Oid,
    ) -> Result<(FileMode, Oid)> {
        Ok(match value {
            Snapshot::File { shadow, executable } => {
                let mode = if *executable {
                    FileMode::BlobExecutable
                } else {
                    FileMode::Blob
                };
                let oid = self.repository().blob(&shadow.to_bytes())?;
                (mode, oid)
            }
            Snapshot::Link { target } => {
                let mode = FileMode::Link;
                let oid = self.repository().blob(target.as_bytes())?;
                (mode, oid)
            }
            Snapshot::Tree { entries } => {
                let mode = FileMode::Tree;
                let mut builder = self.repository().treebuilder(None)?;
                builder.insert(
                    ShadowTreeEntryName::Marker.encode(),
                    empty_blob_oid,
                    FileMode::Blob.into(),
                )?;
                for (name, value) in entries.iter() {
                    let (child_mode, child_oid) =
                        self.plant_snapshot_inner(value, empty_blob_oid)?;
                    builder.insert(name.encode(), child_oid, child_mode.into())?;
                }
                let oid = builder.write()?;
                (mode, oid)
            }
        })
    }

    pub fn store_snapshot(
        &self,
        substance: &impl Substance,
        tree: Oid,
        subject: &Path,
    ) -> Result<()> {
        self.unique_shadows(tree, |path, shadow| {
            let src = subject.join(path.to_string());
            eprint!(".");
            substance.store(shadow.content_hash(), &src)?;
            Ok(())
        })?;
        Ok(())
    }
}
