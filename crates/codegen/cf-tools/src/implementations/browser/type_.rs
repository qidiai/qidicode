//! `browser_type` -- type text into a field, optionally submitting.

use crate::types::output::ToolOutput;
use crate::types::tool::{ToolKind, ToolNamespace};

use super::types::BrowserTypeInput;
use super::BROWSER_TYPE_TOOL_NAME;

#[derive(Debug, Default)]
pub struct TypeImpl;

impl crate::types::tool_metadata::ToolMetadata for TypeImpl {
    fn kind(&self) -> ToolKind {
        ToolKind::BrowserAct
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::QidiBuild
    }

    fn description_template(&self) -> &str {
        "Type text into a page field identified by a snapshot ref number or CSS selector. \
         Clears the field first by default; set submit=true to press Enter after typing \
         (search boxes, form submits). This acts on the live page."
    }
}

impl cf_tool_runtime::Tool for TypeImpl {
    type Args = BrowserTypeInput;
    type Output = ToolOutput;

    fn id(&self) -> cf_tool_protocol::ToolId {
        cf_tool_protocol::ToolId::new(BROWSER_TYPE_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::cf_tool_runtime::ListToolsContext,
    ) -> cf_tool_types::ToolDescription {
        cf_tool_types::ToolDescription::new(
            BROWSER_TYPE_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::description_template(self),
        )
    }

    fn capabilities(&self) -> cf_tool_protocol::ToolCapabilities {
        cf_tool_protocol::ToolCapabilities {
            is_read_only: false,
            tool_scope: Some(cf_tool_protocol::ToolScope::Write),
            ..Default::default()
        }
    }

    async fn run(
        &self,
        ctx: cf_tool_runtime::ToolCallContext,
        input: BrowserTypeInput,
    ) -> Result<ToolOutput, cf_tool_runtime::ToolError> {
        if input.text.is_empty() && !input.clear {
            return Err(cf_tool_runtime::ToolError::custom(
                "empty_type",
                "text is empty and clear=false: nothing would be typed",
            ));
        }
        let resources = crate::types::tool_metadata::shared_resources(&ctx)?;
        let backend = super::backend_from(&resources).await?;
        let result = backend
            .r#type(crate::computer::types::BrowserTypeRequest {
                selector: input.selector,
                text: input.text,
                submit: input.submit,
                clear: input.clear,
            })
            .await
            .map_err(super::browser_err)?;
        Ok(ToolOutput::Text(result.detail.into()))
    }
}