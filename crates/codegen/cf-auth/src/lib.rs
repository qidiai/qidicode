//! Auth dependency-inversion seam shared between `cf-file-utils`
//! (the holder) and `cf-shell` (the implementer). Keeps shell types
//! out of the downstream HTTP-client crates' import graphs
//! (`cf-sampler`, `cf-tools`) while still letting refresh-aware
//! token resolution drive HTTP requests.

// Lint hygiene gate (matches upstream): this crate hands out
// bearer fragments and credential snapshots, so an accidental
// byte/char-index slice on a credential would be a security-
// relevant bug. 0 indexing_slicing violations at time of gating
// (2026-10-06: `cargo clippy -p cf-auth --all-targets` clean for
// this lint; the 16 pre-existing unwrap/expect warnings in
// retry_middleware's test module are unrelated to this gate and
// predate it).
#![deny(clippy::indexing_slicing)]

pub mod auth_provider;
pub mod bearer_fragment;
#[cfg(feature = "middleware")]
pub mod retry_middleware;
pub mod visibility;

pub use auth_provider::{AuthCredentialProvider, CredentialSnapshot, StaticAuthCredentialProvider};
pub use bearer_fragment::{BEARER_SUFFIX_LEN, bearer_suffix};
#[cfg(feature = "middleware")]
pub use retry_middleware::{AuthRetryMiddleware, StampedBearerSuffix, execute_with_stamp};
pub use visibility::HttpAuth;
