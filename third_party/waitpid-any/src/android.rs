//! Android (bionic) implementation.
//!
//! Neither of the other UNIX backends is usable on Android: bionic has no
//! `kqueue(2)` (so `bsd.rs` cannot be used), and `pidfd_open(2)` is not
//! exposed by the `rustix` crate for `target_os = "android"` (so `linux.rs`
//! cannot be used either).
//!
//! Instead, the exit of the target process is detected by polling the
//! `/proc/<pid>` directory, which is present on Android and readable for
//! processes of the same user. Deliberately, `waitpid(2)` is *not* used: it
//! would reap the child and thus steal the exit status from the caller, which
//! is the owner of the process.

use std::io::{Error, ErrorKind, Result};
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Interval between two `/proc/<pid>` existence checks while waiting.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// A handle to an arbitrary process, tracked by its PID.
///
/// Unlike the `pidfd` based handles this handle does not pin the process
/// entity, so a PID recycled after the target exited and got reaped may be
/// observed as "not exited" again. Open and wait on the handle as early as
/// possible to mitigate this, as documented on
/// [`WaitHandle::open`](crate::WaitHandle::open).
///
/// Once the target has been observed to be gone, the handle fuses: every
/// later [`wait`] call returns `Ok(Some(()))` immediately, mirroring the
/// behaviour of the *BSD backend where `kqueue` only emits the event once.
#[derive(Debug)]
pub struct WaitHandle {
    /// The process ID this handle watches.
    pid: i32,
    /// Set once the target has been observed to be gone.
    exited: bool,
}

/// Checks whether the process with `pid` is no longer present in `/proc`.
fn is_gone(pid: i32) -> bool {
    Path::new("/proc").join(pid.to_string()).metadata().is_err()
}

/// Opens a handle to the process with the given PID.
///
/// Returns `InvalidInput` if `pid` is not a positive process ID. This function
/// does not block, does not signal, and does not reap the target process.
pub fn open(pid: i32) -> Result<WaitHandle> {
    if pid <= 0 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("invalid PID {pid}"),
        ));
    }
    Ok(WaitHandle {
        pid,
        exited: false,
    })
}

/// Waits until the target process is gone, or until `timeout` elapses.
///
/// Returns `Ok(Some(()))` once the target is gone (immediately, if it already
/// is), and `Ok(None)` if `timeout` elapsed first. With `timeout` set to
/// `None` this waits indefinitely.
///
/// A process that exited but has not been reaped yet still has a
/// `/proc/<pid>` entry, so in that state this function keeps waiting until the
/// owner reaps it. This function never reaps the process itself and never
/// returns an error.
pub fn wait(handle: &mut WaitHandle, timeout: Option<Duration>) -> Result<Option<()>> {
    if handle.exited {
        return Ok(Some(()));
    }

    let deadline = match timeout {
        Some(dur) => match Instant::now().checked_add(dur) {
            Some(deadline) => Some(deadline),
            // A timeout too large to be represented is treated as no timeout.
            None => None,
        },
        None => None,
    };

    loop {
        if is_gone(handle.pid) {
            // Process exited. Fuse the handle.
            handle.exited = true;
            return Ok(Some(()));
        }

        let sleep_for = match deadline {
            Some(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    // Timeout. Re-check once more on the next call.
                    return Ok(None);
                }
                (deadline - now).min(POLL_INTERVAL)
            }
            None => POLL_INTERVAL,
        };
        sleep(sleep_for);
    }
}
