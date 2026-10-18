use anyhow::{Result, anyhow};
use git2::Oid;

use crate::{Database, ShadowPath, ShadowPathComponent};

impl Database {
    pub fn remove(
        &self,
        big_tree: Oid,
        path: &ShadowPath, // precondition: non-empty
    ) -> Result<Oid> {
        self.remove_inner(self.empty_blob_oid()?, big_tree, path.components())
    }

    fn remove_inner(
        &self,
        empty_blob_oid: Oid,
        big_tree: Oid,
        path: &[ShadowPathComponent],
    ) -> Result<Oid> {
        let orig = self.repository().find_tree(big_tree)?;
        let mut builder = self.repository().treebuilder(Some(&orig))?;
        let (head, tail) = path.split_first().unwrap();
        let old_entry = builder
            .get(&head.encode())?
            .ok_or_else(|| anyhow!("path does not exist in tree"))?
            .to_owned();
        builder.remove(&head.encode()).unwrap();
        if !tail.is_empty() {
            let new_oid = self.remove_inner(empty_blob_oid, old_entry.id(), tail)?;
            builder.insert(head.encode(), new_oid, old_entry.filemode())?;
        }
        Ok(builder.write()?)
    }
}
