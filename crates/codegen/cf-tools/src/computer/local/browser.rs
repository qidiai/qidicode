//! Local browser backend driving a Chromium-family browser over CDP.
//!
//! Third member of the `computer` family (after `LocalFs` and
//! `LocalTerminalBackend`): a lazily-launched, headless Edge/Chrome/Chromium
//! instance owned by this process. The executable is resolved by
//! chromiumoxide's detection (`CHROME` env var, then PATH / registry /
//! install paths, Edge included), so hosts rarely need to configure anything.
//!
//! Interaction model: snapshot lists interactive elements as numbered refs
//! (stored in a page-bound JS map); `click`/`type` accept either a ref number
//! or a CSS selector. Click/typing are dispatched via the element API over
//! CDP, which routes through native input synthesis.

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use tokio::sync::{Mutex, OnceCell};

use crate::computer::types::{
    BrowserActionResult, BrowserBackend, BrowserClickRequest, BrowserElementRef,
    BrowserNavigateRequest, BrowserNavigateResult, BrowserReadRequest, BrowserReadResult,
    BrowserSnapshotResult, BrowserTypeRequest, ComputerError,
};

/// Extra settle time after the `load` event before a navigation returns.
const DEFAULT_SETTLE_MS: u64 = 300;
/// Upper bound for waiting on `document.readyState` after `goto`.
const READY_TIMEOUT_MS: u64 = 20_000;
/// Cap for the markdown body a snapshot/read produces, before the tool-level
/// truncation config gets a chance to cut further.
const MAX_CONTENT_CHARS: usize = 40_000;
/// Cap for the interactive-element listing.
const MAX_REFS: usize = 400;

/// Extra attempts for a CDP call that lost a race with a navigation's
/// execution-context swap (see [`retry_stale_context`]).
const CONTEXT_RETRY_LIMIT: usize = 2;
/// First backoff step; doubles each retry (25ms, 50ms).
const CONTEXT_RETRY_BASE_DELAY_MS: u64 = 25;

/// Whether an error is Chromium's "execution context is gone" signal.
///
/// A cross-document navigation destroys the old JS execution context and
/// registers the new one asynchronously. The first `Runtime.evaluate` after
/// `goto` can therefore hit the retired context and fail with
/// `Cannot find context with specified id`, even though the page itself
/// loaded and the very next attempt succeeds.
fn is_stale_context_error(err: &ComputerError) -> bool {
    let msg = err.to_string();
    msg.contains("Cannot find context with specified id")
        || msg.contains("Cannot find context with specified ID")
}

/// Run `op`, retrying **only** stale-execution-context failures.
///
/// Bounded at [`CONTEXT_RETRY_LIMIT`] retries with exponential backoff
/// (25ms, 50ms). Every other error surfaces on the first attempt, so
/// unrelated failures keep their current behaviour. The CDP calls wrapped
/// here are either reads or evaluate calls that did not run (the context was
/// already gone when Chromium rejected them), so a retry cannot duplicate a
/// side effect.
async fn retry_stale_context<T, F, Fut>(mut op: F) -> Result<T, ComputerError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, ComputerError>>,
{
    let mut attempt = 0usize;
    loop {
        match op().await {
            Ok(value) => return Ok(value),
            Err(err) if attempt < CONTEXT_RETRY_LIMIT && is_stale_context_error(&err) => {
                let delay = CONTEXT_RETRY_BASE_DELAY_MS << attempt;
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            Err(err) => return Err(err),
        }
    }
}

struct Inner {
    /// Keep the browser handle alive for the process lifetime.
    _browser: chromiumoxide::Browser,
    /// Owned profile dir. The default backend lives in a static OnceCell,
    /// which Rust never drops: on exit this temp profile dir is left to OS
    /// temp cleanup (tracked as F9.1). The Edge process tree is enrolled in
    /// the `ProcessGroup` held by `_group` below (best-effort coverage -- see
    /// `launch`).
    _user_data: tempfile::TempDir,
    /// Owns the Job Object / process group that reaps the Edge tree.
    /// MUST be kept alive for the process lifetime: `ProcessScope::register`
    /// stores only a `Weak`, so dropping this `Arc` would turn the exit-path
    /// `kill_all` into a silent no-op.
    _group: Option<std::sync::Arc<crate::util::ProcessGroup>>,
    page: chromiumoxide::Page,
}

/// Result of the snapshot-ref lookup evaluated in the page.
///
/// Distinguishes the three failure shapes so the model gets an actionable
/// message instead of a blanket "stale": refs can be absent because the page
/// was never snapshotted (`no_snapshot`), because the number is not in the
/// current snapshot (`missing` -- typically renumbered by a later snapshot),
/// or because the mapped element no longer exists.
#[derive(Debug, serde::Deserialize)]
struct RefLookup {
    state: String,
    #[serde(default)]
    css: Option<String>,
}

/// Browser backend bound to a lazily-launched local Chromium-family process.
pub struct LocalBrowserBackend {
    inner: OnceCell<Inner>,
    /// Reaper for the launched browser tree. The Edge child is enrolled here
    /// at launch (Windows: Job Object) so the TUI exit paths can `kill_all()`
    /// it. Defaults to the process-global scope; tests inject their own to
    /// avoid latching the global.
    scope: crate::util::ProcessScope,
    /// Serializes snapshot/click/type sequences so a stale ref map can never
    /// race a concurrent snapshot.
    op_lock: Mutex<()>,
}

impl Default for LocalBrowserBackend {
    fn default() -> Self {
        Self {
            inner: OnceCell::new(),
            // The process-global scope is what cf-pager's exit paths
            // `kill_all()`; `ProcessScope::default()` would be an unrelated
            // empty scope and would leave the Edge tree unreaped.
            scope: crate::util::global_process_scope().clone(),
            op_lock: Mutex::new(()),
        }
    }
}

impl LocalBrowserBackend {
    pub fn new() -> Self {
        Self::default()
    }

    async fn page(&self) -> Result<&chromiumoxide::Page, ComputerError> {
        // Clone the scope before entering the `OnceCell` initializer: the
        // closure must own it, otherwise it would borrow `self.scope` while
        // `self.inner` is already borrowed by `get_or_try_init`.
        let scope = self.scope.clone();
        let inner = self
            .inner
            .get_or_try_init(|| async move { launch(&scope).await })
            .await
            .map_err(|e: ComputerError| e)?;
        Ok(&inner.page)
    }

    /// Resolve a snapshot ref number or CSS selector to a live element handle.
    ///
    /// Refs come from the most recent `browser_snapshot` on this same page.
    async fn resolve_element(
        &self,
        page: &chromiumoxide::Page,
        selector: &str,
    ) -> Result<chromiumoxide::Element, ComputerError> {
        let selector = selector.trim();
        if selector.is_empty() {
            return Err(ComputerError::io(
                "empty selector: pass a snapshot ref number or a CSS selector",
            ));
        }
        // Pure number -> snapshot ref.
        if selector.chars().all(|c| c.is_ascii_digit()) && selector != "0" {
            // A stale execution context can only be observed here; the map
            // lookup itself is read-only, so retrying is safe.
            let lookup: Option<RefLookup> = retry_stale_context(|| async {
                page.evaluate_expression(format!(
                    "(() => {{ const m = window.__qidiRefs; \
                     if (!m) return {{ state: 'no_snapshot' }}; \
                     const e = m[{selector}]; \
                     return e ? {{ state: 'found', css: e.css_path }} : {{ state: 'missing' }}; }})()"
                ))
                .await
                .map_err(|e| ComputerError::io(format!("ref lookup failed: {e}")))
            })
            .await
            .ok()
            .and_then(|r| r.into_value().ok());
            match lookup {
                Some(RefLookup {
                    state,
                    css: Some(css),
                }) if state == "found" => {
                    if let Ok(el) = page.find_element(&css).await {
                        return Ok(el);
                    }
                    // The ref is in the map but the element is gone: the page
                    // changed since the snapshot that produced the ref.
                    return Err(ComputerError::io(format!(
                        "ref {selector} is stale -- the page changed after the last snapshot; \
                         run browser_snapshot first and use a fresh ref"
                    )));
                }
                Some(RefLookup {
                    state,
                    css: None,
                }) if state == "no_snapshot" => {
                    return Err(ComputerError::io(format!(
                        "ref {selector} cannot be resolved: this page has no snapshot refs \
                         (the page navigated, or browser_snapshot was never run) -- run \
                         browser_snapshot first and use a listed ref"
                    )));
                }
                Some(RefLookup { state, .. }) if state == "missing" => {
                    return Err(ComputerError::io(format!(
                        "ref {selector} does not exist in the current snapshot -- refs are \
                         renumbered on every browser_snapshot; run it again and use a listed ref"
                    )));
                }
                _ => {
                    return Err(ComputerError::io(format!(
                        "ref {selector} could not be resolved (ref lookup failed) -- run \
                         browser_snapshot first and use a listed ref"
                    )));
                }
            }
        }
        page.find_element(selector).await.map_err(|e| {
            ComputerError::io(format!("no element matches selector `{selector}`: {e}"))
        })
    }

    /// Wait for `document.readyState` to reach `complete` (bounded).
    async fn wait_ready(&self, page: &chromiumoxide::Page) {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(READY_TIMEOUT_MS);
        loop {
            let state: Option<String> = page
                .evaluate_expression("(() => document.readyState)()")
                .await
                .ok()
                .and_then(|r| r.into_value().ok());
            if state.as_deref() == Some("complete") {
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                return;
            }
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
    }

    async fn url_title(
        &self,
        page: &chromiumoxide::Page,
    ) -> Result<(String, String), ComputerError> {
        #[derive(serde::Deserialize, Default)]
        struct UrlTitle {
            #[serde(default)]
            url: String,
            #[serde(default)]
            title: String,
        }
        // First call after a navigation: tolerate the execution-context swap.
        let probe: UrlTitle = retry_stale_context(|| async {
            page.evaluate_expression("(() => ({url: location.href, title: document.title}))()")
                .await
                .map_err(|e| ComputerError::io(format!("url/title probe failed: {e}")))?
                .into_value()
                .map_err(|e| ComputerError::io(format!("url/title decode failed: {e}")))
        })
        .await?;
        Ok((probe.url, probe.title))
    }

    /// Collect visible interactive elements, (re)number them, and store the
    /// `ref -> css` map on the page for later click/type resolution.
    async fn collect_refs(
        &self,
        page: &chromiumoxide::Page,
    ) -> Result<Vec<BrowserElementRef>, ComputerError> {
        let res: String = retry_stale_context(|| async {
            page.evaluate_expression(format!("(() => {{ {COLLECT_REFS_JS} }})()"))
                .await
                .map_err(|e| ComputerError::io(format!("snapshot refs failed: {e}")))?
                .into_value()
                .map_err(|e| ComputerError::io(format!("snapshot refs decode failed: {e}")))
        })
        .await?;
        // The JS returns JSON.stringify(...) -- a string holding the array.
        let arr: Vec<BrowserElementRef> = serde_json::from_str(&res)
            .map_err(|e| ComputerError::io(format!("snapshot refs parse failed: {e}")))?;
        Ok(arr.into_iter().take(MAX_REFS).collect())
    }
}

#[async_trait::async_trait]
impl BrowserBackend for LocalBrowserBackend {
    async fn navigate(
        &self,
        request: BrowserNavigateRequest,
    ) -> Result<BrowserNavigateResult, ComputerError> {
        let _guard = self.op_lock.lock().await;
        let page = self.page().await?;
        page.goto(request.url.as_str())
            .await
            .map_err(|e| ComputerError::io(format!("navigation failed: {e}")))?;
        self.wait_ready(page).await;
        let settle = request.wait_ms.unwrap_or(DEFAULT_SETTLE_MS).min(10_000);
        tokio::time::sleep(Duration::from_millis(settle)).await;
        let (final_url, title) = self.url_title(page).await?;
        Ok(BrowserNavigateResult { final_url, title })
    }

    async fn snapshot(&self) -> Result<BrowserSnapshotResult, ComputerError> {
        let _guard = self.op_lock.lock().await;
        let page = self.page().await?;
        // Refresh the ref map before reading content so refs match what the
        // markdown below describes.
        let refs = self.collect_refs(page).await?;
        let html = page
            .content()
            .await
            .map_err(|e| ComputerError::io(format!("content fetch failed: {e}")))?;
        let markdown = html_to_markdown(&html);
        let (url, title) = self.url_title(page).await?;
        Ok(BrowserSnapshotResult {
            url,
            title,
            markdown,
            refs,
        })
    }

    async fn click(
        &self,
        request: BrowserClickRequest,
    ) -> Result<BrowserActionResult, ComputerError> {
        let _guard = self.op_lock.lock().await;
        let page = self.page().await?;
        let element = self.resolve_element(page, &request.selector).await?;
        element
            .click()
            .await
            .map_err(|e| ComputerError::io(format!("click failed: {e}")))?;
        // Give handlers/navigation a beat; don't block on full load.
        tokio::time::sleep(Duration::from_millis(DEFAULT_SETTLE_MS)).await;
        let (url, title) = self.url_title(page).await?;
        Ok(BrowserActionResult {
            detail: format!("clicked; page is now `{title}` ({url})"),
        })
    }

    async fn r#type(
        &self,
        request: BrowserTypeRequest,
    ) -> Result<BrowserActionResult, ComputerError> {
        let _guard = self.op_lock.lock().await;
        let page = self.page().await?;
        let element = self.resolve_element(page, &request.selector).await?;
        // Always focus: with clear=false the field may never have been
        // touched, and native key events would land on document.activeElement
        // (possibly the body) instead of the target field.
        element
            .focus()
            .await
            .map_err(|e| ComputerError::io(format!("focus failed: {e}")))?;
        if request.clear {
            // Clear document.activeElement via JS: the element API has no
            // clear in 0.9, and this covers inputs, textareas, and
            // contenteditable alike.
            // The evaluate either ran in a live context or was rejected before
            // executing, so retrying a stale-context failure cannot double-clear.
            retry_stale_context(|| async {
                page.evaluate_expression(
                    "(() => { const el = document.activeElement; if (!el) return false; \
                     if (typeof el.value === 'string') { el.value = ''; \
                     el.dispatchEvent(new Event('input', {bubbles:true})); return true; } \
                     if (el.isContentEditable) { el.textContent = ''; \
                     el.dispatchEvent(new Event('input', {bubbles:true})); return true; } \
                     return false; })()",
                )
                .await
                .map_err(|e| ComputerError::io(format!("clear failed: {e}")))
            })
            .await?;
        }
        if request.text.is_ascii() {
            // ASCII path: native key events, so site key handlers see them.
            element
                .type_str(request.text.as_str())
                .await
                .map_err(|e| ComputerError::io(format!("typing failed: {e}")))?;
        } else {
            // Non-ASCII path: chromiumoxide 0.9 walks a US keyboard layout
            // and hard-errors on CJK ("Key not found"). execCommand inserts
            // any text on the focused field and fires the input event. The
            // builder keeps the payload pure ASCII and single-line so it
            // survives the CDP Runtime.evaluate round-trip unchanged.
            retry_stale_context(|| async {
                page.evaluate_expression(build_insert_text_expression(request.text.as_str()))
                    .await
                    .map_err(|e| ComputerError::io(format!("insertText failed: {e}")))
            })
            .await?;
        }
        let mut detail = format!("typed into `{}`", request.selector.trim());
        if request.submit {
            element
                .press_key("Enter")
                .await
                .map_err(|e| ComputerError::io(format!("submit (Enter) failed: {e}")))?;
            tokio::time::sleep(Duration::from_millis(DEFAULT_SETTLE_MS)).await;
            detail.push_str(" and submitted (Enter)");
        }
        Ok(BrowserActionResult { detail })
    }

    async fn read(&self, request: BrowserReadRequest) -> Result<BrowserReadResult, ComputerError> {
        let _guard = self.op_lock.lock().await;
        let page = self.page().await?;
        let content = match request.selector {
            None => {
                let html = page
                    .content()
                    .await
                    .map_err(|e| ComputerError::io(format!("content fetch failed: {e}")))?;
                html_to_markdown(&html)
            }
            Some(selector) => {
                let element = self.resolve_element(page, &selector).await?;
                // Read-only: safe to retry across a context swap.
                let text: Option<String> = retry_stale_context(|| async {
                    element
                        .inner_text()
                        .await
                        .map_err(|e| ComputerError::io(format!("inner_text failed: {e}")))
                })
                .await?;
                truncate_chars(text.unwrap_or_default().trim(), MAX_CONTENT_CHARS)
            }
        };
        Ok(BrowserReadResult { content })
    }
}

/// JS executed inside the page: numbers the visible interactive elements,
/// stores `window.__qidiRefs = {ref -> record}`, returns a JSON array string.
const COLLECT_REFS_JS: &str = r#"
    const visible = (el) => {
        if (!el.isConnected) return false;
        const r = el.getBoundingClientRect();
        if (r.width <= 0 || r.height <= 0) return false;
        const s = getComputedStyle(el);
        return s.visibility !== 'hidden' && s.display !== 'none';
    };
    const cssPath = (el) => {
        if (!(el instanceof Element)) return '';
        const parts = [];
        let cur = el;
        while (cur && cur.nodeType === 1 && parts.length < 24) {
            let sel = cur.tagName.toLowerCase();
            if (cur.id) { parts.unshift(sel + '#' + cur.id); break; }
            const parent = cur.parentElement;
            if (parent) {
                const same = Array.from(parent.children).filter(
                    (c) => c.tagName === cur.tagName);
                if (same.length > 1) {
                    const idx = same.indexOf(cur) + 1;
                    sel += ':nth-of-type(' + idx + ')';
                }
            }
            parts.unshift(sel);
            cur = parent;
        }
        return parts.join(' > ');
    };
    const nodes = document.querySelectorAll(
        'a, button, input, select, textarea, summary, [role="button"], ' +
        '[role="link"], [role="tab"], [role="menuitem"], [onclick], [contenteditable="true"]');
    const refs = {};
    const out = [];
    let n = 0;
    for (const el of nodes) {
        if (!visible(el)) continue;
        n += 1;
        if (n > 400) break;
        const tag = el.tagName.toLowerCase();
        let role = tag;
        if (tag === 'a') role = 'link';
        else if (tag === 'button' || el.getAttribute('role') === 'button') role = 'button';
        else if (tag === 'input' || tag === 'textarea') role = 'textbox';
        else if (el.hasAttribute('onclick')) role = 'clickable';
        let text = (el.innerText || el.value || el.getAttribute('aria-label') ||
                    el.getAttribute('placeholder') || el.getAttribute('title') || '')
            .replace(/\s+/g, ' ').trim().slice(0, 200);
        const rec = { ref_id: n, tag: tag, role: role, text: text, css_path: cssPath(el) };
        refs[n] = rec;
        out.push(rec);
    }
    window.__qidiRefs = refs;
    return JSON.stringify(out);
"#;

/// Convert raw HTML to markdown using the crate's existing `htmd`
/// dependency, skipping the same tag set as web_fetch so script/style
/// source cannot eat the truncation budget or pollute the context.
fn html_to_markdown(html: &str) -> String {
    let converter = htmd::HtmlToMarkdown::builder()
        .skip_tags(vec![
            "script", "style", "noscript", "svg", "iframe", "object", "embed",
        ])
        .build();
    let md = converter.convert(html).unwrap_or_default();
    truncate_chars(md.trim(), MAX_CONTENT_CHARS)
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("\n\n[truncated]");
    out
}

/// Build a single-line, pure-ASCII JS expression that inserts `text` into the
/// focused element via `document.execCommand('insertText', ...)`.
///
/// The payload starts as a JSON string literal (`serde_json` already escapes
/// `"`, `\`, and control characters), then every non-ASCII character is
/// rewritten to a JS `\uXXXX` escape -- astral-plane scalars expand to a
/// UTF-16 surrogate pair. The result satisfies `expr.is_ascii()`, so it crosses
/// the CDP `Runtime.evaluate` boundary intact; an earlier multi-line template
/// with a `\` line-continuation corrupted non-ASCII payloads into a V8
/// `SyntaxError: Invalid or unexpected token`.
fn build_insert_text_expression(text: &str) -> String {
    // `&str -> JSON` is infallible; the closure is an unreachable guard.
    let json = serde_json::to_string(text).unwrap_or_else(|_| String::from("\"\""));
    let mut literal = String::with_capacity(json.len());
    for ch in json.chars() {
        let cp = ch as u32;
        if cp <= 0x7F {
            literal.push(ch);
        } else if cp <= 0xFFFF {
            literal.push_str(&format!("\\u{cp:04x}"));
        } else {
            // Astral plane -> UTF-16 surrogate pair.
            let offset = cp - 0x1_0000;
            let high = 0xD800 + (offset >> 10);
            let low = 0xDC00 + (offset & 0x3FF);
            literal.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
        }
    }
    format!(
        "(() => {{ const el = document.activeElement; if (!el) return false; return document.execCommand('insertText', false, {literal}); }})()"
    )
}

/// Extra Chromium switches for the local browser.
///
/// These are **bare keys**, not `--`-prefixed flags: chromiumoxide's
/// `ArgsBuilder` prepends `--` to every entry, so `"--no-first-run"` reaches
/// the process as `----no-first-run` -- a no-op switch Chromium silently
/// ignores. All three switches were shipped in that broken form; keeping them
/// as constants plus [`tests::browser_switches_are_bare_keys`] pins the
/// contract. `disable-extensions` is deliberately absent: chromiumoxide adds
/// it itself whenever no extension is requested.
const BROWSER_SWITCHES: &[&str] = &["no-first-run", "no-default-browser-check"];

/// Environment overrides applied to the browser process.
///
/// `__COMPAT_LAYER` is injected into a qidi process's environment block by
/// Windows AppCompat (`DetectorsAppHealth`). Edge inherits it and **relaunches
/// itself** with `--edge-skip-compat-layer-relaunch`, exiting the process
/// chromiumoxide spawned with status 0; the replacement does not inherit
/// chromiumoxide's stderr pipe, so `DevTools listening on ws://...` is never
/// observed and the launch looks like an instant exit while the browser is
/// actually alive. An empty value is equivalent to leaving the variable unset
/// for Chromium's compat-layer check, so Edge stays in the spawned process and
/// the CDP address flows through stderr again.
const BROWSER_ENV: &[(&str, &str)] = &[("__COMPAT_LAYER", "")];

/// Build the browser configuration used by [`launch`].
///
/// Split out so tests can inspect the environment overrides
/// ([`chromiumoxide::BrowserConfig::process_envs`] is public; the argument list
/// is not).
fn build_browser_config(
    user_data: &std::path::Path,
) -> Result<chromiumoxide::BrowserConfig, String> {
    // Default builder mode is headless; HeadlessMode is not re-exported at the
    // crate root in 0.9, so plain headless it is.
    let mut builder = chromiumoxide::BrowserConfig::builder()
        .user_data_dir(user_data)
        .window_size(1280, 900)
        // Drop the default 800x600 emulation override so the layout viewport
        // follows the 1280x900 window: snapshot refs and click coordinates
        // then describe the same page a headed browser would show.
        .viewport(None)
        .args(BROWSER_SWITCHES.iter().copied());
    for (key, value) in BROWSER_ENV {
        builder = builder.env(*key, *value);
    }
    builder.build()
}

/// Enroll an already-spawned browser process into `scope` so its tree is
/// reaped at exit (Windows: Job Object, also crash-safe via kill-on-close).
///
/// Coverage is **best-effort**, not a guarantee: `AssignProcessToJobObject`
/// only captures processes forked *after* the assignment, so children Edge
/// had already forked by the time this runs stay outside the Job. See
/// [`launch`] for the exact boundary and the tracked root fix (F9.3).
///
/// Builds a [`crate::util::ProcessGroup`], attaches the already-spawned pid,
/// registers the owning `Arc` with `scope`, and returns that `Arc`. The caller
/// must keep it alive for the process lifetime, because `register` stores only
/// a `Weak`.
#[cfg(windows)]
fn enroll_browser_tree(
    scope: &crate::util::ProcessScope,
    pid: u32,
) -> std::io::Result<std::sync::Arc<crate::util::ProcessGroup>> {
    let mut group = crate::util::ProcessGroup::new()?;
    group.attach_pid(pid)?;
    let group = std::sync::Arc::new(group);
    scope.register(&group);
    Ok(group)
}

/// Non-Windows enrollment is deliberately inert (tracked as F9.2).
///
/// On unix `ProcessGroup::new()` builds an empty group and `attach_pid` only
/// records the pid as a group-leader id; `killpg` then requires that pid to be
/// a process-group leader. chromiumoxide exposes no pre-exec hook to `setpgid`
/// the Edge child, so the child inherits qidi's group and enrolling it would
/// be either a no-op (ESRCH) or -- worse -- a `killpg` of qidi's own group,
/// killing qidi and the whole shell tree. Returning an unattached,
/// unregistered group keeps `launch` platform-uniform without ever calling
/// `killpg` on an unverified group.
#[cfg(not(windows))]
fn enroll_browser_tree(
    _scope: &crate::util::ProcessScope,
    _pid: u32,
) -> std::io::Result<std::sync::Arc<crate::util::ProcessGroup>> {
    Ok(std::sync::Arc::new(crate::util::ProcessGroup::new()?))
}

/// Launch a hidden browser and open a blank page.
///
/// The spawned Edge process tree is enrolled in `scope` (Windows: Job Object)
/// so it does not normally outlive this process -- best-effort; the coverage
/// boundary is stated at the enrollment site below.
async fn launch(scope: &crate::util::ProcessScope) -> Result<Inner, ComputerError> {
    let user_data = tempfile::Builder::new()
        .prefix("qidi-browser-")
        .tempdir()
        .map_err(|e| ComputerError::io(format!("profile dir failed: {e}")))?;
    let config = build_browser_config(user_data.path()).map_err(ComputerError::io)?;
    let (mut browser, mut handler) = chromiumoxide::Browser::launch(config)
        .await
        .map_err(|e| ComputerError::io(format!("browser launch failed: {e}")))?;
    // The CDP connection needs its event handler pumped continuously.
    tokio::spawn(async move { while let Some(_event) = handler.next().await {} });
    // Enroll the Edge tree right after launch. What this actually buys, stated
    // precisely (the earlier "the Job Object is what reaps it" was an
    // over-claim):
    //
    // - Covered: the browser process itself -- kill-on-close fires on a normal
    //   exit, a panic, and a force-kill -- plus every child Edge forks from the
    //   moment the assignment lands (renderers/GPU spawned later, for instance).
    // - NOT covered: children already forked inside this spawn->enroll window
    //   (CDP handshake, crashpad handler). `AssignProcessToJobObject` only
    //   captures forks that happen *after* it, so those processes never join
    //   the Job and no scope-level kill can reach them. In practice they are
    //   reclaimed by Chromium's own parent-watch (the browser dies, its
    //   helpers follow), which is why the tree looks fully clean today -- but
    //   that is Chromium's behaviour, not a property of this code.
    //
    // Overall this is best-effort reaping, not a guarantee. The root fix --
    // start the child inside the Job (suspend->assign->resume, or
    // PROC_THREAD_ATTRIBUTE_JOB_LIST) so enrollment has no pre-existing
    // window -- is tracked as F9.3 and deliberately not done here.
    let pid = browser.get_mut_child().and_then(|c| c.as_mut_inner().id());
    let _group = match pid {
        Some(pid) => match enroll_browser_tree(scope, pid) {
            Ok(group) => Some(group),
            Err(e) => {
                // Not fatal: the browser still works, it just may be orphaned.
                tracing::warn!(
                    pid,
                    error = %e,
                    "failed to enroll browser process tree; it may be orphaned on exit"
                );
                None
            }
        },
        None => {
            tracing::warn!(
                "browser launch returned no child pid; its process tree cannot be reaped on exit"
            );
            None
        }
    };
    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| ComputerError::io(format!("new page failed: {e}")))?;
    Ok(Inner {
        _browser: browser,
        _user_data: user_data,
        _group,
        page,
    })
}

/// Process-wide default backend used when no session `Browser` resource was
/// injected. One browser per CLI process; per-session override goes through
/// `resources.insert(Browser(Arc<dyn BrowserBackend>))`.
///
/// Known MVP limitations, stated here deliberately: the single page is
/// shared by the main session AND every subagent (their snapshots and
/// clicks act on the same page -- cross-session interference is possible),
/// and the temp profile dir is not cleaned up on process exit (the static
/// backend is never dropped; tracked as F9.1). The Edge process tree is
/// reaped on exit only **best-effort**: `launch` enrolls it in this backend's
/// `ProcessScope` (the process-global one by default), whose Job Object kills
/// the browser process and everything Edge forks *after* enrollment when the
/// handle closes -- including qidi crashing or being force-killed -- and whose
/// exit-path `kill_all()` kills them immediately on a normal exit. Children
/// forked before enrollment are outside the Job and rely on Chromium's own
/// parent-watch instead (see `launch`; root fix tracked as F9.3). The
/// `Browser` resource seam above is where a per-session lifecycle would plug
/// in.
static DEFAULT: OnceCell<Arc<LocalBrowserBackend>> = OnceCell::const_new();

pub async fn default_shared() -> Arc<LocalBrowserBackend> {
    DEFAULT
        .get_or_init(|| async { Arc::new(LocalBrowserBackend::new()) })
        .await
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes tests that observe or mutate the process-global scope: the
    /// regression probe below registers a group there, and the E2E asserts a
    /// browser group is enrolled there. Without this lock the probe's
    /// transient registration could make the E2E assertion pass spuriously.
    static GLOBAL_SCOPE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Windows-only: enrolling an already-spawned child and killing the scope
    /// must actually reap it. `kill_all` is the exit path's active kill; the
    /// Job Object's kill-on-close (same handle, closed when the process ends)
    /// is what makes this crash-safe.
    #[cfg(windows)]
    #[tokio::test]
    async fn browser_tree_enrollment_reaps_child() {
        use std::process::Stdio;

        // Never latch the process-global scope: use a self-owned one.
        let scope = crate::util::ProcessScope::new();

        let mut cmd = tokio::process::Command::new("cmd");
        cmd.args(["/c", "ping", "-n", "60", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[allow(clippy::disallowed_methods)] // test: spawns the child to enroll
        let mut child = cmd.spawn().expect("spawn long-lived child");
        let pid = child.id().expect("spawned child must have a pid");

        let group = enroll_browser_tree(&scope, pid).expect("enroll child process tree");
        assert_eq!(
            scope.live_count(),
            1,
            "the enrolled group must be live while its owner Arc is held"
        );

        scope.kill_all();
        let died = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
        if died.is_err() {
            // Don't leak the child if the assertion below fails.
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        drop(group);
        assert!(
            died.is_ok(),
            "kill_all must terminate the enrolled child within 3s; \
             wait() timing out means the Job Object did not reap it"
        );
    }

    /// Regression: `default()` must attach the process-global scope, not the
    /// unrelated empty `ProcessScope::default()`.
    ///
    /// `ProcessScope` exposes no identity accessor, so this pins the observable
    /// contract instead: a group registered through the backend's own `scope`
    /// must become visible to `global_process_scope()` -- exactly the set the
    /// cf-pager exit path's `kill_all()` operates on. The probe is an
    /// unattached (hence inert) `ProcessGroup`, dropped before returning; the
    /// lock keeps the E2E from observing it.
    #[test]
    fn default_backend_uses_global_process_scope() {
        let _guard = GLOBAL_SCOPE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let backend = LocalBrowserBackend::default();
        let global = crate::util::global_process_scope();
        let before = global.live_count();
        let probe = Arc::new(crate::util::ProcessGroup::new().expect("process group"));
        backend.scope.register(&probe);
        let after = global.live_count();
        assert!(
            after >= before + 1,
            "default() must use the process-global scope: a group registered on \
             the backend scope was not visible to global_process_scope() \
             (before={before}, after={after})"
        );
        drop(probe);
    }

    #[test]
    fn html_to_markdown_strips_markup() {
        let md = html_to_markdown("<h1>Hello</h1><p>World &amp; more</p>");
        assert!(md.to_lowercase().contains("hello"), "md: {md}");
        assert!(md.to_lowercase().contains("world"), "md: {md}");
    }

    #[test]
    fn truncate_caps_length() {
        let long = "x".repeat(50_000);
        let out = truncate_chars(&long, MAX_CONTENT_CHARS);
        assert!(out.chars().count() <= MAX_CONTENT_CHARS + 16);
        assert!(out.ends_with("[truncated]"));
    }

    /// Regression: chromiumoxide's `ArgsBuilder` prepends `--`, so an entry
    /// that already carries the prefix reaches Chromium as `----flag` and is
    /// silently ignored. All three switches shipped in that broken form.
    #[test]
    fn browser_switches_are_bare_keys() {
        for switch in BROWSER_SWITCHES {
            assert!(
                !switch.starts_with('-'),
                "switch {switch:?} must not carry the `--` prefix (ArgsBuilder adds it)"
            );
            assert!(!switch.is_empty(), "empty switch would render as `--`");
        }
        assert!(BROWSER_SWITCHES.contains(&"no-first-run"));
        assert!(BROWSER_SWITCHES.contains(&"no-default-browser-check"));
        // chromiumoxide adds this one itself when no extension is requested;
        // listing it here produced a duplicate `----disable-extensions`.
        assert!(!BROWSER_SWITCHES.contains(&"disable-extensions"));
    }

    /// Regression: the Windows AppCompat `__COMPAT_LAYER` inherited from the
    /// qidi process made Edge relaunch itself (status 0 on the spawned
    /// process, stderr pipe lost, CDP address never read). The launch config
    /// must clear it.
    #[test]
    fn browser_config_clears_appcompat_layer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = build_browser_config(dir.path()).expect("browser config");
        assert_eq!(
            config.user_data_dir.as_deref(),
            Some(dir.path()),
            "profile dir must be passed through"
        );
        let envs = config.process_envs.expect("env overrides must be present");
        assert_eq!(
            envs.get("__COMPAT_LAYER").map(String::as_str),
            Some(""),
            "the compat layer must be cleared with an empty value"
        );
    }

    /// Pin the JS<->Rust contract: COLLECT_REFS_JS emits ref_id/css_path
    /// (not ref/css). The E2E covers the real browser path; this catches
    /// key renames on browserless hosts too.
    #[test]
    fn collect_refs_json_contract() {
        let sample = r#"[{"ref_id":7,"tag":"a","role":"link","text":"Home","css_path":"nav > a"}]"#;
        let refs: Vec<BrowserElementRef> =
            serde_json::from_str(sample).expect("collect-refs JSON contract");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].ref_id, 7);
        assert_eq!(refs[0].role, "link");
        assert_eq!(refs[0].css_path, "nav > a");
    }

    #[test]
    fn insert_text_expression_cjk_is_ascii_single_line() {
        let expr = build_insert_text_expression("中文测试一二三");
        assert!(expr.is_ascii(), "expression must be pure ASCII: {expr}");
        assert!(
            !expr.contains('\n'),
            "expression must be single line: {expr}"
        );
        assert!(expr.starts_with("(() => {"), "expr: {expr}");
        assert!(expr.ends_with("})()"), "expr: {expr}");
        assert!(expr.contains("execCommand('insertText'"), "expr: {expr}");
        assert!(expr.contains("\\u4e2d\\u6587"), "cjk escape: {expr}");
    }

    #[test]
    fn insert_text_expression_astral_uses_surrogate_pair() {
        let expr = build_insert_text_expression("😀");
        assert!(expr.is_ascii(), "expression must be pure ASCII: {expr}");
        assert!(expr.contains("\\ud83d\\ude00"), "surrogate pair: {expr}");
    }

    #[test]
    fn insert_text_expression_escapes_quotes_backslash_and_cjk() {
        let expr = build_insert_text_expression("He said \"hi\" \\ ok 中文");
        assert!(expr.is_ascii(), "expression must be pure ASCII: {expr}");
        assert!(expr.contains("\\\""), "escaped double quote: {expr}");
        assert!(expr.contains("\\\\"), "escaped backslash: {expr}");
        assert!(expr.contains("\\u4e2d"), "cjk escape: {expr}");
    }

    #[test]
    fn insert_text_expression_handles_empty_control_and_single_quote() {
        let empty = build_insert_text_expression("");
        assert!(empty.is_ascii(), "expression: {empty}");
        assert!(empty.contains("false, \"\")"), "empty literal: {empty}");

        let newline = build_insert_text_expression("a\nb");
        assert!(newline.is_ascii(), "expression: {newline}");
        assert!(newline.contains("a\\nb"), "escaped newline: {newline}");

        let apostrophe = build_insert_text_expression("it's");
        assert!(apostrophe.is_ascii(), "expression: {apostrophe}");
        assert!(apostrophe.contains("\"it's\""), "literal: {apostrophe}");
    }

    /// Real-browser E2E: `QIDI_BROWSER_E2E=1 cargo test -p cf-tools browser_e2e -- --nocapture`.
    #[tokio::test]
    async fn browser_e2e_navigate_snapshot_click_type_read() {
        if std::env::var("QIDI_BROWSER_E2E").is_err() {
            // Not a failure: the suite must stay green on browserless hosts.
            return;
        }
        let backend = default_shared().await;
        let nav = backend
            .navigate(BrowserNavigateRequest {
                url: "data:text/html,<title>t</title><button onclick=\"this.textContent='done'\">go</button><input id=\"q\"/>".to_owned(),
                wait_ms: Some(0),
            })
            .await
            .expect("navigate");
        assert_eq!(nav.title, "t");

        // End-to-end evidence that the real launch path enrolled the Edge tree
        // in the process-global scope -- the exact set cf-pager's exit path
        // `kill_all()`s. Non-Windows enrollment is deliberately inert (F9.2),
        // so only Windows can assert it.
        #[cfg(windows)]
        {
            let _scope_guard = GLOBAL_SCOPE_TEST_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            assert!(
                crate::util::global_process_scope().live_count() >= 1,
                "launch must enroll the browser process tree in the global scope"
            );
        }

        let snap = backend.snapshot().await.expect("snapshot");
        assert!(
            snap.refs.iter().any(|r| r.role == "button"),
            "refs: {:?}",
            snap.refs
        );
        let btn_ref = snap
            .refs
            .iter()
            .find(|r| r.role == "button")
            .map(|r| r.ref_id.to_string())
            .expect("button ref");
        backend
            .click(BrowserClickRequest { selector: btn_ref })
            .await
            .expect("click by ref");
        let typed = backend
            .r#type(BrowserTypeRequest {
                selector: "#q".to_owned(),
                text: "hello".to_owned(),
                submit: false,
                clear: true,
            })
            .await
            .expect("type");
        assert!(typed.detail.contains("#q"), "detail: {}", typed.detail);
        let read = backend
            .read(BrowserReadRequest { selector: None })
            .await
            .expect("read");
        assert!(read.content.contains("done"), "content: {}", read.content);

        // Ref errors must say *which* failure happened: a number that is not in
        // the current snapshot is not the same as a page that was never
        // snapshotted.
        let missing = backend
            .click(BrowserClickRequest {
                selector: "9999".into(),
            })
            .await
            .expect_err("unknown ref must fail");
        assert!(
            missing.to_string().contains("does not exist"),
            "unknown ref: {missing}"
        );

        backend
            .navigate(BrowserNavigateRequest {
                url: "data:text/html,<title>fresh</title><p>new document</p>".to_owned(),
                wait_ms: Some(0),
            })
            .await
            .expect("renavigate");
        let no_snapshot = backend
            .click(BrowserClickRequest {
                selector: "1".into(),
            })
            .await
            .expect_err("no snapshot refs must fail");
        assert!(
            no_snapshot.to_string().contains("no snapshot refs"),
            "fresh document: {no_snapshot}"
        );
    }

    fn stale_context_error() -> ComputerError {
        ComputerError::io("snapshot refs failed: Cannot find context with specified id")
    }

    /// Windows-only snapshot of the Chromium-family process table as
    /// `(pid, parent_pid, command_line)`, via WMI.
    ///
    /// The kill assertion below has to observe the whole browser *tree*,
    /// including children forked before Job enrollment -- precisely the set
    /// `AssignProcessToJobObject` cannot describe. Enumerating the system table
    /// is the only way to see them from a test, and WMI is the one enumerator
    /// that needs no extra crate feature (the workspace `windows` pin enables
    /// neither `ToolHelp` nor `Diagnostics`).
    #[cfg(windows)]
    async fn chromium_process_table() -> std::io::Result<Vec<(u32, u32, String)>> {
        // Single quotes throughout: nothing to escape when this crosses the
        // `powershell -Command` argument boundary.
        const SCRIPT: &str = r#"Get-CimInstance Win32_Process | Where-Object { $_.Name -match '^(msedge|chrome|chromium)\.exe$' } | ForEach-Object { '{0}|{1}|{2}' -f $_.ProcessId, $_.ParentProcessId, $_.CommandLine }"#;
        let output = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
            .output()
            .await?;
        if !output.status.success() {
            return Err(std::io::Error::other(format!(
                "process enumeration failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut rows = Vec::new();
        for line in text.lines() {
            // `splitn(3, ..)` keeps `|` characters inside the command line.
            let mut fields = line.trim().splitn(3, '|');
            let (Some(pid), Some(parent)) = (fields.next(), fields.next()) else {
                continue;
            };
            let (Ok(pid), Ok(parent)) = (pid.trim().parse(), parent.trim().parse()) else {
                continue;
            };
            rows.push((pid, parent, fields.next().unwrap_or_default().to_owned()));
        }
        Ok(rows)
    }

    /// Size of the browser tree rooted at the process whose command line
    /// carries `profile_marker` (the launch's unique `--user-data-dir` name):
    /// the root plus its transitive children among `rows`.
    #[cfg(windows)]
    fn chromium_tree_size(rows: &[(u32, u32, String)], profile_marker: &str) -> usize {
        use std::collections::{HashMap, HashSet};

        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for (pid, parent, _) in rows {
            children.entry(*parent).or_default().push(*pid);
        }
        let mut pending: Vec<u32> = rows
            .iter()
            .filter(|(_, _, command_line)| command_line.contains(profile_marker))
            .map(|(pid, _, _)| *pid)
            .collect();
        let mut seen: HashSet<u32> = HashSet::new();
        while let Some(pid) = pending.pop() {
            if !seen.insert(pid) {
                continue;
            }
            if let Some(kids) = children.get(&pid) {
                pending.extend(kids.iter().copied());
            }
        }
        seen.len()
    }

    /// Real-browser E2E: `kill_all()` must actually reap the Edge tree, not
    /// merely close a handle. Opt-in (`QIDI_BROWSER_E2E=1`) because it needs a
    /// real Chromium-family browser on the host:
    /// `QIDI_BROWSER_E2E=1 cargo test -p cf-tools browser_e2e_kill_all -- --nocapture`.
    ///
    /// Runs on its own [`crate::util::ProcessScope`], never the process-global
    /// one: `kill_all` latches a scope closed, so killing the global scope here
    /// would break every other test that enrolls into it.
    ///
    /// The assertion is deliberately **not** "the tree is instantaneously
    /// empty". Reaping is best-effort (see `launch`): children forked before
    /// Job enrollment are outside the Job and exit only when Chromium's own
    /// parent-watch reaches them, on a timing this code does not control. So
    /// the test polls (bounded) for the tree to drain and accepts a tree that
    /// shrank but has not yet reached zero; it fails only when the kill
    /// demonstrably did nothing (as many processes alive as before it).
    #[cfg(windows)]
    #[tokio::test]
    async fn browser_e2e_kill_all_reaps_edge_tree() {
        if std::env::var("QIDI_BROWSER_E2E").is_err() {
            return; // not a failure: browserless hosts must stay green
        }
        let scope = crate::util::ProcessScope::new();
        let backend = LocalBrowserBackend {
            inner: OnceCell::new(),
            scope: scope.clone(),
            op_lock: Mutex::new(()),
        };
        backend
            .navigate(BrowserNavigateRequest {
                url: "data:text/html,<title>kill</title><p>kill me</p>".to_owned(),
                wait_ms: Some(0),
            })
            .await
            .expect("navigate");
        assert_eq!(
            scope.live_count(),
            1,
            "launch must enroll exactly one browser tree in the backend's scope"
        );

        // The launch's temp profile name is unique per run and appears in the
        // browser's own command line, so it identifies this tree's root.
        let marker = backend
            .inner
            .get()
            .expect("navigate succeeded, so Inner is initialised")
            ._user_data
            .path()
            .file_name()
            .expect("temp profile dir has a name")
            .to_string_lossy()
            .into_owned();

        let before = match chromium_process_table().await {
            Ok(rows) => chromium_tree_size(&rows, &marker),
            Err(e) => {
                eprintln!("[browser_e2e] skipping kill assertion: {e}");
                return;
            }
        };
        if before == 0 {
            eprintln!(
                "[browser_e2e] skipping kill assertion: no process found for profile {marker:?} \
                 (WMI could not see the browser tree on this host)"
            );
            return;
        }

        scope.kill_all();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
        let mut remaining = before;
        loop {
            remaining = chromium_process_table()
                .await
                .map(|rows| chromium_tree_size(&rows, &marker))
                .unwrap_or(remaining);
            if remaining == 0 || tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        assert!(
            remaining < before,
            "kill_all must reap the enrolled browser tree, but {remaining} of {before} \
             process(es) for profile {marker:?} were still running -- the kill had no effect"
        );
        if remaining > 0 {
            // Best-effort residue, explicitly not a failure: these are the
            // pre-enrollment children Chromium's parent-watch reclaims on its
            // own schedule (F9.3 makes the coverage exact).
            eprintln!(
                "[browser_e2e] best-effort: {remaining} of {before} process(es) for profile \
                 {marker:?} still visible after kill_all (pre-enrollment children; see F9.3)"
            );
        }
    }

    /// A stale-context failure is retried (bounded) and the second attempt
    /// succeeds. This pins the retry contract without a real browser: the
    /// CDP error text is what Chromium emits when a navigation replaces the
    /// execution context between `goto` and the next `Runtime.evaluate`.
    #[tokio::test]
    async fn retry_stale_context_recovers_within_the_bound() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let attempts = AtomicUsize::new(0);
        let result: Result<u32, ComputerError> = retry_stale_context(|| {
            let attempt = attempts.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt < 2 {
                    Err(stale_context_error())
                } else {
                    Ok(7)
                }
            }
        })
        .await;
        assert_eq!(result.expect("retry should recover"), 7);
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            3,
            "must succeed on the third attempt (2 retries)"
        );
    }

    /// Non-context errors are never retried: unrelated failures keep their
    /// previous behaviour (one attempt, original error).
    #[tokio::test]
    async fn retry_stale_context_leaves_other_errors_alone() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let attempts = AtomicUsize::new(0);
        let result: Result<(), ComputerError> = retry_stale_context(|| {
            attempts.fetch_add(1, Ordering::SeqCst);
            async {
                Err(ComputerError::io(
                    "navigation failed: net::ERR_NAME_NOT_RESOLVED",
                ))
            }
        })
        .await;
        let err = result.expect_err("non-context error must propagate");
        assert!(
            err.to_string().contains("ERR_NAME_NOT_RESOLVED"),
            "original error must survive: {err}"
        );
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            1,
            "non-context errors get exactly one attempt"
        );
    }

    /// A persistent stale-context error is retried at most
    /// `CONTEXT_RETRY_LIMIT` extra times, then surfaces unchanged.
    #[tokio::test]
    async fn retry_stale_context_is_bounded() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let attempts = AtomicUsize::new(0);
        let result: Result<(), ComputerError> = retry_stale_context(|| {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Err(stale_context_error()) }
        })
        .await;
        assert!(result.is_err(), "exhausted retries must surface the error");
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            CONTEXT_RETRY_LIMIT + 1,
            "total attempts must be bounded to limit + 1"
        );
    }

    /// Both spellings Chromium uses for the retired execution context are
    /// recognized; unrelated messages are not.
    #[test]
    fn stale_context_error_detection_is_specific() {
        assert!(is_stale_context_error(&ComputerError::io(
            "Cannot find context with specified id"
        )));
        assert!(is_stale_context_error(&ComputerError::io(
            "Cannot find context with specified ID"
        )));
        assert!(!is_stale_context_error(&ComputerError::io(
            "Cannot find element with selector `#missing`"
        )));
    }
}
