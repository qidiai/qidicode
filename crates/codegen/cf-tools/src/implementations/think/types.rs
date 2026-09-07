//! Input/output types for the think tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Input for the `think` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ThinkInput {
    /// Your step-by-step reasoning written as a working scratchpad:
    /// restate the problem, decompose it, list candidate approaches,
    /// weigh trade-offs, and settle on a plan. Dense shorthand is fine.
    pub thought: String,
}

/// Output schema for `think` (used for JSON Schema generation only).
#[derive(Debug, JsonSchema)]
pub struct ThinkOutput {
    /// Acknowledgement text.
    pub ack: String,
}
