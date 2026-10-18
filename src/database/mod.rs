use std::process::Command;

use anyhow::{Error, Result};
use git2::{Commit, Oid, Repository, Signature, Tree};

use crate::{exit_ok, shallow_diff, ShallowDifference};

mod append;
mod remove;
mod traverse;
mod snapshot;
mod index;
mod fs;

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

    pub fn invoke_git(&self, args: &[impl AsRef<str>]) -> Result<()> {
        let mut cmd = Command::new("git");
        cmd.env_clear();
        cmd.env("GIT_DIR", self.repository().path());
        for arg in args {
            cmd.arg(arg.as_ref());
        }
        eprintln!("{:?}", cmd); // TODO remove
        exit_ok(cmd.status()?)?;
        Ok(())
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
        shallow_diff(&self.repository, tree_a, tree_b, callback).map_err(Error::from)
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

    pub fn safe_merge(&self, progress: Oid) -> Result<()> {
        self.invoke_git(&[
            "merge".to_owned(),
            "--quiet".to_owned(),
            "--ff-only".to_owned(),
            progress.to_string(),
        ])
    }
}
