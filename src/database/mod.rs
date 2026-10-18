use anyhow::{Error, Result};
use git2::{Commit, Oid, Repository, Signature, Tree};

use crate::{ShallowDifference, shallow_diff};

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
        shallow_diff(&self.repository, tree_a, tree_b, callback)}

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

    pub fn safe_merge(&self, progress: Oid) -> Result<()> {
        let repo = self.repository();
        let mut head = repo.head().unwrap();
        let old = head.target().unwrap();
        assert!(
            progress == old || repo.graph_descendant_of(progress, old)?,
            "{} is not a fast-forward of {}",
            progress,
            old
        );
        head.set_target(progress, "keep: fast-forward")?; // fails if HEAD moved since it was read
        Ok(())
    }
}
