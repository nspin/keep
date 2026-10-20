use std::collections::BTreeMap;
use std::fs;
use std::iter::Peekable;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::str;

use anyhow::{Context, Result, ensure};
use lazy_static::lazy_static;
use regex::{Captures, Regex};

use crate::{Shadow, ShadowPath, ShadowPathComponent, path_sha256sum, with_report_progress};

#[derive(Clone, Debug)]
pub enum Snapshot<T = Shadow> {
    File {
        shadow: T,
        executable: bool,
    },
    Link {
        target: String,
    },
    Tree {
        entries: BTreeMap<ShadowPathComponent, Snapshot<T>>,
    },
}

impl<T> Snapshot<T> {
    fn traverse_with_path<U, E, F>(self, f: &mut F) -> Result<Snapshot<U>, E>
    where
        F: FnMut(&ShadowPath, T) -> Result<U, E>,
    {
        self.traverse_with_path_inner(&mut ShadowPath::new(), f)
    }

    fn traverse_with_path_inner<U, E, F>(
        self,
        path: &mut ShadowPath,
        f: &mut F,
    ) -> Result<Snapshot<U>, E>
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
                        let value = value.traverse_with_path_inner(path, f);
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
        let meta_snapshot = parse_find_output(run_find(subject)?)?;
        with_report_progress(
            1000,
            |i| log::info!("hashed {i} blobs"),
            |p| {
                meta_snapshot.traverse_with_path(&mut |path, meta| {
                    let file_path = path.append_to(subject);
                    meta.check(&file_path)?;
                    let content_hash = path_sha256sum(&file_path)?;
                    (p)();
                    Ok(Shadow::new(content_hash, Some(meta.size)))
                })
            },
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Meta {
    ty: char, // [dflcbsp]
    perms: u16,
    size: u64,
    ctime: i64,
    ctime_ns: i64,
    mtime: i64,
    mtime_ns: i64,
    inode: u64,
    uid: u32,
    gid: u32,
    dev: u64,
}

impl Meta {
    fn is_executable(&self) -> bool {
        self.perms & 0o100 != 0
    }

    fn stat(path: &Path) -> Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        let ty = match metadata.mode() >> 12 {
            0o10 => 'f',
            0o04 => 'd',
            0o12 => 'l',
            0o06 => 'b',
            0o02 => 'c',
            0o01 => 'p',
            0o14 => 's',
            _ => panic!(),
        };
        let perms = (metadata.mode() & 0o7777).try_into().unwrap();
        Ok(Self {
            ty,
            perms,
            size: metadata.size(),
            ctime: metadata.ctime(),
            ctime_ns: metadata.ctime_nsec(),
            mtime: metadata.mtime(),
            mtime_ns: metadata.mtime_nsec(),
            inode: metadata.ino(),
            uid: metadata.uid(),
            gid: metadata.gid(),
            dev: metadata.dev(),
        })
    }

    fn check(&self, path: &Path) -> Result<()> {
        let actual = Self::stat(path)?;
        ensure!(
            self == &actual,
            "{} changed while taking snapshot: {:?} != {:?}",
            path.display(),
            self,
            actual
        );
        Ok(())
    }
}

#[derive(Debug)]
struct RawFindLine {
    meta: Meta,
    path: String,
    target: String,
}

impl RawFindLine {
    fn parse(s: &str) -> Vec<Self> {
        lazy_static! {
            static ref RE: Regex = Regex::new(
                r"(?x)
                \A
                (?P<type>[dflcbsp])                             # %y
                \ 0(?P<perms>[0-7]*)                            # %#m
                \ (?P<size>[0-9]+)                              # %s
                \ (?P<ctime>[0-9]+)\.(?P<ctime_ns>[0-9]{9})0    # %C@
                \ (?P<mtime>[0-9]+)\.(?P<mtime_ns>[0-9]{9})0    # %T@
                \ (?P<inode>[0-9]+)                             # %i
                \ (?P<uid>[0-9]+)                               # %U
                \ (?P<gid>[0-9]+)                               # %G
                \ (?P<dev>[0-9]+)                               # %D
                \ (?P<path>[^\x00]*)\x00                        # %P\0
                \ (?P<target>[^\x00]*)\x00                      # %l\0
                \n
                "
            )
            .unwrap();
        }
        let mut lines = vec![];
        let mut rest = s;
        while !rest.is_empty() {
            let caps = RE.captures(rest).unwrap();
            lines.push(Self::parse_captures(&caps));
            rest = &rest[caps.get(0).unwrap().end()..];
        }
        lines
    }

    fn parse_captures(caps: &Captures) -> Self {
        Self {
            meta: Meta {
                ty: caps["type"].chars().nth(0).unwrap(),
                perms: u16::from_str_radix(&caps["perms"], 8).unwrap(),
                size: caps["size"].parse().unwrap(),
                ctime: caps["ctime"].parse().unwrap(),
                ctime_ns: caps["ctime_ns"].parse().unwrap(),
                mtime: caps["mtime"].parse().unwrap(),
                mtime_ns: caps["mtime_ns"].parse().unwrap(),
                inode: caps["inode"].parse().unwrap(),
                uid: caps["uid"].parse().unwrap(),
                gid: caps["gid"].parse().unwrap(),
                dev: caps["dev"].parse().unwrap(),
            },
            path: caps["path"].to_string(),
            target: caps["target"].to_string(),
        }
    }
}

fn run_find(subject: &Path) -> Result<Vec<RawFindLine>> {
    assert!(!subject.as_os_str().as_bytes().starts_with(b"-"));
    let output = Command::new("find")
        .arg(subject)
        .arg("-printf")
        .arg(r#"%y %#m %s %C@ %T@ %i %U %G %D %P\0 %l\0\n"#)
        .stderr(Stdio::inherit())
        .output()
        .context("failed to run find")?;
    ensure!(output.status.success());
    Ok(RawFindLine::parse(str::from_utf8(&output.stdout).unwrap()))
}

fn parse_find_output(lines: Vec<RawFindLine>) -> Result<Snapshot<Meta>> {
    let mut lines = lines
        .into_iter()
        .filter(|line| {
            let keep = matches!(line.meta.ty, 'f' | 'l' | 'd');
            if !keep {
                log::warn!("skipping {:?}", line);
            }
            keep
        })
        .peekable();
    let line = lines.next().unwrap();
    let line_path = line.path.parse::<ShadowPath>()?;
    assert!(line_path.components().is_empty());
    let snapshot = parse_find_output_inner(&mut lines, line, &line_path)?;
    assert!(lines.peek().is_none());
    Ok(snapshot)
}

fn parse_find_output_inner(
    lines: &mut Peekable<impl Iterator<Item = RawFindLine>>,
    line: RawFindLine,
    line_path: &ShadowPath,
) -> Result<Snapshot<Meta>> {
    Ok(match line.meta.ty {
        'f' => {
            let executable = line.meta.is_executable();
            Snapshot::File {
                shadow: line.meta,
                executable,
            }
        }
        'l' => Snapshot::Link {
            target: line.target,
        },
        'd' => {
            let mut entries = BTreeMap::new();
            while let Some(next_line) = lines.peek() {
                let next_line_path = next_line.path.parse::<ShadowPath>()?;
                if let Some([name]) = next_line_path
                    .components()
                    .strip_prefix(line_path.components())
                {
                    let next_line = lines.next().unwrap();
                    let value = parse_find_output_inner(lines, next_line, &next_line_path)?;
                    assert!(entries.insert(name.clone(), value).is_none());
                } else {
                    break;
                }
            }
            Snapshot::Tree { entries }
        }
        _ => unreachable!(),
    })
}
