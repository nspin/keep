use std::path::Path;

use anyhow::Result;
use git2::{FileMode, Oid};

use crate::{Database, Entry, ShadowTreeEntryName, Snapshot, Substance, with_report_progress};

impl Database {
    pub fn plant_snapshot(&self, value: &Snapshot) -> Result<Entry> {
        self.plant_snapshot_inner(value, self.empty_blob_oid()?)
    }

    fn plant_snapshot_inner(&self, value: &Snapshot, empty_blob_oid: Oid) -> Result<Entry> {
        Ok(match value {
            Snapshot::File { shadow, executable } => Entry {
                mode: if *executable {
                    FileMode::BlobExecutable
                } else {
                    FileMode::Blob
                },
                oid: self.repository().blob(&shadow.to_bytes())?,
            },
            Snapshot::Link { target } => Entry {
                mode: FileMode::Link,
                oid: self.repository().blob(target.as_bytes())?,
            },
            Snapshot::Tree { entries } => {
                let mut builder = self.repository().treebuilder(None)?;
                builder.insert(
                    ShadowTreeEntryName::Marker.encode(),
                    empty_blob_oid,
                    FileMode::Blob.into(),
                )?;
                for (name, value) in entries.iter() {
                    self.plant_snapshot_inner(value, empty_blob_oid)?
                        .insert_into_builder(&mut builder, name)?;
                }
                Entry {
                    mode: FileMode::Tree,
                    oid: builder.write()?,
                }
            }
        })
    }

    pub fn store_snapshot(
        &self,
        substance: &impl Substance,
        entry: &Entry,
        subject: &Path,
    ) -> Result<()> {
        with_report_progress(
            1000,
            |i| log::info!("stored {i} files"),
            |p| {
                self.unique_shadows(entry, |path, shadow| {
                    substance.store_content(shadow, &path.append_to(subject))?;
                    (p)();
                    Ok(())
                })
            },
        )?;
        Ok(())
    }
}
