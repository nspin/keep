use std::collections::BTreeMap;
use std::convert::{TryFrom, TryInto};
use std::error::Error;
use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::iter::{FromIterator, IntoIterator};
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{Result, bail, ensure};
use fuser::{
    Config, Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags, Generation, INodeNo,
    LockOwner, MountOption, OpenFlags, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyOpen, Request,
};
use git2::{FileMode, ObjectType, Oid, Repository, TreeEntry};
use itertools::Itertools;
use log::error;

use crate::{Database, Shadow, ShadowPathComponent, ShadowTreeEntryName, Substance};

const FS_NAME: &str = "keep";

impl Database {
    pub fn mount(
        self,
        tree: Oid,
        mountpoint: impl AsRef<Path>,
        substance: impl Substance + Send + 'static,
        uid: u32,
        gid: u32,
    ) -> Result<()> {
        let mount_options = vec![
            MountOption::RO,
            MountOption::NoDev,
            MountOption::NoExec,
            MountOption::NoAtime,
            MountOption::Sync,
            MountOption::DirSync,
            MountOption::FSName(FS_NAME.to_string()),
            // TODO
            // MountOption::AutoUnmount
        ];
        let mut config = Config::default();
        config.mount_options = mount_options;
        let fs = DatabaseFilesystem::new(self.repository, tree, substance, uid, gid);
        fuser::mount(fs, mountpoint, &config)?;
        Ok(())
    }
}

const TTL: Duration = Duration::from_secs(1);

macro_rules! fry {
    ($reply:ident, $x:expr) => {{
        match $x {
            Ok(ok) => ok,
            Err(err) => {
                error!("error at {},{}: {}", file!(), line!(), err);
                $reply.error(Errno::EINVAL);
                return;
            }
        }
    }};
}

enum InodeEntry {
    File { oid: Oid, executable: bool },
    Link { oid: Oid },
    Tree { oid: Oid, parent: INodeNo },
}

pub struct DatabaseFilesystem<T> {
    state: Mutex<DatabaseFilesystemState<T>>,
}

struct DatabaseFilesystemState<T> {
    repository: Repository,
    inodes: BTreeMap<INodeNo, InodeEntry>,
    family_tree: BTreeMap<(INodeNo, usize), INodeNo>,
    next_inode: INodeNo,
    file_handles: BTreeMap<INodeNo, SharedFile>,
    substance: T,
    uid: u32,
    gid: u32,
}

struct SharedFile {
    file: File,
    reference_count: usize,
}

impl SharedFile {
    fn new(file: File) -> Self {
        Self {
            file,
            reference_count: 1,
        }
    }

    fn increment(&mut self) {
        self.reference_count += 1;
    }

    fn decrement(&mut self) -> bool {
        self.reference_count -= 1;
        self.reference_count > 0
    }
}

impl<T> DatabaseFilesystem<T> {
    pub fn new(repository: Repository, tree: Oid, substance: T, uid: u32, gid: u32) -> Self {
        Self {
            state: Mutex::new(DatabaseFilesystemState::new(
                repository, tree, substance, uid, gid,
            )),
        }
    }
}

impl<T> DatabaseFilesystemState<T> {
    fn new(repository: Repository, tree: Oid, substance: T, uid: u32, gid: u32) -> Self {
        Self {
            repository,
            inodes: BTreeMap::from_iter([(
                INodeNo::ROOT,
                InodeEntry::Tree {
                    parent: INodeNo::ROOT,
                    oid: tree,
                },
            )]),
            family_tree: BTreeMap::new(),
            next_inode: INodeNo(INodeNo::ROOT.0 + 1),
            file_handles: BTreeMap::new(),
            substance,
            uid,
            gid,
        }
    }
}

impl<T: Substance> DatabaseFilesystemState<T> {
    fn get_inode(&mut self, parent: INodeNo, entry: TreeEntry<'static>) -> Result<INodeNo> {
        let ino = self.next_inode;
        self.next_inode = INodeNo(ino.0 + 1);
        let oid = entry.id();
        let mode = entry.filemode();
        let entry = match entry.kind().unwrap() {
            ObjectType::Blob => {
                if mode == FileMode::Link.into() {
                    InodeEntry::Link { oid }
                } else {
                    let executable = if mode == FileMode::Blob.into() {
                        true
                    } else if mode == FileMode::BlobExecutable.into() {
                        false
                    } else {
                        bail!("")
                    };
                    InodeEntry::File { oid, executable }
                }
            }
            ObjectType::Tree => {
                ensure!(mode == FileMode::Tree.into());
                InodeEntry::Tree { oid, parent }
            }
            _ => {
                bail!("");
            }
        };
        self.inodes.insert(ino, entry);
        Ok(ino)
    }

    fn fetch_attr(&self, ino: INodeNo) -> Result<FileAttr> {
        let (kind, perm, size) = match self.inodes.get(&ino).unwrap() {
            InodeEntry::File { oid, executable } => {
                let kind = FileType::RegularFile;
                let perm = 0o444 | (if *executable { 0o000 } else { 0o111 });
                let blob = self.repository.find_blob(*oid)?;
                let shadow = Shadow::from_bytes(blob.content())?;
                let size = shadow.content_size();
                (kind, perm, size)
            }
            InodeEntry::Link { oid } => {
                let kind = FileType::Symlink;
                let perm = 0o555;
                let blob = self.repository.find_blob(*oid)?;
                let size = blob.size().try_into().unwrap();
                (kind, perm, size)
            }
            InodeEntry::Tree { .. } => {
                let kind = FileType::Directory;
                let perm = 0o555;
                let size = 0; // TODO
                (kind, perm, size)
            }
        };
        Ok(FileAttr {
            ino,
            size,
            blocks: 0,
            atime: UNIX_EPOCH,
            mtime: UNIX_EPOCH,
            ctime: UNIX_EPOCH,
            crtime: UNIX_EPOCH,
            kind,
            perm,
            nlink: 0,
            uid: self.uid,
            gid: self.gid,
            rdev: 0,
            blksize: 0,
            flags: 0,
        })
    }

    fn open_blob(&mut self, ino: INodeNo) -> Result<()> {
        if let Some(shared) = self.file_handles.get_mut(&ino) {
            shared.increment();
            return Ok(());
        }
        let oid = match self.inodes.get(&ino).unwrap() {
            InodeEntry::File { oid, .. } => oid,
            _ => bail!("not a file"),
        };
        let blob = self.repository.find_blob(*oid)?;
        let shadow = Shadow::from_bytes(blob.content())?;
        let blob_path = self.substance.blob_path(shadow.content_hash());
        let file = OpenOptions::new().read(true).open(blob_path)?;
        self.file_handles.insert(ino, SharedFile::new(file));
        Ok(())
    }

    fn close_blob(&mut self, ino: INodeNo) -> Result<()> {
        if !self.file_handles.get_mut(&ino).unwrap().decrement() {
            self.file_handles.remove(&ino);
        }
        Ok(())
    }
}

impl<T: Substance + Send + 'static> Filesystem for DatabaseFilesystem<T> {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        self.state.lock().unwrap().lookup(parent, name, reply)
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        reply: ReplyDirectory,
    ) {
        self.state.lock().unwrap().readdir(ino, offset, reply)
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        self.state.lock().unwrap().getattr(ino, reply)
    }

    fn readlink(&self, _req: &Request, ino: INodeNo, reply: ReplyData) {
        self.state.lock().unwrap().readlink(ino, reply)
    }

    fn open(&self, _req: &Request, ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        self.state.lock().unwrap().open(ino, reply)
    }

    fn release(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        self.state.lock().unwrap().release(ino, reply)
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        self.state.lock().unwrap().read(ino, offset, size, reply)
    }
}

impl<T: Substance> DatabaseFilesystemState<T> {
    fn lookup(&mut self, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let oid = fry!(
            reply,
            match self.inodes.get_mut(&parent).unwrap() {
                InodeEntry::Tree { oid, .. } => Ok(oid),
                _ => Err(Box::<dyn Error>::from(format!(
                    "lookup: parent inode {} not present",
                    parent
                ))),
            }
        );
        let tree = self.repository.find_tree(*oid).unwrap();
        let entry_name = name
            .to_str()
            .unwrap()
            .parse::<ShadowPathComponent>()
            .unwrap()
            .encode();
        let entries = tree.iter().map(|entry| entry.to_owned()).collect_vec();
        drop(tree);
        for (i, entry) in entries.into_iter().enumerate() {
            if entry.name().unwrap() == entry_name {
                let ino = match self.family_tree.get(&(parent, i)) {
                    Some(ino) => *ino,
                    None => {
                        let ino = fry!(reply, self.get_inode(parent, entry));
                        self.family_tree.insert((parent, i), ino);
                        ino
                    }
                };
                let attr = fry!(reply, self.fetch_attr(ino));
                reply.entry(&TTL, &attr, Generation(0));
                return;
            }
        }
        reply.error(Errno::ENOENT);
    }

    fn readdir(&mut self, ino: INodeNo, offset: u64, mut reply: ReplyDirectory) {
        let (oid, parent) = fry!(
            reply,
            match self.inodes.get(&ino).unwrap() {
                InodeEntry::Tree { oid, parent } => Ok((*oid, *parent)),
                _ => Err(Box::<dyn Error>::from(format!(
                    "readdir: inode {} not present",
                    ino
                ))),
            }
        );
        let always: Vec<Result<Option<(INodeNo, FileType, String)>>> = vec![
            Ok(Some((ino, FileType::Directory, ".".into()))),
            Ok(Some((parent, FileType::Directory, "..".into()))),
        ];
        let tree_entries = self
            .repository
            .find_tree(oid)
            .unwrap()
            .iter()
            .map(|entry| entry.to_owned())
            .collect_vec();
        let entries = always
            .into_iter()
            .chain(tree_entries.into_iter().enumerate().map(|(i, entry)| {
                let name = match ShadowTreeEntryName::decode(entry.name().unwrap()).unwrap() {
                    ShadowTreeEntryName::Marker => return Ok(None),
                    ShadowTreeEntryName::Child(child) => child.to_string(),
                };
                let ino = match self.family_tree.get(&(ino, i)) {
                    Some(ino) => *ino,
                    None => {
                        let ino = self.get_inode(ino, entry)?;
                        self.family_tree.insert((ino, i), ino);
                        ino
                    }
                };
                let kind = match self.inodes.get(&ino).unwrap() {
                    InodeEntry::File { .. } => FileType::RegularFile,
                    InodeEntry::Link { .. } => FileType::Symlink,
                    InodeEntry::Tree { .. } => FileType::Directory,
                };
                Ok(Some((ino, kind, name)))
            }));
        for (i, fallible_entry) in entries.enumerate().skip(offset.try_into().unwrap()) {
            if let Some((ino, kind, name)) = fallible_entry.unwrap() {
                // i + 1 means the index of the next entry
                let full = reply.add(ino, (i + 1) as u64, kind, name);
                if full {
                    break;
                }
            }
        }
        reply.ok();
    }

    fn getattr(&mut self, ino: INodeNo, reply: ReplyAttr) {
        let attr = fry!(reply, self.fetch_attr(ino));
        reply.attr(&TTL, &attr);
    }

    fn readlink(&mut self, ino: INodeNo, reply: ReplyData) {
        let oid = fry!(
            reply,
            match self.inodes.get(&ino).unwrap() {
                InodeEntry::Link { oid, .. } => Ok(oid),
                _ => Err(Box::<dyn Error>::from(format!(
                    "readlink: inode {} not present",
                    ino
                ))),
            }
        );
        let blob = self.repository.find_blob(*oid).unwrap();
        let target = blob.content();
        reply.data(target);
    }

    fn open(&mut self, ino: INodeNo, reply: ReplyOpen) {
        fry!(reply, self.open_blob(ino));
        reply.opened(FileHandle(0), FopenFlags::empty())
    }

    fn release(&mut self, ino: INodeNo, reply: ReplyEmpty) {
        fry!(reply, self.close_blob(ino));
        reply.ok()
    }

    fn read(&mut self, ino: INodeNo, offset: u64, size: u32, reply: ReplyData) {
        let file = &mut self.file_handles.get_mut(&ino).unwrap().file;
        let mut buf = vec![0u8; size.try_into().unwrap()];
        let n = unsafe {
            libc::pread(
                file.as_raw_fd(),
                buf.as_mut_ptr() as *mut libc::c_void,
                size.try_into().unwrap(),
                offset.try_into().unwrap(),
            )
        };
        assert!(n >= 0);
        let n = usize::try_from(n).unwrap();
        reply.data(&buf[..n]);
    }
}
