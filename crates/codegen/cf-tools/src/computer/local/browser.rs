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
    /// Owned profile dir. The default backend lives in a static OnceCell,
    /// which Rust never drops: on exit the Edge process tree and this
    /// profile dir are left to OS temp cleanup (see default_shared).
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
                     return e ? e.css_path : null; }})()"
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
            page.evaluate_expression(build_insert_text_expression(request.text.as_str()))
                .await
                .map_err(|e| ComputerError::io(format!("insertText failed: {e}")))?;
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
        // Drop the default 800x600 emulation override so the layout viewport
        // follows the 1280x900 window: snapshot refs and click coordinates
        // then describe the same page a headed browser would show.
        .viewport(None)
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-extensions")
        .build()
        .map_err(ComputerError::io)?;
    let (browser, mut handler) = chromiumoxide::Browser::launch(config)
        .await
        .map_err(|e| ComputerError::io(format!("browser launch failed: {e}")))?;
    // The CDP connection needs its event handler pumped continuously.
    tokio::spawn(async move { while let Some(_event) = handler.next().await {} });
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
///
/// Known MVP limitations, stated here deliberately: the single page is
/// shared by the main session AND every subagent (their snapshots and
/// clicks act on the same page -- cross-session interference is possible),
/// and neither the Edge process tree nor the temp profile dir is cleaned
/// up on process exit (statics never drop; orphaned headless Edge is left
/// to the OS). The `Browser` resource seam above is where a per-session
/// lifecycle would plug in.
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
