//! Tool configuration entry type, duplicated from cf-tools to avoid
//! circular dependency (cf-tools -> xai-tool-runtime -> cf-tools).
//!
//! This is a minimal subset used only for serialization/deserialization
//! of the `session.bind` wire format.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// A single tool configuration entry in the session bind metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfigEntry {
    /// Fully-qualified tool id, e.g. `cf_tools::grep`.
    #[serde(default)]
    pub id: String,
    /// Optional JSON string with tool parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params_json: Option<String>,
    /// Optional client-facing name override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_override: Option<String>,
    /// { canonical param -> client-facing param } overrides.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub params_name_overrides: HashMap<String, String>,
    /// Per-tool behavior version override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior_version: Option<String>,
    /// When `Some`, replaces the tool's description template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description_override: Option<String>,
}
