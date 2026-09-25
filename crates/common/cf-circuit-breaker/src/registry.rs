//! Per-key registry of [`CircuitBreaker`] instances (one per upstream
//! endpoint, one per tenant, etc.).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::breaker::CircuitBreaker;
use crate::config::BreakerConfig;

pub struct CircuitBreakerRegistry {
    config: BreakerConfig,
    breakers: Mutex<HashMap<String, Arc<CircuitBreaker>>>,
    /// Maximum number of distinct breakers (prevents unbounded memory
    /// growth from untrusted keys). Default: 256.
    max_breakers: usize,
}

/// Default cap on the number of distinct breaker keys tracked.
const DEFAULT_MAX_BREAKERS: usize = 256;
/// Maximum key length — keys longer than this are rejected to prevent
/// memory abuse from oversized keys.
const MAX_KEY_LEN: usize = 512;

impl CircuitBreakerRegistry {
    pub fn new(config: BreakerConfig) -> Self {
        Self {
            config,
            breakers: Mutex::new(HashMap::new()),
            max_breakers: DEFAULT_MAX_BREAKERS,
        }
    }

    /// Returns `None` if the registry's config has `enabled = false`;
    /// otherwise returns (and lazily creates) the breaker for `key`.
    ///
    /// SECURITY: Keys longer than 512 bytes are rejected to prevent memory
    /// abuse. The total number of breakers is capped at `max_breakers` to
    /// prevent unbounded growth from untrusted key sources.
    pub fn get(&self, key: &str) -> Option<Arc<CircuitBreaker>> {
        if !self.config.enabled {
            return None;
        }
        // Reject oversized keys to prevent memory abuse.
        if key.len() > MAX_KEY_LEN {
            tracing::warn!(
                key_len = key.len(),
                "circuit breaker key exceeds max length {}, rejecting",
                MAX_KEY_LEN
            );
            return None;
        }
        let mut breakers = self.breakers.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cb) = breakers.get(key) {
            return Some(Arc::clone(cb));
        }
        // Enforce max breaker count to prevent unbounded growth.
        if breakers.len() >= self.max_breakers {
            tracing::warn!(
                count = breakers.len(),
                max = self.max_breakers,
                "circuit breaker registry at capacity, not creating new breaker for key"
            );
            return None;
        }
        let cb = Arc::new(CircuitBreaker::new(self.config.clone()));
        let ret = Arc::clone(&cb);
        breakers.insert(key.to_owned(), cb);
        Some(ret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_returns_none_when_disabled() {
        let cfg = BreakerConfig {
            enabled: false,
            ..Default::default()
        };
        let reg = CircuitBreakerRegistry::new(cfg);
        assert!(reg.get("endpoint-a").is_none());
    }

    #[test]
    fn registry_returns_same_breaker_for_same_key() {
        let reg = CircuitBreakerRegistry::new(BreakerConfig::default());
        let a = reg.get("endpoint-a").unwrap();
        let b = reg.get("endpoint-a").unwrap();
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn registry_returns_distinct_breakers_for_distinct_keys() {
        let reg = CircuitBreakerRegistry::new(BreakerConfig::default());
        let a = reg.get("endpoint-a").unwrap();
        let b = reg.get("endpoint-b").unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
    }
}
