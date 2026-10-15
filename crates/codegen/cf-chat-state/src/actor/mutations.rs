//! Mutation handlers for the ChatStateActor.

use cf_sampling_types::{
    ContentPart, ConversationItem, DanglingToolCallReason, dedup_duplicate_tool_results,
    repair_dangling_tool_calls,
};

use super::ChatStateActor;
use crate::events::ChatStateEvent;
use crate::types::ChatStateSnapshot;

/// Static string label for tracing on `ConversationItem` (avoids pulling
/// the `Role` enum into the format string).
fn item_kind_str(item: &ConversationItem) -> &'static str {
    match item {
        ConversationItem::System(_) => "system",
        ConversationItem::User(_) => "user",
        ConversationItem::Assistant(_) => "assistant",
        ConversationItem::ToolResult(_) => "tool_result",
        ConversationItem::BackendToolCall(_) => "backend_tool_call",
        ConversationItem::Reasoning(_) => "reasoning",
    }
}

/// Marker prefix for a synthesized visible-text (C-1) Reasoning-summary
/// sibling. The sibling is rendered as an `Assistant` text item — never a
/// `Reasoning` item, since some providers drop `reasoning_content` wholesale
/// (design doc C-1, haiku-4.5 实锤).
const REASONING_SUMMARY_SIBLING_PREFIX: &str = "[reasoning summary] ";

/// Render the visible-text sibling body for a Reasoning block's summary.
fn reasoning_summary_sibling_text(summary: &str) -> String {
    format!("{REASONING_SUMMARY_SIBLING_PREFIX}{summary}")
}

/// Args fingerprint for the C-4 replay debounce: a tool call's identity as the
/// model issued it (name + arguments), so repeated re-runs of the same call
/// share one debounce counter.
fn replay_args_fingerprint(name: &str, args: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    args.hash(&mut hasher);
    hasher.finish()
}

impl ChatStateActor {
    /// Repair any dangling tool calls in the conversation and persist the fix.
    ///
    /// A "dangling" tool call is an assistant message with tool call IDs that
    /// lack matching `ToolResult` entries. This can happen when:
    /// - The user cancels (Ctrl+C) mid-tool-execution in a live session
    /// - The process crashes between pushing the assistant and tool results
    /// - The tokio task is aborted at an `.await` point
    ///
    /// This method repairs the state in-place and persists the fix to disk.
    /// It is idempotent — calling it on a clean conversation is a cheap no-op
    /// (single forward scan, no allocations).
    ///
    /// Only call at write boundaries where the previous turn is definitively
    /// over (`ChatState::new()`, `push_user_message()`, `BuildConversationRequest`).
    /// Do NOT call from read handlers — background tasks run concurrently with
    /// tool execution and would misidentify in-flight calls as dangling.
    pub(super) fn ensure_conversation_integrity(&mut self) {
        self.ensure_conversation_integrity_with_reason(DanglingToolCallReason::UserCancelled);
    }

    /// Like [`Self::ensure_conversation_integrity`] but takes an explicit reason.
    pub(super) fn ensure_conversation_integrity_with_reason(
        &mut self,
        reason: DanglingToolCallReason,
    ) {
        // In-place integrity repair can add/remove items ahead of an active capture's
        // boundary, so snapshot + rebase the offset like the replace/restore paths.
        self.snapshot_turn_slice();
        let deduped = dedup_duplicate_tool_results(&mut self.state.conversation);
        if deduped > 0 {
            tracing::info!(
                deduped_count = deduped,
                "Removed duplicate tool results in conversation"
            );
        }
        let repaired = repair_dangling_tool_calls(&mut self.state.conversation, reason);
        if repaired > 0 || deduped > 0 {
            tracing::info!(
                repaired_count = repaired,
                "Repaired dangling tool calls in conversation"
            );
            self.persistence.replace_history(&self.state.conversation);
        }
        self.rebase_turn_capture_offset();
    }

    /// Repair dangling tool calls after a harness-initiated halt.
    pub(super) fn repair_dangling_after_harness_halt(&mut self, class: &'static str) {
        self.ensure_conversation_integrity_with_reason(DanglingToolCallReason::HarnessHalted {
            class,
        });
    }

    /// Out-of-band history repair (`x.ai/session/repair`): run
    /// [`crate::compaction_utils::repair_history`] and persist changes via
    /// [`Self::replace_conversation`]. Unlike
    /// [`Self::ensure_conversation_integrity`], this also removes orphaned
    /// `ToolResult`s — the shape that bricks a session with provider 400s.
    /// `dry_run` only reports.
    pub(super) fn repair_history(
        &mut self,
        dry_run: bool,
    ) -> crate::compaction_utils::HistoryRepairReport {
        if dry_run {
            let mut copy = self.state.conversation.clone();
            return crate::compaction_utils::repair_history(&mut copy);
        }
        let mut items = std::mem::take(&mut self.state.conversation);
        let report = crate::compaction_utils::repair_history(&mut items);
        if report.changed() {
            tracing::warn!(
                duplicates_removed = report.duplicates_removed,
                stripped_tool_result_ids = ?report.stripped_tool_result_ids,
                synthetic_results_inserted = report.synthetic_results_inserted,
                "History repair modified the conversation"
            );
            // Full replace: persists atomically and re-bases token estimates.
            self.replace_conversation(items, false);
        } else {
            // Nothing changed — put the conversation back untouched.
            self.state.conversation = items;
        }
        report
    }

    /// Push any conversation item (user, assistant, or tool result) and persist it.
    pub(super) fn push_message(&mut self, item: ConversationItem) {
        let count_in_delta = !matches!(item, ConversationItem::Assistant(_));
        if count_in_delta {
            let estimated_tokens = super::state::estimate_item_tokens(&item);
            self.state.estimated_tokens_since_model += estimated_tokens;
            tracing::debug!(
                item_kind = item_kind_str(&item),
                estimated_tokens_delta = estimated_tokens,
                estimated_total = self.state.total_tokens + self.state.estimated_tokens_since_model,
                model_reported_total = self.state.total_tokens,
                "ChatState: push_message updated estimated_tokens_since_model"
            );
        }
        self.persistence.persist_message(&item);
        self.state.conversation.push(item);
    }

    /// Push a user message, ensuring conversation integrity first.
    ///
    /// When the user cancels a turn while the model was executing parallel
    /// tool calls, the conversation may have dangling tool call IDs. This
    /// method repairs them before appending the new message so the on-disk
    /// and in-memory state stay consistent.
    ///
    /// Also runs [`prune_retained_conversation`] to eagerly hard-clear very
    /// old tool results from the in-memory state, bounding long-session
    /// retained memory without waiting for the context-window threshold.
    pub(super) fn push_user_message(&mut self, item: ConversationItem) {
        self.push_user_message_with_repair_reason(item, DanglingToolCallReason::UserCancelled);
    }

    /// Like [`Self::push_user_message`] but takes an explicit repair reason.
    pub(super) fn push_user_message_with_repair_reason(
        &mut self,
        item: ConversationItem,
        reason: DanglingToolCallReason,
    ) {
        self.ensure_conversation_integrity_with_reason(reason);
        let estimated_tokens = super::state::estimate_item_tokens(&item);
        self.state.estimated_tokens_since_model += estimated_tokens;
        tracing::debug!(
            item_kind = item_kind_str(&item),
            estimated_tokens_delta = estimated_tokens,
            estimated_total = self.state.total_tokens + self.state.estimated_tokens_since_model,
            model_reported_total = self.state.total_tokens,
            "ChatState: push_user_message updated estimated_tokens_since_model"
        );
        self.persistence.persist_message(&item);
        self.state.conversation.push(item);
        self.prune_retained_conversation();
    }

    /// Eagerly hard-clear tool results from very old turns in the retained
    /// in-memory conversation, freeing the actual string bytes.
    ///
    /// Unlike the API-copy pruning in `build_conversation_request` (which runs
    /// on a *clone* only when context > 50% full), this operates on
    /// `self.state.conversation` directly and runs after every user turn.
    ///
    /// # What this does
    ///
    /// Only **hard-clears** are applied (no soft-trim).  Soft-trimming is a
    /// context-management operation that changes what the model sees;
    /// hard-clearing is a memory-management operation that replaces content
    /// that is so old the model should not need it again.  The threshold is
    /// controlled by `PruningConfig::hard_clear_age_turns`.
    ///
    /// # Retained-memory measurement
    ///
    /// When any clearing occurs, a `tracing::debug!` event reports:
    /// - `hard_cleared` — number of tool results cleared
    /// - `bytes_freed` — approximate bytes recovered (sum of content lengths)
    /// - `conversation_len` — total item count after the pass
    ///
    /// # Synthetic User items and turn-age accuracy
    ///
    /// The shell can inject synthetic `User` items mid-turn (e.g. system
    /// corrective warnings) without calling `increment_prompt_index`.  These
    /// do not represent real user turns.  The backward scan here counts every
    /// `User` item as a turn boundary, so synthetic items would normally cause
    /// old tool results to appear older than they really are.
    ///
    /// This is compensated by raising the effective clearing threshold by the
    /// number of synthetic User items (`total_user_items - prompt_index`).
    /// The result: a tool result is never cleared before `hard_clear_age_turns`
    /// REAL turns have elapsed, even in sessions with many synthetic messages.
    ///
    /// # Replay / rewind correctness
    ///
    /// `updates.jsonl` is **never touched**, so cross-compaction
    /// `replay_to_prompt` is unaffected.  The pruned `chat_history.jsonl`
    /// on disk mirrors the in-memory state — both lose old bulk content but
    /// `updates.jsonl` retains the original data for replay.
    pub(super) fn prune_retained_conversation(&mut self) -> usize {
        if !self.pruning_config.enabled {
            return 0;
        }
        // Phase C batched mode (C-6): route to the low-watermark batch clear.
        // When the flag is off this branch is never taken, so the per-turn
        // path below is byte-identical to the pre-Phase-C build.
        if self.pruning_config.batched_mode() {
            return self.prune_retained_batched();
        }
        // Fast exit: not enough turns have elapsed for any hard-clear to apply.
        if self.state.prompt_index < self.pruning_config.hard_clear_age_turns {
            return 0;
        }

        // Compute how many synthetic User items exist (system reminders, etc.).
        // Synthetic User items are NOT real user turns — they are injected by the
        // shell mid-turn and do not increment `prompt_index`.  The naive backward
        // scan counts every User item as a turn boundary, so synthetic items make
        // old tool results appear older than they really are and can cause
        // premature hard-clears.
        //
        // Fix: raise the effective clearing threshold by the number of synthetic
        // User items.  This guarantees a tool result is never cleared before
        // `hard_clear_age_turns` REAL turns have elapsed, regardless of how many
        // synthetic messages the session contains.
        let total_user_items = self
            .state
            .conversation
            .iter()
            .filter(|i| matches!(i, ConversationItem::User(_)))
            .count();
        let synthetic_count = total_user_items.saturating_sub(self.state.prompt_index);
        let effective_threshold = self
            .pruning_config
            .hard_clear_age_turns
            .saturating_add(synthetic_count);

        let before_bytes = self.conversation_content_bytes();
        let mut cleared = 0usize;
        let mut turn_from_end: usize = 0;
        let mut seen_first_user = false;

        for i in (0..self.state.conversation.len()).rev() {
            if matches!(&self.state.conversation[i], ConversationItem::User(_)) {
                if seen_first_user {
                    turn_from_end += 1;
                }
                seen_first_user = true;
                continue;
            }

            let ConversationItem::ToolResult(tr) = &mut self.state.conversation[i] else {
                continue;
            };

            if turn_from_end < effective_threshold {
                continue;
            }

            if !super::request_builder::is_hard_cleared(tr.content.as_ref()) {
                tr.content =
                    super::request_builder::hard_clear_replacement(tr.content.as_ref());
                cleared += 1;
            }
        }

        if cleared > 0 {
            let after_bytes = self.conversation_content_bytes();
            tracing::debug!(
                hard_cleared = cleared,
                bytes_freed = before_bytes.saturating_sub(after_bytes),
                conversation_len = self.state.conversation.len(),
                "ChatState: in-memory tool-result prune"
            );
            self.persistence.replace_history(&self.state.conversation);
        }

        cleared
    }

    /// Low-watermark batch clear for the batched pruning mode (Phase C, C-1/C-2).
    ///
    /// Unlike the per-turn path, this does **not** hard-clear old tool results
    /// on every turn. It fires a single batch clear only when context
    /// utilization enters `[low_watermark, 50%)` (see
    /// `super::request_builder::should_batch_prune`), clearing every
    /// `ToolResult` aged ≥ `hard_clear_age_turns` in one pass, then closes the
    /// window (`last_batch_clear_at` = current prompt index) so no further
    /// change occurs until utilization drops back below the low watermark and
    /// re-crosses it (C-7 hysteresis). Between windows the retained layer is
    /// inert — the foundation of the C-2 prefix-growth invariant.
    ///
    /// # C-3 exclusions (never batch-cleared)
    /// - tool results in the last `keep_last_n_turns` turns (the first result
    ///   batch after the last real user query);
    /// - `search_tool` tool results (exempt by tool name).
    ///
    /// # Clearing rules
    /// - `ToolResult` content is batch-cleared with the soft-guidance
    ///   placeholder (idempotent across windows; spill pointers preserved).
    /// - `Reasoning` is replaced with a synthesized **visible-text `Assistant`
    ///   sibling** (carrying the `[reasoning summary]` marker) **only when**
    ///   the block is old enough AND its content fingerprint is present in the
    ///   pre-generated `reasoning_summaries` table: encrypted blocks are
    ///   skipped wholesale, and a table miss leaves the block untouched
    ///   (C-1b never-bare-clear). The sibling is never a `Reasoning` item
    ///   (C-1: some providers drop `reasoning_content`).
    fn prune_retained_batched(&mut self) -> usize {
        // Fast exit: not enough turns have elapsed for any clear to apply.
        if self.state.prompt_index < self.pruning_config.hard_clear_age_turns {
            return 0;
        }

        let watermark = self.pruning_config.effective_low_watermark_percent();
        let utilization_percent = super::request_builder::context_utilization_percent(
            self.state.total_tokens,
            self.state.sampling_config.context_window,
        );

        // Below the low watermark (reclaim line): re-arm the window and leave
        // the retained layer untouched. This is the hysteresis de-assert — the
        // window only reopens after a batch clear once utilization drops back
        // under the line (C-7 "触发线/回收线分离").
        if utilization_percent < u64::from(watermark) {
            self.state.last_batch_clear_at = None;
            return 0;
        }
        // Above the high-water mark the API-copy layer owns the conversation;
        // the retained layer stays inert here.
        if !super::request_builder::should_batch_prune(
            self.state.total_tokens,
            self.state.sampling_config.context_window,
            watermark,
        ) {
            return 0;
        }
        // Hysteresis window: a batch clear already fired for this window
        // episode (the utilization drop that would re-arm us has not happened).
        if self.state.last_batch_clear_at.is_some() {
            return 0;
        }

        // Same synthetic-User compensation as the per-turn path: a synthetic
        // User item must not make a tool result look older than it really is.
        let total_user_items = self
            .state
            .conversation
            .iter()
            .filter(|i| matches!(i, ConversationItem::User(_)))
            .count();
        let synthetic_count = total_user_items.saturating_sub(self.state.prompt_index);
        let effective_threshold = self
            .pruning_config
            .hard_clear_age_turns
            .saturating_add(synthetic_count);

        // Tool-call id → (name, args) map so the C-3 `search_tool` exemption
        // and the C-4 replay debounce can look up each result's originating
        // call.
        let mut tool_call_by_id: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new();
        for item in &self.state.conversation {
            if let ConversationItem::Assistant(a) = item {
                for tc in &a.tool_calls {
                    tool_call_by_id.insert(
                        tc.id.to_string(),
                        (tc.name.clone(), tc.arguments.to_string()),
                    );
                }
            }
        }

        let before_bytes = self.conversation_content_bytes();
        let mut cleared = 0usize;
        let mut turn_from_end: usize = 0;
        let mut seen_first_user = false;

        for i in (0..self.state.conversation.len()).rev() {
            if matches!(&self.state.conversation[i], ConversationItem::User(_)) {
                if seen_first_user {
                    turn_from_end += 1;
                }
                seen_first_user = true;
                continue;
            }

            // Reasoning branch (C-1b): the summary channel. Encrypted blocks
            // are never cleared (②); a block that is old enough AND whose
            // fingerprint is present in the pre-generated table is replaced
            // with a **visible-text sibling** — an `Assistant` text item, never
            // a `Reasoning` item (some providers drop `reasoning_content`
            // wholesale, C-1). A table miss leaves the block alone
            // (① never-bare-clear).
            if let ConversationItem::Reasoning(r) = &self.state.conversation[i] {
                if r.encrypted_content.is_some() {
                    continue;
                }
                // R5/B6: symmetric age gate — recent reasoning inside the live
                // window is preserved, using the same `effective_threshold` as
                // the ToolResult branch below.
                if turn_from_end < effective_threshold {
                    continue;
                }
                let text = cf_sampling_types::reasoning_item_text(r);
                let fingerprint = super::reasoning_summary::reasoning_block_fingerprint(&text);
                let Some(summary) = self.state.reasoning_summaries.get(&fingerprint).cloned() else {
                    continue;
                };
                self.state.conversation[i] =
                    ConversationItem::assistant(reasoning_summary_sibling_text(&summary));
                cleared += 1;
                continue;
            }

            // Resolve the scratch facts under an immutable borrow so the C-4
            // debounce bookkeeping below can take `&mut self.state` without a
            // borrow conflict.
            let (tool_call_id, already_cleared, current_content) =
                match &self.state.conversation[i] {
                    ConversationItem::ToolResult(tr) => (
                        tr.tool_call_id.clone(),
                        super::request_builder::is_batch_cleared(tr.content.as_ref()),
                        tr.content.clone(),
                    ),
                    _ => continue,
                };

            // C-3: recent turns (the first result batch after the last real
            // user query) are never batch-cleared.
            if turn_from_end < self.pruning_config.keep_last_n_turns {
                continue;
            }
            if turn_from_end < effective_threshold {
                continue;
            }
            // C-3: search_tool results are exempt by tool name.
            let tool_call = tool_call_by_id.get(tool_call_id.as_str());
            if tool_call.map(|(name, _)| name.as_str())
                == Some(super::request_builder::SEARCH_TOOL_NAME)
            {
                continue;
            }
            // Idempotency: already batch-cleared results are left as-is.
            if already_cleared {
                continue;
            }
            // C-4 debounce: after the same tool call (name+args) has been
            // batch-cleared `REPLAY_PROMPT_MAX_COUNT` times, drop the recovery
            // guidance (keep the terse placeholder) so a repeated re-run does
            // not keep re-injecting the pointer.
            let replay_fp = match tool_call {
                Some((name, args)) => replay_args_fingerprint(name, args),
                None => replay_args_fingerprint("", tool_call_id.as_str()),
            };
            let suppressed = self.state.replay_prompt_suppressed(replay_fp);
            let replacement = if suppressed {
                super::request_builder::batch_clear_suppressed_replacement(current_content.as_ref())
            } else {
                super::request_builder::batch_clear_replacement(current_content.as_ref())
            };
            if let ConversationItem::ToolResult(tr) = &mut self.state.conversation[i] {
                tr.content = replacement;
            }
            cleared += 1;
        }

        self.state.last_batch_clear_at = Some(self.state.prompt_index);

        if cleared > 0 {
            let after_bytes = self.conversation_content_bytes();
            tracing::debug!(
                batch_cleared = cleared,
                bytes_freed = before_bytes.saturating_sub(after_bytes),
                conversation_len = self.state.conversation.len(),
                "ChatState: batched in-memory prune (tool results + reasoning summaries)"
            );
            self.persistence.replace_history(&self.state.conversation);
        }

        cleared
    }

    /// Approximate byte footprint of all string content in the conversation.
    ///
    /// Used for before/after measurement logging when pruning runs.
    /// Sums the byte lengths of all string fields; does not allocate.
    fn conversation_content_bytes(&self) -> usize {
        self.state
            .conversation
            .iter()
            .map(|item| match item {
                ConversationItem::System(s) => s.content.len(),
                ConversationItem::User(u) => u
                    .content
                    .iter()
                    .map(|p| match p {
                        ContentPart::Text { text } => text.len(),
                        ContentPart::Image { url } => url.len(),
                    })
                    .sum::<usize>(),
                ConversationItem::Assistant(a) => a.content.len(),
                ConversationItem::ToolResult(tr) => tr.content.len(),
                ConversationItem::BackendToolCall(b) => b.text_summary().len(),
                ConversationItem::Reasoning(r) => {
                    cf_sampling_types::reasoning_item_text(r).len()
                        + r.encrypted_content.as_deref().map(str::len).unwrap_or(0)
                }
            })
            .sum()
    }

    /// Record accumulated token usage and emit an event.
    pub(super) fn record_token_usage(&mut self, total_tokens: u64) {
        self.state.estimated_tokens_since_model = 0;
        self.state.estimate_at_last_response =
            super::state::estimate_conversation_tokens(&self.state.conversation);
        self.state.total_tokens = total_tokens;
        self.send_event(ChatStateEvent::TokensUpdated { total_tokens });
    }

    /// Stash the per-turn `TokenUsage` from the most recent model response.
    /// No event is emitted — this slot is read on demand at `PromptResponse`
    /// construction time, not pushed to subscribers.
    pub(super) fn record_last_turn_usage(&mut self, usage: cf_sampling_types::TokenUsage) {
        self.state.last_turn_usage = Some(usage);
    }

    pub(super) fn record_model_call_usage(
        &mut self,
        model_id: Option<String>,
        usage: &cf_sampling_types::TokenUsage,
        api_duration_ms: Option<u64>,
        cost_usd_ticks: Option<i64>,
    ) {
        let model_key = match model_id.as_deref() {
            Some(id) if !id.is_empty() => id,
            _ => self.state.sampling_config.model.as_str(),
        }
        .to_owned();
        self.state
            .prompt_usage
            .get_or_insert_default()
            .record_main_loop_call(&model_key, usage, api_duration_ms, cost_usd_ticks);
        self.state.session_usage.record_main_loop_call(
            &model_key,
            usage,
            api_duration_ms,
            cost_usd_ticks,
        );
    }

    pub(super) fn record_subagent_usage(
        &mut self,
        by_model: &[(String, crate::usage::UsageTotals)],
        attribute_to_prompt: bool,
        incomplete: bool,
    ) {
        if by_model.is_empty() && !incomplete {
            return;
        }
        if attribute_to_prompt {
            self.state
                .prompt_usage
                .get_or_insert_default()
                .record_subagent(by_model, incomplete);
        }
        // The session ledger always folds, even when the usage is not
        // attributable to the open prompt (its pin may belong to an earlier
        // prompt). Reporting that gap is the coordinator's sticky flag's job —
        // never mark a different live prompt's ledger.
        self.state
            .session_usage
            .record_subagent(by_model, incomplete);
    }

    pub(super) fn mark_usage_incomplete(&mut self, prompt: bool, session: bool) {
        if prompt {
            self.state
                .prompt_usage
                .get_or_insert_default()
                .mark_incomplete();
        }
        if session {
            self.state.session_usage.mark_incomplete();
        }
    }

    pub(super) fn increment_prompt_index(&mut self) {
        self.state.prompt_usage = None;
        self.state.prompt_index += 1;
        self.send_event(ChatStateEvent::PromptIndexChanged {
            new_index: self.state.prompt_index,
        });
    }

    /// Replace the entire conversation, persist, re-estimate `total_tokens`,
    /// and emit reset + token-update events.
    ///
    /// Compaction replaces carry the provider-side overhead forward as a
    /// *ratio* (`base_estimate × provider_total ÷ estimate_at_last_response`,
    /// capped at the pre-compaction total; `base_estimate` when that estimate is
    /// 0) so the reseed neither springs back nor over-counts (see
    /// `COMPACTION.md`).
    pub(super) fn replace_conversation(
        &mut self,
        items: Vec<ConversationItem>,
        is_compaction: bool,
    ) {
        self.snapshot_turn_slice();
        if is_compaction && let Some(cap) = &mut self.state.turn_capture {
            cap.compaction_occurred = true;
        }
        let pre_replace_total = self.state.total_tokens;
        // `harness_trace_buffer` / `harness_trace_turns` intentionally untouched:
        // the planner/verifier subagents ran, so their sealed trace turns survive
        // a conversation replace (same intent as the `TruncateToPromptIndex` arm).
        self.persistence.replace_history(&items);
        let base_estimate = super::state::estimate_conversation_tokens(&items);
        let mut estimated_tokens =
            if is_compaction && pre_replace_total > 0 && self.state.estimate_at_last_response > 0 {
                let ratio = pre_replace_total as f64 / self.state.estimate_at_last_response as f64;
                (base_estimate as f64 * ratio).round() as u64
            } else {
                base_estimate
            };
        // Compaction must never appear to increase usage.
        if is_compaction && pre_replace_total > 0 {
            estimated_tokens = estimated_tokens.min(pre_replace_total);
        }
        self.state.conversation = items;
        self.state.estimated_tokens_since_model = 0;
        self.state.total_tokens = estimated_tokens;
        self.state.estimate_at_last_response =
            super::state::estimate_conversation_tokens(&self.state.conversation);
        self.rebase_turn_capture_offset();
        self.send_event(ChatStateEvent::ConversationReset {
            new_len: self.state.conversation.len(),
        });
        self.send_event(ChatStateEvent::TokensUpdated {
            total_tokens: estimated_tokens,
        });
    }

    /// Atomically swap the leading `System` message with `prompt` (or insert one
    /// if absent), persisting when changed. Runs inside the actor's command loop
    /// so it serializes with turn pushes — no lost-update race on a mid-turn
    /// reconnect. Returns whether the conversation changed.
    ///
    /// The conversation is cloned (items are `Arc`-backed, so the clone is
    /// shallow) rather than `mem::take`n: `replace_conversation` snapshots the
    /// in-flight turn-capture tail from `state.conversation` before swapping,
    /// so the state must stay intact until then.
    pub(super) fn replace_system_head(&mut self, prompt: &str) -> bool {
        if let Some(ConversationItem::System(sys)) = self.state.conversation.first()
            && crate::conversation_util::canonical_system_prompt_eq(sys.content.as_ref(), prompt)
        {
            return false;
        }
        let mut conversation = self.state.conversation.clone();
        let changed =
            crate::conversation_util::replace_or_insert_system_head(&mut conversation, prompt);
        debug_assert!(changed, "head mismatch must produce a change");
        self.replace_conversation(conversation, false);
        changed
    }

    /// Restore all state fields from a snapshot.
    pub(super) fn restore_snapshot(&mut self, snap: ChatStateSnapshot) {
        self.snapshot_turn_slice();
        // Harness trace buffers are transient (not part of the snapshot) and
        // intentionally survive a restore — see `replace_conversation`.
        self.state.conversation = snap.conversation;
        self.rebase_turn_capture_offset();
        self.state.sampling_config = snap.sampling_config;
        self.state.prompt_index = snap.prompt_index;
        self.state.total_tokens = snap.total_tokens;
        self.state.estimated_tokens_since_model = 0;
        self.state.estimate_at_last_response = if snap.estimate_at_last_response > 0 {
            snap.estimate_at_last_response
        } else {
            super::state::estimate_conversation_tokens(&self.state.conversation)
        };
        self.state.agent_edited_paths = snap.agent_edited_paths;
        self.state.prompt_texts = snap.prompt_texts;
        self.state.stream_start_ms = snap.stream_start_ms;
        self.state.turn_start_ms = snap.turn_start_ms;
        self.state.last_compaction_prompt_index = snap.last_compaction_prompt_index;
        self.state.credentials = snap.credentials;
        // Drop abandoned prompt billing; session ledger is lifetime.
        self.state.prompt_usage = None;
    }

    /// If turn capture is active, append the current turn's tail items into
    /// `pre_replacement_messages` before an in-place mutation shifts or drops them.
    pub(super) fn snapshot_turn_slice(&mut self) {
        if let Some(cap) = &mut self.state.turn_capture {
            cap.pre_replacement_messages
                .extend_from_slice(Self::turn_tail(
                    &self.state.conversation,
                    cap.turn_start_offset,
                ));
        }
    }

    /// Re-base an active turn capture's start offset to the current conversation
    /// length after an in-place mutation, keeping the tail slice valid.
    pub(super) fn rebase_turn_capture_offset(&mut self) {
        if let Some(cap) = &mut self.state.turn_capture {
            cap.turn_start_offset = self.state.conversation.len();
        }
    }

    /// Fail-safe `conversation[offset..]` for turn capture: a capture accounting
    /// slip must never abort the user's session (a raw index here SIGABRT-crashed
    /// a live CLI), so an out-of-range offset yields an empty slice — loud in dev
    /// via `debug_assert!`, with a prod breadcrumb via `error!`.
    pub(super) fn turn_tail(
        conversation: &[ConversationItem],
        offset: usize,
    ) -> &[ConversationItem] {
        debug_assert!(
            offset <= conversation.len(),
            "turn_start_offset {offset} > len {}",
            conversation.len()
        );
        conversation.get(offset..).unwrap_or_else(|| {
            tracing::error!(
                offset,
                len = conversation.len(),
                "turn-capture offset past conversation end; trace tail dropped"
            );
            &[]
        })
    }
}
