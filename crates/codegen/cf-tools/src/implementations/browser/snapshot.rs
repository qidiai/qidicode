//! `browser_snapshot` -- markdown body plus numbered interactive refs.

use crate::types::output::ToolOutput;
use crate::types::tool::{ToolKind, ToolNamespace};

use super::types::BrowserSnapshotInput;
use super::BROWSER_SNAPSHOT_TOOL_NAME;

#[derive(Debug, Default)]
pub struct SnapshotImpl;

impl crate::types::tool_metadata::ToolMetadata for SnapshotImpl {
    fn kind(&self) -> ToolKind {
        ToolKind::BrowserRead
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::QidiBuild
    }

    fn description_template(&self) -> &str {
        "Observe the current page: title, URL, a numbered list of interactive elements (links, \
         buttons, inputs), and the page content rendered as markdown. The numbers are refs -- \
         pass them as the selector to browser_click / browser_type. Refs go stale after \
         navigation; re-snapshot after any page change."
    }
}

impl cf_tool_runtime::Tool for SnapshotImpl {
    type Args = BrowserSnapshotInput;
    type Output = ToolOutput;

    fn id(&self) -> cf_tool_protocol::ToolId {
        cf_tool_protocol::ToolId::new(BROWSER_SNAPSHOT_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::cf_tool_runtime::ListToolsContext,
    ) -> cf_tool_types::ToolDescription {
        cf_tool_types::ToolDescription::new(
            BROWSER_SNAPSHOT_TOOL_NAME,
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
        input: BrowserSnapshotInput,
    ) -> Result<ToolOutput, cf_tool_runtime::ToolError> {
        let resources = crate::types::tool_metadata::shared_resources(&ctx)?;
        let backend = super::backend_from(&resources).await?;
        let snap = backend.snapshot().await.map_err(super::browser_err)?;
        let mut out = format!("# {}\nURL: {}\n", snap.title, snap.url);
        if !snap.refs.is_empty() {
            out.push_str("\n## Interactive elements\n");
            for r in &snap.refs {
                let text = if r.text.is_empty() { "-" } else { &r.text };
                out.push_str(&format!("- [{}] {}: {:?}\n", r.ref_id, r.role, text));
            }
        }
        if !input.refs_only {
            out.push_str("\n## Content\n");
            out.push_str(&snap.markdown);
        }
        Ok(ToolOutput::Text(out.into()))
    }
}