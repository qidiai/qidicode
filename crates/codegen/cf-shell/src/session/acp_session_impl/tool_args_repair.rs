//! Conservative repair of malformed tool-call argument JSON.
//!
//! Some sampling providers deterministically drop the quotes around a bare
//! token when generating native function-call arguments — e.g. the model emits
//! `"model": KIMI-K3-222` instead of `"model": "KIMI-K3-222"`. The result is
//! invalid JSON that `serde_json` rejects on every retry, so the model can
//! never self-correct (the corruption is a function of the token sequence, not
//! of the model's intent). Observed on the GLM-5.3 line with slugs such as
//! `KIMI-K3-222`, `GLM-5.3-222` and `kimi-k2.7-code-highspeed-222`.
//!
//! [`repair_tool_arguments`] performs a *single*, *conservative* repair pass:
//! it quotes bare tokens that can only have been intended as strings. It is
//! only ever consulted **after** the ordinary parse has already failed, and it
//! returns a value **only when the repaired text re-parses as valid JSON** — so
//! it can never turn a parse failure into a silently-wrong-but-valid payload,
//! and it has zero effect on arguments that already parsed.

/// One conservative repair pass over raw tool-call arguments whose JSON parse
/// failed: quote bare tokens that can only have been intended as strings.
/// Returns `Some(repaired)` only when the repaired text parses as valid JSON;
/// otherwise `None` (caller falls through to the existing error path).
///
/// The input string is never modified; the repair is built into a fresh buffer.
pub(super) fn repair_tool_arguments(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let n = bytes.len();
    // A little slack for the inserted quote pairs.
    let mut out = String::with_capacity(n + 16);
    let mut i = 0;
    while i < n {
        match bytes[i] {
            // Copy string literals verbatim (honouring `\` escapes) so that
            // separators inside a value's text are never mistaken for syntax.
            b'"' => {
                let start = i;
                i += 1;
                while i < n {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                let end = i.min(n);
                out.push_str(&raw[start..end]);
                i = end;
            }
            // A key can only follow `{` or `,`.
            b'{' | b',' => {
                out.push(char::from(bytes[i]));
                i += 1;
                i = repair_key(raw, bytes, i, n, &mut out);
            }
            // A value follows `:`.
            b':' => {
                out.push(':');
                i += 1;
                i = repair_value(raw, bytes, i, n, &mut out);
            }
            _ => {
                // Copy whole UTF-8 chars (not raw bytes) to keep slices valid.
                let ch = raw[i..].chars().next().expect("index is within bounds");
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }

    // Adopt the repair only when it actually parses.
    if serde_json::from_str::<serde_json::Value>(&out).is_ok() {
        // A silent, best-effort fix must still be visible: emit a single warn
        // (with a truncated summary of the original) so the repair never
        // swallows an upstream bug quietly. It stays a WARN rather than an
        // error because the call succeeds.
        tracing::warn!(
            original = %summarize(raw),
            "Repaired malformed tool arguments JSON (unquoted token); \
             parsed after one conservative repair pass"
        );
        Some(out)
    } else {
        None
    }
}

/// UTF-8-safe truncation for the repair warn (keeps the log line small).
fn summarize(s: &str) -> String {
    const MAX: usize = 160;
    if s.len() <= MAX {
        return s.to_string();
    }
    let mut end = MAX;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\u{2026}", &s[..end])
}

/// After `{` or `,`, quote a bare identifier used as an object key
/// (`name:` → `"name":`). Intervening whitespace is preserved verbatim.
/// Returns the index to resume scanning from.
fn repair_key(raw: &str, bytes: &[u8], mut i: usize, n: usize, out: &mut String) -> usize {
    let ws_start = i;
    while i < n && is_json_ws(bytes[i]) {
        i += 1;
    }
    out.push_str(&raw[ws_start..i]);

    if let Some((s, e)) = match_ident(bytes, i, n) {
        // The token is a key only if it is immediately followed by `:`.
        let mut j = e;
        while j < n && is_json_ws(bytes[j]) {
            j += 1;
        }
        if j < n && bytes[j] == b':' {
            out.push('"');
            out.push_str(&raw[s..e]);
            out.push('"');
            return e;
        }
    }
    i
}

/// After `:`, quote a bare identifier used as a string value (`x` → `"x"`).
/// JSON literals (`true`/`false`/`null`) are left untouched, and the token must
/// be terminated by `,`/`}`/`]` (or end-of-input) to qualify.
/// Returns the index to resume scanning from.
fn repair_value(raw: &str, bytes: &[u8], mut i: usize, n: usize, out: &mut String) -> usize {
    let ws_start = i;
    while i < n && is_json_ws(bytes[i]) {
        i += 1;
    }
    out.push_str(&raw[ws_start..i]);

    if let Some((s, e)) = match_ident(bytes, i, n) {
        let mut j = e;
        while j < n && is_json_ws(bytes[j]) {
            j += 1;
        }
        let terminated = j >= n || matches!(bytes[j], b',' | b'}' | b']');
        let word = &raw[s..e];
        if terminated && !matches!(word, "true" | "false" | "null") {
            out.push('"');
            out.push_str(word);
            out.push('"');
            return e;
        }
    }
    i
}

/// Match a bare identifier token (`[A-Za-z_][A-Za-z0-9_\-.]*`) at `i`.
/// Returns its `(start, end)` byte range, or `None` if none starts here.
fn match_ident(bytes: &[u8], i: usize, n: usize) -> Option<(usize, usize)> {
    if i >= n || !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
        return None;
    }
    let start = i;
    let mut k = i + 1;
    while k < n && is_ident_continue(bytes[k]) {
        k += 1;
    }
    Some((start, k))
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')
}

fn is_json_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}
