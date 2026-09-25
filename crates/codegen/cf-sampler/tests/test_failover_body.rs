//! End-to-end propagation tests for model failover: the `model` field in
//! the *actual serialized HTTP body* must switch to the fallback model
//! after a failover, across the full chain
//! `run_request_task` -> `try_model_failover` -> `conversation_stream`
//! -> `ChatCompletionRequest` -> wire body.
//!
//! Regression coverage for the P1 where the fallback client was rebuilt
//! correctly but the stale per-request `model` override kept routing the
//! body to the dead primary model.
//!
//! Deliberately avoids `cf_test_support` so this target builds standalone.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::routing::post;
use futures_util::stream;
use indexmap::IndexMap;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

use cf_sampler::{ApiBackend, RequestId, RetryPolicy, SamplerActor, SamplerConfig};
use cf_sampling_types::{ContentPart, ConversationItem, ConversationRequest, UserItem};

// ---------------------------------------------------------------------------
// Mock server harness (same shape as test_actor.rs, minus cf_test_support)
// ---------------------------------------------------------------------------

struct MockServer {
    addr: SocketAddr,
    shutdown_tx: oneshot::Sender<()>,
}

impl MockServer {
    async fn spawn(app: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        Self { addr, shutdown_tx }
    }

    fn base_url(&self) -> String {
        format!("http://{}/v1", self.addr)
    }

    fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
    }
}

/// Shared log of the `model` field seen in each request body.
type SeenModels = Arc<Mutex<Vec<String>>>;

fn record_model(seen: &SeenModels, body: &serde_json::Value) {
    seen.lock()
        .unwrap()
        .push(body["model"].as_str().unwrap_or("").to_string());
}

/// Router that answers every chat/completions POST with 502 and records
/// the model named in the body (what a dead AI Bridge upstream does).
fn dead_upstream_router(seen: SeenModels) -> Router {
    Router::new().route(
        "/v1/chat/completions",
        post(move |axum::Json(body): axum::Json<serde_json::Value>| {
            let seen = Arc::clone(&seen);
            async move {
                record_model(&seen, &body);
                (StatusCode::BAD_GATEWAY, "upstream dead")
            }
        }),
    )
}

fn text_chunk(content: &str, finish: bool) -> Event {
    let chunk = json!({
        "id": "chatcmpl-test",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": "glm-test",
        "choices": [{
            "index": 0,
            "delta": { "role": "assistant", "content": content },
            "finish_reason": if finish { json!("stop") } else { json!(null) }
        }]
    });
    Event::default().data(chunk.to_string())
}

/// Router that streams a successful SSE completion and records the model
/// named in the body.
fn healthy_upstream_router(seen: SeenModels) -> Router {
    Router::new().route(
        "/v1/chat/completions",
        post(move |axum::Json(body): axum::Json<serde_json::Value>| {
            let seen = Arc::clone(&seen);
            async move {
                record_model(&seen, &body);
                let events = vec![text_chunk("fallback ok", false), text_chunk("", true)];
                Sse::new(stream::iter(
                    events.into_iter().map(Ok::<_, std::convert::Infallible>),
                ))
            }
        }),
    )
}

fn test_config(base_url: String, model: &str, max_retries: Option<u32>) -> SamplerConfig {
    SamplerConfig {
        api_key: Some("test-key".into()),
        base_url,
        model: model.into(),
        max_completion_tokens: Some(1024),
        temperature: None,
        top_p: None,
        api_backend: ApiBackend::ChatCompletions,
        auth_scheme: Default::default(),
        extra_headers: IndexMap::new(),
        context_window: 128_000,
        force_http1: false,
        max_retries,
        stream_tool_calls: false,
        idle_timeout_secs: Some(30),
        reasoning_effort: None,
        origin_client: None,
        client_identifier: None,
        deployment_id: None,
        user_id: None,
        client_version: None,
        attribution_callback: None,
        bearer_resolver: None,
        supports_backend_search: false,
        compactions_remaining: None,
        compaction_at_tokens: None,
        doom_loop_recovery: None,
        header_injector: None,
        fallback_configs: Vec::new(),
    }
}

/// Shell-style request carrying an explicit per-request model override
/// naming the primary model -- the exact shape that regressed.
fn user_request_with_model(text: &str, model: &str) -> ConversationRequest {
    ConversationRequest {
        model: Some(model.to_string()),
        items: vec![ConversationItem::User(UserItem {
            content: vec![ContentPart::Text {
                text: std::sync::Arc::<str>::from(text),
            }],
            synthetic_reason: None,
            ..Default::default()
        })],
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// P1 regression: after failover the HTTP body sent to the fallback
/// endpoint must name the fallback model, even though the request carried
/// an explicit override for the (dead) primary model.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failover_switches_model_in_http_body() {
    let primary_seen: SeenModels = Arc::new(Mutex::new(Vec::new()));
    let fallback_seen: SeenModels = Arc::new(Mutex::new(Vec::new()));
    let primary = MockServer::spawn(dead_upstream_router(Arc::clone(&primary_seen))).await;
    let fallback = MockServer::spawn(healthy_upstream_router(Arc::clone(&fallback_seen))).await;

    // max_retries = 1: first failure exhausts the budget -> immediate
    // failover, no backoff sleeps in the test.
    let mut cfg = test_config(primary.base_url(), "agnes-test", Some(1));
    cfg.fallback_configs = vec![test_config(fallback.base_url(), "glm-test", Some(1))];

    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let handle = SamplerActor::spawn(cfg, RetryPolicy::default(), event_tx);

    let (response, _metrics) = handle
        .submit_and_collect(
            RequestId::from("req-failover-body"),
            user_request_with_model("hi", "agnes-test"),
        )
        .await
        .expect("fallback model should complete the request");

    primary.shutdown();
    fallback.shutdown();

    let a = response.assistant().expect("assistant item present");
    assert_eq!(a.content.as_ref(), "fallback ok");

    let primary_models = primary_seen.lock().unwrap().clone();
    let fallback_models = fallback_seen.lock().unwrap().clone();
    assert!(
        !primary_models.is_empty(),
        "primary endpoint must have been attempted"
    );
    assert!(
        primary_models.iter().all(|m| m == "agnes-test"),
        "pre-failover bodies name the primary model, got {primary_models:?}"
    );
    // THE regression assertion: the body that reached the fallback
    // endpoint names the fallback model, not the stale override.
    assert_eq!(
        fallback_models,
        vec!["glm-test".to_string()],
        "post-failover body must name the fallback model"
    );
}

/// P2: with a fallback configured, a failover-eligible error stops
/// retrying after FAILOVER_MAX_RETRIES (2) attempts instead of burning
/// the full budget (15 attempts / minutes of backoff).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failover_cap_switches_after_two_attempts() {
    let primary_seen: SeenModels = Arc::new(Mutex::new(Vec::new()));
    let fallback_seen: SeenModels = Arc::new(Mutex::new(Vec::new()));
    let primary = MockServer::spawn(dead_upstream_router(Arc::clone(&primary_seen))).await;
    let fallback = MockServer::spawn(healthy_upstream_router(Arc::clone(&fallback_seen))).await;

    // Generous per-model budget: without the failover cap this test would
    // sit through ~10 attempts of exponential backoff.
    let mut cfg = test_config(primary.base_url(), "agnes-test", Some(10));
    cfg.fallback_configs = vec![test_config(fallback.base_url(), "glm-test", Some(1))];

    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let handle = SamplerActor::spawn(cfg, RetryPolicy::default(), event_tx);

    let (response, _metrics) = handle
        .submit_and_collect(
            RequestId::from("req-failover-cap"),
            user_request_with_model("hi", "agnes-test"),
        )
        .await
        .expect("fallback model should complete the request");

    primary.shutdown();
    fallback.shutdown();

    let a = response.assistant().expect("assistant item present");
    assert_eq!(a.content.as_ref(), "fallback ok");

    // Exactly FAILOVER_MAX_RETRIES (2) attempts hit the dead primary
    // (initial + one retry), then the switch happened.
    assert_eq!(
        primary_seen.lock().unwrap().len(),
        2,
        "failover cap must bound attempts against the dead primary"
    );
    assert_eq!(
        fallback_seen.lock().unwrap().clone(),
        vec!["glm-test".to_string()]
    );
}
