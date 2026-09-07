//! Input types for the think tool.
//!
//! The tool's output is a plain `ToolOutput::Text` acknowledgement, so no
//! dedicated output type is needed.

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

