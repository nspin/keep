use std::collections::BTreeMap;
use std::fs;
use std::iter::Peekable;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::str;

use anyhow::{anyhow, bail, ensure, Context, Result};
use lazy_static::lazy_static;
use regex::Regex;

use crate::ShadowPathComponent;
use crate::{exit_ok, sha256sum, Shadow, ShadowPath};

#[derive(Clone, Debug)]
pub enum Snapshot<T = Shadow> {
    File { shadow: T, executable: bool },
    Link { target: String },
    Tree { entries: BTreeMap<ShadowPathComponent, Snapshot<T>> },
}

impl<T> Snapshot<T> {
    fn traverse_with_path<U, E, F>(self, path: &mut ShadowPath, f: &mut F) -> Result<Snapshot<U>, E>
    where
        F: FnMut(&ShadowPath, T) -> Result<U, E>,
    {
        Ok(match self {
            Snapshot::File { shadow, executable } => Snapshot::File {
                shadow: f(path, shadow)?,
                executable,
            },
            Snapshot::Link { target } => Snapshot::Link { target },
            Snapshot::Tree { entries } => Snapshot::Tree {
                entries: entries
                    .into_iter()
                    .map(|(name, value)| {
                        path.push(name);
                        let value = value.traverse_with_path(path, f);
                        let name = path.pop().unwrap();
                        Ok((name, value?))
                    })
                    .collect::<Result<_, E>>()?,
            },
        })
    }
}

impl Snapshot {
    pub fn take(subject: &Path) -> Result<Snapshot> {
        let lines = run_find(subject)?;
        parse_find_output(lines)?.traverse_with_path(&mut ShadowPath::new(), &mut |path, pre| {
            refine_shadow(subject, path, pre)
        })
    }
}

#[derive(Debug)]
struct RawFindLine {
    ty: char, // [dflcbsp]
    pre_shadow: PreShadow,
    path: String,
    target: String,
}

#[derive(Debug, PartialEq, Eq)]
struct PreShadow {
    mode: u16,
    size: Option<u64>,
    ctime: i64,
    ctime_ns: i64,
    mtime: i64,
    mtime_ns: i64,
    inode: u64,
}

impl RawFindLine {
    fn is_executable(&self) -> bool {
        self.pre_shadow.mode & 0o100 != 0
    }
}

fn run_find(subject: &Path) -> Result<Vec<RawFindLine>> {
    assert!(!subject.as_os_str().as_bytes().starts_with(b"-"));
    let output = Command::new("find")
        .arg(&subject)
        .arg("-printf")
        .arg(r#"%y %#m %s %C@ %T@ %i %P\0 %l\0\n"#)
        .stderr(Stdio::inherit())
        .output()
        .context("failed to run find")?;
    exit_ok(output.status)?;
    parse_raw_find_lines(str::from_utf8(&output.stdout).context("find output is not UTF-8")?)
}

fn parse_raw_find_lines(mut rest: &str) -> Result<Vec<RawFindLine>> {
    lazy_static! {
        static ref RE: Regex = Regex::new(
            r"\A(?P<type>[dflcbsp]) 0(?P<mode>[0-7]+) (?P<size>[0-9]+|\?) (?P<ctime>[0-9]+)\.(?P<ctime_ns>[0-9]{9})0 (?P<mtime>[0-9]+)\.(?P<mtime_ns>[0-9]{9})0 (?P<inode>[0-9]+) (?P<path>[^\x00]*)\x00 (?P<target>[^\x00]*)\x00\n"
        )
        .unwrap();
    }
    let mut lines = vec![];
    while !rest.is_empty() {
        let caps = RE.captures(rest).unwrap();
        lines.push(RawFindLine {
            ty: caps["type"].chars().nth(0).unwrap(),
            pre_shadow: PreShadow {
                mode: u16::from_str_radix(&caps["mode"], 8)?,
                size: match &caps["size"] {
                    "?" => None,
                    size => Some(size.parse().unwrap()),
                },
                ctime: caps["ctime"].parse().unwrap(),
                ctime_ns: caps["ctime_ns"].parse().unwrap(),
                mtime: caps["mtime"].parse().unwrap(),
                mtime_ns: caps["mtime_ns"].parse().unwrap(),
                inode: caps["inode"].parse().unwrap(),
            },
            path: caps["path"].to_string(),
            target: caps["target"].to_string(),
        });
        rest = &rest[caps.get(0).unwrap().end()..];
    }
    Ok(lines)
}

fn parse_find_output(lines: Vec<RawFindLine>) -> Result<Snapshot<PreShadow>> {
    let mut entries = lines
        .into_iter()
        .map(|line| Ok((line.path.parse::<ShadowPath>()?, line)))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .peekable();
    let (path, line) = entries.next().ok_or_else(|| anyhow!("empty find output"))?;
    ensure!(path.components().is_empty(), "first find output entry is not the root");
    let ret = parse_find_output_inner(&mut entries, &path, line)?
        .ok_or_else(|| anyhow!("unsupported root file type"))?;
    if let Some((path, _)) = entries.peek() {
        bail!("find output entry out of order: {}", path);
    }
    Ok(ret)
}

fn parse_find_output_inner(
    entries: &mut Peekable<impl Iterator<Item = (ShadowPath, RawFindLine)>>,
    path: &ShadowPath,
    line: RawFindLine,
) -> Result<Option<Snapshot<PreShadow>>> {
    Ok(Some(match line.ty {
        'f' => {
            let executable = line.is_executable();
            Snapshot::File {
                shadow: line.pre_shadow,
                executable,
            }
        }
        'l' => Snapshot::Link { target: line.target },
        'd' => {
            let mut children = BTreeMap::new();
            while let Some((child_path, _)) = entries.peek() {
                if let Some((_, parent)) = child_path.components().split_last() && parent == path.components() {
                    let (mut child_path, child_line) = entries.next().unwrap();
                    let child = parse_find_output_inner(entries, &child_path, child_line)?;
                    let child_name = child_path.pop().unwrap();
                    if let Some(child) = child {
                        assert!(children.insert(child_name, child).is_none());
                    }
                } else {
                    break;
                }
            }
            Snapshot::Tree { entries: children }
        }
        _ => {
            log::warn!("skipping {:?}", line);
            return Ok(None);
        }
    }))
}

fn refine_shadow(subject: &Path, path: &ShadowPath, pre: PreShadow) -> Result<Shadow> {
    let file_path = path.append_to(subject);
    let metadata = fs::symlink_metadata(&file_path)?;
    let post = PreShadow {
        mode: (metadata.mode() & 0o7777) as u16,
        size: Some(metadata.size()),
        ctime: metadata.ctime(),
        ctime_ns: metadata.ctime_nsec(),
        mtime: metadata.mtime(),
        mtime_ns: metadata.mtime_nsec(),
        inode: metadata.ino(),
    };
    ensure!(
        pre == post,
        "{} changed while taking snapshot: {:?} != {:?}",
        file_path.display(),
        pre,
        post
    );
    let content_hash = sha256sum(&file_path)?;
    Ok(Shadow::new(content_hash, pre.size))
}
