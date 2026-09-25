//! Tests for credit-limit upsells, paywall gating, and auto-topup.

use super::*;

// ── Credit-limit upsell / max-tier tests ───────────────────────────

/// Open the non-max-tier Q&A upsell modal. Panics if the modal was not created.
fn open_upsell_qa(app: &mut AppView, mode: CreditLimitUpsellMode) {
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    open_credit_limit_upsell(agent, mode, false);
}

/// Open the max-tier inline scrollback card upsell.
fn open_upsell_max_card(app: &mut AppView, mode: CreditLimitUpsellMode) {
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    open_credit_limit_upsell(agent, mode, true);
}

/// Dispatch a `BillingFetched` task result with sensible defaults.
fn dispatch_billing(
    app: &mut AppView,
    balance: Option<crate::views::credit_bar::CreditBalance>,
    silent: bool,
    subscription_tier: Option<String>,
) {
    dispatch(
        Action::TaskComplete(TaskResult::BillingFetched {
            agent_id: AgentId(0),
            balance,
            silent,
            subscription_tier,
            autotopup: crate::views::credit_bar::AutoTopupFetch::Unchanged,
        }),
        app,
    );
}

#[test]
fn credit_limit_retry_preserves_image_submission_state() {
    let mut app = test_app_with_agent();
    let mut image = crate::prompt_images::from_clipboard_data(&crate::clipboard::ImageData {
        data: vec![1, 2, 3],
        mime_type: "image/png".into(),
    });
    image.display_number = 1;
    let prompt = crate::app::agent::InFlightPrompt {
        text: "retry [Image #1]".into(),
        images: vec![image],
        scrollback_entry: crate::scrollback::EntryId::new(0),
        chip_elements: vec![crate::app::agent::ChipElement {
            range: 6..16,
            kind: crate::views::prompt_widget::KIND_IMAGE,
            display: None,
        }],
    };
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .credit_limit_stashed_prompt = Some(prompt);

    let effects = dispatch(
        Action::TaskComplete(TaskResult::CreditLimitRecheckComplete {
            agent_id: AgentId(0),
            meta: Some(serde_json::json!({"subscription_tier": "Upgraded"})),
        }),
        &mut app,
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::SendPromptBlocks { .. }))
    );
    let in_flight = app.agents[&AgentId(0)]
        .session
        .in_flight_prompt
        .as_ref()
        .unwrap();
    assert_eq!(in_flight.images.len(), 1);
    assert_eq!(in_flight.chip_elements.len(), 1);
}

#[test]
fn is_max_tier_positive_match() {
    assert!(is_max_tier(Some("supergrok_heavy")));
    assert!(is_max_tier(Some("SuperGrok Heavy")));
    assert!(is_max_tier(Some("SUPERQIDI_HEAVY")));
}

#[test]
fn is_max_tier_non_max_and_unknown() {
    assert!(!is_max_tier(Some("supergrok")));
    assert!(!is_max_tier(Some("premium")));
    assert!(!is_max_tier(Some("free")));
    // Unknown defaults to non-max → Q&A shown.
    assert!(!is_max_tier(None));
}

#[test]
fn is_max_tier_handles_mixed_case_and_whitespace() {
    assert!(is_max_tier(Some("SuperGrok_Heavy")));
    assert!(is_max_tier(Some("supergrok heavy")));
    assert!(is_max_tier(Some("SUPERGROK HEAVY")));
}

#[test]
fn is_max_tier_rejects_partial_matches() {
    assert!(!is_max_tier(Some("supergrok_heav")));
    assert!(!is_max_tier(Some("supergrok_heavy_plus")));
    assert!(!is_max_tier(Some("")));
}

#[test]
fn upsell_popups_disabled_credit_limit_no_modal_no_card() {
    // QIDI local patch: `open_credit_limit_upsell` returns immediately
    // (self-hosted model endpoints make the upstream paywall irrelevant).
    // Guard the patch: neither the non-max Q&A modal nor the max-tier
    // inline scrollback card may appear, on either billing mode.
    for mode in [
        CreditLimitUpsellMode::UnifiedCredits,
        CreditLimitUpsellMode::LegacyPayg { enabled: false },
        CreditLimitUpsellMode::LegacyPayg { enabled: true },
    ] {
        let mut app = test_app_with_agent();
        let before = agent_scrollback_len(&app);

        open_upsell_qa(&mut app, mode);
        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            agent.question_view.is_none(),
            "Q&A upsell modal must not open for {mode:?}"
        );
        assert_eq!(
            agent_scrollback_len(&app),
            before,
            "Q&A path must not push a scrollback block for {mode:?}"
        );

        open_upsell_max_card(&mut app, mode);
        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            agent.question_view.is_none(),
            "max-tier card path must not open a modal for {mode:?}"
        );
        assert_eq!(
            agent_scrollback_len(&app),
            before,
            "max-tier path must not push a scrollback block for {mode:?}"
        );
    }
}
#[test]
fn credit_limit_upsell_mode_prefers_unified_flag() {
    let mut bal = test_bal(100.0);
    bal.is_unified_billing_user = Some(true);
    bal.pay_as_you_go = true; // must not override explicit unified
    assert_eq!(
        credit_limit_upsell_mode(Some(&bal)),
        CreditLimitUpsellMode::UnifiedCredits
    );
    bal.is_unified_billing_user = Some(false);
    bal.pay_as_you_go = false;
    assert_eq!(
        credit_limit_upsell_mode(Some(&bal)),
        CreditLimitUpsellMode::LegacyPayg { enabled: false }
    );
    bal.is_unified_billing_user = None;
    bal.pay_as_you_go = true;
    assert_eq!(
        credit_limit_upsell_mode(Some(&bal)),
        CreditLimitUpsellMode::LegacyPayg { enabled: true }
    );
    bal.pay_as_you_go = false;
    assert_eq!(
        credit_limit_upsell_mode(Some(&bal)),
        CreditLimitUpsellMode::UnifiedCredits
    );
    assert_eq!(
        credit_limit_upsell_mode(None),
        CreditLimitUpsellMode::UnifiedCredits
    );
}

#[test]
fn is_credit_limit_error_matches_legacy_403_and_pool_402() {
    assert!(is_credit_limit_error(
        Some(403),
        "status 403: run out of credits"
    ));
    // 402 Payment Required is always credit/spend on this surface.
    assert!(is_credit_limit_error(Some(402), "anything"));
    assert!(is_credit_limit_error(
        None,
        "API error (status 402 Payment Required): QIDI Code usage balance exhausted"
    ));
    assert!(is_credit_limit_error(
        None,
        "status 403: run out of credits"
    ));
    assert!(!is_credit_limit_error(Some(403), "content safety blocked"));
    assert!(!is_credit_limit_error(Some(500), "internal server error"));
    // Pool phrases alone without 402/403 status do not match.
    assert!(!is_credit_limit_error(
        None,
        "usage balance exhausted without status"
    ));
}

#[test]
fn show_usage_returns_fetch_billing_effect() {
    let mut app = test_app_with_agent();
    let effects = dispatch(Action::ShowUsage, &mut app);
    // One non-silent FetchBilling — the effect pulls billing + auto-topup
    // together and renders a single summary.
    assert_eq!(effects.len(), 1, "got: {effects:?}");
    assert!(
        matches!(&effects[0], Effect::FetchBilling { agent_id, silent } if *agent_id == AgentId(0) && !*silent),
        "effect should be a non-silent FetchBilling, got: {effects:?}"
    );
}

// ── BillingFetched dispatch tests ───────────────────────────────────

#[test]
fn billing_fetched_updates_app_credit_balance() {
    let mut app = test_app_with_agent();
    dispatch_billing(&mut app, Some(test_bal(42.0)), true, None);
    assert!(app.credit_balance.is_some());
    assert_eq!(app.credit_balance.as_ref().unwrap().usage_pct, 42.0);
}

#[test]
fn billing_fetched_updates_subscription_tier() {
    let mut app = test_app_with_agent();
    dispatch_billing(&mut app, None, true, Some("supergrok_heavy".into()));
    assert_eq!(app.subscription_tier.as_deref(), Some("supergrok_heavy"));
}

#[test]
fn billing_fetched_silent_does_not_push_scrollback() {
    let mut app = test_app_with_agent();
    let before = agent_scrollback_len(&app);
    dispatch_billing(&mut app, Some(test_bal(50.0)), true, None);
    assert_eq!(
        agent_scrollback_len(&app),
        before,
        "silent billing fetch should not push a scrollback message"
    );
}

#[test]
fn billing_fetched_non_silent_pushes_scrollback_message() {
    let mut app = test_app_with_agent();
    let before = agent_scrollback_len(&app);
    let bal = crate::views::credit_bar::CreditBalance {
        pay_as_you_go: true,
        on_demand_cap_cents: Some(1000),
        on_demand_used_cents: Some(350),
        period_end_display: Some("Jul 1, 00:00".into()),
        ..test_bal(75.5)
    };
    dispatch_billing(&mut app, Some(bal), false, None);
    assert_eq!(
        agent_scrollback_len(&app),
        before + 1,
        "non-silent billing fetch should push a scrollback message"
    );
}

#[test]
fn billing_fetched_none_balance_shows_no_data_message() {
    let mut app = test_app_with_agent();
    let before = agent_scrollback_len(&app);
    dispatch_billing(&mut app, None, false, None);
    assert_eq!(agent_scrollback_len(&app), before + 1);
}

#[test]
fn billing_fetched_none_balance_clears_cached() {
    let mut app = test_app_with_agent();
    // Seed a known balance + polling, as a prior successful fetch would.
    dispatch_billing(&mut app, Some(test_bal(80.0)), true, None);
    app.billing_poll_wanted = true;
    // A response carrying no billing config clears the cached balance and
    // polling so the status bar agrees with the "No billing data" message
    // (parse/transport failures route to BillingError, not here).
    dispatch_billing(&mut app, None, false, None);
    assert!(
        app.credit_balance.is_none(),
        "None balance should clear the cached credit balance"
    );
    assert!(
        !app.billing_poll_wanted,
        "None balance should disable billing polling"
    );
}

#[test]
fn billing_fetched_high_usage_enables_poll() {
    let mut app = test_app_with_agent();
    assert!(!app.billing_poll_wanted);
    dispatch_billing(&mut app, Some(test_bal(99.5)), true, None);
    assert!(
        app.billing_poll_wanted,
        "usage >= 99% should enable billing polling"
    );
}

#[test]
fn billing_fetched_low_usage_disables_poll() {
    let mut app = test_app_with_agent();
    app.billing_poll_wanted = true;
    dispatch_billing(&mut app, Some(test_bal(50.0)), true, None);
    assert!(
        !app.billing_poll_wanted,
        "usage < 99% should disable billing polling"
    );
}

#[test]
fn billing_fetched_propagates_balance_to_agent() {
    let mut app = test_app_with_agent();
    let bal = crate::views::credit_bar::CreditBalance {
        effective_usage_pct: 60.0,
        pay_as_you_go: true,
        on_demand_cap_cents: Some(5000),
        on_demand_used_cents: Some(1200),
        period_end_display: Some("Aug 15, 00:00".into()),
        ..test_bal(88.0)
    };
    dispatch_billing(&mut app, Some(bal), true, None);
    let agent_bal = app
        .agents
        .get(&AgentId(0))
        .unwrap()
        .credit_balance
        .as_ref()
        .unwrap();
    assert_eq!(agent_bal.usage_pct, 88.0);
    assert_eq!(agent_bal.effective_usage_pct, 60.0);
    assert!(agent_bal.pay_as_you_go);
    assert_eq!(agent_bal.on_demand_cap_cents, Some(5000));
    assert_eq!(agent_bal.on_demand_used_cents, Some(1200));
}

#[test]
fn billing_fetched_stores_autotopup_on_app_and_agent() {
    let mut app = test_app_with_agent();
    let bal = crate::views::credit_bar::CreditBalance {
        prepaid_balance_cents: Some(1500),
        ..test_bal(100.0)
    };
    let autotopup = crate::views::credit_bar::AutoTopupInfo {
        enabled: true,
        topup_amount_cents: Some(2000),
        max_amount_cents: Some(10000),
    };
    dispatch(
        Action::TaskComplete(TaskResult::BillingFetched {
            agent_id: AgentId(0),
            balance: Some(bal),
            silent: true,
            subscription_tier: None,
            autotopup: crate::views::credit_bar::AutoTopupFetch::Resolved(autotopup),
        }),
        &mut app,
    );
    assert!(app.auto_topup.as_ref().is_some_and(|at| at.enabled));
    let agent_at = app.agents.get(&AgentId(0)).unwrap().auto_topup.as_ref();
    assert_eq!(agent_at.and_then(|at| at.max_amount_cents), Some(10000));
}

#[test]
fn billing_fetched_unchanged_autotopup_keeps_cached_rule() {
    let mut app = test_app_with_agent();
    let bal = || crate::views::credit_bar::CreditBalance {
        prepaid_balance_cents: Some(1500),
        ..test_bal(100.0)
    };
    let resolved = crate::views::credit_bar::AutoTopupFetch::Resolved(
        crate::views::credit_bar::AutoTopupInfo {
            enabled: true,
            topup_amount_cents: Some(2000),
            max_amount_cents: None,
        },
    );
    dispatch(
        Action::TaskComplete(TaskResult::BillingFetched {
            agent_id: AgentId(0),
            balance: Some(bal()),
            silent: true,
            subscription_tier: None,
            autotopup: resolved,
        }),
        &mut app,
    );
    // A later refresh whose auto-topup fetch failed must not clear the rule.
    dispatch(
        Action::TaskComplete(TaskResult::BillingFetched {
            agent_id: AgentId(0),
            balance: Some(bal()),
            silent: true,
            subscription_tier: None,
            autotopup: crate::views::credit_bar::AutoTopupFetch::Unchanged,
        }),
        &mut app,
    );
    assert!(app.auto_topup.as_ref().is_some_and(|at| at.enabled));
    let agent_at = app.agents.get(&AgentId(0)).unwrap().auto_topup.as_ref();
    assert!(agent_at.is_some_and(|at| at.enabled));
}

#[test]
fn billing_fetched_cleared_autotopup_resets_cache() {
    let mut app = test_app_with_agent();
    // Seed a known rule while credits exist.
    dispatch(
        Action::TaskComplete(TaskResult::BillingFetched {
            agent_id: AgentId(0),
            balance: Some(crate::views::credit_bar::CreditBalance {
                prepaid_balance_cents: Some(1500),
                ..test_bal(100.0)
            }),
            silent: true,
            subscription_tier: None,
            autotopup: crate::views::credit_bar::AutoTopupFetch::Resolved(
                crate::views::credit_bar::AutoTopupInfo {
                    enabled: true,
                    topup_amount_cents: Some(2000),
                    max_amount_cents: None,
                },
            ),
        }),
        &mut app,
    );
    // Credits gone → `Cleared` resets the cached rule to "unknown" so a later
    // credits period can't read a stale rule.
    dispatch(
        Action::TaskComplete(TaskResult::BillingFetched {
            agent_id: AgentId(0),
            balance: Some(test_bal(50.0)),
            silent: true,
            subscription_tier: None,
            autotopup: crate::views::credit_bar::AutoTopupFetch::Cleared,
        }),
        &mut app,
    );
    assert!(app.auto_topup.is_none());
    assert!(app.agents.get(&AgentId(0)).unwrap().auto_topup.is_none());
}

#[test]
fn app_billing_fetched_stores_autotopup() {
    let mut app = test_app_with_agent();
    let bal = crate::views::credit_bar::CreditBalance {
        prepaid_balance_cents: Some(500),
        ..test_bal(0.0)
    };
    dispatch(
        Action::TaskComplete(TaskResult::AppBillingFetched {
            balance: Some(bal),
            autotopup: crate::views::credit_bar::AutoTopupFetch::Resolved(
                crate::views::credit_bar::AutoTopupInfo::disabled(),
            ),
        }),
        &mut app,
    );
    assert_eq!(
        app.credit_balance.and_then(|b| b.prepaid_balance_cents),
        Some(500)
    );
    assert!(app.auto_topup.is_some_and(|at| !at.enabled));
}

// ── BillingError dispatch tests ─────────────────────────────────────

#[test]
fn billing_error_silent_does_not_push_scrollback() {
    let mut app = test_app_with_agent();
    let before = agent_scrollback_len(&app);
    dispatch(
        Action::TaskComplete(TaskResult::BillingError {
            agent_id: AgentId(0),
            error: "network timeout".into(),
            silent: true,
        }),
        &mut app,
    );
    assert_eq!(
        agent_scrollback_len(&app),
        before,
        "silent billing error should not push a scrollback message"
    );
}

#[test]
fn billing_error_non_silent_pushes_error_message() {
    let mut app = test_app_with_agent();
    let before = agent_scrollback_len(&app);
    dispatch(
        Action::TaskComplete(TaskResult::BillingError {
            agent_id: AgentId(0),
            error: "service unavailable".into(),
            silent: false,
        }),
        &mut app,
    );
    assert_eq!(
        agent_scrollback_len(&app),
        before + 1,
        "non-silent billing error should push an error message"
    );
}

// ── Free-usage paywall tests ────────────────────────────────────────

#[test]
fn free_usage_error_detected_by_embedded_code() {
    // parse_error_bytes flattens the 429 body to "<code>: <message>".
    assert!(is_free_usage_exhausted_error(
        "API error (status 429 Too Many Requests): \
         subscription:free-usage-exhausted: You have used all your free usage."
    ));
    // Generic rate limits and other WKE codes must not match.
    assert!(!is_free_usage_exhausted_error(
        "API error (status 429 Too Many Requests): Rate limit exceeded"
    ));
    assert!(!is_free_usage_exhausted_error(
        "unauthorized:missing-acl: nope"
    ));
}

/// QIDI local patch: `open_free_usage_upsell` is a no-op (self-hosted
/// endpoints never hit the upstream free-usage paywall). The 429
/// exhaustion path still marks the session blocked; it just no longer
/// opens a modal. Guard both halves of that contract.
#[test]
fn free_usage_upsell_disabled_marks_block_without_modal() {
    use crate::app::acp_handler::apply_session_event_for_test;
    use cf_shell::extensions::notification::{RetryState, SessionUpdate};

    let mut app = test_app_with_agent();
    let id = AgentId(0);

    // 1. Real send.
    let effects = dispatch(Action::SendPrompt("draw me a cat".into()), &mut app);
    assert!(
        matches!(&effects[0], Effect::SendPrompt { text, .. } if text == "draw me a cat"),
        "send must dispatch: {effects:?}"
    );
    let prompt_id = app.agents[&id].session.current_prompt_id.clone();
    assert!(prompt_id.is_some(), "send must mint a prompt id");

    // 2+3. Real notification sequence through the production handler.
    {
        let agent = app.agents.get_mut(&id).unwrap();
        apply_session_event_for_test(
            &SessionUpdate::RetryState(RetryState::Retrying {
                attempt: 1,
                max_retries: 2,
                reason: "429 Too Many Requests".into(),
            }),
            &mut agent.session,
            &mut agent.scrollback,
        );
        apply_session_event_for_test(
            &SessionUpdate::RetryState(RetryState::Exhausted {
                attempts: 2,
                reason: "API error (status 429 Too Many Requests): \
                         subscription:free-usage-exhausted: You have used all your free usage."
                    .into(),
                is_rate_limited: true,
            }),
            &mut agent.session,
            &mut agent.scrollback,
        );
        assert!(agent.session.free_usage_blocked);
    }

    // 4. Turn-end RPC error: no paywall modal. (The session-level
    // `free_usage_blocked` flag is transient turn state -- finish_turn
    // clears it during the same dispatch -- so the surviving contract is
    // the absent modal, not the flag.)
    let _ = dispatch(
        Action::TaskComplete(TaskResult::PromptResponse {
            agent_id: id,
            result: Err("rate limited".into()),
            http_status: Some(429),
            prompt_id,
        }),
        &mut app,
    );
    let agent = &app.agents[&id];
    assert!(
        agent.question_view.is_none(),
        "paywall modal must NOT open under the QIDI patch"
    );

    // 5. Direct upsell call is likewise a no-op.
    let agent = app.agents.get_mut(&id).unwrap();
    open_free_usage_upsell(agent, None);
    assert!(
        agent.question_view.is_none(),
        "open_free_usage_upsell must be a no-op under the QIDI patch"
    );
}
/// QIDI local patch: restricted commands are still intercepted (never
/// passthrough, never enqueued, composer cleared when no modal is up)
/// but the SuperGrok upsell modal itself is disabled — no question view
/// may appear. These tests guard that exact contract.
#[test]
fn restricted_command_intercepted_without_upsell_modal() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    app.agents
        .get_mut(&id)
        .unwrap()
        .set_restricted_commands(&["imagine".to_string()]);

    let effects = dispatch(Action::SendPrompt("/imagine a sunset".into()), &mut app);

    assert!(
        effects.is_empty(),
        "restricted command must not produce a SendPrompt: {effects:?}"
    );
    let agent = &app.agents[&id];
    assert!(
        agent.session.pending_prompts.is_empty(),
        "restricted command must not be enqueued"
    );
    assert!(agent.prompt.text().is_empty(), "composer consumed");
    assert!(
        agent.question_view.is_none(),
        "upsell modal must NOT open under the QIDI patch"
    );
}

/// Aliases of a restricted command hit the same interception (deny-list
/// matching covers aliases via the registry).
#[test]
fn restricted_command_alias_also_intercepted() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    app.agents
        .get_mut(&id)
        .unwrap()
        .set_restricted_commands(&["usage".to_string()]);

    let effects = dispatch(Action::SendPrompt("/cost".into()), &mut app);

    assert!(effects.is_empty());
    assert!(
        app.agents[&id].question_view.is_none(),
        "no upsell modal under the QIDI patch"
    );
    assert!(
        app.agents[&id].session.pending_prompts.is_empty(),
        "alias interception must not enqueue"
    );
}

/// A restricted submit while ANOTHER question modal is already open
/// must not silently drop the typed text: the composer keeps the text
/// for a later resubmit. No passthrough, nothing enqueued, and the
/// existing modal survives untouched. (The modal here is a local
/// QuestionView built directly — not an upsell — so it stays open.)
#[test]
fn restricted_command_with_open_modal_keeps_composer_text() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    {
        let agent = app.agents.get_mut(&id).unwrap();
        agent.set_restricted_commands(&["imagine".to_string()]);
        // A local question modal is already up (credit-limit upsell is a
        // no-op under the patch, so build one directly).
        agent.question_view = Some(
            crate::views::question_view::QuestionViewState::new(
                "test-tool-call".to_string(),
                Vec::new(),
                crate::views::prompt_widget::StashedPrompt::from_submission(
                    String::new(),
                    Vec::new(),
                    Vec::new(),
                ),
            )
            .with_local_kind(crate::views::question_view::LocalQuestionKind::CreditLimitUpsell {
                choices: Vec::new(),
            }),
        );
        assert!(agent.question_view.is_some());
        // The user typed the restricted command into the composer.
        agent.prompt.set_text("/imagine a sunset");
    }

    let effects = dispatch(Action::SendPrompt("/imagine a sunset".into()), &mut app);

    assert!(effects.is_empty(), "no passthrough / send: {effects:?}");
    let agent = &app.agents[&id];
    assert_eq!(
        agent.prompt.text(),
        "/imagine a sunset",
        "composer text must be preserved for a later resubmit"
    );
    assert!(
        matches!(
            agent
                .question_view
                .as_ref()
                .and_then(|qv| qv.local_kind.as_ref()),
            Some(crate::views::question_view::LocalQuestionKind::CreditLimitUpsell { .. })
        ),
        "the pre-existing modal must survive (no second modal)"
    );
    assert!(
        agent.session.pending_prompts.is_empty(),
        "nothing may be enqueued"
    );
}
#[test]
fn unknown_non_restricted_command_still_passes_through() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    app.agents
        .get_mut(&id)
        .unwrap()
        .set_restricted_commands(&["imagine".to_string()]);

    let effects = dispatch(Action::SendPrompt("/frobnicate arg".into()), &mut app);

    assert_eq!(effects.len(), 1);
    assert!(
        matches!(&effects[0], Effect::SendPrompt { text, .. } if text == "/frobnicate arg"),
        "unknown command must still pass through: {effects:?}"
    );
    assert!(
        app.agents[&id].question_view.is_none(),
        "no upsell for genuinely unknown commands"
    );
}
