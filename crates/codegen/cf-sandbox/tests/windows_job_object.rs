//! B3: Windows Job Object sandbox integration tests.
//!
//! Only compiled/run on Windows with the `enforce` feature (the default),
//! where `SandboxManager::apply` creates a real Job Object.
#![cfg(all(windows, feature = "enforce"))]

use cf_sandbox::{ProfileName, SandboxManager};
use std::path::Path;

#[test]
fn job_object_created_and_applied() {
    let mut mgr = SandboxManager::new(ProfileName::ReadOnly, Path::new("."));
    mgr.apply(Path::new(".")).expect("Job Object sandbox apply");
    assert!(mgr.is_applied());
}

#[test]
fn off_profile_is_not_applied() {
    let mut mgr = SandboxManager::new(ProfileName::Off, Path::new("."));
    mgr.apply(Path::new(".")).expect("apply with profile off");
    assert!(!mgr.is_applied());
}

#[test]
fn readonly_profile_restricts_child_network() {
    let mut mgr = SandboxManager::new(ProfileName::ReadOnly, Path::new("."));
    mgr.apply(Path::new(".")).expect("Job Object sandbox apply");
    assert!(mgr.is_applied());
    assert!(mgr.restrict_child_network());
}
