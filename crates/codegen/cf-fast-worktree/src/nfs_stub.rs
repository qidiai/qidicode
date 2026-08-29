//! Windows stand-in for the upstream `nfs/` module (FUSE/NFS worktrees are
//! Linux-only). Provides just the surface `api/gc.rs` consumes; every probe
//! declines and the orphan-pin pass is a no-op.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

#[cfg(feature = "metadata")]
use crate::db::WorktreeRecord;

/// Strategy names that mark a grove (FUSE/NFS) worktree. None exist off Unix.
pub const STRATEGY_GROVE_FUSE: &str = "grove-fuse";
pub const STRATEGY_GROVE_NFS: &str = "grove-nfs";
pub const STRATEGY_NFS: &str = "nfs";

/// Always false off Unix: no grove strategies exist, so `creation_mode` never
/// marks a record as one.
pub(crate) fn is_grove_strategy(s: &str) -> bool {
    let _ = s;
    false
}

/// Whether `path` is a dest we know is unmounted (so a plain `exists()` is
/// safe). Off Unix nothing is ever NFS-mounted, so every path is "known
/// unmounted" — `exists()`/`canonicalize` are always safe to call directly.
pub(crate) fn dest_is_known_unmounted(_path: &Path) -> bool {
    true
}

/// Path-containment probe that avoids stat'ing an NFS dest. Off Unix this is
/// just a prefix check.
pub(crate) fn dest_path_contains(parent: &Path, child: &Path) -> bool {
    child.starts_with(parent)
}

/// Dead-record detection for NFS-backed rows. Off Unix the normal
/// `!path.exists()` sweep handles every row.
pub(crate) fn nfs_record_is_dead(_dest: &Path, _backing: Option<&Path>) -> bool {
    false
}

/// Grove data dirs to scan for orphan pins. None off Unix.
pub(crate) fn candidate_data_dirs() -> Vec<PathBuf> {
    Vec::new()
}

/// Orphan-pin GC report (all-zero off Unix).
#[derive(Debug, Default, Clone)]
pub(crate) struct PinGcReport {
    pub examined: u64,
    pub pruned: u64,
    pub deferred_grace: u64,
    pub kept_live: u64,
    pub pruned_ids: Vec<String>,
}

/// Orphan-pin GC pass. Nothing to scan off Unix; reports zero work.
pub(crate) fn gc_orphan_pins(
    _dir: &Path,
    _existing: &[String],
    _now: i64,
    _dry_run: bool,
) -> anyhow::Result<PinGcReport> {
    let _ = (_dir, _existing, _now, _dry_run);
    Ok(PinGcReport::default())
}

/// Identities (by id) that worktree records still hold.
#[cfg(feature = "metadata")]
pub(crate) fn identities_from_worktree_records(_recs: &[WorktreeRecord]) -> Vec<String> {
    Vec::new()
}

#[cfg(not(feature = "metadata"))]
pub(crate) fn identities_from_worktree_records(_recs: &[()]) -> Vec<String> {
    Vec::new()
}
