//! `deploy_skill` — the sanctioned channel for installing a user skill.
//!
//! The agent stages a skill (a `SKILL.md` plus optional `scripts/`, tests, …)
//! in a scratch directory and calls this tool to install it into
//! `<grok_home>/skills/user-<name>/`. Direct `Edit`-class writes into the
//! skills tree are separately gated to `Ask` by the evolution permission rule
//! (see `cf-workspace`'s evolution module), so this tool is the only supported
//! path — and the only one that emits a structured deployment audit event.
//!
//! ## Bounds
//!
//! Aligned with the bounded archive-extraction spirit of
//! `cf-shell/src/bundle.rs`: at most [`MAX_DEPLOY_FILES`] files, each at most
//! [`MAX_DEPLOY_FILE_BYTES`] bytes, with a total of at most
//! [`MAX_DEPLOY_TOTAL_BYTES`] bytes.
//!
//! ## Phase 1 approval model
//!
//! The caller must supply a non-empty `approval_note` (free text). It is a
//! *declarative* record written into the session event log; it is **not** a
//! cryptographic token. The actual approval gate is the path-level `Ask`
//! permission rule, which forces a user prompt before the tool ever runs.
//! (Signed tokens are Phase 2.)

use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cf_tool_protocol::ToolId;
use cf_tool_runtime::{Tool, ToolCallContext, ToolCapabilities, ToolError, ToolScope};

use crate::types::output::ToolOutput;
use crate::types::resources::SessionFolder;
use crate::types::tool::{ToolKind, ToolNamespace};

/// Registered name of the `deploy_skill` tool.
pub const DEPLOY_SKILL_TOOL_NAME: &str = "deploy_skill";

/// Maximum number of files a single deploy may copy.
pub const MAX_DEPLOY_FILES: usize = 200;
/// Maximum size of a single deployed file: 1 MiB.
pub const MAX_DEPLOY_FILE_BYTES: u64 = 1024 * 1024;
/// Maximum total size of a deploy: 10 MiB.
pub const MAX_DEPLOY_TOTAL_BYTES: u64 = 10 * 1024 * 1024;
/// Maximum skill name length, aligned with skill discovery's `MAX_NAME_LEN`.
pub const MAX_SKILL_NAME_LEN: usize = 64;

/// Input for the `deploy_skill` tool.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct DeploySkillInput {
    /// Directory holding the staged skill. Must exist and contain `SKILL.md`.
    pub staging_dir: String,
    /// Skill name. Must match `^[a-z0-9][a-z0-9-]{0,63}$`.
    pub name: String,
    /// Overwrite an existing skill whose content differs. The existing
    /// directory is backed up first. Defaults to `false`.
    #[serde(default)]
    pub force: bool,
    /// Explicit declaration of human approval for this deployment. Required
    /// and non-empty. Phase 1: a declarative audit record only — the enforced
    /// gate is the path-level `Ask` permission rule, not this string.
    pub approval_note: String,
}

/// Successful deployment summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployReport {
    /// Final install directory (`<grok_home>/skills/user-<name>/`).
    pub target_dir: PathBuf,
    /// Number of files copied (or already present, when idempotent).
    pub file_count: usize,
    /// Where the previous content was moved, when an existing skill was
    /// replaced.
    pub backup_dir: Option<PathBuf>,
    /// True when an identical skill was already installed (no writes).
    pub idempotent: bool,
}

impl DeployReport {
    /// Model-facing deployment summary.
    pub fn summary(&self) -> String {
        if self.idempotent {
            format!(
                "Skill already up to date at {} ({} files); no changes made.",
                self.target_dir.display(),
                self.file_count
            )
        } else {
            let mut text = format!(
                "Deployed skill to {} ({} files).",
                self.target_dir.display(),
                self.file_count
            );
            if let Some(backup) = &self.backup_dir {
                text.push_str(&format!(
                    " Previous content backed up to {}.",
                    backup.display()
                ));
            }
            text
        }
    }
}

/// Why a deployment was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeployError {
    /// `name` does not match `^[a-z0-9][a-z0-9-]{0,63}$`.
    InvalidName(String),
    /// `staging_dir` does not exist or is not a directory.
    StagingNotFound(String),
    /// `staging_dir` has no `SKILL.md`.
    MissingSkillMd(String),
    /// A staged member escapes the staging root (`..`, absolute path, or a
    /// symlink).
    UnsafeMember(String),
    /// More than [`MAX_DEPLOY_FILES`] files were staged.
    TooManyFiles(usize),
    /// A single staged file exceeds [`MAX_DEPLOY_FILE_BYTES`].
    FileTooLarge { path: String, bytes: u64 },
    /// The staged total exceeds [`MAX_DEPLOY_TOTAL_BYTES`].
    TotalTooLarge(u64),
    /// A skill with this name exists with different content and `force` is off.
    TargetConflict(String),
    /// Filesystem failure.
    Io(String),
}

impl DeployError {
    fn io(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl std::fmt::Display for DeployError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidName(name) => write!(
                f,
                "invalid skill name {name:?}: must match ^[a-z0-9][a-z0-9-]{{0,63}}$"
            ),
            Self::StagingNotFound(path) => {
                write!(f, "staging directory does not exist: {path}")
            }
            Self::MissingSkillMd(path) => {
                write!(f, "staging directory has no SKILL.md: {path}")
            }
            Self::UnsafeMember(path) => write!(
                f,
                "unsafe staged member {path:?}: path components must not contain `..`, \
                 absolute paths, or symlinks"
            ),
            Self::TooManyFiles(count) => write!(
                f,
                "staged skill has {count} files, exceeding the limit of {MAX_DEPLOY_FILES}"
            ),
            Self::FileTooLarge { path, bytes } => write!(
                f,
                "staged file {path} is {bytes} bytes, exceeding the per-file limit of \
                 {MAX_DEPLOY_FILE_BYTES}"
            ),
            Self::TotalTooLarge(bytes) => write!(
                f,
                "staged skill totals {bytes} bytes, exceeding the limit of {MAX_DEPLOY_TOTAL_BYTES}"
            ),
            Self::TargetConflict(path) => write!(
                f,
                "target exists with different content: {path}; use force=true or resolve manually"
            ),
            Self::Io(detail) => write!(f, "deployment I/O error: {detail}"),
        }
    }
}

impl std::error::Error for DeployError {}

/// A validated file staged for deployment.
struct Member {
    /// Path relative to the staging root (never absolute, never `..`).
    relative: PathBuf,
    /// Absolute source path.
    source: PathBuf,
    /// File size in bytes.
    size: u64,
}

/// True when `name` matches `^[a-z0-9][a-z0-9-]{0,63}$` (length 1..=64).
pub fn is_valid_deploy_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_SKILL_NAME_LEN {
        return false;
    }
    let first = bytes[0];
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// A staged member is safe iff it is non-empty and every component is a plain
/// name (no `..`, no root, no drive prefix).
fn relative_member_is_safe(relative: &Path) -> bool {
    !relative.as_os_str().is_empty()
        && relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// Install the staged skill at `staging_dir` into
/// `<grok_home>/skills/user-<name>/`.
///
/// `grok_home` is passed explicitly so the engine is testable without touching
/// the process environment. See the module docs for the Phase 1 approval model
/// and the [`MAX_DEPLOY_FILES`] / [`MAX_DEPLOY_FILE_BYTES`] /
/// [`MAX_DEPLOY_TOTAL_BYTES`] bounds.
pub fn deploy_skill(
    staging_dir: &Path,
    name: &str,
    force: bool,
    grok_home: &Path,
) -> Result<DeployReport, DeployError> {
    if !is_valid_deploy_name(name) {
        return Err(DeployError::InvalidName(name.to_string()));
    }
    if !staging_dir.is_dir() {
        return Err(DeployError::StagingNotFound(
            staging_dir.display().to_string(),
        ));
    }
    let staged_skill_md = staging_dir.join("SKILL.md");
    if !staged_skill_md.is_file() {
        return Err(DeployError::MissingSkillMd(
            staged_skill_md.display().to_string(),
        ));
    }

    let members = collect_members(staging_dir)?;
    if members.len() > MAX_DEPLOY_FILES {
        return Err(DeployError::TooManyFiles(members.len()));
    }
    let mut total: u64 = 0;
    for member in &members {
        if member.size > MAX_DEPLOY_FILE_BYTES {
            return Err(DeployError::FileTooLarge {
                path: member.relative.display().to_string(),
                bytes: member.size,
            });
        }
        total = total.saturating_add(member.size);
        if total > MAX_DEPLOY_TOTAL_BYTES {
            return Err(DeployError::TotalTooLarge(total));
        }
    }

    let target_dir = grok_home.join("skills").join(format!("user-{name}"));
    let mut backup_dir = None;

    if target_dir.exists() {
        if target_is_identical(&target_dir, &staged_skill_md)? {
            return Ok(DeployReport {
                target_dir,
                file_count: members.len(),
                backup_dir: None,
                idempotent: true,
            });
        }
        if !force {
            return Err(DeployError::TargetConflict(target_dir.display().to_string()));
        }
        let backups_root = grok_home.join("evolution").join("backups");
        std::fs::create_dir_all(&backups_root).map_err(DeployError::io)?;
        let backup = unique_dir(
            backups_root.join(format!("{}-{name}", utc_timestamp(SystemTime::now()))),
        );
        std::fs::rename(&target_dir, &backup).map_err(DeployError::io)?;
        backup_dir = Some(backup);
    }

    // The publish step is where a half-deployed skill could be created. It is
    // all-or-nothing; see [`publish_target`] for why a partial tree must never
    // survive (it is the correctness precondition for the SKILL.md-only
    // idempotency check in [`target_is_identical`]).
    publish_target(&target_dir, &members)?;

    Ok(DeployReport {
        target_dir,
        file_count: members.len(),
        backup_dir,
        idempotent: false,
    })
}

/// Copy `members` into `target_dir`, creating the tree as it goes.
///
/// This is the only step that makes the deployed tree observable, so it must be
/// all-or-nothing with respect to the idempotency decision: [`target_is_identical`]
/// (the redeploy check) inspects **only** `SKILL.md`, so a half-written tree
/// whose `SKILL.md` already matches would be mistaken for a complete deploy and
/// never repaired — the missing files would stay missing while the hot-reload
/// loader sees a partial skill. On any failure the partial `target_dir` is
/// removed so that state can never be observed by a later redeploy. This is
/// safe: a pre-existing skill was already moved to the backup by the caller, so
/// nothing of the user's is lost by deleting the freshly-created tree.
fn publish_target(target_dir: &Path, members: &[Member]) -> Result<(), DeployError> {
    let result = (|| -> Result<(), DeployError> {
        std::fs::create_dir_all(target_dir).map_err(DeployError::io)?;
        for member in members {
            let destination = target_dir.join(&member.relative);
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).map_err(DeployError::io)?;
            }
            std::fs::copy(&member.source, &destination).map_err(DeployError::io)?;
        }
        Ok(())
    })();
    if result.is_err() {
        // Best-effort cleanup; the original error is what the caller reports.
        let _ = std::fs::remove_dir_all(target_dir);
    }
    result
}

/// Walk `root` and collect every regular file as a [`Member`], rejecting any
/// member that escapes the root (via `..`, an absolute component, or a
/// symlink).
fn collect_members(root: &Path) -> Result<Vec<Member>, DeployError> {
    let mut members = Vec::new();
    let mut pending = vec![root.to_path_buf()];

    while let Some(dir) = pending.pop() {
        let entries = std::fs::read_dir(&dir).map_err(DeployError::io)?;
        for entry in entries {
            let entry = entry.map_err(DeployError::io)?;
            let source = entry.path();
            let relative = source.strip_prefix(root).map_err(|_| {
                DeployError::UnsafeMember(source.display().to_string())
            })?;
            if !relative_member_is_safe(relative) {
                return Err(DeployError::UnsafeMember(relative.display().to_string()));
            }

            // `symlink_metadata` does not follow links, so a symlinked member
            // is rejected rather than silently followed out of the staging
            // root.
            let metadata = std::fs::symlink_metadata(&source).map_err(DeployError::io)?;
            if metadata.file_type().is_symlink() {
                return Err(DeployError::UnsafeMember(relative.display().to_string()));
            }
            if metadata.is_dir() {
                pending.push(source);
            } else if metadata.is_file() {
                members.push(Member {
                    relative: relative.to_path_buf(),
                    source,
                    size: metadata.len(),
                });
            }
        }
    }

    // Deterministic copy order and file count regardless of `read_dir` order.
    members.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(members)
}

/// Whether the installed skill's `SKILL.md` has the same SHA-256 as the staged
/// one (idempotent redeploy).
fn target_is_identical(target_dir: &Path, staged_skill_md: &Path) -> Result<bool, DeployError> {
    let installed = target_dir.join("SKILL.md");
    if !installed.is_file() {
        return Ok(false);
    }
    Ok(sha256_file(staged_skill_md)? == sha256_file(&installed)?)
}

fn sha256_file(path: &Path) -> Result<String, DeployError> {
    cf_file_utils::sha256_hex_from_file(path, None)
        .map_err(|error| DeployError::Io(format!("{}: {error}", path.display())))
}

/// Return `base` if it is free, else the first `base-<n>` that is free.
fn unique_dir(base: PathBuf) -> PathBuf {
    if !base.exists() {
        return base;
    }
    let stem = base
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("backup")
        .to_string();
    // evo-1.5 (K3 B1 plan A): collision suffix uses ~, which is OUTSIDE the
    // skill-name alphabet ([a-z0-9-]), so <ts>-<name>~<n> can never be
    // mistaken for a sibling skill's bare backup when the rollback command
    // reads this directory back. The historical -N shape stays ambiguous
    // for read-back; backups_for() resolves -N tails against known siblings.
    for n in 1..10_000u32 {
        let candidate = base.with_file_name(format!("{stem}~{n}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    base
}

/// Format a `SystemTime` as a UTC `YYYYMMDD-HHMMSS` stamp.
fn utc_timestamp(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = seconds / 86_400;
    let rem = seconds % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

/// Convert days since the Unix epoch to a `(year, month, day)` civil date
/// (Howard Hinnant's `civil_from_days` algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Best-effort session-scoped event writer for the deployment audit trail.
async fn event_writer_for(ctx: &ToolCallContext) -> cf_file_utils::events::EventWriter {
    let Ok(resources) = crate::types::tool_metadata::shared_resources(ctx) else {
        return cf_file_utils::events::EventWriter::noop();
    };
    let guard = resources.lock().await;
    match guard.get::<SessionFolder>() {
        Some(folder) => cf_file_utils::events::EventWriter::open(&folder.0),
        None => cf_file_utils::events::EventWriter::noop(),
    }
}

/// The `deploy_skill` tool.
#[derive(Debug, Default)]
pub struct DeploySkillTool;

impl crate::types::tool_metadata::ToolMetadata for DeploySkillTool {
    fn kind(&self) -> ToolKind {
        // `ToolKind::Other` is deliberate. The renderer's kind→name map keeps
        // the FIRST tool of each kind, so reusing `Write` (or `Skill`) here
        // would shadow `${{ tools.by_kind.write }}` / `${{ tools.by_kind.skill }}`
        // in the system-prompt templates. No other preset tool is `Other`, and
        // `kind_allowed` drops `Other` outside `CapabilityMode::All`, so a
        // restricted subagent cannot deploy skills — a safe default for an
        // install tool. (The root session is always `All`.)
        ToolKind::Other
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::QidiBuild
    }

    fn description_template(&self) -> &str {
        "Deploy a staged skill into the user skills directory \
         (`<grok_home>/skills/user-<name>/`). `staging_dir` must be a directory containing a \
         `SKILL.md` (optionally with `scripts/` and other files); it is validated so no member \
         escapes the staging root. If a skill with the same name already exists, an identical \
         `SKILL.md` makes the call a no-op, while different content is an error unless \
         `force=true`, in which case the existing directory is moved to \
         `<grok_home>/evolution/backups/`. `approval_note` is required: record the human \
         approval that authorizes this deployment."
    }
}

impl Tool for DeploySkillTool {
    type Args = DeploySkillInput;
    type Output = ToolOutput;

    fn id(&self) -> ToolId {
        ToolId::new(DEPLOY_SKILL_TOOL_NAME).expect("static tool id is valid")
    }

    fn description(&self, _ctx: &cf_tool_runtime::ListToolsContext) -> cf_tool_types::ToolDescription {
        cf_tool_types::ToolDescription::new(
            DEPLOY_SKILL_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::description_template(self),
        )
    }

    fn capabilities(&self) -> ToolCapabilities {
        ToolCapabilities {
            is_read_only: false,
            tool_scope: Some(ToolScope::Write),
            ..Default::default()
        }
    }

    async fn run(
        &self,
        ctx: ToolCallContext,
        input: DeploySkillInput,
    ) -> Result<ToolOutput, ToolError> {
        if input.approval_note.trim().is_empty() {
            return Err(ToolError::invalid_arguments(
                "`approval_note` must be a non-empty description of the human approval \
                 authorizing this deployment (Phase 1: recorded as a declarative audit \
                 entry; the path-level `Ask` rule is the enforced gate).",
            ));
        }

        let writer = event_writer_for(&ctx).await;
        let grok_home = crate::util::grok_home::grok_home();
        let staging_dir = PathBuf::from(&input.staging_dir);

        match deploy_skill(&staging_dir, &input.name, input.force, &grok_home) {
            Ok(report) => {
                writer.emit(cf_file_utils::events::Event::SkillDeploy {
                    name: input.name.clone(),
                    staging_dir: input.staging_dir.clone(),
                    target_path: Some(report.target_dir.display().to_string()),
                    backup_path: report
                        .backup_dir
                        .as_ref()
                        .map(|path| path.display().to_string()),
                    file_count: report.file_count,
                    forced: input.force,
                    success: true,
                    error: None,
                    approval_note: input.approval_note.clone(),
                });
                Ok(ToolOutput::Text(report.summary().into()))
            }
            Err(error) => {
                writer.emit(cf_file_utils::events::Event::SkillDeploy {
                    name: input.name.clone(),
                    staging_dir: input.staging_dir.clone(),
                    target_path: None,
                    backup_path: None,
                    file_count: 0,
                    forced: input.force,
                    success: false,
                    error: Some(error.to_string()),
                    approval_note: input.approval_note.clone(),
                });
                Err(match error {
                    DeployError::Io(_) => {
                        ToolError::execution(self.id(), error.to_string())
                    }
                    other => ToolError::invalid_arguments(other.to_string()),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Create a staging dir containing `SKILL.md` (and optionally more files).
    fn staging(extra: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("SKILL.md"), "---\nname: demo\n---\nbody\n").expect("write");
        for (rel, content) in extra {
            let path = dir.path().join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("mkdir");
            }
            fs::write(path, content).expect("write");
        }
        dir
    }

    fn home() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    // ── name validation ────────────────────────────────────────────

    #[test]
    fn rejects_invalid_names() {
        for name in ["", "..", "../evil", "UPPER", "has space", "a/b", "a.b"] {
            assert!(
                !is_valid_deploy_name(name),
                "name {name:?} must be rejected"
            );
        }
        assert!(!is_valid_deploy_name(&"a".repeat(65)));
    }

    #[test]
    fn accepts_valid_names() {
        for name in ["a", "abc", "user-foo", "x1", "a-b-c", "0"] {
            assert!(is_valid_deploy_name(name), "name {name:?} must be accepted");
        }
        assert!(is_valid_deploy_name(&"a".repeat(64)));
    }

    #[test]
    fn deploy_rejects_invalid_name_before_touching_disk() {
        let stage = staging(&[]);
        let home = home();
        let err = deploy_skill(stage.path(), "../evil", false, home.path()).unwrap_err();
        assert!(matches!(err, DeployError::InvalidName(_)));
        assert!(!home.path().join("skills").exists());
    }

    // ── staging validation ─────────────────────────────────────────

    #[test]
    fn rejects_missing_staging_dir() {
        let home = home();
        let err = deploy_skill(Path::new("/nonexistent/qidi/staging"), "demo", false, home.path())
            .unwrap_err();
        assert!(matches!(err, DeployError::StagingNotFound(_)));
    }

    #[test]
    fn rejects_staging_without_skill_md() {
        let stage = tempfile::tempdir().expect("tempdir");
        fs::write(stage.path().join("README.md"), "no skill here").expect("write");
        let home = home();
        let err = deploy_skill(stage.path(), "demo", false, home.path()).unwrap_err();
        assert!(matches!(err, DeployError::MissingSkillMd(_)));
    }

    #[test]
    fn rejects_unsafe_member_components() {
        assert!(!relative_member_is_safe(Path::new("../evil")));
        assert!(!relative_member_is_safe(Path::new("a/../../evil")));
        assert!(!relative_member_is_safe(Path::new("/abs")));
        assert!(!relative_member_is_safe(Path::new("")));
        assert!(relative_member_is_safe(Path::new("SKILL.md")));
        assert!(relative_member_is_safe(Path::new("scripts/run.py")));
    }

    #[test]
    #[cfg(unix)]
    fn rejects_symlinked_member() {
        use std::os::unix::fs::symlink;
        let stage = staging(&[]);
        let outside = tempfile::tempdir().expect("tempdir");
        fs::write(outside.path().join("secret"), "leak").expect("write");
        symlink(outside.path().join("secret"), stage.path().join("link")).expect("symlink");

        let home = home();
        let err = deploy_skill(stage.path(), "demo", false, home.path()).unwrap_err();
        assert!(matches!(err, DeployError::UnsafeMember(_)));
    }

    // ── size / count bounds ────────────────────────────────────────

    #[test]
    fn rejects_oversized_single_file() {
        let big = "x".repeat(MAX_DEPLOY_FILE_BYTES as usize + 1);
        let stage = staging(&[("big.txt", &big)]);
        let home = home();
        let err = deploy_skill(stage.path(), "demo", false, home.path()).unwrap_err();
        assert!(matches!(err, DeployError::FileTooLarge { .. }));
    }

    #[test]
    fn rejects_too_many_files() {
        let stage = staging(&[]);
        for i in 0..MAX_DEPLOY_FILES {
            fs::write(stage.path().join(format!("f{i}.txt")), "x").expect("write");
        }
        let home = home();
        // SKILL.md + MAX_DEPLOY_FILES files exceeds the cap.
        let err = deploy_skill(stage.path(), "demo", false, home.path()).unwrap_err();
        assert!(matches!(err, DeployError::TooManyFiles(_)));
    }

    // ── happy path ─────────────────────────────────────────────────

    #[test]
    fn deploys_skill_with_nested_files() {
        let stage = staging(&[("scripts/run.py", "print('hi')\n")]);
        let home = home();
        let report = deploy_skill(stage.path(), "demo", false, home.path()).expect("deploy");

        assert!(!report.idempotent);
        assert_eq!(report.file_count, 2);
        assert!(report.backup_dir.is_none());
        assert_eq!(report.target_dir, home.path().join("skills").join("user-demo"));
        assert!(report.target_dir.join("SKILL.md").is_file());
        assert!(report.target_dir.join("scripts").join("run.py").is_file());
    }

    // ── failed-publish cleanup ─────────────────────────────────────

    /// A failure part-way through publishing must leave no partial tree, so a
    /// later redeploy cannot mistake a half-written skill for a complete one via
    /// the SKILL.md-only idempotency check (`target_is_identical`).
    ///
    /// Construction: two members are handed to [`publish_target`] directly with
    /// relative paths `a` and `a/b`. Copying `a` first creates `target/a` as a
    /// FILE, so the `create_dir_all(target/a)` needed for `a/b` then fails
    /// mid-publish. A real staging dir cannot hold both a file `a` and a
    /// directory `a`, which is why the collision is built at the [`Member`]
    /// level instead of on disk — and it fails identically on Windows and Unix.
    #[test]
    fn failed_publish_removes_partial_target() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("skills").join("user-demo");
        let source_a = tmp.path().join("source-a");
        let source_b = tmp.path().join("source-b");
        fs::write(&source_a, "A").expect("write");
        fs::write(&source_b, "B").expect("write");

        let members = vec![
            Member {
                relative: PathBuf::from("a"),
                source: source_a,
                size: 1,
            },
            Member {
                relative: PathBuf::from("a").join("b"),
                source: source_b,
                size: 1,
            },
        ];

        let err = publish_target(&target, &members).unwrap_err();
        assert!(matches!(err, DeployError::Io(_)), "got {err:?}");
        assert!(
            !target.exists(),
            "a failed publish must remove the partial target tree"
        );
    }

    // ── same-name detection ────────────────────────────────────────

    #[test]
    fn identical_content_is_idempotent() {
        let stage = staging(&[]);
        let home = home();
        deploy_skill(stage.path(), "demo", false, home.path()).expect("first deploy");

        let report = deploy_skill(stage.path(), "demo", false, home.path()).expect("second deploy");
        assert!(report.idempotent, "an identical redeploy must be a no-op");
        assert!(report.backup_dir.is_none());
        assert!(!home.path().join("evolution").exists());
    }

    #[test]
    fn conflicting_content_without_force_errors() {
        let home = home();
        let first = staging(&[]);
        deploy_skill(first.path(), "demo", false, home.path()).expect("first deploy");

        // Same name, different SKILL.md.
        let second = tempfile::tempdir().expect("tempdir");
        fs::write(second.path().join("SKILL.md"), "---\nname: demo\n---\ndifferent\n")
            .expect("write");

        let err = deploy_skill(second.path(), "demo", false, home.path()).unwrap_err();
        assert!(matches!(err, DeployError::TargetConflict(_)));
        assert!(
            err.to_string().contains("use force=true or resolve manually"),
            "error must hint at force: {err}"
        );
    }

    #[test]
    fn force_backs_up_then_overwrites() {
        let home = home();
        let first = staging(&[]);
        deploy_skill(first.path(), "demo", false, home.path()).expect("first deploy");

        let second = tempfile::tempdir().expect("tempdir");
        fs::write(second.path().join("SKILL.md"), "---\nname: demo\n---\nversion two\n")
            .expect("write");

        let report = deploy_skill(second.path(), "demo", true, home.path()).expect("force deploy");
        assert!(!report.idempotent);
        let backup = report.backup_dir.expect("backup recorded");

        // The backup lives under <grok_home>/evolution/backups/<ts>-<name>/ and
        // holds the OLD content.
        assert!(backup.starts_with(home.path().join("evolution").join("backups")));
        let backup_name = backup.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        assert!(
            backup_name.ends_with("-demo"),
            "backup dir must end with -<name>: {backup_name}"
        );
        assert_eq!(
            fs::read_to_string(backup.join("SKILL.md")).expect("read backup"),
            "---\nname: demo\n---\nbody\n",
            "backup must contain the previous SKILL.md"
        );
        // And the target now holds the new content.
        assert_eq!(
            fs::read_to_string(report.target_dir.join("SKILL.md")).expect("read target"),
            "---\nname: demo\n---\nversion two\n"
        );
    }

    // ── timestamp ──────────────────────────────────────────────────

    #[test]
    fn timestamp_is_compact_and_monotonic_shape() {
        let stamp = utc_timestamp(UNIX_EPOCH);
        assert_eq!(stamp, "19700101-000000");
        // 2024-01-01T00:00:00Z == 1704067200
        let stamp = utc_timestamp(UNIX_EPOCH + std::time::Duration::from_secs(1_704_067_200));
        assert_eq!(stamp, "20240101-000000");
    }
}
