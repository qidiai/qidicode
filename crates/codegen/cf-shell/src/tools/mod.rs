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

// Re-export key types from qidi-code for convenience
pub use self::todo::{TodoId, TodoItem, TodoPriority, TodoStatus};
pub use cf_tools::types::output::ToolOutput;
pub use cf_tools::types::{MCPToolInput, ToolInput};
