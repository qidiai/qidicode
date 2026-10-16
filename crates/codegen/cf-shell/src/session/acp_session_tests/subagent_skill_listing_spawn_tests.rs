//! F2R: subagent baseline skill listing injection — production (spawn) path.
//!
//! These tests drive [`inject_subagent_baseline_listing`], the exact function
//! `spawn_session_actor` calls when `prompt_audience == Subagent`. A subagent
//! never sends `SessionCommand::Initialize`, so the `session_setup.rs` seam
//! (`inject_baseline_skill_reminder`) is unreachable for it at spawn — that was
//! the original F2 defect. Driving the actor method there proved only "branch
//! logic is correct", not "production reaches it".
//!
//! Instead of the actor-method seam, the `ToolBridge` here is built through the
//! real `ToolBridge::finalize_builder` path that `spawn_session_actor` uses (a
//! fresh `SkillManager` seeded from `ctx.skills`, `AvailableSkills` seeded
//! unprojected), then the spawn-time helper is invoked over the conversation
//! the spawn path assembles. This guards against the "function-level green,
//! production unreachable" regression.

use cf_agent::prompt::skills::SkillsConfig;
use cf_sampling_types::{ConversationItem, SyntheticReason};
use cf_tools::computer::local::{LocalTerminalBackend, MockFs};
use cf_tools::computer::types::{AsyncFileSystem, TerminalBackend};
use cf_tools::notification::ToolNotificationHandle;
use cf_tools::registry::types::{SessionContext, ToolServerConfig};
use cf_tools::types::resources::AvailableSkills;

use super::spawn::inject_subagent_baseline_listing;

/// `check-work` is on the built-in essential allowlist; the other two are not.
fn seed_skill(name: &str) -> cf_tools::implementations::skills::types::SkillInfo {
    cf_tools::implementations::skills::types::SkillInfo {
        name: name.to_owned(),
        description: format!("Seed description for {name}."),
        path: format!("/tmp/skills/{name}/SKILL.md"),
        ..Default::default()
    }
}

/// Build a `ToolBridge` exactly the way `spawn_session_actor` does: through the
/// registry `finalize()` path, seeding a fresh `SkillManager` + unprojected
/// `AvailableSkills` from `ctx.skills`.
async fn bridge_with_skills() -> crate::tools::bridge::ToolBridge {
    let cwd = std::env::temp_dir().join("qidi-subagent-listing-spawn-e2e");
    let fs: std::sync::Arc<dyn AsyncFileSystem> = std::sync::Arc::new(MockFs::new());
    let terminal: std::sync::Arc<dyn TerminalBackend> =
        std::sync::Arc::new(LocalTerminalBackend::new());
    let builder = crate::tools::bridge::ToolBridge::get_builder();
    let ctx = SessionContext {
        backend: terminal,
        fs,
        cwd,
        session_folder: std::env::temp_dir().join("qidi-subagent-listing-spawn-e2e-folder"),
        session_env: std::sync::Arc::new(std::collections::HashMap::new()),
        notification_handle: ToolNotificationHandle::noop(),
        owner_session_id: None,
        parent_scheduler_handle: None,
        skills: vec![
            seed_skill("check-work"),
            seed_skill("bid-write"),
            seed_skill("random-tool"),
        ],
        state_path: std::env::temp_dir().join("qidi-subagent-listing-spawn-e2e/tool_state.json"),
        memory_backend: None,
        web_search_config: Default::default(),
        web_fetch_config: Default::default(),
        lsp: None,
        image_gen_config: Default::default(),
        video_gen_config: Default::default(),
        app_builder_deployer_config: Default::default(),
        api_key_provider: None,
        auth_provider: None,
        attribution_callback: None,
        system_reminder_tag: cf_tools::reminders::DEFAULT_REMINDER_TAG,
    };
    crate::tools::bridge::ToolBridge::finalize_builder(
        builder,
        ToolServerConfig {
            tools: vec![],
            behavior_preset: None,
        },
        ctx,
    )
    .await
    .expect("finalize_builder should succeed")
}

/// Index of the single synthetic `SystemReminder`, if present.
fn reminder_index(conversation: &[ConversationItem]) -> Option<usize> {
    conversation.iter().position(|item| {
        matches!(
            item, ConversationItem::User(u) if u.synthetic_reason ==
            Some(SyntheticReason::SystemReminder)
        )
    })
}

async fn available_skill_names(bridge: &crate::tools::bridge::ToolBridge) -> Vec<String> {
    let resources = bridge.shared_resources().await;
    let res = resources.lock().await;
    let mut names: Vec<String> = res
        .get::<AvailableSkills>()
        .map(|a| a.0.iter().map(|s| s.name.clone()).collect())
        .unwrap_or_default();
    names.sort_unstable();
    names
}

/// The spawn path injects the Essential projection before the child's first
/// real user turn, and installs the matching (projected) runtime set.
#[tokio::test]
async fn spawn_path_injects_essential_projection_before_first_user_turn() {
    let bridge = bridge_with_skills().await;
    // `finalize()` seeded `AvailableSkills` with the full, unprojected catalog.
    assert_eq!(
        available_skill_names(&bridge).await,
        vec!["bid-write", "check-work", "random-tool"],
        "precondition: finalize seeds the unprojected catalog"
    );

    // The conversation the spawn path assembles for a New subagent: just the
    // system prompt before the task prompt arrives.
    let mut conv = vec![ConversationItem::system("SP")];
    inject_subagent_baseline_listing(&mut conv, &bridge, &SkillsConfig::default(), false).await;

    assert_eq!(
        conv.len(),
        2,
        "subagent spawn must append exactly one baseline skill reminder"
    );
    let idx = reminder_index(&conv).expect("a synthetic system-reminder must be injected");
    assert_eq!(idx, 1, "the reminder must sit right after the system prompt");
    let text = conv[idx].text_content();
    assert!(
        text.contains("<system-reminder>") && text.contains("</system-reminder>"),
        "the injected block must be wrapped as a system-reminder:\n{text}"
    );
    assert!(
        text.contains("check-work"),
        "allowlisted core skill must be present in the Essential projection:\n{text}"
    );
    assert!(
        !text.contains("bid-write") && !text.contains("random-tool"),
        "non-allowlisted skills must be projected out (essential, not full):\n{text}"
    );

    // It precedes every real (non-synthetic) user turn — the task prompt the
    // spawn path sends afterwards.
    let first_real_user = conv.iter().position(|item| {
        matches!(item, ConversationItem::User(u) if u.synthetic_reason.is_none())
    });
    assert!(
        first_real_user.is_none_or(|first| idx < first),
        "the baseline listing must precede the first real user turn"
    );

    // O-1: the skill tool's activatable set now matches the listing.
    assert_eq!(
        available_skill_names(&bridge).await,
        vec!["check-work"],
        "AvailableSkills must be projected to match the listing (O-1)"
    );
}

/// `full` injects every skill and keeps the whole runtime set.
#[tokio::test]
async fn spawn_path_full_listing_lists_and_enables_every_skill() {
    let bridge = bridge_with_skills().await;
    let cfg = SkillsConfig {
        subagent_listing: Some(cf_agent::prompt::skills::SubagentSkillListing::Full),
        ..Default::default()
    };
    let mut conv = vec![ConversationItem::system("SP")];
    inject_subagent_baseline_listing(&mut conv, &bridge, &cfg, false).await;

    let idx = reminder_index(&conv).expect("full listing must inject");
    let text = conv[idx].text_content();
    for name in ["check-work", "bid-write", "random-tool"] {
        assert!(text.contains(name), "`full` must list {name}:\n{text}");
    }
    assert_eq!(
        available_skill_names(&bridge).await,
        vec!["bid-write", "check-work", "random-tool"],
        "`full` must keep the whole runtime set"
    );
}

/// `none` injects nothing and leaves the runtime set untouched.
#[tokio::test]
async fn spawn_path_none_listing_injects_nothing() {
    let bridge = bridge_with_skills().await;
    let cfg = SkillsConfig {
        subagent_listing: Some(cf_agent::prompt::skills::SubagentSkillListing::None),
        ..Default::default()
    };
    let mut conv = vec![ConversationItem::system("SP")];
    inject_subagent_baseline_listing(&mut conv, &bridge, &cfg, false).await;

    assert_eq!(conv.len(), 1, "`none` must not inject a baseline reminder");
    assert!(
        reminder_index(&conv).is_none(),
        "no system-reminder may be present when subagent_listing = none"
    );
    assert_eq!(
        available_skill_names(&bridge).await,
        vec!["bid-write", "check-work", "random-tool"],
        "`none` must not override the runtime set"
    );
}

/// The injected reminder is read-only against the announcement ledger, and the
/// stale pending baseline is dropped so the first tool call cannot drain a
/// second listing.
#[tokio::test]
async fn spawn_path_injection_is_read_only_and_drops_stale_pending() {
    let bridge = bridge_with_skills().await;
    // `finalize()` populated the ledger with the full catalog; the child must
    // still get its listing and must not mutate the ledger.
    let ledger_before = bridge.get_announced_skill_names().await;
    assert!(
        !ledger_before.is_empty(),
        "precondition: finalize populated the ledger"
    );

    let mut conv = vec![ConversationItem::system("SP")];
    inject_subagent_baseline_listing(&mut conv, &bridge, &SkillsConfig::default(), false).await;

    assert!(
        reminder_index(&conv).is_some(),
        "an inherited non-empty ledger must NOT suppress the subagent listing"
    );
    assert_eq!(
        bridge.get_announced_skill_names().await,
        ledger_before,
        "the subagent baseline path must not mutate the announcement ledger"
    );
    assert!(
        bridge.apply_pending_skill_update().await.is_none(),
        "the stale pending baseline must be dropped so the first tool call \
         cannot inject a second listing"
    );
}
