//! Built-in browser tool family driving the [`BrowserBackend`] resource.
//!
//! Five tools over one persistent browser session:
//! - `browser_navigate` -- load a URL (read-only)
//! - `browser_snapshot` -- markdown body + numbered interactive refs
//! - `browser_click` -- click a ref/selector (mutates external state)
//! - `browser_type` -- type into a field, optionally submit (mutates)
//! - `browser_read` -- page or element text (read-only)
//!
//! ## Resources
//!
//! - `Browser` -- browser backend (optional). When the host does not inject
//!   one, the tools fall back to the process-wide [`LocalBrowserBackend`],
//!   which lazily launches a hidden Edge/Chrome/Chromium on first use.

pub mod click;
pub mod navigate;
pub mod read;
pub mod snapshot;
pub mod type_;
pub mod types;

pub use click::ClickImpl;
pub use navigate::NavigateImpl;
pub use read::ReadImpl;
pub use snapshot::SnapshotImpl;
pub use type_::TypeImpl;

use std::sync::Arc;

use crate::computer::types::BrowserBackend;
use crate::types::resources::{Browser, SharedResources};

/// Wire names (single source of truth shared by definitions and tests).
pub const BROWSER_NAVIGATE_TOOL_NAME: &str = "browser_navigate";
pub const BROWSER_SNAPSHOT_TOOL_NAME: &str = "browser_snapshot";
pub const BROWSER_CLICK_TOOL_NAME: &str = "browser_click";
pub const BROWSER_TYPE_TOOL_NAME: &str = "browser_type";
pub const BROWSER_READ_TOOL_NAME: &str = "browser_read";

/// Resolve the session-injected `Browser` backend, falling back to the
/// process-wide local browser when the host did not inject one.
pub(crate) async fn backend_from(
    resources: &SharedResources,
) -> Result<Arc<dyn BrowserBackend>, cf_tool_runtime::ToolError> {
    {
        let res = resources.lock().await;
        if let Some(b) = res.get::<Browser>() {
            return Ok(b.0.clone());
        }
    }
    Ok(crate::computer::local::browser::default_shared().await)
}

/// Map a backend error onto the tool-error surface.
pub(crate) fn browser_err(e: crate::computer::types::ComputerError) -> cf_tool_runtime::ToolError {
    cf_tool_runtime::ToolError::custom("browser_error", e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::computer::types::{
        BrowserActionResult, BrowserNavigateRequest, BrowserNavigateResult, BrowserReadRequest,
        BrowserReadResult, BrowserSnapshotResult, BrowserTypeRequest,
    };
    use crate::types::output::ToolOutput;
    use crate::types::resources::Resources;
    use crate::types::tool::ToolKind;

    /// Canned backend: records the last request of each flavor and returns
    /// fixed results, so the tool layer is testable without a browser.
    #[derive(Default)]
    struct MockBrowser {
        last_navigate: std::sync::Mutex<Option<BrowserNavigateRequest>>,
        last_click: std::sync::Mutex<Option<String>>,
        last_type: std::sync::Mutex<Option<BrowserTypeRequest>>,
        last_read: std::sync::Mutex<Option<Option<String>>>,
    }

    #[async_trait::async_trait]
    impl crate::computer::types::BrowserBackend for MockBrowser {
        async fn navigate(
            &self,
            request: BrowserNavigateRequest,
        ) -> Result<BrowserNavigateResult, crate::computer::types::ComputerError> {
            *self.last_navigate.lock().unwrap() = Some(request);
            Ok(BrowserNavigateResult {
                final_url: "https://example.test/final".to_owned(),
                title: "Example".to_owned(),
            })
        }

        async fn snapshot(&self) -> Result<BrowserSnapshotResult, crate::computer::types::ComputerError> {
            Ok(BrowserSnapshotResult {
                url: "https://example.test".to_owned(),
                title: "Example".to_owned(),
                markdown: "# Example\n\nbody text".to_owned(),
                refs: vec![crate::computer::types::BrowserElementRef {
                    ref_id: 1,
                    tag: "button".to_owned(),
                    role: "button".to_owned(),
                    text: "go".to_owned(),
                    css_path: "button".to_owned(),
                }],
            })
        }

        async fn click(
            &self,
            request: crate::computer::types::BrowserClickRequest,
        ) -> Result<BrowserActionResult, crate::computer::types::ComputerError> {
            *self.last_click.lock().unwrap() = Some(request.selector);
            Ok(BrowserActionResult {
                detail: "clicked; page is now `x` (https://example.test)".to_owned(),
            })
        }

        async fn r#type(
            &self,
            request: BrowserTypeRequest,
        ) -> Result<BrowserActionResult, crate::computer::types::ComputerError> {
            *self.last_type.lock().unwrap() = Some(request);
            Ok(BrowserActionResult {
                detail: "typed into `#q`".to_owned(),
            })
        }

        async fn read(
            &self,
            request: BrowserReadRequest,
        ) -> Result<BrowserReadResult, crate::computer::types::ComputerError> {
            *self.last_read.lock().unwrap() = Some(request.selector);
            Ok(BrowserReadResult {
                content: "# Example\n\nbody text".to_owned(),
            })
        }
    }

    /// Build a ToolCallContext whose resources carry the mock backend.
    fn ctx_with_browser(backend: Arc<MockBrowser>) -> cf_tool_runtime::ToolCallContext {
        let mut res = Resources::new();
        res.insert(crate::types::resources::Browser(backend));
        let shared: SharedResources = Arc::new(tokio::sync::Mutex::new(res));
        let mut ctx = cf_tool_runtime::ToolCallContext::default();
        ctx.insert(shared);
        ctx
    }

    /// The five wire names are the wire identifiers; pin against typos.
    #[test]
    fn browser_tool_constants_match_registered_ids() {
        assert_eq!(BROWSER_NAVIGATE_TOOL_NAME, "browser_navigate");
        assert_eq!(BROWSER_SNAPSHOT_TOOL_NAME, "browser_snapshot");
        assert_eq!(BROWSER_CLICK_TOOL_NAME, "browser_click");
        assert_eq!(BROWSER_TYPE_TOOL_NAME, "browser_type");
        assert_eq!(BROWSER_READ_TOOL_NAME, "browser_read");
        assert_eq!(
            cf_tool_runtime::Tool::id(&NavigateImpl).to_string(),
            BROWSER_NAVIGATE_TOOL_NAME
        );
        assert_eq!(
            cf_tool_runtime::Tool::id(&ClickImpl).to_string(),
            BROWSER_CLICK_TOOL_NAME
        );
        assert_eq!(
            cf_tool_runtime::Tool::id(&SnapshotImpl).to_string(),
            BROWSER_SNAPSHOT_TOOL_NAME
        );
        assert_eq!(
            cf_tool_runtime::Tool::id(&TypeImpl).to_string(),
            BROWSER_TYPE_TOOL_NAME
        );
        assert_eq!(
            cf_tool_runtime::Tool::id(&ReadImpl).to_string(),
            BROWSER_READ_TOOL_NAME
        );
    }

    /// The advertised names must come from the same constants.
    #[test]
    fn browser_tool_descriptions_use_constant_names() {
        let ctx = cf_tool_runtime::ListToolsContext::default();
        for (name, desc) in [
            (BROWSER_NAVIGATE_TOOL_NAME, cf_tool_runtime::Tool::description(&NavigateImpl, &ctx)),
            (BROWSER_SNAPSHOT_TOOL_NAME, cf_tool_runtime::Tool::description(&SnapshotImpl, &ctx)),
            (BROWSER_CLICK_TOOL_NAME, cf_tool_runtime::Tool::description(&ClickImpl, &ctx)),
            (BROWSER_TYPE_TOOL_NAME, cf_tool_runtime::Tool::description(&TypeImpl, &ctx)),
            (BROWSER_READ_TOOL_NAME, cf_tool_runtime::Tool::description(&ReadImpl, &ctx)),
        ] {
            assert_eq!(desc.name, name);
        }
    }

    /// Taxonomy/capability intent: read-trio is read-only in the QidiBuild
    /// namespace; click/type are mutating (Write scope, like bash).
    #[test]
    fn browser_tool_meta_split_read_vs_act() {
        use crate::types::tool_metadata::ToolMetadata;
        use crate::types::tool::ToolNamespace;
        // Compute per-impl values first: the array must hold one uniform
        // (ToolKind, ToolCapabilities) type, and the concrete impls are
        // distinct types.
        let read_side = [
            (ToolMetadata::kind(&NavigateImpl), cf_tool_runtime::Tool::capabilities(&NavigateImpl)),
            (ToolMetadata::kind(&SnapshotImpl), cf_tool_runtime::Tool::capabilities(&SnapshotImpl)),
            (ToolMetadata::kind(&ReadImpl), cf_tool_runtime::Tool::capabilities(&ReadImpl)),
        ];
        for (kind, caps) in read_side {
            assert!(matches!(kind, ToolKind::BrowserRead));
            assert!(ToolKind::BrowserRead.is_read_only());
            assert!(caps.is_read_only);
            assert_eq!(caps.tool_scope, Some(cf_tool_protocol::ToolScope::Read));
        }
        let act_side = [
            (ToolMetadata::kind(&ClickImpl), cf_tool_runtime::Tool::capabilities(&ClickImpl)),
            (ToolMetadata::kind(&TypeImpl), cf_tool_runtime::Tool::capabilities(&TypeImpl)),
        ];
        for (kind, caps) in act_side {
            assert!(matches!(kind, ToolKind::BrowserAct));
            assert!(!ToolKind::BrowserAct.is_read_only());
            assert!(!caps.is_read_only);
            assert_eq!(caps.tool_scope, Some(cf_tool_protocol::ToolScope::Write));
        }
        for ns in [
            ToolMetadata::tool_namespace(&NavigateImpl),
            ToolMetadata::tool_namespace(&SnapshotImpl),
            ToolMetadata::tool_namespace(&ReadImpl),
            ToolMetadata::tool_namespace(&ClickImpl),
            ToolMetadata::tool_namespace(&TypeImpl),
        ] {
            assert_eq!(ns, ToolNamespace::QidiBuild);
        }
    }

    /// run() dispatches to the session Browser resource and formats output.
    #[tokio::test]
    async fn navigate_run_uses_backend_and_validates_url() {
        let mock = Arc::new(MockBrowser::default());
        let ctx = ctx_with_browser(mock.clone());
        let out = cf_tool_runtime::Tool::run(
            &NavigateImpl,
            ctx,
            types::BrowserNavigateInput {
                url: "https://example.test".to_owned(),
                wait_ms: None,
            },
        )
        .await
        .expect("navigate run");
        assert!(
            matches!(&out, ToolOutput::Text(t) if t.text.contains("https://example.test/final")),
            "out: {out:?}"
        );
        assert_eq!(
            mock.last_navigate.lock().unwrap().as_ref().unwrap().url,
            "https://example.test"
        );

        // Fail-closed on non-http schemes.
        let ctx2 = ctx_with_browser(mock.clone());
        let err = cf_tool_runtime::Tool::run(
            &NavigateImpl,
            ctx2,
            types::BrowserNavigateInput {
                url: "javascript:alert(1)".to_owned(),
                wait_ms: None,
            },
        )
        .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn snapshot_and_read_format_output() {
        let mock = Arc::new(MockBrowser::default());
        let snap = cf_tool_runtime::Tool::run(
            &SnapshotImpl,
            ctx_with_browser(mock.clone()),
            types::BrowserSnapshotInput { refs_only: false },
        )
        .await
        .expect("snapshot run");
        assert!(
            matches!(&snap, ToolOutput::Text(t) if t.text.contains("[1] button: \"go\"") && t.text.contains("body text")),
            "snap: {snap:?}"
        );

        let read = cf_tool_runtime::Tool::run(
            &ReadImpl,
            ctx_with_browser(mock.clone()),
            types::BrowserReadInput { selector: None },
        )
        .await
        .expect("read run");
        assert!(
            matches!(&read, ToolOutput::Text(t) if t.text.contains("body text")),
            "read: {read:?}"
        );
        assert_eq!(mock.last_read.lock().unwrap().as_ref().unwrap(), &None);
    }

    #[tokio::test]
    async fn click_and_type_forward_requests() {
        let mock = Arc::new(MockBrowser::default());
        let click = cf_tool_runtime::Tool::run(
            &ClickImpl,
            ctx_with_browser(mock.clone()),
            types::BrowserClickInput {
                selector: "3".to_owned(),
            },
        )
        .await
        .expect("click run");
        assert!(matches!(&click, ToolOutput::Text(t) if t.text.contains("clicked")));
        assert_eq!(mock.last_click.lock().unwrap().as_deref(), Some("3"));

        let typed = cf_tool_runtime::Tool::run(
            &TypeImpl,
            ctx_with_browser(mock.clone()),
            types::BrowserTypeInput {
                selector: "#q".to_owned(),
                text: "hello".to_owned(),
                submit: true,
                clear: true,
            },
        )
        .await
        .expect("type run");
        assert!(matches!(&typed, ToolOutput::Text(t) if t.text.contains("#q")));
        let last = mock.last_type.lock().unwrap().clone().unwrap();
        assert!(last.submit);
        assert!(last.clear);
    }

    /// Wire shape: the two new kinds serialize snake_case and older binaries
    /// degrade them to `Other` instead of erroring.
    #[test]
    fn browser_tool_kinds_serde_round_trip_and_degrade() {
        for (kind, wire) in [
            (ToolKind::BrowserRead, "browser_read"),
            (ToolKind::BrowserAct, "browser_act"),
        ] {
            let v = serde_json::to_value(kind).unwrap();
            assert_eq!(v, serde_json::json!(wire));
            assert_eq!(serde_json::from_value::<ToolKind>(v).unwrap(), kind);
        }
    }
}