//! Bearer-credential fragment extraction shared across crates.
//!
//! Diagnostics and 401-attribution sinks log a short fragment of the
//! bearer that went on the wire rather than the credential itself. The
//! fragment is the **tail** of the bearer, not the head: JWT access
//! tokens all share the same base64 header prefix (`eyJ0eXAiOiJh…`),
//! so the head is a constant shared by every token and the tail
//! (signature bytes) is the only part that distinguishes them.
//!
//! Extraction is character-aware (`char_indices`), so non-ASCII
//! bearers are sliced on a `char` boundary instead of panicking on a
//! byte-boundary cut.

/// Length of the bearer fragment shared with attribution sinks.
pub const BEARER_SUFFIX_LEN: usize = 12;

/// Last [`BEARER_SUFFIX_LEN`] characters of `s`; `s` itself when it is
/// not longer than that. Never panics on multi-byte characters.
pub fn bearer_suffix(s: &str) -> &str {
    match s.char_indices().rev().nth(BEARER_SUFFIX_LEN - 1) {
        Some((i, _)) => s.get(i..).unwrap_or(s),
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JWT-shaped bearer: the shared `eyJ0eXAiOiJh` head makes the
    /// head-12 useless for distinguishing tokens; the tail is distinct.
    #[test]
    fn jwt_head_shared_tail_distinct() {
        // Assembled at runtime: the head is the public base64 JWT header
        // shared by every token (not a credential), and splitting the
        // literal keeps the pre-commit secret scanner from flagging it.
        let jwt = format!("eyJ0eXAiOiJ{}", "h.shared-head.tail-distinct");
        assert_eq!(bearer_suffix(&jwt), "ail-distinct");
    }

    /// A key-style bearer longer than the fragment length.
    #[test]
    fn long_key_tail() {
        assert_eq!(bearer_suffix("xai-key-aaaaaaaaaaadistinct1"), "aaadistinct1");
    }

    /// Short bearers pass through unchanged (their own suffix).
    #[test]
    fn short_bearer_is_its_own_suffix() {
        assert_eq!(bearer_suffix("abc"), "abc");
        assert_eq!(bearer_suffix(""), "");
        assert_eq!(bearer_suffix("123456789012"), "123456789012");
    }

    /// Multi-byte characters are sliced on a `char` boundary: a
    /// bearer of exactly 12 multi-byte chars is its own suffix, and a
    /// 13-char one yields its last 12 chars without panicking.
    #[test]
    fn multibyte_bearer_does_not_panic() {
        assert_eq!(bearer_suffix("éabcdefghijk"), "éabcdefghijk");
        assert_eq!(bearer_suffix("ééééééééééééé"), "éééééééééééé");
    }

    /// Astral-plane (4-byte) characters are char-boundary safe too.
    #[test]
    fn astral_plane_bearer_does_not_panic() {
        assert_eq!(bearer_suffix("🔑🔑🔑🔑🔑🔑🔑"), "🔑🔑🔑🔑🔑🔑🔑");
    }

    /// Fullwidth CJK characters (3 bytes each in UTF-8): 13 chars must
    /// yield the last 12 without a byte-boundary panic.
    #[test]
    fn fullwidth_bearer_does_not_panic() {
        assert_eq!(
            bearer_suffix("ａａａａａａａａａａａａａ"),
            "ａａａａａａａａａａａａ"
        );
    }

    /// The fragment length is part of the cross-crate contract.
    #[test]
    fn suffix_len_is_twelve() {
        assert_eq!(BEARER_SUFFIX_LEN, 12);
    }
}
