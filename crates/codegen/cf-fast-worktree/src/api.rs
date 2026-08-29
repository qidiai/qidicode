//! Public API for fast worktree creation.
//!
//! This module provides a higher-level, explicit API (builder + enums) that makes
//! behavior clear (what to copy, whether to copy ignored files, and how to finalize).
//!

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use tokio_util::sync::CancellationToken;

use crate::copy::CopyStats;
pub use crate::copy::DirtyFilesReport;
use crate::copy::ParallelCopyConfig;

// ============================================================================
// BtrfsDelegate – delegate privileged btrfs ops to an external service
// ============================================================================

/// Result from a delegated btrfs snapshot creation.
#[derive(Debug, Clone)]
pub struct DelegateSnapshotResult {
    /// Path to the actual btrfs snapshot.
    pub snapshot_path: PathBuf,
    /// Path where the worktree is accessible (bind-mounted from `snapshot_path`).
    pub worktree_path: PathBuf,
    /// Whether a bind mount was created from `snapshot_path` to `worktree_path`.
    pub bind_mounted: bool,
}

/// Delegate privileged btrfs operations to an external helper.
///
/// When the caller runs inside a sandbox without `CAP_SYS_ADMIN`, it cannot
/// execute `btrfs subvolume snapshot/delete` directly. This trait lets it
/// delegate those operations to a privileged process (e.g. over IPC).
///
/// Implementations must be `Send + Sync` (shared across threads).
pub trait BtrfsDelegate: Send + Sync {
    /// Create a btrfs snapshot of `source` accessible at `dest`.
    ///
    /// The implementation is expected to:
    /// 1. Detect whether `source` is a btrfs subvolume
    /// 2. Create a snapshot (inside the btrfs filesystem)
    /// 3. Bind mount `dest` from snapshot if source is bind-mounted
    /// 4. Clean up stale git state (lock files, worktree registrations)
    fn create_snapshot(&self, source: &Path, dest: &Path) -> Result<DelegateSnapshotResult>;

    /// Delete a btrfs snapshot worktree.
    ///
    /// If `worktree_path` is a bind mount, the implementation should unmount it,
    /// delete the btrfs snapshot, and clean up the mount point.
    fn delete_snapshot(&self, worktree_path: &Path) -> Result<RemoveReport>;

    /// Mount an overlayfs at `target` in the *caller's* mount namespace.
    ///
    /// A FUSE+overlay worktree needs a new overlay mount, which a rootless
    /// caller can't do (no `CAP_SYS_ADMIN`); the privileged delegate mounts it
    /// inside the caller's namespace (an overlay mount can't be exposed via a
    /// namespace-crossing symlink the way a btrfs snapshot can). Default impl
    /// errors so btrfs-only delegates still compile.
    fn mount_overlay(&self, lower: &Path, upper: &Path, work: &Path, target: &Path) -> Result<()> {
        let _ = (lower, upper, work, target);
        anyhow::bail!("overlay mount delegation not supported by this delegate")
    }

    /// Unmount an overlay worktree previously mounted via [`Self::mount_overlay`]
    /// (in the caller's mount namespace).
    fn unmount_overlay(&self, target: &Path) -> Result<()> {
        let _ = target;
        anyhow::bail!("overlay unmount delegation not supported by this delegate")
    }
}

/// How to treat the source working tree when creating the destination worktree.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum WorkingTreeMode {
    /// Replicate the working tree exactly as-is (including local modifications and untracked files).
    #[default]
    PreserveWorkingTree,
    /// Produce a clean checked-out working tree for tracked files.
    ///
    /// Local modifications and untracked files from the source are not copied.
    CleanTracked,
    /// Produce a clean worktree and also remove any untracked files (equivalent to
    /// `git reset --hard` + `git clean -fd`).
    ///
    /// Note: ignored files are not removed by default `git clean`.
    CleanAll,
}

/// Whether (and how) to copy `.gitignore`'d files after the worktree is ready.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum IgnoredFilesMode {
    /// Do not copy ignored files.
    #[default]
    Skip,
    /// Copy ignored files, optionally skipping additional patterns.
    Copy { skip_patterns: Vec<String> },
    /// Copy ONLY ignored files (no worktree creation), optionally skipping additional patterns.
    /// This is for standalone use via `copy_ignored_only()`.
    CopyOnly { skip_patterns: Vec<String> },
}

/// How to handle BTRFS snapshot optimization on Linux.
///
/// On Linux systems where the source repo is on a BTRFS subvolume,
/// we can use BTRFS snapshots for O(1) worktree creation instead of
/// file-by-file CoW cloning.
///
/// The snapshot creates a complete standalone git repository (not a
/// linked git worktree), which is immediately usable.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum BtrfsMode {
    /// Auto-detect: use BTRFS snapshot if source is on a BTRFS subvolume.
    /// Falls back to file-by-file copy if not on BTRFS or not a subvolume.
    #[default]
    Auto,
    /// Force use of BTRFS snapshot. Returns an error if the source is not
    /// on a BTRFS subvolume.
    Force,
    /// Disable BTRFS snapshot optimization. Always use file-by-file copy.
    Disabled,
}

/// Strategy for creating the worktree.
///
/// Consolidates the choice of linked vs standalone, BTRFS snapshots,
/// and git-native checkout into a single enum.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum CreationMode {
    /// Linked worktree via `git worktree add --no-checkout` followed by
    /// parallel CoW file copy and index finalization. On Linux with BTRFS,
    /// auto-detects and uses instant snapshots when possible.
    ///
    /// This is the fastest mode for large repos on APFS/Btrfs.
    #[default]
    Linked,

    /// Standalone repository copy with its own independent `.git/`
    /// directory (CoW'd from the source). Can be promoted to replace the
    /// source via a simple `rename()`, with no worktree cleanup needed.
    ///
    /// On Linux with BTRFS, auto-detects and uses instant snapshots.
    Standalone,

    /// Plain `git worktree add` with full checkout. Lets git handle the
    /// entire worktree creation including index and working tree
    /// population. Simpler and avoids split-index / index-copy edge
    /// cases, but git does the checkout single-threaded.
    GitCheckout,
}

impl CreationMode {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::Linked => "linked",
            Self::Standalone => "standalone",
            Self::GitCheckout => "git",
        }
    }
}

/// A structured report for a copy phase.
#[derive(Clone, Debug, Default)]
pub struct CopyReport {
    pub files_copied: u64,
    pub dirs_created: u64,
    pub symlinks_copied: u64,
    pub files_skipped: u64,
    /// Non-fatal issues encountered during copying.
    pub issues: Vec<String>,
    pub dirty_files: Option<DirtyFilesReport>,
}

impl From<CopyStats> for CopyReport {
    fn from(stats: CopyStats) -> Self {
        Self {
            files_copied: stats.files_copied,
            dirs_created: stats.dirs_created,
            symlinks_copied: stats.symlinks_copied,
            files_skipped: stats.files_skipped,
            issues: stats.issues,
            dirty_files: None,
        }
    }
}

/// Result of creating a worktree via the new API.
#[derive(Debug)]
pub struct WorktreeReport {
    pub worktree_path: PathBuf,
    pub commit: String,
    pub unignored_copy: CopyReport,
    pub ignored_copy: Option<CopyReport>,
}

/// High-level builder API for creating fast git worktrees.
///
/// All operations are **synchronous/blocking**. Callers should use `spawn_blocking`
/// when calling from async contexts.
#[derive(Clone)]
pub struct WorktreeBuilder {
    source: PathBuf,
    dest: PathBuf,
    git_ref: String,
    parallelism: usize,
    channel_buffer: usize,
    ignored_parallelism: usize,
    working_tree: WorkingTreeMode,
    ignored_files: IgnoredFilesMode,
    creation_mode: CreationMode,
    cancellation_token: CancellationToken,
    btrfs_delegate: Option<Arc<dyn BtrfsDelegate>>,
    #[cfg(feature = "metadata")]
    worktree_kind: Option<crate::db::WorktreeKind>,
    #[cfg(feature = "metadata")]
    session_id: Option<String>,
    #[cfg(feature = "metadata")]
    worktree_id: Option<String>,
    #[cfg(feature = "metadata")]
    metadata: Option<serde_json::Value>,
}

impl std::fmt::Debug for WorktreeBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorktreeBuilder")
            .field("source", &self.source)
            .field("dest", &self.dest)
            .field("git_ref", &self.git_ref)
            .field("parallelism", &self.parallelism)
            .field("creation_mode", &self.creation_mode)
            .field("btrfs_delegate", &self.btrfs_delegate.is_some())
            .finish_non_exhaustive()
    }
}

impl WorktreeBuilder {
    pub fn new(source: impl Into<PathBuf>, dest: impl Into<PathBuf>) -> Self {
        Self {
            source: source.into(),
            dest: dest.into(),
            git_ref: "HEAD".to_string(),
            parallelism: 0,
            channel_buffer: 256,
            ignored_parallelism: 0,
            working_tree: WorkingTreeMode::PreserveWorkingTree,
            ignored_files: IgnoredFilesMode::Skip,
            creation_mode: CreationMode::default(),
            cancellation_token: CancellationToken::new(),
            btrfs_delegate: None,
            #[cfg(feature = "metadata")]
            worktree_kind: None,
            #[cfg(feature = "metadata")]
            session_id: None,
            #[cfg(feature = "metadata")]
            worktree_id: None,
            #[cfg(feature = "metadata")]
            metadata: None,
        }
    }

    /// Set a cancellation token that can be used to stop a copy operation in progress.
    /// When the token is cancelled, the copy will stop as soon as possible.
    pub fn cancellation_token(mut self, token: CancellationToken) -> Self {
        self.cancellation_token = token;
        self
    }

    pub fn git_ref(mut self, git_ref: impl Into<String>) -> Self {
        self.git_ref = git_ref.into();
        self
    }

    pub fn parallelism(mut self, parallelism: usize) -> Self {
        self.parallelism = parallelism;
        self
    }

    pub fn ignored_parallelism(mut self, parallelism: usize) -> Self {
        self.ignored_parallelism = parallelism;
        self
    }

    pub fn channel_buffer(mut self, channel_buffer: usize) -> Self {
        self.channel_buffer = channel_buffer;
        self
    }

    pub fn working_tree_mode(mut self, mode: WorkingTreeMode) -> Self {
        self.working_tree = mode;
        self
    }

    pub fn ignored_files_mode(mut self, mode: IgnoredFilesMode) -> Self {
        self.ignored_files = mode;
        self
    }

    /// Set the worktree creation strategy.
    ///
    /// - `Linked` (default): `git worktree add --no-checkout` + parallel
    ///   CoW file copy + index finalization. Fastest on large repos.
    /// - `Standalone`: Independent `.git/` copy (CoW'd). Can be promoted
    ///   to replace the source via `rename()`.
    /// - `GitCheckout`: Plain `git worktree add` with full checkout. Simpler,
    ///   avoids split-index issues, but single-threaded checkout.
    pub fn creation_mode(mut self, mode: CreationMode) -> Self {
        self.creation_mode = mode;
        self
    }

    /// Set the worktree kind for metadata tracking.
    /// When set, `create()` auto-registers the worktree in the metadata DB.
    #[cfg(feature = "metadata")]
    pub fn worktree_kind(mut self, kind: crate::db::WorktreeKind) -> Self {
        self.worktree_kind = Some(kind);
        self
    }

    /// Set the session ID associated with this worktree.
    #[cfg(feature = "metadata")]
    pub fn session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Override the worktree ID (default: derived from dest path).
    #[cfg(feature = "metadata")]
    pub fn worktree_id(mut self, id: impl Into<String>) -> Self {
        self.worktree_id = Some(id.into());
        self
    }

    /// Set arbitrary metadata to store alongside the worktree record.
    #[cfg(feature = "metadata")]
    pub fn metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Shorthand for `.creation_mode(CreationMode::Standalone)`.
    pub fn standalone(mut self, standalone: bool) -> Self {
        if standalone {
            self.creation_mode = CreationMode::Standalone;
        }
        self
    }

    /// Shorthand for setting the BTRFS snapshot mode (Linux only).
    ///
    /// BTRFS snapshots are automatically used by `Linked` and `Standalone`
    /// modes when the source is on a BTRFS subvolume. This method is only
    /// needed to *force* or *disable* that auto-detection.
    pub fn btrfs_mode(self, mode: BtrfsMode) -> Self {
        // BtrfsMode is now handled inside execute.rs based on CreationMode.
        // This method is kept for backward compatibility with the CLI.
        tracing::warn!(
            ?mode,
            "WorktreeBuilder::btrfs_mode() is deprecated and has no effect. \
             BtrfsMode is now handled automatically based on CreationMode."
        );
        self
    }

    /// Set a delegate for privileged btrfs operations.
    ///
    /// When the caller lacks `CAP_SYS_ADMIN` (e.g., inside a bwrap sandbox),
    /// btrfs snapshot creation/deletion can be delegated to a privileged
    /// process via this trait. The delegate is tried as a fallback when
    /// direct btrfs operations fail or are unavailable.
    pub fn btrfs_delegate(mut self, delegate: Arc<dyn BtrfsDelegate>) -> Self {
        self.btrfs_delegate = Some(delegate);
        self
    }

    /// Create the worktree using the configured options.
    ///
    /// This is a **blocking** operation. Callers should use `spawn_blocking`
    /// when calling from async contexts.
    pub fn create(self) -> Result<WorktreeReport> {
        // Clone source/git_ref/creation_mode for DB registration before the move
        // into WorktreePlan. These are one-per-create, not a hot path.
        #[cfg(feature = "metadata")]
        let meta_fields = (
            self.worktree_kind,
            self.session_id,
            self.worktree_id,
            self.source.clone(),
            self.creation_mode.as_db_str(),
            self.git_ref.clone(),
            self.metadata,
        );

        let plan = crate::worktree::WorktreePlan {
            source: self.source,
            dest: self.dest,
            git_ref: self.git_ref,
            parallelism: self.parallelism,
            channel_buffer: self.channel_buffer,
            working_tree: self.working_tree,
            ignored_files: self.ignored_files,
            ignored_parallelism: self.ignored_parallelism,
            creation_mode: self.creation_mode,
            cancellation_token: self.cancellation_token,
            btrfs_delegate: self.btrfs_delegate,
        };

        let result = crate::worktree::execute_plan(plan).map_err(annotate_disk_full)?;

        #[cfg(feature = "metadata")]
        {
            let (kind, session_id, wt_id, source, creation_mode, git_ref, metadata) = meta_fields;
            if let Some(kind) = kind {
                register_worktree(
                    &result.worktree_path,
                    &source,
                    kind,
                    creation_mode,
                    &git_ref,
                    &result.commit,
                    session_id,
                    wt_id,
                    metadata,
                );
            }
        }

        let mut unignored_copy: CopyReport = result.copy_stats.into();
        unignored_copy.dirty_files = result.dirty_files_report;

        Ok(WorktreeReport {
            worktree_path: result.worktree_path,
            commit: result.commit,
            unignored_copy,
            ignored_copy: result.ignored_stats.map(Into::into),
        })
    }

    /// Copy ONLY `.gitignore`'d (ignored) files from `source` to `dest`.
    ///
    /// This does **not** create or finalize a worktree. It's intended to be run after a
    /// worktree already exists at `dest`, to populate ignored artifacts (node_modules, target, etc.).
    ///
    /// This is a **blocking** operation. Callers should use `spawn_blocking`
    /// when calling from async contexts.
    pub fn copy_ignored_only(self) -> Result<CopyReport> {
        let source = &self.source;
        let dest = &self.dest;

        let num_workers = if self.ignored_parallelism != 0 {
            self.ignored_parallelism
        } else if self.parallelism != 0 {
            self.parallelism
        } else {
            num_cpus::get()
        };

        let skip_patterns = match self.ignored_files {
            IgnoredFilesMode::Skip => vec![],
            IgnoredFilesMode::Copy { skip_patterns } => skip_patterns,
            IgnoredFilesMode::CopyOnly { skip_patterns } => skip_patterns,
        };

        tracing::info!(
            source = %source.display(),
            dest = %dest.display(),
            parallelism = num_workers,
            channel_buffer = self.channel_buffer,
            "copying ignored files (ignored-only)"
        );

        let start = std::time::Instant::now();
        let unignored_paths = crate::copy::collect_unignored_paths(source, num_workers)?;

        let copy_config = ParallelCopyConfig {
            num_workers,
            channel_buffer: self.channel_buffer,
            skip_files: Some(Arc::new(unignored_paths)),
            respect_gitignore: false,
            skip_patterns,
        };

        let copy_result =
            crate::copy::copy_parallel(source, dest, copy_config, self.cancellation_token.clone())?;

        // `copy_parallel` returns Ok with partial stats on cancellation; surface
        // it so an interrupted copy isn't treated as success.
        if self.cancellation_token.is_cancelled() {
            anyhow::bail!("cancelled during ignored-only copy");
        }

        tracing::debug!(
            elapsed = ?start.elapsed(),
            files = copy_result.stats.files_copied,
            dirs = copy_result.stats.dirs_created,
            symlinks = copy_result.stats.symlinks_copied,
            skipped = copy_result.stats.files_skipped,
            "copying ignored files (ignored-only) complete"
        );

        Ok(copy_result.stats.into())
    }
}

/// Error context attached when worktree creation fails on a full disk. The
/// pager matches on it, so this constant is the cross-crate contract.
pub const OUT_OF_DISK_CONTEXT: &str = "not enough free disk space";

/// POSIX disk-full text `git` prints to stderr; the text fallback for the
/// typed `ErrorKind::StorageFull` check.
pub const ENOSPC_OS_MESSAGE: &str = "No space left on device";

/// Detect a disk-full failure anywhere in an error chain.
///
/// Worktree creation touches the disk in many places (reflink/copy of files
/// and the git index, directory creation, `git worktree add`). When the volume
/// fills up the underlying `std::io::Error` reports `ErrorKind::StorageFull` —
/// std maps `ENOSPC` (Linux/macOS) and `ERROR_DISK_FULL` /
/// `ERROR_HANDLE_DISK_FULL` (Windows) onto it, so this is correct on every
/// platform. `git` subcommands instead surface the failure only as stderr text.
fn is_out_of_disk(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        if let Some(io) = cause.downcast_ref::<std::io::Error>()
            && io.kind() == std::io::ErrorKind::StorageFull
        {
            return true;
        }
        // Fallback for `git` subcommands, which report this only as stderr text.
        cause.to_string().contains(ENOSPC_OS_MESSAGE)
    })
}

/// Promote a disk-full reason to the top of the error chain.
///
/// Downstream layers (the workspace hub, ACP) flatten the `anyhow` chain to its
/// top-level message via `Display`, discarding the root `io::Error`. Without
/// this, a full disk surfaces to the user as an opaque
/// `"failed to copy index from … to …"`. Promoting the reason to the outermost
/// context ensures it survives that flattening; the original chain is preserved
/// underneath for logs (`{:#}` / `{:?}`).
fn annotate_disk_full(err: anyhow::Error) -> anyhow::Error {
    if is_out_of_disk(&err) {
        err.context(OUT_OF_DISK_CONTEXT)
    } else {
        err
    }
}

/// Result of removing a worktree.
#[derive(Clone, Debug)]
pub struct RemoveReport {
    /// Whether a btrfs subvolume delete was used (O(1)) vs git worktree remove (O(n)).
    pub used_btrfs_delete: bool,
    /// Whether a bind mount was unmounted before deletion.
    pub unmounted_bind: bool,
    /// Whether an overlay mount was unmounted before deletion.
    pub unmounted_overlay: bool,
}

/// Remove a worktree, using the fastest available method.
///
/// Detection order:
/// 1. If the worktree is a symlink/bind-mount to a btrfs snapshot, or a direct btrfs subvolume → unmount if needed + `btrfs subvolume delete` (O(1))
/// 2. Otherwise → `rm -rf` + deregister from `.git/worktrees/`
///
/// **Why not `git worktree remove --force`?** On large repos (100K+ files),
/// `git worktree remove` walks all files to delete them (often tens of seconds).
/// Using `rm -rf` + deregistration is ~10x faster because the kernel handles
/// bulk deletion more efficiently, and we avoid git's per-file validation.
///
/// This is a **blocking** operation. Callers should use `spawn_blocking`
/// when calling from async contexts.
pub fn remove_worktree(worktree_path: &std::path::Path) -> Result<RemoveReport> {
    remove_worktree_inner(worktree_path, None)
}

/// Remove a worktree with an optional delegate for privileged btrfs operations.
///
/// When the caller has a `BtrfsDelegate` (e.g., from a sandbox with IPC to a
/// privileged helper), this function uses it as a fallback when direct btrfs
/// operations fail (e.g., due to missing `CAP_SYS_ADMIN`).
pub fn remove_worktree_with_delegate(
    worktree_path: &std::path::Path,
    delegate: Option<Arc<dyn BtrfsDelegate>>,
) -> Result<RemoveReport> {
    remove_worktree_inner(worktree_path, delegate.as_ref())
}

fn remove_worktree_inner(
    worktree_path: &std::path::Path,
    delegate: Option<&Arc<dyn BtrfsDelegate>>,
) -> Result<RemoveReport> {
    let report = remove_worktree_from_disk(worktree_path, delegate)?;

    // Unregister only AFTER a successful on-disk removal: a failed removal (e.g.
    // EPERM on btrfs delete) must keep the record so the worktree stays tracked
    // by list/gc instead of leaking untracked on disk.
    #[cfg(feature = "metadata")]
    unregister_worktree(worktree_path);

    Ok(report)
}

/// Remove the worktree from disk (overlay/btrfs/metadata fast paths or `rm -rf`
/// + deregister), without touching the metadata DB. Returns `Err` if the on-disk
/// removal fails, so the caller can keep the DB record.
fn remove_worktree_from_disk(
    worktree_path: &std::path::Path,
    delegate: Option<&Arc<dyn BtrfsDelegate>>,
) -> Result<RemoveReport> {
    use anyhow::Context;

    #[cfg(not(target_os = "linux"))]
    let _ = delegate;

    // Try overlay removal first (Linux only) — unmount overlay + delete btrfs snapshot
    #[cfg(target_os = "linux")]
    {
        if let Some(report) = try_overlay_remove(worktree_path, delegate)? {
            return Ok(report);
        }
    }

    // Try btrfs metadata-based removal (crash recovery)
    #[cfg(target_os = "linux")]
    {
        if let Some(report) = try_btrfs_remove_from_metadata(worktree_path, delegate)? {
            return Ok(report);
        }
    }

    // Try btrfs fast path (Linux only)
    #[cfg(target_os = "linux")]
    {
        if let Some(report) = try_btrfs_remove(worktree_path, delegate)? {
            return Ok(report);
        }
    }

    // Fast path: rm -rf the worktree directory, then deregister from .git/worktrees/.
    // This is ~10x faster than `git worktree remove --force` on large repos.
    tracing::debug!(
        path = %worktree_path.display(),
        "removing worktree via rm -rf + deregister"
    );

    // Read the worktree's .git file to find the registration dir BEFORE deleting.
    // Linked worktrees have `.git` as a file containing `gitdir: /path/to/.git/worktrees/<name>`.
    let registration_dir = read_worktree_gitdir(worktree_path);

    // symlink_metadata, not `exists()` (which follows the link): a worktree
    // exposed as a symlink — including a now-dangling one — must be unlinked, not
    // skipped. (On Linux, symlinks are normally handled earlier in try_btrfs_remove.)
    match std::fs::symlink_metadata(worktree_path) {
        Ok(md) if md.file_type().is_symlink() => {
            std::fs::remove_file(worktree_path).context(format!(
                "failed to remove worktree symlink: {}",
                worktree_path.display()
            ))?;
        }
        Ok(_) => {
            std::fs::remove_dir_all(worktree_path).context(format!(
                "failed to remove worktree directory: {}",
                worktree_path.display()
            ))?;
        }
        Err(_) => {} // nothing at the path
    }

    // Deregister: remove the `.git/worktrees/<name>/` directory.
    // This is what `git worktree remove` does after deleting the working tree.
    if let Some(reg_dir) = registration_dir
        && reg_dir.exists()
    {
        tracing::debug!(
            registration_dir = %reg_dir.display(),
            "removing worktree registration from .git/worktrees/"
        );
        let _ = std::fs::remove_dir_all(&reg_dir);
    }

    Ok(RemoveReport {
        used_btrfs_delete: false,
        unmounted_bind: false,
        unmounted_overlay: false,
    })
}

/// Report from cleaning up multiple worktrees.
#[derive(Debug, Default)]
pub struct CleanupReport {
    /// Number of worktrees successfully removed.
    pub removed: u64,
    /// Number of overlay mounts unmounted.
    pub overlays_unmounted: u64,
    /// Number of btrfs subvolumes deleted.
    pub btrfs_deleted: u64,
    /// Number of errors encountered (worktrees that couldn't be removed).
    pub errors: u64,
}

/// Remove all worktrees under a directory.
///
/// Scans the given directory for subdirectories (one or two levels deep to
/// handle `~/.qidi/worktrees/<repo>/<session>/`) and calls `remove_worktree()`
/// on each. Useful during session teardown to clean up all session worktrees.
///
/// This is a **blocking** operation.
pub fn cleanup_worktrees_in(dir: &std::path::Path) -> CleanupReport {
    cleanup_worktrees_in_with_delegate(dir, None)
}

/// Remove all worktrees under a directory, using an optional delegate for
/// privileged btrfs operations.
///
/// Like `cleanup_worktrees_in`, but forwards the delegate to each
/// `remove_worktree_with_delegate` call so that rootless hosts can clean up
/// btrfs snapshots via a privileged helper.
pub fn cleanup_worktrees_in_with_delegate(
    dir: &std::path::Path,
    delegate: Option<Arc<dyn BtrfsDelegate>>,
) -> CleanupReport {
    let mut report = CleanupReport::default();

    let Ok(entries) = std::fs::read_dir(dir) else {
        tracing::debug!(dir = %dir.display(), "cleanup: directory not readable");
        return report;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        // symlink_metadata so a symlink-exposed worktree (btrfs snapshot layout),
        // including a now-dangling one, is handled — `is_dir()` follows the link
        // and returns false for a broken symlink, leaking it.
        let Ok(md) = path.symlink_metadata() else {
            continue;
        };
        if md.file_type().is_symlink() {
            // remove_worktree handles the snapshot delete + symlink unlink.
            cleanup_single_worktree(&path, delegate.as_ref(), &mut report);
            continue;
        }
        if !md.is_dir() {
            continue;
        }

        let has_git = path.join(".git").exists();

        if has_git {
            cleanup_single_worktree(&path, delegate.as_ref(), &mut report);
        } else {
            if let Ok(sub_entries) = std::fs::read_dir(&path) {
                for sub_entry in sub_entries.flatten() {
                    let sub_path = sub_entry.path();
                    if let Ok(sub_md) = sub_path.symlink_metadata()
                        && (sub_md.file_type().is_symlink() || sub_md.is_dir())
                    {
                        cleanup_single_worktree(&sub_path, delegate.as_ref(), &mut report);
                    }
                }
            }
            let _ = std::fs::remove_dir(&path);
        }
    }

    tracing::info!(
        dir = %dir.display(),
        removed = report.removed,
        overlays = report.overlays_unmounted,
        btrfs = report.btrfs_deleted,
        errors = report.errors,
        "worktree cleanup complete"
    );

    report
}

/// Remove a single worktree and update the report.
fn cleanup_single_worktree(
    path: &std::path::Path,
    delegate: Option<&Arc<dyn BtrfsDelegate>>,
    report: &mut CleanupReport,
) {
    match remove_worktree_inner(path, delegate) {
        Ok(r) => {
            report.removed += 1;
            if r.unmounted_overlay {
                report.overlays_unmounted += 1;
            }
            if r.used_btrfs_delete {
                report.btrfs_deleted += 1;
            }
        }
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "failed to clean up worktree"
            );
            report.errors += 1;
        }
    }
}

/// Scan known overlay roots under `/local/repo-fuse-*/worktrees/` for orphaned
/// overlay snapshots.
///
/// An overlay snapshot is orphaned if its metadata file exists but the
/// `mount_target` doesn't exist or isn't mounted. For each orphan: delete
/// the btrfs snapshot, remove the work dir, and clean up metadata.
///
/// Intended for host startup / periodic cleanup of leftovers from unclean
/// exits.
///
/// This is a **blocking** operation.
#[cfg(target_os = "linux")]
pub fn cleanup_orphaned_overlay_snapshots() -> CleanupReport {
    crate::overlay::cleanup_orphaned_overlay_snapshots()
}

/// Try to remove an overlay worktree.
/// Returns `Ok(Some(report))` if overlay was detected and removed, `Ok(None)` to fall back.
#[cfg(target_os = "linux")]
fn try_overlay_remove(
    worktree_path: &std::path::Path,
    delegate: Option<&Arc<dyn BtrfsDelegate>>,
) -> Result<Option<RemoveReport>> {
    use crate::overlay;

    // Method 1: Check live mountinfo
    if let Some(report) = overlay::try_remove_from_mountinfo(worktree_path, delegate)? {
        return Ok(Some(report));
    }

    // Method 2: Check persisted metadata (crash recovery)
    if let Some(report) = overlay::try_remove_from_metadata(worktree_path, delegate)? {
        return Ok(Some(report));
    }

    Ok(None)
}

/// Read the `gitdir:` pointer from a linked worktree's `.git` file.
///
/// Linked worktrees have `.git` as a plain file containing:
/// ```text
/// gitdir: /path/to/main-repo/.git/worktrees/<name>
/// ```
///
/// Returns the resolved path to the registration directory, or `None`
/// if the worktree doesn't have a `.git` file (standalone repo or missing).
fn read_worktree_gitdir(worktree_path: &std::path::Path) -> Option<std::path::PathBuf> {
    let git_file = worktree_path.join(".git");
    let content = std::fs::read_to_string(&git_file).ok()?;
    let gitdir = content.trim().strip_prefix("gitdir: ")?;
    let path = std::path::Path::new(gitdir);
    // Resolve relative paths against the worktree directory
    let resolved = if path.is_relative() {
        worktree_path.join(path)
    } else {
        path.to_path_buf()
    };
    // Canonicalize to clean up any `..` components
    dunce::canonicalize(&resolved).ok().or(Some(resolved))
}

/// Delete `snapshot_path`, falling back to the delegate's `delete_snapshot`
/// (keyed by `worktree_path`) when the direct btrfs delete fails — e.g. EPERM on
/// a rootless host (no `CAP_SYS_ADMIN`) where only a privileged helper can run
/// `btrfs subvolume delete`.
///
/// `Some` means the delegate handled it; `None` means the direct delete succeeded
/// and the caller still owns local cleanup.
#[cfg(target_os = "linux")]
fn delete_snapshot_with_delegate_fallback(
    snapshot_path: &std::path::Path,
    worktree_path: &std::path::Path,
    delegate: Option<&Arc<dyn BtrfsDelegate>>,
    delete: impl FnOnce(&std::path::Path) -> Result<()>,
) -> Result<Option<RemoveReport>> {
    let Err(e) = delete(snapshot_path) else {
        return Ok(None);
    };
    if let Some(delegate) = delegate {
        tracing::info!(
            path = %worktree_path.display(),
            "btrfs subvolume delete failed, trying delegate"
        );
        match delegate.delete_snapshot(worktree_path) {
            Ok(report) => return Ok(Some(report)),
            Err(delegate_err) => {
                tracing::warn!(error = %delegate_err, "delegate deletion also failed");
            }
        }
    }
    Err(e)
}

/// Try to remove a worktree using btrfs subvolume delete.
/// Returns `Ok(Some(report))` if btrfs was used, `Ok(None)` to fall back to git.
///
/// Handles three cases:
/// 1. **Symlinked worktree** (delegate path): `worktree_path` is a symlink to a
///    btrfs snapshot. Delete the snapshot, then remove the symlink.
/// 2. **Bind-mounted worktree**: `worktree_path` is a bind mount from a btrfs
///    snapshot. Unmount, then delete the snapshot subvolume.
/// 3. **Direct btrfs worktree**: `worktree_path` itself is the btrfs subvolume.
///    Delete it directly.
#[cfg(target_os = "linux")]
fn try_btrfs_remove(
    worktree_path: &std::path::Path,
    delegate: Option<&Arc<dyn BtrfsDelegate>>,
) -> Result<Option<RemoveReport>> {
    use crate::btrfs;
    use anyhow::Context;

    // Case 1: Symlink to a btrfs snapshot (created by the delegate path on
    // rootless hosts). Symlinks cross mount namespaces — this is the
    // counterpart to the privileged helper's symlink creation.
    if worktree_path.is_symlink() {
        let link_target = match std::fs::read_link(worktree_path) {
            Ok(t) => t,
            // Broken/unreadable symlink: unlink it so it isn't left dangling
            // (the `rm -rf` fallback follows the dead link and would miss it).
            Err(_) => {
                let _ = std::fs::remove_file(worktree_path);
                return Ok(None);
            }
        };

        let resolved = if link_target.is_relative() {
            worktree_path
                .parent()
                .unwrap_or(std::path::Path::new("/"))
                .join(&link_target)
        } else {
            link_target
        };

        if let Ok(Some(_)) = btrfs::is_btrfs_subvolume(&resolved) {
            // Refuse to follow a confused/planted symlink into deleting a
            // subvolume outside the snapshot storage (e.g. the live source repo).
            // The symlink itself is just a pointer, so removing it is always safe.
            if !btrfs::is_safe_snapshot_delete_target(&resolved) {
                tracing::warn!(
                    symlink = %worktree_path.display(),
                    target = %resolved.display(),
                    "refusing to delete subvolume outside snapshot storage; removing only the symlink"
                );
                let _ = std::fs::remove_file(worktree_path);
                return Ok(Some(RemoveReport {
                    used_btrfs_delete: false,
                    unmounted_bind: false,
                    unmounted_overlay: false,
                }));
            }

            tracing::info!(
                symlink = %worktree_path.display(),
                target = %resolved.display(),
                "removing symlinked btrfs worktree"
            );

            // Delete snapshot first — if this fails, the symlink still
            // references it so cleanup can be retried.
            //
            // Known residual TOCTOU: validation `lstat`s/canonicalizes then we
            // delete by path (the `btrfs subvolume delete` CLI takes a path, not
            // an fd, so there is no `unlinkat` to close the window). Bounded by:
            // `btrfs` refuses non-subvolumes, the snapshot dir is grok-owned, and
            // `..`/symlink targets are already rejected. Accepted as-is.
            if let Some(report) = delete_snapshot_with_delegate_fallback(
                &resolved,
                worktree_path,
                delegate,
                btrfs::delete_snapshot,
            )? {
                return Ok(Some(report));
            }
            btrfs::remove_btrfs_metadata(&resolved);
            let _ = std::fs::remove_file(worktree_path);

            return Ok(Some(RemoveReport {
                used_btrfs_delete: true,
                unmounted_bind: false,
                unmounted_overlay: false,
            }));
        }

        // Symlink to non-btrfs target — remove symlink, fall through.
        let _ = std::fs::remove_file(worktree_path);
    }

    // Case 2 & 3: Check if the worktree path is a btrfs subvolume.
    let btrfs_info = match btrfs::is_btrfs_subvolume(worktree_path) {
        Ok(Some(info)) => info,
        Ok(None) => return Ok(None), // Not a btrfs subvolume, fall back
        Err(e) => {
            tracing::debug!(
                path = %worktree_path.display(),
                error = %e,
                "btrfs detection failed, falling back to git worktree remove"
            );
            return Ok(None);
        }
    };

    tracing::info!(
        path = %worktree_path.display(),
        bind_mount = ?btrfs_info.bind_mount_source,
        "removing worktree via btrfs subvolume delete (O(1))"
    );

    let mut unmounted_bind = false;

    // Case 2: Legacy bind mount — unmount first, then delete snapshot.
    if btrfs_info.bind_mount_source.is_some() {
        let mut umount_cmd = std::process::Command::new("umount");
        cf_tty_utils::detach_std_command(&mut umount_cmd);
        umount_cmd.stdin(std::process::Stdio::null());
        let output = umount_cmd
            .arg(worktree_path)
            .output()
            .context("failed to execute umount")?;

        if output.status.success() {
            unmounted_bind = true;
            let _ = std::fs::remove_dir(worktree_path);
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(
                path = %worktree_path.display(),
                stderr = %stderr.trim(),
                "umount failed, attempting direct snapshot deletion"
            );
            // Don't return — proceed to delete the snapshot directly.
            // The mount point may be stale after an unclean host restart.
        }
    }

    // Delete the btrfs subvolume (the actual snapshot)
    let snapshot_path = btrfs_info
        .bind_mount_source
        .as_deref()
        .unwrap_or(worktree_path);

    // Reuse the hardened `btrfs::delete_snapshot` (OsStr args, no lossy
    // `.`-default) rather than re-spawning the command inline.
    if let Some(report) = delete_snapshot_with_delegate_fallback(
        snapshot_path,
        worktree_path,
        delegate,
        btrfs::delete_snapshot,
    )? {
        return Ok(Some(report));
    }

    btrfs::remove_btrfs_metadata(snapshot_path);

    tracing::info!(
        path = %worktree_path.display(),
        "btrfs subvolume deleted successfully"
    );

    Ok(Some(RemoveReport {
        used_btrfs_delete: true,
        unmounted_bind,
        unmounted_overlay: false,
    }))
}

/// Try to remove via persisted btrfs snapshot metadata (crash recovery).
///
/// Scans btrfs mount points for `*.btrfs-meta.json` files whose
/// `mount_target` matches `target`. Works even after the bind mount is gone.
#[cfg(target_os = "linux")]
fn try_btrfs_remove_from_metadata(
    target: &std::path::Path,
    delegate: Option<&Arc<dyn BtrfsDelegate>>,
) -> Result<Option<RemoveReport>> {
    let mount_entries = match crate::mount_info::parse_mountinfo() {
        Ok(e) => e,
        Err(_) => return Ok(None),
    };

    try_btrfs_remove_from_metadata_inner(target, &mount_entries, delegate)
}

#[cfg(target_os = "linux")]
fn try_btrfs_remove_from_metadata_inner(
    target: &std::path::Path,
    mount_entries: &[crate::mount_info::MountEntry],
    delegate: Option<&Arc<dyn BtrfsDelegate>>,
) -> Result<Option<RemoveReport>> {
    use crate::btrfs;

    for entry in mount_entries {
        if entry.fs_type != "btrfs" {
            continue;
        }

        for subdir in btrfs::BTRFS_SNAPSHOT_SUBDIRS {
            let dir = entry.mount_point.join(subdir);
            let Ok(dir_entries) = std::fs::read_dir(&dir) else {
                continue;
            };

            for dir_entry in dir_entries.flatten() {
                let name = dir_entry.file_name();
                let name_str = name.to_string_lossy();
                if !name_str.ends_with(btrfs::BTRFS_META_SUFFIX) {
                    continue;
                }

                let meta_path = dir_entry.path();
                let Ok(content) = std::fs::read_to_string(&meta_path) else {
                    continue;
                };
                let Ok(meta) = serde_json::from_str::<btrfs::BtrfsSnapshotMetadata>(&content)
                else {
                    continue;
                };

                if meta.mount_target != target {
                    continue;
                }

                tracing::info!(
                    target = %target.display(),
                    snapshot = %meta.snapshot_path.display(),
                    "found btrfs snapshot metadata for worktree"
                );

                // `meta.snapshot_path` comes from an attacker-controllable
                // metadata file. Only delete it when it is a contained snapshot
                // subvolume located directly inside the directory we scanned.
                let snapshot_contained = meta.snapshot_path.parent() == Some(dir.as_path())
                    && btrfs::is_safe_snapshot_delete_target(&meta.snapshot_path);

                let target_is_symlink = target.is_symlink();
                let mut unmounted = false;

                // A legacy bind-mount directory must be unmounted before its
                // snapshot subvolume can be deleted; a symlink needs no umount.
                if !target_is_symlink {
                    let mut umount_cmd = std::process::Command::new("umount");
                    cf_tty_utils::detach_std_command(&mut umount_cmd);
                    umount_cmd.stdin(std::process::Stdio::null());
                    if let Ok(output) = umount_cmd.arg(target).output() {
                        unmounted = output.status.success();
                    }
                }

                // Delete the snapshot BEFORE removing the worktree reference, so
                // the link/dir still points at it if deletion fails (retriable) —
                // consistent with `try_btrfs_remove` Case 1.
                let mut deleted = false;
                let mut refused = false;
                if meta.snapshot_path.exists() {
                    if snapshot_contained {
                        if let Err(e) = btrfs::delete_snapshot(&meta.snapshot_path) {
                            // Try delegate fallback for sandboxed/rootless setups.
                            if let Some(delegate) = delegate {
                                tracing::info!(
                                    path = %meta.snapshot_path.display(),
                                    "btrfs delete failed in metadata path, trying delegate"
                                );
                                match delegate.delete_snapshot(target) {
                                    Ok(report) => return Ok(Some(report)),
                                    Err(delegate_err) => {
                                        tracing::warn!(
                                            error = %delegate_err,
                                            "delegate deletion also failed in metadata path"
                                        );
                                    }
                                }
                            }
                            return Err(e);
                        }
                        deleted = true;
                    } else {
                        refused = true;
                        tracing::warn!(
                            snapshot = %meta.snapshot_path.display(),
                            dir = %dir.display(),
                            "refusing to delete btrfs snapshot referenced by metadata: \
                             path is outside the scanned snapshot storage; preserving metadata"
                        );
                    }
                }

                // Remove the worktree reference (symlink file or empty dir). The
                // pointer is always safe to drop regardless of the refusal above.
                if target_is_symlink {
                    let _ = std::fs::remove_file(target);
                } else {
                    let _ = std::fs::remove_dir(target);
                }

                // Discard the metadata only when we handled the snapshot (deleted
                // it, or it was already gone). On refusal, keep it so the orphan
                // scanner can retry / it can be inspected.
                if !refused {
                    let _ = std::fs::remove_file(&meta_path);
                }

                return Ok(Some(RemoveReport {
                    used_btrfs_delete: deleted,
                    unmounted_bind: unmounted,
                    unmounted_overlay: false,
                }));
            }
        }
    }

    Ok(None)
}

/// Scan btrfs mount points for orphaned direct btrfs snapshots.
///
/// A btrfs snapshot is orphaned if its metadata file exists but the
/// `mount_target` is not an active mount point. For each orphan: unmount
/// stale target, delete the btrfs snapshot, and remove metadata.
///
/// This is the btrfs counterpart to `cleanup_orphaned_overlay_snapshots()`.
#[cfg(target_os = "linux")]
pub fn cleanup_orphaned_btrfs_snapshots() -> CleanupReport {
    let mount_entries = match crate::mount_info::parse_mountinfo() {
        Ok(e) => e,
        Err(_) => return CleanupReport::default(),
    };

    cleanup_orphaned_btrfs_snapshots_inner(&mount_entries)
}

/// Whether the symlink at `link` resolves to `target`.
///
/// Returns `false` when `link` is not a symlink or cannot be read. Used to
/// recognize a **live** symlink worktree (current layout) whose `mount_target`
/// never appears in mountinfo, so the orphan scanner does not destroy it.
#[cfg(target_os = "linux")]
fn symlink_resolves_to(link: &std::path::Path, target: &std::path::Path) -> bool {
    if !link.is_symlink() {
        return false;
    }
    match std::fs::read_link(link) {
        Ok(t) if t.is_relative() => {
            link.parent().unwrap_or(std::path::Path::new("/")).join(t) == target
        }
        Ok(t) => t == target,
        Err(_) => false,
    }
}

#[cfg(target_os = "linux")]
fn cleanup_orphaned_btrfs_snapshots_inner(
    mount_entries: &[crate::mount_info::MountEntry],
) -> CleanupReport {
    use crate::btrfs;

    let mut report = CleanupReport::default();

    for mount_point in mount_entries
        .iter()
        .filter(|e| e.fs_type == "btrfs")
        .map(|e| &e.mount_point)
    {
        for subdir in btrfs::BTRFS_SNAPSHOT_SUBDIRS {
            let dir = mount_point.join(subdir);
            let Ok(dir_entries) = std::fs::read_dir(&dir) else {
                continue;
            };

            for dir_entry in dir_entries.flatten() {
                let name = dir_entry.file_name();
                let name_str = name.to_string_lossy();
                if !name_str.ends_with(btrfs::BTRFS_META_SUFFIX) {
                    continue;
                }

                let meta_path = dir_entry.path();
                let Ok(content) = std::fs::read_to_string(&meta_path) else {
                    let _ = std::fs::remove_file(&meta_path);
                    continue;
                };
                let Ok(meta) = serde_json::from_str::<btrfs::BtrfsSnapshotMetadata>(&content)
                else {
                    let _ = std::fs::remove_file(&meta_path);
                    continue;
                };

                // A live worktree is active either as a bind mount (appears in
                // mountinfo) or as a symlink resolving to its snapshot (the
                // current layout — `mount_target` is a symlink, never in
                // mountinfo). Both must be treated as active, not orphaned.
                let is_active = mount_entries
                    .iter()
                    .any(|e| e.mount_point == meta.mount_target)
                    || symlink_resolves_to(&meta.mount_target, &meta.snapshot_path);

                if is_active {
                    tracing::debug!(
                        snapshot = %meta.snapshot_path.display(),
                        target = %meta.mount_target.display(),
                        "skipping active btrfs snapshot"
                    );
                    continue;
                }

                // If the mount_target's parent dir is missing we cannot prove the snapshot
                // is orphaned: this scanner runs before restore recreates worktree dirs, so a
                // snapshot about to be re-exposed would be wrongly destroyed. Skipping at worst
                // leaks a true orphan (reclaimed on a later cycle) — strictly safer than deleting.
                if let Some(parent) = meta.mount_target.parent()
                    && !parent.exists()
                {
                    tracing::debug!(
                        snapshot = %meta.snapshot_path.display(),
                        target = %meta.mount_target.display(),
                        "skipping btrfs snapshot: mount_target parent missing (cannot prove orphaned)"
                    );
                    continue;
                }

                // Untrusted metadata: only delete a snapshot contained directly
                // in the directory we scanned. Leave anything else (and its
                // metadata) untouched for inspection.
                let snapshot_contained = meta.snapshot_path.parent() == Some(dir.as_path())
                    && btrfs::is_safe_snapshot_delete_target(&meta.snapshot_path);
                if meta.snapshot_path.exists() && !snapshot_contained {
                    tracing::warn!(
                        snapshot = %meta.snapshot_path.display(),
                        dir = %dir.display(),
                        "refusing to delete btrfs snapshot outside scanned storage"
                    );
                    report.errors += 1;
                    continue;
                }

                tracing::info!(
                    target = %meta.mount_target.display(),
                    snapshot = %meta.snapshot_path.display(),
                    "cleaning up orphaned btrfs snapshot"
                );

                // Remove the worktree reference: a symlink is unlinked; a legacy
                // bind-mount dir is unmounted then removed.
                if meta.mount_target.is_symlink() {
                    let _ = std::fs::remove_file(&meta.mount_target);
                } else {
                    let mut umount_cmd = std::process::Command::new("umount");
                    cf_tty_utils::detach_std_command(&mut umount_cmd);
                    umount_cmd.stdin(std::process::Stdio::null());
                    let _ = umount_cmd.arg(&meta.mount_target).output();
                    let _ = std::fs::remove_dir(&meta.mount_target);
                }

                if meta.snapshot_path.exists() {
                    if let Err(e) = btrfs::delete_snapshot(&meta.snapshot_path) {
                        tracing::warn!(
                            path = %meta.snapshot_path.display(),
                            error = %e,
                            "failed to delete orphaned btrfs snapshot"
                        );
                        report.errors += 1;
                        // Preserve metadata so the orphan scanner can retry
                        // on the next cycle instead of losing track of it.
                        continue;
                    } else {
                        report.btrfs_deleted += 1;
                    }
                }

                let _ = std::fs::remove_file(&meta_path);
                report.removed += 1;
            }
        }
    }

    if report.removed > 0 || report.errors > 0 {
        tracing::info!(
            removed = report.removed,
            btrfs = report.btrfs_deleted,
            errors = report.errors,
            "orphaned btrfs snapshot cleanup complete"
        );
    }

    report
}

#[cfg(feature = "metadata")]
pub(crate) fn register_worktree(
    worktree_path: &std::path::Path,
    source: &std::path::Path,
    kind: crate::db::WorktreeKind,
    creation_mode: &str,
    git_ref: &str,
    commit: &str,
    session_id: Option<String>,
    worktree_id: Option<String>,
    metadata: Option<serde_json::Value>,
) {
    use crate::db;

    let db = match db::WorktreeDb::open_default() {
        Ok(db) => db,
        Err(e) => {
            tracing::warn!(error = %e, "failed to open worktree DB for registration");
            return;
        }
    };
    let record = db::WorktreeRecord {
        id: worktree_id.unwrap_or_else(|| db::id_from_path(worktree_path)),
        path: worktree_path.to_path_buf(),
        source_repo: source.to_path_buf(),
        repo_name: db::repo_name_from_path(source),
        kind,
        creation_mode: creation_mode.to_owned(),
        git_ref: Some(git_ref.to_owned()),
        head_commit: Some(commit.to_owned()),
        session_id,
        creator_pid: Some(std::process::id()),
        created_at: db::now_epoch_secs(),
        last_accessed_at: None,
        status: db::WorktreeStatus::Alive,
        metadata,
    };
    if let Err(e) = db.register(&record) {
        tracing::warn!(error = %e, "failed to register worktree in DB");
    }
}

#[cfg(feature = "metadata")]
fn unregister_worktree(worktree_path: &std::path::Path) {
    if let Ok(db) = crate::db::WorktreeDb::open_default() {
        let _ = db.unregister_by_path(worktree_path);
    }
}

/// Test-only `BtrfsDelegate` that returns a fixed snapshot and counts
/// `delete_snapshot` calls. Shared by the delegate-arm reclaim tests
/// (`worktree::execute`) and the gc-with-delegate tests.
#[cfg(test)]
pub(crate) struct RecordingDelegate {
    pub snapshot_path: PathBuf,
    pub worktree_path: PathBuf,
    pub deletes: Arc<std::sync::atomic::AtomicUsize>,
}

#[cfg(test)]
impl BtrfsDelegate for RecordingDelegate {
    fn create_snapshot(&self, _source: &Path, _dest: &Path) -> Result<DelegateSnapshotResult> {
        Ok(DelegateSnapshotResult {
            snapshot_path: self.snapshot_path.clone(),
            worktree_path: self.worktree_path.clone(),
            bind_mounted: false,
        })
    }

    fn delete_snapshot(&self, _worktree_path: &Path) -> Result<RemoveReport> {
        self.deletes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(RemoveReport {
            used_btrfs_delete: true,
            unmounted_bind: false,
            unmounted_overlay: false,
        })
    }
}

#[cfg(feature = "metadata")]
#[path = "api/gc.rs"]
pub mod gc;

/// Serializes tests that mutate the process-global CWD (see `cwd_test_guard`).
#[cfg(all(test, feature = "metadata"))]
pub(crate) static CWD_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Take the CWD test lock: tests that chdir (e.g. process-cwd scans) serialize
/// so they don't observe each other's working directories.
#[cfg(all(test, feature = "metadata"))]
pub(crate) fn cwd_test_guard() -> std::sync::MutexGuard<'static, ()> {
    CWD_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Restores the CWD on drop.
#[cfg(all(test, feature = "metadata"))]
pub(crate) struct CwdGuard(pub PathBuf);

#[cfg(all(test, feature = "metadata"))]
impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "metadata")]
    mod metadata_integration {
        use super::*;
        use crate::db::{ListFilter, WorktreeDb, WorktreeKind};

        fn db_at(tmp: &tempfile::TempDir) -> WorktreeDb {
            WorktreeDb::open(tmp.path()).unwrap()
        }

        #[test]
        fn register_worktree_writes_correct_fields() {
            // Isolate QIDI_HOME so register_worktree's open_default write lands
            // in our own DB (lock + private tmp + restore via the fixture).
            let fx = crate::db::GrokHomeFixture::new();

            // Unique basename → unique id, so a concurrent open_default writer
            // (QIDI_HOME is process-global) can't INSERT-OR-REPLACE our row.
            let wt_path = fx.home.join("register-fields-wt");
            std::fs::create_dir(&wt_path).unwrap();

            super::super::register_worktree(
                &wt_path,
                std::path::Path::new("/src/repo"),
                WorktreeKind::Session,
                "linked",
                "main",
                "abc123",
                Some("test-session".to_string()),
                None,
                None,
            );

            // register_worktree wrote to open_default, which resolves to fx.home.
            // Filter to OUR record by path: concurrent tests may add rows here.
            let db = WorktreeDb::open(&fx.home).unwrap();
            let mine: Vec<_> = db
                .list(&ListFilter::default())
                .unwrap()
                .into_iter()
                .filter(|r| r.path == wt_path)
                .collect();
            assert_eq!(mine.len(), 1);
            assert_eq!(mine[0].kind, WorktreeKind::Session);
            assert_eq!(mine[0].session_id.as_deref(), Some("test-session"));
            assert_eq!(mine[0].creation_mode, "linked");
            assert_eq!(mine[0].head_commit.as_deref(), Some("abc123"));
            assert!(mine[0].creator_pid.is_some());
        }

        #[test]
        fn unregister_worktree_removes_by_path() {
            let tmp = tempfile::TempDir::new().unwrap();
            let db = db_at(&tmp);
            let wt_path = tmp.path().join("wt");

            let record = crate::db::WorktreeRecord {
                id: "test-wt".to_string(),
                path: wt_path.clone(),
                source_repo: "/repo".into(),
                repo_name: "repo".to_string(),
                kind: WorktreeKind::Session,
                creation_mode: "linked".to_string(),
                git_ref: None,
                head_commit: None,
                session_id: None,
                creator_pid: None,
                created_at: 100,
                last_accessed_at: None,
                status: crate::db::WorktreeStatus::Alive,
                metadata: None,
            };
            db.register(&record).unwrap();
            assert_eq!(db.list(&ListFilter::default()).unwrap().len(), 1);

            db.unregister_by_path(&wt_path).unwrap();
            assert!(db.list(&ListFilter::default()).unwrap().is_empty());
        }

        #[test]
        fn creation_mode_as_db_str_matches_schema() {
            assert_eq!(CreationMode::Linked.as_db_str(), "linked");
            assert_eq!(CreationMode::Standalone.as_db_str(), "standalone");
            assert_eq!(CreationMode::GitCheckout.as_db_str(), "git");
        }

        #[test]
        fn gc_removes_dead_records() {
            let tmp = tempfile::TempDir::new().unwrap();
            let db = db_at(&tmp);

            // Register a record with a nonexistent path
            let record = crate::db::WorktreeRecord {
                id: "dead-1".to_string(),
                path: "/nonexistent/worktree".into(),
                source_repo: "/repo".into(),
                repo_name: "repo".to_string(),
                kind: WorktreeKind::Session,
                creation_mode: "linked".to_string(),
                git_ref: None,
                head_commit: None,
                session_id: None,
                creator_pid: None,
                created_at: 100,
                last_accessed_at: None,
                status: crate::db::WorktreeStatus::Alive,
                metadata: None,
            };
            db.register(&record).unwrap();

            let report = gc::gc_worktrees(&db, &gc::GcOptions::default()).unwrap();
            assert_eq!(report.dead_removed, 1);

            let all = db
                .list(&ListFilter {
                    include_dead: true,
                    ..Default::default()
                })
                .unwrap();
            assert!(all.is_empty());
        }

        #[test]
        fn gc_skips_alive_pids() {
            let tmp = tempfile::TempDir::new().unwrap();
            let db = db_at(&tmp);
            let my_pid = std::process::id();

            let record = crate::db::WorktreeRecord {
                id: "alive-wt".to_string(),
                path: "/nonexistent/path".into(),
                source_repo: "/repo".into(),
                repo_name: "repo".to_string(),
                kind: WorktreeKind::Session,
                creation_mode: "linked".to_string(),
                git_ref: None,
                head_commit: None,
                session_id: None,
                creator_pid: Some(my_pid),
                created_at: 1, // very old
                last_accessed_at: None,
                status: crate::db::WorktreeStatus::Alive,
                metadata: None,
            };
            db.register(&record).unwrap();

            // sweep_dead will mark it dead (path doesn't exist),
            // but gc with max_age should still check liveness for expiry.
            // Since the path doesn't exist, sweep_dead marks it dead first,
            // then dead_removed cleans it. Let's use a real existing path instead.
            // Real worktree so the gate clears and liveness is the only guard.
            let dir = crate::test_support::deletable_linked_worktree(tmp.path(), "real-wt");
            let source = tmp.path().join("gate-source");
            let mut record2 = record.clone();
            record2.id = "alive-wt2".to_string();
            record2.path = dir.clone();
            record2.source_repo = source.clone();
            db.register(&record2).unwrap();

            let report = gc::gc_worktrees(
                &db,
                &gc::GcOptions {
                    max_age_secs: Some(0), // everything is expired
                    force: false,
                    dry_run: false,
                    ..Default::default()
                },
            )
            .unwrap();

            // Our PID is alive, so the real-path worktree should be skipped
            assert_eq!(report.skipped_alive, 1);
            // The nonexistent-path one gets swept to dead then removed
            assert_eq!(report.dead_removed, 1);
        }

        #[test]
        fn gc_dry_run_preserves_records() {
            let tmp = tempfile::TempDir::new().unwrap();
            let db = db_at(&tmp);

            let record = crate::db::WorktreeRecord {
                id: "dry-1".to_string(),
                path: "/nonexistent".into(),
                source_repo: "/repo".into(),
                repo_name: "repo".to_string(),
                kind: WorktreeKind::Session,
                creation_mode: "linked".to_string(),
                git_ref: None,
                head_commit: None,
                session_id: None,
                creator_pid: None,
                created_at: 100,
                last_accessed_at: None,
                status: crate::db::WorktreeStatus::Alive,
                metadata: None,
            };
            db.register(&record).unwrap();

            let report = gc::gc_worktrees(
                &db,
                &gc::GcOptions {
                    dry_run: true,
                    ..Default::default()
                },
            )
            .unwrap();

            assert_eq!(report.dead_removed, 1); // counted as would-be-removed
            // Dry run must NOT mutate: the record is still present AND still
            // Alive (it was never swept to Dead).
            let all = db
                .list(&ListFilter {
                    include_dead: true,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(all.len(), 1);
            assert_eq!(all[0].status, crate::db::WorktreeStatus::Alive);
        }

        #[test]
        fn gc_force_overrides_liveness() {
            let tmp = tempfile::TempDir::new().unwrap();
            let db = db_at(&tmp);

            let dir = crate::test_support::deletable_linked_worktree(tmp.path(), "force-wt");
            let source = tmp.path().join("gate-source");

            let record = crate::db::WorktreeRecord {
                id: "force-1".to_string(),
                path: dir.clone(),
                source_repo: source.clone(),
                repo_name: "repo".to_string(),
                kind: WorktreeKind::Session,
                creation_mode: "linked".to_string(),
                git_ref: None,
                head_commit: None,
                session_id: None,
                creator_pid: Some(std::process::id()), // our own PID
                created_at: 1,                         // very old
                last_accessed_at: None,
                status: crate::db::WorktreeStatus::Alive,
                metadata: None,
            };
            db.register(&record).unwrap();

            let report = gc::gc_worktrees(
                &db,
                &gc::GcOptions {
                    max_age_secs: Some(0),
                    force: true,
                    dry_run: false,
                ..Default::default() },
            )
            .unwrap();

            assert_eq!(report.expired_removed, 1);
            assert_eq!(report.skipped_alive, 0);
        }

        #[test]
        fn gc_clamps_extreme_max_age_without_overflow() {
            let tmp = tempfile::TempDir::new().unwrap();
            let db = db_at(&tmp);
            let dir = tmp.path().join("fresh-wt");
            std::fs::create_dir(&dir).unwrap();
            let record = crate::db::WorktreeRecord {
                id: "fresh-1".to_string(),
                path: dir.clone(),
                source_repo: "/repo".into(),
                repo_name: "repo".to_string(),
                kind: WorktreeKind::Session,
                creation_mode: "linked".to_string(),
                git_ref: None,
                head_commit: None,
                session_id: None,
                creator_pid: None,
                created_at: i64::MAX,
                last_accessed_at: None,
                status: crate::db::WorktreeStatus::Alive,
                metadata: None,
            };
            db.register(&record).unwrap();
            // `now - i64::MIN` would overflow/wrap the cutoff into the future and
            // reclaim everything; the clamp treats any negative age as 0 so the
            // cutoff is `now` and nothing fresh is reclaimed (and no panic).
            let report = gc::gc_worktrees(
                &db,
                &gc::GcOptions {
                    max_age_secs: Some(i64::MIN),
                    force: false,
                    dry_run: false,
                ..Default::default() },
            )
            .unwrap();
            assert_eq!(report.expired_removed, 0);
            assert!(dir.exists());
        }

        #[test]
        fn gc_honors_last_accessed_time() {
            let tmp = tempfile::TempDir::new().unwrap();
            let db = db_at(&tmp);
            // Real worktrees so both clear the delete gate; the age logic is
            // what must decide between them.
            let fresh = crate::test_support::deletable_linked_worktree(tmp.path(), "fresh-access");
            let stale = crate::test_support::deletable_linked_worktree(tmp.path(), "stale-access");
            let source = tmp.path().join("gate-source");
            let base = crate::db::WorktreeRecord {
                id: String::new(),
                path: std::path::PathBuf::new(),
                source_repo: source.clone(),
                repo_name: "repo".to_string(),
                kind: WorktreeKind::Session,
                creation_mode: "linked".to_string(),
                git_ref: None,
                head_commit: None,
                session_id: None,
                creator_pid: None, // no liveness guard: isolate the age logic
                created_at: 1,     // both are old by creation time
                last_accessed_at: None,
                status: crate::db::WorktreeStatus::Alive,
                metadata: None,
            };
            db.register(&crate::db::WorktreeRecord {
                id: "fresh".to_string(),
                path: fresh.clone(),
                last_accessed_at: Some(i64::MAX), // touched within the window
                ..base.clone()
            })
            .unwrap();
            db.register(&crate::db::WorktreeRecord {
                id: "stale".to_string(),
                path: stale.clone(),
                last_accessed_at: Some(1), // never re-touched
                ..base
            })
            .unwrap();

            let report = gc::gc_worktrees(
                &db,
                &gc::GcOptions {
                    max_age_secs: Some(0),
                    force: false,
                    dry_run: false,
                ..Default::default() },
            )
            .unwrap();

            assert!(
                fresh.exists(),
                "a recently accessed worktree must survive despite an old created_at"
            );
            assert!(
                !stale.exists(),
                "a never-touched expired worktree must be reclaimed"
            );
            assert_eq!(report.expired_removed, 1);
        }

        #[cfg(target_os = "linux")]
        #[test]
        fn gc_cwd_guard_skips_then_reclaims_expired_worktree() {
            use std::time::Duration;
            let tmp = tempfile::TempDir::new().unwrap();
            let db = db_at(&tmp);
            let dir = tmp.path().join("cwd-wt");
            let nested = dir.join("nested");
            std::fs::create_dir_all(&nested).unwrap();
            let record = crate::db::WorktreeRecord {
                id: "cwd-1".to_string(),
                path: dir.clone(),
                source_repo: "/repo".into(),
                repo_name: "repo".to_string(),
                kind: WorktreeKind::Session,
                creation_mode: "linked".to_string(),
                git_ref: None,
                head_commit: None,
                session_id: None,
                creator_pid: None, // creator gone: only the CWD guard can protect it
                created_at: 1,     // very old → expired
                last_accessed_at: None,
                status: crate::db::WorktreeStatus::Alive,
                metadata: None,
            };
            db.register(&record).unwrap();

            // A live process parked inside the expired worktree subtree.
            let mut child = std::process::Command::new("sleep")
                .arg("30")
                .current_dir(&nested)
                .spawn()
                .expect("spawn sleep");
            // Wait for the kernel to reflect the child's CWD before GC scans.
            let want = dunce::canonicalize(&nested).unwrap();
            let link = format!("/proc/{}/cwd", child.id());
            for _ in 0..200 {
                if std::fs::read_link(&link).is_ok_and(|p| p.starts_with(&want)) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }

            let opts = gc::GcOptions {
                max_age_secs: Some(0),
                force: false,
                dry_run: false,
            ..Default::default() };
            let guarded = gc::gc_worktrees(&db, &opts).unwrap();
            assert_eq!(
                guarded.skipped_alive, 1,
                "a live in-tree CWD must protect the expired worktree"
            );
            assert_eq!(guarded.expired_removed, 0);
            assert!(dir.exists());

            // Once the process exits, the same expired worktree is reclaimed.
            child.kill().ok();
            child.wait().ok();
            let reclaimed = gc::gc_worktrees(&db, &opts).unwrap();
            assert_eq!(
                reclaimed.expired_removed, 1,
                "no live process inside ⇒ the expired worktree is reclaimed"
            );
            assert!(!dir.exists());
        }

        #[test]
        fn gc_dry_run_with_max_age_does_not_remove_expired() {
            // An expired worktree whose dir exists must be previewed (counted)
            // but never removed under dry_run. The fixture must be a real
            // worktree that clears the delete gate (an empty dir reads as
            // NoRepo under the gate, which is counted separately).
            let tmp = tempfile::TempDir::new().unwrap();
            let db = db_at(&tmp);

            let dir = crate::test_support::deletable_linked_worktree(tmp.path(), "expired-wt");
            let source = tmp.path().join("gate-source");

            let record = crate::db::WorktreeRecord {
                id: "expired-1".to_string(),
                path: dir.clone(),
                source_repo: source.clone(),
                repo_name: "repo".to_string(),
                kind: WorktreeKind::Session,
                creation_mode: "linked".to_string(),
                git_ref: None,
                head_commit: None,
                session_id: None,
                creator_pid: None, // no liveness guard
                created_at: 1,     // very old
                last_accessed_at: None,
                status: crate::db::WorktreeStatus::Alive,
                metadata: None,
            };
            db.register(&record).unwrap();

            let report = gc::gc_worktrees(
                &db,
                &gc::GcOptions {
                    max_age_secs: Some(0),
                    force: true,
                    dry_run: true,
                ..Default::default() },
            )
            .unwrap();

            assert_eq!(
                report.expired_removed, 1,
                "dry run should count the candidate"
            );
            // No mutation: the dir and the (still Alive) record both survive.
            assert!(dir.exists(), "dry run must not remove the worktree dir");
            let all = db.list(&ListFilter::default()).unwrap();
            assert_eq!(all.len(), 1);
            assert_eq!(all[0].status, crate::db::WorktreeStatus::Alive);
        }

        #[test]
        fn gc_dry_run_missing_and_expired_counted_once() {
            // A record that is Alive, has a MISSING path, AND is expired must be
            // counted EXACTLY once (a real run sweeps it to dead and unregisters
            // it before the expired loop). It belongs to dead_removed, not both.
            let tmp = tempfile::TempDir::new().unwrap();
            let db = db_at(&tmp);

            let record = crate::db::WorktreeRecord {
                id: "missing-expired".to_string(),
                path: "/nonexistent/expired-wt".into(),
                source_repo: "/repo".into(),
                repo_name: "repo".to_string(),
                kind: WorktreeKind::Session,
                creation_mode: "linked".to_string(),
                git_ref: None,
                head_commit: None,
                session_id: None,
                creator_pid: None,
                created_at: 1, // very old → expired
                last_accessed_at: None,
                status: crate::db::WorktreeStatus::Alive,
                metadata: None,
            };
            db.register(&record).unwrap();

            let report = gc::gc_worktrees(
                &db,
                &gc::GcOptions {
                    max_age_secs: Some(0),
                    force: true,
                    dry_run: true,
                ..Default::default() },
            )
            .unwrap();

            assert_eq!(
                report.dead_removed, 1,
                "missing path counts as would-be-dead"
            );
            assert_eq!(
                report.expired_removed, 0,
                "must not also be counted in expired_removed"
            );
        }

        #[test]
        fn gc_expired_failed_removal_keeps_record() {
            // When an expired worktree cannot be reclaimed, the record must
            // survive so a later pass can retry. Under the gate-based gc,
            // "cannot be reclaimed" is a Kept verdict (here: a dirty
            // worktree), which must neither count as removed nor unregister.
            let tmp = tempfile::TempDir::new().unwrap();
            let db = db_at(&tmp);

            let path = crate::test_support::deletable_linked_worktree(tmp.path(), "doomed-wt");
            let source = tmp.path().join("gate-source");
            // Uncommitted work makes the gate keep the worktree.
            std::fs::write(path.join("tracked.txt"), "uncommitted work\n").unwrap();

            let record = crate::db::WorktreeRecord {
                id: "doomed-1".to_string(),
                path: path.clone(),
                source_repo: source.clone(),
                repo_name: "repo".to_string(),
                kind: WorktreeKind::Session,
                creation_mode: "linked".to_string(),
                git_ref: None,
                head_commit: None,
                session_id: None,
                creator_pid: None,
                created_at: 1,
                last_accessed_at: None,
                status: crate::db::WorktreeStatus::Alive,
                metadata: None,
            };
            db.register(&record).unwrap();

            let report = gc::gc_worktrees(
                &db,
                &gc::GcOptions {
                    max_age_secs: Some(0),
                    force: true,
                    dry_run: false,
                ..Default::default() },
            )
            .unwrap();

            assert_eq!(
                report.expired_removed, 0,
                "a kept worktree must not be counted as removed"
            );
            let all = db.list(&ListFilter::default()).unwrap();
            assert_eq!(all.len(), 1, "record must survive when the gate keeps the worktree");
            assert!(path.exists(), "the kept worktree is still present");
        }

        /// True if a record with `path` exists in the DB (assert on our own
        /// record rather than total count: other tests may write to the same
        /// open_default DB concurrently).
        fn record_present(db: &WorktreeDb, path: &std::path::Path) -> bool {
            db.list(&ListFilter::default())
                .unwrap()
                .iter()
                .any(|r| r.path == path)
        }

        #[test]
        fn db_record_survives_failed_removal() {
            // remove_worktree must keep the DB record when the on-disk removal
            // fails, so the worktree isn't lost from tracking while leaking on
            // disk (unregister only after a successful removal).
            let fx = crate::db::GrokHomeFixture::new();

            // A regular file makes remove_dir_all fail (ENOTDIR) deterministically.
            let wt_path = fx.home.join("doomed-wt");
            std::fs::write(&wt_path, b"not a dir").unwrap();

            // Register via the production registration path (uses open_default).
            super::super::register_worktree(
                &wt_path,
                std::path::Path::new("/src/repo"),
                WorktreeKind::Session,
                "linked",
                "main",
                "abc123",
                None,
                None,
                None,
            );
            let db = WorktreeDb::open(&fx.home).unwrap();
            assert!(
                record_present(&db, &wt_path),
                "precondition: record registered"
            );

            assert!(
                crate::remove_worktree(&wt_path).is_err(),
                "removing a non-directory path must fail"
            );

            assert!(
                record_present(&db, &wt_path),
                "record must survive a failed removal"
            );
        }

        #[test]
        fn db_record_removed_after_successful_removal() {
            // The success direction: a removable worktree must still be
            // unregistered from the DB (catches a regression dropping the
            // unregister).
            cf_test_utils::require_git!();
            use cf_test_utils::git::{git_commit_all, init_git_repo};

            let fx = crate::db::GrokHomeFixture::new();

            // A real repo + a real worktree so remove_worktree succeeds on disk.
            let repo = fx.home.join("repo");
            std::fs::create_dir(&repo).unwrap();
            init_git_repo(&repo);
            std::fs::write(repo.join("f.txt"), "x").unwrap();
            git_commit_all(&repo, "init");
            let wt_path = fx.home.join("live-wt");
            crate::WorktreeBuilder::new(&repo, &wt_path)
                .create()
                .unwrap();

            super::super::register_worktree(
                &wt_path,
                &repo,
                WorktreeKind::Session,
                "linked",
                "main",
                "abc123",
                None,
                None,
                None,
            );
            let db = WorktreeDb::open(&fx.home).unwrap();
            assert!(
                record_present(&db, &wt_path),
                "precondition: record registered"
            );

            crate::remove_worktree(&wt_path).unwrap();

            assert!(!wt_path.exists(), "worktree dir should be gone");
            assert!(
                !record_present(&db, &wt_path),
                "a successful removal must unregister the DB record"
            );
        }

        #[test]
        fn gc_with_delegate_removes_expired_and_unregisters() {
            // gc_worktrees_with_delegate threads the delegate through the expired
            // path and, on a successful removal, counts it and drops the record.
            // (The delegate's btrfs fallback only fires on a real btrfs-delete
            // failure, which needs a btrfs host; here the plain-dir fast path
            // succeeds, so the mock's delete_snapshot is not called.)
            use std::sync::atomic::{AtomicUsize, Ordering};

            // QIDI_HOME == the gc DB dir so remove_worktree's open_default
            // unregister hits the same DB the gc record lives in.
            let fx = crate::db::GrokHomeFixture::new();
            let db = WorktreeDb::open(&fx.home).unwrap();

            // Real worktree that clears the delete gate (the gate requires a
            // git worktree; an empty dir is judged NoRepo and never removed).
            let root = fx.home.parent().unwrap().to_path_buf();
            let dir = crate::test_support::deletable_linked_worktree(&root, "expired-wt");
            let source = root.join("gate-source");
            let record = crate::db::WorktreeRecord {
                id: "expired-del-1".to_string(),
                path: dir.clone(),
                source_repo: source.clone(),
                repo_name: "repo".to_string(),
                kind: WorktreeKind::Session,
                creation_mode: "linked".to_string(),
                git_ref: None,
                head_commit: None,
                session_id: None,
                creator_pid: None,
                created_at: 1, // very old → expired
                last_accessed_at: None,
                status: crate::db::WorktreeStatus::Alive,
                metadata: None,
            };
            db.register(&record).unwrap();

            let deletes = Arc::new(AtomicUsize::new(0));
            let delegate: Arc<dyn BtrfsDelegate> = Arc::new(super::super::RecordingDelegate {
                snapshot_path: fx.home.join("unused-snap"),
                worktree_path: dir.clone(),
                deletes: Arc::clone(&deletes),
            });

            let report = gc::gc_worktrees_with_delegate(
                &db,
                &gc::GcOptions {
                    max_age_secs: Some(0),
                    force: true,
                    dry_run: false,
                ..Default::default() },
                Some(delegate),
            )
            .unwrap();

            assert_eq!(
                report.expired_removed, 1,
                "expired worktree should be reclaimed"
            );
            assert!(!dir.exists(), "the worktree dir should be removed");
            assert!(
                db.get("expired-del-1").unwrap().is_none(),
                "the DB record should be unregistered after a successful removal"
            );
            // Plain-dir fast path succeeds without needing the delegate fallback.
            assert_eq!(deletes.load(Ordering::Relaxed), 0);
        }

        #[test]
        fn gc_report_serde_round_trip() {
            let report = gc::GcReport {
                dead_removed: 3,
                expired_removed: 1,
                skipped_alive: 2,
                remove_failed: 4,
                ..Default::default()
            };
            let json = serde_json::to_string(&report).unwrap();
            let deser: gc::GcReport = serde_json::from_str(&json).unwrap();
            assert_eq!(deser.dead_removed, 3);
            assert_eq!(deser.expired_removed, 1);
            assert_eq!(deser.skipped_alive, 2);
            assert_eq!(deser.remove_failed, 4);
        }

        #[test]
        fn gc_options_serde_round_trip() {
            let opts = gc::GcOptions {
                max_age_secs: Some(86400),
                force: true,
                dry_run: false,
            ..Default::default() };
            let json = serde_json::to_string(&opts).unwrap();
            let deser: gc::GcOptions = serde_json::from_str(&json).unwrap();
            assert_eq!(deser.max_age_secs, Some(86400));
            assert!(deser.force);
            assert!(!deser.dry_run);
        }

        #[test]
        fn db_stats_serde_round_trip() {
            let stats = crate::db::DbStats {
                total_records: 10,
                alive_count: 7,
                dead_count: 3,
                db_file_bytes: 4096,
            };
            let json = serde_json::to_string(&stats).unwrap();
            let deser: crate::db::DbStats = serde_json::from_str(&json).unwrap();
            assert_eq!(deser.total_records, 10);
            assert_eq!(deser.alive_count, 7);
            assert_eq!(deser.dead_count, 3);
            assert_eq!(deser.db_file_bytes, 4096);
        }
    }
}
