//! Proactive client-side rate limiter for outbound LLM API requests.
//!
//! Uses a simple token-bucket algorithm: the bucket starts full at `capacity`
//! tokens and refills at 1 token per `refill_interval`. Each API request
//! consumes one token; if the bucket is empty, the caller sleeps until a token
//! is available rather than rejecting the request.
//!
//! This complements the reactive 429/Retry-After handling in `retry.rs`:
//! - Reactive: server says "slow down" → back off after the fact.
//! - Proactive (this module): client self-throttles to avoid hitting 429s.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Default capacity (burst allowance): 10 requests.
pub const DEFAULT_RATE_LIMIT_CAPACITY: u32 = 10;

/// Default refill interval: 1 token per 2 seconds (30 RPM sustained).
pub const DEFAULT_RATE_LIMIT_REFILL_MS: u64 = 2000;

/// Token-bucket rate limiter. Thread-safe via `Mutex`.
pub struct ApiRateLimiter {
    inner: Mutex<TokenBucket>,
}

struct TokenBucket {
    capacity: u32,
    tokens: u32,
    refill_interval: Duration,
    last_refill: Instant,
}

impl ApiRateLimiter {
    pub fn new(capacity: u32, refill_interval_ms: u64) -> Self {
        Self {
            inner: Mutex::new(TokenBucket {
                capacity,
                tokens: capacity,
                refill_interval: Duration::from_millis(refill_interval_ms),
                last_refill: Instant::now(),
            }),
        }
    }

    /// Consume one token, blocking (sleeping) until one is available.
    /// Call this before every outbound API request.
    pub async fn acquire(&self) {
        loop {
            let wait = {
                let mut bucket = self.inner.lock().expect("rate limiter mutex poisoned");
                let now = Instant::now();
                let elapsed = now.duration_since(bucket.last_refill);
                let refills = (elapsed.as_millis() / bucket.refill_interval.as_millis()) as u32;
                if refills > 0 {
                    bucket.tokens = bucket.tokens.saturating_add(refills).min(bucket.capacity);
                    let refill_step = bucket.refill_interval * refills;
                    bucket.last_refill += refill_step;
                }
                if bucket.tokens > 0 {
                    bucket.tokens -= 1;
                    return;
                }
                // Calculate how long until the next token refills.
                let since_last = now.duration_since(bucket.last_refill);
                bucket.refill_interval.saturating_sub(since_last)
            };
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
        }
    }
}

impl Default for ApiRateLimiter {
    fn default() -> Self {
        Self::new(DEFAULT_RATE_LIMIT_CAPACITY, DEFAULT_RATE_LIMIT_REFILL_MS)
    }
}

impl std::fmt::Debug for ApiRateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bucket = self.inner.lock().expect("rate limiter mutex poisoned");
        f.debug_struct("ApiRateLimiter")
            .field("capacity", &bucket.capacity)
            .field("tokens", &bucket.tokens)
            .field("refill_interval_ms", &bucket.refill_interval.as_millis())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn acquire_consumes_tokens() {
        let rl = ApiRateLimiter::new(3, 100);
        // First 3 should be instant
        rl.acquire().await;
        rl.acquire().await;
        rl.acquire().await;
        // 4th should need to wait for refill
        let start = Instant::now();
        rl.acquire().await;
        // Should have waited ~100ms
        assert!(start.elapsed() >= Duration::from_millis(80));
    }

    #[tokio::test]
    async fn default_creates_valid_limiter() {
        let rl = ApiRateLimiter::default();
        rl.acquire().await; // should not block
    }
}
