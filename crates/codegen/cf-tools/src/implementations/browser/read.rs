//! `browser_read` -- page markdown or one element's inner text.

use crate::types::output::ToolOutput;
use crate::types::tool::{ToolKind, ToolNamespace};

use super::types::BrowserReadInput;
use super::BROWSER_READ_TOOL_NAME;

#[derive(Debug, Default)]
pub struct ReadImpl;

impl crate::types::tool_metadata::ToolMetadata for ReadImpl {
    fn kind(&self) -> ToolKind {
        ToolKind::BrowserRead
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::QidiBuild
    }

    fn description_template(&self) -> &str {
        "Read text from the current page: the whole page rendered as markdown (no selector) or \
         one element's inner text (CSS selector). Use after browser_navigate; unlike \
         browser_snapshot this returns content only, no interactive-element listing."
    }
}

impl cf_tool_runtime::Tool for ReadImpl {
    type Args = BrowserReadInput;
    type Output = ToolOutput;

    fn id(&self) -> cf_tool_protocol::ToolId {
        cf_tool_protocol::ToolId::new(BROWSER_READ_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::cf_tool_runtime::ListToolsContext,
    ) -> cf_tool_types::ToolDescription {
        cf_tool_types::ToolDescription::new(
            BROWSER_READ_TOOL_NAME,
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
        ctx: cf_tool_runtime::ToolCallContext,
        input: BrowserReadInput,
    ) -> Result<ToolOutput, cf_tool_runtime::ToolError> {
        let resources = crate::types::tool_metadata::shared_resources(&ctx)?;
        let backend = super::backend_from(&resources).await?;
        let result = backend
            .read(crate::computer::types::BrowserReadRequest {
                selector: input.selector,
            })
            .await
            .map_err(super::browser_err)?;
        Ok(ToolOutput::Text(result.content.into()))
    }
}