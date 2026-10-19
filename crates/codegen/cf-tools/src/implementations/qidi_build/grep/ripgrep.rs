use std::path::PathBuf;
use std::sync::OnceLock;

#[cfg(bundle_rg)]
const RG_BYTES: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/bundle-rg/rg-",
    env!("QIDI_TOOLS_RG_VER"),
    "-",
    env!("QIDI_TOOLS_RG_TARGET"),
    ".bin"
));

#[cfg(bundle_rg)]
fn resolve_bundled_rg() -> std::io::Result<PathBuf> {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    let p = crate::util::grok_home().join("vendor").join(concat!(
        "rg-",
        env!("QIDI_TOOLS_RG_VER"),
        "-",
        env!("QIDI_TOOLS_RG_TARGET")
    ));
    if !p.exists() {
        fs::create_dir_all(p.parent().unwrap())?;
        fs::write(&p, RG_BYTES)?;
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&p)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&p, perms)?;
        }
    }
    Ok(p)
}

/// Whether the ripgrep binary resolves to a runnable executable.
///
/// The opencode and qidi_build grep/glob implementations shell out to
/// `rg`; their integration tests exercise that real pipeline and need a
/// working binary. Ripgrep is not preinstalled on GitHub's Windows
/// runners, and Windows builds skip the auto-bundle (see `build.rs`:
/// "Users install ripgrep separately (winget / scoop)"), so on such
/// machines the rg-dependent tests skip with an explanatory message
/// instead of panicking with "program not found".
#[cfg(test)]
pub(crate) fn ripgrep_available() -> bool {
    probe_rg_executable(&rg_path())
}

/// Probe one executable path by running `rg --version`.
///
/// Split out from [`rg_path`] so tests can point the probe at a known
/// missing path without poisoning the process-wide `OnceLock` caches.
fn probe_rg_executable(exec: &std::path::Path) -> bool {
    std::process::Command::new(exec)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Cached verdict on whether the resolved ripgrep binary is runnable.
///
/// Probe once per process: spawning `rg --version` on every tool call would
/// add latency, and the binary cannot appear mid-process (Windows locks
/// running executables, and PATH is fixed at startup).
pub fn ripgrep_available_cached() -> bool {
    static RG_AVAILABLE: OnceLock<bool> = OnceLock::new();
    *RG_AVAILABLE.get_or_init(|| probe_rg_executable(&rg_path()))
}

/// User-facing notice emitted (once) when the built-in grep cannot run.
pub const RG_UNAVAILABLE_NOTICE: &str = "warning: built-in grep is unavailable \
    (ripgrep `rg` was not found or is not executable). Use `git grep` for \
    repository searches, or install ripgrep (winget/scoop/apt).";

/// Emit a single startup warning when ripgrep cannot be executed.
///
/// Called once from process initialization; the `OnceLock` makes repeated
/// calls cheap and silent. Windows builds without the bundled zip land here
/// because `rg` is not preinstalled (see `cf-shell/build.rs`).
pub fn warn_if_ripgrep_unavailable() {
    static WARNED: OnceLock<()> = OnceLock::new();
    if ripgrep_available_cached() {
        return;
    }
    WARNED.get_or_init(|| {
        // stderr keeps the notice visible in both TUI and headless modes;
        // tracing alone is filtered out by the default `error` level.
        eprintln!("{RG_UNAVAILABLE_NOTICE}");
        tracing::warn!(
            rg = %rg_path().display(),
            "built-in grep is unavailable; use `git grep` for repository searches"
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_rejects_missing_binary() {
        let missing = std::env::temp_dir().join("qidi-rg-does-not-exist-0123456789");
        assert!(
            !probe_rg_executable(&missing),
            "probe must return false for a missing binary"
        );
    }

    #[test]
    fn unavailable_notice_names_the_workaround() {
        assert!(
            RG_UNAVAILABLE_NOTICE.contains("ripgrep"),
            "notice must name ripgrep: {RG_UNAVAILABLE_NOTICE}"
        );
        assert!(
            RG_UNAVAILABLE_NOTICE.contains("git grep"),
            "notice must point users at `git grep`: {RG_UNAVAILABLE_NOTICE}"
        );
    }
}

/// Get the path to the ripgrep executable.
///
/// In release builds with bundling enabled, this extracts the bundled ripgrep
/// binary to ~/.qidi/vendor/ and returns that path.
/// Otherwise, assumes `rg` is in PATH.
pub fn rg_path() -> PathBuf {
    static RG_EXEC: OnceLock<PathBuf> = OnceLock::new();
    RG_EXEC
        .get_or_init(|| {
            #[cfg(bundle_rg)]
            {
                resolve_bundled_rg().unwrap_or_else(|_| PathBuf::from("rg"))
            }
            #[cfg(not(bundle_rg))]
            {
                // RG_BIN_PATH: explicit override (tests / packaging can set this).
                if let Ok(p) = std::env::var("RG_BIN_PATH") {
                    return PathBuf::from(p);
                }
                // Some hermetic test runners set RUNFILES_DIR and ship rg as a
                // data dependency rather than on PATH. Scan for a directory
                // entry containing "ripgrep_hermetic" and prefer arch-scoped
                // paths when present.
                if let Ok(rf) = std::env::var("RUNFILES_DIR") {
                    let base = PathBuf::from(rf);
                    if let Ok(entries) = std::fs::read_dir(&base) {
                        for entry in entries.flatten() {
                            let name = entry.file_name();
                            if name.to_string_lossy().contains("ripgrep_hermetic") {
                                for sub in ["amd64/rg", "arm64/rg", "rg"] {
                                    let candidate = entry.path().join(sub);
                                    if candidate.exists() {
                                        return candidate;
                                    }
                                }
                            }
                        }
                    }
                }
                PathBuf::from("rg")
            }
        })
        .clone()
}
