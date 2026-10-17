use std::cmp::Ordering;
use std::fmt;
use std::marker::PhantomData;
use std::str::{self, Utf8Error};

use git2::{Error, FileMode, Oid, Repository, TreeEntry, TreeIter};
use itertools::Itertools;

pub struct ShallowDifference<'a> {
    pub parent: &'a [Vec<u8>],
    pub side: &'a ShallowDifferenceSide,
    pub mode: i32,
    pub oid: Oid,
    pub name: &'a [u8],
}

impl<'a> ShallowDifference<'a> {
    fn new(
        parent: &'a [Vec<u8>],
        side: &'a ShallowDifferenceSide,
        entry: &'a TreeEntry<'a>,
    ) -> Self {
        Self {
            parent,
            side,
            mode: entry.filemode(),
            oid: entry.id(),
            name: entry.name_bytes(),
        }
    }

    pub fn render_path(&self) -> Result<String, Utf8Error> {
        self.parent
            .iter()
            .map(AsRef::as_ref)
            .chain([self.name])
            .map(str::from_utf8)
            .process_results(|mut components| components.join("/"))
    }
}

pub enum ShallowDifferenceSide {
    A,
    B,
}

impl fmt::Display for ShallowDifferenceSide {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ShallowDifferenceSide::A => write!(fmt, "-"),
            ShallowDifferenceSide::B => write!(fmt, "+"),
        }
    }
}

impl<'a> fmt::Display for ShallowDifference<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let path = self.render_path().map_err(|_| fmt::Error)?;
        write!(fmt, "{} {:06o} {} {}", self.side, self.mode, self.oid, path)
    }
}

pub fn shallow_diff<'a, E: From<Error> + 'static>(
    repository: &'a Repository,
    tree_a: Oid,
    tree_b: Oid,
    callback: impl for<'b> FnMut(&ShallowDifference<'b>) -> Result<(), E>,
) -> Result<(), E> {
    let mut differ = Differ {
        repository,
        callback,
        path: Vec::new(),
        phantom: PhantomData,
    };
    differ.diff_inner(tree_a, tree_b)
}

struct Differ<'a, T, E> {
    repository: &'a Repository,
    callback: T,
    path: Vec<Vec<u8>>,
    phantom: PhantomData<E>,
}

impl<'a, T, E> Differ<'a, T, E>
where
    T: for<'b> FnMut(&ShallowDifference<'b>) -> Result<(), E>,
    E: From<Error> + 'static,
{
    fn diff_inner(&mut self, tree_a: Oid, tree_b: Oid) -> Result<(), E> {
        let tree_a = self.repository.find_tree(tree_a)?;
        let tree_b = self.repository.find_tree(tree_b)?;
        let mut it_a = tree_a.iter();
        let mut it_b = tree_b.iter();
        let mut opt_entry_a = it_a.next();
        let mut opt_entry_b = it_b.next();
        loop {
            match (
                opt_entry_a.as_ref().map(TreeEntry::to_owned),
                opt_entry_b.as_ref().map(TreeEntry::to_owned),
            ) {
                (None, None) => {
                    break;
                }
                (Some(entry_a), None) => {
                    self.exhaust(&ShallowDifferenceSide::A, &entry_a, &mut it_a)?;
                    opt_entry_a = None;
                }
                (None, Some(entry_b)) => {
                    self.exhaust(&ShallowDifferenceSide::B, &entry_b, &mut it_b)?;
                    opt_entry_b = None;
                }
                (Some(entry_a), Some(entry_b)) => {
                    match entry_a.name_bytes().cmp(&entry_b.name_bytes()) {
                        Ordering::Less => {
                            opt_entry_a = self.report_until(
                                &entry_b,
                                &ShallowDifferenceSide::A,
                                &entry_a,
                                &mut it_a,
                            )?;
                        }
                        Ordering::Greater => {
                            opt_entry_b = self.report_until(
                                &entry_a,
                                &ShallowDifferenceSide::B,
                                &entry_b,
                                &mut it_b,
                            )?;
                        }
                        Ordering::Equal => {
                            let news = if entry_a.filemode() != entry_b.filemode() {
                                true
                            } else if entry_a.id() != entry_b.id() {
                                if entry_a.filemode() == i32::from(FileMode::Tree) {
                                    self.path.push(entry_a.name_bytes().to_vec());
                                    self.diff_inner(entry_a.id(), entry_b.id())?;
                                    self.path.pop();
                                    false
                                } else {
                                    true
                                }
                            } else {
                                false
                            };
                            if news {
                                self.report(&ShallowDifferenceSide::A, &entry_a)?;
                                self.report(&ShallowDifferenceSide::B, &entry_b)?;
                            }
                            opt_entry_a = it_a.next();
                            opt_entry_b = it_b.next();
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn exhaust<'b>(
        &mut self,
        side: &'b ShallowDifferenceSide,
        current: &'b TreeEntry,
        it: &'b mut TreeIter,
    ) -> Result<(), E>
    where
        'a: 'b,
    {
        self.report(side, current)?;
        for entry in it {
            self.report(side, &entry)?;
        }
        Ok(())
    }

    fn report_until<'b>(
        &mut self,
        target_entry: &TreeEntry,
        side: &'b ShallowDifferenceSide,
        current: &'b TreeEntry,
        it: &'b mut TreeIter,
    ) -> Result<Option<TreeEntry<'static>>, E>
    where
        'a: 'b,
    {
        self.report(side, current)?;
        for entry in it {
            if &entry < target_entry {
                self.report(side, &entry)?;
            } else {
                return Ok(Some(entry.to_owned()));
            }
        }
        Ok(None)
    }

    fn report<'b>(&mut self, side: &'b ShallowDifferenceSide, entry: &'b TreeEntry) -> Result<(), E>
    where
        'a: 'b,
    {
        (self.callback)(&ShallowDifference::new(&self.path, side, entry))
    }
}
