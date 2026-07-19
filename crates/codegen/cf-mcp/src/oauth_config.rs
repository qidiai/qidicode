//! OAuth configuration types for MCP servers.
//!
//! Constructed by the host's TOML parsing (`McpServerConfig::oauth_config`)
//! and consumed by [`crate::oauth`].
//!
//! This module re-exports `McpOAuthConfig` from cf-config to ensure a single
//! canonical type across the workspace.

pub use cf_config::xai_grok_config_types::McpOAuthConfig;

/// Per-server OAuth configuration map, keyed by MCP server name.
pub type McpOAuthConfigMap = std::collections::HashMap<String, McpOAuthConfig>;
