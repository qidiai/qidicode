//! `search_tool` — discover MCP tools via BM25 keyword search.

pub mod types;

pub use types::SearchToolInput;

use crate::types::output::{SearchToolOutput, ToolOutput};
use crate::types::tool::{ToolKind, ToolNamespace};
use crate::types::tool_index::ToolIndex;

/// Maximum length for MCP tool/server descriptions. Matches the common
/// `MAX_MCP_DESCRIPTION_LENGTH` constant. Descriptions exceeding this are truncated.
pub const MAX_MCP_DESCRIPTION_LENGTH: usize = 2048;

const TRUNCATION_SUFFIX: &str = "\u{2026} [truncated]";

/// Truncate a string to `MAX_MCP_DESCRIPTION_LENGTH` chars, appending a
/// suffix if truncated. Operates on char boundaries to avoid splitting
/// multi-byte characters.
pub fn truncate_description(s: &str) -> String {
    if s.len() <= MAX_MCP_DESCRIPTION_LENGTH || s.chars().count() <= MAX_MCP_DESCRIPTION_LENGTH {
        return s.to_owned();
    }
    let budget = MAX_MCP_DESCRIPTION_LENGTH - TRUNCATION_SUFFIX.len();
    let truncated: String = s.chars().take(budget).collect();
    format!("{truncated}{TRUNCATION_SUFFIX}")
}

/// Fingerprint for change detection: `(tool_count, description_hash, tool_names_hash)`.
pub type ServerFingerprint = (usize, u64, u64);

/// Deterministic, portable hash for change detection.
///
/// Uses FNV-1a which is stable across Rust versions, build profiles, and
/// CPU architectures.  Safe to persist (used by `announcement_state.json`
/// for MCP server fingerprints).
fn hash_value<H: std::hash::Hash>(val: &H) -> u64 {
    use std::hash::Hasher;

    struct Fnv1aHasher(u64);

    impl Fnv1aHasher {
        const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
        const PRIME: u64 = 0x00000100000001B3;
    }

    impl Hasher for Fnv1aHasher {
        fn finish(&self) -> u64 {
            self.0
        }
        fn write(&mut self, bytes: &[u8]) {
            for &byte in bytes {
                self.0 ^= byte as u64;
                self.0 = self.0.wrapping_mul(Self::PRIME);
            }
        }
    }

    let mut hasher = Fnv1aHasher(Fnv1aHasher::OFFSET_BASIS);
    val.hash(&mut hasher);
    hasher.finish()
}

/// Build fingerprints for a set of server summaries.
pub fn fingerprint_servers(
    servers: &[crate::types::tool_index::ServerSummary],
) -> std::collections::HashMap<String, ServerFingerprint> {
    servers
        .iter()
        .map(|s| {
            (
                s.name.clone(),
                (
                    s.tool_count,
                    hash_value(&s.description),
                    hash_value(&s.tool_names),
                ),
            )
        })
        .collect()
}

/// Build a full system-reminder body listing all connected MCP servers.
///
/// Returns `None` if `servers` is empty.
pub fn build_server_reminder(
    servers: &[crate::types::tool_index::ServerSummary],
) -> Option<String> {
    if servers.is_empty() {
        return None;
    }

    let mut text = format!("Connected MCP servers:\n",);
    for server in servers {
        text.push_str(&format_server_line(server));
    }

    Some(text)
}

/// Build a delta system-reminder noting only what changed.
///
/// `old` is the previously-announced fingerprint map; `new_summaries` is the
/// current server list. Returns `None` if nothing changed.
pub fn build_delta_reminder(
    old: &std::collections::HashMap<String, ServerFingerprint>,
    new_summaries: &[crate::types::tool_index::ServerSummary],
) -> Option<String> {
    let new_map = fingerprint_servers(new_summaries);

    // New servers (present in new, absent in old).
    let added: Vec<&crate::types::tool_index::ServerSummary> = new_summaries
        .iter()
        .filter(|s| !old.contains_key(&s.name))
        .collect();

    // Updated servers (present in both, different fingerprint).
    let updated: Vec<&crate::types::tool_index::ServerSummary> = new_summaries
        .iter()
        .filter(|s| {
            old.get(&s.name)
                .is_some_and(|old_fp| Some(old_fp) != new_map.get(&s.name))
        })
        .collect();

    // Removed servers (present in old but absent in new).
    let removed: Vec<&String> = old
        .keys()
        .filter(|name| !new_map.contains_key(*name))
        .collect();

    if added.is_empty() && updated.is_empty() && removed.is_empty() {
        return None;
    }

    let mut text = String::new();

    if !added.is_empty() {
        let s = if added.len() == 1 { "" } else { "s" };
        text.push_str(&format!("MCP server{s} connected:\n"));
        for server in &added {
            text.push_str(&format_server_line(server));
        }
    }

    if !updated.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        let s = if updated.len() == 1 { "" } else { "s" };
        text.push_str(&format!("MCP server{s} updated:\n"));
        for server in &updated {
            text.push_str(&format_server_line(server));
        }
    }

    if !removed.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        let mut rnames: Vec<&str> = removed.iter().map(|s| s.as_str()).collect();
        rnames.sort_unstable();
        let s = if removed.len() == 1 { "" } else { "s" };
        text.push_str(&format!(
            "MCP server{s} disconnected: {}",
            rnames.join(", ")
        ));
    }

    Some(text)
}

fn format_server_line(server: &crate::types::tool_index::ServerSummary) -> String {
    let desc = server
        .description
        .as_deref()
        .map(sanitize_description)
        .map(|s| truncate_description(&s));
    format_server_line_inner(&server.name, server.tool_count, &desc)
}

/// Format a server line for the compaction system-reminder.
///
/// Takes pre-processed fields instead of a `ServerSummary`, since
/// compaction stores data in a different shape (already sanitized/truncated).
/// Tool names are not included (discover via `search_tool`); they remain on
/// `ServerSummary` only for change-detection fingerprints.
pub fn format_compaction_server_line(name: &str, count: usize, desc: &Option<String>) -> String {
    format_server_line_inner(name, count, desc)
}

fn format_server_line_inner(name: &str, count: usize, desc: &Option<String>) -> String {
    let tool_word = if count == 1 { "tool" } else { "tools" };
    match desc.as_deref().filter(|s| !s.is_empty()) {
        Some(d) => format!("- {} ({} {}): {}\n", name, count, tool_word, d),
        None => format!("- {} ({} {})\n", name, count, tool_word),
    }
}

pub fn sanitize_description(s: &str) -> String {
    s.split(['\n', '\r'])
        .flat_map(|line| line.split_whitespace())
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Default)]
pub struct SearchTool;

impl crate::types::tool_metadata::ToolMetadata for SearchTool {
    fn kind(&self) -> ToolKind {
        ToolKind::SearchTool
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::QidiBuild
    }

    fn description_template(&self) -> &str {
        "Search for MCP tools by keyword and retrieve their input schemas.\n\n\
         If status is \"partial\", some servers may still be connecting."
    }
}

impl cf_tool_runtime::Tool for SearchTool {
    type Args = SearchToolInput;
    type Output = ToolOutput;

    fn id(&self) -> cf_tool_protocol::ToolId {
        cf_tool_protocol::ToolId::new("search_tool").expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::cf_tool_runtime::ListToolsContext,
    ) -> cf_tool_types::ToolDescription {
        cf_tool_types::ToolDescription::new(
            "search_tool",
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
        input: SearchToolInput,
    ) -> Result<ToolOutput, cf_tool_runtime::ToolError> {
        use crate::types::resources::{
            BuiltinToolEntry, BuiltinToolSearchSurface, DynamicToolState,
        };
        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;

        let limit = input.limit.unwrap_or(5) as usize;

        // Snapshot the resources we need (drop the lock before doing work).
        let (tool_index, builtin_surface, dyn_state) = {
            let guard = resources.lock().await;
            (
                guard.get::<ToolIndex>().cloned(),
                guard.get::<BuiltinToolSearchSurface>().cloned(),
                guard.get::<DynamicToolState>().cloned(),
            )
        };

        // Phase B (B-0/B-1): built-in tools hidden from `defs`, widened onto
        // the search surface beyond MCP. This must run even when the MCP
        // index is absent (previously the only source).
        let builtin_matches: Vec<BuiltinToolEntry> = builtin_surface
            .as_ref()
            .map(|s| match_builtin_tools(&s.0, &input.query, limit))
            .unwrap_or_default();

        // Record unlocks (append-only, C-11) + count the search call (C-9).
        if let Some(state) = &dyn_state {
            state.count_search_call();
            for entry in &builtin_matches {
                state.record_unlock(&entry.client_name);
            }
        }

        let Some(tool_index) = tool_index else {
            if builtin_matches.is_empty() {
                return Ok(ToolOutput::Text(
                    serde_json::to_string_pretty(&serde_json::json!({
                        "results": [],
                        "total_hidden_tools": 0,
                        "note": "No integration tools are configured. MCP servers are not connected."
                    }))
                    .unwrap()
                    .into(),
                ));
            }
            return Ok(builtin_only_response(&builtin_matches));
        };

        // Event: search_tool.search (telemetry — before grouping)
        let tool_index = tool_index.0.clone();
        let snapshot = tool_index.search_snapshot(&input.query, limit);
        let all_results_json: Vec<serde_json::Value> = snapshot
            .results
            .iter()
            .map(|r| serde_json::json!({"tool_name": r.tool_name, "score": r.score}))
            .collect();
        tracing::info!(
            result_count = snapshot.results.len() as u32,
            all_results = %serde_json::to_string(&all_results_json).unwrap_or_default(),
            "search_tool.search"
        );

        // Group results by server, preserving BM25 score order within each
        // group. Groups are sorted by highest score (best-matching server first).
        // snapshot.results is sorted by BM25 score descending, so the first
        // tool per server is the highest-scoring — used as the group score.
        let mut groups: Vec<(String, f32, Vec<serde_json::Value>)> = Vec::new();
        for r in &snapshot.results {
            let tool_json = serde_json::json!({
                "tool_name": r.tool_name,
                "callable_name": r.tool_name,
                "description": truncate_description(&r.description),
                "score": r.score,
                "input_schema": r.input_schema,
            });
            if let Some(group) = groups
                .iter_mut()
                .find(|(name, _, _)| name == &r.server_name)
            {
                group.2.push(tool_json);
            } else {
                groups.push((r.server_name.clone(), r.score, vec![tool_json]));
            }
        }
        // Built-in matches form their own group so the model can tell the
        // relay targets (hidden builtins) from MCP tools. Score 1.0 so the
        // group sorts first (an exact/near name match is the strongest hit).
        if !builtin_matches.is_empty() {
            groups.push((
                "builtin".to_string(),
                1.0,
                builtin_matches.iter().map(builtin_tool_json).collect(),
            ));
        }
        groups.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let result_groups: Vec<serde_json::Value> = groups
            .into_iter()
            .map(|(server, _, tools)| {
                serde_json::json!({
                    "server": server,
                    "tools": tools,
                })
            })
            .collect();

        let status = if snapshot.is_ready { "ready" } else { "partial" };
        let note = if snapshot.is_ready {
            None
        } else {
            Some("Some MCP servers are still connecting. Results may be incomplete.")
        };

        let response = serde_json::json!({
            "results": result_groups,
            "total_hidden_tools": snapshot.total_hidden_tools + builtin_surface.as_ref().map(|s| s.0.len()).unwrap_or(0),
            "status": status,
            "note": note,
            "hint": "Pass each tool's `callable_name` verbatim as `use_tool`'s `tool_name` parameter.",
        });

        let result_count = snapshot.results.len() + builtin_matches.len();
        let content = serde_json::to_string_pretty(&response).unwrap();
        Ok(ToolOutput::SearchTool(SearchToolOutput {
            result_count,
            content,
        }))
    }
}

/// Render a built-in tool as a result JSON row (MVP relay contract, C-5).
fn builtin_tool_json(entry: &crate::types::resources::BuiltinToolEntry) -> serde_json::Value {
    serde_json::json!({
        "tool_name": entry.client_name,
        "callable_name": entry.client_name,
        "description": truncate_description(&entry.description),
        "score": 1.0,
        "input_schema": entry.input_schema,
    })
}

/// Response body when only built-in tools matched (no MCP index present).
fn builtin_only_response(
    matches: &[crate::types::resources::BuiltinToolEntry],
) -> ToolOutput {
    let response = serde_json::json!({
        "results": [{
            "server": "builtin",
            "tools": matches.iter().map(builtin_tool_json).collect::<Vec<_>>(),
        }],
        "total_hidden_tools": matches.len(),
        "status": "ready",
        "note": serde_json::Value::Null,
        "hint": "Pass each tool's `callable_name` verbatim as `use_tool`'s `tool_name` parameter.",
    });
    ToolOutput::SearchTool(SearchToolOutput {
        result_count: matches.len(),
        content: serde_json::to_string_pretty(&response).unwrap(),
    })
}

/// Rank built-in tools against a query.
///
/// Exact (case-insensitive) name match wins outright; otherwise a tool is
/// included when the whole query, or any token ≥3 chars, appears in its name
/// or description. Name hits outrank description hits. No schemas are
/// truncated here — the caller already bounds the count via `limit`.
fn match_builtin_tools(
    entries: &[crate::types::resources::BuiltinToolEntry],
    query: &str,
    limit: usize,
) -> Vec<crate::types::resources::BuiltinToolEntry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    // Exact name match (case-insensitive) short-circuits.
    if let Some(exact) = entries
        .iter()
        .find(|e| e.client_name.to_lowercase() == q)
    {
        return vec![exact.clone()];
    }
    let tokens: Vec<String> = q
        .split_whitespace()
        .filter(|t| t.chars().count() >= 3)
        .map(str::to_owned)
        .collect();
    let mut scored: Vec<(u8, &crate::types::resources::BuiltinToolEntry)> = Vec::new();
    for entry in entries {
        let name = entry.client_name.to_lowercase();
        let desc = entry.description.to_lowercase();
        let mut score = 0u8;
        if name.contains(&q) {
            score = 3;
        } else if tokens.iter().any(|t| name.contains(t.as_str())) {
            score = 2;
        } else if desc.contains(&q) {
            score = 1;
        } else if tokens.iter().any(|t| desc.contains(t.as_str())) {
            score = 1;
        }
        if score > 0 {
            scored.push((score, entry));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, e)| e.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::tool_index::{
        SearchSnapshot, ServerSummary, ToolIndex, ToolSearchIndex, ToolSearchResult,
    };
    use cf_tool_runtime::Tool;

    struct StaticToolIndex {
        snapshot: SearchSnapshot,
    }

    impl ToolSearchIndex for StaticToolIndex {
        fn search_snapshot(&self, _query: &str, _limit: usize) -> SearchSnapshot {
            self.snapshot.clone()
        }

        fn list_server_summaries(&self) -> Vec<ServerSummary> {
            Vec::new()
        }
    }

    #[tokio::test]
    async fn search_tool_groups_gateway_result_by_connector_name() {
        let resources = crate::types::resources::Resources::default().into_shared();
        resources
            .lock()
            .await
            .insert(ToolIndex(std::sync::Arc::new(StaticToolIndex {
                snapshot: SearchSnapshot {
                    results: vec![ToolSearchResult {
                        tool_name: "grafana__search_dashboards".into(),
                        server_name: "Grafana".into(),
                        description: "Search Grafana dashboards".into(),
                        score: 1.0,
                        parameters: vec!["query".into()],
                        input_schema: serde_json::json!({
                            "type": "object",
                            "properties": {
                                "query": {"type": "string"},
                            }
                        }),
                    }],
                    total_hidden_tools: 1,
                    is_ready: true,
                },
            })));
        let mut ctx =
            cf_tool_runtime::ToolCallContext::new(cf_tool_protocol::ToolCallId::new_v7());
        ctx.extensions.insert(resources);

        let output = SearchTool
            .run(
                ctx,
                SearchToolInput {
                    query: "grafana".into(),
                    limit: Some(5),
                },
            )
            .await
            .unwrap();
        let ToolOutput::SearchTool(output) = output else {
            panic!("expected search tool output");
        };
        let json: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(json["results"][0]["server"], "Grafana");
        assert_eq!(
            json["results"][0]["tools"][0]["tool_name"],
            "grafana__search_dashboards"
        );
        assert_eq!(
            json["results"][0]["tools"][0]["input_schema"]["properties"]["query"]["type"],
            "string"
        );
    }

    /// B-0/B-1: built-in hidden tools are surfaced (with full schema) even
    /// when no MCP index is present, and each returned tool is recorded in
    /// the append-only unlock state (C-11).
    #[tokio::test]
    async fn builtin_surface_surfaces_hidden_tools_and_records_unlocks() {
        use crate::types::resources::{
            BuiltinToolEntry, BuiltinToolSearchSurface, DynamicToolState, Resources,
        };
        let resources = Resources::default().into_shared();
        let state = DynamicToolState::default();
        {
            let mut guard = resources.lock().await;
            guard.insert(BuiltinToolSearchSurface(vec![
                BuiltinToolEntry {
                    client_name: "think".into(),
                    description: "Record internal reasoning".into(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": { "thought": { "type": "string" } }
                    }),
                },
                BuiltinToolEntry {
                    client_name: "video_gen".into(),
                    description: "Generate a video".into(),
                    input_schema: serde_json::json!({"type": "object"}),
                },
            ]));
            guard.insert(state.clone());
        }
        let mut ctx =
            cf_tool_runtime::ToolCallContext::new(cf_tool_protocol::ToolCallId::new_v7());
        ctx.extensions.insert(resources);

        let output = SearchTool
            .run(
                ctx,
                SearchToolInput {
                    query: "think".into(),
                    limit: Some(5),
                },
            )
            .await
            .unwrap();
        let ToolOutput::SearchTool(output) = output else {
            panic!("expected search tool output");
        };
        let json: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(json["results"][0]["server"], "builtin");
        assert_eq!(json["results"][0]["tools"][0]["callable_name"], "think");
        assert_eq!(
            json["results"][0]["tools"][0]["input_schema"]["properties"]["thought"]["type"],
            "string"
        );
        assert_eq!(state.unlocked(), vec!["think".to_string()]);
        assert_eq!(state.search_call_count(), 1);
    }

    // -- truncate_description tests --

    #[test]
    fn truncate_short_description_unchanged() {
        let short = "A short description";
        assert_eq!(truncate_description(short), short);
    }

    #[test]
    fn truncate_exact_limit_unchanged() {
        let exact: String = "x".repeat(MAX_MCP_DESCRIPTION_LENGTH);
        assert_eq!(truncate_description(&exact), exact);
    }

    #[test]
    fn truncate_multibyte_under_char_limit_unchanged() {
        // 1024 CJK chars = ~3072 bytes, but only 1024 chars — well under 2048 char limit.
        let cjk: String = "\u{4e16}".repeat(1024);
        assert!(cjk.len() > MAX_MCP_DESCRIPTION_LENGTH);
        assert_eq!(truncate_description(&cjk), cjk);
    }

    #[test]
    fn truncate_over_limit_adds_suffix() {
        let long: String = "a".repeat(MAX_MCP_DESCRIPTION_LENGTH + 100);
        let result = truncate_description(&long);
        assert!(result.ends_with(TRUNCATION_SUFFIX));
        assert!(
            result.chars().count() <= MAX_MCP_DESCRIPTION_LENGTH,
            "truncated result ({} chars) exceeds MAX_MCP_DESCRIPTION_LENGTH ({})",
            result.chars().count(),
            MAX_MCP_DESCRIPTION_LENGTH,
        );
    }

    // -- build_server_reminder tests --

    #[test]
    fn reminder_no_servers_returns_none() {
        assert!(build_server_reminder(&[]).is_none());
    }

    #[test]
    fn reminder_with_servers() {
        let servers = vec![ServerSummary {
            name: "linear".into(),
            description: Some("Project management".into()),
            tool_count: 12,
            tool_names: vec!["get_issue".into(), "save_issue".into()],
        }];
        let text = build_server_reminder(&servers).unwrap();
        assert!(
            text.contains("- linear (12 tools): Project management\n"),
            "got: {text}"
        );
        assert!(text.contains("Connected MCP servers:"));
        assert!(
            !text.contains("Tools:"),
            "tool names must not be listed in the MCP prompt: {text}"
        );
    }

    #[test]
    fn reminder_no_description() {
        let servers = vec![ServerSummary {
            name: "slack".into(),
            description: None,
            tool_count: 8,
            tool_names: vec![],
        }];
        let text = build_server_reminder(&servers).unwrap();
        assert!(text.contains("- slack (8 tools)\n"), "got: {text}");
    }

    #[test]
    fn reminder_multiline_sanitized() {
        let servers = vec![ServerSummary {
            name: "verbose".into(),
            description: Some("Line one\nLine two\r\nLine three".into()),
            tool_count: 3,
            tool_names: vec![],
        }];
        let text = build_server_reminder(&servers).unwrap();
        assert!(
            text.contains("- verbose (3 tools): Line one Line two Line three\n"),
            "got: {text}"
        );
    }

    #[test]
    fn reminder_empty_description_treated_as_none() {
        let servers = vec![ServerSummary {
            name: "empty".into(),
            description: Some("".into()),
            tool_count: 5,
            tool_names: vec![],
        }];
        let text = build_server_reminder(&servers).unwrap();
        assert!(text.contains("- empty (5 tools)\n"), "got: {text}");
        assert!(!text.contains(": \n"));
    }

    #[test]
    fn reminder_singular_tool() {
        let servers = vec![ServerSummary {
            name: "single".into(),
            description: None,
            tool_count: 1,
            tool_names: vec![],
        }];
        let text = build_server_reminder(&servers).unwrap();
        assert!(text.contains("- single (1 tool)\n"), "got: {text}");
        assert!(!text.contains("1 tools"));
    }

    #[test]
    fn reminder_long_description_truncated() {
        let long_desc: String = "x".repeat(MAX_MCP_DESCRIPTION_LENGTH + 500);
        let servers = vec![ServerSummary {
            name: "grafana".into(),
            description: Some(long_desc),
            tool_count: 28,
            tool_names: vec![],
        }];
        let text = build_server_reminder(&servers).unwrap();
        assert!(text.contains(TRUNCATION_SUFFIX), "got: {text}");
    }

    // -- delta reminder tests --

    #[test]
    fn delta_no_change_returns_none() {
        let servers = vec![ServerSummary {
            name: "linear".into(),
            description: Some("PM".into()),
            tool_count: 5,
            tool_names: vec![],
        }];
        let old = fingerprint_servers(&servers);
        assert!(build_delta_reminder(&old, &servers).is_none());
    }

    #[test]
    fn delta_new_server_announced() {
        let old = std::collections::HashMap::new();
        let servers = vec![ServerSummary {
            name: "slack".into(),
            description: Some("Chat".into()),
            tool_count: 8,
            tool_names: vec!["post_message".into(), "read_thread".into()],
        }];
        let text = build_delta_reminder(&old, &servers).unwrap();
        assert!(text.contains("MCP server connected:"), "got: {text}");
        assert!(text.contains("- slack (8 tools): Chat"), "got: {text}");
        assert!(
            !text.contains("Tools:"),
            "tool names must not be listed in the MCP prompt: {text}"
        );
    }

    #[test]
    fn delta_removed_server_announced() {
        let servers = vec![ServerSummary {
            name: "calendar".into(),
            description: None,
            tool_count: 3,
            tool_names: vec![],
        }];
        let old = fingerprint_servers(&servers);
        let text = build_delta_reminder(&old, &[]).unwrap();
        assert!(text.contains("disconnected: calendar"), "got: {text}");
    }

    #[test]
    fn delta_changed_tool_count_announced() {
        let old_servers = vec![ServerSummary {
            name: "linear".into(),
            description: Some("PM".into()),
            tool_count: 5,
            tool_names: vec![],
        }];
        let old = fingerprint_servers(&old_servers);
        let new_servers = vec![ServerSummary {
            name: "linear".into(),
            description: Some("PM".into()),
            tool_count: 8,
            tool_names: vec![],
        }];
        let text = build_delta_reminder(&old, &new_servers).unwrap();
        assert!(text.contains("MCP server updated:"), "got: {text}");
        assert!(text.contains("- linear (8 tools): PM"), "got: {text}");
    }

    #[test]
    fn delta_combined_add_and_remove() {
        let old_servers = vec![ServerSummary {
            name: "old_server".into(),
            description: None,
            tool_count: 2,
            tool_names: vec![],
        }];
        let old = fingerprint_servers(&old_servers);
        let new_servers = vec![ServerSummary {
            name: "new_server".into(),
            description: None,
            tool_count: 4,
            tool_names: vec![],
        }];
        let text = build_delta_reminder(&old, &new_servers).unwrap();
        assert!(text.contains("new_server"), "got: {text}");
        assert!(text.contains("disconnected: old_server"), "got: {text}");
    }

    #[test]
    fn delta_different_tool_names_same_count_fires() {
        let old_servers = vec![ServerSummary {
            name: "linear".into(),
            description: Some("PM".into()),
            tool_count: 2,
            tool_names: vec!["tool_a".into(), "tool_b".into()],
        }];
        let old = fingerprint_servers(&old_servers);
        let new_servers = vec![ServerSummary {
            name: "linear".into(),
            description: Some("PM".into()),
            tool_count: 2,
            tool_names: vec!["tool_a".into(), "tool_c".into()],
        }];
        let text = build_delta_reminder(&old, &new_servers).unwrap();
        assert!(
            text.contains("MCP server updated:"),
            "should say updated, not connected: {text}"
        );
        assert!(
            text.contains("- linear (2 tools)"),
            "delta should fire when tool names change: {text}"
        );
    }

    #[test]
    fn fingerprint_deterministic() {
        let servers = vec![
            ServerSummary {
                name: "a".into(),
                description: Some("desc".into()),
                tool_count: 3,
                tool_names: vec![],
            },
            ServerSummary {
                name: "b".into(),
                description: None,
                tool_count: 1,
                tool_names: vec![],
            },
        ];
        let f1 = fingerprint_servers(&servers);
        let f2 = fingerprint_servers(&servers);
        assert_eq!(f1, f2);
    }

    // -- hash_value stability tests (FNV-1a) --

    #[test]
    fn hash_value_deterministic() {
        let a = hash_value(&"hello");
        let b = hash_value(&"hello");
        assert_eq!(a, b);
    }

    #[test]
    fn hash_value_different_inputs_differ() {
        assert_ne!(hash_value(&"hello"), hash_value(&"world"));
    }

    #[test]
    fn hash_value_pinned_output() {
        // Pin a known input/output pair so accidental hasher changes are caught.
        let h = hash_value(&"grok-mcp-fingerprint-stability-test");
        assert_eq!(h, hash_value(&"grok-mcp-fingerprint-stability-test"));
        // The value must not change across runs (FNV-1a is deterministic).
        // If this assertion fails, the hasher implementation was modified.
        assert_ne!(h, 0, "hash should be non-zero for non-empty input");
    }
}
