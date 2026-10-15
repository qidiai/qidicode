use super::*;

use crate::session::reasoning_summarizer::{
    collect_eligible_reasoning_blocks, cross_run_gap_allows, should_pregenerate,
    summarize_with_guard, throttle_interval, try_claim_pregeneration_guard,
    EligibleReasoningBlock, ReasoningSummarizer, SamplingReasoningSummarizer,
};
use cf_chat_state::rule_extract_summary;

/// B-3: maximum reasoning blocks summarized in a single pre-generation round.
/// A round that reaches the cap stops early and the next round picks up the
/// rest (unsummarized blocks simply stay eligible), so a large backlog cannot
/// monopolize the serial, throttled (2–5 s/block) LLM channel for minutes.
const PREGENERATION_BATCH_LIMIT: usize = 20;

/// B-3: the subset of eligible blocks a single pre-generation round processes —
/// the first [`PREGENERATION_BATCH_LIMIT`] (oldest-first), with the remainder
/// deferred to the next round.
fn pregeneration_round_blocks(blocks: &[EligibleReasoningBlock]) -> &[EligibleReasoningBlock] {
    let take = blocks.len().min(PREGENERATION_BATCH_LIMIT);
    &blocks[..take]
}

impl SessionActor {
    /// C-6-gated, non-blocking Reasoning-summary pre-generation trigger.
    ///
    /// Called at turn end (after sampling completes). Spawns a background
    /// task when context utilization reaches the 35% "approach" line so block
    /// summaries are ready before the 40% batch-clear window opens. The
    /// session-scoped reentrancy guard ensures at most one pre-generation runs
    /// at a time; the RAII guard resets it on exit (including panic). The N2
    /// cross-run gap (`reasoning_summarizer::CROSS_RUN_MIN_GAP`) additionally
    /// spaces consecutive runs apart.
    ///
    /// This is the async *pre-generation* half of the pre-generation/
    /// application split: the LLM call stays in this session layer, never in
    /// cf-chat-state.
    pub(super) async fn maybe_pregenerate_reasoning_summaries(self: &Arc<Self>) {
        // R6: check the batched-mode flag *before* claiming the guard or
        // spawning, so the per-turn cost when Phase C is off is a single cheap
        // channel query — not a task spawn that immediately returns.
        if !self.chat_state_handle.is_batched_pruning_enabled().await {
            return;
        }
        // N2 (step-5 第五方审计): cross-run throttle — a completed run buys
        // the serial, throttled LLM channel a breather: if the previous run
        // ended less than `CROSS_RUN_MIN_GAP` ago, give up this round (the
        // next turn end re-attempts). Checked before claiming the guard so a
        // rejected round costs nothing.
        if !cross_run_gap_allows(
            self.reasoning_pregen_last_run_end.get(),
            std::time::Instant::now(),
        ) {
            return;
        }
        // M4: RAII guard — the flag is released on *every* exit path
        // (normal completion, early return, or a panic unwinding the spawned
        // task), not only the happy path.
        let Some(guard) =
            try_claim_pregeneration_guard(Arc::clone(&self.reasoning_pregenerating))
        else {
            return;
        };
        let actor = Arc::clone(self);
        tokio::task::spawn_local(async move {
            if let Err(e) = actor.run_reasoning_pregeneration().await {
                tracing::debug!(error = %e, "reasoning-summary pre-generation failed");
            }
            // N2: stamp the run's end time on every completed run (success or
            // error — the throttled channel time was consumed either way). A
            // panicking run skips the stamp, which conservatively allows the
            // next round.
            actor
                .reasoning_pregen_last_run_end
                .set(Some(std::time::Instant::now()));
            // Explicit drop documents that the guard is released here on the
            // normal path; Drop also covers panic/cancellation.
            drop(guard);
        });
    }

    /// Background pre-generation body: gate → collect → summarize → write.
    async fn run_reasoning_pregeneration(&self) -> Result<(), String> {
        // C-6 gate: batched pruning off → zero action (re-checked inside the
        // spawn as a belt-and-suspenders against a mid-flight flag change).
        if !self.chat_state_handle.is_batched_pruning_enabled().await {
            return Ok(());
        }
        // Trigger line: utilization ≥ 35%.
        let utilization = self
            .chat_state_handle
            .get_context_utilization_percent()
            .await;
        if !should_pregenerate(utilization) {
            return Ok(());
        }

        let conversation = self.chat_state_handle.get_conversation().await;
        let prompt_index = self.chat_state_handle.get_prompt_index().await;
        // M10: read the hard-clear age from the live config so the
        // pre-generation age gate and the prune-side age gate share one source
        // (no hard-coded 10 drifting from a user-configured value).
        let hard_clear_age_turns = self.chat_state_handle.get_hard_clear_age_turns().await;
        // Skip blocks already summarized (the table is merge-written and this
        // batch is rebuilt every run — re-summarizing would churn the C-2
        // prefix and waste throttled LLM calls).
        let already_summarized = self.chat_state_handle.get_reasoning_summaries().await;
        let blocks = collect_eligible_reasoning_blocks(
            &conversation,
            hard_clear_age_turns,
            prompt_index,
            &already_summarized,
        );
        if blocks.is_empty() {
            return Ok(());
        }

        let client = self
            .prepare_chat_completion(false)
            .await
            .map_err(|e| format!("prepare sampling client: {e}"))?;
        let model = self
            .chat_state_handle
            .get_sampling_config()
            .await
            .map(|c| c.model);
        let session_id = self.session_info.id.to_string();
        let summarizer = SamplingReasoningSummarizer::new(client, model, session_id);

        let mut summaries = cf_chat_state::ReasoningSummaries::new();
        // B-3: cap each round at `PREGENERATION_BATCH_LIMIT` blocks (stop early;
        // the next round continues with the rest).
        for (index, block) in pregeneration_round_blocks(&blocks).iter().enumerate() {
            // 2–5 s throttle between consecutive summary calls (skip the first).
            if index > 0 {
                tokio::time::sleep(throttle_interval(index)).await;
            }
            // Never-bare-clear: a block with no extractable floor is skipped.
            let Some(floor) = rule_extract_summary(&block.text) else {
                continue;
            };
            // R2/B2+B3: an LLM error OR an empty attribution line means the
            // block gets no entry — the table *miss* then leaves the original
            // reasoning untouched (C-1b never-bare-clear), rather than writing
            // a bare rule floor.
            let attribution = match summarizer.summarize_block(&block.text).await {
                Ok(attr) if !attr.trim().is_empty() => attr,
                _ => continue,
            };
            let summary = summarize_with_guard(&floor, &attribution, &block.text);
            summaries.insert(block.fingerprint, summary);
        }

        if !summaries.is_empty() {
            self.chat_state_handle.set_reasoning_summaries(summaries);
        }
        Ok(())
    }
}

// TODO(batch 3, 明晚跑): integration test for the real-actor timing path.
// A hermetic unit test can't drive `maybe_pregenerate_reasoning_summaries`
// (it needs a spawned `SessionActor` on a `LocalSet` plus a live
// `chat_state_handle`). When the build is available, add an `acp_session_tests`
// case that: (1) seeds a session with age≥10 non-encrypted Reasoning blocks,
// (2) drives one turn to completion under `batched_pruning=true` + utilization
// ≥35%, (3) asserts `get_reasoning_summaries`-equivalent table is populated
// (or, with a stub summarizer, that the blocks were replaced on the next
// batch-clear window). The pure-logic halves are already covered hermetically:
// `should_pregenerate` / `try_claim_pregeneration` / `collect_eligible_*` /
// `summarize_with_guard`.

#[cfg(test)]
mod pregeneration_round_tests {
    use super::{PREGENERATION_BATCH_LIMIT, pregeneration_round_blocks};
    use crate::session::reasoning_summarizer::EligibleReasoningBlock;

    fn blocks(n: usize) -> Vec<EligibleReasoningBlock> {
        (0..n)
            .map(|i| EligibleReasoningBlock {
                fingerprint: i as u64,
                text: format!("block-{i}"),
            })
            .collect()
    }

    /// B-3: a round processes at most [`PREGENERATION_BATCH_LIMIT`] blocks; the
    /// rest wait for the next round. A shorter backlog is processed in full.
    #[test]
    fn caps_round_at_the_batch_limit() {
        let many = blocks(PREGENERATION_BATCH_LIMIT + 5);
        assert_eq!(
            pregeneration_round_blocks(&many).len(),
            PREGENERATION_BATCH_LIMIT,
            "B-3: a round must process at most {PREGENERATION_BATCH_LIMIT} blocks"
        );
        // Oldest-first: the retained blocks are the prefix of the input.
        assert_eq!(
            pregeneration_round_blocks(&many)[0].fingerprint,
            0,
            "B-3: the round must keep the oldest-first prefix"
        );
        let few = blocks(3);
        assert_eq!(
            pregeneration_round_blocks(&few).len(),
            3,
            "B-3: a round shorter than the cap processes all its blocks"
        );
    }
}
