pub mod auto_update;
mod minimum_version;
// B2: Ed25519 detached-signature verification of downloaded self-update binaries.
pub mod signing;
pub mod version;

pub use auto_update::UpdateStatus;
pub use minimum_version::enforce_minimum_version_or_exit;
pub use version::{UpdateConfig, channel_label, channel_name, write_version_cache};
