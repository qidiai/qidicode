//! `browser_click` -- click a snapshot ref or CSS selector.

use crate::types::output::ToolOutput;
use crate::types::tool::{ToolKind, ToolNamespace};

use super::types::BrowserClickInput;
use super::BROWSER_CLICK_TOOL_NAME;

#[derive(Debug, Default)]
pub struct ClickImpl;

impl crate::types::tool_metadata::ToolMetadata for ClickImpl {
    fn kind(&self) -> ToolKind {
        ToolKind::BrowserAct
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::QidiBuild
    }

    fn description_template(&self) -> &str {
        "Click an element on the current page. Pass the element's snapshot ref number from \
         browser_snapshot (preferred) or a CSS selector. This acts on the live page -- it can \
         submit forms, trigger navigation, or change site state. Re-snapshot afterwards if the \
         page changed."
    }
}

impl cf_tool_runtime::Tool for ClickImpl {
    type Args = BrowserClickInput;
    type Output = ToolOutput;

    fn id(&self) -> cf_tool_protocol::ToolId {
        cf_tool_protocol::ToolId::new(BROWSER_CLICK_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::cf_tool_runtime::ListToolsContext,
    ) -> cf_tool_types::ToolDescription {
        cf_tool_types::ToolDescription::new(
            BROWSER_CLICK_TOOL_NAME,
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
        input: BrowserClickInput,
    ) -> Result<ToolOutput, cf_tool_runtime::ToolError> {
        let resources = crate::types::tool_metadata::shared_resources(&ctx)?;
        let backend = super::backend_from(&resources).await?;
        let result = backend
            .click(crate::computer::types::BrowserClickRequest {
                selector: input.selector,
            })
            .await
            .map_err(super::browser_err)?;
        Ok(ToolOutput::Text(result.detail.into()))
    }
}