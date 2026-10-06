//! 401 attribution callback hook for the sampling client.
//!
//! Every 401 response site can optionally emit an attribution event so
//! a downstream observer can split production 401s into "client sent a
//! stale snapshot bearer that the server rejected" vs. "client sent
//! the live token from its auth source and the server still rejected
//! it" buckets.
//!
//! `cf-sampler` is intentionally decoupled from `cf-shell`
//! (no shell types, no logging crate, no auth-manager dependency). The
//! caller wires an implementation of [`Auth401AttributionCallback`]
//! into [`crate::SamplerConfig::attribution_callback`]; the sampler
//! invokes the callback at each UNAUTHORIZED arm with the bearer that
//! was actually sent on the wire. The implementation is free to join
//! the bearer with whatever live credential source it owns and emit
//! the attribution however it wants.
//!
//! When the callback is `None` (the default), the 401 sites are silent
//! and return the same `SamplingError::Auth` they would otherwise.

use std::sync::Arc;

/// Length of the bearer fragment this crate passes across the
/// attribution-callback boundary. Single source of truth: the
/// constant lives in `cf-auth` so every crate that joins the
/// sent fragment with a live credential slices bearers the
/// same way (tail-12, character-aware).
pub use cf_auth::bearer_fragment::BEARER_SUFFIX_LEN;

/// A logical 401-emitting site inside the sampling client. The string
/// identifier ends up in the consumer field of the attribution event
/// so downstream queries can break down 401s by API path.
///
/// # Scope: sampler endpoints only
///
/// This enum enumerates the six HTTP endpoints owned by
/// `SamplingClient` (chat completions, responses, messages -- each in
/// streaming and non-streaming form). It does *not* cover image
/// generation, video generation, web search, or embedding -- those
/// tools live in `cf-tools`
/// (`crates/codegen/cf-tools/src/implementations/`), have their
/// own HTTP clients that do not flow through `SamplingClient`, and
/// hook into the `cf_tools::ApiKeyProvider` trait rather than
/// this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplingConsumer {
    /// `chat_completion_stream`: OpenAI-compatible streaming OpenAI Chat Completions API.
    ChatCompletionsStream,
    /// `chat_completion`: OpenAI-compatible non-streaming OpenAI Chat Completions API.
    ChatCompletions,
    /// `create_response_stream`: Responses API streaming.
    ResponsesStream,
    /// `create_response`: Responses API non-streaming.
    Responses,
    /// `messages_stream`: Anthropic Messages API streaming.
    MessagesStream,
    /// `messages`: Anthropic Messages API non-streaming.
    Messages,
}

impl SamplingConsumer {
    /// Stable string identifier for this emit site. Callbacks
    /// typically combine this with a fixed prefix (e.g. the client
    /// type) when building the consumer field of the attribution
    /// event.
    pub fn as_endpoint(self) -> &'static str {
        match self {
            Self::ChatCompletionsStream => "chat_completions_stream",
            Self::ChatCompletions => "chat_completions",
            Self::ResponsesStream => "responses_stream",
            Self::Responses => "responses",
            Self::MessagesStream => "messages_stream",
            Self::Messages => "messages",
        }
    }
}

/// Hook invoked by [`crate::SamplingClient`] at every 401 response site.
///
/// Implementations are responsible for joining `sent_bearer_suffix`
/// with whatever live credential source they own (e.g. an auth
/// manager holding the most-recently-refreshed token) and emitting
/// whatever attribution event makes sense for their observability
/// stack.
///
/// Implementations must be cheap to invoke and must not block. They
/// run inside the request's response-handling path and any latency
/// they add is paid by the user-visible 401 error path.
//
// The `Debug` bound is a structural requirement: [`crate::SamplerConfig`]
// derives `Debug` and carries an `Option<Arc<dyn Auth401AttributionCallback>>`
// field, which only compiles when the trait is `Debug`. Do not remove
// the bound when factoring this trait out -- it will break
// `derive(Debug)` on `SamplerConfig`.
pub trait Auth401AttributionCallback: Send + Sync + std::fmt::Debug {
    /// Record a 401 attribution event for one logical 401 response.
    ///
    /// `sent_bearer_suffix` is the [`BEARER_SUFFIX_LEN`]-char **tail**
    /// of the bearer that was actually sent on the wire, captured by
    /// [`crate::SamplingClient`] at send time. The sampler truncates
    /// the bearer to the fragment length **before crossing this trait
    /// boundary** -- the full bearer never leaves
    /// [`crate::SamplingClient`]. This is the scrub-at-the-boundary
    /// invariant: even a misbehaving callback implementation that logs
    /// `sent_bearer_suffix` directly leaks only the fragment, never
    /// the full credential. The fragment is the tail (not the head)
    /// because JWT access tokens share a constant base64 header, so
    /// the head distinguishes nothing and the tail does.
    ///
    /// `None` indicates the request had no bearer header at all
    /// (distinct from "had a bearer that turned out to be stale").
    fn record_401(&self, consumer: SamplingConsumer, sent_bearer_suffix: Option<&str>);
}

/// Shared, cheap-to-clone alias for the attribution callback.
pub type SharedAttributionCallback = Arc<dyn Auth401AttributionCallback>;
