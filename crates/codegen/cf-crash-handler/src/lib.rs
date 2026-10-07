//! Cross-platform crash handler with startup crash detection.
//!
//! - **Unix**: SIGBUS/SIGSEGV via `sigaction(2)`.
//! - **Windows**: access violations via `SetUnhandledExceptionFilter`.
//!
//! # Usage
//!
//! Call [`check_previous_crash`] first to detect crashes from the previous
//! session, then [`install`] early in `main()`, before any async runtime or
//! thread spawning. `check_previous_crash` must run before `install` because
//! `install` opens `last-crash.bin` with `O_TRUNC`.
//!
//! ```rust,no_run
//! use std::path::PathBuf;
//!
//! let crash_dir = PathBuf::from("/home/user/.myapp/crash");
//!
//! if let Some(report) = cf_crash_handler::check_previous_crash(&crash_dir) {
//!     eprintln!("Application crashed during your last session.");
//!     eprintln!("  Signal: {}", report.signal_name);
//!     match &report.report_path {
//!         Some(p) => eprintln!("  Report: {}", p.display()),
//!         None => eprintln!("  Report: <unavailable>"),
//!     }
//! }
//!
//! cf_crash_handler::install(cf_crash_handler::CrashHandlerConfig {
//!     app_version: "0.1.0".to_string(),
//!     crash_dir: crash_dir.clone(),
//! });
//! ```

pub mod format;
mod handler;
pub mod symbolicate;
pub mod terminal;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub use symbolicate::ResolvedFrame;

const MAX_HISTORY: usize = 5;

/// Configuration for the crash handler.
pub struct CrashHandlerConfig {
    /// Application version string (e.g. "0.1.169-alpha.2").
    pub app_version: String,
    /// Directory where crash dumps are written.
    /// Created if it does not exist.
    pub crash_dir: PathBuf,
}

/// Information about a crash from the previous session.
#[derive(Debug)]
pub struct CrashReport {
    /// Human-readable signal name (e.g. "SIGBUS (Bus error)").
    pub signal_name: &'static str,
    /// The `si_code` from `siginfo_t`.
    pub si_code: i32,
    /// The faulting memory address.
    pub faulting_address: u64,
    /// Unix timestamp of the crash.
    pub timestamp: u64,
    /// Application version at crash time.
    pub app_version: String,
    /// Symbolicated backtrace frames.
    pub backtrace: Vec<ResolvedFrame>,
    /// Path to the saved human-readable crash report, if the
    /// report write succeeded (best-effort: a failed write
    /// yields `None` rather than a path to a non-existent file).
    pub report_path: Option<PathBuf>,
}

/// Install the crash handler for SIGBUS and SIGSEGV.
///
/// Must be called early in `main()`, before any async runtime or thread
/// spawning. Creates `crash_dir` if it does not exist.
///
/// Returns `true` if the handler was installed successfully.
/// On unsupported platforms, this is a no-op that returns `false`.
pub fn install(config: CrashHandlerConfig) -> bool {
    handler::install(&config.crash_dir, &config.app_version)
}

/// Install a minimal SIGSEGV/SIGBUS handler that only restores the terminal.
///
/// On Unix, saves the current termios state, allocates an alternate signal
/// stack, and registers a handler that writes terminal restore escape
/// sequences to stderr, restores termios, then re-raises with default
/// disposition (preserving core dumps).
///
/// On Windows, registers an unhandled-exception filter that writes restore
/// sequences; no termios equivalent.
///
/// No-op on unsupported platforms.
///
/// No crash reporting (no file I/O, no stack walking). If [`install`] is
/// called later, it replaces these handlers with full crash-reporting
/// variants.
pub fn install_terminal_restore_only() {
    handler::install_terminal_restore_only()
}

/// Upgrade SIGSEGV/SIGBUS handlers to include terminal escape code
/// restoration. Call when TUI modes are enabled.
pub fn enable_terminal_escape_restore() {
    handler::enable_terminal_escape_restore()
}

/// Downgrade SIGSEGV/SIGBUS handlers to termios-only restoration.
/// Call when TUI modes are disabled.
pub fn disable_terminal_escape_restore() {
    handler::disable_terminal_escape_restore()
}

/// Check for a crash from the previous session.
///
/// Reads `crash_dir/last-crash.bin`, symbolicates the backtrace,
/// writes a human-readable report, and archives it. Returns `Some` if
/// a valid crash file was found, `None` otherwise.
pub fn check_previous_crash(crash_dir: &Path) -> Option<CrashReport> {
    let crash_file = crash_dir.join("last-crash.bin");
    let data = std::fs::read(&crash_file).ok()?;
    let blob = match format::CrashBlob::parse(&data) {
        Some(blob) => blob,
        None => {
            // Unparseable blob (truncated/corrupt dump): remove it
            // so a damaged `last-crash.bin` does not poison every
            // subsequent startup check — the file is rewritten from
            // scratch on the next crash either way.
            let _ = std::fs::remove_file(&crash_file);
            return None;
        }
    };

    let frames = symbolicate::resolve_frames(&blob);
    let report_text = symbolicate::format_report(&blob, &frames);

    // Write the human-readable report (best-effort: a failed
    // write is reported as `None`, not as a path to a file that
    // does not exist).
    let report_path = crash_dir.join("last-crash-report.txt");
    let report_path = std::fs::write(&report_path, &report_text)
        .ok()
        .map(|_| report_path);

    // Archive to history/ (keep last MAX_HISTORY).
    archive_report(crash_dir, &report_text, blob.timestamp);

    // Remove the binary blob so it's not re-processed.
    let _ = std::fs::remove_file(&crash_file);

    Some(CrashReport {
        signal_name: symbolicate::signal_name(blob.signal),
        si_code: blob.si_code,
        faulting_address: blob.si_addr,
        timestamp: blob.timestamp,
        app_version: blob.app_version,
        backtrace: frames,
        report_path,
    })
}

fn archive_report(crash_dir: &Path, report_text: &str, timestamp: u64) {
    let history_dir = crash_dir.join("history");
    let _ = std::fs::create_dir_all(&history_dir);

    // Two crashes within the same second share the bare-timestamp
    // name; add a sequence suffix so the later report does not
    // overwrite the earlier one.
    let mut filename = format!("crash-{}.txt", timestamp);
    let mut seq = 1;
    while history_dir.join(&filename).exists() {
        filename = format!("crash-{timestamp}-{seq}.txt");
        seq += 1;
    }
    let _ = std::fs::write(history_dir.join(&filename), report_text);

    // Prune old reports beyond MAX_HISTORY. Only `crash-*`
    // reports are managed here (the directory may hold other
    // files), ordered by mtime so the newest survive regardless
    // of filename spelling; entries with an unreadable mtime
    // sort first and are pruned first.
    if let Ok(entries) = std::fs::read_dir(&history_dir) {
        let mut files: Vec<(PathBuf, Option<SystemTime>)> = entries
            .filter_map(|e| e.ok())
            .map(|e| {
                let mtime = e.metadata().ok().and_then(|m| m.modified().ok());
                (e.path(), mtime)
            })
            .filter(|(path, _)| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("crash-"))
            })
            .collect();
        files.sort_by_key(|(_, mtime)| *mtime);
        if files.len() > MAX_HISTORY {
            for (old, _) in &files[..files.len() - MAX_HISTORY] {
                let _ = std::fs::remove_file(old);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_previous_crash_returns_none_when_no_file() {
        let dir = PathBuf::from("/tmp/xai-crash-handler-test-nonexistent");
        assert!(check_previous_crash(&dir).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn check_previous_crash_removes_corrupt_blob() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("last-crash.bin"), b"not a GCRX blob")
            .unwrap();
        assert!(check_previous_crash(tmp.path()).is_none());
        assert!(
            !tmp.path().join("last-crash.bin").exists(),
            "corrupt blob must be removed, not left to poison every startup"
        );
    }

    #[cfg(unix)]
    #[test]
    fn archive_report_same_second_gets_sequence_suffix() {
        let tmp = tempfile::TempDir::new().unwrap();
        archive_report(tmp.path(), "first", 12345);
        archive_report(tmp.path(), "second", 12345);
        let history = tmp.path().join("history");
        assert_eq!(
            std::fs::read_to_string(history.join("crash-12345.txt")).unwrap(),
            "first",
            "first same-second report must survive"
        );
        assert_eq!(
            std::fs::read_to_string(history.join("crash-12345-1.txt")).unwrap(),
            "second",
            "second same-second report must get a sequence suffix"
        );
    }

    #[cfg(unix)]
    #[test]
    fn archive_report_prunes_only_crash_prefixed_by_mtime() {
        let tmp = tempfile::TempDir::new().unwrap();
        let history = tmp.path().join("history");
        std::fs::create_dir_all(&history).unwrap();
        // A non-crash file must never be pruned, even when
        // older than the crash-* reports.
        std::fs::write(history.join("keep-me.txt"), "keep").unwrap();
        for ts in 1..=(MAX_HISTORY + 5) {
            archive_report(tmp.path(), &format!("r{ts}"), ts as u64);
        }
        let entries: Vec<String> = std::fs::read_dir(&history)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            entries.contains(&"keep-me.txt".to_string()),
            "non-crash files must not be pruned"
        );
        let crash_files: Vec<String> = entries
            .into_iter()
            .filter(|n| n.starts_with("crash-"))
            .collect();
        assert_eq!(
            crash_files.len(),
            MAX_HISTORY,
            "only the newest MAX_HISTORY crash-* reports survive"
        );
    }
}
