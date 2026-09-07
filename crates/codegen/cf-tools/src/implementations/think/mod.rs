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

    /// The constant is the wire identifier; pin it against typos.
    #[test]
    fn think_tool_constant_matches_registered_id() {
        assert_eq!(THINK_TOOL_NAME, "think");
        assert_eq!(
            cf_tool_runtime::Tool::id(&ThinkImpl).to_string(),
            THINK_TOOL_NAME
        );
    }
}
