//! Tool infrastructure for qidi-code.
//!
//! All tool execution goes through `qidi-code` via the `ToolBridge`.
//! Types (ToolOutput, ToolInput, TodoState, etc.) come from `qidi-code` directly.

pub mod bridge;
pub mod config;
pub mod notification_bridge;
pub mod retry;
pub mod todo;
pub mod tool_context;

pub use self::{
    config::{BashToolConfig, FileToolset, ShellToolsetConfig},
    retry::{RetryConfig, execute_with_retry},
    tool_context::ToolContext,
};

/// Startup probe for the built-in grep's `rg` binary.
///
/// Emits a one-time warning when ripgrep is missing or not executable, so
/// "grep returns nothing" is explained up front instead of being mistaken
/// for "no matches". Re-exported through cf-shell so the binary crate does
/// not need a direct cf-tools dependency.
pub fn warn_if_ripgrep_unavailable() {
    cf_tools::implementations::qidi_build::grep::ripgrep::warn_if_ripgrep_unavailable();
}

// Re-export key types from qidi-code for convenience
pub use self::todo::{TodoId, TodoItem, TodoPriority, TodoStatus};
pub use cf_tools::types::output::ToolOutput;
pub use cf_tools::types::{MCPToolInput, ToolInput};
