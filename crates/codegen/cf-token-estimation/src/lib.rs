//! Pure shared token-estimation primitives.
//!
//! This crate is the single source of truth for the bytes/4 heuristic and the
//! derived-display arithmetic that `/context`, `/session-info`, the auto-compact
//! gates, the preflight overflow check, and every client renderer use to talk
//! about context-window usage.

/// Bytes per token under the rough character-based heuristic.
pub const BYTES_PER_TOKEN: u64 = 4;

/// Per-image approximate token cost when summing
/// low-resolution image patches.
pub const IMAGE_TOKEN_ESTIMATE: u64 = 765;

/// Bytes/4 estimate of a string's token count.
#[inline]
pub fn estimate_tokens(s: &str) -> u64 {
    (s.len() as u64) / BYTES_PER_TOKEN
}

/// Numerator of the per-CJK-character token ratio used by
/// [`estimate_tokens_aware`]: each CJK character counts as
/// `CJK_TOKEN_NUMERATOR / CJK_TOKEN_DENOMINATOR` tokens.
///
/// Measured Chinese text tokenizes at roughly 0.5–0.6 token/字, while
/// UTF-8 encodes those characters at 3 bytes each — so the plain
/// bytes/4 heuristic over-estimates Chinese by ~30–50% (token
/// optimization plan §5-1). We take 3/5 = 0.6, the conservative
/// (higher) end of the measured band:宁可高估（早压缩）不可低估
/// （溢出）, per plan §6 gate 4. Exposed as a constant so the ratio
/// can be re-tuned against a real tokenizer without touching call
/// sites.
pub const CJK_TOKEN_NUMERATOR: u64 = 3;

/// Denominator of the per-CJK-character token ratio
/// ([`CJK_TOKEN_NUMERATOR`] / [`CJK_TOKEN_DENOMINATOR`] tokens per
/// CJK character). See [`CJK_TOKEN_NUMERATOR`] for the rationale.
pub const CJK_TOKEN_DENOMINATOR: u64 = 5;

/// True when `c` falls in one of the CJK Unicode blocks counted by
/// [`estimate_tokens_aware`]: CJK radicals and Han (U+2E80–U+9FFF,
/// which also covers CJK punctuation U+3000–U+303F and Extension A
/// U+3400–U+4DBF), compatibility ideographs (U+F900–U+FAFF), and
/// fullwidth forms (U+FF00–U+FFEF).
fn is_cjk(c: char) -> bool {
    // U+3000–303F（CJK 标点）与 Extension A（U+3400–4DBF）均为
    // U+2E80–U+9FFF 的子区间，故不单列——单列反而是不可达模式。
    matches!(
        c as u32,
        0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF
    )
}

/// Character-aware token estimate: ASCII bytes at [`BYTES_PER_TOKEN`],
/// CJK characters at [`CJK_TOKEN_NUMERATOR`] / [`CJK_TOKEN_DENOMINATOR`]
/// tokens each (ceiling of the aggregate), and every other non-ASCII
/// byte at [`BYTES_PER_TOKEN`].
///
/// Formula: `ascii_len / 4 + ceil(cjk_chars * 3 / 5) + other_bytes / 4`.
///
/// The bytes/4 heuristic ([`estimate_tokens`]) assumes 4 bytes per
/// token, but a CJK character costs 3 UTF-8 bytes while tokenizing at
/// ~0.5–0.6 tokens/字， so bytes/4 over-counts Chinese text by
/// ~30–50%. This variant splits the string into ASCII / CJK / other
/// and applies the appropriate rate to each class; pure-ASCII input
/// yields exactly the same value as [`estimate_tokens`] for a single
/// string, so ASCII-only callers see no change at the string level.
/// The equality is per string: callers that fold the estimate over
/// several segments (such as `estimate_item_tokens` in cf-chat-state,
/// which sums per user text part, per assistant content and per
/// tool-call arguments) floor each segment separately, so a
/// multi-segment item can land up to (segments − 1) tokens below
/// the old merged Σbytes/4 figure — pure ASCII included.
///
/// Other non-ASCII scripts and emoji are counted as raw bytes at
/// [`BYTES_PER_TOKEN`] — their per-script token ratios are not
/// measured here, and the bytes/4 default stays on the conservative
/// side for them.
#[inline]
pub fn estimate_tokens_aware(s: &str) -> u64 {
    let mut ascii_len: u64 = 0;
    let mut cjk_chars: u64 = 0;
    let mut other_bytes: u64 = 0;
    for c in s.chars() {
        if c.is_ascii() {
            ascii_len += 1;
        } else if is_cjk(c) {
            cjk_chars += 1;
        } else {
            other_bytes += c.len_utf8() as u64;
        }
    }
    // ceil(cjk * 3 / 5) in integer arithmetic: (x + d - 1) / d.
    let cjk_tokens = (cjk_chars
        .saturating_mul(CJK_TOKEN_NUMERATOR)
        .saturating_add(CJK_TOKEN_DENOMINATOR - 1))
        / CJK_TOKEN_DENOMINATOR;
    ascii_len / BYTES_PER_TOKEN + cjk_tokens + other_bytes / BYTES_PER_TOKEN
}

/// Inverse of [`estimate_tokens`]: convert a token budget into a character
/// budget. Used by skill discovery to size text passages against the model's
/// context window.
#[inline]
pub fn estimate_chars(tokens: u64) -> u64 {
    tokens.saturating_mul(BYTES_PER_TOKEN)
}

/// Token estimate for `image_count` images at [`IMAGE_TOKEN_ESTIMATE`] each.
#[inline]
pub fn estimate_image_tokens(image_count: u64) -> u64 {
    image_count.saturating_mul(IMAGE_TOKEN_ESTIMATE)
}

/// Usage percentage as `f64`, clamped to `100.0`. Returns `0.0` when
/// `total == 0`.
#[inline]
pub fn usage_percentage(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        ((used as f64) / (total as f64) * 100.0).min(100.0)
    }
}

/// Usage percentage rounded to `u8`, clamped to `100`.
#[inline]
pub fn usage_percentage_u8(used: u64, total: u64) -> u8 {
    usage_percentage(used, total).round() as u8
}

/// Integer-arithmetic (truncating) usage percentage, clamped to `100`.
///
/// Differs from [`usage_percentage_u8`] in two ways: no `f64` round-trip,
/// and the result is **truncated** (not rounded).
///
/// Returns `u8` because the result is bounded to `100`. Saturates on
/// overflow via `saturating_mul`.
#[inline]
pub fn usage_percentage_truncated_u8(used: u64, total: u64) -> u8 {
    if total == 0 {
        0
    } else {
        ((used.saturating_mul(100) / total).min(100)) as u8
    }
}

/// `total - used`, saturating at zero. The "free" portion of the context
/// window for `/context` rendering.
#[inline]
pub fn free_tokens(total: u64, used: u64) -> u64 {
    total.saturating_sub(used)
}

/// True when `used >= context_window * threshold_percent / 100`. Returns
/// `false` for `context_window == 0` so callers do not have to special-case
/// missing windows. Computed in integer arithmetic to match the existing
/// auto-compact gate semantics.
#[inline]
pub fn exceeds_threshold(used: u64, context_window: u64, threshold_percent: u8) -> bool {
    if context_window == 0 {
        return false;
    }
    used.saturating_mul(100) >= context_window.saturating_mul(threshold_percent as u64)
}

/// True when `used * 100 >= context_window * threshold_percent - headroom * 100`,
/// the scaled form of [`exceeds_threshold`] minus a token headroom.
/// Returns `false` for `context_window == 0`.
#[inline]
pub fn exceeds_threshold_with_headroom(
    used: u64,
    context_window: u64,
    threshold_percent: u8,
    headroom: u64,
) -> bool {
    if context_window == 0 {
        return false;
    }
    used.saturating_mul(100)
        >= context_window
            .saturating_mul(threshold_percent as u64)
            .saturating_sub(headroom.saturating_mul(100))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_is_bytes_over_four() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abc"), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens(&"x".repeat(4000)), 1000);
    }

    #[test]
    fn cjk_ratio_constants_expose_the_tunable() {
        // Locks the current tuning surface; re-tuning against a real
        // tokenizer updates these two numbers deliberately.
        assert_eq!(CJK_TOKEN_NUMERATOR, 3);
        assert_eq!(CJK_TOKEN_DENOMINATOR, 5);
    }

    #[test]
    fn estimate_tokens_aware_matches_bytes_over_four_for_ascii() {
        // Pure ASCII never diverges from the legacy heuristic.
        assert_eq!(estimate_tokens_aware(""), 0);
        assert_eq!(estimate_tokens_aware("abc"), 0);
        assert_eq!(estimate_tokens_aware("abcd"), 1);
        assert_eq!(estimate_tokens_aware("hello world"), estimate_tokens("hello world"));
        assert_eq!(estimate_tokens_aware(&"x".repeat(4000)), 1000);
    }

    #[test]
    fn estimate_tokens_aware_chinese_sits_in_measured_band() {
        // 100 CJK chars -> ceil(100 * 3 / 5) = 60 tokens, i.e. 0.6
        // token/字. Assert the measured 0.5–0.6 band (50..=62) rather
        // than the exact value, and that we stay strictly below the
        // bytes/4 over-estimate (100 * 3 bytes / 4 = 75).
        let s = "你".repeat(100);
        let t = estimate_tokens_aware(&s);
        assert!(
            (50..=62).contains(&t),
            "100 CJK chars should land in the 0.5-0.6 token/字 band, got {t}"
        );
        assert!(t < estimate_tokens(&s), "aware must under-cut the bytes/4 over-estimate");
    }

    #[test]
    fn estimate_tokens_aware_mixed_ascii_and_cjk() {
        // 你好世界 = 4 CJK -> ceil(12/5) = 3; "hello" = 5 ASCII bytes
        // -> 5/4 = 1. Total 4.
        assert_eq!(estimate_tokens_aware("你好世界hello"), 4);
        // Trailing CJK rounds up on its own aggregate: 1 CJK ->
        // ceil(3/5) = 1, plus "ab" -> 0.
        assert_eq!(estimate_tokens_aware("ab你"), 1);
    }

    #[test]
    fn estimate_tokens_aware_empty() {
        assert_eq!(estimate_tokens_aware(""), 0);
    }

    #[test]
    fn estimate_tokens_aware_treats_emoji_as_other_bytes() {
        // 🙂 (U+1F642) is 4 UTF-8 bytes and outside every counted CJK
        // block, so it falls in the "other" bucket at bytes/4.
        assert_eq!(estimate_tokens_aware("🙂"), 1);
        assert_eq!(estimate_tokens_aware("🙂🙂"), 2);
    }

    #[test]
    fn estimate_tokens_aware_counts_fullwidth_punctuation_as_cjk() {
        // ，= U+FF0C and ！= U+FF01 (fullwidth forms), 。= U+3002 (CJK
        // punctuation): 3 CJK chars -> ceil(9/5) = 2.
        assert_eq!(estimate_tokens_aware("，。！"), 2);
    }

    #[test]
    fn estimate_tokens_aware_cjk_block_boundaries() {
        // U+2E80 (first CJK radical) and U+9FFF (last Han) are both
        // counted: 2 CJK -> ceil(6/5) = 2.
        assert_eq!(estimate_tokens_aware("\u{2E80}\u{9FFF}"), 2);
        // U+A000 (Yi syllable) is NOT in any counted block -> other,
        // 3 bytes -> 0 tokens at bytes/4.
        assert_eq!(estimate_tokens_aware("\u{A000}"), 0);
        // One non-CJK char plus one Han char: ceil(3/5) = 1 + 0.
        assert_eq!(estimate_tokens_aware("\u{A000}\u{4E00}"), 1);
        // U+3000 (ideographic space, CJK punctuation block) counts.
        assert_eq!(estimate_tokens_aware("\u{3000}"), 1);
    }

    #[test]
    fn estimate_tokens_aware_counts_cjk_punctuation_via_han_arm() {
        // U+3001 (、) and U+3002 (。) sit in the CJK punctuation
        // block U+3000–U+303F, which is a sub-range of the
        // U+2E80–U+9FFF arm (not listed separately): they must
        // still be counted as CJK via that arm. 2 CJK chars ->
        // ceil(6/5) = 2.
        assert_eq!(estimate_tokens_aware("\u{3001}\u{3002}"), 2);
    }

    #[test]
    fn estimate_chars_is_inverse() {
        assert_eq!(estimate_chars(0), 0);
        assert_eq!(estimate_chars(1), 4);
        assert_eq!(estimate_chars(1000), 4000);
    }

    #[test]
    fn estimate_image_tokens_uses_constant() {
        assert_eq!(estimate_image_tokens(0), 0);
        assert_eq!(estimate_image_tokens(1), IMAGE_TOKEN_ESTIMATE);
        assert_eq!(estimate_image_tokens(3), 3 * IMAGE_TOKEN_ESTIMATE);
    }

    #[test]
    fn usage_percentage_clamps_and_handles_zero_total() {
        assert_eq!(usage_percentage(0, 0), 0.0);
        assert_eq!(usage_percentage(50, 100), 50.0);
        assert_eq!(usage_percentage(150, 100), 100.0);
        assert_eq!(usage_percentage(100, 0), 0.0);
    }

    #[test]
    fn usage_percentage_u8_rounds() {
        assert_eq!(usage_percentage_u8(0, 100), 0);
        assert_eq!(usage_percentage_u8(50, 100), 50);
        assert_eq!(usage_percentage_u8(99, 100), 99);
        // 12_700 / 256_000 = 0.04960... -> 5 after rounding
        assert_eq!(usage_percentage_u8(12_700, 256_000), 5);
        assert_eq!(usage_percentage_u8(150, 100), 100);
    }

    /// Half-boundary contract — locks rounding direction. `85 / 200 = 0.425`
    /// becomes `42.5%` which rounds half-up to `43`. The truncating helper
    /// returns `42` for the same input (see `usage_percentage_truncated_u8`).
    #[test]
    fn usage_percentage_u8_rounds_half_up() {
        assert_eq!(usage_percentage_u8(85, 200), 43);
        // 7 / 8 = 0.875, rounds to 88 (truncated would be 87).
        assert_eq!(usage_percentage_u8(7, 8), 88);
    }

    #[test]
    fn usage_percentage_truncated_u8_clamps_and_handles_zero_total() {
        assert_eq!(usage_percentage_truncated_u8(0, 0), 0);
        assert_eq!(usage_percentage_truncated_u8(50, 100), 50);
        assert_eq!(usage_percentage_truncated_u8(150, 100), 100);
        // Large values do not overflow because we use saturating_mul.
        assert_eq!(usage_percentage_truncated_u8(u64::MAX, 1), 100);
    }

    /// Truncation contract — distinguishes this helper from
    /// `usage_percentage_u8`, which rounds. Locks in that
    /// `exceeds_threshold(used, cw, p)` and
    /// `usage_percentage_truncated_u8(used, cw) >= p` agree.
    #[test]
    fn usage_percentage_truncated_u8_truncates_does_not_round() {
        // 85 / 200 = 0.425, truncated -> 42 (rounded would be 43).
        assert_eq!(usage_percentage_truncated_u8(85, 200), 42);
        // 7 / 8 = 0.875, truncated -> 87 (rounded would be 88).
        assert_eq!(usage_percentage_truncated_u8(7, 8), 87);
    }

    #[test]
    fn free_tokens_saturates() {
        assert_eq!(free_tokens(100, 30), 70);
        assert_eq!(free_tokens(100, 100), 0);
        assert_eq!(free_tokens(100, 200), 0);
    }

    #[test]
    fn exceeds_threshold_matches_integer_pct() {
        assert!(!exceeds_threshold(50, 100, 85));
        assert!(exceeds_threshold(85, 100, 85));
        assert!(exceeds_threshold(99, 100, 85));
        assert!(!exceeds_threshold(50, 0, 85));
    }

    /// Strict-boundary contract — pin the `>=` semantics. At cw=1000,
    /// pct=85, `850 * 100 == 1000 * 85` so the gate must fire at exactly
    /// 850 tokens. This is one token earlier than the legacy `>` gate
    /// (`total > cw * pct / 100` which fired at 851).
    #[test]
    fn exceeds_threshold_fires_on_strict_boundary() {
        assert!(exceeds_threshold(850, 1000, 85));
        assert!(!exceeds_threshold(849, 1000, 85));
        // 1000 * 85 / 100 = 850, so 850 is the new strict boundary.
        // Same shape at the other commonly-configured threshold (95%):
        assert!(exceeds_threshold(950, 1000, 95));
        assert!(!exceeds_threshold(949, 1000, 95));
    }

    /// Property: with `headroom == 0` the helper agrees with
    /// [`exceeds_threshold`] across a representative grid of inputs,
    /// including the non-round windows where floor-divide drifts.
    #[test]
    fn exceeds_threshold_with_headroom_zero_headroom_matches_exceeds_threshold() {
        for cw in [0_u64, 1, 50, 100, 101, 1024, 100_000, 128_001, 1_000_001] {
            for pct in [0_u8, 1, 50, 85, 99, 100] {
                for used in [
                    0_u64,
                    1,
                    cw / 2,
                    cw.saturating_sub(1),
                    cw,
                    cw + 1,
                    cw + 1000,
                ] {
                    assert_eq!(
                        exceeds_threshold_with_headroom(used, cw, pct, 0),
                        exceeds_threshold(used, cw, pct),
                        "mismatch at used={used} cw={cw} pct={pct}",
                    );
                }
            }
        }
    }

    #[test]
    fn exceeds_threshold_with_headroom_subtracts_headroom() {
        // 100K window, 85% threshold = 85_000. Headroom 4_000 -> fires at 81_000.
        assert!(!exceeds_threshold_with_headroom(80_999, 100_000, 85, 4_000));
        assert!(exceeds_threshold_with_headroom(81_000, 100_000, 85, 4_000));
    }

    #[test]
    fn exceeds_threshold_with_headroom_zero_window() {
        assert!(!exceeds_threshold_with_headroom(0, 0, 85, 0));
        assert!(!exceeds_threshold_with_headroom(100, 0, 85, 4_000));
    }

    #[test]
    fn exceeds_threshold_with_headroom_headroom_larger_than_threshold_saturates() {
        // 100K * 85% = 85_000 (8_500_000 scaled). Headroom 1M tokens scales to
        // 100_000_000 — saturating sub yields 0, so any used fires.
        assert!(exceeds_threshold_with_headroom(0, 100_000, 85, 1_000_000));
    }
}
