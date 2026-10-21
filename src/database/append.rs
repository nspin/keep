use anyhow::{Result, bail};
use git2::{FileMode, Oid};

use crate::{Database, Entry, ShadowPath, ShadowPathComponent, ShadowTreeEntryName};

impl Database {
    pub fn append(
        &self,
        big_tree: Oid,
        path: &ShadowPath, // precondition: non-empty
        entry: &Entry,
        can_replace: bool,
    ) -> Result<Oid> {
        self.append_inner(
            self.empty_blob_oid()?,
            big_tree,
            path.components(),
            *entry,
            can_replace,
        )
    }

    fn append_inner(
        &self,
        empty_blob_oid: Oid,
        big_tree: Oid,
        path: &[ShadowPathComponent],
        target_entry: Entry,
        can_replace: bool,
    ) -> Result<Oid> {
        let mut builder = self
            .repository()
            .treebuilder(Some(&self.repository().find_tree(big_tree)?))?;
        let (head, tail) = path.split_first().unwrap();
        let local_entry = if tail.is_empty() {
            if !can_replace && builder.get(head.encode())?.is_some() {
                bail!("would replace"); // TODO structured error
            }
            target_entry
        } else {
            Entry {
                mode: FileMode::Tree,
                oid: match builder.get(head.encode())? {
                    None => self.append_inner_create(empty_blob_oid, tail, target_entry)?,
                    Some(entry) => {
                        assert_eq!(entry.filemode(), FileMode::Tree.into());
                        self.append_inner(
                            empty_blob_oid,
                            entry.id(),
                            tail,
                            target_entry,
                            can_replace,
                        )?
                    }
                },
            }
        };
        if builder.get(head.encode())?.is_some() {
            builder.remove(head.encode()).unwrap();
        }
        local_entry.insert_into_builder(&mut builder, head)?;
        Ok(builder.write()?)
    }

    fn append_inner_create(
        &self,
        empty_blob_oid: Oid,
        path: &[ShadowPathComponent],
        target_entry: Entry,
    ) -> Result<Oid> {
        let mut builder = self.repository().treebuilder(None)?;
        builder.insert(
            ShadowTreeEntryName::Marker.encode(),
            empty_blob_oid,
            FileMode::Blob.into(),
        )?;
        let (head, tail) = path.split_first().unwrap();
        let local_entry = if tail.is_empty() {
            target_entry
        } else {
            Entry {
                mode: FileMode::Tree,
                oid: self.append_inner_create(empty_blob_oid, tail, target_entry)?,
            }
        };
        local_entry.insert_into_builder(&mut builder, head)?;
        Ok(builder.write()?)
    }
}
