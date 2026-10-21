use std::fs::{self, OpenOptions, Permissions};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};
use digest_io::IoWrapper;
use sha2::{Digest, Sha256};

use crate::{ContentSha256, Shadow};

pub trait Substance {
    fn content_path(&self, shadow: &Shadow) -> PathBuf;
    fn store_content(&self, shadow: &Shadow, src: &Path) -> Result<()>;

    fn have_content(&self, shadow: &Shadow) -> bool {
        self.content_path(shadow).is_file()
    }

    fn check_content_size(&self, shadow: &Shadow) -> Result<()> {
        check_path_size(&self.content_path(shadow), shadow.content_size())
    }

    fn check_content_hash(&self, shadow: &Shadow) -> Result<()> {
        check_path_sha256sum(&self.content_path(shadow), shadow.content_hash())
    }
}

pub struct FilesystemSubstance {
    path: PathBuf,
}

impl FilesystemSubstance {
    const SPLIT: usize = 3;

    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    fn content_dir(&self) -> PathBuf {
        self.path.join("blobs")
    }

    fn partial_dir(&self) -> PathBuf {
        self.path.join("partial")
    }

    fn content_relative_path(shadow: &Shadow) -> (String, String) {
        let hex = shadow.content_hash().to_hex();
        let (parent, child) = hex.split_at(Self::SPLIT);
        (parent.to_owned(), child.to_owned())
    }

    fn content_parent(&self, shadow: &Shadow) -> PathBuf {
        let (parent, _child) = Self::content_relative_path(shadow);
        self.content_dir().join(&parent)
    }

    fn partial_path(&self, shadow: &Shadow) -> PathBuf {
        self.partial_dir().join(shadow.content_hash().to_hex())
    }
}

impl Substance for FilesystemSubstance {
    fn content_path(&self, shadow: &Shadow) -> PathBuf {
        let (parent, child) = Self::content_relative_path(shadow);
        self.content_dir().join(&parent).join(&child)
    }

    fn store_content(&self, shadow: &Shadow, src: &Path) -> Result<()> {
        if self.have_content(shadow) {
            return Ok(());
        }

        let content_path = self.content_path(shadow);
        let partial_path = self.partial_path(shadow);

        assert!(src.is_file());
        let mut src_file = OpenOptions::new().read(true).open(src)?;

        let mut partial_file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&partial_path)?;

        // NOTE
        // On Linux, io::copy is specialized to use copy_file_range. On MacOS, fs::copy is
        // specialized to use fclonefileat and fcopyfile, but io::copy has not been specialized yet.
        io::copy(&mut src_file, &mut partial_file)?;

        partial_file.set_permissions(Permissions::from_mode(0o444))?;

        // NOTE
        // Could use digest_io::HashReader to hash and copy at the same time, but would miss out on
        // copy_file_range specialization.
        check_path_sha256sum(&partial_path, shadow.content_hash())?;

        let content_parent = self.content_parent(shadow);
        ensure_dir_exists(&content_parent)?;

        fs::rename(&partial_path, &content_path)?;
        Ok(())
    }
}

fn ensure_dir_exists(path: &Path) -> Result<()> {
    if path.exists() {
        assert!(path.is_dir());
    } else {
        fs::create_dir(path)?;
    }
    Ok(())
}

pub fn path_sha256sum(path: &Path) -> Result<ContentSha256> {
    let mut file = OpenOptions::new().read(true).open(path)?;
    let mut hasher = IoWrapper(Sha256::new());
    io::copy(&mut file, &mut hasher)?;
    Ok(ContentSha256::new(hasher.0.finalize().into()))
}

fn check_path_sha256sum(path: &Path, expected: &ContentSha256) -> Result<()> {
    let observerd = path_sha256sum(path)?;
    ensure!(expected == &observerd);
    Ok(())
}

pub fn path_size(path: &Path) -> Result<u64> {
    let meta = fs::metadata(path)?;
    Ok(meta.len())
}

fn check_path_size(path: &Path, expected: u64) -> Result<()> {
    let observerd = path_size(path)?;
    ensure!(expected == observerd);
    Ok(())
}
