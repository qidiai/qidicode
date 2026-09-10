//! `browser_navigate` -- load a URL in the shared browser page.

use crate::types::output::ToolOutput;
use crate::types::tool::{ToolKind, ToolNamespace};

use super::types::BrowserNavigateInput;
use super::BROWSER_NAVIGATE_TOOL_NAME;

#[derive(Debug, Default)]
pub struct NavigateImpl;

impl crate::types::tool_metadata::ToolMetadata for NavigateImpl {
    fn kind(&self) -> ToolKind {
        ToolKind::BrowserRead
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::QidiBuild
    }

    fn description_template(&self) -> &str {
        "Load a URL in the built-in headless browser and return the final URL and page title. \
         The browser session persists across tool calls: navigate, then use browser_snapshot to \
         see the page as markdown with numbered interactive elements, browser_click / \
         browser_type to act on them, and browser_read to extract text. Prefer this over \
         web_fetch for JavaScript-heavy pages that need rendering."
    }
}

impl cf_tool_runtime::Tool for NavigateImpl {
    type Args = BrowserNavigateInput;
    type Output = ToolOutput;

    fn id(&self) -> cf_tool_protocol::ToolId {
        cf_tool_protocol::ToolId::new(BROWSER_NAVIGATE_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::cf_tool_runtime::ListToolsContext,
    ) -> cf_tool_types::ToolDescription {
        cf_tool_types::ToolDescription::new(
            BROWSER_NAVIGATE_TOOL_NAME,
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
        input: BrowserNavigateInput,
    ) -> Result<ToolOutput, cf_tool_runtime::ToolError> {
        let url = input.url.trim().to_owned();
        let lowered = url.to_ascii_lowercase();
        let scheme_ok = ["http://", "https://", "about:", "data:", "file:"]
            .iter()
            .any(|s| lowered.starts_with(s));
        if !scheme_ok {
            return Err(cf_tool_runtime::ToolError::custom(
                "invalid_url",
                format!(
                    "url must be absolute with an http/https/about/data/file scheme, got `{url}`"
                ),
            ));
        }
        let resources = crate::types::tool_metadata::shared_resources(&ctx)?;
        let backend = super::backend_from(&resources).await?;
        let result = backend
            .navigate(crate::computer::types::BrowserNavigateRequest {
                url,
                wait_ms: input.wait_ms,
            })
            .await
            .map_err(super::browser_err)?;
        Ok(ToolOutput::Text(
            format!("Loaded: {}\nTitle: {}", result.final_url, result.title).into(),
        ))
    }
}