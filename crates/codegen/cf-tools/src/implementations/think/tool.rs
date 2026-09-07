//! `think` tool — external reasoning scratchpad (`Tool` trait).
//!
//! Inspired by omp's externalThinking: when a provider's hidden reasoning
//! channel is unavailable (or disabled), the model can write its analysis
//! into this tool's parameters instead. Tool arguments are always visible
//! to the harness, so the reasoning becomes observable and persists in the
//! transcript like any other tool call.

use super::types::ThinkInput;
use super::THINK_TOOL_NAME;
use crate::types::output::ToolOutput;
use crate::types::tool::{ToolKind, ToolNamespace};

#[derive(Debug, Default)]
pub struct ThinkImpl;

impl crate::types::tool_metadata::ToolMetadata for ThinkImpl {
    fn kind(&self) -> ToolKind {
        ToolKind::Think
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::QidiBuild
    }

    fn description_template(&self) -> &str {
        "Record your internal reasoning before acting. Use this tool to think through \
         complex problems step by step BEFORE calling other tools or answering. In the \
         `thought` parameter, write your working analysis: restate the problem, \
         decompose it, list candidate approaches, weigh trade-offs, and settle on a \
         plan. The tool has no side effects and returns only an acknowledgement, so \
         call it freely whenever a structured scratchpad helps — especially when \
         hidden reasoning is unavailable, and for multi-step plans, tricky bugs, or \
         risky changes."
    }
}

impl cf_tool_runtime::Tool for ThinkImpl {
    type Args = ThinkInput;
    type Output = ToolOutput;

    fn id(&self) -> cf_tool_protocol::ToolId {
        cf_tool_protocol::ToolId::new(THINK_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::cf_tool_runtime::ListToolsContext,
    ) -> cf_tool_types::ToolDescription {
        cf_tool_types::ToolDescription::new(
            THINK_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::description_template(self),
        )
    }

    fn capabilities(&self) -> cf_tool_protocol::ToolCapabilities {
        cf_tool_protocol::ToolCapabilities {
            is_read_only: true,
            tool_scope: Some(cf_tool_protocol::ToolScope::Read),
            ..Default::default()
        }
    }

    async fn run(
        &self,
        _ctx: cf_tool_runtime::ToolCallContext,
        _input: ThinkInput,
    ) -> Result<ToolOutput, cf_tool_runtime::ToolError> {
        Ok(ToolOutput::Text("Thought recorded.".into()))
    }
}
