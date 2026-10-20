use std::fs;
use std::os::unix::fs::MetadataExt;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use git2::Oid;

use keep::{Database, Shadow, Substance};

pub(crate) fn run(
    db: &mut Database,
    substance: &mut impl Substance,
    tree: Oid,
    deep: bool,
) -> Result<()> {
    let mut ok = true;
    let mut total_bytes = 0;
    db.unique_shadows(tree, |path, blob| {
        if blob.content_size().is_none() {
            log::warn!("missing size: {} {}", blob.content_hash(), path);
        }
        if !substance.have_blob(blob.content_hash()) {
            log::error!("missing blob: {} {}", blob.content_hash(), path);
            ok = false;
        } else if let Some(size) = blob.content_size() {
            total_bytes += size;
            if !substance.check_blob_size(blob.content_hash(), size).is_ok() {
                log::error!(
                    "blob size mismatch: {} {} (expected: {size})",
                    blob.content_hash(),
                    path
                );
            }
        }
        Ok(())
    })?;
    // if !ok {
    //     bail!("some failures");
    // }
    if deep {
        let v = {
            let mut v = vec![];
            db.unique_shadows(tree, |path, blob| {
                if substance.have_blob(blob.content_hash()) {
                    v.push((blob_ord(substance, blob), (path.clone(), blob.clone())));
                }
                Ok(())
            })?;
            v.sort_by_key(|(ord, _)| *ord);
            v
        };
        let mut i = 0;
        let total_mb = b_to_mb(total_bytes);
        let mut cur_b = 0;
        let mut prev_b = cur_b;
        let t_start = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let mut t_prev = t_start;
        for (_, (path, blob)) in v.iter() {
            if !substance.check_blob_sha256sum(blob.content_hash()).is_ok() {
                log::error!("blob content mismatch: {} {}", blob.content_hash(), path);
            }
            i += 1;
            if let Some(size) = blob.content_size() {
                cur_b += size;
            }
            if i % 1000 == 0 {
                let cur_mb = b_to_mb(cur_b);
                let d_mb = b_to_mb(cur_b - prev_b);
                let t_cur = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                let d_t = t_cur - t_prev;
                let mb_per_sec = d_mb as f64 / d_t.as_secs_f64();
                let overall_d_t = t_cur - t_start;
                let overall_mb_per_sec = cur_mb as f64 / overall_d_t.as_secs_f64();
                log::info!(
                    "checked {i}/{} blobs, {cur_mb}/{total_mb} MB, {mb_per_sec:.2} MB/sec ({overall_mb_per_sec:.2} MB/sec overall)",
                    v.len()
                );
                prev_b = cur_b;
                t_prev = t_cur;
            }
        }
        if !ok {
            bail!("some failures");
        }
    }
    Ok(())
}

// HACK does this even help?
fn blob_ord(substance: &mut impl Substance, blob: &Shadow) -> u64 {
    fs::symlink_metadata(substance.blob_path(blob.content_hash()))
        .unwrap()
        .ino()
}

fn b_to_mb(b: u64) -> u64 {
    b / 1_000_000
}
