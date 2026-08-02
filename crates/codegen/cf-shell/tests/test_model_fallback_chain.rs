//! Integration tests for model-level failover (`fallback_models` in
//! `[model.<id>]`): TOML parsing with backward-compatible defaults, the
//! failover-eligibility classifier, and the chain planning (dedup / cycle
//! guard / missing-model skip). Lives in an integration-test target because
//! the cf-shell lib test target has pre-existing compile errors unrelated
//! to this feature.
//!
//! ```bash
//! cargo test -p cf-shell --test test_model_fallback_chain
//! ```

use cf_sampler::retry::is_failover_error;
use cf_sampling_types::SamplingError;
use cf_shell::agent::config::{
    base_url_env_var, plan_fallback_chain, resolve_fallback_sampler_configs, resolve_model_list,
    Config, ModelEntry,
};
use indexmap::IndexMap;
use reqwest::StatusCode;
use serial_test::serial;

/// RAII env-var override: restores the previous value on drop. Local copy —
/// `cf_env::EnvVarGuard` is `#[cfg(test)]`-gated and thus invisible to this
/// integration-test crate; cross-test races are prevented via `#[serial]`.
struct EnvVarGuard {
    key: String,
    prev: Option<String>,
}
impl EnvVarGuard {
    fn remove(key: &str) -> Self {
        let prev = std::env::var(key).ok();
        unsafe { std::env::remove_var(key) };
        Self {
            key: key.to_string(),
            prev,
        }
    }
}
impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.prev.take() {
            Some(prev) => unsafe { std::env::set_var(&self.key, prev) },
            None => unsafe { std::env::remove_var(&self.key) },
        }
    }
}

/// Parse a raw config.toml string and run the full model-catalog resolution
/// (built-in defaults → `[model.<id>]` overrides → env base_url overrides).
fn resolve_from_toml(toml_src: &str) -> IndexMap<String, ModelEntry> {
    let raw: toml::Value = toml::from_str(toml_src).expect("test TOML must parse");
    let cfg = Config::new_from_toml_cfg(&raw).expect("config must build from TOML");
    resolve_model_list(&cfg, None)
}

/// The AI Bridge failover scenario: a primary behind a local reverse proxy
/// with one fallback that has its own catalog entry (own base_url).
const BRIDGE_TOML: &str = r#"
[model."agnes-2.0-flash"]
model = "agnes-2.0-flash"
base_url = "http://127.0.0.1:9800/v1"
api_backend = "chat_completions"
fallback_models = ["glm-5.2"]

[model."glm-5.2"]
model = "glm-5.2"
base_url = "http://127.0.0.1:9900/v1"
api_backend = "chat_completions"
"#;

/// (a) `fallback_models` parses from `[model.<id>]` into the catalog entry;
/// entries that don't set it default to an empty chain (old configs keep
/// working unchanged).
#[test]
#[serial]
fn fallback_models_parses_and_defaults_empty() {
    let models = resolve_from_toml(BRIDGE_TOML);
    let primary = models
        .get("agnes-2.0-flash")
        .expect("primary entry must exist");
    assert_eq!(primary.fallback_models, vec!["glm-5.2".to_string()]);
    let fallback = models.get("glm-5.2").expect("fallback entry must exist");
    assert!(
        fallback.fallback_models.is_empty(),
        "unset fallback_models must default to empty"
    );
    // Built-in models never configure a chain either.
    let builtin = models.get("gpt-4o").expect("built-in gpt-4o entry exists");
    assert!(builtin.fallback_models.is_empty());
}

/// (b) Failover eligibility: transport failures and HTTP 5xx are eligible;
/// auth/parameter (4xx) and deterministic errors must never switch models.
#[test]
fn failover_error_classification() {
    let api_err = |status: StatusCode| SamplingError::Api {
        status,
        message: "x".to_string(),
        model_metadata: None,
        retry_after_secs: None,
        should_retry: None,
    };
    // Eligible: the upstream (or the path to it) is broken — another model
    // with its own base_url may still work.
    assert!(is_failover_error(&api_err(StatusCode::BAD_GATEWAY)));
    assert!(is_failover_error(&api_err(StatusCode::INTERNAL_SERVER_ERROR)));
    assert!(is_failover_error(&SamplingError::EventStreamError(
        "connection reset".into()
    )));
    assert!(is_failover_error(&SamplingError::StreamError {
        error_type: "server_error".into(),
        message: "mid-stream failure".into(),
    }));
    assert!(is_failover_error(&SamplingError::IdleTimeout {
        elapsed_secs: 300
    }));
    // Not eligible: the request (credentials/parameters) is wrong — a
    // different model would fail the same way or mask a config bug.
    assert!(!is_failover_error(&api_err(StatusCode::UNAUTHORIZED)));
    assert!(!is_failover_error(&api_err(StatusCode::BAD_REQUEST)));
    assert!(!is_failover_error(&api_err(StatusCode::TOO_MANY_REQUESTS)));
    assert!(!is_failover_error(&SamplingError::Auth("expired".into())));
    assert!(!is_failover_error(&SamplingError::MaxTokensTruncation));
}

/// (c) Chain planning: IDs missing from the catalog are skipped, repeats of
/// the primary or of earlier chain entries are dropped (cycle guard), and
/// the surviving order is preserved (first_available).
#[test]
#[serial]
fn plan_skips_missing_and_duplicate_models() {
    let models = resolve_from_toml(BRIDGE_TOML);
    let chain = [
        "glm-5.2".to_string(),
        "no-such-model".to_string(),
        "glm-5.2".to_string(),          // repeat of an earlier entry
        "agnes-2.0-flash".to_string(),  // cycle back to the primary
    ];
    let plan = plan_fallback_chain("agnes-2.0-flash", &chain, &models);
    assert_eq!(plan.resolved, vec!["glm-5.2".to_string()]);
    assert_eq!(plan.skipped_missing, vec!["no-such-model".to_string()]);
    assert_eq!(
        plan.skipped_duplicate,
        vec!["glm-5.2".to_string(), "agnes-2.0-flash".to_string()]
    );
}

/// (c) The cycle guard compares catalog keys, not raw strings: a fallback ID
/// given as a routing slug (`my-model`) must be recognized as the same entry
/// as its catalog key (`my.model`).
#[test]
#[serial]
fn plan_dedups_by_catalog_key_not_raw_id() {
    let models = resolve_from_toml(
        r#"
[model."my.model"]
model = "my-model"
base_url = "http://127.0.0.1:9800/v1"
api_backend = "chat_completions"
"#,
    );
    // Slug spelling of the primary itself → duplicate, not a new entry.
    let plan = plan_fallback_chain("my.model", &["my-model".to_string()], &models);
    assert!(plan.resolved.is_empty());
    assert_eq!(plan.skipped_duplicate, vec!["my-model".to_string()]);
    // From another primary, the slug resolves to the catalog key.
    let plan = plan_fallback_chain("gpt-4o", &["my-model".to_string()], &models);
    assert_eq!(plan.resolved, vec!["my.model".to_string()]);
}

/// End-to-end shell-side resolution: each fallback becomes a SamplerConfig
/// built from that model's *own* catalog entry (own base_url), and the
/// nested chain is empty (single-level expansion, no recursion).
#[test]
#[serial]
fn resolve_fallback_sampler_configs_uses_own_catalog_entry() {
    let _e1 = EnvVarGuard::remove(&base_url_env_var("agnes-2.0-flash"));
    let _e2 = EnvVarGuard::remove(&base_url_env_var("glm-5.2"));
    let models = resolve_from_toml(BRIDGE_TOML);
    let configs = resolve_fallback_sampler_configs("agnes-2.0-flash", &models, None, None, None);
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].model, "glm-5.2");
    assert_eq!(
        configs[0].base_url, "http://127.0.0.1:9900/v1",
        "fallback must use its own catalog base_url, not the primary's"
    );
    assert!(
        configs[0].fallback_configs.is_empty(),
        "single-level expansion: a fallback's own chain is never followed"
    );
    // A model with no chain resolves to no fallback configs.
    assert!(resolve_fallback_sampler_configs("glm-5.2", &models, None, None, None).is_empty());
}
