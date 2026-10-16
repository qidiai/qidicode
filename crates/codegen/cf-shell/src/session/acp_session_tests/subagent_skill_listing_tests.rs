//! F2: subagent baseline skill listing injection.
//!
//! A subagent (`PromptAudience::Subagent`) session must inject its baseline
//! skill listing **unconditionally**, read-only against the cross-session
//! announcement ledger (`SkillManager::announced_names`). The projection is
//! governed by `[skills].subagent_listing` (essential | full | none).
//!
//! These tests drive the real `inject_baseline_skill_reminder` seam and assert
//! the injected reminder, the projection, and — critically — that the ledger
//! is never touched by the subagent path.

use super::SessionActor;
use super::support::create_test_actor;
use cf_agent::prompt::context::PromptAudience;
use cf_agent::prompt::skills::{SkillsConfig, SubagentSkillListing};
use cf_sampling_types::{ConversationItem, SyntheticReason};
use cf_tools::types::compat::CompatConfig;
use cf_tools::types::skill_discovery_tracker::SkillsMode;

/// An identifiable seed skill. `check-work` is on the built-in essential
/// allowlist; the other two are not.
fn seed_skill(name: &str) -> cf_tools::implementations::skills::types::SkillInfo {
    cf_tools::implementations::skills::types::SkillInfo {
        name: name.to_owned(),
        description: format!("Seed description for {name}."),
        path: format!("/tmp/skills/{name}/SKILL.md"),
        ..Default::default()
    }
}

/// Build a test actor whose rebuild spec carries `audience` and the requested
/// `[skills].subagent_listing`.
async fn actor_with(
    audience: PromptAudience,
    listing: Option<SubagentSkillListing>,
) -> SessionActor {
    let (gw_tx, _gw_rx) = tokio::sync::mpsc::unbounded_channel();
    let (persist_tx, _persist_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut actor = create_test_actor(0, 256_000, 85, gw_tx, persist_tx).await;
    let spec = std::sync::Arc::get_mut(&mut actor.rebuild_spec)
        .expect("test actor uniquely owns its rebuild spec");
    spec.prompt_audience = audience;
    spec.skills_config = SkillsConfig {
        subagent_listing: listing,
        ..Default::default()
    };
    actor
}

/// Seed the actor's live ToolBridge with three skills. `global_mode` sets the
/// *primary* projection so we can prove the child's knob is independent.
async fn seed_skills(actor: &SessionActor, global_mode: SkillsMode) {
    let bridge = actor.agent.borrow().tool_bridge().clone();
    bridge
        .seed_skill_discovery(
            Some(std::path::PathBuf::from("/tmp")),
            None,
            vec![
                seed_skill("check-work"),
                seed_skill("bid-write"),
                seed_skill("random-tool"),
            ],
            None,
            Some(256_000),
            None,
            CompatConfig::default(),
            global_mode,
            None,
        )
        .await;
}

/// The single synthetic `SystemReminder` text in the conversation, if any.
fn reminder_text(conversation: &[ConversationItem]) -> Option<String> {
    conversation
        .iter()
        .find(|item| {
            matches!(
                item, ConversationItem::User(u) if u.synthetic_reason ==
                Some(SyntheticReason::SystemReminder)
            )
        })
        .map(ConversationItem::text_content)
}

#[tokio::test(flavor = "current_thread")]
async fn subagent_default_injects_essential_projection() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let actor = actor_with(PromptAudience::Subagent, None).await;
            // Primary projection is Full here, to prove the child's
            // essential projection is independent of it.
            seed_skills(&actor, SkillsMode::Full).await;
            let mut conv = vec![ConversationItem::system("SP")];

            actor.inject_baseline_skill_reminder(&mut conv).await;

            assert_eq!(
                conv.len(),
                2,
                "subagent must append exactly one baseline skill reminder"
            );
            let text = reminder_text(&conv).expect("a system-reminder must be injected");
            assert!(
                text.contains("The following skills are available for use:"),
                "reminder must carry the skill catalog header:\n{text}"
            );
            assert!(
                text.contains("check-work"),
                "allowlisted core skill must be present in the essential projection:\n{text}"
            );
            assert!(
                !text.contains("bid-write") && !text.contains("random-tool"),
                "non-allowlisted skills must be projected out (essential, not full):\n{text}"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn subagent_listing_full_injects_every_skill() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let actor = actor_with(PromptAudience::Subagent, Some(SubagentSkillListing::Full)).await;
            // Primary projection is Essential, to prove the child's Full
            // projection is independent of it.
            seed_skills(&actor, SkillsMode::Essential).await;
            let mut conv = vec![ConversationItem::system("SP")];

            actor.inject_baseline_skill_reminder(&mut conv).await;

            let text = reminder_text(&conv).expect("full listing must inject");
            for name in ["check-work", "bid-write", "random-tool"] {
                assert!(
                    text.contains(name),
                    "`full` must list {name}:\n{text}"
                );
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn subagent_listing_none_injects_nothing() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let actor = actor_with(PromptAudience::Subagent, Some(SubagentSkillListing::None)).await;
            seed_skills(&actor, SkillsMode::Full).await;
            let mut conv = vec![ConversationItem::system("SP")];

            actor.inject_baseline_skill_reminder(&mut conv).await;

            assert_eq!(
                conv.len(),
                1,
                "`none` must not inject any baseline skill reminder"
            );
            assert!(
                reminder_text(&conv).is_none(),
                "no system-reminder may be present when subagent_listing = none"
            );
        })
        .await;
}

/// The subagent injection is a **read-only bypass**: it must not read the
/// inherited ledger to suppress itself, nor write it. It also drops the stale
/// pending baseline so the first tool call cannot drain a second listing.
#[tokio::test(flavor = "current_thread")]
async fn subagent_injection_bypasses_and_never_pollutes_ledger() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let actor = actor_with(PromptAudience::Subagent, None).await;
            let bridge = actor.agent.borrow().tool_bridge().clone();
            // Simulate the inherited "everything already announced" ledger.
            bridge
                .restore_announced_skill_names(
                    ["check-work", "bid-write", "random-tool"]
                        .iter()
                        .map(|s| (*s).to_string())
                        .collect(),
                )
                .await;
            seed_skills(&actor, SkillsMode::Full).await;
            // Re-inherit after seed so the ledger is non-empty going in.
            bridge
                .restore_announced_skill_names(
                    ["check-work", "bid-write", "random-tool"]
                        .iter()
                        .map(|s| (*s).to_string())
                        .collect(),
                )
                .await;
            let ledger_before = bridge.get_announced_skill_names().await;
            assert_eq!(ledger_before.len(), 3, "precondition: ledger is populated");

            let mut conv = vec![ConversationItem::system("SP")];
            actor.inject_baseline_skill_reminder(&mut conv).await;

            assert!(
                reminder_text(&conv).is_some(),
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
        })
        .await;
}

/// Primary regression: the primary session path is unchanged — it still drains
/// the pending baseline and writes the ledger, and the `subagent_listing` knob
/// is ignored for it.
#[tokio::test(flavor = "current_thread")]
async fn primary_session_path_unchanged_and_writes_ledger() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            // A primary actor with a deliberate `subagent_listing = none` — it
            // must have no effect on the primary injection.
            let actor =
                actor_with(PromptAudience::Primary, Some(SubagentSkillListing::None)).await;
            seed_skills(&actor, SkillsMode::Full).await;
            let bridge = actor.agent.borrow().tool_bridge().clone();

            let mut conv = vec![ConversationItem::system("SP")];
            actor.inject_baseline_skill_reminder(&mut conv).await;

            let text = reminder_text(&conv)
                .expect("primary must still receive its baseline skill listing");
            assert!(text.contains("check-work"), "{text}");
            assert!(
                !bridge.get_announced_skill_names().await.is_empty(),
                "primary path must still write the announcement ledger"
            );
        })
        .await;
}
