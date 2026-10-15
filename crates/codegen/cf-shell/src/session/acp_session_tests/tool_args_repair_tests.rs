//! Tests for the conservative tool-argument repair pass
//! (`tool_args_repair::repair_tool_arguments`).
//!
//! Style mirrors `build_tool_parse_error_message_tests.rs`: regression tests
//! anchored on real incidents, plus defensive tests that guard the "only quote
//! what can only have been a string" invariant.

use super::*;

// ── Regression: the actual observed corruption ─────────────────────────

/// Exact incident shape from the GLM-5.3 line: the provider dropped the quotes
/// around a bare value token (`KIMI-K3-222`). The sampled text even carries the
/// stray trailing space before the comma (`KIMI-K3-222 ,`) — the repair must
/// preserve the value intact and recover a parseable object.
#[test]
fn repairs_bare_value_token_with_trailing_space() {
    let raw = r#"{"description": "K3 audit", "model": KIMI-K3-222 , "prompt": "x", "subagent_type": "general-purpose"}"#;
    assert!(
        serde_json::from_str::<serde_json::Value>(raw).is_err(),
        "precondition: the raw sample must be invalid JSON"
    );

    let repaired = repair_tool_arguments(raw).expect("must repair the bare value token");
    let v: serde_json::Value =
        serde_json::from_str(&repaired).expect("repaired text must parse as JSON");

    assert_eq!(v["model"], "KIMI-K3-222");
    assert_eq!(v["description"], "K3 audit");
    assert_eq!(v["prompt"], "x");
    assert_eq!(v["subagent_type"], "general-purpose");
}

/// Same corruption without the stray trailing space — the common case.
#[test]
fn repairs_bare_value_token_without_trailing_space() {
    let raw = r#"{"model": KIMI-K3-222, "prompt": "x"}"#;
    let repaired = repair_tool_arguments(raw).expect("must repair");
    let v: serde_json::Value = serde_json::from_str(&repaired).unwrap();
    assert_eq!(v["model"], "KIMI-K3-222");
    assert_eq!(v["prompt"], "x");
}

/// Other slugs observed to trigger the corruption must also be recovered.
#[test]
fn repairs_other_observed_slugs() {
    for slug in ["GLM-5.3-222", "kimi-k2.7-code-highspeed-222"] {
        let raw = format!(r#"{{"model": {slug}}}"#);
        let repaired =
            repair_tool_arguments(&raw).unwrap_or_else(|| panic!("must repair {slug}"));
        let v: serde_json::Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["model"], slug);
    }
}

// ── Defensive: never quote legitimate JSON literals / numbers ──────────

/// The repair must not touch `true` / `false` / `null` / numbers. For this
/// (already-valid) object the repair is a no-op and leaves the text byte-for-byte
/// identical.
#[test]
fn does_not_quote_literals_or_numbers() {
    let raw = r#"{"a": true, "b": null, "c": 42}"#;
    // Precondition: this is valid JSON, so repair is only invoked defensively.
    assert!(serde_json::from_str::<serde_json::Value>(raw).is_ok());

    let repaired = repair_tool_arguments(raw).expect("valid JSON stays parseable");
    assert_eq!(repaired, raw, "no false repairs on a valid object");
    assert!(!repaired.contains("\"true\""));
    assert!(!repaired.contains("\"false\""));
    assert!(!repaired.contains("\"null\""));
    assert!(!repaired.contains("\"42\""));
}

/// Explicit negative-value variant: `false` and a negative number must survive
/// untouched while an adjacent bare string token is repaired.
#[test]
fn repairs_bare_string_but_spares_negative_number() {
    let raw = r#"{"flag": false, "offset": -3, "model": KIMI-K3-222}"#;
    let repaired = repair_tool_arguments(raw).expect("must repair");
    let v: serde_json::Value = serde_json::from_str(&repaired).unwrap();
    assert_eq!(v["flag"], false);
    assert_eq!(v["offset"], -3);
    assert_eq!(v["model"], "KIMI-K3-222");
    assert!(!repaired.contains("\"false\""));
    assert!(!repaired.contains("\"-3\""));
}

// ── Double-bare: unquoted key AND value ────────────────────────────────

#[test]
fn repairs_bare_key_and_value() {
    let raw = r#"{model: KIMI-K3-222}"#;
    let repaired = repair_tool_arguments(raw).expect("must repair key and value");
    let v: serde_json::Value = serde_json::from_str(&repaired).unwrap();
    assert_eq!(v["model"], "KIMI-K3-222");
}

// ── Non-repairable inputs fall through (return None) ───────────────────

#[test]
fn truncated_object_is_not_repairable() {
    assert!(repair_tool_arguments(r#"{"a": "b""#).is_none());
}

#[test]
fn stray_brace_is_not_repairable() {
    assert!(repair_tool_arguments(r#"{"a": }}"#).is_none());
}

#[test]
fn non_json_garbage_is_not_repairable() {
    assert!(repair_tool_arguments("not json at all").is_none());
}

/// A value token that is *not* terminated by a structural delimiter (here a
/// `:` follows) must not be quoted — the input stays unparseable, so `None`.
#[test]
fn value_not_terminated_by_delimiter_is_left_alone() {
    assert!(repair_tool_arguments(r#"{"a": foo bar}"#).is_none());
}

// ── Call-site sequence: mirrors `prepare_tool_call` ────────────────────

/// Reproduce the exact production decision sequence (`tool_calls.rs`):
/// `normalize_empty_arguments` → parse (fails) → repair → parse. The repaired
/// arguments must yield the same `Value` the tool would have received had the
/// model emitted valid JSON.
#[test]
fn production_sequence_recovers_arguments() {
    let arguments = r#"{"description": "K3 audit", "model": KIMI-K3-222 , "prompt": "x", "subagent_type": "general-purpose"}"#;

    // Step 1: parse the (normalized) raw arguments — must fail.
    let normalized = crate::session::helpers::tool_input_parsing::normalize_empty_arguments(arguments);
    assert!(serde_json::from_str::<serde_json::Value>(normalized).is_err());

    // Step 2: conservative repair, then re-parse — must succeed.
    let repaired = repair_tool_arguments(arguments).expect("repair must succeed");
    let v: serde_json::Value = serde_json::from_str(&repaired).expect("repaired must parse");
    assert_eq!(
        v,
        serde_json::json!({
            "description": "K3 audit",
            "model": "KIMI-K3-222",
            "prompt": "x",
            "subagent_type": "general-purpose",
        })
    );
}

/// Valid arguments must never be routed through the repair (the call site only
/// consults it when the ordinary parse failed), so a normal call is unchanged.
#[test]
fn production_sequence_valid_args_untouched() {
    let arguments = r#"{"model": "KIMI-K3-222", "prompt": "x"}"#;
    let normalized = crate::session::helpers::tool_input_parsing::normalize_empty_arguments(arguments);
    let v: serde_json::Value = serde_json::from_str(normalized).expect("valid args parse directly");
    assert_eq!(v["model"], "KIMI-K3-222");
}

// ── Warn visibility ────────────────────────────────────────────────────

/// The repair is a silent, best-effort fix, so it must always leave a WARN
/// behind. Uses a scoped subscriber capture (same pattern as
/// `agent/mvp_agent/tests.rs`).
mod capture {
    use tokio::sync::mpsc;
    use tracing::Subscriber;
    use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
    use tracing_subscriber::registry::LookupSpan;

    pub(crate) struct CapturedEvent {
        pub level: tracing::Level,
        pub fields: String,
    }

    pub(crate) struct Captured {
        pub events_rx: mpsc::UnboundedReceiver<CapturedEvent>,
        _guard: tracing::subscriber::DefaultGuard,
    }

    pub(crate) fn capture() -> Captured {
        let (tx, rx) = mpsc::unbounded_channel();
        let subscriber = tracing_subscriber::registry().with(CaptureLayer { tx });
        let guard = tracing::subscriber::set_default(subscriber);
        Captured {
            events_rx: rx,
            _guard: guard,
        }
    }

    struct CaptureLayer {
        tx: mpsc::UnboundedSender<CapturedEvent>,
    }

    impl<S> Layer<S> for CaptureLayer
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
            let mut v = Visitor::default();
            event.record(&mut v);
            let _ = self.tx.send(CapturedEvent {
                level: *event.metadata().level(),
                fields: v.out,
            });
        }
    }

    #[derive(Default)]
    struct Visitor {
        out: String,
    }

    impl tracing::field::Visit for Visitor {
        fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
            if !self.out.is_empty() {
                self.out.push(' ');
            }
            self.out.push_str(f.name());
            self.out.push('=');
            self.out.push_str(&format!("{v:?}"));
        }
        fn record_str(&mut self, f: &tracing::field::Field, v: &str) {
            if !self.out.is_empty() {
                self.out.push(' ');
            }
            self.out.push_str(f.name());
            self.out.push('=');
            self.out.push_str(v);
        }
    }
}

#[test]
fn successful_repair_emits_warn_with_summary() {
    let captured = capture::capture();
    let raw = r#"{"model": KIMI-K3-222}"#;
    let repaired = repair_tool_arguments(raw).expect("must repair");
    assert!(serde_json::from_str::<serde_json::Value>(&repaired).is_ok());

    let mut rx = captured.events_rx;
    let mut saw = false;
    while let Ok(event) = rx.try_recv() {
        if event.level == tracing::Level::WARN
            && event.fields.contains("Repaired malformed tool arguments JSON")
            && event.fields.contains("original=")
        {
            saw = true;
            break;
        }
    }
    assert!(saw, "a successful repair must emit a WARN with the original summary");
}

#[test]
fn failed_repair_emits_no_warn() {
    let captured = capture::capture();
    assert!(repair_tool_arguments(r#"{"a": "b""#).is_none());

    let mut rx = captured.events_rx;
    assert!(
        rx.try_recv().is_err(),
        "a failed repair (None) must not warn"
    );
}
