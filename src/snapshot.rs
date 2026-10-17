use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str;

use anyhow::{anyhow, Context, Error, Result};
use fallible_iterator::FallibleIterator;
use lazy_static::lazy_static;
use regex::Regex;

use crate::{exit_ok, Shadow, ShadowPath};

const TAKE_SNAPSHOT_SCRIPT: &'static [u8] = include_bytes!("../scripts/take-snapshot.bash");

pub struct Snapshot<'a> {
    path: &'a Path,
}

impl<'a> Snapshot<'a> {
    const FILES: &'static [&'static str] =
        &["subject.txt", "sha256sum.txt", "nodes", "files", "digests"];

    pub fn new(path: &'a Path) -> Snapshot<'a> {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn nodes_path(&self) -> PathBuf {
        self.path().join("nodes")
    }

    fn digests_path(&self) -> PathBuf {
        self.path().join("digests")
    }

    pub fn entries(&self) -> Result<SnapshotEntries<impl io::BufRead>> {
        Ok(SnapshotEntries {
            nodes_entries: NodesEntries {
                reader: io::BufReader::new(fs::File::open(self.nodes_path())?),
            },
            digests_entries: DigestsEntries {
                reader: io::BufReader::new(fs::File::open(self.digests_path())?),
            },
        })
    }

    pub fn take(&self, subject: &Path) -> Result<()> {
        exit_ok(
            Command::new("bash")
                .arg("-c")
                .arg(OsStr::from_bytes(TAKE_SNAPSHOT_SCRIPT))
                .arg("--")
                .arg(subject)
                .arg(&self.path)
                .status()?,
        )?;
        Ok(())
    }

    pub fn remove(&self) -> Result<()> {
        for file in Self::FILES {
            fs::remove_file(&self.path().join(file))?;
        }
        fs::remove_dir(self.path())?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct SnapshotEntry {
    pub path: ShadowPath,
    pub value: SnapshotEntryValue,
}

#[derive(Clone, Debug)]
pub enum SnapshotEntryValue {
    File { shadow: Shadow, executable: bool },
    Link { target: String },
    Tree,
}

pub struct SnapshotEntries<T> {
    nodes_entries: NodesEntries<T>,
    digests_entries: DigestsEntries<T>,
}

impl<T: io::BufRead> FallibleIterator for SnapshotEntries<T> {
    type Item = SnapshotEntry;
    type Error = Error;

    fn next(&mut self) -> Result<Option<Self::Item>, Self::Error> {
        while let Some(node_line) = self.nodes_entries.next()? {
            let path = node_line.path.parse().context(format!("{:?}", node_line))?;
            let value = match node_line.ty {
                'd' => SnapshotEntryValue::Tree,
                'l' => SnapshotEntryValue::Link {
                    target: node_line.target,
                },
                'f' => {
                    let digest_line = self.digests_entries.next()?.unwrap();
                    assert_eq!(node_line.path, digest_line.path);
                    SnapshotEntryValue::File {
                        shadow: Shadow::new(digest_line.digest.parse()?, node_line.size),
                        executable: node_line.is_executable(),
                    }
                }
                _ => {
                    log::warn!("skipping {:?}", node_line);
                    continue;
                }
            };
            return Ok(Some(SnapshotEntry { path, value }));
        }
        Ok(None)
    }
}

#[derive(Debug)]
struct NodesEntry {
    ty: char, // [dflcbsp]
    mode: u16,
    size: Option<u64>,
    path: String,
    target: String,
}

impl NodesEntry {
    fn is_executable(&self) -> bool {
        self.mode & 0o100 != 0
    }
}

struct NodesEntries<T> {
    reader: T,
}

impl<T: io::BufRead> FallibleIterator for NodesEntries<T> {
    type Item = NodesEntry;
    type Error = Error;

    fn next(&mut self) -> Result<Option<Self::Item>, Self::Error> {
        lazy_static! {
            static ref RE: Regex = Regex::new(
                r"^(?P<type>[dflcbsp]) 0(?P<mode>[0-9]{3}[0-9]*) (?P<size>([0-9]+|\?)) (?P<path>.*)\x00 (?P<target>.*)\x00\n$"
            )
            .unwrap();
        }
        let mut buf = vec![];
        if self.reader.fill_buf()?.is_empty() {
            return Ok(None);
        }
        // TODO handle malformed input
        assert_ne!(self.reader.read_until(0, &mut buf)?, 0);
        assert_ne!(self.reader.read_until(0, &mut buf)?, 0);
        assert_eq!(self.reader.read_until(b'\n', &mut buf)?, 1);
        let caps = RE
            .captures(str::from_utf8(&buf)?)
            .ok_or(anyhow!("regex does not match"))?;
        let size = match &caps["size"] {
            "?" => None,
            s => Some(s.parse()?),
        };
        Ok(Some(NodesEntry {
            ty: caps["type"].chars().nth(0).unwrap(),
            mode: u16::from_str_radix(&caps["mode"], 8)?,
            size,
            path: caps["path"].to_string(),
            target: caps["target"].to_string(),
        }))
    }
}

#[derive(Debug)]
struct DigestsEntry {
    digest: String,
    path: String,
}

struct DigestsEntries<T> {
    reader: T,
}

impl<T: io::BufRead> FallibleIterator for DigestsEntries<T> {
    type Item = DigestsEntry;
    type Error = Error;

    fn next(&mut self) -> Result<Option<Self::Item>, Self::Error> {
        lazy_static! {
            static ref RE: Regex =
                Regex::new(r"^(?P<digest>[a-z0-9]{64}|[?]{64}) \*(?P<path>.*)\x00\n$").unwrap();
        }
        let mut buf = vec![];
        if self.reader.fill_buf()?.is_empty() {
            return Ok(None);
        }
        // TODO handle malformed input
        assert_ne!(self.reader.read_until(0, &mut buf)?, 0);
        assert_eq!(self.reader.read_until(b'\n', &mut buf)?, 1);
        let caps = RE
            .captures(str::from_utf8(&buf)?)
            .ok_or(anyhow!("regex does not match"))?;
        Ok(Some(DigestsEntry {
            digest: caps["digest"].to_string(),
            path: caps["path"].to_string(),
        }))
    }
}
