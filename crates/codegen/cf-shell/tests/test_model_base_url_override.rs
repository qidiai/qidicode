//! Integration tests for the model `base_url` override feature:
//! `[model.<id>]` in config.toml, the `QIDI_LLM_BASE_URL_<MODEL>` env
//! override, and `normalize_model_base_url`. Lives in an integration-test
//! target (not the in-crate `mod tests`) because the cf-shell lib test
//! target has pre-existing compile errors unrelated to this feature.
//!
//! ```bash
//! cargo test -p cf-shell --test test_model_base_url_override
//! ```

use cf_sampler::AuthScheme;
use cf_shell::agent::config::{
    base_url_env_var, env_base_url_collisions, normalize_model_base_url, resolve_credentials,
    resolve_model_list, Config, ModelEntry,
};
use cf_shell::sampling::ApiBackend;
use indexmap::IndexMap;
use serial_test::serial;

/// RAII env-var override: restores the previous value on drop. Local copy —
/// `cf_env::EnvVarGuard` is `#[cfg(test)]`-gated and thus invisible to this
/// integration-test crate; cross-test races are prevented via `#[serial]`.
struct EnvVarGuard {
    key: String,
    prev: Option<String>,
}
impl EnvVarGuard {
    fn set(key: &str, value: &str) -> Self {
        let prev = std::env::var(key).ok();
        unsafe { std::env::set_var(key, value) };
        Self {
            key: key.to_string(),
            prev,
        }
    }
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

/// (a) A `[model.<id>]` block in config.toml overrides the built-in model's
/// base_url, with trailing-slash normalization applied at the config seam.
#[test]
#[serial]
fn config_toml_overrides_builtin_model_base_url() {
    let _env = EnvVarGuard::remove(&base_url_env_var("gpt-4o"));
    let models = resolve_from_toml(
        r#"
[model."gpt-4o"]
base_url = "http://127.0.0.1:9800/v1/"
"#,
    );
    let entry = models.get("gpt-4o").expect("built-in gpt-4o entry exists");
    assert_eq!(entry.info.base_url, "http://127.0.0.1:9800/v1");
    assert_eq!(
        entry.api_base_url, None,
        "a base_url override must route every auth path to it"
    );
}

/// (b) `QIDI_LLM_BASE_URL_<MODEL>` beats the config.toml value
/// (ENV > config.toml > built-in default) and is normalized (bare origin
/// gets `/v1` appended for chat_completions).
#[test]
#[serial]
fn env_overrides_config_toml_base_url() {
    let _env = EnvVarGuard::set(&base_url_env_var("gpt-4o"), "http://127.0.0.1:9800");
    let models = resolve_from_toml(
        r#"
[model."gpt-4o"]
base_url = "http://config.example.com/v1"
"#,
    );
    let entry = models.get("gpt-4o").expect("built-in gpt-4o entry exists");
    assert_eq!(
        entry.info.base_url, "http://127.0.0.1:9800/v1",
        "env override must win over config.toml and be normalized"
    );
    assert_eq!(entry.api_base_url, None);
}

/// (c) A brand-new `[model.<id>]` entry (AI Bridge gateway model) is parsed
/// with api_backend/auth_scheme/env_key, and resolving credentials without
/// the token env var set yields api_key=None without panicking.
#[test]
#[serial]
fn custom_model_entry_resolves_without_token() {
    let _e1 = EnvVarGuard::remove("AI_BRIDGE_TOKEN");
    let _e2 = EnvVarGuard::remove("XAI_API_KEY");
    let _e3 = EnvVarGuard::remove("QIDI_API_KEY");
    let _e4 = EnvVarGuard::remove(&base_url_env_var("agnes-2.0-flash"));
    let models = resolve_from_toml(
        r#"
[model."agnes-2.0-flash"]
model = "agnes-2.0-flash"
base_url = "http://127.0.0.1:9800/v1"
api_backend = "chat_completions"
auth_scheme = "bearer"
env_key = "AI_BRIDGE_TOKEN"
context_window = 128000
"#,
    );
    let entry = models
        .get("agnes-2.0-flash")
        .expect("new model id becomes a catalog entry");
    assert_eq!(entry.info.model, "agnes-2.0-flash");
    assert_eq!(entry.info.base_url, "http://127.0.0.1:9800/v1");
    assert_eq!(entry.info.api_backend, ApiBackend::ChatCompletions);
    assert_eq!(entry.info.auth_scheme, AuthScheme::Bearer);
    assert_eq!(
        entry.env_key.as_ref().and_then(|k| k.primary()),
        Some("AI_BRIDGE_TOKEN")
    );
    // No token anywhere: must not panic, must fall through to api_key=None
    // (the sampler then skips the Authorization header entirely).
    let creds = resolve_credentials(entry, None);
    assert_eq!(creds.api_key, None);
    assert_eq!(creds.base_url, "http://127.0.0.1:9800/v1");
    assert_eq!(creds.auth_scheme, AuthScheme::Bearer);
}

/// (d) base_url normalization: trailing slashes, missing/present `/v1`, a
/// pasted full endpoint, custom-path URLs, and non-chat_completions backends.
#[test]
fn normalize_model_base_url_handles_v1_and_slashes() {
    let cc = ApiBackend::ChatCompletions;
    for raw in [
        "http://127.0.0.1:9800",
        "http://127.0.0.1:9800/",
        "http://127.0.0.1:9800/v1",
        "http://127.0.0.1:9800/v1/",
        "  http://127.0.0.1:9800/v1  ",
        "http://127.0.0.1:9800/v1/chat/completions",
    ] {
        assert_eq!(
            normalize_model_base_url(raw, &cc),
            "http://127.0.0.1:9800/v1",
            "raw={raw:?}"
        );
    }
    // A URL that already carries a custom path stays verbatim (Azure-style).
    assert_eq!(
        normalize_model_base_url("https://res.azure.com/openai/deployments/gpt", &cc),
        "https://res.azure.com/openai/deployments/gpt"
    );
    // Non-chat_completions backends only get whitespace/slash trimming.
    assert_eq!(
        normalize_model_base_url("https://api.anthropic.com/", &ApiBackend::Messages),
        "https://api.anthropic.com"
    );
}

/// Env-var naming: model id is uppercased with every non-alphanumeric
/// character mapped to `_`.
#[test]
fn base_url_env_var_uppercases_and_underscores() {
    assert_eq!(
        base_url_env_var("agnes-2.0-flash"),
        "QIDI_LLM_BASE_URL_AGNES_2_0_FLASH"
    );
    assert_eq!(base_url_env_var("gpt-4o"), "QIDI_LLM_BASE_URL_GPT_4O");
}

/// (e) `base_url_env_var` is non-injective: two distinct model ids mapping to
/// the same env-var name must be reported as one collision group; ids with a
/// unique env-var name must not appear.
#[test]
#[serial]
fn env_var_collision_detected_for_distinct_model_ids() {
    let models = resolve_from_toml(
        r#"
[model."agnes-2.0-flash"]
model = "agnes-2.0-flash"
base_url = "http://127.0.0.1:9800/v1"
api_backend = "chat_completions"

[model."agnes-2-0-flash"]
model = "agnes-2-0-flash"
base_url = "http://127.0.0.1:9801/v1"
api_backend = "chat_completions"
"#,
    );
    let collisions = env_base_url_collisions(&models);
    let group = collisions
        .iter()
        .find(|(var, _)| var == "QIDI_LLM_BASE_URL_AGNES_2_0_FLASH")
        .expect("the two agnes spellings must be reported as a collision");
    assert_eq!(
        group.1,
        vec!["agnes-2.0-flash".to_string(), "agnes-2-0-flash".to_string()],
        "collision group must list both catalog keys in catalog order"
    );
    // Built-in ids have unique env-var names — none of them may be flagged.
    assert!(
        !collisions
            .iter()
            .any(|(var, _)| var == &base_url_env_var("gpt-4o")),
        "gpt-4o has a unique env-var name and must not be reported"
    );
}

/// (f) A catalog key whose routing slug maps to the same env-var name is NOT
/// a collision (same model, one override target).
#[test]
#[serial]
fn env_var_no_collision_for_key_and_own_slug() {
    let models = resolve_from_toml(
        r#"
[model."my.model"]
model = "my-model"
base_url = "http://127.0.0.1:9800/v1"
api_backend = "chat_completions"
"#,
    );
    // `my.model` (key) and `my-model` (slug) both map to
    // QIDI_LLM_BASE_URL_MY_MODEL, but belong to one catalog entry.
    assert!(
        !env_base_url_collisions(&models)
            .iter()
            .any(|(var, _)| var == "QIDI_LLM_BASE_URL_MY_MODEL"),
        "a key and its own routing slug must not count as a collision"
    );
}
