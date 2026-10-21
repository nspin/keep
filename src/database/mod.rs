use std::fmt;

use anyhow::{Error, Result};
use git2::{Commit, FileMode, Oid, Repository, Signature, Tree, TreeBuilder, TreeEntry};

use crate::{ShadowPathComponent, ShallowDifference, shallow_diff};

mod append;
mod fs;
mod remove;
mod snapshot;
mod traverse;

pub use traverse::{
    TraversalCallbacks, Traverser, Visit, VisitLink, VisitShadow, VisitTree, VisitTreeDecision,
};

pub struct Database {
    repository: Repository,
}

impl Database {
    pub fn new(repository: Repository) -> Self {
        Self { repository }
    }

    pub fn repository(&self) -> &Repository {
        &self.repository
    }

    pub fn resolve_blobish(&self, blobish: &str) -> Result<Oid> {
        // TODO validate blobish?
        Ok(self
            .repository()
            .revparse_single(blobish)?
            .peel_to_blob()?
            .id())
    }

    pub fn resolve_treeish(&self, treeish: &str) -> Result<Oid> {
        // TODO validate treeish?
        Ok(self
            .repository()
            .revparse_single(treeish)?
            .peel_to_tree()?
            .id())
    }

    pub fn empty_blob_oid(&self) -> Result<Oid> {
        Ok(self.repository().blob(&[])?)
    }

    pub fn shallow_diff(
        &self,
        tree_a: Oid,
        tree_b: Oid,
        callback: impl for<'b> FnMut(&ShallowDifference<'b>) -> Result<(), Error>,
    ) -> Result<()> {
        shallow_diff(&self.repository, tree_a, tree_b, callback)
    }

    pub fn commit_simple(
        &self,
        message: &str,
        tree: &Tree<'_>,
        parent: &Commit<'_>,
    ) -> Result<Oid> {
        let dummy_sig = Signature::now("x", "x@x")?;
        Ok(self
            .repository()
            .commit(None, &dummy_sig, &dummy_sig, message, tree, &[parent])?)
    }

    pub fn safe_merge(&self, new: Oid) -> Result<()> {
        let repo = self.repository();
        let mut head = repo.head().unwrap();
        let old = head.target().unwrap();
        assert!(
            new == old || repo.graph_descendant_of(new, old)?,
            "{new} is not a fast-forward of {old}",
        );
        head.set_target(new, "keep: fast-forward")?; // fails if HEAD moved since it was read
        Ok(())
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Entry {
    pub mode: FileMode,
    pub oid: Oid,
}

impl Entry {
    fn insert_into_builder<'a>(
        &self,
        builder: &'a mut TreeBuilder<'_>,
        name: &ShadowPathComponent,
    ) -> Result<TreeEntry<'a>, git2::Error> {
        builder.insert(name.encode(), self.oid, self.mode.into())
    }
}

impl fmt::Display for Entry {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{:06o},{}", u32::from(self.mode), self.oid)
    }
}

pub fn decode_entry_filemode(raw: i32) -> Option<FileMode> {
    Some(if raw == FileMode::Tree.into() {
        FileMode::Tree
    } else if raw == FileMode::Link.into() {
        FileMode::Link
    } else if raw == FileMode::Blob.into() {
        FileMode::Blob
    } else if raw == FileMode::BlobExecutable.into() {
        FileMode::BlobExecutable
    } else {
        return None;
    })
}
