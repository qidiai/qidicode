//! `qidi/*` extension-method handlers for the mobile remote client.
//!
//! These ride the ACP extension-method channel (`_qidi/...` on the wire, see
//! `agent-client-protocol`'s `_`-prefix routing in `AgentSide::decode_request`).
//! The mobile client is a thin browser client, so these methods are deliberately
//! narrow: they return plain JSON with a small, stable schema.

use agent_client_protocol as acp;
use serde::{Deserialize, Serialize};

use super::super::mvp_agent::MvpAgent;
use crate::session::merge::{MergedSession, fetch_merged};
use crate::session::ExtMethodResult;

/// Default session-list page size when the client omits `limit`.
pub const DEFAULT_LIST_LIMIT: usize = 20;
/// Hard cap so a misbehaving client cannot force an unbounded payload.
pub const MAX_LIST_LIMIT: usize = 500;

/// Params for `qidi/session/list`.
///
/// ```json
/// { "cwd": "G:/repo", "query": "keyword", "limit": 20 }
/// ```
/// Every field is optional; omitted fields fall back to defaults (`cwd = None`,
/// `query = None`, `limit = 20`).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QidiSessionListRequest {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Response for `qidi/session/list`.
///
/// ```json
/// { "sessions": [ { "sessionId": "...", "summary": "...", ... } ], "truncated": false }
/// ```
/// `sessions` entries are [`MergedSession`] (camelCase), already deduplicated
/// and sorted most-recent-first by [`fetch_merged`]. `truncated` is `true` when
/// the result was capped at `limit`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QidiSessionListResponse {
    pub sessions: Vec<MergedSession>,
    pub truncated: bool,
}

/// Cap a list at `limit`, reporting whether anything was dropped.
///
/// Pure and unit-testable; the handler applies it as a defensive final cap on
/// top of `fetch_merged`'s own internal limit.
pub fn truncate_sessions(mut sessions: Vec<MergedSession>, limit: usize) -> (Vec<MergedSession>, bool) {
    let truncated = sessions.len() > limit;
    sessions.truncate(limit);
    (sessions, truncated)
}

/// Build the list response from an already-fetched session list + a limit.
///
/// Pure so the schema/truncation behaviour is testable without touching disk
/// or the network.
pub fn build_list_response(
    sessions: Vec<MergedSession>,
    limit: usize,
) -> QidiSessionListResponse {
    let (sessions, truncated) = truncate_sessions(sessions, limit);
    QidiSessionListResponse {
        sessions,
        truncated,
    }
}

/// Router for `qidi/*` extension methods.
pub async fn handle(
    agent: &MvpAgent,
    args: &acp::ExtRequest,
) -> Result<acp::ExtResponse, acp::Error> {
    match args.method.as_ref() {
        "qidi/session/list" => handle_session_list(agent, args).await,
        _ => Err(acp::Error::method_not_found()),
    }
}

async fn handle_session_list(
    agent: &MvpAgent,
    args: &acp::ExtRequest,
) -> Result<acp::ExtResponse, acp::Error> {
    let req: QidiSessionListRequest = serde_json::from_str(args.params.get())
        .map_err(|e| acp::Error::invalid_params().data(format!("invalid params: {e}")))?;

    // Clamp into (0, MAX_LIST_LIMIT]; a `0` limit is interpreted as "default"
    // so an empty/omitted field never produces a useless empty page.
    let limit = req
        .limit
        .filter(|l| *l > 0)
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .min(MAX_LIST_LIMIT);

    // Reuse the desktop `grok sessions` enumeration path (local + remote merge).
    let registry_client = agent.session_registry_client();
    let sessions = fetch_merged(
        registry_client.as_ref(),
        req.cwd.as_deref(),
        req.query.as_deref(),
        limit,
    )
    .await;

    let response = build_list_response(sessions, limit);

    ExtMethodResult::success(response)
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

// ── `qidi/usage` push schema ────────────────────────────────────────────────

/// Params for the `qidi/usage` extension **notification** (server → client).
///
/// ```json
/// { "sessionId": "abc", "inputTokens": 1234, "outputTokens": 567, "cost": 0.012 }
/// ```
///
/// The ACP `SessionUpdate::UsageUpdate` variant exists (`unstable_session_usage`,
/// enabled in this workspace) but no producer emits it today, so this push is
/// the defined channel for the mobile client to receive token/cost telemetry.
// `allow(dead_code)`: this is the *defined* push schema for the mobile client's
// token/cost telemetry. The producer (a sampling-pipeline hook) is a follow-up;
// until it lands the struct is referenced only by tests, which `cargo check`
// (lib target) does not compile.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QidiUsageNotification {
    pub session_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}

#[allow(dead_code)]
impl QidiUsageNotification {
    /// Build the params payload for an ext notification (`_qidi/usage` on the
    /// wire). Pure and unit-testable.
    pub fn new(
        session_id: impl Into<String>,
        input_tokens: u64,
        output_tokens: u64,
        cost: Option<f64>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            input_tokens,
            output_tokens,
            cost,
        }
    }

    /// Serialize the notification params (used as `ExtNotification.params`).
    pub fn to_params_json(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(session_id: &str) -> MergedSession {
        MergedSession {
            session_id: session_id.into(),
            summary: "summary".into(),
            ..MergedSession::default()
        }
    }

    #[test]
    fn empty_list_yields_empty_sessions_not_truncated() {
        let resp = build_list_response(vec![], DEFAULT_LIST_LIMIT);
        assert!(resp.sessions.is_empty());
        assert!(!resp.truncated);
        // Serializes to a JSON object with an empty array.
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["sessions"], serde_json::json!([]));
        assert_eq!(json["truncated"], serde_json::json!(false));
    }

    #[test]
    fn missing_fields_default() {
        let req: QidiSessionListRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(req.cwd, None);
        assert_eq!(req.query, None);
        assert_eq!(req.limit, None);
    }

    #[test]
    fn list_truncation_reports_and_caps() {
        let sessions: Vec<MergedSession> = (0..10).map(|i| sample(&format!("s{i}"))).collect();
        let resp = build_list_response(sessions, 3);
        assert_eq!(resp.sessions.len(), 3);
        assert!(resp.truncated);
        assert_eq!(resp.sessions[0].session_id, "s0");
        assert_eq!(resp.sessions[2].session_id, "s2");
    }

    #[test]
    fn list_below_limit_not_truncated() {
        let sessions: Vec<MergedSession> = (0..3).map(|i| sample(&format!("s{i}"))).collect();
        let resp = build_list_response(sessions, 10);
        assert_eq!(resp.sessions.len(), 3);
        assert!(!resp.truncated);
    }

    #[test]
    fn usage_notification_round_trips() {
        let notif = QidiUsageNotification::new("sess-1", 1200, 300, Some(0.042));
        let json = notif.to_params_json().unwrap();
        assert_eq!(json["sessionId"], serde_json::json!("sess-1"));
        assert_eq!(json["inputTokens"], serde_json::json!(1200));
        assert_eq!(json["outputTokens"], serde_json::json!(300));
        assert_eq!(json["cost"], serde_json::json!(0.042));

        let parsed: QidiUsageNotification = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, notif);
    }

    #[test]
    fn usage_notification_omits_cost_when_absent() {
        let notif = QidiUsageNotification::new("sess-2", 10, 20, None);
        let json = notif.to_params_json().unwrap();
        assert!(json.get("cost").is_none());
    }
}
