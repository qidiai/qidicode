//! 401 attribution: callback hook + shared helpers for tool HTTP clients.

use std::sync::Arc;

/// Bearer tail length shared across crate boundaries.
/// Single source of truth: [`cf_auth::bearer_fragment`].
pub use cf_auth::bearer_fragment::BEARER_SUFFIX_LEN;

/// Which tool endpoint produced the 401.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolConsumer {
    ImageGen,
    VideoGenStart,
    VideoGenPoll,
    WebSearch,
}

impl ToolConsumer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ImageGen => "ImageGen",
            Self::VideoGenStart => "VideoGen.start",
            Self::VideoGenPoll => "VideoGen.poll",
            Self::WebSearch => "WebSearch",
        }
    }
}

/// 401 attribution callback. Shell wires this to emit telemetry.
pub trait Auth401AttributionCallback: Send + Sync + std::fmt::Debug {
    /// `sent_bearer_suffix` is the bearer's [`BEARER_SUFFIX_LEN`]-char
    /// tail, scrubbed by [`emit_401`] (via
    /// [`cf_auth::bearer_fragment::bearer_suffix`]) before crossing
    /// this boundary. `None` = no bearer was sent.
    fn record_401(&self, consumer: ToolConsumer, sent_bearer_suffix: Option<&str>);
}

/// Shared, cheap-to-clone alias for the attribution callback.
pub type SharedAttributionCallback = Arc<dyn Auth401AttributionCallback>;

/// Record a 401 attribution event if a callback is wired. Scrubs
/// the bearer to its [`BEARER_SUFFIX_LEN`]-char tail before crossing
/// the trait boundary. The shell side compares this fragment against
/// `bearer_suffix` of the held token, so the fragment must be the
/// tail, never the head: JWT access tokens share the same base64
/// header prefix, so a head fragment misclassifies every >12-char
/// bearer as a stale snapshot. The scrub is char-safe (multi-byte
/// bearers slice on a char boundary, no panic).
pub(crate) fn emit_401(
    callback: Option<&SharedAttributionCallback>,
    consumer: ToolConsumer,
    sent_bearer: Option<&str>,
) {
    if let Some(cb) = callback {
        let suffix = sent_bearer.map(cf_auth::bearer_fragment::bearer_suffix);
        cb.record_401(consumer, suffix);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records every `record_401` invocation for assertion.
    #[derive(Debug, Default)]
    struct RecordingCallback {
        calls: std::sync::Mutex<Vec<(ToolConsumer, Option<String>)>>,
    }

    impl Auth401AttributionCallback for RecordingCallback {
        fn record_401(
            &self,
            consumer: ToolConsumer,
            sent_bearer_suffix: Option<&str>,
        ) {
            self.calls
                .lock()
                .unwrap()
                .push((consumer, sent_bearer_suffix.map(|s| s.to_string())));
        }
    }

    /// Cross-boundary direction check: `emit_401` must hand the
    /// callback the bearer's **tail** (last
    /// [`BEARER_SUFFIX_LEN`] chars), not its head. The shell
    /// side compares the fragment against `bearer_suffix` of the
    /// held token, so a head fragment misclassifies every
    /// >12-char bearer as a stale snapshot (JWT access tokens
    /// share the same base64 header prefix, so the head
    /// distinguishes nothing).
    ///
    /// RED pre-fix: `truncate_to_prefix` cut the first 12 chars
    /// (`"live-token-1"`), which never equals the held token's
    /// tail (`"567890abcdef"`).
    #[test]
    fn emit_401_passes_bearer_tail_not_head() {
        let cb = std::sync::Arc::new(RecordingCallback::default());
        let cb_dyn: SharedAttributionCallback = cb.clone();
        emit_401(
            Some(&cb_dyn),
            ToolConsumer::WebSearch,
            Some("live-token-1234567890abcdef"),
        );
        let calls = cb.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, ToolConsumer::WebSearch);
        assert_eq!(
            calls[0].1.as_deref(),
            Some("567890abcdef"),
            "the fragment crossing the trait boundary must be the bearer's 12-char tail, not its head"
        );
    }

    /// `None` bearer (fail-closed: no credential on the wire)
    /// must cross the boundary as `None`, not as a fragment of
    /// something else.
    #[test]
    fn emit_401_passes_none_when_no_bearer() {
        let cb = std::sync::Arc::new(RecordingCallback::default());
        let cb_dyn: SharedAttributionCallback = cb.clone();
        emit_401(Some(&cb_dyn), ToolConsumer::ImageGen, None);
        let calls = cb.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, None);
    }

    /// A bearer no longer than the fragment length is its own
    /// tail and must pass through unchanged.
    #[test]
    fn emit_401_short_bearer_is_its_own_tail() {
        let cb = std::sync::Arc::new(RecordingCallback::default());
        let cb_dyn: SharedAttributionCallback = cb.clone();
        emit_401(Some(&cb_dyn), ToolConsumer::VideoGenStart, Some("abc"));
        let calls = cb.calls.lock().unwrap();
        assert_eq!(calls[0].1.as_deref(), Some("abc"));
    }

    /// Char safety: multi-byte bearers must yield their last 12
    /// chars sliced on char boundaries. Two probes:
    /// (1) 13 fullwidth chars (3 bytes each) -- a byte-truncate
    ///     at 12 lands on a boundary here but yields a garbage
    ///     4-char fragment (RED pre-fix observed `Some("ａａａａ")`);
    /// (2) a mixed-width sequence where byte 12 lands strictly
    ///     inside a 4-byte emoji -- `String::truncate(12)`
    ///     PANICS outright on such input.
    #[test]
    fn emit_401_non_ascii_bearer_does_not_panic() {
        let cb = std::sync::Arc::new(RecordingCallback::default());
        let cb_dyn: SharedAttributionCallback = cb.clone();
        emit_401(
            Some(&cb_dyn),
            ToolConsumer::VideoGenPoll,
            Some("ａａａａａａａａａａａａａ"),
        );
        let calls = cb.calls.lock().unwrap();
        assert_eq!(
            calls[0].1.as_deref(),
            Some("ａａａａａａａａａａａａ"),
            "multi-byte bearers must be sliced on a char boundary"
        );
        drop(calls);

        // 13 chars: 2 fullwidth + 2 emoji (4 bytes each) + 9
        // fullwidth. Byte offset 12 falls inside the second
        // emoji (bytes 10-13), so a byte-truncate panics.
        let mixed = "ａａ😃😃ａａａａａａａａａ";
        let cb2 = std::sync::Arc::new(RecordingCallback::default());
        let cb2_dyn: SharedAttributionCallback = cb2.clone();
        emit_401(Some(&cb2_dyn), ToolConsumer::VideoGenPoll, Some(mixed));
        let calls2 = cb2.calls.lock().unwrap();
        assert_eq!(
            calls2[0].1.as_deref(),
            // last 12 chars: drop the leading fullwidth char
            Some("ａ😃😃ａａａａａａａａａ"),
            "mixed-width bearers must be sliced on a char boundary, not a byte offset"
        );
    }

    #[test]
    fn tool_consumer_as_str_stable_identifiers() {
        assert_eq!(ToolConsumer::ImageGen.as_str(), "ImageGen");
        assert_eq!(ToolConsumer::VideoGenStart.as_str(), "VideoGen.start");
        assert_eq!(ToolConsumer::VideoGenPoll.as_str(), "VideoGen.poll");
        assert_eq!(ToolConsumer::WebSearch.as_str(), "WebSearch");
    }
}
