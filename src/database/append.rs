use anyhow::{Result, bail};
use git2::{FileMode, Oid};

use crate::{Database, ShadowPath, ShadowPathComponent, ShadowTreeEntryName};

impl Database {
    pub fn append(
        &self,
        big_tree: Oid,
        path: &ShadowPath, // precondition: non-empty
        mode: FileMode,
        object: Oid,
        can_replace: bool,
    ) -> Result<Oid> {
        self.append_inner(
            self.empty_blob_oid()?,
            big_tree,
            path.components(),
            mode,
            object,
            can_replace,
        )
    }

    fn append_inner(
        &self,
        empty_blob_oid: Oid,
        big_tree: Oid,
        path: &[ShadowPathComponent],
        mode: FileMode,
        object: Oid,
        can_replace: bool,
    ) -> Result<Oid> {
        let orig = self.repository().find_tree(big_tree)?;
        let mut builder = self.repository().treebuilder(Some(&orig))?;
        let (head, tail) = path.split_first().unwrap();
        let (head_mode, head_oid) = if tail.is_empty() {
            if !can_replace && builder.get(head.encode())?.is_some() {
                bail!("would replace"); // TODO structured error
            }
            (mode, object)
        } else {
            let head_oid = match builder.get(head.encode())? {
                None => self.append_inner_create(empty_blob_oid, tail, mode, object)?,
                Some(entry) => {
                    assert_eq!(entry.filemode(), FileMode::Tree.into());
                    self.append_inner(empty_blob_oid, entry.id(), tail, mode, object, can_replace)?
                }
            };
            (FileMode::Tree, head_oid)
        };
        if builder.get(head.encode())?.is_some() {
            builder.remove(head.encode()).unwrap();
        }
        builder.insert(head.encode(), head_oid, head_mode.into())?;
        Ok(builder.write()?)
    }

    fn append_inner_create(
        &self,
        empty_blob_oid: Oid,
        path: &[ShadowPathComponent],
        mode: FileMode,
        object: Oid,
    ) -> Result<Oid> {
        let mut builder = self.repository().treebuilder(None)?;
        builder.insert(
            ShadowTreeEntryName::Marker.encode(),
            empty_blob_oid,
            FileMode::Blob.into(),
        )?;
        let (head, tail) = path.split_first().unwrap();
        let (head_mode, head_oid) = if tail.is_empty() {
            (mode, object)
        } else {
            let head_oid = self.append_inner_create(empty_blob_oid, tail, mode, object)?;
            (FileMode::Tree, head_oid)
        };
        builder.insert(head.encode(), head_oid, head_mode.into())?;
        Ok(builder.write()?)
    }
}
