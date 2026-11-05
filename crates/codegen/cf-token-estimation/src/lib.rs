//! Pure shared token-estimation primitives.
//!
//! This crate is the single source of truth for the bytes/4 heuristic and the
//! derived-display arithmetic that `/context`, `/session-info`, the auto-compact
//! gates, the preflight overflow check, and every client renderer use to talk
//! about context-window usage.
//!
//! Two rates live here, on purpose:
//! - the legacy bytes/4 heuristic ([`estimate_tokens`] / [`estimate_chars`] /
//!   [`BYTES_PER_TOKEN`]), kept uncalibrated for truncation and budget callers;
//! - the calibrated character-aware estimate ([`estimate_tokens_aware`]:
//!   3.4 ASCII chars/token + 4/5 token per CJK character), which the context
//!   usage display and the compact/preflight gates use.

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

/// Numerator of the per-ASCII-character token ratio used by
/// [`estimate_tokens_aware`]: `ASCII_TOKEN_NUMERATOR /
/// ASCII_TOKEN_DENOMINATOR` tokens per ASCII character = 10/34 ≈ 0.294
/// token/char, i.e. **3.4 ASCII characters per token** (floored).
///
/// Calibrated against the official DeepSeek V4.1 vocabulary: the real
/// ASCII workload (tool output / JSON / code / markdown) tokenizes at
/// ~3.1 chars/token, and the stratified calibration corpus (40 sessions,
/// 6,376 items, 3.91M reference tokens) showed the old bytes/4 (4
/// chars/token) figure under-reporting the aggregate by ~18% (measured
/// estimate/reference = 0.819). 3.4 is deliberately on the
/// conservative (higher-token) side of the measured ASCII band:
/// 宁可高估（早压缩）不可低估（溢出）, per token-optimization plan §6
/// gate 4. Exposed as a constant so the ratio can be re-tuned against a
/// real tokenizer without touching call sites.
///
/// Note: [`BYTES_PER_TOKEN`] / [`estimate_tokens`] / [`estimate_chars`]
/// are **not** calibrated and keep the raw 4-bytes-per-token contract
/// for their own callers (read_file truncation, web_fetch budgets,
/// tool-definition sizing, base64 counting).
pub const ASCII_TOKEN_NUMERATOR: u64 = 10;

/// Denominator of the per-ASCII-character token ratio
/// ([`ASCII_TOKEN_NUMERATOR`] / `ASCII_TOKEN_DENOMINATOR` tokens per
/// ASCII character). See [`ASCII_TOKEN_NUMERATOR`] for the calibration
/// rationale.
pub const ASCII_TOKEN_DENOMINATOR: u64 = 34;

/// Numerator of the per-CJK-character token ratio used by
/// [`estimate_tokens_aware`]: each CJK character counts as
/// `CJK_TOKEN_NUMERATOR / CJK_TOKEN_DENOMINATOR` tokens = 4/5 = 0.8
/// token/字 (ceiling of the aggregate).
///
/// Calibrated jointly with [`ASCII_TOKEN_NUMERATOR`]: with ASCII at 3.4
/// chars/token, 4/5 is the CJK rate that pulls the stratified-corpus
/// aggregate estimate/reference ratio from 0.819 to ~0.981. Pure-Chinese
/// text on its own measures lower (~0.57 token/字), but Chinese rarely
/// appears without the ASCII scaffolding it is priced against, and the
/// old 3/5 (0.6) left the mixed-content aggregate 18% short. Erring high
/// keeps the compact/preflight gates on the safe side (宁可高估（早压缩）
/// 不可低估（溢出）, plan §6 gate 4). Exposed as a constant so the ratio
/// can be re-tuned against a real tokenizer without touching call
/// sites.
pub const CJK_TOKEN_NUMERATOR: u64 = 4;

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
    // CJK 标点 U+3000–303F 与 Extension A U+3400–4DBF 均为
    // U+2E80–U+9FFF 的子区间，单列只会冗余；若排在 Han 臂之后
    // 还会触发 unreachable_patterns。
    matches!(
        c as u32,
        0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF
    )
}

/// Character-aware token estimate: ASCII characters at
/// [`ASCII_TOKEN_NUMERATOR`] / [`ASCII_TOKEN_DENOMINATOR`] tokens each
/// (≈3.4 chars/token, floor of the aggregate), CJK characters at
/// [`CJK_TOKEN_NUMERATOR`] / [`CJK_TOKEN_DENOMINATOR`] tokens each
/// (≈0.8 token/字, ceiling of the aggregate), and every other non-ASCII
/// byte at [`BYTES_PER_TOKEN`].
///
/// Formula: `floor(ascii_len * 10 / 34) + ceil(cjk_chars * 4 / 5) +
/// other_bytes / 4`.
///
/// The bytes/4 heuristic ([`estimate_tokens`]) assumes 4 bytes per
/// token. Measured against the official DeepSeek V4.1 vocabulary the
/// real ASCII workload — tool output, JSON, code, markdown — runs at
/// ~3.1–3.4 characters per token, so bytes/4 **under**-counts it by
/// ~18%; on the stratified calibration corpus (3.91M reference tokens)
/// the old estimate covered only 0.819 of the truth, which is what
/// pushed the auto-compact gate from its configured threshold up to
/// ~89–93% of the real window. This variant splits the string into
/// ASCII / CJK / other and applies the calibrated rate to each class,
/// landing the same corpus at ~0.981.
///
/// It therefore **diverges** from [`estimate_tokens`] for ASCII input
/// (3.4 vs 4 chars/token). That is deliberate: `estimate_tokens` /
/// [`estimate_chars`] keep the raw bytes/4 contract for their own
/// callers (read_file truncation, web_fetch budgets, tool-definition
/// sizing, base64 counting) — only this function is calibrated.
///
/// Callers that fold the estimate over several segments (such as
/// `estimate_item_tokens` in cf-chat-state, which sums per user text
/// part, per assistant content and per tool-call arguments) floor each
/// ASCII/other-bytes segment separately and ceil each CJK segment
/// separately, so a multi-segment item can land a couple of tokens
/// below the merged single-pass figure.
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
    // floor(ascii * 10 / 34): integer division truncates.
    let ascii_tokens = ascii_len.saturating_mul(ASCII_TOKEN_NUMERATOR) / ASCII_TOKEN_DENOMINATOR;
    // ceil(cjk * 4 / 5) in integer arithmetic: (x + d - 1) / d.
    let cjk_tokens = (cjk_chars
        .saturating_mul(CJK_TOKEN_NUMERATOR)
        .saturating_add(CJK_TOKEN_DENOMINATOR - 1))
        / CJK_TOKEN_DENOMINATOR;
    ascii_tokens + cjk_tokens + other_bytes / BYTES_PER_TOKEN
}

/// Inverse of [`estimate_tokens`]: convert a token budget into a character
/// budget. Used by skill discovery to size text passages against the model's
/// context window.
#[inline]
pub fn estimate_chars(tokens: u64) -> u64 {
    tokens.saturating_mul(BYTES_PER_TOKEN)
}

/// Inverse of the pure-ASCII branch of [`estimate_tokens_aware`]: return the
/// minimum ASCII character/byte budget whose floored estimate reaches
/// `tokens`. CJK and mixed-content estimates are not exact inverses of this
/// function.
#[inline]
pub fn estimate_chars_aware(tokens: u64) -> u64 {
    tokens
        .saturating_mul(ASCII_TOKEN_DENOMINATOR)
        .div_ceil(ASCII_TOKEN_NUMERATOR)
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
        // tokenizer updates these numbers deliberately.
        assert_eq!(CJK_TOKEN_NUMERATOR, 4);
        assert_eq!(CJK_TOKEN_DENOMINATOR, 5);
        assert_eq!(ASCII_TOKEN_NUMERATOR, 10);
        assert_eq!(ASCII_TOKEN_DENOMINATOR, 34);
    }

    #[test]
    fn estimate_tokens_aware_ascii_uses_calibrated_rate() {
        // Pure ASCII runs at 3.4 chars/token: floor(chars * 10 / 34).
        assert_eq!(estimate_tokens_aware(""), 0);
        assert_eq!(estimate_tokens_aware("abc"), 0); // floor(30/34)
        assert_eq!(estimate_tokens_aware("abcd"), 1); // floor(40/34)
        assert_eq!(estimate_tokens_aware("hello world"), 3); // floor(110/34)
        assert_eq!(estimate_tokens_aware(&"x".repeat(4000)), 1176); // floor(40_000/34)
        // The calibrated rate deliberately reports MORE than bytes/4 for
        // ASCII — that gap is the ~18% under-count being fixed.
        assert!(
            estimate_tokens_aware("hello world") > estimate_tokens("hello world"),
            "calibrated ASCII rate must exceed the bytes/4 under-estimate"
        );
    }

    #[test]
    fn estimate_tokens_aware_cjk_uses_calibrated_rate() {
        // 100 CJK chars -> ceil(100 * 4 / 5) = 80 tokens, i.e. 0.8
        // token/字 — above the 75 the raw bytes/4 heuristic reports.
        let s = "你".repeat(100);
        let t = estimate_tokens_aware(&s);
        assert_eq!(t, 80, "100 CJK chars must price at ceil(400/5) tokens");
        assert!(
            (78..=82).contains(&t),
            "100 CJK chars should land near 0.8 token/字, got {t}"
        );
        assert!(
            t > estimate_tokens(&s),
            "calibrated CJK rate now sits above the bytes/4 figure (75)"
        );
    }

    #[test]
    fn estimate_tokens_aware_mixed_ascii_and_cjk() {
        // 你好世界 = 4 CJK -> ceil(16/5) = 4; "hello" = 5 ASCII bytes
        // -> floor(50/34) = 1. Total 5.
        assert_eq!(estimate_tokens_aware("你好世界hello"), 5);
        // Trailing CJK rounds up on its own aggregate: 1 CJK ->
        // ceil(4/5) = 1, plus "ab" -> floor(20/34) = 0.
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
        // punctuation): 3 CJK chars -> ceil(12/5) = 3.
        assert_eq!(estimate_tokens_aware("，。！"), 3);
    }

    #[test]
    fn estimate_tokens_aware_cjk_block_boundaries() {
        // U+2E80 (first CJK radical) and U+9FFF (last Han) are both
        // counted: 2 CJK -> ceil(8/5) = 2.
        assert_eq!(estimate_tokens_aware("\u{2E80}\u{9FFF}"), 2);
        // U+A000 (Yi syllable) is NOT in any counted block -> other,
        // 3 bytes -> 0 tokens at bytes/4.
        assert_eq!(estimate_tokens_aware("\u{A000}"), 0);
        // One non-CJK char (3 bytes -> 0) plus one Han char
        // (ceil(4/5) = 1).
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
        // ceil(8/5) = 2.
        assert_eq!(estimate_tokens_aware("\u{3001}\u{3002}"), 2);
    }

    /// Calibration fixture, fully hand-computed: representative pure-ASCII
    /// (JSON + code), pure-CJK and mixed JSON samples must land exactly on
    /// `floor(ascii*10/34) + ceil(cjk*4/5) + other/4`.
    ///
    /// Each row carries the hand-counted per-class sizes *and* the
    /// hand-computed token total, so a drifting literal fails loudly here
    /// (count mismatch) instead of quietly re-deriving a wrong expectation.
    #[test]
    fn calibrated_estimates_match_hand_computed_samples() {
        // (sample, ascii chars, cjk chars, other bytes, expected tokens)
        //
        // ① pure-ASCII JSON, 41 chars:
        //    floor(41 * 10 / 34) = floor(410/34) = 12
        // ② pure-ASCII code, 29 chars:
        //    floor(29 * 10 / 34) = floor(290/34) = 8
        // ③ pure Chinese, 12 CJK chars (，。 are fullwidth/CJK punct):
        //    ceil(12 * 4 / 5) = ceil(9.6) = 10
        // ④ mixed JSON, 13 ASCII + 8 CJK:
        //    floor(130/34) = 3, ceil(32/5) = 7  ->  10
        let samples = [
            ("{\"path\":\"src/lib.rs\",\"line\":42,\"ok\":true}", 41, 0, 0, 12),
            ("fn main() { println!(\"hi\"); }", 29, 0, 0, 8),
            ("你好世界，这是一个测试。", 0, 12, 0, 10),
            ("{\"路径\":\"你好\",\"状态\":\"完成\"}", 13, 8, 0, 10),
        ];
        for (sample, ascii, cjk, other, expected) in samples {
            let (mut a, mut c, mut o) = (0_u64, 0_u64, 0_u64);
            for ch in sample.chars() {
                if ch.is_ascii() {
                    a += 1;
                } else if is_cjk(ch) {
                    c += 1;
                } else {
                    o += ch.len_utf8() as u64;
                }
            }
            assert_eq!(
                (a, c, o),
                (ascii, cjk, other),
                "hand-count drifted for {sample:?}"
            );
            let want = ascii * 10 / 34 + (cjk * 4 + 4) / 5 + other / 4;
            assert_eq!(want, expected, "hand-computed total drifted for {sample:?}");
            assert_eq!(
                estimate_tokens_aware(sample),
                expected,
                "estimate drifted from the calibrated formula for {sample:?}"
            );
        }
    }

    #[test]
    fn estimate_chars_variants_are_inverse() {
        assert_eq!(estimate_chars(0), 0);
        assert_eq!(estimate_chars(1), 4);
        assert_eq!(estimate_chars(1000), 4000);
        assert_eq!(estimate_chars_aware(0), 0, "zero ASCII token budget");
        assert_eq!(estimate_chars_aware(1), 4, "ceil(1 * 34 / 10)");
        assert_eq!(estimate_chars_aware(2), 7, "ceil(2 * 34 / 10)");
        assert_eq!(estimate_chars_aware(1000), 3400, "1000 ASCII tokens");
        for tokens in [0, 1, 2, 3, 17, 1000] {
            let chars = estimate_chars_aware(tokens);
            assert_eq!(
                estimate_tokens_aware(&"a".repeat(chars as usize)),
                tokens,
                "ASCII inverse must round-trip exactly for {tokens} tokens"
            );
        }
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
