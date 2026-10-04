//! P0-2 A-closure regression tests: the `/context` tool-definition
//! accounting (`build_session_info`) and the per-turn tool list
//! (`turn_base_tool_specs`) must apply the *same* `web_search`
//! drop (single fn `drop_web_search_for_backend_search`), so the
//! tool-definition count/token口径 the client sees in `/context`
//! matches the toolset a turn actually sends.
use super::support::*;
use super::*;

/// Backend-hosted search active (agent build toggle AND per-model
/// support flag): the local `web_search` tool is replaced by the
/// server-side `HostedTool::WebSearch`, so it must be absent from
/// the turn tool list — and `/context` must account for exactly
/// the toolset the turn sends.
#[tokio::test(flavor = "current_thread")]
async fn context_tool_definitions_match_turn_specs_under_backend_search() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<cf_acp_lib::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            actor.agent.replace(test_agent_backend_search().await);
            actor.supports_backend_search.set(true);

            let defs = actor.prepare_tool_definitions_inner().await;
            assert!(
                defs.iter().any(|td| td.function.name == "web_search"),
                "fixture must expose the local web_search tool"
            );

            // What a turn actually sends.
            let specs = actor.turn_base_tool_specs(&defs);
            assert!(
                !specs.iter().any(|s| s.name == "web_search"),
                "backend search replaces the local web_search tool"
            );

            // /context accounting must use the same caliber as the turn.
            let info = actor.build_session_info().await;
            assert_eq!(
                info.context.tool_definitions_count as usize,
                specs.len(),
                "/context tool_definitions_count vs actual turn specs len (backend search ON): {} vs {}",
                info.context.tool_definitions_count,
                specs.len()
            );
            let expected_tokens =
                cf_chat_state::estimate_tool_definitions_tokens(&drop_web_search_for_backend_search(
                    defs,
                    true,
                ));
            assert_eq!(
                info.context.tool_definitions_tokens,
                expected_tokens,
                "/context tool_definitions_tokens vs actual turn specs tokens (backend search ON): {} vs {}",
                info.context.tool_definitions_tokens,
                expected_tokens
            );
        })
        .await;
}

/// Same backend-search agent, but the per-model support flag is
/// off: the conjunction gate keeps the local `web_search` tool in
/// both the turn tool list and the `/context` accounting.
#[tokio::test(flavor = "current_thread")]
async fn context_tool_definitions_keep_web_search_without_backend_search() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<cf_acp_lib::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            actor.agent.replace(test_agent_backend_search().await);
            // supports_backend_search left at its default (false).

            let defs = actor.prepare_tool_definitions_inner().await;
            let specs = actor.turn_base_tool_specs(&defs);
            assert!(
                specs.iter().any(|s| s.name == "web_search"),
                "without backend search the local web_search tool ships"
            );

            let info = actor.build_session_info().await;
            assert_eq!(
                info.context.tool_definitions_count as usize,
                specs.len(),
                "/context tool_definitions_count vs actual turn specs len (backend search OFF): {} vs {}",
                info.context.tool_definitions_count,
                specs.len()
            );
            let expected_tokens = cf_chat_state::estimate_tool_definitions_tokens(&defs);
            assert_eq!(
                info.context.tool_definitions_tokens,
                expected_tokens,
                "/context tool_definitions_tokens vs actual turn specs tokens (backend search OFF): {} vs {}",
                info.context.tool_definitions_tokens,
                expected_tokens
            );
        })
        .await;
}
