//! Phase C Reasoning-summary pre-generation (session layer): the async side of
//! the pre-generation/application split.
//!
//! The deterministic half lives in `cf-chat-state` (`rule_extract_summary`,
//! `reasoning_block_fingerprint`, `subset_check`). This module owns the LLM
//! attribution call ([`ReasoningSummarizer`]) plus the pure trigger/throttle/
//! composition logic that is unit-testable without a live session.
//!
//! # Session-layer hook-up (batch 3)
//!
//! The `SessionActor` hook-up lives in
//! `crate::session::acp_session::reasoning_pregenerate` — after each sampling
//! completes, `maybe_pregenerate_reasoning_summaries` fires a background task
//! that [`collect_eligible_reasoning_blocks`]s, summarizes each block through
//! a [`ReasoningSummarizer`] (throttled by [`throttle_interval_ms`]), then
//! batch-writes the results via
//! `cf_chat_state::ChatStateHandle::set_reasoning_summaries`. The 40% batch
//! clear in `cf-chat-state` only applies already-ready summaries, so the
//! pre-generation is non-blocking.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cf_sampling_types::{ConversationItem, ConversationRequest};

/// Context-utilization percentage at which pre-generation kicks in (Phase C,
/// C-1). Fires well below the 40% batch-clear watermark so summaries are
/// ready before the clear window opens.
pub const PREGENERATION_UTILIZATION_PERCENT: u64 = 35;

/// Minimum / maximum throttle delay between consecutive summary calls
/// (Phase C, C-1: 2–5 s interval).
pub const SUMMARY_THROTTLE_MIN_MS: u64 = 2_000;
pub const SUMMARY_THROTTLE_MAX_MS: u64 = 5_000;

// M10: the pre-generation age gate is **not** a hard-coded constant. The
// session layer reads `hard_clear_age_turns` from the live config
// (`ChatStateHandle::get_hard_clear_age_turns`) and passes it into
// `collect_eligible_reasoning_blocks`, so the pre-generation and prune age
// thresholds share a single source and cannot drift.

/// B-2: reasoning blocks shorter than this many characters are never
/// summarized. A short block's rule floor plus attribution is very unlikely to
/// be smaller than the block itself (net-negative replacement), and it would
/// also cost a wasted LLM input pass for no gain — so short blocks are left
/// entirely alone (a table miss keeps the original, C-1b never-bare-clear).
pub const MIN_SUMMARIZABLE_BLOCK_CHARS: usize = 80;

/// The summarizer prompt, written verbatim per the design doc (C-1):
/// ① keep key qualitative attribution words verbatim ② keep secondary
/// corroborating numbers with their units verbatim ③ never invent causal
/// claims or numbers absent from the original ④ never rewrite existing numbers.
pub const REASONING_SUMMARIZER_PROMPT: &str = "你是摘要器。对给定推理块输出 1-2 行归因框架摘要：①关键定性归因句逐字保留定性词 ②次级佐证数字连同单位逐字保留 ③禁止补写任何原文没有的因果或数字 ④禁止改写已有数字";

/// A candidate reasoning block for summarization (age ≥ threshold,
/// non-encrypted). Produced by [`collect_eligible_reasoning_blocks`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EligibleReasoningBlock {
    /// Content fingerprint (cf-chat-state `reasoning_block_fingerprint`) —
    /// the key the session layer writes into the `ReasoningSummaries` table.
    pub fingerprint: u64,
    /// Visible text of the block (what the summarizer reads).
    pub text: String,
}

/// Whether pre-generation should start: utilization ≥
/// [`PREGENERATION_UTILIZATION_PERCENT`].
pub fn should_pregenerate(utilization_percent: u64) -> bool {
    utilization_percent >= PREGENERATION_UTILIZATION_PERCENT
}

/// Try to claim the session-scoped pre-generation guard. Returns `true` when
/// this call won the claim (no pre-generation was running) and the guard is
/// now held; `false` when a pre-generation is already running (reentrant
/// call). The caller must release the guard (`.store(false, Release)`) when
/// the background task finishes.
pub fn try_claim_pregeneration(guard: &AtomicBool) -> bool {
    guard
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
}

/// M4: owning RAII guard for the session-scoped pre-generation reentrancy
/// flag. Holding a [`PregenerationGuard`] means this task owns the flag;
/// dropping it — on normal completion, an early `return`, **or unwinding from
/// a panic** — resets the flag so a later turn can pre-generate again. This
/// replaces the previous manual `.store(false)` at task exit, which was
/// skipped on panic and would have permanently stalled pre-generation for the
/// session.
pub struct PregenerationGuard {
    flag: Arc<AtomicBool>,
}

impl Drop for PregenerationGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Release);
    }
}

/// M4: try to claim the pre-generation guard, returning an owning RAII guard
/// on success and `None` when a pre-generation is already running. The caller
/// moves the guard into the spawned pre-generation task so the flag is
/// released on every exit path (including panic / task cancellation).
pub fn try_claim_pregeneration_guard(flag: Arc<AtomicBool>) -> Option<PregenerationGuard> {
    if try_claim_pregeneration(flag.as_ref()) {
        Some(PregenerationGuard { flag })
    } else {
        None
    }
}

/// Deterministic throttle delay for the i-th summary call, clamped to
/// [`SUMMARY_THROTTLE_MIN_MS`, `SUMMARY_THROTTLE_MAX_MS`]. A simple arithmetic
/// stagger (no RNG) so calls in a batch don't fire together.
pub fn throttle_interval_ms(block_index: usize) -> u64 {
    let spread = SUMMARY_THROTTLE_MAX_MS - SUMMARY_THROTTLE_MIN_MS;
    let step = spread / 4;
    let offset = (block_index as u64 % 5).saturating_mul(step);
    SUMMARY_THROTTLE_MIN_MS
        .saturating_add(offset)
        .min(SUMMARY_THROTTLE_MAX_MS)
}

/// [`throttle_interval_ms`] as a [`Duration`] (for `tokio::time::sleep`).
pub fn throttle_interval(block_index: usize) -> Duration {
    Duration::from_millis(throttle_interval_ms(block_index))
}

/// N2 (step-5 第五方审计): minimum gap between the END of one pre-generation
/// run and the START of the next. The session-scoped
/// [`try_claim_pregeneration_guard`] only serializes runs; without a
/// cross-run gap a run that just finished can be followed immediately by
/// another (every turn end re-enters), so the throttled 2–5 s/block LLM
/// channel never rests between rounds. Enforced at claim time by the
/// `SessionActor` (`reasoning_pregen_last_run_end`).
pub const CROSS_RUN_MIN_GAP: Duration = Duration::from_secs(5);

/// N2 pure decision: may a new pre-generation run start at `now`, given when
/// the previous run ended? `None` (no completed run yet) always allows; a gap
/// of exactly [`CROSS_RUN_MIN_GAP`] allows (only "less than 5 s" gives up).
/// A "future" end stamp (clock oddity) saturates to a zero gap → blocked,
/// which is the conservative direction for the channel.
pub fn cross_run_gap_allows(last_run_end: Option<Instant>, now: Instant) -> bool {
    match last_run_end {
        None => true,
        Some(end) => now.saturating_duration_since(end) >= CROSS_RUN_MIN_GAP,
    }
}

/// Compose the final summary: deterministic rule floor + the LLM attribution
/// line. An empty attribution yields the bare floor.
pub fn compose_summary(rule_floor: &str, attribution: &str) -> String {
    let attribution = attribution.trim();
    if attribution.is_empty() {
        return rule_floor.to_string();
    }
    format!("{rule_floor}；归因：{attribution}")
}

/// Compose the summary with the hard subset assertion (Phase C, C-1): if the
/// LLM attribution introduces any number not present in the original block,
/// the whole block degrades to the pure rule floor. Blind fix 5
/// (deepseek-flash audit): a floor that *itself* violates the subset contract
/// (e.g. byte truncation split a number atom, `120` → `1`) must not be
/// returned as the degrade result — the empty string tells the caller to
/// write nothing so the batched-prune table miss (never-bare-clear) keeps the
/// original block.
pub fn summarize_with_guard(rule_floor: &str, attribution: &str, original: &str) -> String {
    let composed = compose_summary(rule_floor, attribution);
    if cf_chat_state::subset_check(&composed, original) {
        composed
    } else if cf_chat_state::subset_check(rule_floor, original) {
        rule_floor.to_string()
    } else {
        String::new()
    }
}

/// Marker carried by a synthesized Reasoning-summary sibling (see
/// `cf-chat-state`'s `rule_extract_summary` output prefix). A block whose text
/// already contains this marker is a prior summary and must never be
/// re-summarized (R2, sibling re-summarization defense).
pub const REASONING_SUMMARY_MARKER: &str = "[推理摘要|";

/// Marker prefix of the R1 visible-text (`Assistant`) sibling. The sibling is
/// an `Assistant` item, so it never enters the Reasoning scan domain — this is
/// **defense in depth**: if a Reasoning-shaped summary ever escapes
/// (regression), both markers still exclude it.
pub const REASONING_SUMMARY_SIBLING_MARKER: &str = "[reasoning summary]";

/// Backward-scan for eligible (age ≥ `effective_age_turns`, non-encrypted)
/// Reasoning blocks, mirroring the retained-prune backward scan so both layers
/// agree on turn-age: every `User` item is a turn boundary, and a block's age
/// is its distance in turns from the newest turn.
///
/// # Synthetic-User compensation (R5/B6)
///
/// The backward scan counts every `User` item as a turn boundary, but the shell
/// injects synthetic `User` items (system reminders, corrective warnings)
/// mid-turn without calling `increment_prompt_index`. Those would make blocks
/// look older than they are, so the effective age threshold is raised by the
/// synthetic count (`total_user_items − prompt_index`) — the same compensation
/// the retained-prune uses (`mutations.rs`), so both layers agree.
///
/// # Filters
/// - Encrypted blocks are skipped (C-1b ② — never summarized, never cleared).
/// - Blocks shorter than [`MIN_SUMMARIZABLE_BLOCK_CHARS`] are skipped (B-2 —
///   net-negative to summarize, and a wasted LLM input pass).
/// - Blocks whose text already carries [`REASONING_SUMMARY_MARKER`] are skipped
///   (never re-summarize a summary).
/// - Blocks already present in `already_summarized` are skipped (the table is
///   merge-written and rebuilt each pre-generation run; re-summarizing would
///   churn the C-2 prefix and waste LLM calls).
///
/// Returns blocks oldest-first for stable throttling order.
pub fn collect_eligible_reasoning_blocks(
    conversation: &[ConversationItem],
    age_turns: usize,
    prompt_index: usize,
    already_summarized: &cf_chat_state::ReasoningSummaries,
) -> Vec<EligibleReasoningBlock> {
    // Synthetic-User compensation, aligned with the retained-prune
    // (`mutations.rs`): a synthetic User item must not make a block look older
    // than it really is.
    let total_user_items = conversation
        .iter()
        .filter(|i| matches!(i, ConversationItem::User(_)))
        .count();
    let synthetic_count = total_user_items.saturating_sub(prompt_index);
    let effective_age_turns = age_turns.saturating_add(synthetic_count);

    let mut out = Vec::new();
    let mut turn_from_end: usize = 0;
    let mut seen_first_user = false;

    for item in conversation.iter().rev() {
        if matches!(item, ConversationItem::User(_)) {
            if seen_first_user {
                turn_from_end += 1;
            }
            seen_first_user = true;
            continue;
        }
        let ConversationItem::Reasoning(r) = item else {
            continue;
        };
        if r.encrypted_content.is_some() {
            continue;
        }
        if turn_from_end < effective_age_turns {
            continue;
        }
        let text = cf_sampling_types::reasoning_item_text(r);
        // B-2: skip short blocks — summarizing them is net-negative (their
        // rule floor can exceed the block) and wastes an LLM input pass. A
        // skipped block simply has no table entry, so the batched prune leaves
        // the original untouched (C-1b never-bare-clear).
        if text.chars().count() < MIN_SUMMARIZABLE_BLOCK_CHARS {
            continue;
        }
        // Never re-summarize a synthesized summary sibling — defense in depth:
        // the rule-floor marker *and* the R1 visible-text (`[reasoning summary]`)
        // marker both exclude it.
        if text.contains(REASONING_SUMMARY_MARKER)
            || text.contains(REASONING_SUMMARY_SIBLING_MARKER)
        {
            continue;
        }
        let fingerprint = cf_chat_state::reasoning_block_fingerprint(&text);
        // Skip blocks the session has already summarized.
        if already_summarized.contains_key(&fingerprint) {
            continue;
        }
        out.push(EligibleReasoningBlock { fingerprint, text });
    }

    out.reverse();
    out
}

/// Produce the LLM attribution line for a reasoning block via the session's
/// sampler — the existing one-shot sampling channel (mirrors
/// `run_dream_model_call`: `prepare_chat_completion` →
/// `conversation_collect` → `assistant_text`).
pub async fn summarize_block_with_client(
    client: &cf_sampler::SamplingClient,
    model: Option<String>,
    session_id: &str,
    block_text: &str,
) -> Result<String, String> {
    let request = ConversationRequest {
        items: vec![
            ConversationItem::system(REASONING_SUMMARIZER_PROMPT),
            ConversationItem::user(block_text),
        ],
        model,
        x_grok_conv_id: Some(session_id.to_string()),
        x_grok_req_id: Some(format!("xai-reasoning-summary-{}", uuid::Uuid::new_v4())),
        ..Default::default()
    };
    let response = client
        .conversation_collect(request)
        .await
        .map_err(|e| format!("reasoning summary call failed: {e}"))?;
    Ok(response.assistant_text())
}

/// The LLM attribution side of the pre-generation/application split.
#[async_trait::async_trait]
pub trait ReasoningSummarizer: Send + Sync {
    /// Produce a 1–2 line attribution summary for `block_text`. Errors are
    /// surfaced so the caller can degrade to the pure rule floor.
    async fn summarize_block(&self, block_text: &str) -> Result<String, String>;
}

/// [`ReasoningSummarizer`] backed by a fresh [`cf_sampler::SamplingClient`],
/// constructed per pre-generation run so it carries refreshed auth (mirrors
/// `run_dream_model_call`'s `prepare_chat_completion` path).
pub struct SamplingReasoningSummarizer {
    client: cf_sampler::SamplingClient,
    model: Option<String>,
    session_id: String,
}

impl SamplingReasoningSummarizer {
    pub fn new(
        client: cf_sampler::SamplingClient,
        model: Option<String>,
        session_id: String,
    ) -> Self {
        Self {
            client,
            model,
            session_id,
        }
    }
}

#[async_trait::async_trait]
impl ReasoningSummarizer for SamplingReasoningSummarizer {
    async fn summarize_block(&self, block_text: &str) -> Result<String, String> {
        summarize_block_with_client(&self.client, self.model.clone(), &self.session_id, block_text)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reasoning_block(text: &str, encrypted: Option<&str>) -> ConversationItem {
        ConversationItem::Reasoning(cf_sampling_types::rs::ReasoningItem {
            id: String::new(),
            summary: vec![cf_sampling_types::rs::SummaryPart::SummaryText(
                cf_sampling_types::rs::SummaryTextContent {
                    text: text.to_string(),
                },
            )],
            content: None,
            encrypted_content: encrypted.map(str::to_owned),
            status: None,
        })
    }

    /// B-2: a reasoning block text comfortably above [`MIN_SUMMARIZABLE_BLOCK_CHARS`]
    /// so it is not filtered by the short-block gate.
    fn long_text(prefix: &str) -> String {
        format!("{prefix} {}", "x".repeat(90))
    }

    #[test]
    fn should_pregenerate_gates_at_35_percent() {
        assert!(!should_pregenerate(0));
        assert!(!should_pregenerate(34));
        assert!(should_pregenerate(35));
        assert!(should_pregenerate(50));
        assert!(should_pregenerate(100));
    }

    #[test]
    fn try_claim_pregeneration_is_reentrancy_guarded() {
        let guard = AtomicBool::new(false);
        // First claim wins.
        assert!(try_claim_pregeneration(&guard));
        // A reentrant claim while held fails.
        assert!(!try_claim_pregeneration(&guard));
        assert!(!try_claim_pregeneration(&guard));
        // Release then re-claim succeeds.
        guard.store(false, Ordering::Release);
        assert!(try_claim_pregeneration(&guard));
    }

    /// M4: the RAII guard releases the flag even when the holder panics, so a
    /// later turn can pre-generate again (the old manual release was skipped on
    /// panic and permanently stalled pre-generation).
    #[test]
    fn pregeneration_guard_releases_on_panic_and_drop() {
        let flag = Arc::new(AtomicBool::new(false));

        // Normal drop releases.
        {
            let _g = try_claim_pregeneration_guard(Arc::clone(&flag)).expect("first claim wins");
            assert!(flag.load(Ordering::Acquire), "flag held while guard alive");
        }
        assert!(
            !flag.load(Ordering::Acquire),
            "M4: dropping the guard must release the flag"
        );

        // Panic unwinds through Drop → flag released.
        let panic_flag = Arc::clone(&flag);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = try_claim_pregeneration_guard(Arc::clone(&panic_flag)).expect("re-claim after drop");
            panic!("simulated pre-generation panic");
        }));
        assert!(result.is_err(), "the closure must have panicked");
        assert!(
            !flag.load(Ordering::Acquire),
            "M4: a panic must still release the flag via Drop"
        );
        assert!(
            try_claim_pregeneration_guard(Arc::clone(&flag)).is_some(),
            "M4: the flag must be re-claimable after a panic release"
        );
    }

    #[test]
    fn throttle_interval_stays_within_bounds() {
        for i in 0..50 {
            let ms = throttle_interval_ms(i);
            assert!(
                (SUMMARY_THROTTLE_MIN_MS..=SUMMARY_THROTTLE_MAX_MS).contains(&ms),
                "throttle index {i} must stay in [2s, 5s], got {ms}ms"
            );
        }
        // Deterministic: same index → same delay.
        assert_eq!(throttle_interval_ms(3), throttle_interval_ms(3));
    }

    /// N2: the cross-run gap decision — no previous run always allows; a gap
    /// shorter than [`CROSS_RUN_MIN_GAP`] gives up; exactly the gap (or more)
    /// allows.
    #[test]
    fn cross_run_gap_allows_enforces_minimum_interval() {
        let now = Instant::now();
        assert!(
            cross_run_gap_allows(None, now),
            "N2: the first run (no completed predecessor) must always be allowed"
        );
        assert!(
            !cross_run_gap_allows(Some(now), now),
            "N2: a run ending now must block an immediate re-entry (< 5 s)"
        );
        assert!(
            !cross_run_gap_allows(Some(now - Duration::from_millis(4_999)), now),
            "N2: 4.999 s after the last run's end must still give up (< 5 s)"
        );
        assert!(
            cross_run_gap_allows(Some(now - Duration::from_secs(5)), now),
            "N2: exactly 5 s after the last run's end must allow (gap >= min)"
        );
        assert!(
            cross_run_gap_allows(Some(now - Duration::from_secs(3_600)), now),
            "N2: a long-idle channel must allow a new run"
        );
    }

    #[test]
    fn compose_summary_appends_attribution_line() {
        assert_eq!(
            compose_summary("[推理摘要|规则抽取] 数字/参数：42", "关键归因：42 个"),
            "[推理摘要|规则抽取] 数字/参数：42；归因：关键归因：42 个"
        );
        // Empty attribution degrades to the bare floor.
        assert_eq!(
            compose_summary("[推理摘要|规则抽取] 数字/参数：42", "  "),
            "[推理摘要|规则抽取] 数字/参数：42"
        );
    }

    #[test]
    fn summarize_with_guard_keeps_valid_attribution() {
        let floor = "[推理摘要|规则抽取] 数字/参数：42";
        let attribution = "关键归因：42 个";
        let original = "处理 42 个文件";
        // Attribution numbers (42) ⊆ original numbers (42).
        assert_eq!(
            summarize_with_guard(floor, attribution, original),
            format!("{floor}；归因：{attribution}")
        );
    }

    #[test]
    fn summarize_with_guard_degrades_on_out_of_bounds_number() {
        let floor = "[推理摘要|规则抽取] 数字/参数：42";
        let attribution = "关键归因：128 个"; // 128 not in original
        let original = "处理 42 个文件";
        // 128 ∉ {42} → degrade to the pure rule floor.
        assert_eq!(summarize_with_guard(floor, attribution, original), floor);
    }

    /// Blind fix 5 (deepseek-flash audit): when the attribution pushes the
    /// composed line out of contract AND the rule floor itself violates the
    /// contract (a byte-truncation-split atom, `120` → `1`), the degrade path
    /// must return empty — never write a contract-violating floor; the caller
    /// skips the write and the table miss (C-1b never-bare-clear) keeps the
    /// original block.
    #[test]
    fn summarize_with_guard_returns_empty_when_floor_itself_violates_subset() {
        let floor = "数字/参数：1"; // split atom of `120` (byte-truncation class)
        let attribution = "关键归因：999 个"; // invented number → composed fails
        let original = "处理 120 个";
        assert_eq!(
            summarize_with_guard(floor, attribution, original),
            "",
            "盲面5/C-1b: a floor that itself violates the subset contract must degrade to empty, not be written"
        );
        // Control: the same poisoned floor with a *clean* attribution must
        // still degrade to empty — the floor check is unconditional.
        assert_eq!(
            summarize_with_guard(floor, "", original),
            "",
            "盲面5/C-1b: empty attribution + poisoned floor must degrade to empty as well"
        );
    }

    #[test]
    fn collect_eligible_reasoning_blocks_filters_by_age_and_encryption() {
        // B-2: all clear-text blocks are long enough to clear the short-block
        // gate, so age/encryption are the only filters exercised here.
        let old = long_text("old clear text");
        let conv = vec![
            ConversationItem::user("q0"),
            reasoning_block(&old, None), // age 1 → eligible
            ConversationItem::user("q1"),
            reasoning_block(&long_text("old encrypted"), Some("enc")), // age 1 but encrypted → skip
            ConversationItem::user("q2"),
            reasoning_block(&long_text("young text"), None), // age 0 → skip (too young)
        ];

        let eligible = collect_eligible_reasoning_blocks(
            &conv,
            1,
            3, // three real turns → no synthetic inflation
            &cf_chat_state::ReasoningSummaries::new(),
        );
        assert_eq!(eligible.len(), 1, "only the aged non-encrypted block is eligible");
        assert_eq!(eligible[0].text, old);
        assert_eq!(
            eligible[0].fingerprint,
            cf_chat_state::reasoning_block_fingerprint(&old)
        );
    }

    /// B-2: a block shorter than [`MIN_SUMMARIZABLE_BLOCK_CHARS`] is never
    /// collected (its rule floor can exceed the block / wastes an LLM pass).
    #[test]
    fn collect_eligible_reasoning_blocks_skips_short_blocks() {
        let short = "x".repeat(MIN_SUMMARIZABLE_BLOCK_CHARS - 1);
        let long = long_text("boundary");
        assert!(long.chars().count() >= MIN_SUMMARIZABLE_BLOCK_CHARS);
        let conv = vec![
            reasoning_block(&short, None), // just under the gate → skip
            reasoning_block(&long, None),  // at/over the gate → eligible
            ConversationItem::user("q0"),
        ];
        let eligible = collect_eligible_reasoning_blocks(
            &conv,
            0,
            1,
            &cf_chat_state::ReasoningSummaries::new(),
        );
        assert_eq!(
            eligible.len(),
            1,
            "B-2: only the >= {MIN_SUMMARIZABLE_BLOCK_CHARS}-char block is eligible, got {eligible:?}"
        );
        assert_eq!(eligible[0].text, long, "B-2: the long block must be the collected one");
    }

    #[test]
    fn collect_eligible_reasoning_blocks_empty_when_none_eligible() {
        let conv = vec![
            ConversationItem::user("q0"),
            reasoning_block(&long_text("enc"), Some("enc")),
        ];
        let empty = cf_chat_state::ReasoningSummaries::new();
        assert!(collect_eligible_reasoning_blocks(&conv, 0, 1, &empty).is_empty());
        assert!(collect_eligible_reasoning_blocks(&[], 0, 0, &empty).is_empty());
    }

    #[test]
    fn collect_eligible_reasoning_blocks_compensates_for_synthetic_user_items() {
        // The block sits before the only User item → turn_from_end = 0.
        let blk = long_text("blk");
        let conv = vec![reasoning_block(&blk, None), ConversationItem::user("q0")];
        let empty = cf_chat_state::ReasoningSummaries::new();
        // prompt_index == user count → no synthetic users → eligible at age 0.
        let with_real = collect_eligible_reasoning_blocks(&conv, 0, 1, &empty);
        assert_eq!(with_real.len(), 1, "R5/B6: a real turn makes the block eligible");
        // prompt_index < user count → one synthetic User inflates the effective
        // threshold to 1, so the same block is NOT eligible.
        let with_synthetic = collect_eligible_reasoning_blocks(&conv, 0, 0, &empty);
        assert!(
            with_synthetic.is_empty(),
            "R5/B6: a synthetic User item must raise the effective age threshold"
        );
    }

    #[test]
    fn collect_eligible_reasoning_blocks_skips_already_summarized_and_markers() {
        // B-2: every block is long enough that only the summary/marker filters
        // (not the short-block gate) can be responsible for the empty result.
        let fresh = long_text("fresh block");
        let floor_marked = long_text("[推理摘要|规则抽取] 数字/参数：1");
        let sibling_marked = long_text("[reasoning summary] 早期合成摘要无规则地板前缀");
        let conv = vec![
            reasoning_block(&fresh, None),
            reasoning_block(&floor_marked, None),
            // R1 visible-text sibling marker WITHOUT the rule-floor prefix —
            // uniquely exercises the `[reasoning summary]` filter branch.
            reasoning_block(&sibling_marked, None),
            ConversationItem::user("q0"),
        ];
        let mut table = cf_chat_state::ReasoningSummaries::new();
        table.insert(
            cf_chat_state::reasoning_block_fingerprint(&fresh),
            "s".to_string(),
        );
        let eligible = collect_eligible_reasoning_blocks(&conv, 0, 1, &table);
        assert!(
            eligible.is_empty(),
            "R2: already-summarized and marker-carrying blocks (rule-floor + R1 sibling) \
             must be skipped, got {eligible:?}"
        );
    }
}
