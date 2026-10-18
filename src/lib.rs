mod paths;
mod shadow;
mod substance;
mod snapshot;
mod shallow_diff;
mod database;
mod cli;

#[rustfmt::skip]
pub use crate::{
    paths::{
        ShadowPath, ShadowPathComponent, ShadowTreeEntryName,
    },
    shadow::{
        Shadow, ContentSha256,
    },
    substance::{
        Substance, FilesystemSubstance,
        sha256sum,
    },
    snapshot::{
        Snapshot, SnapshotEntries, SnapshotEntry, SnapshotEntryValue,
    },
    shallow_diff::{
        ShallowDifference, ShallowDifferenceSide,
        shallow_diff,
    },
    database::{
        Database,
        TraversalCallbacks, Traverser,
        Visit, VisitShadow, VisitLink, VisitTree, VisitTreeDecision,
    },
    cli::{
        cli_main,
    },
};

pub(crate) fn exit_ok(status: std::process::ExitStatus) -> anyhow::Result<()> {
    anyhow::ensure!(
        status.success(),
        "process exited unsuccessfully: {}",
        status
    );
    Ok(())
}
