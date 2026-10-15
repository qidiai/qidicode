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

/// Canonical low-watermark (reclaim) context-utilization percentage for the
/// **batched** pruning mode (Phase C, design doc O-4 / C-7). When the batched
/// mode is enabled, a single batch clear fires once utilization reaches this
/// percentage — and is still below the high-water [`PRUNE_WATERMARK_PERCENT`].
/// Fixed at 40 by the Phase C 采数 (15 sessions / 183 turns, recovery-curve
/// knee; 10 pp headroom below the 50% hard line).
pub const LOW_WATERMARK_PERCENT_DEFAULT: u8 = 40;

/// High-water (trigger) context-utilization percentage: the
/// `request_builder::should_prune` gate fires above this line. Kept as a named
/// constant (instead of the historical inline `/ 2`) so the C-7 compile-time
/// assertion below can pin the reclaim line against the trigger line and
/// prevent the two from drifting together.
pub const PRUNE_WATERMARK_PERCENT: u8 = 50;

// C-7 hysteresis invariant: the batched reclaim line must sit strictly below
// the trigger line (design doc §Q3 / Phase C; mirrors the image-compaction
// hysteresis at `request_builder.rs`). Enforced at compile time so the two
// constants can't drift together.
const _: () = assert!(LOW_WATERMARK_PERCENT_DEFAULT < PRUNE_WATERMARK_PERCENT);

/// M1 (C-7 hysteresis dead zone): percentage points below the low-watermark
/// trigger line at which the batch-clear window re-arms. A batch clear closes
/// the window; it only reopens once utilization falls this far back below the
/// trigger line (e.g. 40% trigger → 35% reclaim). Between the reclaim line and
/// the trigger line the retained layer stays inert, so a small dip-and-re-rise
/// near the trigger cannot fire a second clear.
pub const LOW_WATERMARK_RECLAIM_MARGIN_PERCENT: u8 = 5;

// The reclaim margin must be small enough that the reclaim line stays strictly
// below the canonical trigger line (a margin ≥ the trigger itself would make
// the dead zone collapse). Enforced at compile time.
const _: () = assert!(LOW_WATERMARK_RECLAIM_MARGIN_PERCENT < LOW_WATERMARK_PERCENT_DEFAULT);

/// Phase C Reasoning-summary table (block fingerprint → summary text).
///
/// Pre-generated asynchronously by the session layer and written via
/// [`crate::handle::ChatStateHandle::set_reasoning_summaries`]; applied as
/// synthetic visible-text siblings by the batched prune
/// (`prune_retained_batched`) when a batch-clear window fires. The `u64` key
/// is the block's content fingerprint (see
/// [`crate::actor::reasoning_summary::reasoning_block_fingerprint`]), aligning
/// with the batch-1 `replay_prompt_fingerprints` key semantics.
///
/// Transient pre-generation cache: intentionally **not** part of
/// [`ChatStateSnapshot`] (matches `last_batch_clear_at` /
/// `replay_prompt_fingerprints` precedent) — it is fork/rewind-irrelevant.
pub type ReasoningSummaries = std::collections::BTreeMap<u64, String>;

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
    /// Phase C batched-pruning execution-mode flag (default `false` = per-turn,
    /// byte-identical to the pre-Phase-C build). When `true`, the retained
    /// layer switches from per-turn hard-clears to a low-watermark batch clear.
    pub batched_pruning: bool,
    /// Low-watermark context-utilization percentage for the batched mode.
    /// `None` = no batched watermark configured (batched mode inert);
    /// `Some(p)` = the batch clear fires once utilization reaches `p` percent
    /// (and stays below [`PRUNE_WATERMARK_PERCENT`]). Must be < 50% (C-7).
    pub low_watermark_percent: Option<u8>,
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
            batched_pruning: false,
            low_watermark_percent: None,
        }
    }
}

impl PruningConfig {
    /// Whether the batched execution mode is active (Phase C, C-6).
    ///
    /// Both the flag and a configured low watermark must be present — the flag
    /// is the master switch, the watermark supplies the trigger line. `false`
    /// keeps every new Phase C path inert, byte-identical to the pre-Phase-C
    /// build (the C-6 flag-off regression anchor).
    pub fn batched_mode(&self) -> bool {
        self.batched_pruning && self.low_watermark_percent.is_some()
    }

    /// The effective low-watermark percentage: the configured value, or the
    /// canonical [`LOW_WATERMARK_PERCENT_DEFAULT`] when unset.
    ///
    /// M1: the value is clamped. A configured watermark `>= ` the high-water
    /// [`PRUNE_WATERMARK_PERCENT`] (50%) is invalid — it would put the trigger
    /// line at or above the API-copy gate, so the batched layer could never
    /// fire in its own region — and is treated as the canonical
    /// [`LOW_WATERMARK_PERCENT_DEFAULT`] with a `warn` for post-mortem.
    pub fn effective_low_watermark_percent(&self) -> u8 {
        match self.low_watermark_percent {
            Some(p) if p < PRUNE_WATERMARK_PERCENT => p,
            Some(p) => {
                tracing::warn!(
                    configured = p,
                    fallback = LOW_WATERMARK_PERCENT_DEFAULT,
                    "low_watermark_percent must be < PRUNE_WATERMARK_PERCENT (50%); \
                     M1: falling back to the canonical low watermark"
                );
                LOW_WATERMARK_PERCENT_DEFAULT
            }
            None => LOW_WATERMARK_PERCENT_DEFAULT,
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
/// **Status (N6, step-5 第五方审计)**: Phase C **shipped** — the batched
/// execution mode is live behind `[compaction.pruning].batched_pruning`
/// (cf-config, default `false`, R7), wired through
/// `PruningConfig::batched_mode()`. This struct itself still has **no
/// production call site**: it is reserved as the C-2 applicability-domain
/// declaration, and the tests below pin its membership semantics so a
/// future C-2 wiring can adopt it as-is.
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
    /// The 50% context watermark (design doc v3.3 ⑧), derived from
    /// [`PRUNE_WATERMARK_PERCENT`] so the C-7 trigger line has a single
    /// source of truth (the inline `/ 2` in `should_prune` would otherwise
    /// drift from this fraction).
    pub const WATERMARK: f64 = PRUNE_WATERMARK_PERCENT as f64 / 100.0;

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
    /// fires below the 50% context watermark. **N6 (step-5 第五方审计)**:
    /// the batched mode has since shipped (R7,
    /// `[compaction.pruning].batched_pruning`); this test keeps pinning the
    /// domain *declaration* — the default (per-turn) domain sits
    /// outside it, and every quadrant of the domain
    /// predicate evaluates as specified.
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

    /// C-7 (design doc §Q3 / Phase C): the batched reclaim line
    /// (`LOW_WATERMARK_PERCENT_DEFAULT`) must sit strictly below the trigger
    /// line (`PRUNE_WATERMARK_PERCENT`). The compile-time `assert!` above
    /// pins the two constants; this runtime test is the belt-and-suspenders
    /// form so a drift shows up in CI even where a const-eval lint would not.
    #[test]
    fn c7_batched_watermark_lines_do_not_drift() {
        assert!(
            LOW_WATERMARK_PERCENT_DEFAULT < PRUNE_WATERMARK_PERCENT,
            "C-7: low watermark ({LOW_WATERMARK_PERCENT_DEFAULT}%) must sit below \
             the trigger line ({PRUNE_WATERMARK_PERCENT}%)"
        );
        assert_eq!(
            BatchedPruningDomain::WATERMARK,
            f64::from(PRUNE_WATERMARK_PERCENT) / 100.0,
            "C-7: domain WATERMARK must stay derived from PRUNE_WATERMARK_PERCENT"
        );
    }

    /// C-6 flag-off anchor (skeleton): the default config is per-turn —
    /// batched mode is off and the low watermark is unset, so every new
    /// Phase C path is inert.
    #[test]
    fn default_pruning_config_is_per_turn_not_batched() {
        let cfg = PruningConfig::default();
        assert!(
            !cfg.batched_mode(),
            "C-6: default PruningConfig must not activate batched mode"
        );
        assert!(
            !cfg.batched_pruning,
            "C-6: batched_pruning flag defaults to false"
        );
        assert!(
            cfg.low_watermark_percent.is_none(),
            "C-6: low_watermark_percent defaults to None (per-turn)"
        );
        assert_eq!(
            cfg.effective_low_watermark_percent(),
            LOW_WATERMARK_PERCENT_DEFAULT,
            "effective_low_watermark_percent must fall back to the canonical 40%"
        );
    }

    /// C-6 flag-on + watermark: batched mode activates only when both the
    /// flag and a configured watermark are present.
    #[test]
    fn batched_mode_requires_flag_and_watermark() {
        assert!(
            PruningConfig {
                batched_pruning: true,
                low_watermark_percent: Some(40),
                ..Default::default()
            }
            .batched_mode(),
            "C-6: flag + watermark must activate batched mode"
        );
        assert!(
            !PruningConfig {
                batched_pruning: true,
                low_watermark_percent: None,
                ..Default::default()
            }
            .batched_mode(),
            "C-6: flag alone (no watermark) must NOT activate batched mode"
        );
        assert!(
            !PruningConfig {
                batched_pruning: false,
                low_watermark_percent: Some(40),
                ..Default::default()
            }
            .batched_mode(),
            "C-6: watermark alone (no flag) must NOT activate batched mode"
        );
    }

    /// M1: a configured low watermark `>=` the 50% high-water line is invalid
    /// (the trigger would sit at/above the API-copy gate) and falls back to the
    /// canonical 40%; valid values pass through unchanged.
    #[test]
    fn effective_low_watermark_percent_clamps_invalid_values() {
        let cfg = |p: Option<u8>| PruningConfig {
            low_watermark_percent: p,
            ..Default::default()
        };
        assert_eq!(
            cfg(Some(30)).effective_low_watermark_percent(),
            30,
            "M1: a valid watermark passes through unchanged"
        );
        assert_eq!(
            cfg(Some(49)).effective_low_watermark_percent(),
            49,
            "M1: 49% is still valid (strictly below the 50% line)"
        );
        assert_eq!(
            cfg(Some(50)).effective_low_watermark_percent(),
            LOW_WATERMARK_PERCENT_DEFAULT,
            "M1: 50% is invalid (>= high-water) → fall back to the canonical default"
        );
        assert_eq!(
            cfg(Some(80)).effective_low_watermark_percent(),
            LOW_WATERMARK_PERCENT_DEFAULT,
            "M1: >50% is invalid → fall back to the canonical default"
        );
        assert_eq!(
            cfg(None).effective_low_watermark_percent(),
            LOW_WATERMARK_PERCENT_DEFAULT,
            "M1: unset → canonical default"
        );
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
