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
/// numeric/parameter atom nor any assertion sentence.
///
/// `None` tells the caller to skip the block entirely: the batched prune
/// applies never-bare-clear on a table miss, so a block with nothing to
/// extract must not get a bare/empty summary.
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

    let mut sections: Vec<String> = Vec::with_capacity(2);
    if !params.is_empty() {
        sections.push(format!("数字/参数：{}", params.join("、")));
    }
    if !sentences.is_empty() {
        let quoted: Vec<String> = sentences.iter().map(|s| format!("\"{s}\"")).collect();
        sections.push(format!("断言：{}", quoted.join("、")));
    }
    Some(format!("[推理摘要|规则抽取] {}", sections.join("；")))
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
}
