//! Core tool configuration and type definitions.
//!
//! This module is the central hub for tool-related types, re-exporting
//! from submodules for convenient access.

// --- Submodule declarations ---
pub mod config_validation;
pub mod slash_commands;
pub mod tool;
pub mod tool_io;
pub mod output;
pub mod requirements;
pub mod resources;
pub mod tool_metadata;
pub mod template_renderer;
pub mod definition;
pub mod compat;
pub mod schema;
pub mod config_source;
pub mod memory_backend;
pub mod tool_index;
pub mod context;
pub mod agents_md_tracker;
pub mod skill_discovery_tracker;
pub mod api_key_provider;
pub mod process_manager;
pub mod session_mode;
pub mod description;
pub mod error;
pub mod params_validation;

// --- Re-exports from the deleted types.rs ---
pub use config_validation::{
    ToolConfigEntryError, ToolConfigEntryErrorKind, parse_params_json, validate_name_override,
};
pub use slash_commands::UPDATE_GOAL_TOOL_NAME;
pub use tool::{ToolNamespace, ToolKind, claude_names_for, qidi_names_for, kind_for};
pub use tool_io::ToolInput;
pub use tool_io::MCPToolInput;
pub use output::ToolOutput;
pub use api_key_provider::SharedApiKeyProvider;


// Additional re-exports for qidi-code compatibility
pub use crate::computer::types::KillOutcome;
pub use session_mode::SessionMode;
pub use compat::{CompatConfig, CompatConfigToml};
pub use api_key_provider::ApiKeyProvider;

/// A single tool configuration entry in the session bind metadata.
/// This mirrors the wire-format ToolConfigEntry from qidi-code-api.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfigEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_override: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub params_name_overrides: std::collections::HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description_override: Option<String>,
}

/// Capability mode for a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum CapabilityMode {
    #[default]
    ReadOnly,
    ReadWrite,
}

/// A runtime tool configuration (after wire-format parsing).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolConfig {
    pub id: cf_tool_protocol::ToolId,
    pub name_override: Option<String>,
    pub params: Option<serde_json::Map<String, serde_json::Value>>,
    pub behavior_version: Option<String>,
    pub description_override: Option<String>,
    pub kind: Option<CapabilityMode>,
}

impl ToolConfig {
    pub fn new(id: cf_tool_protocol::ToolId) -> Self {
        Self {
            id,
            name_override: None,
            params: None,
            behavior_version: None,
            description_override: None,
            kind: None,
        }
    }
}

/// Server-level tool configuration aggregation.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ToolServerConfig {
    pub tools: Vec<ToolConfig>,
    pub name_overrides: std::collections::HashMap<cf_tool_protocol::ToolId, String>,
}

impl ToolServerConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_entries(entries: &[ToolConfigEntry]) -> Result<Self, ToolConfigEntryError> {
        use config_validation::ToolConfigEntryError as CVE;
        let mut tools = Vec::new();
        let mut name_overrides = std::collections::HashMap::new();

        for (i, entry) in entries.iter().enumerate() {
            let id = cf_tool_protocol::ToolId::new(&entry.id).map_err(|e| CVE::new(
                i,
                entry.id.clone(),
                ToolConfigEntryErrorKind::NameOverrideInvalid {
                    name: entry.id.clone(),
                    error: e.to_string(),
                },
            ))?;

            let params = config_validation::parse_params_json(i, &entry.id, entry.params_json.as_deref())
                .map_err(|e| CVE { ..e })?;

            if let Some(ref no) = entry.name_override {
                name_overrides.insert(id.clone(), no.clone());
            }

            tools.push(ToolConfig {
                id,
                name_override: entry.name_override.clone(),
                params,
                behavior_version: entry.behavior_version.clone(),
                description_override: entry.description_override.clone(),
                kind: None,
            });
        }

        Ok(Self { tools, name_overrides })
    }
}

// Re-export register_resource macro from resources submodule

// Re-export TaskSnapshot from process_manager
pub use process_manager::TaskSnapshot;

// Re-export GrokIntegerSchema from schema
pub use schema::GrokIntegerSchema;
