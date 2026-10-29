//! Backstop release stamp for `folder_trust`.
//!
//! `cf_workspace::folder_trust::is_local_build` treats an unstamped binary as a
//! dev build and auto-trusts every folder. The product binary cannot stamp this
//! crate from its own build script (`rustc-env` is per-crate and this crate
//! compiles first), so any `--release` build that forgot `QIDI_VERSION` would
//! ship with the whole trust gate silently inert. Stamp it here instead.

fn main() {
    println!("cargo:rerun-if-env-changed=QIDI_VERSION");
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("QIDI_VERSION").is_ok() {
        return;
    }
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
        println!("cargo:rustc-env=QIDI_VERSION={version}");
    }
}
