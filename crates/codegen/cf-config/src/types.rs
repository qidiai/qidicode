
// from xai-grok-config-types/flags.rs
//! Config-value resolution leaf types and per-model laziness config,
//! extracted from qidi-code for dependency inversion.

use serde::{Deserialize, Serialize};
use crate::env_bool;

/// Where a resolved config value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    // Note: strum::Display derive removed (strum not in deps)
    Requirement,
    Cli,
    Env,
    SystemManagedConfig,
    ManagedConfig,
    UserConfig,
    Config,
    Remote,
    Default,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ConfigSource::Requirement => "requirement",
            ConfigSource::Cli => "cli",
            ConfigSource::Env => "env",
            ConfigSource::SystemManagedConfig => "system_managed_config",
            ConfigSource::ManagedConfig => "managed_config",
            ConfigSource::UserConfig => "user_config",
            ConfigSource::Config => "config",
            ConfigSource::Remote => "remote",
            ConfigSource::Default => "default",
        };
        write!(f, "{}", s)
    }
}
/// Announcement from remote settings or local override.
/// Matches the structure in xai-grok-announcements to allow re-export.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnouncementCta {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub caption: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteAnnouncement {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub cta: Option<AnnouncementCta>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub dismissible: Option<bool>,
    #[serde(default)]
    pub persistent: Option<bool>,
}


/// A resolved config value with its source for diagnostics.
#[derive(Debug, Clone)]
pub struct Resolved<T> {
    pub value: T,
    pub source: ConfigSource,
}

impl<T> Resolved<T> {
    pub fn new(value: T, source: ConfigSource) -> Self {
        Self { value, source }
    }
}

impl<T: std::fmt::Display + std::fmt::Debug> std::fmt::Display for Resolved<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({:?})", self.value, self.source)
    }
}
/// Resolve a boolean feature flag: requirement > cli > env > config > managed > feature flag > default.
pub struct BoolFlag<'a> {
    requirement: Option<bool>,
    cli: Option<bool>,
    env_var: &'a str,
    config: Option<bool>,
    managed: Option<bool>,
    feature_flag: Option<bool>,
    default: bool,
}

impl<'a> BoolFlag<'a> {
    pub fn env(env_var: &'a str) -> Self {
        Self {
            requirement: None,
            cli: None,
            env_var,
            config: None,
            managed: None,
            feature_flag: None,
            default: false,
        }
    }

    pub fn requirement(mut self, v: Option<bool>) -> Self {
        self.requirement = v;
        self
    }
    pub fn cli(mut self, v: Option<bool>) -> Self {
        self.cli = v;
        self
    }
    pub fn config(mut self, v: Option<bool>) -> Self {
        self.config = v;
        self
    }
    pub fn managed(mut self, v: Option<bool>) -> Self {
        self.managed = v;
        self
    }
    pub fn feature_flag(mut self, v: Option<bool>) -> Self {
        self.feature_flag = v;
        self
    }
    pub fn default(mut self, v: bool) -> Self {
        self.default = v;
        self
    }

    pub fn resolve(self) -> Resolved<bool> {
        resolve_bool_flag(
            self.requirement,
            self.cli,
            self.env_var,
            self.config,
            self.managed,
            self.feature_flag,
            self.default,
        )
    }
}

fn resolve_bool_flag(
    requirement: Option<bool>,
    cli_arg: Option<bool>,
    env_var: &str,
    config_val: Option<bool>,
    managed_val: Option<bool>,
    feature_flag_val: Option<bool>,
    default: bool,
) -> Resolved<bool> {
    if let Some(val) = requirement {
        return Resolved::new(val, ConfigSource::Requirement);
    }
    if let Some(val) = cli_arg {
        return Resolved::new(val, ConfigSource::Cli);
    }
    if let Some(val) = env_bool(env_var) {
        return Resolved::new(val, ConfigSource::Env);
    }
    if let Some(val) = config_val {
        return Resolved::new(val, ConfigSource::Config);
    }
    if let Some(val) = managed_val {
        return Resolved::new(val, ConfigSource::ManagedConfig);
    }
    if let Some(val) = feature_flag_val {
        return Resolved::new(val, ConfigSource::Remote);
    }
    Resolved::new(default, ConfigSource::Default)
}
/// Per-model configuration for the Layer-3 LazinessDetector.
///
/// All fields default to the disabled state. Activation is a deliberate
/// two-step opt-in: setting `enabled = true` lets the classifier fire
/// (and emit `LazinessClassifierFired` telemetry), but a nudge is only
/// injected when `max_nudges_per_session > 0` as well. This makes
/// observation-only rollout (classify-but-don't-act) the natural
/// intermediate state.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LazinessDetectorPerModelConfig {
    /// Master switch. When `false` (the default), the classifier never
    /// fires for this model and no per-classification cost is incurred.
    #[serde(default)]
    pub enabled: bool,
    /// Hard cap on `<system-reminder>` nudges injected per session for
    /// this model. Default `0` makes `enabled = true` alone an
    /// observation-only mode (classifier fires, no nudges).
    #[serde(default)]
    pub max_nudges_per_session: u32,
    /// How long the session must be idle before the classifier runs.
    /// `None` defers to the harness default (10 seconds).
    #[serde(default)]
    pub idle_threshold_ms: Option<u64>,
    /// Minimum classifier confidence required to inject a nudge. `None`
    /// defers to the harness default (0.7).
    #[serde(default)]
    pub min_confidence: Option<f32>,
    /// When `Some(true)` (or `None` — the default), the classifier sees
    /// the assistant's plain-text reasoning as `[assistant reasoning]`
    /// lines. `Some(false)` drops them (the pre-2026-05 behavior).
    /// `None` defers to the harness default (`LAZINESS_INCLUDE_REASONING`,
    /// currently `true`).
    #[serde(default)]
    pub include_reasoning: Option<bool>,
}

// from xai-grok-config-types/lib.rs
// mod flags;
// pub use flags::*;
// mod memory;
// pub use memory::*;
// mod mcp;
// pub use mcp::*;
// mod permission;
// pub use permission::*;
// mod pool;
// pub use pool::*;
// // use cf_announcements::RemoteAnnouncement; // replaced with local stub
/// A remote `campaigns[]` entry: an `id` gate plus a full-power
/// flattened config patch (the JSON sibling of a `[[campaigns]]` TOML override).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct CampaignOverride {
    #[serde(default, alias = "campaign_id")]
    pub id: Option<String>,
    #[serde(flatten, default)]
    pub patch: serde_json::Map<String, serde_json::Value>,
}
/// Doom-loop recovery settings: ONE struct serves both the local
/// `[doom_loop_recovery]` TOML table and the remote settings
/// `doom_loop_recovery` JSON object, so the two stay 1:1. All fields are
/// `Option` with per-field defaults (a partial object never fails the parse,
/// and unknown future keys are ignored); unset fields fall through per-field
/// in `resolve_doom_loop_recovery` (env > TOML > remote > default). Distinct
/// namespace from the removed legacy `doom_loop_*` keys.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct DoomLoopRecoverySettings {
    /// Send the `x-grok-doom-loop-check` header and parse the reported
    /// triggers. `Some(false)` is a kill-switch; absent ⇒ client default (off).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Highest `tail_repetition` threshold considered confident (clamped to
    /// 2..=64). Absent ⇒ client default (8). CLIENT-side filter over the
    /// trigger labels the server returns — the server emits every fired
    /// threshold; this is never sent as a request parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_threshold: Option<u32>,
    /// Resample budget per turn (clamped to 0..=5). Absent ⇒ client
    /// default (2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
}
/// Display-refresh probe + auto-cadence settings: ONE struct for local
/// `[ui.display_refresh]`, remote settings `display_refresh`, and `UiConfig`.
/// Field-wise tolerant deserialize (wrong types → `None`); unknown keys kept in
/// [`Self::extra`] so settings save cannot drop future knobs. Resolved by
/// `resolve_display_refresh`. Client defaults: probe on, auto off, floor 8 ms,
/// ceiling 16 ms, Hz band 55–165.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DisplayRefreshSettings {
    /// Once-per-process primary-display Hz probe. `Some(false)` is a kill-switch.
    #[serde(
        default,
        deserialize_with = "de_opt_bool_tolerant",
        skip_serializing_if = "Option::is_none"
    )]
    pub probe_enabled: Option<bool>,
    /// Derive paint/scroll cadence from a successful in-band probe (default off).
    #[serde(
        default,
        deserialize_with = "de_opt_bool_tolerant",
        skip_serializing_if = "Option::is_none"
    )]
    pub auto_cadence_enabled: Option<bool>,
    /// Lower clamp for auto-derived ms (default 8).
    #[serde(
        default,
        deserialize_with = "de_opt_u32_tolerant",
        skip_serializing_if = "Option::is_none"
    )]
    pub floor_ms: Option<u32>,
    /// Upper clamp for auto-derived ms (default 16).
    #[serde(
        default,
        deserialize_with = "de_opt_u32_tolerant",
        skip_serializing_if = "Option::is_none"
    )]
    pub ceiling_ms: Option<u32>,
    /// Minimum accepted probe Hz for auto-cadence (default 55).
    #[serde(
        default,
        deserialize_with = "de_opt_u32_tolerant",
        skip_serializing_if = "Option::is_none"
    )]
    pub min_hz: Option<u32>,
    /// Maximum accepted probe Hz for auto-cadence (default 165).
    #[serde(
        default,
        deserialize_with = "de_opt_u32_tolerant",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_hz: Option<u32>,
    /// Unknown / future object members (preserved across config rewrite).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
impl DisplayRefreshSettings {
    /// True when no field is set (all inherit remote/default).
    pub fn is_default(&self) -> bool {
        self.probe_enabled.is_none()
            && self.auto_cadence_enabled.is_none()
            && self.floor_ms.is_none()
            && self.ceiling_ms.is_none()
            && self.min_hz.is_none()
            && self.max_hz.is_none()
            && self.extra.is_empty()
    }
}
fn de_opt_bool_tolerant<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<bool>, D::Error> {
    struct V;
    impl<'de> serde::de::Visitor<'de> for V {
        type Value = Option<bool>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("bool (wrong types ignored)")
        }
        fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
            Ok(Some(v))
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<A: serde::de::Deserializer<'de>>(
            self,
            d: A,
        ) -> Result<Self::Value, A::Error> {
            d.deserialize_any(V)
        }
        fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_string<E: serde::de::Error>(self, _: String) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
            Ok(None)
        }
    }
    deserializer.deserialize_any(V)
}
fn de_opt_u32_tolerant<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    struct V;
    impl<'de> serde::de::Visitor<'de> for V {
        type Value = Option<u32>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("u32 (wrong types ignored)")
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(u32::try_from(v).ok())
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(u32::try_from(v).ok())
        }
        fn visit_u32<E: serde::de::Error>(self, v: u32) -> Result<Self::Value, E> {
            Ok(Some(v))
        }
        fn visit_i32<E: serde::de::Error>(self, v: i32) -> Result<Self::Value, E> {
            Ok(u32::try_from(v).ok())
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<A: serde::de::Deserializer<'de>>(
            self,
            d: A,
        ) -> Result<Self::Value, A::Error> {
            d.deserialize_any(V)
        }
        fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_string<E: serde::de::Error>(self, _: String) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
            Ok(None)
        }
    }
    deserializer.deserialize_any(V)
}
/// Remote settings fetched from cli-chat-proxy `GET /v1/settings`.
///
/// All fields are `Option` with `#[serde(default)]` so that:
/// - Missing fields from old servers are gracefully ignored
/// - New fields added in the future don't break existing clients
/// - Callers can distinguish "server said false" from "server didn't say"
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RemoteSettings {
    /// When `Some(true)`, the server recommends enabling leader mode.
    /// Used as a fallback when the user hasn't set `[cli] use_leader` locally.
    #[serde(default)]
    pub leader_mode: Option<bool>,
    #[serde(default)]
    pub max_upload_file_bytes: Option<u64>,
    #[serde(default)]
    pub max_upload_untracked_bytes: Option<u64>,
    /// When `Some(true)`, capture workspace files for non-git project dirs (client default: off).
    #[serde(default)]
    pub non_git_workspace_capture: Option<bool>,
    #[serde(default)]
    pub persistent_local_shell: Option<bool>,
    /// Release channel: `"stable"` or `"alpha"`.
    /// Fallback when no local `[cli] channel` or `--alpha`/`--stable` flag is set.
    #[serde(default)]
    pub release_channel: Option<String>,
    /// When `Some(true)`, enable LOC attribution tracking for this session.
    #[serde(default)]
    pub loc_tracking: Option<bool>,
    /// Enable the experimental memory system remotely.
    #[serde(default)]
    pub memory_enabled: Option<bool>,
    #[serde(default)]
    pub memory_search_max_results: Option<u32>,
    #[serde(default)]
    pub memory_search_min_score: Option<f32>,
    #[serde(default)]
    pub memory_initial_injection_enabled: Option<bool>,
    #[serde(default)]
    pub memory_initial_injection_min_score: Option<f32>,
    #[serde(default)]
    pub memory_embedding_model: Option<String>,
    #[serde(default)]
    pub memory_embedding_dimensions: Option<u32>,
    #[serde(default)]
    pub pruning_enabled: Option<bool>,
    #[serde(default)]
    pub pruning_keep_last_n_turns: Option<u32>,
    #[serde(default)]
    pub pruning_soft_trim_threshold: Option<u32>,
    #[serde(default)]
    pub flush_enabled: Option<bool>,
    #[serde(default)]
    pub flush_soft_threshold_tokens: Option<u64>,
    #[serde(default)]
    pub flush_idle_timeout_secs: Option<u64>,
    #[serde(default)]
    pub flush_semantic_dedup_threshold: Option<f64>,
    #[serde(default)]
    pub memory_temporal_decay_enabled: Option<bool>,
    #[serde(default)]
    pub memory_temporal_decay_half_life_days: Option<f64>,
    #[serde(default)]
    pub memory_mmr_enabled: Option<bool>,
    #[serde(default)]
    pub memory_mmr_lambda: Option<f64>,
    #[serde(default)]
    pub memory_watcher_enabled: Option<bool>,
    #[serde(default)]
    pub dream_enabled: Option<bool>,
    #[serde(default)]
    pub dream_min_hours: Option<u64>,
    #[serde(default)]
    pub dream_min_sessions: Option<u64>,
    #[serde(default)]
    pub dream_check_interval_secs: Option<u64>,
    /// Cadence (seconds) of the pager's free→paid subscription watch.
    /// `0` disables it; the pager clamps and defaults (see its
    /// `app::subscription` module). Forwarded from the `qidi_build_settings`
    /// remote settings flag via the CCP `/settings` flatten catch-all.
    #[serde(default)]
    pub subscription_watch_interval_secs: Option<u64>,
    #[serde(default)]
    pub writeback_enabled: Option<bool>,
    /// OAuth2 provider issuer URL (e.g., "https://auth.x.ai"). When present
    /// together with `oauth2_client_id`, the client uses OAuth2 authorization code
    /// flow. Controlled via remote settings for gradual rollout.
    #[serde(default)]
    pub oauth2_issuer: Option<String>,
    /// OAuth2 client_id for the CLI. Paired with `oauth2_issuer`.
    #[serde(default)]
    pub oauth2_client_id: Option<String>,
    /// When `Some(true)`, enable grok's default OAuth2 (xAI auth.x.ai).
    /// Enterprise OIDC (user's own IdP via `oidc` config) always wins.
    /// Controlled via remote settings; `--oauth` CLI flag overrides.
    #[serde(default)]
    pub grok_oauth_enabled: Option<bool>,
    #[serde(default)]
    pub lsp_tools_enabled: Option<bool>,
    /// Folder-trust gate kill-switch / remote default. Gates whether repo-local
    /// MCP/LSP servers (commands sourced from working-tree config files) require
    /// a per-folder trust decision before they are spawned. `Some(true)`
    /// enables, `Some(false)` is a kill-switch, `None` falls back to the client
    /// default (on). Sits below env `QIDI_FOLDER_TRUST`, user
    /// `[folder_trust] enabled`, and managed config in the resolver chain. See
    /// `agent::folder_trust::feature_enabled`.
    #[serde(default)]
    pub folder_trust_enabled: Option<bool>,
    #[serde(default)]
    pub write_file_enabled: Option<bool>,
    /// File toolset: `"standard"` or `"hashline"`.
    /// Server-side default; local `[toolset] file_toolset` in config.toml
    /// takes precedence when set.
    #[serde(default)]
    pub file_toolset: Option<String>,
    /// Per-chunk idle timeout in seconds for inference streaming.
    /// Fallback when no per-model `inference_idle_timeout_secs` is set in config.toml.
    #[serde(default)]
    pub inference_idle_timeout_secs: Option<u64>,
    /// Global default MCP startup-handshake timeout (seconds); lowest-precedence
    /// fallback (per-server config, env, and requirements/managed override it).
    #[serde(default)]
    pub mcp_startup_timeout_secs: Option<u64>,
    /// remote settings `qidi_build_settings.max_mcp_output_bytes` — global default
    /// MCP tool-result inline cap (bytes). Overridden by requirements, env,
    /// and `config.toml [mcp] max_output_bytes`. Built-in default 20_000.
    #[serde(default)]
    pub max_mcp_output_bytes: Option<u64>,
    /// When `Some(true)`, enable session registry hooks (register, update, finalize, memory upload).
    /// When absent or `Some(false)`, all hooks are disabled (default: disabled).
    #[serde(default)]
    pub session_registry_enabled: Option<bool>,
    /// The remote settings `doom_loop_recovery` JSON object; see
    /// [`DoomLoopRecoverySettings`]. Absent ⇒ every knob falls through to
    /// TOML/defaults; a partial object falls through per-field.
    #[serde(default)]
    pub doom_loop_recovery: Option<DoomLoopRecoverySettings>,
    /// Enable/disable the runtime turn-end TodoGate remotely.
    /// Precedence: CLI `--todo-gate` > this field > built-in default (`false`).
    /// The gate ships disabled; set this to `Some(true)` (via the
    /// `qidi_build_settings` remote settings key) to enable it. See
    /// `session::acp_session::resolve_reminder_policy`.
    #[serde(default)]
    pub todo_gate_enabled: Option<bool>,
    /// Hard cap on TodoGate fires per user prompt.
    /// Precedence: this field > built-in default (`DEFAULT_TODO_GATE_MAX_FIRES`).
    /// No CLI override. See `session::acp_session::resolve_reminder_policy`.
    #[serde(default)]
    pub todo_gate_max_fires_per_prompt: Option<u32>,
    #[serde(default)]
    pub auto_wake_enabled: Option<bool>,
    #[serde(default)]
    pub cursor_skills_enabled: Option<bool>,
    #[serde(default)]
    pub cursor_rules_enabled: Option<bool>,
    #[serde(default)]
    pub cursor_agents_enabled: Option<bool>,
    #[serde(default)]
    pub claude_skills_enabled: Option<bool>,
    #[serde(default)]
    pub claude_rules_enabled: Option<bool>,
    #[serde(default)]
    pub claude_agents_enabled: Option<bool>,
    #[serde(default)]
    pub cursor_mcps_enabled: Option<bool>,
    #[serde(default)]
    pub cursor_hooks_enabled: Option<bool>,
    #[serde(default)]
    pub claude_mcps_enabled: Option<bool>,
    #[serde(default)]
    pub claude_hooks_enabled: Option<bool>,
    #[serde(default)]
    pub cursor_sessions_enabled: Option<bool>,
    #[serde(default)]
    pub claude_sessions_enabled: Option<bool>,
    #[serde(default)]
    pub codex_sessions_enabled: Option<bool>,
    /// When `Some(true)`, enable goal mode remotely.
    /// When `Some(false)`, force-disable it (kill-switch).
    /// Absent ⇒ client default (enabled).
    #[serde(default)]
    pub goal_enabled: Option<bool>,
    /// When `Some(true)`, enable the goal-completion classifier remotely.
    /// When `Some(false)`, force-disable it.
    /// Absent ⇒ default tracks goal mode (enabled iff goal mode is on).
    #[serde(default)]
    pub goal_classifier_enabled: Option<bool>,
    /// When `Some(true)`, enable the goal planner remotely.
    /// When `Some(false)`, force-disable it.
    /// Absent ⇒ default tracks goal mode (enabled iff goal mode is on).
    #[serde(default)]
    pub goal_planner_enabled: Option<bool>,
    /// When `Some(true)`, enable the goal summarizer remotely (the one-shot
    /// closing "what was accomplished" summary on a verified achievement).
    /// When `Some(false)`, force-disable it (kill-switch).
    /// Absent ⇒ default tracks goal mode (enabled iff goal mode is on).
    #[serde(default)]
    pub goal_summary_enabled: Option<bool>,
    /// Number of adversarial skeptics spawned per goal-verification
    /// attempt (step ② of the staged gate). Clamped to `1..=5` at the
    /// resolver. Absent ⇒ harness default of
    /// `goal_classifier::GOAL_VERIFIER_SKEPTIC_COUNT` (3 today).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_verifier_count: Option<u32>,
    /// Maximum per-goal classifier runs before the goal auto-pauses
    /// (BackOff). Clamped to `1..=10` at the resolver. Absent ⇒ harness
    /// default of `goal_classifier::GOAL_CLASSIFIER_MAX_RUNS_DEFAULT`
    /// (3 today).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_classifier_max_runs: Option<u32>,
    /// Fire the stall-triggered strategist every N consecutive
    /// `NotAchieved` verifications. Clamped to `>= 1` at the resolver.
    /// Absent ⇒ default of `max(1, goal_classifier_max_runs / 2)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_strategist_every: Option<u32>,
    /// Planner role model+toolset. Absent ⇒ inherit current model. A
    /// present-but-malformed value is tolerantly dropped to `None` (not a
    /// hard parse error) so it cannot nuke the whole `RemoteSettings`
    /// payload (see [`deserialize_tolerant_goal_role_model`]).
    #[serde(
        default,
        deserialize_with = "deserialize_tolerant_goal_role_model",
        skip_serializing_if = "Option::is_none"
    )]
    pub goal_planner_model: Option<GoalRoleModel>,
    /// Strategist role model+toolset. Absent ⇒ inherit current model. A
    /// present-but-malformed value is tolerantly dropped to `None`
    /// (see [`deserialize_tolerant_goal_role_model`]).
    #[serde(
        default,
        deserialize_with = "deserialize_tolerant_goal_role_model",
        skip_serializing_if = "Option::is_none"
    )]
    pub goal_strategist_model: Option<GoalRoleModel>,
    /// Ordered skeptic pool. `pool[0]` = skeptic-0's model; skeptics
    /// `1..N` are assigned round-robin over the pool. Empty/absent ⇒
    /// inherit the current model. A single malformed pool entry is
    /// dropped rather than discarding the whole pool (see
    /// [`deserialize_tolerant_goal_skeptic_models`]).
    #[serde(
        default,
        deserialize_with = "deserialize_tolerant_goal_skeptic_models",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub goal_skeptic_models: Vec<GoalRoleModel>,
    /// Remote fallback for managed MCP connector fetching.
    #[serde(default)]
    pub managed_mcps_enabled: Option<bool>,
    #[serde(default)]
    pub managed_mcp_gateway_tools_enabled: Option<bool>,
    /// Fleet kill switch for the **external OTEL** stream (customer
    /// collectors). Restrictive-only by construction: there is deliberately
    /// no `external_otel_enabled` remote field — remote settings are fetched
    /// per-run and never persisted, so a remote "enable" could never reach
    /// init; org-wide enable ships via managed config instead. Applied
    /// in-process (tighten-only) via
    /// `cf_telemetry::external::apply_remote_policy`.
    #[serde(default)]
    pub external_otel_disabled: Option<bool>,
    /// Force the external stream's content gates (`OTEL_LOG_USER_PROMPTS`,
    /// `OTEL_LOG_TOOL_DETAILS`) off regardless of local env/config.
    /// Tighten-only, like `external_otel_disabled`.
    #[serde(default)]
    pub external_otel_content_gates_locked: Option<bool>,
    #[serde(default)]
    pub telemetry_enabled: Option<bool>,
    /// Telemetry mode override (string): `"session-metrics"`, `"full"`, `"off"`.
    /// Takes precedence over `telemetry_enabled` (bool) when present.
    #[serde(default)]
    pub telemetry_mode: Option<String>,
    #[serde(default)]
    pub trace_upload_enabled: Option<bool>,
    /// Enable user-facing feedback (heuristic popups, `/feedback` command).
    /// Session analytics (signal sync, turn deltas) are gated separately
    /// by `telemetry_enabled`.
    #[serde(default)]
    pub feedback_enabled: Option<bool>,
    /// Two-pass (prefire) compaction. When approaching the auto-compact
    /// threshold the shell speculatively summarizes the history prefix in the
    /// background (pass 1 → NOTE₁); at compaction it summarizes NOTE₁ + the
    /// recent tail (pass 2 → final summary), keeping summarizer latency off the
    /// critical path. `Some(true)` enables (remote rollout), `Some(false)` forces
    /// off, `None` falls back to `[features] two_pass_compaction` /
    /// `QIDI_TWO_PASS_COMPACTION` / default (off).
    #[serde(default)]
    pub two_pass_compaction_enabled: Option<bool>,
    /// Dynamic tip list from remote settings. When present with non-empty entries,
    /// one tip is shown at startup (rotated daily by UTC day).
    /// `None` or `[]` = no tips shown.
    #[serde(default)]
    pub tips: Option<Vec<String>>,
    /// When present, controls the non-Git-repo warning at session start.
    /// Controlled via remote settings (`non_git_warning` in `qidi_build_settings`).
    /// Takes precedence over `[features] non_git_warning` in config.toml:
    /// `Some(true)` enables, `Some(false)` acts as a kill-switch, `None` falls back to local config.
    #[serde(default)]
    pub non_git_warning: Option<bool>,
    /// remote settings gate for first-run auto-registration of the official xAI
    /// marketplace source. `Some(true)` enables, `Some(false)` is a kill-switch,
    /// `None` falls back to env/default (off).
    #[serde(default)]
    pub official_marketplace_auto_register: Option<bool>,
    /// remote settings gate for the inline plugin-install CTA (keyword-matched
    /// marketplace upsell above the prompt). `Some(true)` enables, `Some(false)`
    /// is a kill-switch, `None` falls back to env/default (off).
    #[serde(default)]
    pub plugin_cta: Option<bool>,
    /// Remote announcements list from proxy. Malformed items are skipped entirely.
    /// `None` or `[]` = no announcements to display.
    #[serde(default, deserialize_with = "deserialize_tolerant_announcements")]
    pub announcements: Option<Vec<RemoteAnnouncement>>,
    #[serde(default)]
    pub web_search_model: Option<String>,
    #[serde(default)]
    pub session_summary_model: Option<String>,
    #[serde(default)]
    pub image_description_model: Option<String>,
    /// Server-side pin for the next-prompt suggestion model (tab-autocomplete
    /// ghost text), from the `qidi_build_settings` remote settings flag. Sits below
    /// env (`QIDI_PROMPT_SUGGESTIONS_MODEL`) and `[models] prompt_suggestion`
    /// in config.toml, above the client hint and the built-in
    /// `cf-tools-0.1` default. The effective model is catalog-guarded: when
    /// it is not in the shell's model catalog the suggestion request is
    /// skipped entirely (never the session model). See
    /// `ModelOverrideConfig::resolve` and `handle_suggest_prompt`.
    #[serde(default)]
    pub prompt_suggestion_model: Option<String>,
    /// Server-recommended default model ID for new sessions.
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub campaigns: Vec<CampaignOverride>,
    /// When `Some(true)`, foreground commands that hit the default timeout are
    /// auto-backgrounded instead of killed. Fallback when no local
    /// `[toolset.bash] auto_background_on_timeout` is set in config.toml.
    #[serde(default)]
    pub auto_background_on_timeout: Option<bool>,
    /// When `Some(false)`, foreground commands containing a background `&`
    /// operator are rejected. Fallback when no local `[toolset.bash]
    /// allow_background_operator` is set; absent → client default (allow).
    #[serde(default)]
    pub allow_background_operator: Option<bool>,
    /// remote settings fallback for `[toolset.ask_user_question] timeout_enabled`.
    /// When `Some(false)`, questionnaires wait forever unless a higher tier
    /// (requirements / env / user / managed config) sets otherwise.
    #[serde(default)]
    pub ask_user_question_timeout_enabled: Option<bool>,
    /// remote settings fallback for `[toolset.ask_user_question] timeout_secs`
    /// (positive seconds). Absent → client default (1800 / 30 minutes).
    #[serde(default)]
    pub ask_user_question_timeout_secs: Option<u64>,
    /// When `Some(true)`, a completed subagent's isolated worktree is snapshotted
    /// into a durable git ref and its directory deleted (resume rehydrates from
    /// the ref). Fallback when no local `[features] subagent_worktree_snapshot`
    /// is set in config.toml. Absent → default (**disabled** — ships dark).
    #[serde(default)]
    pub subagent_worktree_snapshot_enabled: Option<bool>,
    /// When `Some(true)`, enable the `image_gen` tool for session-based auth users.
    /// When `Some(false)` or absent, the tool is hidden regardless of credentials.
    #[serde(default)]
    pub image_gen_enabled: Option<bool>,
    /// remote settings flag: optional Imagine model override for `image_gen`.
    /// When present and non-empty, `image_gen` uses this model slug
    /// (e.g. `grok-imagine-image`) instead of the default quality model
    /// (`grok-imagine-image-quality`). Absent/empty → default model.
    #[serde(default)]
    pub image_gen_model_override: Option<String>,
    /// When `Some(true)`, enable the `video_gen` tool for session-based auth users.
    /// When `Some(false)` or absent, the tool is hidden regardless of credentials.
    #[serde(default)]
    pub video_gen_enabled: Option<bool>,
    /// When `Some(true)`, enable the process-wide image normalize cache that
    /// amortises decode + integrity-check + re-encode work across SessionActors.
    /// Default: disabled. See `session::normalize_cache`.
    #[serde(default)]
    pub image_normalize_cache_enabled: Option<bool>,
    /// When `Some(true)`, enrich path-not-found errors with CWD reminders,
    /// "did you mean?" corrections, and similar-name suggestions.
    /// When `Some(false)` or absent, error messages are unchanged.
    #[serde(default)]
    pub path_not_found_hints: Option<bool>,
    /// Remote enable tier for the per-tip contextual hints. Each field is a
    /// soft default for one tip: `Some(false)` disables, `Some(true)` enables,
    /// absent/null ⇒ client default (on). User config beats this tier.
    #[serde(default)]
    pub contextual_hints: Option<ContextualHintsRemote>,
    /// Server-recommended worktree creation type. Fallback when no local
    /// `[cli] worktree_type` is set in config.toml.
    #[serde(default)]
    pub worktree_type: Option<String>,
    /// Server-recommended default for `restore_code` in worktree resume.
    /// Fallback when no local `[cli] restore_code` is set in config.toml.
    #[serde(default)]
    pub restore_code: Option<bool>,
    /// When `Some(true)`, Ctrl+C before the first server activity rewinds
    /// the prompt back into the input box instead of cancelling the turn.
    #[serde(default)]
    pub cancel_rewind_enabled: Option<bool>,
    /// Enables the session recap feature (`/recap` + automatic return-from-away).
    /// Optional remote kill-switch; shell defaults ON when unset (set `false` to disable).
    #[serde(default)]
    pub session_recap: Option<bool>,
    /// Enables the `ask_user_question` tool. Optional remote kill-switch:
    /// `Some(false)` strips the tool; `Some(true)` or absent → the shell
    /// default (ON). Feature-flagged via remote settings.
    #[serde(default)]
    pub ask_user_question_enabled: Option<bool>,
    /// When `Some(true)`, enable the `web_fetch` tool.
    /// When `Some(false)` or absent, the tool is not registered.
    /// Feature-flagged via remote settings for gradual rollout.
    #[serde(default)]
    pub web_fetch_enabled: Option<bool>,
    /// Egress proxy endpoint for the web_fetch tool.
    /// Fallback when no local `[toolset.web_fetch] proxy_endpoint` is set.
    #[serde(default)]
    pub web_fetch_proxy: Option<String>,
    /// Domain allowlist for the web_fetch tool.
    /// Fallback when no local `[toolset.web_fetch] allowed_domains` is set.
    #[serde(default)]
    pub web_fetch_allowed_domains: Option<Vec<String>>,
    /// When `Some(false)`, hide the resolved model ID in /session-info.
    #[serde(default)]
    pub show_resolved_model: Option<bool>,
    /// When `Some(true)`, enable session sharing.
    /// When `Some(false)` or absent, sharing is disabled.
    #[serde(default)]
    pub sharing_enabled: Option<bool>,
    /// Voice mode (STT dictation). Client default is **on** when absent.
    /// `Some(false)` is a remote kill switch; `Some(true)` forces on.
    /// Overridable locally via `QIDI_VOICE_MODE`. Free-tier SuperGrok upsell
    /// is a separate client tier gate.
    #[serde(default)]
    pub voice_mode_enabled: Option<bool>,
    /// Whether ZDR (Zero Data Retention) users are allowed to use the product.
    /// Controlled via remote settings. Default `false` (blocked) during beta.
    #[serde(default)]
    pub zdr_access_enabled: Option<bool>,
    /// remote settings tier of the `remember_tool_approvals` gate (whether per-tool
    /// "Always allow …" prompt options are shown). Lowest precedence; typically
    /// targeted per-org. Default `false`.
    #[serde(default)]
    pub remember_tool_approvals: Option<bool>,
    /// remote settings tier of the crash-handler install gate. Lowest precedence in
    /// `resolve_crash_handler_enabled`; default off. `Some(false)` is a kill-switch.
    #[serde(default)]
    pub crash_handler_enabled: Option<bool>,
    /// Whether the TUI shows agent thinking/reasoning blocks in scrollback.
    /// `None` defers to local config / env / default (`true`).
    /// `Some(false)` is a remote kill-switch. Resolved via
    /// `resolve_show_thinking_blocks` (requirements > env > user > managed >
    /// remote > default true).
    #[serde(default)]
    pub show_thinking_blocks: Option<bool>,
    /// Whether the TUI folds runs of consecutive non-destructive tool calls
    /// (reads/searches/lists) into one transcript row. `None` defers to local
    /// config / env / default (`true`). `Some(false)` is a remote
    /// kill-switch. Resolved via `resolve_group_tool_verbs` (requirements >
    /// env > user > managed > remote > default true).
    #[serde(default)]
    pub group_tool_verbs: Option<bool>,
    /// Whether the TUI shows Edit tool calls as a collapsed one-line `+N/-M`
    /// diffstat summary by default (expand for the diff). `None` defers to
    /// local config / env / default (`false`); `Some(false)` is a remote kill
    /// switch. Resolved via `resolve_collapsed_edit_blocks` (requirements >
    /// env > user > managed > remote > default false). Explicit pager.toml
    /// `[scrollback.blocks.edit]` shape keys override the flag client-side.
    #[serde(default)]
    pub collapsed_edit_blocks: Option<bool>,
    /// Display-refresh probe + auto-cadence. See [`DisplayRefreshSettings`].
    /// Partial object falls through per-field; resolved via `resolve_display_refresh`.
    #[serde(default)]
    pub display_refresh: Option<DisplayRefreshSettings>,
    /// Raw remote settings JSON for the `[auto_mode]` table (gate `enabled`,
    /// `prompt_type`, `classifier_model`). Coerced into the shell's typed
    /// `AutoModeConfig` (config-types stays dependency-light). Lowest-precedence
    /// layer in `resolve_auto_permission_mode_enabled` (client default ON).
    #[serde(default)]
    pub auto_mode: Option<serde_json::Value>,
    /// Soft default permission mode (`"ask"` / `"auto"` / `"always-approve"` /
    /// `"default"`). Used only when no effective TOML permission key is set.
    #[serde(default)]
    pub permission_mode: Option<String>,
    /// User's subscription tier from remote settings `qidi_build_access_gate`.
    /// E.g. "free", "premium", "supergrok", "supergrok_heavy".
    /// Stamped on analytics events + user profile for filtering.
    #[serde(default)]
    pub subscription_tier: Option<String>,
    #[serde(default)]
    pub gate_message: Option<String>,
    #[serde(default)]
    pub gate_url: Option<String>,
    #[serde(default)]
    pub gate_label: Option<String>,
    /// Whether the session picker groups entries by repo name.
    /// When `None` or `Some(false)`, sessions are shown in a flat list.
    #[serde(default)]
    pub session_picker_grouped: Option<bool>,
    /// Whether the user is allowed to use QIDI Code. Set by remote settings
    /// `qidi_build_access_gate` targeting rules. `None` = no server response
    /// yet (client uses own fallback check). `Some(false)` = blocked.
    #[serde(default)]
    pub allow_access: Option<bool>,
    /// User-friendly display name for the current subscription tier
    /// (e.g. "SuperGrok", "X Premium+", "Free", "API Key"). Set by CCP
    /// from the JWT tier claim (OAuth) or credential kind (API key).
    /// Free/Invalid OAuth → `"Free"`; API keys → `"API Key"` (Mixpanel
    /// `api_key`, never free).
    #[serde(default)]
    pub subscription_tier_display: Option<String>,
    /// Whether on-demand credit usage is enabled. When `Some(false)`, the
    /// billing extension blocks on-demand cap changes.
    #[serde(default)]
    pub on_demand_enabled: Option<bool>,
    /// When set to a non-empty URL, the pager's `/usage` command shows a link
    /// to that URL instead of fetching billing data from the backend.
    /// Server-controlled via the remote settings `qidi_build_usage_redirect_url`
    /// feature flag (target it at personal-team users). `None`/empty keeps the
    /// default behaviour of fetching usage from the backend.
    #[serde(default)]
    pub usage_billing_redirect_url: Option<String>,
    /// Enable the shell command suggestion pipeline remotely.
    #[serde(default)]
    pub suggestions_enabled: Option<bool>,
    /// Enable AI-powered shell command suggestions remotely.
    #[serde(default)]
    pub suggestions_ai_enabled: Option<bool>,
    /// Global auto-compact threshold percent (0-100) from remote settings
    /// `qidi_build_settings`. Per-model override on `ModelInfo`
    /// (`qidi_build_models`) takes precedence; user config and env var
    /// further override per the resolver chain.
    #[serde(default)]
    pub auto_compact_threshold_percent: Option<u8>,
    /// Global system-prompt identity label. Per-model override wins; see
    /// `resolve_system_prompt_label`.
    #[serde(default)]
    pub system_prompt_label: Option<String>,
    /// Global per-compaction wall-clock budget (seconds) from remote settings;
    /// `0` disables. Env (`QIDI_COMPACTION_WALL_CLOCK_SECS`) overrides it.
    /// Resolved via `resolve_compaction_wall_clock_budget_secs`.
    #[serde(default)]
    pub compaction_wall_clock_budget_secs: Option<u64>,
    /// Compaction mode (`summary` | `transcript` | `segments`) from remote settings.
    /// Env (`QIDI_COMPACTION_MODE`) and user config override it.
    #[serde(default)]
    pub compaction_mode: Option<String>,
    /// Segments verbatim detail (`none` | `minimal` | `balanced` | `verbose`)
    /// from remote settings. Env (`QIDI_COMPACTION_DETAIL`) and config override it.
    #[serde(default)]
    pub compaction_detail: Option<String>,
    /// remote settings verbatim-input flag; env (`QIDI_COMPACTION_VERBATIM_INPUT`) and config override it. `None` = default (true).
    #[serde(default)]
    pub compaction_verbatim_input: Option<bool>,
    /// remote settings denylist of optional imagine tools to disable
    /// (e.g. `["image_edit"]`). When a tool is listed it is authoritatively
    /// removed from the toolset and local env/config can't re-enable it.
    /// Absent or not listed → each tool keeps its own default.
    /// See `Config::resolve_image_edit`.
    #[serde(default)]
    pub imagine_tools_disabled: Option<Vec<String>>,
    /// remote settings gate for the `grok workspace` CLI command (Computer Hub
    /// workspace exposure), from `qidi_build_settings.workspace_command_enabled`.
    /// `Some(true)` enables it; `None`/`Some(false)` (the default) keep it off.
    #[serde(default)]
    pub workspace_command_enabled: Option<bool>,
    /// Master switch for jemalloc heap sampling + threshold dumps.
    /// `Some(true)` enables, `Some(false)` kill-switch, `None` = client default off.
    #[serde(default)]
    pub jemalloc_heap_profile_enabled: Option<bool>,
    /// Resident-byte thresholds (e.g. 2G/5G/10G as byte counts).
    /// `None` and `[]` are distinct on the wire.
    #[serde(default)]
    pub jemalloc_heap_profile_thresholds_bytes: Option<Vec<u64>>,
    /// Stats poll interval in seconds when set.
    #[serde(default)]
    pub jemalloc_heap_profile_poll_interval_secs: Option<u64>,
}
impl RemoteSettings {
    /// Denylist check for an optional imagine tool. Returns `true` when the
    /// server sent `imagine_tools_disabled` and it contains `tool` (force-off);
    /// otherwise `false` (defer to the tool's own default).
    pub fn imagine_tool_disabled(&self, tool: &str) -> bool {
        self.imagine_tools_disabled
            .as_ref()
            .is_some_and(|list| list.iter().any(|t| t == tool))
    }
}
/// Remote enable tier for the per-tip contextual hints (mirrors the client's
/// `[ui.contextual_hints]` shape). Each field is a soft default for one tip;
/// `None` defers to the client default (on). All fields `#[serde(default)]` so
/// a partial object from remote settings never fails the whole `RemoteSettings` parse.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct ContextualHintsRemote {
    /// Undo tip (Ctrl+Z after a substantial draft wipe).
    #[serde(default)]
    pub undo: Option<bool>,
    /// Plan-mode nudge (typing a planning keyword).
    #[serde(default)]
    pub plan_mode: Option<bool>,
    /// Clipboard-image input tip.
    #[serde(default)]
    pub image_input: Option<bool>,
    /// Send-now tip after queuing a mid-turn follow-up (InterjectPrompt chord).
    #[serde(default)]
    pub send_now: Option<bool>,
    /// Small-screen tip (`/compact-mode` hint on smallish terminals).
    #[serde(default)]
    pub small_screen: Option<bool>,
    /// Word-select tip after double-click fold/nav (settings discoverability).
    #[serde(default)]
    pub word_select: Option<bool>,
}
/// Tolerant deserializer for `Option<Vec<RemoteAnnouncement>>`.
/// Parses as Vec<Value>, tries each as RemoteAnnouncement, drops failures.
/// This ensures one bad item does not poison the whole RemoteSettings.
/// Logs a warning when malformed items are dropped.
fn deserialize_tolerant_announcements<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<RemoteAnnouncement>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<serde_json::Value> = serde::Deserialize::deserialize(deserializer)?;
    match opt {
        None => Ok(None),
        Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                match serde_json::from_value::<RemoteAnnouncement>(item) {
                    Ok(a) => out.push(a),
                    Err(e) => {
                        tracing::warn!(
                            error = % e,
                            "remote settings announcements: dropped malformed item"
                        );
                    }
                }
            }
            Ok(Some(out))
        }
        Some(_) => Ok(None),
    }
}
/// Parse one JSON value as a [`GoalRoleModel`], returning `None` (with a
/// `tracing::warn!`) instead of erroring when the value is malformed.
/// Shared by the tolerant deserializers for the single-pair role fields
/// and the skeptic pool so all three goal-role-model fields drop bad
/// remote payloads rather than failing the whole `RemoteSettings` parse.
fn parse_goal_role_model_tolerant(value: serde_json::Value) -> Option<GoalRoleModel> {
    match serde_json::from_value::<GoalRoleModel>(value) {
        Ok(model) => Some(model),
        Err(e) => {
            tracing::warn!(
                error = % e, "remote settings goal role model: dropped malformed value"
            );
            None
        }
    }
}
/// Tolerant deserializer for `Option<GoalRoleModel>` (the single-pair
/// role fields). Parses as `Option<Value>`; a present-but-malformed value
/// (or an explicit `null`) maps to `None` via
/// [`parse_goal_role_model_tolerant`] rather than erroring, so one bad
/// remote payload cannot nuke the whole `RemoteSettings` parse.
fn deserialize_tolerant_goal_role_model<'de, D>(
    deserializer: D,
) -> Result<Option<GoalRoleModel>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<serde_json::Value> = serde::Deserialize::deserialize(deserializer)?;
    Ok(match opt {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => parse_goal_role_model_tolerant(value),
    })
}
/// Tolerant deserializer for `Vec<GoalRoleModel>` (the skeptic pool).
/// Parses as `Option<Value>`; a non-array value (or null/absent) yields
/// an empty pool, and within an array each malformed entry is dropped
/// (via [`parse_goal_role_model_tolerant`]) instead of nuking the whole
/// pool. Survivor order is preserved — the skeptic round-robin assignment
/// (`expand_skeptic_assignment`) depends on pool order.
fn deserialize_tolerant_goal_skeptic_models<'de, D>(
    deserializer: D,
) -> Result<Vec<GoalRoleModel>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<serde_json::Value> = serde::Deserialize::deserialize(deserializer)?;
    match opt {
        Some(serde_json::Value::Array(arr)) => Ok(arr
            .into_iter()
            .filter_map(parse_goal_role_model_tolerant)
            .collect()),
        _ => Ok(Vec::new()),
    }
}
/// A model + the harness whose system prompt / toolset flavor that model must
/// run against. The pair is the atomic configurable unit because a model is
/// only guaranteed to work with a compatible harness (cursor vs cf-tools).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalRoleModel {
    /// Model id, e.g. "grok-4". Resolved against available models at
    /// spawn time; unknown/unauthorized ⇒ fail-open to current model.
    pub model: String,
    /// Harness `agent_type` (e.g. "cursor", "cf-tools-plan") whose
    /// `AgentDefinition` decides the role subagent's harness flavor (system
    /// prompt + cursor-vs-cf-tools toolset), applied REGARDLESS of the
    /// session/parent agent. Resolved by NAME (project/plugin/builtin lookup,
    /// then re-flavored by the subagent toolset resolver) — NOT via the main
    /// session's env/ACP/strict-harness precedence chain. NOT a subagent type:
    /// the role always spawns `general-purpose`, so the harness only re-flavors
    /// that toolset. An `agent_type` that doesn't resolve, that resolves to a
    /// strict harness whose flavor the subagent system can't represent (e.g.
    /// `codex`), or whose role toolset can't satisfy the role, fails open to the
    /// session model + harness before commit.
    pub agent_type: String,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_settings_vendor_sessions_round_trip_and_default_absent() {
        let session_flags = |settings: &RemoteSettings| {
            (
                settings.cursor_sessions_enabled,
                settings.claude_sessions_enabled,
                settings.codex_sessions_enabled,
            )
        };
        let json = r#"{
            "cursor_sessions_enabled": true,
            "claude_sessions_enabled": false,
            "codex_sessions_enabled": true
        }"#;
        let settings: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(
            session_flags(&settings),
            (Some(true), Some(false), Some(true))
        );
        let serialized = serde_json::to_string(&settings).unwrap();
        let round_trip: RemoteSettings = serde_json::from_str(&serialized).unwrap();
        assert_eq!(
            session_flags(&round_trip),
            (Some(true), Some(false), Some(true))
        );
        let absent: RemoteSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(session_flags(&absent), (None, None, None));
    }
    #[test]
    fn remote_settings_image_description_model_round_trip() {
        let json = r#"{"image_description_model": "cf-tools"}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.image_description_model.as_deref(), Some("cf-tools"));
        let out = serde_json::to_string(&s).unwrap();
        let s2: RemoteSettings = serde_json::from_str(&out).unwrap();
        assert_eq!(s2.image_description_model, s.image_description_model);
    }
    #[test]
    fn remote_settings_prompt_suggestion_model_round_trip() {
        let json = r#"{"prompt_suggestion_model": "cf-tools-0.1"}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.prompt_suggestion_model.as_deref(), Some("cf-tools-0.1"));
        let out = serde_json::to_string(&s).unwrap();
        let s2: RemoteSettings = serde_json::from_str(&out).unwrap();
        assert_eq!(s2.prompt_suggestion_model, s.prompt_suggestion_model);
        let s3: RemoteSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s3.prompt_suggestion_model, None);
    }
    #[test]
    fn remote_settings_announcements_absent() {
        let json = r#"{}"#;
        let settings: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.announcements, None);
    }
    #[test]
    fn remote_settings_announcements_populated() {
        let json = r#"{"announcements": [{"id": "a", "message": "m"}]}"#;
        let settings: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(
            settings.announcements,
            Some(vec![RemoteAnnouncement {
                id: Some("a".to_string()),
                message: Some("m".to_string()),
                severity: None,
                title: None,
                cta: None,
                updated_at: None,
                expires_at: None,
                dismissible: None,
                persistent: None,
            }])
        );
    }
    #[test]
    fn remote_settings_announcements_one_bad_item_does_not_poison() {
        let json = r#"{
            "announcements": [
                {"id": "good", "message": "ok"},
                {"id": 999, "message": "bad-id-type"}
            ]
        }"#;
        let settings: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(
            settings.announcements,
            Some(vec![RemoteAnnouncement {
                id: Some("good".to_string()),
                message: Some("ok".to_string()),
                severity: None,
                title: None,
                cta: None,
                updated_at: None,
                expires_at: None,
                dismissible: None,
                persistent: None,
            }])
        );
    }
    #[test]
    fn remote_settings_goal_role_models_absent_default_clean() {
        let json = r#"{}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.goal_planner_model, None);
        assert_eq!(s.goal_strategist_model, None);
        assert!(s.goal_skeptic_models.is_empty());
    }
    #[test]
    fn remote_settings_goal_planner_model_round_trip() {
        let json =
            r#"{"goal_planner_model": {"model": "grok-4", "agent_type": "general-purpose"}}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(
            s.goal_planner_model,
            Some(GoalRoleModel {
                model: "grok-4".to_string(),
                agent_type: "general-purpose".to_string(),
            })
        );
        let out = serde_json::to_string(&s).unwrap();
        let s2: RemoteSettings = serde_json::from_str(&out).unwrap();
        assert_eq!(s2.goal_planner_model, s.goal_planner_model);
    }
    #[test]
    fn remote_settings_goal_strategist_model_round_trip() {
        let json = r#"{"goal_strategist_model": {"model": "grok-4.5", "agent_type": "cursor"}}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(
            s.goal_strategist_model,
            Some(GoalRoleModel {
                model: "grok-4.5".to_string(),
                agent_type: "cursor".to_string(),
            })
        );
        let out = serde_json::to_string(&s).unwrap();
        let s2: RemoteSettings = serde_json::from_str(&out).unwrap();
        assert_eq!(s2.goal_strategist_model, s.goal_strategist_model);
    }
    #[test]
    fn remote_settings_goal_skeptic_models_fully_valid_pool_round_trips() {
        let json = r#"{"goal_skeptic_models": [
            {"model": "grok-4", "agent_type": "general-purpose"},
            {"model": "grok-3", "agent_type": "cursor"}
        ]}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(
            s.goal_skeptic_models,
            vec![
                GoalRoleModel {
                    model: "grok-4".to_string(),
                    agent_type: "general-purpose".to_string(),
                },
                GoalRoleModel {
                    model: "grok-3".to_string(),
                    agent_type: "cursor".to_string(),
                },
            ]
        );
        let out = serde_json::to_string(&s).unwrap();
        let s2: RemoteSettings = serde_json::from_str(&out).unwrap();
        assert_eq!(s2.goal_skeptic_models, s.goal_skeptic_models);
    }
    #[test]
    fn remote_settings_goal_skeptic_models_one_bad_item_does_not_poison_pool() {
        let json = r#"{"goal_skeptic_models": [
            {"model": "grok-4", "agent_type": "general-purpose"},
            {"model": "grok-broken"},
            {"model": "grok-3", "agent_type": "cursor"}
        ]}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(
            s.goal_skeptic_models,
            vec![
                GoalRoleModel {
                    model: "grok-4".to_string(),
                    agent_type: "general-purpose".to_string(),
                },
                GoalRoleModel {
                    model: "grok-3".to_string(),
                    agent_type: "cursor".to_string(),
                },
            ]
        );
    }
    #[test]
    fn remote_settings_goal_skeptic_models_null_yields_empty() {
        let json = r#"{"goal_skeptic_models": null}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert!(s.goal_skeptic_models.is_empty());
    }
    #[test]
    fn remote_settings_goal_skeptic_models_empty_array_yields_empty() {
        let json = r#"{"goal_skeptic_models": []}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert!(s.goal_skeptic_models.is_empty());
    }
    #[test]
    fn remote_settings_goal_skeptic_models_all_entries_bad_yields_empty() {
        let json = r#"{"goal_skeptic_models": [
            {"model": "only-model"},
            {"agent_type": "only-agent-type"},
            "scalar-not-an-object",
            42
        ]}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert!(s.goal_skeptic_models.is_empty());
    }
    #[test]
    fn remote_settings_goal_skeptic_models_non_array_yields_empty() {
        for json in [
            r#"{"goal_skeptic_models": {"model": "x", "agent_type": "y"}}"#,
            r#"{"goal_skeptic_models": "not-an-array"}"#,
            r#"{"goal_skeptic_models": 7}"#,
        ] {
            let s: RemoteSettings = serde_json::from_str(json).unwrap();
            assert!(
                s.goal_skeptic_models.is_empty(),
                "non-array pool must yield empty for {json}"
            );
        }
    }
    #[test]
    fn remote_settings_goal_skeptic_models_missing_model_entry_dropped() {
        let json = r#"{"goal_skeptic_models": [
            {"agent_type": "general-purpose"},
            {"model": "grok-3", "agent_type": "cursor"}
        ]}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(
            s.goal_skeptic_models,
            vec![GoalRoleModel {
                model: "grok-3".to_string(),
                agent_type: "cursor".to_string(),
            }]
        );
    }
    #[test]
    fn remote_settings_goal_skeptic_models_wrong_typed_scalar_dropped() {
        let json = r#"{"goal_skeptic_models": [
            {"model": 123, "agent_type": "general-purpose"},
            {"model": "grok-3", "agent_type": ["cursor"]},
            {"model": "grok-4", "agent_type": "general-purpose"}
        ]}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(
            s.goal_skeptic_models,
            vec![GoalRoleModel {
                model: "grok-4".to_string(),
                agent_type: "general-purpose".to_string(),
            }]
        );
    }
    #[test]
    fn remote_settings_goal_skeptic_models_extra_unknown_fields_kept() {
        let json = r#"{"goal_skeptic_models": [
            {"model": "grok-4", "agent_type": "general-purpose", "reasoning_effort": "high"}
        ]}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(
            s.goal_skeptic_models,
            vec![GoalRoleModel {
                model: "grok-4".to_string(),
                agent_type: "general-purpose".to_string(),
            }]
        );
    }
    #[test]
    fn remote_settings_goal_skeptic_models_survivor_order_preserved() {
        let json = r#"{"goal_skeptic_models": [
            {"model": "first", "agent_type": "general-purpose"},
            {"model": "bad"},
            {"model": "second", "agent_type": "cursor"},
            "garbage",
            {"model": "third", "agent_type": "general-purpose"}
        ]}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        let order: Vec<&str> = s
            .goal_skeptic_models
            .iter()
            .map(|m| m.model.as_str())
            .collect();
        assert_eq!(order, vec!["first", "second", "third"]);
    }
    #[test]
    fn remote_settings_goal_planner_model_malformed_yields_none() {
        for json in [
            r#"{"goal_planner_model": {"model": "only-model"}}"#,
            r#"{"goal_planner_model": {"agent_type": "only-agent-type"}}"#,
            r#"{"goal_planner_model": {"model": 1, "agent_type": "x"}}"#,
            r#"{"goal_planner_model": "scalar"}"#,
            r#"{"goal_planner_model": null}"#,
        ] {
            let s: RemoteSettings = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("must not hard-error for {json}: {e}"));
            assert_eq!(s.goal_planner_model, None, "for {json}");
        }
    }
    #[test]
    fn remote_settings_goal_strategist_model_malformed_yields_none() {
        for json in [
            r#"{"goal_strategist_model": {"model": "only-model"}}"#,
            r#"{"goal_strategist_model": {"agent_type": ["x"]}}"#,
            r#"{"goal_strategist_model": 42}"#,
        ] {
            let s: RemoteSettings = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("must not hard-error for {json}: {e}"));
            assert_eq!(s.goal_strategist_model, None, "for {json}");
        }
    }
    #[test]
    fn remote_settings_goal_role_models_malformed_pair_does_not_drop_other_fields() {
        let json = r#"{
            "goal_planner_model": {"model": "broken"},
            "goal_strategist_model": {"model": "grok-4.5", "agent_type": "cursor"},
            "default_model": "grok-4"
        }"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.goal_planner_model, None);
        assert_eq!(
            s.goal_strategist_model,
            Some(GoalRoleModel {
                model: "grok-4.5".to_string(),
                agent_type: "cursor".to_string(),
            })
        );
        assert_eq!(s.default_model.as_deref(), Some("grok-4"));
    }
    #[test]
    fn remote_settings_goal_role_model_extra_unknown_fields_kept_single_pair() {
        let json = r#"{"goal_planner_model": {"model": "grok-4", "agent_type": "general-purpose", "future": true}}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(
            s.goal_planner_model,
            Some(GoalRoleModel {
                model: "grok-4".to_string(),
                agent_type: "general-purpose".to_string(),
            })
        );
    }
    #[test]
    fn remote_settings_inference_idle_timeout_present() {
        let json = r#"{"inference_idle_timeout_secs": 180}"#;
        let settings: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.inference_idle_timeout_secs, Some(180));
    }
    #[test]
    fn remote_settings_initial_injection_deserialize_present() {
        let json = r#"{"memory_initial_injection_enabled": false, "memory_initial_injection_min_score": 0.66}"#;
        let settings: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.memory_initial_injection_enabled, Some(false));
        assert_eq!(settings.memory_initial_injection_min_score, Some(0.66));
    }
    #[test]
    fn remote_settings_initial_injection_deserialize_absent() {
        let json = r#"{"memory_enabled": true}"#;
        let settings: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.memory_initial_injection_enabled, None);
        assert_eq!(settings.memory_initial_injection_min_score, None);
    }
    #[test]
    fn remote_settings_inference_idle_timeout_absent() {
        let json = r#"{"memory_enabled": true}"#;
        let settings: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.inference_idle_timeout_secs, None);
    }
    #[test]
    fn remote_settings_unknown_fields_tolerated() {
        let json = r#"{
            "inference_idle_timeout_secs": 120,
            "future_remote_field": 42,
            "verification_staleness_enabled": true
        }"#;
        let settings: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.inference_idle_timeout_secs, Some(120));
    }
    #[test]
    fn remote_settings_web_fetch_fields_absent() {
        let json = r#"{}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.web_fetch_enabled, None);
        assert_eq!(s.web_fetch_proxy, None);
        assert_eq!(s.web_fetch_allowed_domains, None);
    }
    #[test]
    fn remote_settings_web_fetch_all_populated() {
        let json = r#"{
            "web_fetch_enabled": true,
            "web_fetch_proxy": "https://proxy.corp.example.com",
            "web_fetch_allowed_domains": ["docs.rs", "example.com"]
        }"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.web_fetch_enabled, Some(true));
        assert_eq!(
            s.web_fetch_proxy.as_deref(),
            Some("https://proxy.corp.example.com")
        );
        assert_eq!(
            s.web_fetch_allowed_domains,
            Some(vec!["docs.rs".to_owned(), "example.com".to_owned()])
        );
    }
    #[test]
    fn remote_settings_web_fetch_enabled_false() {
        let json = r#"{"web_fetch_enabled": false}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.web_fetch_enabled, Some(false));
        assert_eq!(s.web_fetch_proxy, None);
    }
    #[test]
    fn remote_settings_ask_user_question_absent() {
        let json = r#"{}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.ask_user_question_enabled, None);
    }
    #[test]
    fn remote_settings_ask_user_question_enabled_round_trip() {
        let json = r#"{"ask_user_question_enabled": true}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.ask_user_question_enabled, Some(true));
        let out = serde_json::to_string(&s).unwrap();
        let s2: RemoteSettings = serde_json::from_str(&out).unwrap();
        assert_eq!(s2.ask_user_question_enabled, Some(true));
    }
    #[test]
    fn remote_settings_ask_user_question_enabled_false() {
        let json = r#"{"ask_user_question_enabled": false}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.ask_user_question_enabled, Some(false));
    }
    #[test]
    fn remote_settings_web_fetch_empty_domains() {
        let json = r#"{"web_fetch_allowed_domains": []}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.web_fetch_allowed_domains, Some(vec![]));
    }
    #[test]
    fn remote_settings_display_refresh_present_partial() {
        let json = r#"{"display_refresh": {"auto_cadence_enabled": true, "floor_ms": 7}}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        let dr = s.display_refresh.expect("display_refresh present");
        assert_eq!(dr.auto_cadence_enabled, Some(true));
        assert_eq!(dr.floor_ms, Some(7));
        assert_eq!(dr.probe_enabled, None);
        assert_eq!(dr.ceiling_ms, None);
        assert_eq!(dr.min_hz, None);
        assert_eq!(dr.max_hz, None);
    }
    #[test]
    fn remote_settings_display_refresh_absent() {
        let s: RemoteSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.display_refresh, None);
    }
    #[test]
    fn remote_settings_display_refresh_unknown_keys_preserved() {
        let json =
            r#"{"display_refresh": {"probe_enabled": true, "future_knob": 42, "floor_ms": "bad"}}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        let dr = s.display_refresh.expect("display_refresh present");
        assert_eq!(dr.probe_enabled, Some(true));
        assert_eq!(dr.floor_ms, None, "wrong-typed floor_ms ignored");
        assert_eq!(dr.extra.get("future_knob"), Some(&serde_json::json!(42)));
        let out = serde_json::to_value(&dr).unwrap();
        assert_eq!(out.get("future_knob"), Some(&serde_json::json!(42)));
        assert_eq!(out.get("probe_enabled"), Some(&serde_json::json!(true)));
    }
    #[test]
    fn remote_settings_contextual_hints_present() {
        let json = r#"{"contextual_hints": {"undo": false, "plan_mode": true}}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        let hints = s.contextual_hints.expect("contextual_hints present");
        assert_eq!(hints.undo, Some(false));
        assert_eq!(hints.plan_mode, Some(true));
        assert_eq!(hints.image_input, None);
    }
    #[test]
    fn remote_settings_contextual_hints_absent() {
        let json = r#"{}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.contextual_hints, None);
    }
    #[test]
    fn remote_settings_goal_planner_enabled_present() {
        let json = r#"{"goal_planner_enabled": true}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.goal_planner_enabled, Some(true));
    }
    #[test]
    fn remote_settings_goal_planner_enabled_false() {
        let json = r#"{"goal_planner_enabled": false}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.goal_planner_enabled, Some(false));
    }
    #[test]
    fn remote_settings_goal_planner_enabled_absent() {
        let json = r#"{}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.goal_planner_enabled, None);
    }
    #[test]
    fn remote_settings_goal_summary_enabled_present() {
        let json = r#"{"goal_summary_enabled": true}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.goal_summary_enabled, Some(true));
    }
    #[test]
    fn remote_settings_goal_summary_enabled_false() {
        let json = r#"{"goal_summary_enabled": false}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.goal_summary_enabled, Some(false));
    }
    #[test]
    fn remote_settings_goal_summary_enabled_absent() {
        let json = r#"{}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.goal_summary_enabled, None);
    }
    #[test]
    fn remote_settings_folder_trust_enabled_present() {
        let json = r#"{"folder_trust_enabled": true}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.folder_trust_enabled, Some(true));
    }
    #[test]
    fn remote_settings_folder_trust_enabled_false() {
        let json = r#"{"folder_trust_enabled": false}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.folder_trust_enabled, Some(false));
    }
    #[test]
    fn remote_settings_folder_trust_enabled_absent() {
        let json = r#"{}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.folder_trust_enabled, None);
    }
    #[test]
    fn remote_settings_workspace_command_enabled_present() {
        let json = r#"{"workspace_command_enabled": true}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.workspace_command_enabled, Some(true));
    }
    #[test]
    fn remote_settings_workspace_command_enabled_false() {
        let json = r#"{"workspace_command_enabled": false}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.workspace_command_enabled, Some(false));
    }
    #[test]
    fn remote_settings_workspace_command_enabled_absent() {
        let json = r#"{}"#;
        let s: RemoteSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.workspace_command_enabled, None);
    }
    #[test]
    fn remote_settings_permission_mode_deserializes() {
        let s: RemoteSettings = serde_json::from_str(r#"{"permission_mode": "auto"}"#).unwrap();
        assert_eq!(s.permission_mode.as_deref(), Some("auto"));
        let s2: RemoteSettings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(s2.permission_mode.as_deref(), Some("auto"));
        let s: RemoteSettings =
            serde_json::from_str(r#"{"permission_mode": "always-approve"}"#).unwrap();
        assert_eq!(s.permission_mode.as_deref(), Some("always-approve"));
        let s: RemoteSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.permission_mode, None);
        let s: RemoteSettings = serde_json::from_str(r#"{"permission_mode": null}"#).unwrap();
        assert_eq!(s.permission_mode, None);
    }
    #[test]
    fn remote_settings_crash_handler_enabled_present() {
        let s: RemoteSettings = serde_json::from_str(r#"{"crash_handler_enabled": true}"#).unwrap();
        assert_eq!(s.crash_handler_enabled, Some(true));
        let s2: RemoteSettings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(s2.crash_handler_enabled, Some(true));
    }
    #[test]
    fn remote_settings_crash_handler_enabled_false() {
        let s: RemoteSettings =
            serde_json::from_str(r#"{"crash_handler_enabled": false}"#).unwrap();
        assert_eq!(s.crash_handler_enabled, Some(false));
    }
    #[test]
    fn remote_settings_crash_handler_enabled_absent() {
        let s: RemoteSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.crash_handler_enabled, None);
    }
    type JemallocFields<'a> = (Option<bool>, Option<&'a [u64]>, Option<u64>);
    fn jemalloc_fields(s: &RemoteSettings) -> JemallocFields<'_> {
        (
            s.jemalloc_heap_profile_enabled,
            s.jemalloc_heap_profile_thresholds_bytes.as_deref(),
            s.jemalloc_heap_profile_poll_interval_secs,
        )
    }
    fn parse_remote(json: &str) -> RemoteSettings {
        serde_json::from_str(json).unwrap_or_else(|e| panic!("parse failed for {json}: {e}"))
    }
    fn round_trip_remote(s: &RemoteSettings) -> RemoteSettings {
        let out = serde_json::to_string(s).unwrap();
        parse_remote(&out)
    }
    fn assert_jemalloc_round_trip(json: &str, expected: JemallocFields<'_>) {
        let s = parse_remote(json);
        assert_eq!(jemalloc_fields(&s), expected);
        assert_eq!(jemalloc_fields(&round_trip_remote(&s)), expected);
    }
    fn assert_remote_parse_err(json: &str) {
        assert!(
            serde_json::from_str::<RemoteSettings>(json).is_err(),
            "expected parse error for {json}"
        );
    }
    #[test]
    fn remote_settings_jemalloc_heap_profile_fields_absent_and_null() {
        assert_eq!(jemalloc_fields(&parse_remote("{}")), (None, None, None));
        assert_eq!(
            jemalloc_fields(&parse_remote(
                r#"{
                    "jemalloc_heap_profile_enabled": null,
                    "jemalloc_heap_profile_thresholds_bytes": null,
                    "jemalloc_heap_profile_poll_interval_secs": null
                }"#
            )),
            (None, None, None)
        );
    }
    #[test]
    fn remote_settings_jemalloc_heap_profile_enabled_true_false_round_trip() {
        assert_jemalloc_round_trip(
            r#"{"jemalloc_heap_profile_enabled": true}"#,
            (Some(true), None, None),
        );
        assert_jemalloc_round_trip(
            r#"{"jemalloc_heap_profile_enabled": false}"#,
            (Some(false), None, None),
        );
    }
    #[test]
    fn remote_settings_jemalloc_heap_profile_kill_switch_with_non_empty_thresholds() {
        assert_jemalloc_round_trip(
            r#"{
                "jemalloc_heap_profile_enabled": false,
                "jemalloc_heap_profile_thresholds_bytes": [2147483648, 5368709120],
                "jemalloc_heap_profile_poll_interval_secs": 30
            }"#,
            (Some(false), Some(&[2_147_483_648, 5_368_709_120]), Some(30)),
        );
    }
    #[test]
    fn remote_settings_jemalloc_heap_profile_thresholds_populated_and_empty_round_trip() {
        assert_jemalloc_round_trip(
            r#"{
                "jemalloc_heap_profile_thresholds_bytes": [2147483648, 5368709120, 10737418240]
            }"#,
            (
                None,
                Some(&[2_147_483_648, 5_368_709_120, 10_737_418_240]),
                None,
            ),
        );
        assert_jemalloc_round_trip(
            r#"{"jemalloc_heap_profile_thresholds_bytes": []}"#,
            (None, Some(&[]), None),
        );
        assert_jemalloc_round_trip(
            r#"{
                "jemalloc_heap_profile_enabled": true,
                "jemalloc_heap_profile_thresholds_bytes": []
            }"#,
            (Some(true), Some(&[]), None),
        );
    }
    #[test]
    fn remote_settings_jemalloc_heap_profile_poll_interval_round_trip() {
        assert_jemalloc_round_trip(
            r#"{"jemalloc_heap_profile_poll_interval_secs": 60}"#,
            (None, None, Some(60)),
        );
    }
    #[test]
    fn remote_settings_jemalloc_heap_profile_all_fields_populated_round_trip() {
        assert_jemalloc_round_trip(
            r#"{
                "jemalloc_heap_profile_enabled": true,
                "jemalloc_heap_profile_thresholds_bytes": [2147483648, 5368709120],
                "jemalloc_heap_profile_poll_interval_secs": 15
            }"#,
            (Some(true), Some(&[2_147_483_648, 5_368_709_120]), Some(15)),
        );
    }
    #[test]
    fn remote_settings_jemalloc_heap_profile_boundary_u64_values() {
        let json = format!(
            r#"{{
                "jemalloc_heap_profile_thresholds_bytes": [0, 1, {}],
                "jemalloc_heap_profile_poll_interval_secs": {}
            }}"#,
            u64::MAX,
            u64::MAX
        );
        assert_jemalloc_round_trip(&json, (None, Some(&[0, 1, u64::MAX]), Some(u64::MAX)));
    }
    #[test]
    fn remote_settings_jemalloc_heap_profile_coexists_with_unknown_keys() {
        let s = parse_remote(
            r#"{
                "jemalloc_heap_profile_enabled": true,
                "jemalloc_heap_profile_thresholds_bytes": [1073741824],
                "jemalloc_heap_profile_poll_interval_secs": 30,
                "future_jemalloc_knob": 42,
                "unrelated_remote_field": "ok"
            }"#,
        );
        assert_eq!(
            jemalloc_fields(&s),
            (Some(true), Some([1_073_741_824].as_slice()), Some(30))
        );
    }
    #[test]
    fn remote_settings_jemalloc_heap_profile_malformed_fails_whole_parse() {
        for json in [
            r#"{"jemalloc_heap_profile_enabled": "yes"}"#,
            r#"{"jemalloc_heap_profile_enabled": 1}"#,
            r#"{"jemalloc_heap_profile_enabled": []}"#,
            r#"{"jemalloc_heap_profile_enabled": {}}"#,
            r#"{"jemalloc_heap_profile_thresholds_bytes": "2G"}"#,
            r#"{"jemalloc_heap_profile_thresholds_bytes": {"bytes": 1}}"#,
            r#"{"jemalloc_heap_profile_thresholds_bytes": [true]}"#,
            r#"{"jemalloc_heap_profile_thresholds_bytes": ["2G"]}"#,
            r#"{"jemalloc_heap_profile_thresholds_bytes": [-1]}"#,
            r#"{"jemalloc_heap_profile_thresholds_bytes": [null]}"#,
            r#"{"jemalloc_heap_profile_thresholds_bytes": [1, null, 2]}"#,
            r#"{"jemalloc_heap_profile_poll_interval_secs": "30"}"#,
            r#"{"jemalloc_heap_profile_poll_interval_secs": true}"#,
            r#"{"jemalloc_heap_profile_poll_interval_secs": -1}"#,
            r#"{
                "jemalloc_heap_profile_enabled": true,
                "jemalloc_heap_profile_thresholds_bytes": "bad",
                "workspace_command_enabled": true
            }"#,
        ] {
            assert_remote_parse_err(json);
        }
    }
}

// from xai-grok-config-types/mcp.rs
//  MCP server configuration value types, extracted from qidi-code
//  (config dependency inversion).

// use agent_client_protocol as acp;
use indexmap::IndexMap;
// use serde::{Deserialize, Serialize};
use std::collections::HashMap;
// // use cf_mcp::oauth_config::McpOAuthConfig; // replaced with local stub

/// serde default helper. Kept module-local rather than shared — the `pool`
/// module keeps its own copy for `PoolConfig`.
fn default_true() -> bool {
    true
}

/// Read an MCP OAuth client secret from the named env var. Moved here with
/// `McpServerConfig` (its only caller).
fn resolve_oauth_client_secret(env_var: Option<&String>) -> Option<String> {
    let env_var = env_var?;
    match std::env::var(env_var) {
        Ok(secret) => Some(secret),
        Err(_) => {
            tracing::warn!(
                env_var = env_var.as_str(),
                "MCP OAuth client_secret env var is configured but not set in the environment; \
                 proceeding without a client secret"
            );
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum McpServerTransportConfig {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        env: Option<HashMap<String, String>>,
        /// Standard MCP JSON supports `cwd`, but ACP stdio server config does not yet expose it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    StreamableHttp {
        url: String,
        #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
        transport_type: Option<String>,
        /// Name of the environment variable to read and set for `Authorization: Bearer <token>`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bearer_token_env_var: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        headers: Option<HashMap<String, String>>,
        /// OAuth client ID for providers that don't support Dynamic Client Registration.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        oauth_client_id: Option<String>,
        /// Name of the env var holding the OAuth client secret (for BYO credentials).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        oauth_client_secret_env_var: Option<String>,
        /// OAuth scopes to request during authorization.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        oauth_scopes: Option<Vec<String>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpJsonOAuthBlock {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret_env_var: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    #[serde(flatten)]
    pub transport: McpServerTransportConfig,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<McpJsonOAuthBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup_timeout_sec: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_timeout_sec: Option<u64>,
    /// Per-tool timeout overrides in seconds: `{ "create_issue" = 120, "search" = 30 }`.
    /// Falls back to `tool_timeout_sec` for tools not listed here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_timeouts: Option<HashMap<String, u64>>,
    /// Also keep the raw base64 in tool-result text so agents can forward
    /// bytes via path-based tools (`base64 -d > /tmp/x.png && send_file ...`).
    /// ~2× tokens per image. Overridden by `_meta.mcpConfig.<server>.exposeImageBase64`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expose_image_base64: Option<bool>,
}
/// OAuth configuration extracted from an MCP server's config.
/// Matches xai-grok-mcp::oauth_config::McpOAuthConfig for re-export.
#[derive(Debug, Clone, Default)]
pub struct McpOAuthConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub scopes: Option<Vec<String>>,
    pub callback_port: Option<u16>,
}

impl McpOAuthConfig {
    pub fn is_configured(&self) -> bool {
        self.client_id.is_some()
    }
}

/// Per-server OAuth configuration map, keyed by MCP server name.
pub type McpOAuthConfigMap = std::collections::HashMap<String, McpOAuthConfig>;

impl McpServerConfig {
    pub fn expand_strings(&mut self, sub: &dyn Fn(&str) -> String) {
        match &mut self.transport {
            McpServerTransportConfig::Stdio {
                command,
                args,
                env,
                cwd,
            } => {
                *command = sub(command);
                for arg in args.iter_mut() {
                    *arg = sub(arg);
                }
                if let Some(env) = env.as_mut() {
                    for value in env.values_mut() {
                        *value = sub(value);
                    }
                }
                if let Some(cwd) = cwd.as_mut() {
                    *cwd = sub(cwd);
                }
            }
            McpServerTransportConfig::StreamableHttp { url, headers, .. } => {
                *url = sub(url);
                if let Some(headers) = headers.as_mut() {
                    for value in headers.values_mut() {
                        *value = sub(value);
                    }
                }
            }
        }
    }

    /// Extract OAuth configuration for this server, if any OAuth fields are set.
    pub fn oauth_config(&self) -> Option<McpOAuthConfig> {
        if let McpServerTransportConfig::StreamableHttp {
            oauth_client_id,
            oauth_client_secret_env_var,
            oauth_scopes,
            ..
        } = &self.transport
            && oauth_client_id.is_some()
        {
            return Some(McpOAuthConfig {
                client_id: oauth_client_id.clone(),
                client_secret: resolve_oauth_client_secret(oauth_client_secret_env_var.as_ref()),
                scopes: oauth_scopes.clone(),
                callback_port: None,
            });
        }

        if let Some(block) = &self.oauth
            && block.client_id.is_some()
        {
            return Some(McpOAuthConfig {
                client_id: block.client_id.clone(),
                client_secret: resolve_oauth_client_secret(block.client_secret_env_var.as_ref()),
                scopes: block.scopes.clone(),
                callback_port: block.callback_port,
            });
        }

        None
    }
}

/// Configuration for relay session sharing.
/// Set in config.toml under [relay] section.
///
/// Example:
/// ```toml
/// [relay]
/// enabled = true
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RelaySyncConfig {
    pub enabled: Option<bool>,
}

impl RelaySyncConfig {
    /// Check if relay sync is enabled. Env var takes precedence over config.
    pub fn is_enabled(&self) -> bool {
        if let Ok(env_val) = std::env::var("QIDI_RELAY_SYNC_ENABLED") {
            return env_val.eq_ignore_ascii_case("true") || env_val == "1";
        }
        self.enabled.unwrap_or(false)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct McpConfig {
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: IndexMap<String, McpServerConfig>,
}

// from xai-grok-config-types/memory.rs
//  Memory-system configuration value types, extracted from qidi-code
//  (config dependency inversion).
// 
//  These are the leaf `[memory.*]` and `[compaction.*]` sub-config structs.
//  The `MemoryConfig` aggregate and its `resolve()` loader stay in
//  `qidi-code` — `resolve()` depends on `toml` and on shell-internal
//  flag resolution, and is part of shell's public API (cross-crate caller).

// use serde::{Deserialize, Serialize};

/// Index and chunking configuration (`[memory.index]`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct MemoryIndexConfig {
    /// Maximum chunk size in characters (approx tokens × 4).
    pub max_chunk_chars: usize,
    /// Character overlap between consecutive chunks.
    pub chunk_overlap_chars: usize,
}

impl Default for MemoryIndexConfig {
    fn default() -> Self {
        Self {
            max_chunk_chars: 1600,
            chunk_overlap_chars: 320,
        }
    }
}

/// Embedding provider configuration (`[memory.embedding]`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct MemoryEmbeddingConfig {
    /// Provider type: `"api"`, `"local"`, or `"auto"`.
    pub provider: String,
    /// Model name for the embedding API. `None` disables vector embeddings.
    pub model: Option<String>,
    /// Embedding vector dimensions.
    pub dimensions: usize,
}

impl Default for MemoryEmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: "api".to_string(),
            model: None,
            dimensions: 1024,
        }
    }
}

/// Hybrid search scoring configuration (`[memory.search]`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct MemorySearchConfig {
    /// Maximum number of search results to return.
    pub max_results: usize,
    /// Minimum score threshold for inclusion.
    pub min_score: f32,
    /// Weight for vector similarity in hybrid scoring.
    pub vector_weight: f32,
    /// Weight for BM25 text similarity in hybrid scoring.
    pub text_weight: f32,
    /// **Deprecated** — use `temporal_decay` instead.
    ///
    /// Per-day decay factor for recency boosting (0.0–1.0).
    /// When `temporal_decay.enabled` is true, this field is ignored.
    /// When `temporal_decay.enabled` is false and this is set, it is
    /// converted to an approximate half-life for backward compatibility:
    /// `half_life ≈ -1 / log₂(recency_decay)`.
    pub recency_decay: f32,
    /// Temporal decay configuration for time-aware scoring.
    pub temporal_decay: TemporalDecayConfig,
    /// MMR diversity re-ranking configuration (opt-in).
    pub mmr: MmrConfig,
    /// Source-type weight multipliers: all default to 1.0.
    pub source_weights: std::collections::HashMap<String, f32>,
}

impl Default for MemorySearchConfig {
    fn default() -> Self {
        let mut source_weights = std::collections::HashMap::new();
        source_weights.insert("workspace".to_string(), 1.0);
        source_weights.insert("session".to_string(), 1.0);
        source_weights.insert("global".to_string(), 1.0);

        Self {
            max_results: 6,
            min_score: 0.35,
            vector_weight: 0.7,
            text_weight: 0.3,
            recency_decay: DEFAULT_RECENCY_DECAY,
            temporal_decay: TemporalDecayConfig::default(),
            mmr: MmrConfig::default(),
            source_weights,
        }
    }
}

/// Temporal decay configuration for time-aware search scoring.
///
/// Controls how memory chunk scores decay over time. Chunks from
/// "evergreen" sources (`global`, `workspace`) are exempt from decay
/// since they contain curated long-term knowledge. Only `session`
/// chunks decay, using an exponential half-life formula:
///
/// ```text
/// decayed_score = base_score × e^(-λ × age_days)
/// where λ = ln(2) / half_life_days
/// ```
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct TemporalDecayConfig {
    /// Whether temporal decay is enabled.
    pub enabled: bool,
    /// Number of days after which a session chunk's score is halved.
    pub half_life_days: f64,
}

impl Default for TemporalDecayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            half_life_days: 7.0,
        }
    }
}

/// MMR (Maximal Marginal Relevance) diversity re-ranking configuration.
///
/// When enabled, re-ranks search results to penalize redundancy. Uses
/// Jaccard similarity on tokenized snippets to measure inter-result
/// similarity, then greedily selects results that balance relevance
/// with diversity:
///
/// ```text
/// MMR(d) = λ × relevance(d) - (1-λ) × max_similarity(d, selected)
/// ```
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct MmrConfig {
    /// Whether MMR re-ranking is enabled. Default: false (opt-in).
    pub enabled: bool,
    /// Trade-off between relevance and diversity.
    /// 0.0 = maximum diversity, 1.0 = pure relevance (no re-ranking).
    /// Clamped to [0.0, 1.0] at parse time. Default: 0.7.
    #[serde(deserialize_with = "deserialize_clamped_unit")]
    pub lambda: f64,
}

impl Default for MmrConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            lambda: 0.7,
        }
    }
}

/// Deserialize an `f64` clamped to [0.0, 1.0].
///
/// Used for fields where values outside the unit interval are meaningless
/// (e.g. cosine similarity thresholds, trade-off lambdas).
fn deserialize_clamped_unit<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = f64::deserialize(deserializer)?;
    Ok(v.clamp(0.0, 1.0))
}

/// Like [`deserialize_clamped_unit`] but for `Option<f64>` fields.
fn deserialize_clamped_unit_option<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: Option<f64> = Option::deserialize(deserializer)?;
    Ok(v.map(|x| x.clamp(0.0, 1.0)))
}

/// Default value for the legacy `recency_decay` field.
pub const DEFAULT_RECENCY_DECAY: f32 = 0.95;

impl MemorySearchConfig {
    /// Resolve the effective half-life for temporal decay.
    ///
    /// Priority order:
    /// 1. `temporal_decay.enabled = true` → use `temporal_decay.half_life_days`
    /// 2. `temporal_decay.enabled = false` AND `recency_decay` differs from
    ///    the default (0.95) → convert the legacy per-day factor to an
    ///    approximate half-life: `half_life ≈ -1.0 / log₂(recency_decay)`.
    ///    This preserves behavior for users who only set `recency_decay`.
    /// 3. Otherwise → `None` (decay fully disabled).
    pub fn effective_half_life_days(&self) -> Option<f64> {
        if self.temporal_decay.enabled {
            if self.temporal_decay.half_life_days <= 0.0 {
                tracing::warn!(
                    half_life_days = self.temporal_decay.half_life_days,
                    "temporal_decay.half_life_days must be positive, disabling decay"
                );
                return None;
            }
            return Some(self.temporal_decay.half_life_days);
        }

        // Legacy backward compat: if the user explicitly set recency_decay
        // to a non-default value, convert it to an approximate half-life.
        if (self.recency_decay - DEFAULT_RECENCY_DECAY).abs() > f32::EPSILON
            && self.recency_decay > 0.0
            && self.recency_decay < 1.0
        {
            let half_life = -1.0 / (self.recency_decay as f64).log2();
            tracing::info!(
                recency_decay = self.recency_decay,
                converted_half_life_days = half_life,
                "converting legacy recency_decay to temporal decay half-life; \
                 consider migrating to [memory.search.temporal_decay]"
            );
            return Some(half_life);
        }

        None
    }
}

/// First-turn memory injection configuration (`[memory.initial_injection]`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct MemoryInitialInjectionConfig {
    /// Whether to search memory and inject a reminder on the first turn.
    pub enabled: bool,
    /// Optional score threshold override for first-turn injection.
    /// When `None`, the first-turn search uses the historical default of `0.0`
    /// (no threshold filtering).
    pub min_score: Option<f32>,
}

impl Default for MemoryInitialInjectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_score: None,
        }
    }
}

/// Session lifecycle configuration (`[memory.session]`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct MemorySessionConfig {
    /// Whether to auto-save a session summary to memory on session end.
    pub save_on_end: bool,
}

impl Default for MemorySessionConfig {
    fn default() -> Self {
        Self { save_on_end: true }
    }
}

/// autoDream consolidation configuration (`[memory.dream]`).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(default)]
pub struct MemoryDreamConfig {
    /// Whether autoDream background consolidation is enabled.
    pub enabled: bool,
    /// Minimum hours between consolidations.
    pub min_hours: u64,
    /// Minimum sessions since last consolidation to trigger.
    pub min_sessions: u64,
    /// Seconds before a stale dream lock is reclaimed.
    pub stale_lock_secs: u64,
    /// Periodic dream check interval in seconds.
    /// `None` = disabled (dream only at session end or via /dream).
    /// When set, the session actor checks dream gates on this interval.
    pub check_interval_secs: Option<u64>,
}

impl Default for MemoryDreamConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_hours: 4,
            min_sessions: 3,
            stale_lock_secs: 3600,
            check_interval_secs: None,
        }
    }
}

/// File watcher configuration for detecting external memory edits (`[memory.watcher]`).
///
/// When enabled, watches `~/.qidi/memory/` for `.md` file changes (create,
/// modify, delete) and syncs the index on the next `memory_search` call:
/// - Created/modified files are reindexed.
/// - Deleted files have their stale chunks removed from the index.
///
/// Events are coalesced in a lock-free `ArcSwap` set; sync runs at most once
/// per search call when dirty files are present and the claim is acquired.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct MemoryWatcherConfig {
    /// Whether the file watcher is enabled. Default: true (when memory is enabled).
    pub enabled: bool,
    /// Seconds after which a reindex claim is considered stale (crashed agent).
    /// Default: 60.
    pub stale_claim_secs: i64,
}

impl Default for MemoryWatcherConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            stale_claim_secs: 60,
        }
    }
}

/// Garbage collection for orphaned workspace memory directories (`[memory.gc]`).
///
/// On session init, directories under `~/.qidi/memory/` are scanned:
/// - `tmp*` dirs: empty ones removed unconditionally, non-empty ones removed
///   after 7 days.
/// - Other workspaces with no session files: removed after `max_age_days`.
/// - Non-empty non-tmp workspaces: never touched.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct MemoryGcConfig {
    pub max_age_days: u64,
}

impl Default for MemoryGcConfig {
    fn default() -> Self {
        Self { max_age_days: 30 }
    }
}

/// Pre-compaction memory flush configuration (`[compaction.memory_flush]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryFlushConfig {
    /// Whether the flush step is enabled before compaction.
    pub enabled: bool,
    /// Token headroom before the compact threshold to trigger flush.
    pub soft_threshold_tokens: u64,
    /// Model to use for the flush turn. `None` = session's primary model.
    pub flush_model: Option<String>,
    /// Max characters the flush response may write to memory.
    pub max_flush_write_chars: usize,
    /// Idle timeout in seconds: when no user message is received for this
    /// duration, a background flush is triggered automatically.
    /// `None` = disabled (flush only before compaction).
    #[serde(default)]
    pub idle_timeout_secs: Option<u64>,
    /// Cosine similarity threshold for semantic dedup of flush content.
    /// When `None`, falls back to the compiled-in default (0.92).
    /// Clamped to [0.0, 1.0] at parse time.
    #[serde(default, deserialize_with = "deserialize_clamped_unit_option")]
    pub semantic_dedup_threshold: Option<f64>,
}

impl Default for MemoryFlushConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            soft_threshold_tokens: 4000,
            flush_model: None,
            max_flush_write_chars: 8000,
            idle_timeout_secs: None,
            semantic_dedup_threshold: None,
        }
    }
}

/// Tool-result pruning configuration (`[compaction.pruning]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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

#[cfg(test)]
mod memory_config_tests {
    use super::*;

    #[test]
    fn sub_config_defaults_match() {
        assert_eq!(MemoryIndexConfig::default().max_chunk_chars, 1600);
        assert_eq!(MemoryEmbeddingConfig::default().dimensions, 1024);
        let s = MemorySearchConfig::default();
        assert_eq!(s.max_results, 6);
        assert_eq!(s.recency_decay, DEFAULT_RECENCY_DECAY);
        assert!(s.temporal_decay.enabled);
        assert!(!s.mmr.enabled);
        assert!(MemorySessionConfig::default().save_on_end);
        assert_eq!(MemoryGcConfig::default().max_age_days, 30);
        assert_eq!(PruningConfig::default().keep_last_n_turns, 3);
    }

    #[test]
    fn mmr_lambda_is_clamped_on_deserialize() {
        let m: MmrConfig = serde_json::from_str(r#"{"enabled": true, "lambda": 5.0}"#).unwrap();
        assert_eq!(m.lambda, 1.0);
        let m: MmrConfig = serde_json::from_str(r#"{"enabled": true, "lambda": -3.0}"#).unwrap();
        assert_eq!(m.lambda, 0.0);
    }

    #[test]
    fn flush_semantic_dedup_threshold_clamped_option() {
        let f: MemoryFlushConfig =
            serde_json::from_str(r#"{"semantic_dedup_threshold": 2.0}"#).unwrap();
        assert_eq!(f.semantic_dedup_threshold, Some(1.0));
        let f: MemoryFlushConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(f.semantic_dedup_threshold, None);
    }

    #[test]
    fn effective_half_life_prefers_temporal_decay() {
        let mut s = MemorySearchConfig::default();
        s.temporal_decay.enabled = true;
        s.temporal_decay.half_life_days = 14.0;
        assert_eq!(s.effective_half_life_days(), Some(14.0));
    }

    #[test]
    fn effective_half_life_converts_legacy_recency_decay() {
        let mut s = MemorySearchConfig::default();
        s.temporal_decay.enabled = false;
        s.recency_decay = 0.5; // non-default → converted
        let hl = s.effective_half_life_days().unwrap();
        assert!(
            (hl - 1.0).abs() < 1e-9,
            "0.5 per-day decay ⇒ ~1 day half-life, got {hl}"
        );
    }

    #[test]
    fn effective_half_life_none_when_disabled_and_default_recency() {
        let mut s = MemorySearchConfig::default();
        s.temporal_decay.enabled = false;
        // recency_decay left at default ⇒ no decay
        assert_eq!(s.effective_half_life_days(), None);
    }
}

// from xai-grok-config-types/permission.rs
//  Permission-policy config value types, extracted from qidi-code
//  (config dependency inversion).

// use serde::{Deserialize, Serialize};

/// Permission policy configuration loaded from `[permission]` section in config.toml.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PermissionConfig {
    pub rules: Vec<PermissionRule>,
}

/// A single permission rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub action: RuleAction,
    #[serde(default)]
    pub tool: ToolFilter,
    pub pattern: Option<String>,
    #[serde(default)]
    pub pattern_mode: PatternMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PatternMode {
    #[default]
    Glob,
    /// Match against URL host rather than full string (from `WebFetch(domain:...)`).
    Domain,
}

/// Action to take when rule matches.
///
/// CWE-1188: Default changed from Allow to Deny so that omitting the
/// `action` field in a TOML permission rule does not silently create a
/// catch-all allow rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    #[default]
    Deny,
    Ask,
}

/// Tool filter for permission rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToolFilter {
    #[default]
    Any,
    Bash,
    Edit,
    Read,
    Grep,
    Mcp,
    WebFetch,
}

// from xai-grok-config-types/pool.rs
//  Worktree-pool configuration value type, extracted from qidi-code
//  (config dependency inversion).

// use serde::{Deserialize, Serialize};

/// Configuration for the pre-created worktree pool.
///
/// The pool pre-creates linked worktrees in the background so that fork
/// flows can acquire a ready-made worktree instead of creating one from
/// scratch. Set in `config.toml` under `[worktree_pool]`.
///
/// Example:
/// ```toml
/// [worktree_pool]
/// enabled = true
/// pool_size = 2
/// file_count_threshold = 50000
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Whether the pool is enabled at all.
    /// Can be set to false to disable pooling regardless of repo size.
    /// Default: true (auto-detect based on file_count_threshold)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Number of worktrees to keep ready in the pool.
    /// 2 is the minimum useful value when forks need parallel worktrees.
    /// Default: 2
    #[serde(default = "default_pool_size")]
    pub pool_size: usize,

    /// Minimum number of tracked files for the pool to activate.
    /// Below this threshold, on-demand creation is fast enough.
    /// Default: 50_000
    #[serde(default = "default_file_count_threshold")]
    pub file_count_threshold: usize,

    /// Number of threads to use for worktree creation when populating the pool.
    /// This can speed up pool population on large repos, but also increases resource usage.
    /// Default: 3.
    #[serde(default = "default_pool_parallelism")]
    pub parallelism: usize,
}


fn default_pool_parallelism() -> usize {
    3
}

fn default_pool_size() -> usize {
    2
}

fn default_file_count_threshold() -> usize {
    50_000
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            pool_size: default_pool_size(),
            file_count_threshold: default_file_count_threshold(),
            parallelism: default_pool_parallelism(),
        }
    }
}

#[cfg(test)]
mod pool_config_tests {
    use super::*;

    #[test]
    fn default_matches_serde_helpers() {
        let c = PoolConfig::default();
        assert!(c.enabled);
        assert_eq!(c.pool_size, 2);
        assert_eq!(c.file_count_threshold, 50_000);
        assert_eq!(c.parallelism, 3);
    }

    #[test]
    fn empty_table_applies_all_field_defaults() {
        let c: PoolConfig = serde_json::from_str("{}").unwrap();
        assert!(c.enabled);
        assert_eq!(c.pool_size, 2);
        assert_eq!(c.file_count_threshold, 50_000);
        assert_eq!(c.parallelism, 3);
    }

    #[test]
    fn partial_override_keeps_other_field_defaults() {
        let c: PoolConfig = serde_json::from_str(r#"{"enabled": false, "pool_size": 5}"#).unwrap();
        assert!(!c.enabled);
        assert_eq!(c.pool_size, 5);
        assert_eq!(c.file_count_threshold, 50_000);
        assert_eq!(c.parallelism, 3);
    }
}
