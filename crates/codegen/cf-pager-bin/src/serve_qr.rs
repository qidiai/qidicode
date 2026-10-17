//! QR pairing output for `qidi agent serve`.
//!
//! Prints a scannable QR code that encodes the **complete** WebSocket URL plus
//! the secret, so the mobile client does zero string assembly (it scans and
//! connects). Three rendering tiers, most capable first, with automatic
//! degradation so the pairing step never silently disappears on a console that
//! cannot render block glyphs:
//!
//! 1. UTF-8 full-block matrix (`█`/space) — the default. Half-block glyphs
//!    (`▀▄▌▐`) are deliberately **not** used because the Windows GBK console
//!    mangles them; `█` (U+2588) survives.
//! 2. ASCII art (`##`/space) — for terminals that mangle UTF-8.
//! 3. Plain-text URL + secret — the final fallback (also used when the QR
//!    payload is too long for the QR encoder).
//!
//! The degradation *decision* is a pure function (`select_tier`), so it is
//! unit-testable without a terminal.

use std::net::SocketAddr;

use qrcode::{Color, QrCode};

/// Rendering tiers, most capable first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrTier {
    /// UTF-8 full-block matrix (`█`/space).
    FullBlock,
    /// ASCII art (`##`/space).
    Ascii,
    /// Plain text URL + secret (no QR).
    PlainText,
}

/// Build the full pairing URL the QR encodes: `ws://<host>:<port>/ws?server-key=<secret>`.
///
/// The phone scans this and connects directly — it must never join the scheme,
/// host, port, path, or secret itself (GLM free-review finding: a client-side
/// join is where `ws://` vs `wss://` and missing `/ws` mismatches creep in).
pub fn pairing_url(bind_addr: SocketAddr, secret: &str) -> String {
    format!("ws://{}/ws?server-key={secret}", bind_addr)
}

/// Pure tier selection: full-block when the console can render UTF-8 block
/// glyphs, ASCII otherwise. Plain text is reserved for QR-generation failure
/// (see [`render_qr`]), not a console-capability tier.
pub fn select_tier(can_render_full_block: bool) -> QrTier {
    if can_render_full_block {
        QrTier::FullBlock
    } else {
        QrTier::Ascii
    }
}

/// Detect the best tier for the current environment.
///
/// Full-block UTF-8 is the default (any UTF-8-capable terminal). Operators on
/// legacy GBK/codepage consoles can force a downgrade with `QIDI_QR=ascii`
/// (ASCII art) or `QIDI_QR=text` (plain text).
pub fn detect_tier() -> QrTier {
    tier_from_env(std::env::var("QIDI_QR").ok().as_deref())
}

/// Pure mapping from the `QIDI_QR` env value to a tier (kept separate from
/// [`detect_tier`] so the degradation decision is testable without mutating
/// process env — `std::env::set_var` is `unsafe` under edition 2024).
fn tier_from_env(value: Option<&str>) -> QrTier {
    match value {
        Some("ascii") => QrTier::Ascii,
        Some("text") | Some("none") | Some("off") => QrTier::PlainText,
        _ => QrTier::FullBlock,
    }
}

/// Render the QR for `url` at `tier`, degrading to plain text if the payload
/// is too long for the QR encoder (which is the honest final fallback).
pub fn render_qr(url: &str, tier: QrTier) -> String {
    if tier == QrTier::PlainText {
        return url.to_string();
    }
    let Ok(code) = QrCode::new(url.as_bytes()) else {
        // Data too long / cannot encode → plain text fallback.
        return url.to_string();
    };
    let colors = code.to_colors();
    match tier {
        QrTier::FullBlock => render_matrix(code.width(), &colors, full_block_module),
        QrTier::Ascii => render_matrix(code.width(), &colors, ascii_module),
        QrTier::PlainText => url.to_string(),
    }
}

/// Render a QR pairing banner for a given bind address + secret at the detected
/// (or explicit) tier.
pub fn render_pairing(bind_addr: SocketAddr, secret: &str, tier: QrTier) -> String {
    render_qr(&pairing_url(bind_addr, secret), tier)
}

fn full_block_module(c: Color) -> &'static str {
    c.select("██", "  ")
}

fn ascii_module(c: Color) -> &'static str {
    c.select("##", "  ")
}

fn render_matrix<F>(width: usize, colors: &[Color], module: F) -> String
where
    F: Fn(Color) -> &'static str,
{
    colors
        .chunks(width)
        .map(|row| row.iter().map(|&c| module(c)).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_url_is_complete_and_self_contained() {
        let url = pairing_url("127.0.0.1:2419".parse().unwrap(), "abc123");
        assert_eq!(url, "ws://127.0.0.1:2419/ws?server-key=abc123");
        assert!(url.starts_with("ws://"));
        assert!(url.contains("/ws?server-key="));
    }

    #[test]
    fn select_tier_prefers_full_block() {
        assert_eq!(select_tier(true), QrTier::FullBlock);
        assert_eq!(select_tier(false), QrTier::Ascii);
    }

    #[test]
    fn full_block_renders_only_block_glyphs() {
        let url = pairing_url("127.0.0.1:2419".parse().unwrap(), "x");
        let out = render_qr(&url, QrTier::FullBlock);
        // Rendered output is square-ish and uses only full block + space.
        assert!(!out.is_empty());
        for c in out.chars() {
            assert!(c == '█' || c == ' ' || c == '\n', "unexpected char: {c:?}");
        }
        assert!(out.contains('█'));
    }

    #[test]
    fn ascii_renders_only_ascii() {
        let url = pairing_url("127.0.0.1:2419".parse().unwrap(), "x");
        let out = render_qr(&url, QrTier::Ascii);
        assert!(out.is_ascii());
        assert!(out.contains('#'));
    }

    #[test]
    fn plain_text_returns_url_verbatim() {
        let url = "ws://10.0.0.5:2419/ws?server-key=secret";
        assert_eq!(render_qr(url, QrTier::PlainText), url);
    }

    #[test]
    fn full_block_and_ascii_have_same_dimensions() {
        let url = pairing_url("127.0.0.1:2419".parse().unwrap(), "abcdef");
        let full = render_qr(&url, QrTier::FullBlock);
        let ascii = render_qr(&url, QrTier::Ascii);
        assert_eq!(full.lines().count(), ascii.lines().count());
        for (a, b) in full.lines().zip(ascii.lines()) {
            assert_eq!(a.chars().count(), b.chars().count());
        }
    }

    #[test]
    fn tier_from_env_maps_overrides_and_defaults() {
        // No env → full-block.
        assert_eq!(tier_from_env(None), QrTier::FullBlock);
        assert_eq!(tier_from_env(Some("")), QrTier::FullBlock);
        assert_eq!(tier_from_env(Some("unknown")), QrTier::FullBlock);
        // Explicit downgrades.
        assert_eq!(tier_from_env(Some("ascii")), QrTier::Ascii);
        assert_eq!(tier_from_env(Some("text")), QrTier::PlainText);
        assert_eq!(tier_from_env(Some("none")), QrTier::PlainText);
        assert_eq!(tier_from_env(Some("off")), QrTier::PlainText);
    }
}
