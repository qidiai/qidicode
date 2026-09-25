pub mod config;
pub mod grok_auth_credentials;
pub mod hooks;

// The foundation utilities live in `qidi-code-base` (upstream of this
// crate so they build in parallel). Re-exported at the original paths so
// existing `crate::util::…` / `cf_shell::util::…` users compile
// unchanged.
pub use cf_shell_base::util::*;

/// Aborts the wrapped tokio task when dropped.
///
/// Use to tie a spawned helper task's lifetime to an async scope so that
/// cancelling the parent future (e.g. a turn abort dropping the tool loop)
/// also tears down the helper instead of leaving it running detached.
/// Aborting an already-finished task is a no-op, so this is safe to hold
/// across normal scope exit too.
pub struct AbortOnDrop(pub tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Idle-refresh gate with a test-only localhost allowance.
///
/// Production behavior is identical to [`cf_shell_base::util::is_cli_chat_proxy_url`].
/// The e2e idle-resume tests bind a mock cli-chat-proxy on 127.0.0.1:0 and
/// need the refresh path reachable; widening the shared predicate would also
/// widen `is_first_party_xai_url` (a `disable_api_key_auth` input), so the
/// seam lives here, compiled out of release builds.
#[cfg(test)]
pub fn is_cli_chat_proxy_url_for_idle_refresh(url: &str) -> bool {
    if cf_shell_base::util::is_cli_chat_proxy_url(url) {
        return true;
    }
    reqwest::Url::parse(url)
        .ok()
        .map(|u| {
            matches!(
                u.host_str(),
                Some("127.0.0.1") | Some("localhost") | Some("::1")
            )
        })
        .unwrap_or(false)
}