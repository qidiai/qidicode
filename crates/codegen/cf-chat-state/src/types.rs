//! Shared domain types for the chat state actor.

use std::collections::BTreeSet;
use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};
use cf_sampling_types::{ConversationItem, SamplingConfig};

/// Canonical marker for an injected memory-context block. Shared by the
/// emitter in `qidi-code` and the upsert/detection here — a drift would
/// silently break dedup and let blocks accumulate in the prompt prefix.
/// Detection assumes the literal never appears in a system prompt except as
/// an injected block.
pub const MEMORY_CONTEXT_OPEN_TAG: &str = "<memory-context>";

/// Closing tag paired with [`MEMORY_CONTEXT_OPEN_TAG`].
pub const MEMORY_CONTEXT_CLOSE_TAG: &str = "</memory-context>";

/// Configuration for the ChatStateActor at spawn time.
#[derive(Debug, Clone)]
pub struct ChatStateConfig {
    /// Initial conversation items to populate the state with.
    pub initial_conversation: Vec<ConversationItem>,
    /// Sampling configuration (model, context window, etc.).
    pub sampling_config: SamplingConfig,
}

/// Immutable snapshot of the actor's state (for forking, rewind).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatStateSnapshot {
    /// The full conversation history.
    pub conversation: Vec<ConversationItem>,
    /// Current sampling configuration.
    pub sampling_config: SamplingConfig,
    /// Current prompt index (incremented per user turn).
    pub prompt_index: usize,
    /// Accumulated token usage.
    pub total_tokens: u64,
    /// Bytes/4 estimate of the conversation as of the last `record_token_usage`.
    /// `0` means unknown (pre-field snapshot); restore re-estimates instead.
    #[serde(default)]
    pub estimate_at_last_response: u64,
    /// File paths the agent has edited.
    pub agent_edited_paths: BTreeSet<String>,
    /// Cached prompt texts for rewind preview.
    pub prompt_texts: Vec<String>,
    /// Timestamp when the current stream started (epoch ms).
    pub stream_start_ms: Option<i64>,
    /// Timestamp when the current turn started (epoch ms).
    pub turn_start_ms: Option<i64>,
    /// Prompt index at which the last compaction occurred.
    pub last_compaction_prompt_index: Option<usize>,
    /// Opaque credential secrets (API key, optional extra auth, client version).
    #[serde(default)]
    pub credentials: Credentials,
}

/// Metadata for session notifications (timing info).
#[derive(Debug, Clone)]
pub struct NotificationMeta {
    /// Timestamp when the current stream started (epoch ms).
    pub stream_start_ms: Option<i64>,
    /// Timestamp when the current turn started (epoch ms).
    pub turn_start_ms: Option<i64>,
}

/// Configuration for tool-result pruning.
///
/// Prunes old, large tool results from the conversation to reclaim context space.
/// Two modes: soft trim (keep head + tail) and hard clear (replace entirely).
#[derive(Debug, Clone)]
pub struct PruningConfig {
    /// Whether pruning is enabled.
    pub enabled: bool,
    /// Number of recent turns whose tool results are never pruned.
    pub keep_last_n_turns: usize,
    /// Character threshold above which old tool results are soft-trimmed.
    pub soft_trim_threshold: usize,
    /// Characters to keep from the start of a soft-trimmed result.
    pub soft_trim_head: usize,
    /// Characters to keep from the end of a soft-trimmed result.
    pub soft_trim_tail: usize,
    /// Turn age after which tool results are hard-cleared (replaced with placeholder).
    pub hard_clear_age_turns: usize,
}

impl Default for PruningConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            keep_last_n_turns: 3,
            soft_trim_threshold: 4000,
            soft_trim_head: 1500,
            soft_trim_tail: 1500,
            hard_clear_age_turns: 10,
        }
    }
}

/// Applicability domain of the **C-2 batched-pruning invariant**
/// (P0-2 第二步 design doc §Q3 / Phase C, v3.2 裁决 ④ + v3.3
/// 双模裁决 ⑧): the "window-internal prefix-growth + one
/// batch-exempt trigger turn" invariant only holds when **both**
/// conditions are true —
///
/// 1. the batched execution mode is enabled (the Phase C
///    `PruningConfig` execution mode; the current default
///    `per_turn` mode runs the retained layer's hard-clear
///    every user turn, so the invariant is vacuously outside
///    its domain there — GLM 加重: per-turn hard-clear
///    breaks the absolute "existing items never rewritten"
///    form even below the watermark), **and**
/// 2. the batch trigger fires below the 50% context
///    watermark (above it the API-copy layer's own
///    `should_prune` gate engages and rewrites items on the
///    clone — a different layer with different rules).
///
/// **Phase A placeholder** (design doc §6 Phase A 范围 4):
/// the batched mode itself ships in Phase C — this module
/// declares the domain and the Phase A test asserts the
/// current (per-turn) build sits **outside** it, so the
/// placeholder is honest: nothing claims the invariant
/// today. Phase C implements the mode and converts the
/// placeholder test into the real invariant.
// `Eq` intentionally omitted: the context fraction is an
// `f64` (watermark comparison), and `f64` is not `Eq`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatchedPruningDomain {
    /// Whether the batched execution mode is enabled
    /// (Phase C flag; default `false` = per-turn).
    pub batched_enabled: bool,
    /// Context-window utilization fraction at the
    /// trigger point (0.0–1.0).
    pub context_fraction: f64,
}

impl BatchedPruningDomain {
    /// The 50% context watermark (design doc v3.3 ⑧).
    pub const WATERMARK: f64 = 0.5;

    /// Whether the C-2 invariant's applicability domain
    /// holds: batched mode enabled **and** the trigger
    /// fired below the 50% watermark.
    pub fn applies(self) -> bool {
        self.batched_enabled && self.context_fraction < Self::WATERMARK
    }
}

impl Default for BatchedPruningDomain {
    /// The Phase A / current-build state: per-turn mode
    /// (batched disabled) — outside the C-2 domain.
    fn default() -> Self {
        Self {
            batched_enabled: false,
            context_fraction: 0.0,
        }
    }
}

/// Where the session's current api_key came from.
/// Determines whether the key can be refreshed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    /// From AuthManager (grok login, OIDC, external binary). Refreshable.
    #[default]
    SessionToken,
    /// From user config ([model.*] api_key, env_key, XAI_API_KEY). Not refreshable.
    ApiKey,
}

/// Credential/secret fields that the actor stores opaquely.
///
/// These are fields from the shell's full `Config` that aren't part of
/// `cf_sampling_types::SamplingConfig` (which is secret-free).
/// The actor just stores and returns them — it never interprets them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Credentials {
    /// API key for authentication.
    pub api_key: Option<String>,
    /// Whether this is a session token (refreshable) or user-provided api key.
    #[serde(default)]
    pub auth_type: AuthType,
    /// Optional extra auth material forwarded with requests when present.
    pub alpha_test_key: Option<String>,
    /// Client version string.
    pub client_version: Option<String>,
}

/// The messages captured during a single conversation turn.
///
/// Produced by `TakeTurnMessages` after a `BeginTurnCapture`/message-push cycle.
#[derive(Debug, Clone)]
pub struct TurnCapture {
    /// The ordered sequence of messages appended during this turn.
    pub messages: Vec<ConversationItem>,
    /// Whether compaction (conversation replacement) occurred mid-turn.
    pub compaction_occurred: bool,
}

/// Item counts for a conversation, broken down by role.
///
/// Returned by `get_conversation_counts()` — avoids cloning the conversation
/// when only role counts and total length are needed (e.g. for telemetry).
#[derive(Debug, Clone, Default)]
pub struct ConversationCounts {
    /// Total number of items in the conversation.
    pub total: usize,
    /// Number of `User` items.
    pub user: usize,
    /// Number of `Assistant` items.
    pub assistant: usize,
    /// Number of `ToolResult` items.
    pub tool_result: usize,
}

/// Info returned when auto-compact threshold is exceeded.
#[derive(Debug, Clone)]
pub struct AutoCompactTrigger {
    /// Current total token count.
    pub total_tokens: u64,
    /// Model's context window size.
    pub context_window: NonZeroU64,
    /// Current utilization as a percentage (0–100).
    pub utilization_percent: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Design-doc contract C-2 applicability domain (P0-2
    /// 第二步 §Q3 / Phase C, v3.3 ⑧): the batched-mode
    /// prefix-growth invariant applies **only** when the
    /// batched execution mode is enabled AND the trigger
    /// fires below the 50% context watermark. **Phase A
    /// placeholder**: the batched mode ships in Phase C,
    /// so this asserts the domain *declaration* — the
    /// current per-turn build (default domain) sits
    /// outside it, and every quadrant of the domain
    /// predicate evaluates as specified. Phase C
    /// converts this into the real invariant test.
    #[test]
    fn c2_applicability_domain_requires_batched_and_below_watermark() {
        // Phase A / current build: per-turn mode —
        // outside the domain regardless of watermark.
        assert!(!BatchedPruningDomain::default().applies());
        assert!(!BatchedPruningDomain {
            batched_enabled: false,
            context_fraction: 0.1,
        }
        .applies());
        // Both conditions required: batched on but at/
        // above the 50% watermark → outside (the
        // API-copy layer's own gate owns that region).
        assert!(!BatchedPruningDomain {
            batched_enabled: true,
            context_fraction: 0.5,
        }
        .applies());
        assert!(!BatchedPruningDomain {
            batched_enabled: true,
            context_fraction: 0.9,
        }
        .applies());
        // Inside the domain: batched enabled AND below
        // the watermark.
        assert!(BatchedPruningDomain {
            batched_enabled: true,
            context_fraction: 0.49,
        }
        .applies());
        assert!(BatchedPruningDomain {
            batched_enabled: true,
            context_fraction: 0.0,
        }
        .applies());
        // Watermark constant is the design-doc 50%.
        assert_eq!(BatchedPruningDomain::WATERMARK, 0.5);
    }

    #[test]
    fn snapshot_round_trips_through_serde_json() {
        let snapshot = ChatStateSnapshot {
            conversation: vec![],
            sampling_config: SamplingConfig {
                base_url: "https://api.example.com".to_string(),
                model: "test-model".to_string(),
                max_completion_tokens: None,
                temperature: None,
                top_p: None,
                api_backend: Default::default(),
                extra_headers: Default::default(),
                context_window: NonZeroU64::new(128_000).unwrap(),
                reasoning_effort: None,
                stream_tool_calls: None,
            },
            prompt_index: 0,
            total_tokens: 0,
            estimate_at_last_response: 0,
            agent_edited_paths: BTreeSet::new(),
            prompt_texts: vec![],
            stream_start_ms: None,
            turn_start_ms: None,
            last_compaction_prompt_index: None,
            credentials: Credentials::default(),
        };

        let json = serde_json::to_string(&snapshot).expect("serialize");
        let deserialized: ChatStateSnapshot = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.prompt_index, 0);
        assert_eq!(deserialized.total_tokens, 0);
        assert!(deserialized.conversation.is_empty());
        assert!(deserialized.agent_edited_paths.is_empty());
        assert!(deserialized.last_compaction_prompt_index.is_none());
    }

    #[test]
    fn snapshot_round_trips_with_data() {
        use cf_sampling_types::ConversationItem;

        let snapshot = ChatStateSnapshot {
            conversation: vec![
                ConversationItem::system("You are a helpful assistant."),
                ConversationItem::user("Hello!"),
                ConversationItem::assistant("Hi there!"),
            ],
            sampling_config: SamplingConfig {
                base_url: "https://api.example.com".to_string(),
                model: "grok-3".to_string(),
                max_completion_tokens: Some(4096),
                temperature: Some(0.7),
                top_p: None,
                api_backend: Default::default(),
                extra_headers: Default::default(),
                context_window: NonZeroU64::new(128_000).unwrap(),
                reasoning_effort: None,
                stream_tool_calls: None,
            },
            prompt_index: 5,
            total_tokens: 1234,
            estimate_at_last_response: 900,
            agent_edited_paths: BTreeSet::from([
                "src/main.rs".to_string(),
                "src/lib.rs".to_string(),
            ]),
            prompt_texts: vec!["first prompt".to_string(), "second prompt".to_string()],
            stream_start_ms: Some(1234567890),
            turn_start_ms: Some(1234567800),
            last_compaction_prompt_index: Some(2),
            credentials: Credentials::default(),
        };

        let json = serde_json::to_string(&snapshot).expect("serialize");
        let deserialized: ChatStateSnapshot = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.prompt_index, 5);
        assert_eq!(deserialized.total_tokens, 1234);
        assert_eq!(deserialized.conversation.len(), 3);
        assert_eq!(deserialized.agent_edited_paths.len(), 2);
        assert_eq!(deserialized.prompt_texts.len(), 2);
        assert_eq!(deserialized.stream_start_ms, Some(1234567890));
        assert_eq!(deserialized.turn_start_ms, Some(1234567800));
        assert_eq!(deserialized.last_compaction_prompt_index, Some(2));
    }
}
