use std::fs;
use std::os::unix::fs::MetadataExt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};

use keep::{Database, Entry, Shadow, Substance};

pub(crate) fn run(
    db: &mut Database,
    substance: &mut impl Substance,
    entry: &Entry,
    deep: bool,
) -> Result<()> {
    let mut ok = true;
    let mut total_b = 0;
    db.unique_shadows(entry, |path, shadow| {
        if !substance.have_content(shadow) {
            log::error!("missing content: {} {path}", shadow.content_hash());
            ok = false;
        } else {
            let size = shadow.content_size();
            total_b += size;
            if !substance.check_content_size(shadow).is_ok() {
                log::error!(
                    "content size mismatch: {} {path} (expected: {size})",
                    shadow.content_hash(),
                );
                ok = false;
            }
        }
        Ok(())
    })?;
    if deep {
        let v = {
            let mut v = vec![];
            db.unique_shadows(entry, |path, shadow| {
                if substance.have_content(shadow) {
                    v.push((
                        content_ord(substance, shadow),
                        (path.clone(), shadow.clone()),
                    ));
                }
                Ok(())
            })?;
            v.sort_by_key(|(ord, _)| *ord);
            v
        };
        let mut i = 0;
        let total_mb = b_to_mb(total_b);
        let mut cur_b = 0;
        let mut prev_b = cur_b;
        let t_start = now();
        let mut t_prev = t_start;
        for (_, (path, shadow)) in v.iter() {
            if !substance.check_content_hash(shadow).is_ok() {
                log::error!("content hash mismatch: {path} {}", shadow.content_hash());
            }
            i += 1;
            cur_b += shadow.content_size();
            if i % 1000 == 0 {
                let cur_mb = b_to_mb(cur_b);
                let t_cur = now();
                let d_t_prev = t_cur - t_prev;
                let mb_per_sec_since_prev =
                    (b_to_mb(cur_b - prev_b) as f64) / d_t_prev.as_secs_f64();
                let d_t_start = t_cur - t_start;
                let mb_per_sec_since_start = (cur_mb as f64) / d_t_start.as_secs_f64();
                log::info!(
                    "checked {i}/{} files, {cur_mb}/{total_mb} MB, {mb_per_sec_since_prev:.2} MB/sec ({mb_per_sec_since_start:.2} MB/sec overall)",
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

fn now() -> Duration {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
}

// HACK does this even help?
fn content_ord(substance: &mut impl Substance, shadow: &Shadow) -> u64 {
    fs::symlink_metadata(substance.content_path(shadow))
        .unwrap()
        .ino()
}

fn b_to_mb(b: u64) -> u64 {
    b / 1_000_000
}
