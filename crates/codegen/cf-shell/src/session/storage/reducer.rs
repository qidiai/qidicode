//! Shared conversation reducer: folds a `updates.jsonl` stream of session
//! updates into `ConversationItem`s.
//!
//! Extracted from `remote/pull.rs` (its birthplace, where it rehydrates
//! pulled remote sessions) so the same fold can serve local verification:
//! the long-term goal is "chat_history.jsonl is a cache derivable from
//! updates.jsonl + compaction checkpoints + spawn-time system prompt".
//!
//! # Fidelity boundary (lossy by design)
//!
//! The reduction is NOT a byte-exact inverse of the live chat history:
//!
//! - **System / project-instruction items are absent.** The live
//!   conversation's leading `System` item is reinstalled from the current
//!   agent config at every spawn (`install_system_prompt`), so the reducer
//!   never needs to produce it. Consumers comparing against
//!   `chat_history.jsonl` must skip the leading System/project-instruction
//!   prefix.
//! - **Tool-result content is display-grade.** The reducer extracts the
//!   ACP `ToolCallUpdate` display text (`content` text blocks, falling
//!   back to `raw_output`); the live conversation stores the model-facing
//!   result, which can be richer. Making this lossless (persisting the
//!   model-facing result into the update stream) is a planned follow-up.
//! - **Chunk coalescing.** Consecutive same-role text chunks merge into one
//!   item, mirroring the local persistence layer's own merge-on-write.

use std::collections::{HashMap, HashSet};

use agent_client_protocol as acp;

use crate::sampling::{AssistantItem, ContentPart, ConversationItem, ToolCall};
use crate::session::storage::SessionUpdate;

/// Folds [`SessionUpdate`]s into conversation items.
///
/// Turn boundaries: User→Agent flushes user, Agent→User flushes agent,
/// tool completion flushes agent before emitting result.
pub(crate) struct ChatReducer {
    user_parts: Vec<ContentPart>,
    agent_text: String,
    agent_tool_calls: Vec<ToolCall>,

    in_user_turn: bool,
    has_agent_content: bool,
    needs_truncate: bool,

    tool_args: HashMap<String, String>,
    emitted_tool_results: HashSet<String>,
    item_count: usize,
}

impl ChatReducer {
    pub(crate) fn new() -> Self {
        Self {
            user_parts: Vec::new(),
            agent_text: String::new(),
            agent_tool_calls: Vec::new(),
            in_user_turn: false,
            has_agent_content: false,
            needs_truncate: false,
            tool_args: HashMap::new(),
            emitted_tool_results: HashSet::new(),
            item_count: 0,
        }
    }

    pub(crate) fn process(&mut self, update: &SessionUpdate) -> Vec<ConversationItem> {
        match update {
            SessionUpdate::Acp(n) => self.handle_acp(&n.update),
            SessionUpdate::Xai(n) => self.handle_xai(&n.update),
        }
    }

    fn handle_acp(&mut self, update: &acp::SessionUpdate) -> Vec<ConversationItem> {
        match update {
            acp::SessionUpdate::UserMessageChunk(chunk) => self.on_user_chunk(chunk),
            acp::SessionUpdate::AgentMessageChunk(chunk) => self.on_agent_chunk(chunk),
            acp::SessionUpdate::ToolCall(tc) => self.on_tool_call(tc),
            acp::SessionUpdate::ToolCallUpdate(tc) => self.on_tool_call_update(tc),
            _ => Vec::new(), // AgentThoughtChunk, Retry, Plan not needed
        }
    }

    fn handle_xai(
        &mut self,
        update: &crate::extensions::notification::SessionUpdate,
    ) -> Vec<ConversationItem> {
        use crate::extensions::notification::SessionUpdate as XaiUpdate;

        match update {
            XaiUpdate::CompactionCheckpoint(_) => {
                self.reset();
                self.needs_truncate = true;
                Vec::new()
            }
            // Informational-only events (RequestHeader, RewindMarker handled
            // by the replay pipeline, memory/feedback notifications, ...) do
            // not contribute conversation content.
            _ => Vec::new(),
        }
    }

    fn on_user_chunk(&mut self, chunk: &acp::ContentChunk) -> Vec<ConversationItem> {
        let mut out = Vec::new();

        if !self.in_user_turn {
            out.extend(self.flush_agent());
            self.in_user_turn = true;
        }

        match &chunk.content {
            acp::ContentBlock::Text(t) => {
                self.user_parts.push(ContentPart::Text {
                    text: std::sync::Arc::<str>::from(t.text.clone()),
                });
            }
            acp::ContentBlock::Image(img) => {
                if let Some(uri) = &img.uri {
                    self.user_parts.push(ContentPart::Image {
                        url: std::sync::Arc::<str>::from(uri.clone()),
                    });
                }
            }
            _ => {} // Audio, Resource, etc. not needed for chat replay
        }

        out
    }

    fn on_agent_chunk(&mut self, chunk: &acp::ContentChunk) -> Vec<ConversationItem> {
        let mut out = Vec::new();

        if self.in_user_turn {
            out.extend(self.flush_user());
            self.in_user_turn = false;
        }

        if let acp::ContentBlock::Text(t) = &chunk.content {
            self.agent_text.push_str(&t.text);
            self.has_agent_content = true;
        }

        out
    }

    fn on_tool_call(&mut self, tc: &acp::ToolCall) -> Vec<ConversationItem> {
        let id = tc.tool_call_id.0.to_string();
        let args = tc
            .raw_input
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_default();

        self.tool_args.insert(id.clone(), args.clone());
        self.agent_tool_calls.push(ToolCall {
            id: std::sync::Arc::<str>::from(id),
            name: tc.title.clone(),
            arguments: std::sync::Arc::<str>::from(args),
        });

        Vec::new()
    }

    fn on_tool_call_update(&mut self, tc: &acp::ToolCallUpdate) -> Vec<ConversationItem> {
        let id = tc.tool_call_id.0.to_string();
        self.maybe_backfill_args(&id, &tc.fields);

        if Self::is_completed(&tc.fields) && self.emitted_tool_results.insert(id.clone()) {
            return self.emit_tool_result(&id, &tc.fields);
        }
        Vec::new()
    }

    /// Backfill tool arguments from ToolCallUpdate if ToolCall didn't have them.
    fn maybe_backfill_args(&mut self, id: &str, fields: &acp::ToolCallUpdateFields) {
        let Some(raw) = &fields.raw_input else { return };
        let needs_backfill = self.tool_args.get(id).is_none_or(String::is_empty);
        if !needs_backfill {
            return;
        }

        let args = raw.to_string();
        self.tool_args.insert(id.to_string(), args.clone());

        if let Some(call) = self
            .agent_tool_calls
            .iter_mut()
            .find(|c| c.id.as_ref() == id)
        {
            call.arguments = std::sync::Arc::<str>::from(args);
        }
    }

    fn is_completed(fields: &acp::ToolCallUpdateFields) -> bool {
        matches!(
            fields.status,
            Some(acp::ToolCallStatus::Completed | acp::ToolCallStatus::Failed)
        )
    }

    fn emit_tool_result(
        &mut self,
        id: &str,
        fields: &acp::ToolCallUpdateFields,
    ) -> Vec<ConversationItem> {
        let mut out = Vec::new();
        out.extend(self.flush_agent());

        let content = extract_tool_result_text(fields);
        let item = ConversationItem::tool_result(id.to_string(), content);
        self.item_count += 1;
        out.push(item);
        out
    }

    fn flush_user(&mut self) -> Option<ConversationItem> {
        if self.user_parts.is_empty() {
            return None;
        }
        let item = ConversationItem::user_with_parts(std::mem::take(&mut self.user_parts));
        self.item_count += 1;
        Some(item)
    }

    fn flush_agent(&mut self) -> Option<ConversationItem> {
        if !self.has_agent_content && self.agent_tool_calls.is_empty() {
            return None;
        }
        let item = ConversationItem::Assistant(AssistantItem {
            content: std::sync::Arc::<str>::from(std::mem::take(&mut self.agent_text)),
            tool_calls: std::mem::take(&mut self.agent_tool_calls),
            model_id: None,
            model_fingerprint: None,
            reasoning_effort: None,
        });
        self.has_agent_content = false;
        self.item_count += 1;
        Some(item)
    }

    pub(crate) fn flush(&mut self) -> Vec<ConversationItem> {
        let mut out = Vec::new();
        out.extend(self.flush_user());
        out.extend(self.flush_agent());
        out
    }

    fn reset(&mut self) {
        self.user_parts.clear();
        self.agent_text.clear();
        self.agent_tool_calls.clear();
        self.tool_args.clear();
        self.emitted_tool_results.clear();
        self.in_user_turn = false;
        self.has_agent_content = false;
        self.item_count = 0;
    }

    pub(crate) fn should_truncate(&self) -> bool {
        self.needs_truncate
    }

    pub(crate) fn clear_truncate_flag(&mut self) {
        self.needs_truncate = false;
    }

    pub(crate) fn count(&self) -> usize {
        self.item_count
    }
}

/// Extract displayable text from a completed ToolCallUpdate.
pub(crate) fn extract_tool_result_text(
    fields: &agent_client_protocol::ToolCallUpdateFields,
) -> String {
    if let Some(content) = &fields.content {
        let text: String = content
            .iter()
            .filter_map(|c| match c {
                agent_client_protocol::ToolCallContent::Content(
                    agent_client_protocol::Content {
                        content: agent_client_protocol::ContentBlock::Text(t),
                        ..
                    },
                ) => Some(t.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        if !text.is_empty() {
            return text;
        }
    }
    if let Some(raw) = &fields.raw_output {
        return raw.to_string();
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::notification::{
        SessionNotification as XaiNotification, SessionUpdate as XaiSessionUpdate,
    };

    fn user_chunk(text: &str) -> SessionUpdate {
        SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
            acp::SessionId::new("s"),
            acp::SessionUpdate::UserMessageChunk(acp::ContentChunk::new(
                acp::ContentBlock::Text(acp::TextContent::new(text.to_string())),
            )),
        )))
    }

    fn agent_chunk(text: &str) -> SessionUpdate {
        SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
            acp::SessionId::new("s"),
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                acp::ContentBlock::Text(acp::TextContent::new(text.to_string())),
            )),
        )))
    }

    fn tool_call(id: &str, title: &str, raw_input: serde_json::Value) -> SessionUpdate {
        SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
            acp::SessionId::new("s"),
            acp::SessionUpdate::ToolCall(
                acp::ToolCall::new(acp::ToolCallId::new(id), title)
                    .kind(acp::ToolKind::Other)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(raw_input),
            ),
        )))
    }

    fn tool_result(id: &str, text: &str) -> SessionUpdate {
        SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
            acp::SessionId::new("s"),
            acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                acp::ToolCallId::new(id),
                acp::ToolCallUpdateFields::new()
                    .status(Some(acp::ToolCallStatus::Completed))
                    .content(Some(vec![acp::ToolCallContent::from(
                        acp::ContentBlock::Text(acp::TextContent::new(text.to_string())),
                    )])),
            )),
        )))
    }

    fn xai(update: XaiSessionUpdate) -> SessionUpdate {
        SessionUpdate::Xai(Box::new(XaiNotification {
            session_id: acp::SessionId::new("s"),
            update,
            meta: None,
        }))
    }

    fn compaction_checkpoint() -> SessionUpdate {
        xai(XaiSessionUpdate::CompactionCheckpoint(Box::new(
            crate::extensions::notification::CompactionCheckpointInfo {
                checkpoint_id: "cp-1".to_string(),
                prompt_index_at_compaction: 1,
                checkpoint_file: "compaction_checkpoints/cp-1.json".to_string(),
                auto_continue: None,
                schema_version: 1,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )))
    }

    fn request_header(reason: &str) -> SessionUpdate {
        xai(XaiSessionUpdate::RequestHeader {
            system_prompt_sha256: Some("a".repeat(64)),
            tool_names: vec!["bash".to_string()],
            model: Some("m".to_string()),
            temperature: None,
            top_p: None,
            max_output_tokens: None,
            reason: reason.to_string(),
        })
    }

    fn reduce(updates: &[SessionUpdate]) -> (Vec<ConversationItem>, bool) {
        let mut r = ChatReducer::new();
        let mut items = Vec::new();
        for u in updates {
            items.extend(r.process(u));
        }
        items.extend(r.flush());
        let truncated = r.should_truncate();
        (items, truncated)
    }

    #[test]
    fn reduces_user_agent_turn_pair() {
        let (items, truncated) = reduce(&[user_chunk("hi"), agent_chunk("hello")]);
        assert!(!truncated);
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0], ConversationItem::User(_)));
        assert!(matches!(items[1], ConversationItem::Assistant(_)));
    }

    #[test]
    fn merges_consecutive_same_role_chunks() {
        let (items, _) = reduce(&[
            user_chunk("hello "),
            user_chunk("world"),
            agent_chunk("answer"),
        ]);
        assert_eq!(items.len(), 2);
        match &items[0] {
            ConversationItem::User(u) => {
                let text = u
                    .content
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => Some(text.to_string()),
                        _ => None,
                    })
                    .collect::<String>();
                assert_eq!(text, "hello world");
            }
            other => panic!("expected user item, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_pairs_agent_then_result() {
        let (items, _) = reduce(&[
            user_chunk("run it"),
            agent_chunk("doing"),
            tool_call("t1", "bash", serde_json::json!({"cmd": "ls"})),
            tool_result("t1", "file1\nfile2"),
        ]);
        // user, assistant(text+tool_call), tool_result
        assert_eq!(items.len(), 3);
        match &items[1] {
            ConversationItem::Assistant(a) => {
                assert_eq!(a.tool_calls.len(), 1);
                assert_eq!(a.tool_calls[0].id.as_ref(), "t1");
            }
            other => panic!("expected assistant, got {other:?}"),
        }
        match &items[2] {
            ConversationItem::ToolResult(tr) => {
                assert_eq!(tr.tool_call_id.as_str(), "t1");
                assert!(tr.content.as_ref().contains("file1"));
            }
            other => panic!("expected tool result, got {other:?}"),
        }
    }

    #[test]
    fn compaction_checkpoint_resets_and_flags_truncate() {
        // The checkpoint clears PENDING state and sets the truncate flag;
        // items already returned to the caller are NOT retracted — the
        // consumer (rebuild_chat_history) rewinds its output file on the
        // truncate flag. This test pins that division of responsibility.
        let mut r = ChatReducer::new();
        let mut items = Vec::new();
        for u in &[
            user_chunk("old turn"),
            agent_chunk("old answer"),
            compaction_checkpoint(),
            user_chunk("new turn"),
            agent_chunk("new answer"),
        ] {
            items.extend(r.process(u));
        }
        items.extend(r.flush());
        assert!(r.should_truncate(), "checkpoint must set the truncate flag");
        // Items flushed BEFORE the checkpoint stay with the caller ("old
        // turn": flushed when the agent chunk arrived). PENDING state at
        // checkpoint time is dropped ("old answer" was buffered agent text,
        // cleared by reset). The consumer rewinds its output file on the
        // truncate flag, which discards the already-delivered prefix.
        let texts: Vec<String> = items.iter().map(|c| c.text_content()).collect();
        assert_eq!(texts, vec!["old turn", "new turn", "new answer"]);
        // ...but the pending-state reset means a flush right after the
        // checkpoint emits nothing extra, and the count reflects only
        // post-checkpoint items.
        assert_eq!(r.count(), 2);
    }

    #[test]
    fn request_header_is_informational() {
        let (items, truncated) = reduce(&[
            request_header("initial"),
            user_chunk("q"),
            agent_chunk("a"),
            request_header("change"),
        ]);
        assert!(!truncated);
        assert_eq!(items.len(), 2, "envelope events must not derive items");
    }

    #[test]
    fn duplicate_tool_result_updates_emit_once() {
        let (items, _) = reduce(&[
            agent_chunk(""),
            tool_call("t1", "bash", serde_json::json!({})),
            tool_result("t1", "out"),
            tool_result("t1", "out again"),
        ]);
        let results = items
            .iter()
            .filter(|c| matches!(c, ConversationItem::ToolResult(_)))
            .count();
        assert_eq!(results, 1, "repeated completion updates must not duplicate");
    }
}
