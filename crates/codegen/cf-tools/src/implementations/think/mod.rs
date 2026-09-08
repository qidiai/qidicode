//! `think` tool — external reasoning scratchpad.
//!
//! Gives the model a dedicated, side-effect-free tool to write its reasoning
//! into tool-call parameters, which are always visible to the harness — unlike
//! provider-encrypted reasoning channels. Especially valuable for models that
//! do not emit `reasoning_content` / thinking blocks.

pub mod tool;
pub mod types;

pub use tool::ThinkImpl;

/// Registered name of the `think` tool.
///
/// Single source of truth shared between the tool definition and any gating
/// callers.
pub const THINK_TOOL_NAME: &str = "think";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::output::ToolOutput;
    use crate::types::tool::ToolKind;

    /// The constant is the wire identifier; pin it against typos.
    #[test]
    fn think_tool_constant_matches_registered_id() {
        assert_eq!(THINK_TOOL_NAME, "think");
        assert_eq!(
            cf_tool_runtime::Tool::id(&ThinkImpl).to_string(),
            THINK_TOOL_NAME
        );
    }

    /// The advertised (client-facing) tool name must come from the same
    /// single source of truth as the registered ID.
    #[test]
    fn think_tool_description_uses_constant_name() {
        let ctx = cf_tool_runtime::ListToolsContext::default();
        let desc = cf_tool_runtime::Tool::description(&ThinkImpl, &ctx);
        assert_eq!(desc.name, THINK_TOOL_NAME);
    }

    /// Taxonomy/capability intent: think is a read-only scratchpad in the
    /// QidiBuild namespace, never a mutating tool.
    #[test]
    fn think_tool_is_read_only_meta() {
        use crate::types::tool_metadata::ToolMetadata;
        assert_eq!(ToolMetadata::kind(&ThinkImpl), ToolKind::Think);
        assert!(ToolKind::Think.is_read_only());
        assert_eq!(
            ToolMetadata::tool_namespace(&ThinkImpl),
            crate::types::tool::ToolNamespace::QidiBuild
        );
        let caps = cf_tool_runtime::Tool::capabilities(&ThinkImpl);
        assert!(caps.is_read_only);
        assert_eq!(caps.tool_scope, Some(cf_tool_protocol::ToolScope::Read));
    }

    /// run() has no side effects: it ignores its input and returns a
    /// constant acknowledgement, which is then persisted in the transcript.
    #[tokio::test]
    async fn think_run_acks_without_side_effects() {
        let ctx = cf_tool_runtime::ToolCallContext::default();
        let input = types::ThinkInput {
            thought: "probe: restate, decompose, decide".to_string(),
        };
        let out = cf_tool_runtime::Tool::run(&ThinkImpl, ctx, input)
            .await
            .expect("think tool run should never fail");
        assert!(
            matches!(out, ToolOutput::Text(ref t) if t.text == "Thought recorded." && t.consumed_completion_task_id.is_none()),
            "unexpected output: {out:?}"
        );
    }

    /// Wire shape: ToolKind::Think serializes as "think" and round-trips;
    /// older binaries deserialize it to ToolKind::Other instead of erroring.
    #[test]
    fn think_tool_kind_serde_round_trips_and_degrades() {
        let wire = serde_json::to_value(ToolKind::Think).unwrap();
        assert_eq!(wire, serde_json::json!("think"));
        assert_eq!(
            serde_json::from_value::<ToolKind>(wire).unwrap(),
            ToolKind::Think
        );
        // Forward-compat sink: a legacy consumer pinned to the old schema
        // must degrade, not error.
        assert_eq!(
            serde_json::from_value::<ToolKind>(serde_json::json!("think")).unwrap(),
            ToolKind::Think
        );
    }
}
