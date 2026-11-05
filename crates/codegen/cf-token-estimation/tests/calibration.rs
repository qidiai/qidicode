//! Calibration fixture for [`estimate_tokens_aware`] (token-optimization
//! plan §4 / C7).
//!
//! `tests/data/calibration_stats.json` holds **desensitized counts only** —
//! no source text — for 194 items drawn from 40 size-stratified sessions:
//! `ascii` (ASCII chars), `cjk` (CJK chars), `other` (non-ASCII, non-CJK
//! bytes), and `exact` (token count from the official DeepSeek V4.1
//! tokenizer, deepseek-recipe@935817c, MIT). The generator script is
//! `target/tmp/token-calib/make_fixture.py` and is deliberately not
//! checked in.
//!
//! The test rebuilds an equivalent string from each count vector and feeds
//! it through the real `estimate_tokens_aware`, so it constrains the
//! implementation. The truth denominator is the `exact` column only: no
//! token arithmetic of the estimator's own formula appears here, which
//! would make the ratio tautologically 1.0.
//!
//! The only arithmetic below is independent bookkeeping on the rebuilt
//! strings (byte length, per-class counts), used to prove the
//! reconstruction matches the recorded counts.

use std::collections::BTreeSet;

use cf_token_estimation::estimate_tokens_aware;

#[derive(serde::Deserialize)]
struct Fixture {
    source: String,
    generated_by: String,
    note: String,
    items: Vec<Item>,
}

#[derive(serde::Deserialize, Debug)]
struct Item {
    kind: String,
    size: String,
    ascii: usize,
    cjk: usize,
    other: usize,
    exact: u64,
}

fn load() -> Fixture {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/calibration_stats.json"
    );
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("calibration fixture missing at {path}: {e}"));
    serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("calibration fixture is not valid JSON: {e}"))
}

/// Character-aware classification mirroring the documented contract of
/// `estimate_tokens_aware` (ASCII / CJK blocks / other bytes). This is a
/// reconstruction aid, not a token count.
fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF
    )
}

/// Rebuild a string with the recorded per-class counts.
///
/// `a` for ASCII, `你` (3-byte CJK) for CJK, and 4-byte emoji for the
/// remaining bytes. `other` is a byte count that need not be a multiple of
/// four (the fixture contains 3, 9, 15, ...). A 1- or 2-byte tail cannot be
/// a UTF-8 suffix of a 4-byte emoji, so it is stood in for by an equal
/// number of ASCII bytes appended after the emoji. The estimate treats
/// `other` as one byte-count class (`other / 4`), so this substitution can
/// move the estimate by at most one "other" byte's worth of grouping —
/// bounded by what the estimator deliberately floors away.
fn rebuild(item: &Item) -> String {
    let mut s = String::with_capacity(item.ascii + item.cjk * 3 + item.other);
    s.push_str(&"a".repeat(item.ascii));
    s.push_str(&"你".repeat(item.cjk));
    s.push_str(&"😀".repeat(item.other / 4));
    s.push_str(&"a".repeat(item.other % 4));
    s
}

/// The rebuilt string must carry exactly the recorded counts, otherwise the
/// `exact` column would be compared against a different text.
fn assert_counts_round_trip(item: &Item) {
    let text = rebuild(item);
    let stand_in = item.other % 4;
    assert_eq!(
        text.len(),
        item.ascii + item.cjk * 3 + item.other,
        "rebuilt byte length must equal ascii + 3*cjk + other for {item:?}"
    );
    let (mut ascii, mut cjk, mut other) = (0usize, 0usize, 0usize);
    for c in text.chars() {
        if c.is_ascii() {
            ascii += 1;
        } else if is_cjk(c) {
            cjk += 1;
        } else {
            other += c.len_utf8();
        }
    }
    // The `stand_in` ASCII bytes belong to the `other` class, not to `ascii`.
    assert_eq!(
        ascii,
        item.ascii + stand_in,
        "ASCII char count drifted: {item:?}"
    );
    assert_eq!(cjk, item.cjk, "CJK char count drifted: {item:?}");
    assert_eq!(
        other + stand_in,
        item.other,
        "other-byte count drift (recount + stand-in) for {item:?}"
    );
}

#[test]
fn fixture_shape_is_15_strata_of_at_least_8() {
    let fx = load();
    assert_eq!(fx.items.len(), 194, "fixture item count");
    assert!(!fx.source.is_empty() && !fx.generated_by.is_empty() && !fx.note.is_empty());

    let kinds: BTreeSet<&str> = fx.items.iter().map(|i| i.kind.as_str()).collect();
    let sizes: BTreeSet<&str> = fx.items.iter().map(|i| i.size.as_str()).collect();
    assert_eq!(kinds.len(), 5, "kind strata: {kinds:?}");
    assert_eq!(sizes.len(), 3, "size strata: {sizes:?}");
    for kind in kinds {
        for size in &sizes {
            let n = fx
                .items
                .iter()
                .filter(|i| i.kind == kind && &i.size == size)
                .count();
            assert!(n >= 8, "stratum ({kind}, {size}) has only {n} items");
        }
    }
}

#[test]
fn rebuild_matches_recorded_class_counts() {
    for item in &load().items {
        assert_counts_round_trip(item);
    }
}

/// Aggregate estimate/reference ratio against the official tokenizer.
#[test]
fn calibration_aggregate_ratio_in_band() {
    let items = load().items;
    let mut estimate_sum = 0u64;
    let mut exact_sum = 0u64;
    for item in &items {
        assert_counts_round_trip(item);
        let h = estimate_tokens_aware(&rebuild(item));
        // Report the offender's identity on failure without recomputing tokens.
        assert!(item.exact > 0, "exact must be positive: {item:?}");
        estimate_sum += h;
        exact_sum += item.exact;
    }
    let ratio = estimate_sum as f64 / exact_sum as f64;
    assert!(
        (0.97..=1.01).contains(&ratio),
        "aggregate estimate/exact ratio {ratio:.4} outside [0.97, 1.01] \
         (estimate={estimate_sum}, exact={exact_sum}, n={})",
        items.len()
    );
}

/// Per-item MAPE against the official tokenizer.
#[test]
fn calibration_mape_at_most_18_percent() {
    let items = load().items;
    let mut relative_err = 0.0f64;
    for item in &items {
        assert_counts_round_trip(item);
        assert!(item.exact > 0, "exact must be positive: {item:?}");
        let h = estimate_tokens_aware(&rebuild(item)) as f64;
        let t = item.exact as f64;
        relative_err += (h - t).abs() / t;
    }
    let n = items.len() as f64;
    let mape = (100.0 / n) * relative_err;
    assert!(
        mape <= 18.0,
        "MAPE {mape:.2}% exceeds 18% (n={})",
        items.len()
    );
}
