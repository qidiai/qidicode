use super::*;

use crate::session::reasoning_summarizer::{
    collect_eligible_reasoning_blocks, should_pregenerate, summarize_with_guard,
    throttle_interval, try_claim_pregeneration, REASONING_SUMMARY_AGE_TURNS,
    ReasoningSummarizer, SamplingReasoningSummarizer,
};
use cf_chat_state::rule_extract_summary;

impl SessionActor {
    /// C-6-gated, non-blocking Reasoning-summary pre-generation trigger.
    ///
    /// Called at turn end (after sampling completes). Spawns a background
    /// task when context utilization reaches the 35% "approach" line so block
    /// summaries are ready before the 40% batch-clear window opens. The
    /// session-scoped reentrancy guard ensures at most one pre-generation runs
    /// at a time; the task resets it on exit.
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
        if !try_claim_pregeneration(self.reasoning_pregenerating.as_ref()) {
            return;
        }
        let actor = Arc::clone(self);
        tokio::task::spawn_local(async move {
            if let Err(e) = actor.run_reasoning_pregeneration().await {
                tracing::debug!(error = %e, "reasoning-summary pre-generation failed");
            }
            actor
                .reasoning_pregenerating
                .store(false, std::sync::atomic::Ordering::Release);
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
        // Skip blocks already summarized (the table is merge-written and this
        // batch is rebuilt every run — re-summarizing would churn the C-2
        // prefix and waste throttled LLM calls).
        let already_summarized = self.chat_state_handle.get_reasoning_summaries().await;
        let blocks = collect_eligible_reasoning_blocks(
            &conversation,
            REASONING_SUMMARY_AGE_TURNS,
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
        for (index, block) in blocks.iter().enumerate() {
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
