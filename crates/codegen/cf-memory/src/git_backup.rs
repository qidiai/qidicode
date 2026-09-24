//! Best-effort git snapshotting of the memory root (`~/.qidi/memory/`).
//!
//! The memory root is turned into a local git repository that tracks **only
//! `*.md` files**. This buys the memory system three things:
//!
//! 1. **A1′ safety net.** Dream consolidation *deletes* the session logs it
//!    has digested (`dream.rs::clean_processed_sessions`, the `remove_file`
//!    call). Once gone they are unrecoverable. A snapshot taken *before* that
//!    deletion puts the soon-to-be-deleted logs into git history, so they can
//!    always be recovered with `git show <commit>:<path>`.
//! 2. **Unbounded history** for `MEMORY.md`. The rolling `.bak-1..3` copies
//!    written by `storage.rs::write_long_term` only keep 3 generations; git
//!    keeps them all (and gives free `diff`).
//! 3. A snapshot of any **hand edits** on the next dream pass.
//!
//! ## Why the whitelist is safe
//!
//! The `.gitignore` we write uses a whitelist (last match wins):
//!
//! ```text
//! *
//! !*/
//! !*.md
//! ```
//!
//! `*` ignores everything (files *and* directories) at any depth; `!*/`
//! re-includes directories so git can recurse into them; `!*.md` re-includes
//! markdown at any depth. Every non-markdown file therefore stays ignored —
//! crucially the SQLite index (`index.sqlite`, `index.sqlite-wal`,
//! `index.sqlite-shm`, `index.h-*.sqlite`) and lock files (`.dream-lock`,
//! `index.lock`, `*.lock`). Committing a *live* WAL database would capture an
//! inconsistent snapshot, which is why this is a hard requirement.
//!
//! Note: the naive `/*` form (`/*` + `!*/` + `!*.md`) is **not** equivalent —
//! `/*` is anchored to the root, so a nested file such as
//! `{ws}/index.sqlite` is never ignored and *would* be committed. The
//! unanchored `*` form above is required to ignore at every depth.
//!
//! `.git/` internals are never tracked by git itself, and both the memory
//! watcher (`watcher.rs:56`, which only reacts to paths whose extension is
//! `md`) and `MemoryStorage::list_memory_files` (`storage.rs:401`, which
//! filters on the `md` extension) gate on `.md`, so nothing under `.git/` can
//! pollute the search index or the dream session counts.
//!
//! ## Failure model
//!
//! [`snapshot`] is best-effort and **never panics**. Every failure — missing
//! `git` binary, `index.lock` contention between concurrent instances, hook
//! rejection, I/O errors — is returned as `Err(String)` for the caller to log
//! and ignore. Concurrency: two instances racing to commit will collide on
//! git's `index.lock`; the loser gets `Err` (which callers ignore) rather than
//! corrupting anything.
//!
//! We shell out to the `git` CLI via [`std::process::Command`] rather than
//! linking a library — the workflow already depends on a `git` binary.

use std::path::Path;
use std::process::Command;

/// Whitelist `.gitignore` — track only markdown. See the module docs for why
/// the unanchored `*` (not `/*`) is required.
const GITIGNORE: &str = "\
# Managed by cf-memory: track only markdown.
#
# Whitelist (order matters, last match wins):
#   *      ignore every file and directory, at any depth
#   !*/    re-include directories so git may recurse into them
#   !*.md  re-include markdown files, at any depth
#
# SQLite databases (index.sqlite + WAL/SHM sidecars) and lock files are
# therefore never committed.
*
!*/
!*.md
";

/// Best-effort git snapshot of the memory root. Never panics; all failures are
/// returned as `Err` (caller logs and ignores).
///
/// On first use this initialises a repository at `memory_root` and writes the
/// whitelist `.gitignore`. It then stages everything the whitelist allows and
/// commits. A commit that finds nothing to record is a normal no-op (`Ok`); a
/// brand-new repository always records one initial (possibly empty) commit so
/// that `git log` is never empty.
pub fn snapshot(memory_root: &Path, message: &str) -> Result<(), String> {
    if !memory_root.is_dir() {
        return Err(format!(
            "memory root is not a directory: {}",
            memory_root.display()
        ));
    }

    // 1. Initialise the repository + whitelist on first use.
    if !memory_root.join(".git").exists() {
        run_git(memory_root, &["init", "-q"])?;
    }
    let gitignore = memory_root.join(".gitignore");
    if !gitignore.exists() {
        std::fs::write(&gitignore, GITIGNORE)
            .map_err(|e| format!("failed to write .gitignore: {e}"))?;
    }

    // 2. Ensure a *repo-local* identity (never touches global config).
    ensure_identity(memory_root)?;

    // 3. Stage everything the whitelist permits.
    run_git(memory_root, &["add", "-A"])?;

    let staged = has_staged_changes(memory_root)?;
    if !staged {
        // Nothing changed. If the repo already has history this is a no-op;
        // otherwise record one initial commit so `git log` is never empty.
        if has_head(memory_root) {
            return Ok(());
        }
        return commit(memory_root, message, true);
    }

    commit(memory_root, message, false)
}

/// Ensure `user.name` / `user.email` are set **locally** in the repo.
///
/// A global identity is deliberately ignored: `git config --local` reports
/// "not set" for a fresh repo even when a global value exists, so we always
/// pin the memory repo to its own identity without ever writing global config.
fn ensure_identity(root: &Path) -> Result<(), String> {
    for (key, value) in [
        ("user.name", "qidi-memory"),
        ("user.email", "qidi-memory@local"),
    ] {
        let current = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["config", "--local", key])
            .output()
            .map_err(|e| format!("failed to run git config: {e}"))?;
        let already_set = current.status.success()
            && !String::from_utf8_lossy(&current.stdout).trim().is_empty();
        if !already_set {
            run_git(root, &["config", "--local", key, value])?;
        }
    }
    Ok(())
}

/// `true` if the index has staged changes (`git diff --cached --quiet` exits
/// with code 1 when differences exist).
fn has_staged_changes(root: &Path) -> Result<bool, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["diff", "--cached", "--quiet"])
        .output()
        .map_err(|e| format!("failed to run git diff: {e}"))?;
    match out.status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(format!(
            "git diff --cached failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

/// `true` if the repository has at least one commit (`HEAD` resolves).
fn has_head(root: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--verify", "--quiet", "HEAD"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run `git commit -q -m <message>`, treating "nothing to commit" as a
/// successful no-op. When `allow_empty` is set the initial commit is forced so
/// a fresh memory root still gets a baseline.
fn commit(root: &Path, message: &str, allow_empty: bool) -> Result<(), String> {
    let mut args = vec!["commit", "-q"];
    if allow_empty {
        args.push("--allow-empty");
    }
    args.push("-m");
    args.push(message);

    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(&args)
        .output()
        .map_err(|e| format!("failed to run git commit: {e}"))?;

    if out.status.success() {
        return Ok(());
    }

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if combined.contains("nothing to commit") || combined.contains("no changes added to commit") {
        return Ok(());
    }

    Err(format!("git commit failed: {}", combined.trim()))
}

/// Run a `git -C <root> <args>` command, mapping a non-zero exit to `Err`.
fn run_git(root: &Path, args: &[&str]) -> Result<(), String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git {}: {e}", args.join(" ")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    /// Run a `git -C <root> ...` command, asserting success, returning stdout.
    fn git(root: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Number of commits reachable from HEAD.
    fn commit_count(root: &Path) -> usize {
        git(root, &["rev-list", "--count", "HEAD"])
            .trim()
            .parse()
            .unwrap()
    }

    // ① Empty directory: snapshot must init the repo and make a first commit.
    #[test]
    fn empty_dir_snapshot_inits_and_makes_first_commit() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        snapshot(root, "first snapshot").unwrap();

        assert!(root.join(".git").is_dir(), ".git should exist");
        assert!(root.join(".gitignore").is_file(), ".gitignore should exist");
        let log = git(root, &["log", "--oneline"]);
        assert_eq!(
            log.lines().count(),
            1,
            "expected exactly one commit, log:\n{log}"
        );
    }

    // ② Unchanged snapshot: no-op, still Ok, no new commit.
    #[test]
    fn unchanged_snapshot_is_noop_and_ok() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("MEMORY.md"), "hello").unwrap();

        snapshot(root, "first").unwrap();
        let before = commit_count(root);
        assert_eq!(before, 1, "first snapshot should commit once");

        snapshot(root, "second").unwrap();
        snapshot(root, "third").unwrap();

        assert_eq!(
            commit_count(root),
            before,
            "no-change snapshots must not add commits"
        );
    }

    // ③ A1′ core: a session log deleted after the pre-dream snapshot is still
    // recoverable from git history.
    #[test]
    fn pre_snapshot_preserves_deleted_session_log() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let rel = "ws-abc12345/sessions/2024-01-01-foo-abcd1234.md";
        let abs = root
            .join("ws-abc12345")
            .join("sessions")
            .join("2024-01-01-foo-abcd1234.md");
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(&abs, "IMPORTANT session content").unwrap();

        snapshot(root, "pre-dream snapshot").unwrap();
        let hash = git(root, &["rev-parse", "HEAD"]).trim().to_string();

        // Simulate dream cleanup deleting the consolidated session log.
        std::fs::remove_file(&abs).unwrap();
        assert!(!abs.exists(), "session log should be gone from the worktree");

        // ...yet the content survives in the pre-dream commit.
        let shown = git(root, &["show", &format!("{hash}:{rel}")]);
        assert!(
            shown.contains("IMPORTANT session content"),
            "recovered content mismatch:\n{shown}"
        );
    }

    // ④ SQLite databases and lock files must never be tracked (audit red line).
    #[test]
    fn sqlite_and_lock_files_are_never_tracked() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        snapshot(root, "init").unwrap();

        for name in [
            "index.sqlite",
            "index.sqlite-wal",
            "index.sqlite-shm",
            "index.h-abc123.sqlite",
            ".dream-lock",
            "index.lock",
            "foo.lock",
        ] {
            std::fs::write(root.join(name), "x").unwrap();
        }
        // A nested workspace copy (the `/*` form would leak this).
        std::fs::create_dir_all(root.join("ws-abc12345")).unwrap();
        std::fs::write(root.join("ws-abc12345/index.sqlite"), "x").unwrap();
        std::fs::write(root.join("ws-abc12345/MEMORY.md"), "md").unwrap();

        snapshot(root, "after").unwrap();

        let tracked = git(root, &["ls-files"]);
        for line in tracked.lines() {
            assert!(!line.contains("sqlite"), "sqlite must not be tracked: {line}");
            assert!(!line.contains("lock"), "lock file must not be tracked: {line}");
        }
        // Sanity: markdown *is* tracked (whitelist isn't over-broad).
        assert!(
            tracked.contains("ws-abc12345/MEMORY.md"),
            "MEMORY.md should be tracked:\n{tracked}"
        );
    }

    // Missing memory root is a soft error, never a panic.
    #[test]
    fn missing_root_returns_err() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        assert!(snapshot(&missing, "msg").is_err());
    }
}
