use std::collections::BTreeSet;
use std::str;

use anyhow::{bail, ensure, Result};
use git2::{FileMode, ObjectType, Oid, Repository};

use crate::{BlobShadow, BulkPath, BulkTreeEntryName, Database};

impl Database {
    pub fn traverser<'a, T: TraversalCallbacks>(
        &'a self,
        callbacks: &'a mut T,
    ) -> Traverser<'a, T> {
        Traverser {
            repository: &self.repository(),
            callbacks,
            empty_blob_oid: None,
        }
    }

    pub fn check(&self, tree: Oid) -> Result<()> {
        struct CheckCallbacks;
        impl TraversalCallbacks for CheckCallbacks {
            fn on_blob(&mut self, blob: &Visit<VisitBlob>) -> Result<()> {
                let _ = blob.read_blob()?;
                Ok(())
            }
            fn on_link(&mut self, link: &Visit<VisitLink>) -> Result<()> {
                let _ = link.read_link()?;
                Ok(())
            }
        }
        let mut callbacks = OnUnique::new(CheckCallbacks);
        self.traverser(&mut callbacks).traverse(tree)
    }

    pub fn unique_blobs(
        &self,
        tree: Oid,
        callback: impl FnMut(&BulkPath, &BlobShadow) -> Result<()>,
    ) -> Result<()> {
        struct UniqueBlobsCallbacks<T> {
            callback: T,
        }
        impl<T: FnMut(&BulkPath, &BlobShadow) -> Result<()>> TraversalCallbacks
            for UniqueBlobsCallbacks<T>
        {
            fn on_blob(&mut self, blob: &Visit<VisitBlob>) -> Result<()> {
                let st_blob = blob.read_blob()?;
                (self.callback)(blob.path, &st_blob)?;
                Ok(())
            }
        }
        let mut callbacks = OnUnique::new(UniqueBlobsCallbacks { callback });
        self.traverser(&mut callbacks).traverse(tree)
    }
}

pub trait TraversalCallbacks {
    fn on_blob(&mut self, _blob: &Visit<VisitBlob>) -> Result<()> {
        Ok(())
    }

    fn on_link(&mut self, _link: &Visit<VisitLink>) -> Result<()> {
        Ok(())
    }

    fn on_tree(&mut self, _tree: &Visit<VisitTree>) -> Result<VisitTreeDecision> {
        Ok(VisitTreeDecision::Descend)
    }
}

pub struct OnUnique<T> {
    seen: BTreeSet<Oid>,
    callbacks: T,
}

impl<T> OnUnique<T> {
    pub fn new(callbacks: T) -> Self {
        Self {
            seen: BTreeSet::new(),
            callbacks,
        }
    }
}

impl<T: TraversalCallbacks> TraversalCallbacks for OnUnique<T> {
    fn on_blob(&mut self, blob: &Visit<VisitBlob>) -> Result<()> {
        if self.seen.insert(blob.oid()) {
            self.callbacks.on_blob(blob)
        } else {
            Ok(())
        }
    }

    fn on_link(&mut self, link: &Visit<VisitLink>) -> Result<()> {
        if self.seen.insert(link.oid()) {
            self.callbacks.on_link(link)
        } else {
            Ok(())
        }
    }

    fn on_tree(&mut self, tree: &Visit<VisitTree>) -> Result<VisitTreeDecision> {
        if self.seen.insert(tree.oid()) {
            self.callbacks.on_tree(tree)
        } else {
            Ok(VisitTreeDecision::Skip)
        }
    }
}

pub struct Visit<'a, T> {
    repository: &'a Repository,
    path: &'a BulkPath,
    oid: Oid,
    extra: T,
}

pub struct VisitBlob {
    executable: bool,
}

pub struct VisitLink;
pub struct VisitTree;

pub enum VisitTreeDecision {
    Descend,
    Skip,
}

impl<'a, T> Visit<'a, T> {
    pub fn oid(&self) -> Oid {
        self.oid
    }

    pub fn path(&self) -> &BulkPath {
        self.path
    }
}

impl<'a> Visit<'a, VisitBlob> {
    pub fn executable(&self) -> bool {
        self.extra.executable
    }

    pub fn read_blob(&self) -> Result<BlobShadow> {
        let blob = self.repository.find_blob(self.oid)?;
        Ok(BlobShadow::from_bytes(blob.content())?)
    }
}

impl<'a> Visit<'a, VisitLink> {
    pub fn read_link(&self) -> Result<String> {
        let blob = self.repository.find_blob(self.oid)?;
        Ok(str::from_utf8(blob.content())?.to_owned())
    }
}

pub struct Traverser<'a, T> {
    repository: &'a Repository,
    callbacks: &'a mut T,
    empty_blob_oid: Option<Oid>,
}

impl<'a, T: TraversalCallbacks> Traverser<'a, T> {
    fn ensure_blob_is_empty(&mut self, oid: Oid) -> Result<()> {
        if let Some(expected_oid) = self.empty_blob_oid {
            ensure!(oid == expected_oid);
        } else {
            let blob = self.repository.find_blob(oid)?;
            ensure!(blob.size() == 0);
            self.empty_blob_oid = Some(oid);
        }
        Ok(())
    }

    pub fn traverse(&mut self, tree: Oid) -> Result<()> {
        self.traverse_from(&mut BulkPath::new(), tree)
    }

    pub fn traverse_from(&mut self, path: &mut BulkPath, tree: Oid) -> Result<()> {
        if let VisitTreeDecision::Skip = self.callbacks.on_tree(&Visit {
            repository: self.repository,
            path: &path,
            oid: tree,
            extra: VisitTree,
        })? {
            return Ok(());
        }

        let tree = self.repository.find_tree(tree)?;

        let mut first = true;
        for entry in tree.iter() {
            let name = BulkTreeEntryName::decode(entry.name().unwrap())?;
            let mode = entry.filemode();
            let kind = entry.kind().unwrap();
            let oid = entry.id();

            if first {
                ensure!(name.is_marker());
                ensure!(mode == FileMode::Blob.into());
                ensure!(kind == ObjectType::Blob);
                self.ensure_blob_is_empty(oid)?;
                first = false;
                continue;
            }

            let name = name.child().unwrap();
            path.push(name.clone());
            match kind {
                ObjectType::Blob => {
                    if mode == FileMode::Link.into() {
                        self.callbacks.on_link(&Visit {
                            repository: self.repository,
                            path: &path,
                            oid,
                            extra: VisitLink,
                        })?;
                    } else {
                        let executable = if mode == FileMode::Blob.into() {
                            true
                        } else if mode == FileMode::BlobExecutable.into() {
                            false
                        } else {
                            bail!("")
                        };
                        self.callbacks.on_blob(&Visit {
                            repository: self.repository,
                            path: &path,
                            oid,
                            extra: VisitBlob { executable },
                        })?;
                    }
                }
                ObjectType::Tree => {
                    ensure!(mode == FileMode::Tree.into());
                    self.traverse_from(path, oid)?;
                }
                _ => {
                    bail!("");
                }
            }
            path.pop();
        }
        Ok(())
    }
}
