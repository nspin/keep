use std::fs::{self, OpenOptions, Permissions};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, Result};
use digest_io::IoWrapper;
use lazy_static::lazy_static;
use regex::bytes::Regex;
use sha2::{Digest, Sha256};

use crate::{exit_ok, ContentSha256};

pub trait Substance {
    fn blob_path(&self, blob: &ContentSha256) -> PathBuf;
    fn store(&self, blob: &ContentSha256, src: &Path) -> Result<()>;

    fn have_blob(&self, blob: &ContentSha256) -> bool {
        self.blob_path(blob).is_file()
    }

    fn check_blob(&self, blob: &ContentSha256) -> Result<()> {
        check_sha256sum(blob, &self.blob_path(blob))
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

    fn blob_dir(&self) -> PathBuf {
        self.path.join("blobs")
    }

    fn partial_dir(&self) -> PathBuf {
        self.path.join("partial")
    }

    fn blob_relative_path(blob: &ContentSha256) -> (String, String) {
        let mut hex = blob.to_hex();
        let child = hex.split_off(Self::SPLIT);
        (hex, child)
    }

    fn blob_parent(&self, blob: &ContentSha256) -> PathBuf {
        let (parent, _child) = Self::blob_relative_path(blob);
        self.blob_dir().join(&parent)
    }

    fn partial_path(&self, blob: &ContentSha256) -> PathBuf {
        let (parent, child) = Self::blob_relative_path(blob);
        self.partial_dir().join(&parent).join(&child)
    }

    fn partial_parent(&self, blob: &ContentSha256) -> PathBuf {
        let (parent, _child) = Self::blob_relative_path(blob);
        self.partial_dir().join(&parent)
    }
}

impl Substance for FilesystemSubstance {
    fn blob_path(&self, blob: &ContentSha256) -> PathBuf {
        let (parent, child) = Self::blob_relative_path(blob);
        self.blob_dir().join(&parent).join(&child)
    }

    fn store(&self, blob: &ContentSha256, src: &Path) -> Result<()> {
        if self.have_blob(blob) {
            return Ok(());
        }

        let blob_path = self.blob_path(blob);
        let partial_path = self.partial_path(blob);

        assert!(src.is_file());
        let mut source_file = OpenOptions::new().read(true).open(src)?;

        let partial_parent = self.partial_parent(blob);
        ensure_dir_exists(&partial_parent)?;

        let mut partial_file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&partial_path)?;

        // TODO
        // - https://github.com/rust-lang/rust/blob/55ccbd090d96ec3bb28dbcb383e65bbfa3c293ff/library/std/src/sys/unix/fs.rs#L1277
        // - linux:
        //      - copy_file_range
        //      - https://lwn.net/Articles/846403/, https://lwn.net/Articles/846670/
        //      - https://github.com/rust-lang/rust/commit/4ddedd521418d67e845ecb43dc02c09b0af53022
        // - macos:
        //      - fclonefileat and fcopyfile
        io::copy(&mut source_file, &mut partial_file)?;

        partial_file.set_permissions(Permissions::from_mode(0o444))?;

        check_sha256sum(blob, &partial_path)?;

        let blob_parent = self.blob_parent(blob);
        ensure_dir_exists(&blob_parent)?;

        fs::rename(&partial_path, &blob_path)?;
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

pub struct MockSubstance {
    token_blob_path: PathBuf,
}

impl MockSubstance {
    pub fn new(token_blob_path: impl AsRef<Path>) -> Self {
        Self {
            token_blob_path: token_blob_path.as_ref().to_path_buf(),
        }
    }
}

impl Substance for MockSubstance {
    fn blob_path(&self, _: &ContentSha256) -> PathBuf {
        self.token_blob_path.clone()
    }

    fn store(&self, blob: &ContentSha256, src: &Path) -> Result<()> {
        check_sha256sum(blob, src)?;
        Ok(())
    }
}

pub fn sha256sum_coreutils(path: &Path) -> Result<ContentSha256> {
    lazy_static! {
        static ref RE: Regex =
            Regex::new(r"(?-u)(?P<digest>[a-z0-9]{64}|[?]{64}) \*(?P<path>.*)\x00").unwrap();
    }
    let output = Command::new("sha256sum")
        .arg("-bz")
        .arg(path)
        .stderr(Stdio::inherit())
        .output()
        .unwrap();
    exit_ok(output.status)?;
    // use std::os::unix::ffi::OsStrExt;
    // eprintln!("{:?}", std::ffi::OsStr::from_bytes(&output.stdout));
    let caps = RE
        .captures(&output.stdout)
        .ok_or(anyhow!("regex does not match"))?;
    Ok(std::str::from_utf8(&caps["digest"])?.parse()?)
}

#[allow(dead_code)]
pub fn sha256sum_rust(path: &Path) -> Result<ContentSha256> {
    let mut file = OpenOptions::new().read(true).open(path)?;
    let mut hasher = IoWrapper(Sha256::new());
    io::copy(&mut file, &mut hasher)?;
    let hash = hasher.0.finalize();
    Ok(ContentSha256::from_slice(&hash))
}

pub fn sha256sum(path: &Path) -> Result<ContentSha256> {
    sha256sum_coreutils(path)
}

fn check_sha256sum(expected: &ContentSha256, path: &Path) -> Result<()> {
    let observerd = sha256sum(path)?;
    assert_eq!(expected, &observerd);
    Ok(())
}
