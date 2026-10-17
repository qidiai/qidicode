//! Connection rate limiting + failed-auth lockout for `qidi agent serve`.
//!
//! Two independent guards, both parameterised by constants in this one module
//! (single source of truth):
//!
//! 1. **Connection rate limit** — anti-flood: rejects a burst of connection
//!    attempts above [`MAX_CONNECTIONS_PER_WINDOW`] within
//!    [`CONNECTION_WINDOW_SECS`]. Deliberately generous so a weak-network
//!    mobile client that reconnects a handful of times is never throttled.
//! 2. **Failed-auth lockout** — anti-brute-force: after
//!    [`MAX_FAILED_AUTH_PER_WINDOW`] *invalid-secret* attempts within
//!    [`FAILED_AUTH_WINDOW_SECS`], the peer is locked out for
//!    [`LOCKOUT_DURATION_SECS`].
//!
//! The key weak-network invariant (R-009): the failed-auth lockout counts
//! **only invalid-secret attempts**. A client that reconnects repeatedly with a
//! *valid* secret (dropped Wi-Fi, half-open sockets) never accumulates failures,
//! so it can never be locked out — the two code paths are orthogonal and the
//! pure decision functions below make that testable without a live socket.

use std::sync::Mutex;
use std::time::Instant;

/// Window over which failed-auth attempts are counted (seconds).
pub const FAILED_AUTH_WINDOW_SECS: u64 = 60;
/// Failed-auth attempts within the window that trigger a lockout.
pub const MAX_FAILED_AUTH_PER_WINDOW: usize = 10;
/// How long a locked-out peer is rejected (seconds).
pub const LOCKOUT_DURATION_SECS: u64 = 300;
/// Connection attempts tolerated within the connection window before throttling.
pub const MAX_CONNECTIONS_PER_WINDOW: usize = 300;
/// Window over which connection attempts are counted (seconds).
pub const CONNECTION_WINDOW_SECS: u64 = 10;

const MS_PER_SEC: u64 = 1000;

/// Monotonic milliseconds since process start (stable within a run).
fn now_ms() -> u64 {
    use std::sync::OnceLock;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Pure: should a peer with `failure_times_ms` (monotonic ms) be locked out at
/// `now_ms`?
pub fn failed_auth_locked_out(failure_times_ms: &[u64], now_ms: u64) -> bool {
    let window_ms = FAILED_AUTH_WINDOW_SECS * MS_PER_SEC;
    let recent = failure_times_ms
        .iter()
        .filter(|&&t| now_ms.saturating_sub(t) < window_ms)
        .count();
    recent >= MAX_FAILED_AUTH_PER_WINDOW
}

/// Pure: has a peer exceeded the connection rate limit given its attempt
/// timestamps?
pub fn connection_rate_limited(attempt_times_ms: &[u64], now_ms: u64) -> bool {
    let window_ms = CONNECTION_WINDOW_SECS * MS_PER_SEC;
    let recent = attempt_times_ms
        .iter()
        .filter(|&&t| now_ms.saturating_sub(t) < window_ms)
        .count();
    recent >= MAX_CONNECTIONS_PER_WINDOW
}

/// Thread-safe tracker used by the WebSocket handler.
///
/// Records every connection attempt and every failed auth; prunes old entries
/// lazily on record so the in-memory vectors stay bounded.
#[derive(Debug, Default)]
pub struct AuthRateLimiter {
    attempts: Mutex<Vec<u64>>,
    failures: Mutex<Vec<u64>>,
    locked_until_ms: Mutex<Option<u64>>,
}

impl AuthRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    fn prune(times: &mut Vec<u64>, window_ms: u64, now_ms: u64) {
        times.retain(|&t| now_ms.saturating_sub(t) < window_ms);
    }

    /// Record one connection attempt.
    pub fn record_attempt(&self) {
        let now = now_ms();
        let mut attempts = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        Self::prune(&mut attempts, CONNECTION_WINDOW_SECS * MS_PER_SEC, now);
        attempts.push(now);
    }

    /// Record one failed auth (invalid secret).
    ///
    /// Only records the timestamp — it does **not** by itself engage the
    /// lockout. The lockout is decided by [`Self::is_locked_out`] once the
    /// failure count crosses [`MAX_FAILED_AUTH_PER_WINDOW`], so a single typo
    /// never bricks the peer for [`LOCKOUT_DURATION_SECS`].
    pub fn record_failure(&self) {
        let now = now_ms();
        let mut failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        Self::prune(&mut failures, FAILED_AUTH_WINDOW_SECS * MS_PER_SEC, now);
        failures.push(now);
    }

    /// True when the peer is currently locked out (failed-auth lockout active).
    ///
    /// Fast path: if a lockout horizon is already set and still in the future,
    /// return true. Otherwise re-evaluate the failure window; on crossing the
    /// threshold, arm a fresh [`LOCKOUT_DURATION_SECS`] horizon so a peer that
    /// stops guessing still stays locked for the full cooldown.
    pub fn is_locked_out(&self) -> bool {
        let now = now_ms();
        {
            let mut locked = self.locked_until_ms.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(until) = *locked {
                if now < until {
                    return true;
                }
                *locked = None;
            }
        }
        let failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        let locked = failed_auth_locked_out(&failures, now);
        if locked {
            let mut horizon = self.locked_until_ms.lock().unwrap_or_else(|e| e.into_inner());
            *horizon = Some(now + LOCKOUT_DURATION_SECS * MS_PER_SEC);
        }
        locked
    }

    /// True when the connection attempt rate limit is exceeded. Callers should
    /// `record_attempt` first, then query this.
    pub fn connection_throttled(&self) -> bool {
        let now = now_ms();
        let attempts = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        connection_rate_limited(&attempts, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_auth_locked_out_empty_is_not_locked() {
        assert!(!failed_auth_locked_out(&[], 0));
    }

    #[test]
    fn failed_auth_locked_out_threshold() {
        // 9 failures in window → not locked; 10 → locked.
        let now = 100_000;
        let recent: Vec<u64> = (0..MAX_FAILED_AUTH_PER_WINDOW - 1)
            .map(|i| now - i as u64)
            .collect();
        assert!(!failed_auth_locked_out(&recent, now));

        let mut at_threshold = recent;
        at_threshold.push(now);
        assert!(failed_auth_locked_out(&at_threshold, now));
    }

    #[test]
    fn failed_auth_locked_out_ignores_stale_failures() {
        // A single failure outside the window must not count.
        let now = 1_000_000;
        let stale = vec![now - (FAILED_AUTH_WINDOW_SECS * MS_PER_SEC + 1)];
        assert!(!failed_auth_locked_out(&stale, now));
    }

    #[test]
    fn connection_rate_limited_threshold() {
        let now = 50_000;
        let below: Vec<u64> = (0..MAX_CONNECTIONS_PER_WINDOW - 1)
            .map(|i| now - i as u64)
            .collect();
        assert!(!connection_rate_limited(&below, now));

        let mut at = below;
        at.push(now);
        assert!(connection_rate_limited(&at, now));
    }

    /// R-009: a weak-network client reconnects repeatedly with a **valid**
    /// secret. Those reconnects record zero failures, so the failed-auth lockout
    /// can never engage — the two guards are orthogonal.
    #[test]
    fn weak_network_reconnects_do_not_lock_out() {
        let limiter = AuthRateLimiter::new();
        // Simulate 5 reconnects (mobile backoff 1/2/4/8/16s) with a valid secret.
        for _ in 0..5 {
            limiter.record_attempt(); // valid secret → no record_failure
        }
        assert!(!limiter.is_locked_out());
        // 5 attempts over ~30s is far below the connection window cap.
        assert!(!limiter.connection_throttled());
    }

    #[test]
    fn brute_force_locks_out() {
        let limiter = AuthRateLimiter::new();
        for _ in 0..MAX_FAILED_AUTH_PER_WINDOW {
            limiter.record_failure();
        }
        assert!(limiter.is_locked_out());
    }
}
