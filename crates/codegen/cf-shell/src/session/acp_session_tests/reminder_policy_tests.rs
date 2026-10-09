use super::support::create_test_actor;
use super::{
    date_rollover_reminder, goal_slash_and_harness_available, initial_announced_date,
    laziness_injection_active, prefix_stamped_date, resolve_reminder_policy, todo_gate_active,
};
use crate::sampling::ConversationItem;
use crate::session::persistence::PersistenceMsg;
use crate::util::config::RemoteSettings;
use cf_agent::AgentDefinition;
use cf_agent::prompt::context::{PromptAudience, TemplateOverride};
use cf_agent::system_reminder::{
    DEFAULT_TODO_GATE_MAX_FIRES, ReminderPolicy, TodoGateConfig,
};
/// Helper: a `RemoteSettings` whose only non-default fields are the
/// TodoGate knobs we want to vary. Mirrors `Default::default()` for
/// everything else so the test stays robust to unrelated additions.
fn remote_with_todo_gate(enabled: Option<bool>, cap: Option<u32>) -> RemoteSettings {
    RemoteSettings {
        todo_gate_enabled: enabled,
        todo_gate_max_fires_per_prompt: cap,
        ..RemoteSettings::default()
    }
}
#[test]
fn remote_none_preserves_built_in_defaults() {
    let policy = resolve_reminder_policy(None, false);
    assert_eq!(
        policy.todo_gate,
        TodoGateConfig {
            enabled: false,
            max_fires_per_prompt: DEFAULT_TODO_GATE_MAX_FIRES,
        },
    );
    assert!(policy.enabled);
    assert!(policy.todo_nudge.enabled);
}
#[test]
fn remote_disable_matches_default_path() {
    let remote = remote_with_todo_gate(Some(false), None);
    let policy = resolve_reminder_policy(Some(&remote), false);
    assert_eq!(
        policy.todo_gate,
        TodoGateConfig {
            enabled: false,
            max_fires_per_prompt: DEFAULT_TODO_GATE_MAX_FIRES,
        },
    );
}
#[test]
fn remote_enable_true_overrides_default() {
    let remote = remote_with_todo_gate(Some(true), None);
    let policy = resolve_reminder_policy(Some(&remote), false);
    assert_eq!(
        policy.todo_gate,
        TodoGateConfig {
            enabled: true,
            max_fires_per_prompt: DEFAULT_TODO_GATE_MAX_FIRES,
        },
    );
}
#[test]
fn remote_cap_override_applies_without_enabling_gate() {
    let remote = remote_with_todo_gate(None, Some(5));
    let policy = resolve_reminder_policy(Some(&remote), false);
    assert_eq!(
        policy.todo_gate,
        TodoGateConfig {
            enabled: false,
            max_fires_per_prompt: 5,
        },
    );
}
#[test]
fn cli_todo_gate_overrides_remote_enable_false() {
    let remote = remote_with_todo_gate(Some(false), Some(7));
    let policy = resolve_reminder_policy(Some(&remote), true);
    assert_eq!(
        policy.todo_gate,
        TodoGateConfig {
            enabled: true,
            max_fires_per_prompt: 7,
        },
    );
}
#[test]
fn remote_settings_deserializes_without_todo_gate_fields() {
    let legacy_json = "{}";
    let settings: RemoteSettings = serde_json::from_str(legacy_json).unwrap();
    assert_eq!(settings.todo_gate_enabled, None);
    assert_eq!(settings.todo_gate_max_fires_per_prompt, None);
    let policy = resolve_reminder_policy(Some(&settings), false);
    assert_eq!(
        policy.todo_gate,
        TodoGateConfig {
            enabled: false,
            max_fires_per_prompt: DEFAULT_TODO_GATE_MAX_FIRES,
        },
    );
}
#[test]
fn remote_settings_accepts_explicit_null_todo_gate_fields() {
    let json = r#"{
            "todo_gate_enabled": null,
            "todo_gate_max_fires_per_prompt": null
        }"#;
    let settings: RemoteSettings = serde_json::from_str(json).unwrap();
    assert_eq!(settings.todo_gate_enabled, None);
    assert_eq!(settings.todo_gate_max_fires_per_prompt, None);
}
#[test]
fn remote_settings_preserves_false_and_zero_todo_gate_fields() {
    let json = r#"{
            "todo_gate_enabled": false,
            "todo_gate_max_fires_per_prompt": 0
        }"#;
    let settings: RemoteSettings = serde_json::from_str(json).unwrap();
    assert_eq!(settings.todo_gate_enabled, Some(false));
    assert_eq!(settings.todo_gate_max_fires_per_prompt, Some(0));
}
fn def_with_template(tpl: TemplateOverride) -> AgentDefinition {
    let mut def = AgentDefinition::default_qidi_build();
    def.system_prompt = tpl;
    def
}
fn policy_with_gate(enabled: bool) -> ReminderPolicy {
    let mut p = ReminderPolicy::default();
    p.todo_gate.enabled = enabled;
    p
}
use crate::session::goal_tracker::GoalStatus;
#[test]
fn goal_slash_and_harness_available_predicate_matrix() {
    use cf_tools::implementations::qidi_build::UPDATE_GOAL_TOOL_NAME;
    let other = vec!["todo_write".to_string()];
    let with_update = vec![UPDATE_GOAL_TOOL_NAME.to_string()];
    for (goal_enabled, tool_names, expect) in [
        (false, &other, false),
        (true, &other, false),
        (true, &with_update, true),
        (false, &with_update, false),
    ] {
        assert_eq!(
            goal_slash_and_harness_available(goal_enabled, tool_names),
            expect,
            "goal_enabled={goal_enabled} tools={tool_names:?}",
        );
    }
}
#[test]
fn laziness_injection_active_predicate_matrix() {
    let def = def_with_template(TemplateOverride::None);
    let policy_on = policy_with_gate(true);
    for (goal_harness_enabled, goal_status, expect) in [
        (false, None, false),
        (false, Some(GoalStatus::Active), false),
        (true, None, false),
        (true, Some(GoalStatus::Active), true),
        (true, Some(GoalStatus::Complete), false),
        (true, Some(GoalStatus::UserPaused), false),
    ] {
        assert_eq!(
            laziness_injection_active(goal_harness_enabled, goal_status),
            expect,
            "goal_harness_enabled={goal_harness_enabled} status={goal_status:?}",
        );
        assert!(
            !todo_gate_active(
                &policy_on,
                PromptAudience::Primary,
                &def,
                goal_harness_enabled,
                goal_status,
            ),
            "todo gate must be suppressed during the active goal loop",
        );
    }
}
#[test]
fn todo_gate_active_predicate_matrix() {
    let def = def_with_template(TemplateOverride::None);
    let policy_off = policy_with_gate(false);
    let policy_on = policy_with_gate(true);
    for (policy, audience, goal_harness_enabled, goal_status, expect) in [
        (&policy_off, PromptAudience::Primary, true, None, false),
        (&policy_off, PromptAudience::Subagent, true, None, false),
        (
            &policy_off,
            PromptAudience::Primary,
            true,
            Some(GoalStatus::Active),
            false,
        ),
        (
            &policy_on,
            PromptAudience::Primary,
            true,
            Some(GoalStatus::Active),
            false,
        ),
        (
            &policy_on,
            PromptAudience::Subagent,
            true,
            Some(GoalStatus::Active),
            false,
        ),
        (&policy_on, PromptAudience::Primary, false, None, false),
        (
            &policy_on,
            PromptAudience::Primary,
            false,
            Some(GoalStatus::Active),
            false,
        ),
        (&policy_on, PromptAudience::Primary, true, None, false),
    ] {
        assert_eq!(
            todo_gate_active(policy, audience, &def, goal_harness_enabled, goal_status),
            expect,
            "gate.enabled={} audience={audience:?} goal_harness_enabled={goal_harness_enabled} status={goal_status:?}",
            policy.todo_gate.enabled
        );
    }
    for status in [
        GoalStatus::Complete,
        GoalStatus::UserPaused,
        GoalStatus::BackOffPaused,
        GoalStatus::InfraPaused,
        GoalStatus::Blocked,
        GoalStatus::BudgetLimited,
    ] {
        assert!(
            !todo_gate_active(
                &policy_on,
                PromptAudience::Primary,
                &def,
                true,
                Some(status)
            ),
            "non-active status {status:?} must not enable gate"
        );
    }
    let templates = vec![
        TemplateOverride::None,
        TemplateOverride::Codex,
        TemplateOverride::Custom("custom".into()),
    ];
    for tpl in templates {
        let def = def_with_template(tpl);
        for audience in [PromptAudience::Primary, PromptAudience::Subagent] {
            assert!(
                !todo_gate_active(&policy_on, audience, &def, true, None),
                "built-in template without active goal must not enable gate"
            );
        }
    }
}
use chrono::NaiveDate;
fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("valid test date")
}
#[test]
fn date_rollover_reminder_silent_when_same_day() {
    let today = ymd(2026, 4, 24);
    assert!(date_rollover_reminder(today, today).is_none());
}
#[test]
fn date_rollover_reminder_fires_when_day_advances() {
    let last = ymd(2026, 4, 24);
    let today = ymd(2026, 4, 25);
    let msg = date_rollover_reminder(today, last).expect("rollover should fire");
    assert!(
        msg.contains("2026-04-25"),
        "must announce the new date: {msg}"
    );
    assert!(
        !msg.contains("2026-04-24"),
        "must not echo the stale date: {msg}"
    );
}
#[test]
fn date_rollover_reminder_fires_across_month_and_year_boundaries() {
    assert!(date_rollover_reminder(ymd(2026, 5, 1), ymd(2026, 4, 30)).is_some());
    assert!(date_rollover_reminder(ymd(2027, 1, 1), ymd(2026, 12, 31)).is_some());
}
#[test]
fn date_rollover_reminder_silent_when_clock_moves_backward() {
    let last = ymd(2026, 4, 25);
    let today = ymd(2026, 4, 24);
    assert!(date_rollover_reminder(today, last).is_none());
}
#[tokio::test(flavor = "current_thread")]
async fn same_session_rolls_over_once_when_local_date_advances() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<cf_acp_lib::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(50_000, 256_000, 85, gateway_tx, persistence_tx).await;
            let today = chrono::Local::now().date_naive();
            assert_eq!(actor.last_announced_local_date.get(), today);
            actor.maybe_inject_date_rollover_reminder().await;
            assert_eq!(
                actor.chat_state_handle.get_conversation_len().await,
                0,
                "same-day turn must not inject a rollover reminder"
            );
            let yesterday = today.pred_opt().expect("today is never the min date");
            actor.last_announced_local_date.set(yesterday);
            actor.maybe_inject_date_rollover_reminder().await;
            let conv = actor.chat_state_handle.get_conversation().await;
            assert_eq!(conv.len(), 1, "rollover must inject exactly one reminder");
            let text = conv[0].text_content();
            assert!(
                text.contains("<system-reminder>"),
                "rollover reminder must be wrapped in system-reminder tags: {text}"
            );
            assert!(
                text.contains("The local date has changed since this session started"),
                "rollover reminder must announce the date change: {text}"
            );
            assert!(
                text.contains(&today.to_string()),
                "rollover reminder must carry today's date {today}: {text}"
            );
            assert_eq!(actor.last_announced_local_date.get(), today);
            actor.maybe_inject_date_rollover_reminder().await;
            assert_eq!(
                actor.chat_state_handle.get_conversation_len().await,
                1,
                "rollover must not re-fire on a later same-day turn"
            );
        })
        .await;
}
/// A realistic `<user_info>` prefix as built by `construct_user_message_minimal`.
fn user_info_prefix(date: NaiveDate) -> String {
    format!(
        "<user_info>\nOS Version: windows\nShell: pwsh\nWorkspace Path: C:\\repo\n\
         Today's date: {date}\n</user_info>"
    )
}
#[test]
fn prefix_stamped_date_parses_user_info_prefix() {
    // Prefix lives at index 1, right after the system prompt.
    let conv = vec![
        ConversationItem::system("sys"),
        ConversationItem::user(user_info_prefix(ymd(2026, 1, 1))),
    ];
    assert_eq!(prefix_stamped_date(&conv), Some(ymd(2026, 1, 1)));
    // Seeding picks up the stamped (stale) date instead of "now".
    assert_eq!(initial_announced_date(&conv), ymd(2026, 1, 1));
}
#[test]
fn prefix_stamped_date_returns_none_without_or_with_bad_stamp() {
    // No prefix at all (fresh session).
    assert_eq!(prefix_stamped_date(&[]), None);
    assert_eq!(
        prefix_stamped_date(&[ConversationItem::system("sys")]),
        None
    );
    // Malformed date → no parseable stamp.
    let bad = vec![ConversationItem::user(
        "<user_info>\nToday's date: not-a-date\n</user_info>",
    )];
    assert_eq!(prefix_stamped_date(&bad), None);
    // Non-user items are skipped, not mistaken for the prefix.
    let sys_only = vec![ConversationItem::system("Today's date: 2026-01-01")];
    assert_eq!(prefix_stamped_date(&sys_only), None);
}
#[test]
fn prefix_stamped_date_only_scans_conversation_head() {
    // Companion to the fallback: a stamp buried past the head window must NOT
    // be found (guards against an O(n) full-history scan on long sessions).
    let mut conv = vec![ConversationItem::system("sys")];
    for _ in 0..6 {
        conv.push(ConversationItem::user("filler turn"));
    }
    conv.push(ConversationItem::user(user_info_prefix(ymd(2026, 1, 1))));
    assert_eq!(
        prefix_stamped_date(&conv),
        None,
        "a prefix buried past the head window must be ignored"
    );
}
#[test]
fn initial_announced_date_falls_back_to_now_without_prefix() {
    let today = chrono::Local::now().date_naive();
    assert_eq!(initial_announced_date(&[]), today);
    let bad = vec![ConversationItem::user(
        "<user_info>\nToday's date: garbage\n</user_info>",
    )];
    assert_eq!(initial_announced_date(&bad), today);
}
/// Bug B end-to-end at the pure-function seam: a session resumed on a later day
/// seeds its baseline from the stale prefix, and the very next turn's rollover
/// reminder fires with the current date.
#[tokio::test(flavor = "current_thread")]
async fn resumed_prefix_seed_makes_rollover_reminder_fire() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<cf_acp_lib::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(50_000, 256_000, 85, gateway_tx, persistence_tx).await;
            let today = chrono::Local::now().date_naive();
            let yesterday = today.pred_opt().expect("today is never the min date");
            // Simulate what spawn does on resume: seed from the persisted prefix.
            let resumed = vec![
                ConversationItem::system("sys"),
                ConversationItem::user(user_info_prefix(yesterday)),
            ];
            actor.last_announced_local_date.set(initial_announced_date(&resumed));
            assert_eq!(
                actor.last_announced_local_date.get(),
                yesterday,
                "baseline must be seeded from the stale prefix, not 'now'"
            );
            actor.maybe_inject_date_rollover_reminder().await;
            let conv = actor.chat_state_handle.get_conversation().await;
            assert_eq!(conv.len(), 1, "rollover must inject exactly one reminder");
            let text = conv[0].text_content();
            assert!(
                text.contains(&today.to_string()),
                "the injected reminder must carry the current date {today}: {text}"
            );
            assert_eq!(actor.last_announced_local_date.get(), today);
        })
        .await;
}
