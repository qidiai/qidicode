//! WebSocket server for remote agent connections.
//!
//! This module provides a WebSocket server that allows remote TUI clients to
//! connect to a grok agent running on a different machine.
//!
//! The agent persists across WebSocket reconnections: a single MvpAgent instance
//! is created on first connection and reused for all subsequent connections. This
//! ensures that session actors (and any in-flight prompts) survive client
//! disconnects — when a client reconnects and loads an existing session, ongoing
//! work continues to stream to the new connection.

use std::cell::RefCell;
use std::net::SocketAddr;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;

use axum::{
    Json, Router,
    extract::{
        ConnectInfo, Query, State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, simplex};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::time::Duration;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{info, warn};

use agent_client_protocol as acp;
use cf_acp_lib::{
    AcpAgentGatewayReceiver as GatewayReceiver, AcpAgentGatewaySender as GatewaySender,
    AcpClientMessage, LineBufferedRead,
};

use crate::agent::server_rate_limit::AuthRateLimiter;

use crate::agent::config::{Config as AgentConfig, ModelEntry};
use crate::agent::models::{ModelFetchAuth, prefetch_models_blocking};
use crate::agent::mvp_agent::MvpAgent;

use indexmap::IndexMap;

/// Swappable destination for the relay task.
///
/// Points at the current ACP connection's gateway sender. When no client is
/// connected, the value is `None` and outbound messages are silently dropped
/// (matching the old behaviour where the gateway channel's receiver was simply
/// gone).
type RelayDest = Rc<RefCell<Option<mpsc::UnboundedSender<AcpClientMessage>>>>;

const MAX_BUFFER_SIZE: usize = 8 * 1024 * 1024;
const KEEPALIVE_INTERVAL_SECS: u64 = 15;
/// Half-open detection: if no frame (data or pong) is read from the client in
/// this window, the connection is treated as dead and closed. Derived from
/// [`KEEPALIVE_INTERVAL_SECS`] (a healthy client pongs every keepalive, so a
/// 3× margin tolerates one lost ping/pong without killing a live client).
const READ_TIMEOUT_SECS: u64 = KEEPALIVE_INTERVAL_SECS * 3;
/// How often the agent thread refreshes the shared active-session gauge that
/// `/healthz` reports.
const ACTIVE_SESSIONS_REFRESH_SECS: u64 = 1;

/// WS close code sent to a second, later-arriving client when the single-client
/// guard rejects it (application-defined 4000-4999 range).
const REJECT_SECOND_CLIENT_CODE: u16 = 4001;
const REJECT_SECOND_CLIENT_REASON: &str = "another client is already connected";

/// Guards the "one client at a time" invariant. The simplest semantics: the
/// first connection owns the slot; a later-arriving connection is rejected with
/// a clear close reason. Reconnection works because a disconnect releases the
/// slot before the mobile client's next backoff attempt lands.
#[derive(Debug, Default)]
struct SingleClientGuard(AtomicBool);

impl SingleClientGuard {
    /// Try to claim the single client slot. Returns `true` on success.
    fn try_acquire(&self) -> bool {
        self.0
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    fn release(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// JSON body for `GET /healthz`.
#[derive(Debug, serde::Serialize)]
struct HealthzResponse {
    status: &'static str,
    active_sessions: usize,
    uptime_seconds: u64,
}

/// Liveness probe: reports the number of live agent sessions plus server uptime.
/// The session gauge is refreshed periodically by the agent thread, so it is
/// eventually-consistent (bounded by [`ACTIVE_SESSIONS_REFRESH_SECS`]).
async fn healthz(State(state): State<Arc<ServerState>>) -> Json<HealthzResponse> {
    Json(HealthzResponse {
        status: "ok",
        active_sessions: state.active_sessions.load(Ordering::Relaxed),
        uptime_seconds: state.started.elapsed().as_secs(),
    })
}

/// Configuration for the agent WebSocket server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to bind the server to
    pub bind_addr: SocketAddr,
    /// Secret token for client authentication (required)
    pub secret: String,
}

/// Shared state for the WebSocket server.
struct ServerState {
    agent_config: AgentConfig,
    secret: String,
    /// Channel to send new WebSocket connections to the persistent agent thread.
    /// Lazily initialised on first connection; protected by a tokio Mutex so the
    /// axum handler (which is `Send`) can acquire it.
    agent_conn_tx: tokio::sync::Mutex<Option<mpsc::UnboundedSender<NewConnectionChannels>>>,
    /// Connection rate limit + failed-auth lockout (single source of params).
    rate_limiter: Arc<AuthRateLimiter>,
    /// Number of live agent sessions, refreshed periodically by the agent thread.
    active_sessions: Arc<AtomicUsize>,
    /// Server start instant for `/healthz` uptime.
    started: Instant,
    /// One-client-at-a-time guard.
    client_guard: SingleClientGuard,
}

/// Channels bridging a single WebSocket connection to the agent thread.
struct NewConnectionChannels {
    from_ws_rx: mpsc::UnboundedReceiver<String>,
    to_ws_tx: mpsc::UnboundedSender<String>,
}

/// Query parameters for WebSocket connection.
#[derive(Debug, serde::Deserialize, Default)]
pub struct WsQueryParams {
    #[serde(rename = "server-key")]
    pub server_key: Option<String>,
}

/// Validate the bearer token from request headers or query parameters.
fn validate_auth(headers: &HeaderMap, query: &WsQueryParams, expected_secret: &str) -> bool {
    // Try Authorization header
    if let Some(token) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        return token == expected_secret;
    }

    // Fall back to query parameter for browser connections
    if let Some(ref key) = query.server_key {
        return key == expected_secret;
    }

    false
}

/// WebSocket upgrade handler with authentication.
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ServerState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<WsQueryParams>,
) -> Response {
    // Rate-limit every connection attempt; lockout is checked on the next
    // attempt after the threshold of failed auths has been crossed.
    state.rate_limiter.record_attempt();
    if state.rate_limiter.is_locked_out() {
        warn!("Rejecting locked-out peer {}", addr);
        return (StatusCode::TOO_MANY_REQUESTS, "Too many failed auth attempts")
            .into_response();
    }
    if state.rate_limiter.connection_throttled() {
        warn!("Rejecting connection-rate-limited peer {}", addr);
        return (StatusCode::TOO_MANY_REQUESTS, "Connection rate limit exceeded")
            .into_response();
    }

    // Validate secret token from header or query param
    if !validate_auth(&headers, &query, &state.secret) {
        warn!("Unauthorized connection attempt from {}", addr);
        state.rate_limiter.record_failure();
        return (
            StatusCode::UNAUTHORIZED,
            "Invalid or missing authorization token",
        )
            .into_response();
    }

    info!("Authenticated WebSocket connection from {}", addr);
    ws.on_upgrade(move |socket| handle_connection(socket, state, addr))
}

/// Handle an authenticated WebSocket connection.
///
/// On first connection, spawns a persistent agent thread that owns the MvpAgent.
/// On subsequent connections (reconnects), sends new WS channels to the existing
/// agent thread so that session actors can continue streaming to the new client.
async fn handle_connection(mut ws: WebSocket, state: Arc<ServerState>, peer_addr: SocketAddr) {
    info!("New WebSocket connection from {}", peer_addr);

    // Single-client guard: reject a second, later-arriving client with a clear
    // close reason. A reconnect is unaffected because the previous connection's
    // teardown (below) releases the slot before the next backoff attempt lands.
    if !state.client_guard.try_acquire() {
        warn!("Rejecting second concurrent client from {}", peer_addr);
        let _ = ws
            .send(Message::Close(Some(CloseFrame {
                code: REJECT_SECOND_CLIENT_CODE,
                reason: REJECT_SECOND_CLIENT_REASON.into(),
            })))
            .await;
        return;
    }

    let (mut ws_write, mut ws_read) = ws.split();

    // Channels for bridging WS <-> Agent thread
    let (to_agent_tx, to_agent_rx) = mpsc::unbounded_channel::<String>();
    let (from_agent_tx, mut from_agent_rx) = mpsc::unbounded_channel::<String>();

    // Ensure the persistent agent thread is running (lazy init on first connection).
    // If the previous agent thread died (panic, etc.), clear the stale sender so we
    // respawn a fresh one.
    {
        let mut agent_tx_guard = state.agent_conn_tx.lock().await;

        // Check if existing sender is still alive (receiver not dropped)
        if let Some(ref tx) = *agent_tx_guard
            && tx.is_closed()
        {
            warn!("Persistent agent thread died — will respawn");
            *agent_tx_guard = None;
        }

        if agent_tx_guard.is_none() {
            let (conn_tx, conn_rx) = mpsc::unbounded_channel();

            let agent_config = state.agent_config.clone();
            let active_sessions = state.active_sessions.clone();
            let _agent_thread = thread::Builder::new()
                .name("agent-persistent".to_string())
                .spawn(move || {
                    // Prefetch models before creating the runtime (blocking is OK here)
                    let auth = agent_config.create_auth_manager().current();
                    let fetch_auth =
                        ModelFetchAuth::resolve(&agent_config.endpoints, auth.is_some());
                    let prefetched_models = if auth.is_some()
                        || agent_config.endpoints.has_custom_endpoint()
                        || fetch_auth != ModelFetchAuth::Session
                    {
                        prefetch_models_blocking(&agent_config.endpoints, auth.as_ref(), fetch_auth)
                    } else {
                        None
                    };

                    info!("Prefetched models: {:?}", prefetched_models);

                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to create runtime for agent");

                    let local_set = tokio::task::LocalSet::new();
                    local_set.block_on(&rt, async move {
                        run_persistent_agent(agent_config, conn_rx, prefetched_models, active_sessions)
                            .await
                    });

                    warn!("Persistent agent thread exiting");
                });

            *agent_tx_guard = Some(conn_tx);
            info!("Persistent agent thread spawned");
        }

        // Send new WS channels to the agent thread
        if let Some(ref tx) = *agent_tx_guard
            && tx
                .send(NewConnectionChannels {
                    from_ws_rx: to_agent_rx,
                    to_ws_tx: from_agent_tx,
                })
                .is_err()
        {
            warn!("Failed to send connection channels to agent thread");
        }
    }

    // Task: Read from WS, send to agent thread. A `tokio::time::timeout`
    // detects half-open connections: a healthy client replies to our keepalive
    // pings (resetting the timer via `Pong`), a dead one leaves the read idle
    // until `READ_TIMEOUT_SECS` elapses and we close it.
    let read_task = tokio::spawn(async move {
        loop {
            let next = tokio::time::timeout(
                Duration::from_secs(READ_TIMEOUT_SECS),
                ws_read.next(),
            )
            .await;
            let msg = match next {
                Ok(Some(msg)) => msg,
                Ok(None) => break,
                Err(_) => {
                    warn!("WebSocket read timeout from {} (half-open)", peer_addr);
                    break;
                }
            };
            match msg {
                Ok(Message::Text(text)) => {
                    let text_str: &str = text.as_ref();
                    let trimmed = text_str.trim_end_matches(['\r', '\n']);
                    // Skip browser keepalive pings (non-JSON text)
                    if trimmed == "ping" || trimmed.is_empty() {
                        continue;
                    }
                    if to_agent_tx.send(trimmed.to_string()).is_err() {
                        break;
                    }
                }
                Ok(Message::Binary(bin)) => {
                    if let Ok(s) = std::str::from_utf8(&bin) {
                        let trimmed = s.trim_end_matches(['\r', '\n']);
                        if trimmed == "ping" || trimmed.is_empty() {
                            continue;
                        }
                        if to_agent_tx.send(trimmed.to_string()).is_err() {
                            break;
                        }
                    }
                }
                Ok(Message::Close(frame)) => {
                    if let Some(f) = frame {
                        info!(
                            "WebSocket close from {}: {} {}",
                            peer_addr, f.code, f.reason
                        );
                    }
                    break;
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
                Err(e) => {
                    warn!("WebSocket read error from {}: {:?}", peer_addr, e);
                    break;
                }
            }
        }
    });

    // Task: Read from agent thread, send to WS (with keepalive)
    let write_task = tokio::spawn(async move {
        let mut keepalive = tokio::time::interval(Duration::from_secs(KEEPALIVE_INTERVAL_SECS));

        loop {
            tokio::select! {
                Some(msg) = from_agent_rx.recv() => {
                    if ws_write.send(Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
                _ = keepalive.tick() => {
                    if ws_write.send(Message::Ping(vec![].into())).await.is_err() {
                        break;
                    }
                }
                else => break,
            }
        }
    });

    // Wait for either task to complete
    tokio::select! {
        _ = read_task => {}
        _ = write_task => {}
    }

    // Release the single-client slot so the next (re)connection can acquire it.
    // Must be paired with the `try_acquire` at the top; otherwise the first
    // client's disconnect would permanently lock out every subsequent client.
    state.client_guard.release();
    info!("WebSocket connection ended for {}", peer_addr);
}

/// Run the persistent agent on a dedicated thread with LocalSet.
///
/// The MvpAgent is created **once** and reused across WebSocket reconnections.
/// A persistent gateway channel ensures that session actors (which hold cloned
/// `GatewaySender` handles) can always send notifications. A relay task forwards
/// messages from the persistent channel to the *current* ACP connection's channel,
/// so notifications reach whichever client is currently connected.
async fn run_persistent_agent(
    agent_config: AgentConfig,
    mut connection_rx: mpsc::UnboundedReceiver<NewConnectionChannels>,
    prefetched_models: Option<IndexMap<String, ModelEntry>>,
    active_sessions: Arc<AtomicUsize>,
) {
    // Persistent gateway channel — the MvpAgent and all session actors hold
    // clones of `gw_tx`. This channel survives across reconnections.
    let (gw_tx, mut gw_rx) = tokio::sync::mpsc::unbounded_channel::<AcpClientMessage>();
    let gateway = GatewaySender::new(gw_tx);

    // Create MvpAgent ONCE -- it persists for the lifetime of the server.
    let auth_manager = Arc::new(agent_config.create_auth_manager());
    // Proactive token refresh; runs until process exit.
    auth_manager.start_proactive_refresh(tokio_util::sync::CancellationToken::new());
    // Restore managed policy right before bootstrap reads it — the agent is created lazily here,
    // so an earlier restore could go stale before the gate.
    crate::managed_config::ensure_managed_policy_present(&auth_manager).await;
    let agent = Rc::new(
        MvpAgent::new(gateway, &agent_config, auth_manager, prefetched_models)
            .unwrap_or_else(crate::agent::init::exit_on_config_error),
    );

    let relay_dest: RelayDest = Rc::new(RefCell::new(None));

    // Relay task: reads from the persistent gateway channel and forwards to
    // whichever ACP connection is currently active.
    let relay_dest_for_task = relay_dest.clone();
    tokio::task::spawn_local(async move {
        while let Some(msg) = gw_rx.recv().await {
            let maybe_tx = relay_dest_for_task.borrow().clone();
            if let Some(tx) = maybe_tx
                && tx.send(msg).is_err()
            {
                // Connection's gateway receiver was dropped — clear it.
                *relay_dest_for_task.borrow_mut() = None;
            }
            // If no connection, the message (and its response_tx) is dropped.
            // The caller (session actor) gets a send error which is already
            // handled with `let _ = ...`.
        }
    });

    // Refresh the shared active-session gauge that `/healthz` reports. Runs on
    // the LocalSet because `MvpAgent` is `!Send`; `session_count()` purges dead
    // session entries as a side effect, keeping the gauge accurate.
    let refresh_agent = agent.clone();
    let refresh_gauge = active_sessions.clone();
    tokio::task::spawn_local(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(ACTIVE_SESSIONS_REFRESH_SECS));
        loop {
            interval.tick().await;
            let count = refresh_agent.activity.session_count();
            refresh_gauge.store(count, Ordering::Relaxed);
        }
    });

    // Accept new connections in a loop
    while let Some(channels) = connection_rx.recv().await {
        info!("Agent thread: setting up new ACP connection (reconnect)");
        setup_acp_connection(agent.clone(), channels, relay_dest.clone());
    }

    info!("Agent thread: connection channel closed, exiting");
}

/// Set up a new ACP connection for a WebSocket connection, reusing the existing
/// MvpAgent. The relay destination is updated so that session actor notifications
/// flow to the new client.
fn setup_acp_connection(
    agent: Rc<MvpAgent>,
    channels: NewConnectionChannels,
    relay_dest: RelayDest,
) {
    let NewConnectionChannels {
        mut from_ws_rx,
        to_ws_tx,
    } = channels;

    // Create new simplex IO streams for this ACP connection
    let (agent_read_rx, mut agent_read_tx) = simplex(MAX_BUFFER_SIZE);
    let (agent_write_rx, agent_write_tx) = simplex(MAX_BUFFER_SIZE);

    let incoming = agent_read_rx.compat();
    let outgoing = agent_write_tx.compat_write();

    // Create a per-connection gateway channel for the GatewayReceiver.
    // The relay task will forward persistent-channel messages here.
    let (conn_gw_tx, conn_gw_rx) = tokio::sync::mpsc::unbounded_channel::<AcpClientMessage>();
    // Clone kept for the teardown guard below, so it can tell whether the relay
    // still points at *this* connection's channel (vs a newer reconnect's).
    let conn_gw_tx_for_guard = conn_gw_tx.clone();

    // Point the relay at this new connection's channel
    *relay_dest.borrow_mut() = Some(conn_gw_tx);

    // Create new ACP connection reusing the same MvpAgent (via Rc clone).
    // `Agent` is implemented for `Rc<T: Agent>` so this works.
    let incoming = LineBufferedRead::spawn_local(incoming);
    let (conn, handle_io) = acp::AgentSideConnection::new(agent, outgoing, incoming, |fut| {
        tokio::task::spawn_local(fut);
    });
    tokio::task::spawn_local(
        GatewayReceiver::new(conn_gw_rx, conn)
            .with_on_meta(cf_file_utils::trace_context::span_from_meta_traceparent)
            .run(),
    );

    // Task: Forward WS messages → agent (incoming ACP bytes)
    tokio::task::spawn_local(async move {
        while let Some(msg) = from_ws_rx.recv().await {
            // Log messages that lack both `id` and `method` — the ACP layer
            // only prints "received message with neither id nor method" without
            // the payload, making debugging impossible.
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg)
                && v.get("id").is_none()
                && v.get("method").is_none()
            {
                warn!(
                    len = msg.len(),
                    "incoming WS message has neither id nor method"
                );
            }
            if agent_read_tx.write_all(msg.as_bytes()).await.is_err() {
                break;
            }
            if agent_read_tx.write_all(b"\n").await.is_err() {
                break;
            }
        }
        // WS disconnected — the simplex writer is dropped, causing `handle_io`
        // to complete. The GatewayReceiver for this connection will also stop.
        // But the MvpAgent and session actors stay alive, ready for the next
        // connection.
    });

    // Task: Forward agent messages → WS (outgoing ACP bytes)
    tokio::task::spawn_local(async move {
        let mut reader = BufReader::new(agent_write_rx);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    let msg = line.trim_end_matches(['\r', '\n']);
                    if !msg.is_empty() && to_ws_tx.send(msg.to_string()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Run the ACP IO handler — fire-and-forget since we don't block the
    // connection loop. It completes when the WS disconnects. On completion the
    // upstream RPC layer drops every pending response sender (cancelling any
    // in-flight permission/file/terminal reverse request with an error), and we
    // additionally clear the relay destination if it still points at *this*
    // connection's channel — so the dead `GatewayReceiver` exits instead of
    // lingering on the relay-held sender (which would otherwise keep it alive
    // across reconnects).
    let relay_dest_for_teardown = relay_dest.clone();
    tokio::task::spawn_local(async move {
        let _ = handle_io.await;
        info!("ACP connection IO handler completed");
        let is_stale = relay_dest_for_teardown
            .borrow()
            .as_ref()
            .is_some_and(|tx| tx.same_channel(&conn_gw_tx_for_guard));
        if is_stale {
            *relay_dest_for_teardown.borrow_mut() = None;
        }
    });
}

/// Run the agent WebSocket server.
///
/// This starts a WebSocket server that accepts authenticated connections from
/// remote TUI clients. A single agent instance is shared across all connections
/// (persisted across reconnections) so that in-flight session work survives
/// client disconnects.
///
/// # Arguments
/// * `config` - Server configuration (bind address and secret)
/// * `agent_config` - Agent configuration to use for each connection
///
/// # Example
/// ```ignore
/// let server_config = ServerConfig {
///     bind_addr: "0.0.0.0:9000".parse().unwrap(),
///     secret: "my-secret-token".to_string(),
/// };
/// run_agent_server(server_config, agent_config).await?;
/// ```
pub async fn run_agent_server(
    config: ServerConfig,
    agent_config: AgentConfig,
) -> anyhow::Result<()> {
    let state = Arc::new(ServerState {
        agent_config,
        secret: config.secret,
        agent_conn_tx: tokio::sync::Mutex::new(None),
        rate_limiter: Arc::new(AuthRateLimiter::new()),
        active_sessions: Arc::new(AtomicUsize::new(0)),
        started: Instant::now(),
        client_guard: SingleClientGuard::default(),
    });

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/healthz", get(healthz))
        .with_state(state);

    let listener = TcpListener::bind(config.bind_addr).await?;
    info!("Agent server listening on ws://{}/ws", config.bind_addr);
    info!(
        "Clients should connect with: --remote ws://{}:{}/ws --secret <token>",
        config.bind_addr.ip(),
        config.bind_addr.port()
    );

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_client_guard_blocks_second_then_releases() {
        let guard = SingleClientGuard::default();
        // First client acquires the slot.
        assert!(guard.try_acquire());
        // A second, later-arriving client is rejected (T2.7 semantics).
        assert!(!guard.try_acquire());
        // Once the first disconnects, the slot is free for a reconnect.
        guard.release();
        assert!(guard.try_acquire());
    }

    #[test]
    fn read_timeout_derives_from_keepalive_constant() {
        // Half-open detection must reuse KEEPALIVE_INTERVAL_SECS, not invent a
        // new value (T2.5 red line).
        assert_eq!(READ_TIMEOUT_SECS, KEEPALIVE_INTERVAL_SECS * 3);
        assert_eq!(KEEPALIVE_INTERVAL_SECS, 15);
    }

    #[test]
    fn reject_second_client_code_is_application_defined() {
        // WS close codes 4000-4999 are the application-defined range.
        assert!((4000..=4999).contains(&REJECT_SECOND_CLIENT_CODE));
    }
}
