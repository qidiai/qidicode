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

struct Inner {
    /// Keep the browser handle alive for the process lifetime.
    _browser: chromiumoxide::Browser,
    /// Owned profile dir; deleted when the backend drops.
    _user_data: tempfile::TempDir,
    page: chromiumoxide::Page,
}

/// Browser backend bound to a lazily-launched local Chromium-family process.
#[derive(Default)]
pub struct LocalBrowserBackend {
    inner: OnceCell<Inner>,
    /// Serializes snapshot/click/type sequences so a stale ref map can never
    /// race a concurrent snapshot.
    op_lock: Mutex<()>,
}

impl LocalBrowserBackend {
    pub fn new() -> Self {
        Self::default()
    }

    async fn page(&self) -> Result<&chromiumoxide::Page, ComputerError> {
        let inner = self
            .inner
            .get_or_try_init(|| async { launch().await })
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
            let lookup: Option<String> = page
                .evaluate_expression(format!(
                    "(() => {{ const e = window.__qidiRefs && window.__qidiRefs[{selector}]; \
                     return e ? e.css : null; }})()"
                ))
                .await
                .ok()
                .and_then(|r| r.into_value().ok());
            if let Some(css) = lookup {
                if let Ok(el) = page.find_element(&css).await {
                    return Ok(el);
                }
            }
            return Err(ComputerError::io(format!(
                "ref {selector} is stale -- run browser_snapshot first and use a fresh ref"
            )));
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
        let probe: UrlTitle = page
            .evaluate_expression("(() => ({url: location.href, title: document.title}))()")
            .await
            .map_err(|e| ComputerError::io(format!("url/title probe failed: {e}")))?
            .into_value()
            .map_err(|e| ComputerError::io(format!("url/title decode failed: {e}")))?;
        Ok((probe.url, probe.title))
    }

    /// Collect visible interactive elements, (re)number them, and store the
    /// `ref -> css` map on the page for later click/type resolution.
    async fn collect_refs(
        &self,
        page: &chromiumoxide::Page,
    ) -> Result<Vec<BrowserElementRef>, ComputerError> {
        let res: String = page
            .evaluate_expression(format!("(() => {{ {COLLECT_REFS_JS} }})()"))
            .await
            .map_err(|e| ComputerError::io(format!("snapshot refs failed: {e}")))?
            .into_value()
            .map_err(|e| ComputerError::io(format!("snapshot refs decode failed: {e}")))?;
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
        if request.clear {
            // Focus first, then clear document.activeElement via JS: the
            // element API has no clear in 0.9, and the activeElement route
            // covers inputs, textareas, and contenteditable alike.
            element
                .focus()
                .await
                .map_err(|e| ComputerError::io(format!("focus for clear failed: {e}")))?;
            page.evaluate_expression(
                "(() => { const el = document.activeElement; if (!el) return false; \
                 if (typeof el.value === 'string') { el.value = ''; \
                 el.dispatchEvent(new Event('input', {bubbles:true})); return true; } \
                 if (el.isContentEditable) { el.textContent = ''; \
                 el.dispatchEvent(new Event('input', {bubbles:true})); return true; } \
                 return false; })()",
            )
            .await
            .map_err(|e| ComputerError::io(format!("clear failed: {e}")))?;
        }
        element
            .type_str(request.text.as_str())
            .await
            .map_err(|e| ComputerError::io(format!("typing failed: {e}")))?;
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

    async fn read(
        &self,
        request: BrowserReadRequest,
    ) -> Result<BrowserReadResult, ComputerError> {
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
                let text: Option<String> = element
                    .inner_text()
                    .await
                    .map_err(|e| ComputerError::io(format!("inner_text failed: {e}")))?;
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
        const rec = { ref: n, tag: tag, role: role, text: text, css: cssPath(el) };
        refs[n] = rec;
        out.push(rec);
    }
    window.__qidiRefs = refs;
    return JSON.stringify(out);
"#;

/// Convert raw HTML to markdown using the crate's existing `htmd` dependency.
fn html_to_markdown(html: &str) -> String {
    let md = htmd::convert(html).unwrap_or_default();
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

/// Launch a hidden browser and open a blank page.
async fn launch() -> Result<Inner, ComputerError> {
    let user_data = tempfile::Builder::new()
        .prefix("qidi-browser-")
        .tempdir()
        .map_err(|e| ComputerError::io(format!("profile dir failed: {e}")))?;
    // Default builder mode is headless; HeadlessMode is not re-exported at the
    // crate root in 0.9, so plain headless it is.
    let config = chromiumoxide::BrowserConfig::builder()
        .user_data_dir(user_data.path())
        .window_size(1280, 900)
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-extensions")
        .build()
        .map_err(ComputerError::io)?;
    let (browser, mut handler) = chromiumoxide::Browser::launch(config)
        .await
        .map_err(|e| ComputerError::io(format!("browser launch failed: {e}")))?;
    // The CDP connection needs its event handler pumped continuously.
    tokio::spawn(async move {
        while let Some(_event) = handler.next().await {}
    });
    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| ComputerError::io(format!("new page failed: {e}")))?;
    Ok(Inner {
        _browser: browser,
        _user_data: user_data,
        page,
    })
}

/// Process-wide default backend used when no session `Browser` resource was
/// injected. One browser per CLI process; per-session override goes through
/// `resources.insert(Browser(Arc<dyn BrowserBackend>))`.
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
    }
}