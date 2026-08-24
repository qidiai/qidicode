//! Tail checkpoint for fast session replay.
//!
//! `updates.jsonl` is append-only (the only write path is
//! `JsonlStorageAdapter::append_update` → `append_jsonl_line`), so replay can
//! resume from a checkpoint that snapshots the replay state machine at a byte
//! offset instead of re-scanning the whole log:
//!
//! - **live line offsets** — the rewind-filtered (dead-branch-free) set, so a
//!   cold replay slices exactly the live lines out of the mmap instead of
//!   re-scanning, re-peeking and re-filtering hundreds of MB;
//! - **filter state** — `prompt_starts` + the user-run tracker, so a rewind
//!   marker appended *after* the checkpoint still truncates correctly;
//! - **derived counters** — `last_tokens` / `max_event_seq` / `total_live` /
//!   unpaired subagent spawns, so cursor reconnects skip the counter scans.
//!
//! A checkpoint always describes a *prefix* ending on a line boundary. Any
//! file that starts with the same bytes is valid: the reader validates the
//! offset against the mapped length plus an FNV-1a hash of the last 4 KiB of
//! the prefix, then catch-up-feeds only the tail. Appends by binaries that
//! know nothing about checkpoints simply leave the checkpoint stale-but-valid
//! (the catch-up scan covers the gap), and a rejected checkpoint falls back to
//! the full scan — the checkpoint can only ever make replay *faster*, never
//! different.
//!
//! Writes are best-effort and stateless: `append_update` counts bytes since
//! the last refresh and rewrites one every [`TRIGGER_BYTES`] (load the
//! previous checkpoint from disk, feed the tail, write); a full replay of a
//! large session seeds one afterwards so the *next* resume is fast even if
//! the byte trigger never fires.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use super::{
    PreparedReplay, RewindLineClass, SubagentLineEvent, UserRunTurnTracker, classify_rewind_line,
    line_event_id, line_event_seq, line_has_event_id, line_is_available_commands_update,
    line_subagent_event, line_total_tokens,
};

/// Refresh a session's checkpoint after this many bytes of appended updates.
///
/// A stale tail of this size costs a few tens of milliseconds to catch-up
/// scan on the next replay; a refresh costs serializing the (small) state.
/// Sessions that never accumulate this much simply resume with a full scan of
/// a small file — no checkpoint, no loss.
pub(crate) const TRIGGER_BYTES: u64 = 8 * 1024 * 1024;

/// Seed a checkpoint after a full replay of a session at least this big.
/// Below this the full scan is instant and a checkpoint would be pure churn.
pub(crate) const SEED_MIN_BYTES: u64 = 1024 * 1024;

/// How many bytes of the checkpointed prefix are hashed to detect rewrites
/// (the append-only contract makes prefix changes impossible in normal
/// operation; the hash catches manual edits and foreign tools).
const PREFIX_HASH_WINDOW: usize = 4096;

const CHECKPOINT_VERSION: u32 = 1;

/// Checkpoint file name (sibling of `updates.jsonl` in the session dir).
const CHECKPOINT_FILE: &str = "updates_checkpoint.json";

/// Serialized replay state at a byte offset into `updates.jsonl`.
///
/// Field semantics mirror what a full [`super::prepare_replay_lines`] scan
/// would derive, so replay from a checkpoint is byte-for-byte equivalent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct UpdatesCheckpoint {
    pub(crate) version: u32,
    /// Bytes of `updates.jsonl` consumed — always a line boundary (the end of
    /// the last fed line, even when that line was torn mid-write: the healing
    /// append inserts its `\n` exactly here, and the catch-up scan skips the
    /// empty line that follows).
    pub(crate) offset: u64,
    /// FNV-1a hash of `bytes[offset-4096..offset]` (empty window when
    /// `offset == 0`) — cheap prefix-integrity check at load time.
    pub(crate) prefix_hash: u64,
    /// Byte offsets of the live (rewind-filtered) lines in `[0, offset)`,
    /// strictly ascending, ACU-inclusive — the exact set
    /// [`super::filter_rewind_lines`] returns for the prefix.
    pub(crate) live_offsets: Vec<u64>,
    /// Parallel to `live_offsets`: 1 when the line is an
    /// `available_commands_update` (kept on disk, never forwarded, not
    /// counted in `total_live`). Stored so cold replay can skip ACU lines
    /// without re-reading them.
    acu_flags: Vec<u8>,
    /// Indices into `live_offsets` where counted user turns begin — the
    /// truncation targets a rewind marker may refer back to.
    prompt_starts: Vec<usize>,
    /// User-run turn tracker state (see [`UserRunTurnTracker`]).
    tracker: TrackerSnapshot,
    /// Last live line's `_meta.totalTokens` (0 when none) — running token
    /// count restored on resume.
    last_tokens: u64,
    /// Highest `_meta.eventId` counter across live lines, re-seeding the
    /// process-global event counter on resume.
    max_event_seq: Option<u64>,
    /// Count of live non-ACU lines.
    total_live: usize,
    /// Unpaired subagent spawns at this point in the timeline:
    /// `(subagent_id, child_session_id)`, sorted (BTreeMap order).
    pending_subagents: Vec<(String, String)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct TrackerSnapshot {
    seen_marker: bool,
    in_user: bool,
    current_run_pi: Option<usize>,
}

impl TrackerSnapshot {
    fn from_tracker(tracker: &UserRunTurnTracker) -> Self {
        Self {
            seen_marker: tracker.seen_marker,
            in_user: tracker.in_user,
            current_run_pi: tracker.current_run_pi,
        }
    }

    fn into_tracker(self) -> UserRunTurnTracker {
        UserRunTurnTracker {
            seen_marker: self.seen_marker,
            in_user: self.in_user,
            current_run_pi: self.current_run_pi,
        }
    }
}

/// The incremental replay state machine — exactly what
/// [`super::filter_rewind_lines`] + the counter scans in
/// [`super::prepare_replay_lines`] compute, maintained line-by-line.
pub(crate) struct IncrementalReplayState {
    live_offsets: Vec<u64>,
    acu_flags: Vec<u8>,
    prompt_starts: Vec<usize>,
    tracker: UserRunTurnTracker,
    last_tokens: u64,
    max_event_seq: Option<u64>,
    total_live: usize,
    pending_subagents: BTreeMap<String, String>,
    /// Bytes consumed so far (line boundary).
    offset: u64,
}

impl Default for IncrementalReplayState {
    fn default() -> Self {
        Self::new()
    }
}

impl IncrementalReplayState {
    pub(crate) fn new() -> Self {
        Self {
            live_offsets: Vec::new(),
            acu_flags: Vec::new(),
            prompt_starts: Vec::new(),
            tracker: UserRunTurnTracker::new(),
            last_tokens: 0,
            max_event_seq: None,
            total_live: 0,
            pending_subagents: BTreeMap::new(),
            offset: 0,
        }
    }

    fn from_checkpoint(cp: UpdatesCheckpoint) -> Self {
        debug_assert_eq!(cp.live_offsets.len(), cp.acu_flags.len());
        Self {
            live_offsets: cp.live_offsets,
            acu_flags: cp.acu_flags,
            prompt_starts: cp.prompt_starts,
            tracker: cp.tracker.into_tracker(),
            last_tokens: cp.last_tokens,
            max_event_seq: cp.max_event_seq,
            total_live: cp.total_live,
            pending_subagents: cp.pending_subagents.into_iter().collect(),
            offset: cp.offset,
        }
    }

    /// Feed all lines in `contents` after the current offset (typically the
    /// catch-up tail of a mapped file, or the whole file from scratch).
    /// `contents` must be the FULL file contents — a rewind marker truncates
    /// live lines that may live anywhere in the prefix, and the counters are
    /// re-derived over the survivors by slicing them back out of `contents`.
    pub(crate) fn feed(&mut self, contents: &str) {
        if self.offset as usize > contents.len() {
            // The file shrank or was replaced under us (shouldn't happen for
            // an append-only log). Rebuild from scratch rather than feed a
            // misaligned tail.
            tracing::warn!(
                checkpoint_offset = self.offset,
                file_len = contents.len(),
                "replay checkpoint: file shorter than checkpoint offset; rescanning"
            );
            *self = Self::new();
        }
        let mut pos = self.offset as usize;
        let mut had_truncation = false;
        for line in contents[pos..].split('\n') {
            let line_start = pos;
            // +1 for the '\n'; over-counts past a final torn segment but that
            // value is never read (offset is taken from contents.len() below).
            pos += line.len() + 1;
            if line.trim().is_empty() {
                continue;
            }
            match classify_rewind_line(line) {
                RewindLineClass::RewindMarker { target_prompt_index } => {
                    // Mirror filter_rewind_lines exactly: an out-of-range
                    // target keeps everything (truncate(len) is a no-op).
                    let trunc = self
                        .prompt_starts
                        .get(target_prompt_index)
                        .copied()
                        .unwrap_or(self.live_offsets.len());
                    self.live_offsets.truncate(trunc);
                    self.acu_flags.truncate(trunc);
                    self.prompt_starts.truncate(target_prompt_index);
                    self.tracker.on_non_user();
                    had_truncation = true;
                }
                RewindLineClass::UserChunk { prompt_index } => {
                    if self.tracker.on_user_chunk(prompt_index) {
                        self.prompt_starts.push(self.live_offsets.len());
                    }
                    self.push_live(line, line_start as u64);
                }
                RewindLineClass::Other => {
                    self.tracker.on_non_user();
                    self.push_live(line, line_start as u64);
                }
            }
        }
        self.offset = contents.len() as u64;
        // A truncation may have dropped lines whose counter contributions are
        // already baked into the aggregates (last_tokens / max_event_seq /
        // total_live / pending spawns); recompute them over the survivors.
        if had_truncation {
            self.rederive_counters(contents);
        }
    }

    fn push_live(&mut self, line: &str, offset: u64) {
        let is_acu = line_is_available_commands_update(line);
        if !is_acu {
            self.total_live += 1;
        }
        self.acu_flags.push(u8::from(is_acu));
        self.live_offsets.push(offset);
        self.apply_line_counters(line);
    }

    fn apply_line_counters(&mut self, line: &str) {
        if let Some(tokens) = line_total_tokens(line) {
            self.last_tokens = tokens;
        }
        if let Some(seq) = line_event_seq(line) {
            self.max_event_seq = Some(self.max_event_seq.map_or(seq, |m| m.max(seq)));
        }
        match line_subagent_event(line) {
            Some(SubagentLineEvent::Spawned {
                subagent_id,
                child_session_id,
            }) => {
                self.pending_subagents.insert(subagent_id, child_session_id);
            }
            Some(SubagentLineEvent::Finished { subagent_id }) => {
                self.pending_subagents.remove(&subagent_id);
            }
            None => {}
        }
    }

    /// Recompute the derived counters from the surviving live lines. Only
    /// runs after a rewind-marker truncation (rare) — the same values a full
    /// scan would produce over the final filtered set.
    fn rederive_counters(&mut self, contents: &str) {
        self.total_live = 0;
        self.last_tokens = 0;
        self.max_event_seq = None;
        self.pending_subagents.clear();
        let live_offsets = std::mem::take(&mut self.live_offsets);
        for (i, &offset) in live_offsets.iter().enumerate() {
            let line = line_at(contents, offset);
            if self.acu_flags[i] == 0 {
                self.total_live += 1;
            }
            self.apply_line_counters(line);
        }
        self.live_offsets = live_offsets;
    }

    /// Snapshot the state as a checkpoint. `contents` must be the same buffer
    /// the state was fed from (the prefix hash is taken over its bytes).
    pub(crate) fn to_checkpoint(&self, contents: &str) -> UpdatesCheckpoint {
        UpdatesCheckpoint {
            version: CHECKPOINT_VERSION,
            offset: self.offset,
            prefix_hash: prefix_hash(contents, self.offset as usize),
            live_offsets: self.live_offsets.clone(),
            acu_flags: self.acu_flags.clone(),
            prompt_starts: self.prompt_starts.clone(),
            tracker: TrackerSnapshot::from_tracker(&self.tracker),
            last_tokens: self.last_tokens,
            max_event_seq: self.max_event_seq,
            total_live: self.total_live,
            pending_subagents: self
                .pending_subagents
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }
}

/// Take the checkpoint fast path only when it actually pays: the
/// un-checkpointed tail must be a minority of the file. A nearly-fresh or
/// empty checkpoint (`offset` ≈ 0) would make the catch-up feed re-parse
/// almost everything — including the no-rewind short-circuit the full scan
/// gets for free — so the plain full scan is the better deal there.
/// Returns `None` = caller should full-scan.
pub(crate) fn build_prepared_replay_if_faster<'a>(
    cp: &UpdatesCheckpoint,
    contents: &'a str,
    cursor: Option<&str>,
) -> Option<PreparedReplay<'a>> {
    let len = contents.len() as u64;
    let tail = len - cp.offset.min(len);
    if tail * 2 > len {
        return None;
    }
    Some(build_prepared_replay(contents, cursor, cp))
}

/// Build a [`PreparedReplay`] from a validated checkpoint + the mapped file
/// contents: restore the state, catch-up-feed the tail, then resolve the
/// cursor and slice out the lines to forward — the same output
/// [`super::prepare_replay_lines`] full scan produces.
pub(crate) fn build_prepared_replay<'a>(
    contents: &'a str,
    cursor: Option<&str>,
    cp: &UpdatesCheckpoint,
) -> PreparedReplay<'a> {
    let mut state = IncrementalReplayState::from_checkpoint(cp.clone());
    state.feed(contents);

    // Resolve the reconnect cursor against the ACU-inclusive live set — same
    // resolution and refusal rules as the full-scan path (see
    // prepare_replay_lines for the rationale).
    let cursor_pos = cursor
        .and_then(|id| {
            (0..state.live_offsets.len())
                .rev()
                .find(|&i| line_has_event_id(line_at(contents, state.live_offsets[i]), id))
        })
        .filter(|&pos| {
            let bounded = state.live_offsets[pos + 1..].iter().enumerate().all(
                |(rel, &offset)| {
                    let i = pos + 1 + rel;
                    state.acu_flags[i] == 1
                        || line_event_id(line_at(contents, offset)).is_some()
                },
            );
            if !bounded {
                tracing::warn!(
                    "replay: post-cursor tail contains eventId-less lines; full replay instead"
                );
            }
            bounded
        });
    let mark_replay = cursor_pos.is_none();
    let start = cursor_pos.map_or(0, |pos| pos + 1);

    let mut lines = Vec::with_capacity(state.live_offsets.len().saturating_sub(start));
    for i in start..state.live_offsets.len() {
        if state.acu_flags[i] == 0 {
            lines.push(line_at(contents, state.live_offsets[i]));
        }
    }

    PreparedReplay {
        lines,
        mark_replay,
        last_tokens: state.last_tokens,
        max_event_seq: state.max_event_seq,
        total_live: state.total_live,
        unfinished_subagents: state.pending_subagents.into_iter().collect(),
        used_checkpoint: true,
    }
}

// ── load / validate / write ────────────────────────────────────────────────

pub(crate) fn checkpoint_path(updates_path: &Path) -> PathBuf {
    updates_path.with_file_name(CHECKPOINT_FILE)
}

/// Structural + prefix-integrity validation against the mapped contents.
/// Rejecting here simply falls back to a full scan.
pub(crate) fn validate(cp: &UpdatesCheckpoint, contents: &str) -> bool {
    cp.version == CHECKPOINT_VERSION
        && cp.offset <= contents.len() as u64
        && cp.live_offsets.len() == cp.acu_flags.len()
        && cp.acu_flags.iter().all(|&f| f <= 1)
        // Live lines start strictly inside the consumed prefix.
        && cp.live_offsets.iter().all(|&o| o < cp.offset)
        && cp.live_offsets.windows(2).all(|w| w[0] < w[1])
        && cp.prompt_starts.iter().all(|&p| p <= cp.live_offsets.len())
        && cp.prefix_hash == prefix_hash(contents, cp.offset as usize)
}

/// Load and validate a checkpoint for the mapped `contents`.
/// `None` = no usable checkpoint (missing, malformed, or stale) — caller
/// falls back to the full scan.
pub(crate) fn load_validated_checkpoint(
    updates_path: &Path,
    contents: &str,
) -> Option<UpdatesCheckpoint> {
    let cp = load_checkpoint(updates_path)?;
    if validate(&cp, contents) {
        Some(cp)
    } else {
        tracing::debug!(
            path = %checkpoint_path(updates_path).display(),
            "replay checkpoint present but invalid; full scan instead"
        );
        None
    }
}

fn load_checkpoint(updates_path: &Path) -> Option<UpdatesCheckpoint> {
    let bytes = std::fs::read(checkpoint_path(updates_path)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_checkpoint(updates_path: &Path, cp: &UpdatesCheckpoint) -> io::Result<()> {
    static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = checkpoint_path(updates_path);
    // Unique tmp name: the persistence actor and a concurrent replay seed can
    // both write a checkpoint for the same session; a shared tmp name would
    // let one rename the other's partial write into place. tmp + rename keeps
    // the published file always a complete document, and any valid prefix
    // checkpoint is correct even when two writers race (last rename wins).
    let n = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_file_name(format!(
        "{CHECKPOINT_FILE}.tmp-{}-{n}",
        std::process::id()
    ));
    let bytes =
        serde_json::to_vec(cp).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, &path)
}

/// Best-effort refresh of a session's checkpoint: map the current file, load
/// the previous checkpoint (full scan when none/invalid), catch-up-feed the
/// tail, write the result. Called from the append path every
/// [`TRIGGER_BYTES`] bytes.
pub(crate) fn refresh_checkpoint(updates_path: &Path) -> io::Result<()> {
    let Some(view) = super::jsonl_mmap::JsonlMmapView::open(updates_path)? else {
        return Ok(()); // no updates file (fresh session) — nothing to checkpoint
    };
    let Some(contents) = view.as_str() else {
        return Ok(()); // non-UTF-8 log — leave any existing checkpoint alone
    };
    let mut state = match load_checkpoint(updates_path) {
        Some(cp) if validate(&cp, contents) => IncrementalReplayState::from_checkpoint(cp),
        _ => IncrementalReplayState::new(),
    };
    state.feed(contents);
    write_checkpoint(updates_path, &state.to_checkpoint(contents))
}

/// Seed a checkpoint for a session that has none (legacy sessions predating
/// this feature): one incremental pass over the already-mapped contents, then
/// write. Called after a full replay so the *next* resume skips the scan.
pub(crate) fn seed_checkpoint(updates_path: &Path, contents: &str) -> io::Result<()> {
    let mut state = IncrementalReplayState::new();
    state.feed(contents);
    write_checkpoint(updates_path, &state.to_checkpoint(contents))
}

// ── helpers ────────────────────────────────────────────────────────────────

/// The line starting at byte `offset` in `contents` (up to the next `\n` or
/// EOF). Offsets come from a validated checkpoint, so they land on line
/// starts; the `min` clamp is pure paranoia against a corrupted file.
fn line_at<'a>(contents: &'a str, offset: u64) -> &'a str {
    let start = (offset as usize).min(contents.len());
    let rest = &contents[start..];
    match rest.find('\n') {
        Some(rel) => &rest[..rel],
        None => rest,
    }
}

/// FNV-1a over the last [`PREFIX_HASH_WINDOW`] bytes of the prefix.
fn prefix_hash(contents: &str, offset: usize) -> u64 {
    let start = offset.saturating_sub(PREFIX_HASH_WINDOW);
    fnv1a64(&contents.as_bytes()[start..offset])
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test-line builders mirroring the on-disk envelope format (same as the
    // prepare_replay_lines tests in storage/mod.rs).
    fn acp(update: &str, meta: &str) -> String {
        format!(r#"{{"timestamp":1,"method":"session/update","params":{{"sessionId":"s","update":{update},"_meta":{meta}}}}}"#)
    }
    fn acp_no_meta(update: &str) -> String {
        format!(r#"{{"timestamp":1,"method":"session/update","params":{{"sessionId":"s","update":{update}}}}}"#)
    }
    fn xai(update: &str, meta: &str) -> String {
        format!(r#"{{"timestamp":1,"method":"_x.ai/session/update","params":{{"sessionId":"s","update":{update},"_meta":{meta}}}}}"#)
    }
    fn user(text: &str, pi: Option<u64>, ev: Option<&str>) -> String {
        let pi_json = match pi {
            Some(p) => format!(r#","promptIndex":{p}"#),
            None => String::new(),
        };
        let ev_json = match ev {
            Some(e) => format!(r#","eventId":"{e}""#),
            None => String::new(),
        };
        acp(
            &format!(r#"{{"sessionUpdate":"user_message_chunk","content":{{"type":"text","text":"{text}"}}}}"#),
            &format!(r#"{{"promptIndex_dummy":0{pi_json}{ev_json}}}"#),
        )
        .replace(r#""promptIndex_dummy":0,"#, r#""#)
        .replace(r#""promptIndex_dummy":0"#, r#""#)
    }
    fn agent(text: &str, ev: Option<&str>, tokens: Option<u64>) -> String {
        let mut meta = String::new();
        if let Some(e) = ev {
            meta.push_str(&format!(r#""eventId":"{e}""#));
        }
        if let Some(t) = tokens {
            if !meta.is_empty() {
                meta.push(',');
            }
            meta.push_str(&format!(r#""totalTokens":{t}"#));
        }
        acp(
            &format!(r#"{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"{text}"}}}}"#),
            &format!("{{{meta}}}"),
        )
    }
    fn acu(ev: Option<&str>) -> String {
        match ev {
            Some(e) => acp(
                r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#,
                &format!(r#"{{"eventId":"{e}"}}"#),
            ),
            None => acp_no_meta(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#),
        }
    }
    fn rewind(target: usize) -> String {
        xai(
            &format!(r#"{{"sessionUpdate":"rewind_marker","target_prompt_index":{target},"created_at":"2024-01-01"}}"#),
            "{}",
        )
    }
    fn subagent_spawned(id: &str, child: &str, ev: Option<&str>) -> String {
        let ev_json = match ev {
            Some(e) => format!(r#","eventId":"{e}""#),
            None => String::new(),
        };
        xai(
            &format!(r#"{{"sessionUpdate":"subagent_spawned","subagent_id":"{id}","child_session_id":"{child}","agent_type":"explore"}}"#),
            &format!(r#"{{"x":1{ev_json}}}"#),
        )
        .replace(r#""x":1,"#, r#""#)
        .replace(r#""x":1"#, r#""#)
    }
    fn subagent_finished(id: &str, ev: Option<&str>) -> String {
        let ev_json = match ev {
            Some(e) => format!(r#","eventId":"{e}""#),
            None => String::new(),
        };
        xai(
            &format!(r#"{{"sessionUpdate":"subagent_finished","subagent_id":"{id}","reason":"done"}}"#),
            &format!(r#"{{"x":1{ev_json}}}"#),
        )
        .replace(r#""x":1,"#, r#""#)
        .replace(r#""x":1"#, r#""#)
    }

    /// A synthetic history exercising every line class the state machine and
    /// the full-scan filters must agree on: marked/unmarked user chunks,
    /// agents with tokens/eventIds, ACUs with/without ids, subagent spawn
    /// pairs, and rewinds (including an out-of-range target).
    fn sample_history() -> Vec<String> {
        vec![
            user("p0", None, Some("s-1")),            // unmarked turn 1
            agent("r0", Some("s-2"), Some(10)),
            user("p1", Some(1), Some("s-3")),         // marked turn 2
            agent("r1", Some("s-4"), None),
            acu(Some("s-5")),
            subagent_spawned("sub1", "child1", Some("s-6")),
            agent("r1b", Some("s-7"), Some(20)),
            subagent_finished("sub1", Some("s-8")),
            rewind(1),                                 // kills p1..r1b (turn 2 branch)
            user("p1-retry", Some(1), Some("s-9")),    // re-opens turn 2
            agent("r1-retry", Some("s-10"), Some(30)),
            acu(None),
            user("p2", Some(2), Some("s-11")),
            subagent_spawned("sub2", "child2", Some("s-12")),
            agent("r2", Some("s-13"), Some(40)),       // sub2 never finishes
            rewind(99),                                // out-of-range: no-op
            user("p3", Some(3), Some("s-14")),
            agent("r3", Some("s-15"), Some(50)),
        ]
    }

    fn contents_of(lines: &[String]) -> String {
        let mut s = String::new();
        for l in lines {
            s.push_str(l);
            s.push('\n');
        }
        s
    }

    /// Differential oracle: replaying from a checkpoint taken at ANY split
    /// point must produce exactly what a full scan produces.
    #[test]
    fn checkpoint_replay_matches_full_scan_at_every_split() {
        let lines = sample_history();
        let contents = contents_of(&lines);
        let full = super::super::prepare_replay_lines(&contents, None, None);

        // Split after each line boundary (offset of each line start + its
        // length + 1, i.e. every prefix of whole lines).
        let mut split = 0usize;
        for line in &lines {
            split += line.len() + 1;
            let mut state = IncrementalReplayState::new();
            // Feed the prefix in two chunks (first line, then the rest of the
            // prefix) to also exercise offset resume across feed() calls.
            // feed() must be called at line boundaries.
            state.feed(&contents[..lines[0].len() + 1]);
            state.feed(&contents[..split]);
            let cp = state.to_checkpoint(&contents[..split]);
            assert!(
                validate(&cp, &contents),
                "checkpoint at offset {split} must validate"
            );

            let prepared = build_prepared_replay(&contents, None, &cp);
            assert_eq!(
                prepared.lines, full.lines,
                "cold replay lines diverge at split {split}"
            );
            assert_eq!(prepared.mark_replay, full.mark_replay);
            assert_eq!(prepared.last_tokens, full.last_tokens, "tokens at {split}");
            assert_eq!(
                prepared.max_event_seq, full.max_event_seq,
                "max_event_seq at {split}"
            );
            assert_eq!(prepared.total_live, full.total_live, "total_live at {split}");
            assert_eq!(
                prepared.unfinished_subagents, full.unfinished_subagents,
                "unfinished_subagents at {split}"
            );
        }
    }

    /// The same differential oracle over the cursor paths: every eventId in
    /// the history used as a reconnect cursor.
    #[test]
    fn checkpoint_cursor_replay_matches_full_scan() {
        let lines = sample_history();
        let contents = contents_of(&lines);
        let event_ids = [
            "s-1", "s-2", "s-3", "s-4", "s-5", "s-6", "s-7", "s-8", "s-9", "s-10", "s-11",
            "s-12", "s-13", "s-14", "s-15",
        ];
        for cursor in event_ids.iter().map(Some).chain(std::iter::once(None)) {
            let cursor = cursor.map(|v| &**v);
            let full = super::super::prepare_replay_lines(&contents, cursor, None);
            // Checkpoints at two line-boundary split points: after line 4
            // (mid-timeline) and after line 12 (after the second rewind).
            let mut line_end = 0usize;
            let mut split_points = Vec::new();
            for (i, l) in lines.iter().enumerate() {
                line_end += l.len() + 1;
                if i == 3 || i == 11 {
                    split_points.push(line_end);
                }
            }
            for split in split_points {
                let mut state = IncrementalReplayState::new();
                state.feed(&contents[..split]);
                let cp = state.to_checkpoint(&contents[..split]);
                let prepared = build_prepared_replay(&contents, cursor, &cp);
                assert_eq!(
                    prepared.lines, full.lines,
                    "cursor {cursor:?} split {split}: lines diverge"
                );
                assert_eq!(
                    prepared.mark_replay, full.mark_replay,
                    "cursor {cursor:?} split {split}: mark_replay diverges"
                );
                assert_eq!(
                    prepared.last_tokens, full.last_tokens,
                    "cursor {cursor:?} split {split}: last_tokens diverges"
                );
                assert_eq!(
                    prepared.max_event_seq, full.max_event_seq,
                    "cursor {cursor:?} split {split}: max_event_seq diverges"
                );
                assert_eq!(
                    prepared.total_live, full.total_live,
                    "cursor {cursor:?} split {split}: total_live diverges"
                );
                assert_eq!(
                    prepared.unfinished_subagents, full.unfinished_subagents,
                    "cursor {cursor:?} split {split}: unfinished_subagents diverge"
                );
            }
        }
    }

    /// A checkpoint whose prefix was rewritten (hash mismatch) or truncated
    /// (offset beyond EOF) must be rejected — full scan fallback.
    #[test]
    fn invalid_checkpoints_are_rejected() {
        let lines = sample_history();
        let contents = contents_of(&lines);
        let mut state = IncrementalReplayState::new();
        state.feed(&contents);
        let cp = state.to_checkpoint(&contents);
        assert!(validate(&cp, &contents));

        // Rewritten prefix: same length, different bytes.
        let mut rewritten = contents.clone();
        rewritten.replace_range(0..1, "X");
        assert!(!validate(&cp, &rewritten), "hash must detect rewrites");

        // Shrunk file.
        assert!(!validate(&cp, &contents[..contents.len() / 2]));

        // Structural corruption: flag/offset length mismatch.
        let mut bad = cp.clone();
        bad.acu_flags.truncate(1);
        assert!(!validate(&bad, &contents));

        // Wrong version.
        let mut old = cp.clone();
        old.version = 0;
        assert!(!validate(&old, &contents));
    }

    /// Torn-tail healing: a checkpoint taken at a torn line boundary stays
    /// valid after the healing append inserts `\n` at the offset.
    #[test]
    fn torn_tail_checkpoint_survives_healing_append() {
        let l1 = user("p0", None, Some("e1"));
        let l2 = agent("r0", Some("e2"), Some(5));
        let torn = format!("{l1}\n{l2}"); // no trailing \n
        let mut state = IncrementalReplayState::new();
        state.feed(&torn);
        let cp = state.to_checkpoint(&torn);
        assert_eq!(cp.offset as usize, torn.len());

        // Healing append: "\n" + new line.
        let healed = format!("{torn}\n{}\n", user("p1", Some(1), Some("e3")));
        assert!(
            validate(&cp, &healed),
            "healed file must still validate the pre-heal checkpoint"
        );
        let prepared = build_prepared_replay(&healed, None, &cp);
        let full = super::super::prepare_replay_lines(&healed, None, None);
        assert_eq!(prepared.lines, full.lines);
        assert_eq!(prepared.total_live, full.total_live);
    }

    /// Round-trip: state → checkpoint → JSON → state → continue feeding
    /// equals never checkpointing at all.
    #[test]
    fn checkpoint_json_round_trip() {
        let lines = sample_history();
        let contents = contents_of(&lines);
        // A line boundary (after line 8): checkpoint offsets are always line
        // boundaries in production — a torn-tail offset is healed with a `\n`
        // at exactly that byte by the next append before anything else lands.
        let split = lines.iter().take(8).map(|l| l.len() + 1).sum::<usize>();

        let mut st = IncrementalReplayState::new();
        st.feed(&contents[..split]);
        let cp = st.to_checkpoint(&contents[..split]);
        let json = serde_json::to_vec(&cp).unwrap();
        let cp2: UpdatesCheckpoint = serde_json::from_slice(&json).unwrap();
        assert_eq!(cp2.offset, cp.offset);
        assert_eq!(cp2.live_offsets, cp.live_offsets);

        let mut st2 = IncrementalReplayState::from_checkpoint(cp2);
        st2.feed(&contents);
        let direct = {
            let mut d = IncrementalReplayState::new();
            d.feed(&contents);
            d.to_checkpoint(&contents)
        };
        let via_cp = st2.to_checkpoint(&contents);
        assert_eq!(via_cp.live_offsets, direct.live_offsets);
        assert_eq!(via_cp.prompt_starts, direct.prompt_starts);
        assert_eq!(via_cp.acu_flags, direct.acu_flags);
        assert_eq!(via_cp.last_tokens, direct.last_tokens);
        assert_eq!(via_cp.max_event_seq, direct.max_event_seq);
        assert_eq!(via_cp.total_live, direct.total_live);
        assert_eq!(via_cp.pending_subagents, direct.pending_subagents);
    }

    /// File IO: seed/refresh/load through a temp session dir, including the
    /// stale-checkpoint catch-up (append without refreshing, then refresh).
    #[test]
    fn seed_refresh_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "replay-cp-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let updates_path = dir.join("updates.jsonl");

        let l1 = user("p0", None, Some("e1"));
        let l2 = agent("r0", Some("e2"), Some(5));
        std::fs::write(&updates_path, format!("{l1}\n{l2}\n")).unwrap();

        // Seed from a fresh scan.
        seed_checkpoint(&updates_path, &format!("{l1}\n{l2}\n")).unwrap();
        let cp = load_checkpoint(&updates_path).expect("seeded checkpoint");
        assert_eq!(cp.offset as usize, l1.len() + 1 + l2.len() + 1);
        assert_eq!(cp.total_live, 2);

        // Append more (simulating an old binary that ignores checkpoints),
        // then refresh — the stale checkpoint must catch up.
        let l3 = user("p1", Some(1), Some("e3"));
        let l4 = rewind(0);
        let l5 = agent("r1", Some("e4"), Some(9));
        let grown = format!("{l1}\n{l2}\n{l3}\n{l4}\n{l5}\n");
        std::fs::write(&updates_path, &grown).unwrap();
        refresh_checkpoint(&updates_path).unwrap();

        let cp2 = load_checkpoint(&updates_path).expect("refreshed checkpoint");
        assert_eq!(cp2.offset as usize, grown.len());
        // rewind(0) killed p0/r0/p1; only r1 survives.
        assert_eq!(cp2.total_live, 1);
        assert_eq!(cp2.last_tokens, 9);

        let loaded = load_validated_checkpoint(&updates_path, &grown).expect("valid");
        let prepared = build_prepared_replay(&grown, None, &loaded);
        let full = super::super::prepare_replay_lines(&grown, None, None);
        assert_eq!(prepared.lines, full.lines);
        assert_eq!(prepared.last_tokens, full.last_tokens);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `total_live` excludes ACUs; ACU lines still carry eventId for cursor
    /// resolution (the idle-client cursor is usually the last ACU).
    #[test]
    fn acu_lines_counted_out_but_cursor_resolvable() {
        let l1 = user("p0", None, Some("e1"));
        let l2 = acu(Some("e2"));
        let contents = format!("{l1}\n{l2}\n");
        let mut st = IncrementalReplayState::new();
        st.feed(&contents);
        let cp = st.to_checkpoint(&contents);
        assert_eq!(cp.total_live, 1);

        let prepared = build_prepared_replay(&contents, Some("e2"), &cp);
        assert!(!prepared.mark_replay, "ACU eventId must resolve the cursor");
        assert!(prepared.lines.is_empty(), "ACU never forwarded");
    }
}
