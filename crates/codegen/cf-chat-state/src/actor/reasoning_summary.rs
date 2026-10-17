//! Phase C Reasoning-summary channel (batch 2): pure, deterministic rule
//! extraction, block fingerprinting, and subset validation.
//!
//! This module is the *state-layer* half of the pre-generation/application
//! split. No LLM here — the LLM attribution line is produced in the session
//! layer (cf-shell) and only *validated* here via [`subset_check`] before the
//! block's summary is written into the [`crate::types::ReasoningSummaries`]
//! table and later applied by `prune_retained_batched`.
//!
//! Every function is pure and side-effect-free so the unit tests can cover
//! the whole surface hermetically.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

/// Number atom: `\d+(?:\.\d+)?`. Shared by [`rule_extract_summary`] and
/// [`subset_check`] — the design doc requires one regex for both.
static NUMBER_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\d+(?:\.\d+)?").expect("static number regex is valid")
});

/// File-path atom: a Windows drive path (`C:\...`) or a dotted filename
/// (`foo.rs`, `方案.md`). Mirrors the Python prototype `PATH_RE`.
static PATH_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"[A-Za-z]:[\\/][^\s，。；、】)]+|[\w./-]+\.(?:py|md|json|txt|rs|toml|log|csv|ya?ml|xml|html?|js|ts|go|c|cpp|h)\b",
    )
    .expect("static path regex is valid")
});

/// Line-number atom: `第 N 行` / `行号 N` / `:N`. Mirrors `LINE_RE`.
static LINE_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?:第\s*)?\d+\s*行|行号\s*\d+|:\d+\b")
        .expect("static line regex is valid")
});

/// Identifier atom: an ASCII-letter-led word, optionally hyphen-compound,
/// length ≥ 2 (`try-with-resources`, `CRC32-C`). Mirrors `IDENT_RE`.
static IDENT_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"[A-Za-z][A-Za-z0-9]*(?:-[A-Za-z0-9]+)*")
        .expect("static ident regex is valid")
});

/// Assertion-keyword anchors (C-1 rule extractor core lexicon). The Python
/// prototype's `排除|不能|必须|禁止|约束|仅有|只能` plus the design doc's
/// added `关键` / `天花板`.
const ASSERT_KEYWORDS: &[&str] = &[
    "排除", "不能", "必须", "禁止", "约束", "仅有", "只能", "关键", "天花板",
];

/// B-2 rule-floor caps: at most this many numeric/parameter atoms and this
/// many assertion sentences (both in appearance order) are kept, and the whole
/// rule-floor string is truncated to this many UTF-8 bytes (with an ellipsis
/// marker) when it would otherwise exceed them. Without these caps a long
/// reasoning block's rule floor (numbers + every ASCII identifier + long
/// verbatim sentences) can be *larger* than the original — making the batched
/// replacement grow the context instead of shrinking it.
const MAX_SUMMARY_PARAMS: usize = 8;
const MAX_SUMMARY_SENTENCES: usize = 8;
const MAX_SUMMARY_BYTES: usize = 480;
/// Marker appended when the rule floor is byte-truncated (B-2).
const SUMMARY_ELLIPSIS: &str = "…";

/// Sentence-split delimiters, matching the Python prototype `SENT_SPLIT_RE`
/// (`[。；！\n]`).
fn is_sentence_split(c: char) -> bool {
    matches!(c, '。' | '；' | '！' | '\n')
}

/// Extract every number atom in document order.
fn number_atoms(text: &str) -> Vec<String> {
    NUMBER_RE
        .find_iter(text)
        .map(|m| m.as_str().to_string())
        .collect()
}

/// Extract assertion sentences verbatim (unchanged): sentences containing at
/// least one [`ASSERT_KEYWORDS`] keyword. Sentences keep their trailing
/// delimiter (`。`/`；`/`！`/newline) — "verbatim" per the C-1 contract means
/// the full original sentence including punctuation, so manual splitting is
/// used instead of `str::split` (which eats the delimiter).
fn assertion_sentences(text: &str) -> Vec<String> {
    let flush = |cur: &str, out: &mut Vec<String>| {
        let s = cur.trim();
        if !s.is_empty() && ASSERT_KEYWORDS.iter().any(|kw| s.contains(kw)) {
            out.push(s.to_string());
        }
    };
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        cur.push(c);
        if is_sentence_split(c) {
            flush(&cur, &mut out);
            cur.clear();
        }
    }
    if !cur.is_empty() {
        flush(&cur, &mut out); // tail segment without a terminator
    }
    out
}

/// De-duplicate while preserving first-seen order.
fn dedup_preserve_order(items: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(items.len());
    for s in items {
        if seen.insert(s.clone()) {
            out.push(s);
        }
    }
    out
}

/// Phase C rule extractor (batch 2): produce the deterministic "rule floor"
/// summary for a reasoning block, or `None` when the block has neither any
/// numeric/parameter atom nor any assertion sentence, **or** when the B-2
/// byte truncation split a number atom so the truncated floor would violate
/// the C-1 subset contract (blind fix 5, deepseek-flash audit).
///
/// `None` tells the caller to skip the block entirely: the batched prune
/// applies never-bare-clear on a table miss, so a block with nothing to
/// extract — or with a truncation-poisoned floor — must not get a bare/empty
/// (or contract-violating) summary.
///
/// Output format (verbatim per the design doc):
/// `[推理摘要|规则抽取] 数字/参数：<…>；断言："<原句1>"、"<原句2>"`
pub fn rule_extract_summary(text: &str) -> Option<String> {
    let mut params = Vec::new();
    params.extend(number_atoms(text));
    params.extend(PATH_RE.find_iter(text).map(|m| m.as_str().to_string()));
    params.extend(LINE_RE.find_iter(text).map(|m| m.as_str().to_string()));
    params.extend(
        IDENT_RE
            .find_iter(text)
            .map(|m| m.as_str().to_string())
            .filter(|s| s.len() >= 2),
    );
    let params = dedup_preserve_order(params);

    let sentences = assertion_sentences(text);

    if params.is_empty() && sentences.is_empty() {
        return None;
    }

    // B-2: cap both lists (appearance order) before rendering, so the floor is
    // bounded per section.
    let params: Vec<String> = params.into_iter().take(MAX_SUMMARY_PARAMS).collect();
    let sentences: Vec<String> = sentences.into_iter().take(MAX_SUMMARY_SENTENCES).collect();

    let mut sections: Vec<String> = Vec::with_capacity(2);
    if !params.is_empty() {
        sections.push(format!("数字/参数：{}", params.join("、")));
    }
    if !sentences.is_empty() {
        let quoted: Vec<String> = sentences.iter().map(|s| format!("\"{s}\"")).collect();
        sections.push(format!("断言：{}", quoted.join("、")));
    }
    // B-2: byte cap with an ellipsis marker (UTF-8-safe truncation).
    let floor = truncate_summary_bytes(format!(
        "[推理摘要|规则抽取] {}",
        sections.join("；")
    ));
    // Blind fix 5 (deepseek-flash audit): byte truncation can split the tail
    // number atom (e.g. `120` → `1`), which violates the C-1 subset contract
    // at the truncation edge — `summarize_with_guard`'s degrade path returns
    // this floor verbatim, so a split atom would be written into the
    // [`crate::types::ReasoningSummaries`] table and later applied. Re-run
    // the subset check on the *truncated* floor against the original text;
    // on failure return `None` — the caller skips the block and the batched
    // prune's table miss (C-1b never-bare-clear) keeps the original.
    if subset_check(&floor, text) {
        Some(floor)
    } else {
        None
    }
}

/// B-2: truncate a rule-floor summary to [`MAX_SUMMARY_BYTES`] bytes on a
/// UTF-8 char boundary, appending [`SUMMARY_ELLIPSIS`] when truncation
/// occurred. Guarantees the returned length is ≤ [`MAX_SUMMARY_BYTES`].
fn truncate_summary_bytes(mut summary: String) -> String {
    if summary.len() <= MAX_SUMMARY_BYTES {
        return summary;
    }
    let budget = MAX_SUMMARY_BYTES.saturating_sub(SUMMARY_ELLIPSIS.len());
    let mut cut = budget.min(summary.len());
    while cut > 0 && !summary.is_char_boundary(cut) {
        cut -= 1;
    }
    summary.truncate(cut);
    summary.push_str(SUMMARY_ELLIPSIS);
    summary
}

/// Content fingerprint for a reasoning block — the [`crate::types::ReasoningSummaries`]
/// table key (block fingerprint → summary).
///
/// A session-internal dedup key only (no cryptographic requirement), so
/// [`std::collections::hash_map::DefaultHasher`] (fixed seed, deterministic
/// within the process) suffices. The session layer and the prune layer both
/// hash the *same* [`cf_sampling_types::reasoning_item_text`] so a block
/// pre-generated on one side matches the table lookup on the other. The `u64`
/// semantics align with the batch-1 `replay_prompt_fingerprints` key type.
pub fn reasoning_block_fingerprint(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Number-atom set for a string (shared regex with [`rule_extract_summary`]).
fn number_set(text: &str) -> HashSet<String> {
    number_atoms(text).into_iter().collect()
}

/// Subset assertion (Phase C, C-1): every number atom in `summary` must be
/// present in `original`. The session layer uses this to detect an LLM
/// attribution line that invented a number not in the block, and degrade the
/// whole block to the pure rule floor.
pub fn subset_check(summary: &str, original: &str) -> bool {
    number_set(summary).is_subset(&number_set(original))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_extract_captures_numbers() {
        let s = rule_extract_summary("处理 42 个文件，占用 3.5 GB，p99 延迟 120ms").unwrap();
        assert!(s.contains("数字/参数：42、3.5、99、120"), "got: {s}");
    }

    #[test]
    fn rule_extract_captures_assertion_sentences_verbatim() {
        let text = "必须保留原句。这条可以改。禁止删除任何数字。";
        let s = rule_extract_summary(text).unwrap();
        assert!(s.contains("断言：\"必须保留原句。\""), "got: {s}");
        assert!(s.contains("\"禁止删除任何数字。\""), "got: {s}");
        assert!(!s.contains("这条可以改"), "non-assertion sentence must be dropped: {s}");
    }

    #[test]
    fn rule_extract_empty_input_returns_none() {
        // No numbers and no assertion keywords → None (never-bare-clear).
        assert!(rule_extract_summary("这里只有一段普通的描述性文字。").is_none());
        assert!(rule_extract_summary("").is_none());
    }

    #[test]
    fn rule_extract_captures_chinese_paths_and_windows_drive_and_line() {
        let text = "参考 G:\\qidicode\\target\\tmp\\rule_extract.py 与 方案.md 和 第 12 行。";
        let s = rule_extract_summary(text).unwrap();
        assert!(s.contains("rule_extract.py"), "got: {s}");
        assert!(s.contains("方案.md"), "got: {s}");
        assert!(s.contains("第 12 行"), "got: {s}");
    }

    #[test]
    fn rule_extract_captures_identifiers_min_len_2() {
        let s = rule_extract_summary("见 CRC32-C 与 try-with-resources 与 x").unwrap();
        assert!(s.contains("CRC32-C"), "got: {s}");
        assert!(s.contains("try-with-resources"), "got: {s}");
        assert!(!s.contains('x'), "single-char ident 'x' must be filtered: {s}");
    }

    /// B-2: both the numeric/parameter list and the assertion list are capped
    /// at top-8 (appearance order).
    #[test]
    fn rule_extract_caps_params_and_sentences_at_top_n() {
        // 12 distinct numbers in appearance order → only the first 8 survive.
        let s = rule_extract_summary("1 2 3 4 5 6 7 8 9 10 11 12").unwrap();
        assert_eq!(
            s, "[推理摘要|规则抽取] 数字/参数：1、2、3、4、5、6、7、8",
            "B-2 top-N: only the first 8 params may be kept"
        );

        // 10 distinct assertion sentences → only the first 8 survive.
        let text = "必须保留甲。必须保留乙。必须保留丙。必须保留丁。必须保留戊。\
                    必须保留己。必须保留庚。必须保留辛。必须保留壬。必须保留癸。";
        let s = rule_extract_summary(text).unwrap();
        assert!(
            s.contains("\"必须保留辛。\""),
            "B-2 top-N: the 8th assertion sentence must be kept: {s}"
        );
        assert!(
            !s.contains("\"必须保留壬。\"") && !s.contains("\"必须保留癸。\""),
            "B-2 top-N: the 9th/10th assertion sentences must be dropped: {s}"
        );
    }

    /// B-2: an over-long rule floor is byte-truncated to ≤ 480 bytes with an
    /// ellipsis marker (both caps can be satisfied yet the floor still huge).
    #[test]
    fn rule_extract_truncates_over_long_output_at_byte_cap() {
        // 8 long assertion sentences (~370 bytes each) → far over 480 bytes
        // even though the top-8 sentence cap is already applied.
        let mut text = String::new();
        for filler in ["甲", "乙", "丙", "丁", "戊", "己", "庚", "辛"] {
            text.push_str("必须");
            text.push_str(&filler.repeat(120));
            text.push('。');
        }
        let s = rule_extract_summary(&text).unwrap();
        assert!(
            s.len() <= MAX_SUMMARY_BYTES,
            "B-2 byte cap: summary must be <= {} bytes, got {}: {s}",
            MAX_SUMMARY_BYTES,
            s.len()
        );
        assert!(
            s.ends_with(SUMMARY_ELLIPSIS),
            "B-2 byte cap: a truncated floor must carry the ellipsis marker: {s}"
        );
        assert!(
            s.is_char_boundary(s.len()),
            "B-2 byte cap: truncation must land on a UTF-8 char boundary"
        );
    }

    #[test]
    fn fingerprint_is_deterministic_and_distinguishing() {
        let a = reasoning_block_fingerprint("同一个块");
        let b = reasoning_block_fingerprint("同一个块");
        assert_eq!(a, b, "same text must hash to the same fingerprint");
        let c = reasoning_block_fingerprint("另一个块");
        assert_ne!(a, c, "different text should (overwhelmingly) hash differently");
    }

    #[test]
    fn subset_check_passes_when_summary_numbers_are_subset() {
        assert!(subset_check("42 个", "有 42 个文件和 3.5 GB"));
        assert!(subset_check("3.5", "3.5 GB"));
    }

    #[test]
    fn subset_check_fails_when_summary_invents_a_number() {
        assert!(!subset_check("处理 128 个", "处理 42 个"));
    }

    #[test]
    fn subset_check_empty_summary_numbers_vacuously_passes() {
        assert!(subset_check("没有任何数字", "原始也没有数字"));
        assert!(subset_check("", ""));
    }

    /// Blind fix 5 (deepseek-flash audit): a byte-boundary cut through a
    /// number atom (`120` → `1`) must fail the C-1 subset assertion — the
    /// split atom is absent from the original's number set.
    #[test]
    fn subset_check_rejects_a_truncation_split_number_atom() {
        assert!(
            !subset_check("数字/参数：1", "处理 120 个"),
            "盲面5/C-1: a byte-truncation-split number atom (120→1) must fail the subset contract"
        );
    }

    /// Blind fix 5 (deepseek-flash audit): byte truncation must never let a
    /// split number atom past the subset contract. A 503-digit number forces
    /// the 477-byte cut inside the digit run (the 44-byte header precedes it),
    /// so the truncated floor carries a *shorter* digit run that is absent
    /// from the original's number set — the floor must be dropped (`None`):
    /// the caller skips the block and the batched-prune table miss
    /// (C-1b never-bare-clear) keeps the original.
    #[test]
    fn rule_extract_drops_floor_when_truncation_splits_a_number_atom() {
        // 503 digits: params section (44-byte header + 503) alone exceeds the
        // 480-byte cap, and the cut (477) lands inside the ASCII digit run.
        let num = format!("120{}", "0".repeat(500));
        let text = format!("参数必须为 {num} 个。");

        let s = rule_extract_summary(&text);
        assert!(
            s.is_none(),
            "盲面5/C-1: a floor whose byte truncation splits a number atom must be dropped (None), got {s:?}"
        );
    }

    /// Blind fix 5 control: the same block *without* the truncation pressure
    /// keeps its floor — the drop is triggered by the truncation edge only,
    /// not by the digit-heavy content itself.
    #[test]
    fn rule_extract_keeps_floor_when_no_truncation_occurs() {
        let text = "参数必须为 120 个。";
        let s = rule_extract_summary(text).unwrap();
        assert!(
            s.contains("120"),
            "盲面5/C-1 control: a non-truncated floor must keep its number atom: {s}"
        );
        assert!(
            !s.ends_with(SUMMARY_ELLIPSIS),
            "盲面5/C-1 control: no truncation marker expected under the byte cap: {s}"
        );
        assert!(
            subset_check(&s, text),
            "盲面5/C-1 control: the kept floor must satisfy the C-1 subset contract: {s}"
        );
    }
}
