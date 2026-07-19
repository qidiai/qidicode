//! Shared utilities used by both `qidi-code` and its downstream clients
//! (e.g. `qidi-code-render`). This crate sits upstream of `qidi-code`
//! so it must never depend on it.

pub mod clipboard;
pub mod placeholder_images;
pub mod session;
pub mod stderr;
pub mod ui_config;
