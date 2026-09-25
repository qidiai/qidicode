//! Home-directory resolution generally: USERPROFILE-first `home_dir`, plus
//! qidi-home (`$QIDI_HOME` or `<home>/.qidi`). Shared by config and the
//! fast-worktree crates.
//!
//! Which function to call:
//! - [`qidi_home`]: the usual choice, a cached, created path to build on.
//! - [`user_qidi_home`]: `None` instead of a cwd fallback when no home resolves.
//! - [`default_qidi_home`]: the `<home>/.qidi` default, ignoring `$QIDI_HOME`,
//!   so callers can detect an override.
//! - [`resolve_qidi_home`]: a fresh, uncached resolve.
//!
//! TODO(upstream): collapse these getters by threading the path through
//! config as an explicit value.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// `<home>/.qidi`, canonicalized via `dunce` (not `std::fs::canonicalize`,
/// which yields Windows `\\?\` verbatim paths).
fn qidi_home_in(home: &Path) -> PathBuf {
    dunce::canonicalize(home)
        .unwrap_or_else(|_| home.to_path_buf())
        .join(".qidi")
}

/// `$QIDI_HOME` verbatim when non-empty, else `<home>/.qidi`. The env value is
/// used as-is (not canonicalized) so it stays stable and comparable: callers do
/// literal prefix checks against it, and downstream symlink guards must still see
/// its original components.
fn resolve_qidi_home_from(
    qidi_home_env: Option<&OsStr>,
    os_home: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(env) = qidi_home_env.filter(|env| !env.is_empty()) {
        return Some(PathBuf::from(env));
    }
    os_home.map(qidi_home_in)
}

/// Resolve the qidi home from the environment (fresh, no cache); `None` if neither resolves.
pub fn resolve_qidi_home() -> Option<PathBuf> {
    resolve_qidi_home_from(
        std::env::var_os("QIDI_HOME").as_deref(),
        dirs::home_dir().as_deref(),
    )
}

/// The default `<home>/.qidi`, used when `$QIDI_HOME` is unset.
pub fn default_qidi_home() -> PathBuf {
    qidi_home_in(&dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
}

/// The qidi home, created if missing and cached for the process; falls back to
/// [`default_qidi_home`] when neither `$QIDI_HOME` nor a home resolves.
pub fn qidi_home() -> PathBuf {
    static QIDI_HOME: OnceLock<PathBuf> = OnceLock::new();
    QIDI_HOME
        .get_or_init(|| {
            let home = resolve_qidi_home().unwrap_or_else(default_qidi_home);
            if let Err(err) = std::fs::create_dir_all(&home) {
                tracing::warn!(path = %home.display(), %err, "failed to create qidi home");
            }
            home
        })
        .clone()
}

/// Like [`qidi_home`], but `None` when no home resolves (no cwd fallback).
pub fn user_qidi_home() -> Option<PathBuf> {
    resolve_qidi_home().is_some().then(qidi_home)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::ffi::OsString;

    #[test]
    fn env_wins_over_os_home() {
        let resolved =
            resolve_qidi_home_from(Some(OsStr::new("/custom/home")), Some(Path::new("/home/u")));
        assert_eq!(resolved, Some(PathBuf::from("/custom/home")));
    }

    #[test]
    fn env_used_verbatim_even_when_it_exists() {
        // A real, existing dir whose canonical form differs (macOS symlinks
        // `/var` -> `/private/var`): the env value must come back unchanged.
        let tmp = tempfile::tempdir().unwrap();
        let resolved = resolve_qidi_home_from(Some(tmp.path().as_os_str()), None);
        assert_eq!(resolved, Some(tmp.path().to_path_buf()));
    }

    #[test]
    fn empty_env_falls_through_to_os_home() {
        let tmp = tempfile::tempdir().unwrap();
        let resolved = resolve_qidi_home_from(Some(&OsString::new()), Some(tmp.path()));
        assert_eq!(
            resolved,
            Some(dunce::canonicalize(tmp.path()).unwrap().join(".qidi"))
        );
    }

    #[test]
    fn default_qidi_home_has_no_verbatim_prefix() {
        // The reason we canonicalize via dunce: std::fs::canonicalize yields
        // `\\?\` verbatim paths on Windows that break git and byte-exact
        // comparisons. No-op assertion on Unix.
        let home = default_qidi_home();
        assert!(!home.to_string_lossy().starts_with(r"\\?\"));
        assert!(home.ends_with(".qidi"));
    }

    #[test]
    fn none_when_nothing_resolves() {
        assert_eq!(
            resolve_qidi_home_from(/* qidi_home_env */ None, /* os_home */ None),
            None
        );
    }
}
