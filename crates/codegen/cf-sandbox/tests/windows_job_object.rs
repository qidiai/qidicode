//! B3: Windows Job Object sandbox integration tests.
//!
//! Only compiled/run on Windows with the `enforce` feature (the default),
//! where `SandboxManager::apply` creates a real Job Object.
#![cfg(all(windows, feature = "enforce"))]

use cf_sandbox::{ProfileName, SandboxManager};
use std::path::Path;

#[test]
fn job_object_created_and_capability_verified() {
    let mut mgr = SandboxManager::new(ProfileName::ReadOnly, Path::new("."));
    mgr.apply(Path::new(".")).expect("Job Object sandbox apply");
    // Windows contract (by design, see `apply`'s SECURITY note): the Job
    // Object is a startup capability probe only — no process is ever
    // attached to it, so `applied` stays false. Marking it applied would
    // make `is_active()` / `should_auto_allow_bash()` report a sandbox
    // that provides no filesystem/network isolation, falsely enabling YOLO
    // bash auto-approve. Unix/Landlock semantics (applied=true) do NOT
    // apply here.
    assert!(!mgr.is_applied());
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
    // Same Windows contract as above: the Job Object probe does not mark
    // the sandbox applied, so the manager-level network gate stays off —
    // but the process-global child-network restriction IS armed for the
    // ReadOnly profile (enforced by the surrounding nono/Job-Object
    // layer at process-group spawn time).
    assert!(!mgr.is_applied());
    assert!(!mgr.restrict_child_network());
    assert!(cf_sandbox::should_restrict_child_network());
}
