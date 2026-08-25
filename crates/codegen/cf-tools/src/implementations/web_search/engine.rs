//! Built-in scraping engine for the `web_search` tool ("native" mode).
//!
//! Ported from the proven Python shim (desktop machine,
//! `.qidi/tmp/websearch-shim/shim.py`): three engines with chain fallback —
//! 360/Sogou first for CJK queries (best Chinese recall), Bing first for
//! everything else. No API keys, no LLM round-trip: results are returned to
//! the agent verbatim and the session model reads them.
//!
//! Parsing is deliberately regex/`scraper`-based (no headless browser):
//! each engine degrades to the next on any parse miss, so partial markup
//! changes upstream only shrink coverage, never break the tool.

use std::time::Duration;

/// A single search result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

const DESKTOP_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36";

/// Total budget for the whole engine chain per query.
const CHAIN_TIMEOUT: Duration = Duration::from_secs(30);
/// Per-engine HTTP timeout.
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// Default result count per query.
const DEFAULT_COUNT: usize = 5;

/// Handle over the built-in engines. Exists as a type so the client can
/// hold `Option<NativeEngine>` and dispatch cheaply.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NativeEngine;

impl NativeEngine {
    /// Run the engine chain with the overall time budget. `allowed_domains`,
    /// when set, filters results to those domains (mirrors the Responses API
    /// filter semantics).
    pub(crate) async fn run(
        self,
        client: &reqwest::Client,
        query: &str,
        allowed_domains: Option<&[String]>,
    ) -> Vec<SearchResult> {
        let mut results =
            match tokio::time::timeout(CHAIN_TIMEOUT, search(client, query, DEFAULT_COUNT)).await {
                Ok(results) => results,
                Err(_) => {
                    tracing::warn!(query, "native web search: engine chain timed out");
                    Vec::new()
                }
            };
        if let Some(domains) = allowed_domains
            && !domains.is_empty()
        {
            results.retain(|r| domains.iter().any(|d| r.url.contains(d.as_str())));
        }
        results
    }
}

/// Render results as the tool's `content`: a numbered list the agent model
/// reads directly (no second LLM call — that synthesis happens in the
/// session model's head).
pub(crate) fn format_results(query: &str, results: &[SearchResult]) -> String {
    if results.is_empty() {
        return format!("No results found for \"{query}\".");
    }
    let mut out = String::with_capacity(64 + results.len() * 160);
    out.push_str(&format!("Web search results for \"{query}\":\n"));
    for (i, r) in results.iter().enumerate() {
        out.push_str(&format!("\n[{}] {}\n", i + 1, r.title));
        out.push_str(&format!("URL: {}\n", r.url));
        if !r.snippet.is_empty() {
            out.push_str(&format!("Snippet: {}\n", r.snippet));
        }
    }
    out
}

pub fn http_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(DESKTOP_UA)
        .timeout(FETCH_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
}

/// Integration-test entry point (real network, `--ignored` smoke tests).
pub async fn smoke_search(client: &reqwest::Client, query: &str) -> Vec<SearchResult> {
    search(client, query, DEFAULT_COUNT).await
}

fn has_cjk(query: &str) -> bool {
    query.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

/// Run the engine chain for `query`. Engines are tried in locale order until
/// one returns at least one result. Each engine returns an empty vec on any
/// failure so the chain moves on. CJK queries try 360/Sogou first (best
/// Chinese recall); everything else tries Bing first.
pub(crate) async fn search(client: &reqwest::Client, query: &str, count: usize) -> Vec<SearchResult> {
    if has_cjk(query) {
        for results in [
            fetch_so360(client, query, count).await,
            fetch_sogou(client, query, count).await,
            fetch_bing(client, query, count).await,
        ] {
            if !results.is_empty() {
                return results;
            }
        }
    } else {
        for results in [
            fetch_bing(client, query, count).await,
            fetch_so360(client, query, count).await,
            fetch_sogou(client, query, count).await,
        ] {
            if !results.is_empty() {
                return results;
            }
        }
    }
    Vec::new()
}

// ─────────────────────────────────────────────────────────────────────────
// 360 (so.com) — best CJK recall; `data-mdurl` gives the real URL directly.
// ─────────────────────────────────────────────────────────────────────────

async fn fetch_so360(client: &reqwest::Client, query: &str, count: usize) -> Vec<SearchResult> {
    let Ok(resp) = client
        .get("https://www.so.com/s")
        .query(&[("q", query)])
        .header("Accept-Language", "zh-CN,zh;q=0.9")
        .send()
        .await
    else {
        return Vec::new();
    };
    if !resp.status().is_success() {
        return Vec::new();
    }
    let Ok(html) = resp.text().await else {
        return Vec::new();
    };
    parse_so360(&html, count)
}

fn parse_so360(html: &str, count: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    for block in html.split("<li class=\"res-list\"").skip(1) {
        // <h3 …><a href="URL" …>TITLE</a></h3>
        let Some((title, href)) = first_anchor_in_h3(block) else {
            continue;
        };
        // Prefer data-mdurl (real destination) over the tracker href.
        let url = extract_attr(block, "data-mdurl").unwrap_or(href);
        let snippet = extract_class_text(block, "res-desc").unwrap_or_default();
        if !title.is_empty() && url.starts_with("http") {
            results.push(SearchResult { title, url, snippet });
        }
        if results.len() >= count {
            break;
        }
    }
    results
}

// ─────────────────────────────────────────────────────────────────────────
// Sogou — good CJK recall; `/link?url=…` redirects must be resolved.
// ─────────────────────────────────────────────────────────────────────────

async fn fetch_sogou(client: &reqwest::Client, query: &str, count: usize) -> Vec<SearchResult> {
    let Ok(resp) = client
        .get("https://www.sogou.com/web")
        .query(&[("query", query)])
        .header("Accept-Language", "zh-CN,zh;q=0.9")
        .send()
        .await
    else {
        return Vec::new();
    };
    if !resp.status().is_success() {
        return Vec::new();
    }
    let final_url = resp.url().clone();
    let Ok(html) = resp.text().await else {
        return Vec::new();
    };
    // Antispider page: bail so the chain moves to the next engine.
    if final_url.path().contains("antispider") || html[..html.len().min(2000)].contains('验') {
        return Vec::new();
    }
    parse_sogou(&html, count)
}

fn parse_sogou(html: &str, count: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let split_re = regex::Regex::new(r#"<div class="vrwrap">|<div class="rb">"#).unwrap();
    for block in split_re.split(html).skip(1) {
        let Some((title, href)) = first_anchor_in_h3(block) else {
            continue;
        };
        let href = if href.starts_with("/link") {
            format!("https://www.sogou.com{href}")
        } else {
            href
        };
        let snippet = extract_class_text(block, "star-wiki")
            .or_else(|| extract_class_text(block, "space-txt"))
            .or_else(|| {
                let t = first_p_text(block);
                (!t.is_empty()).then_some(t)
            })
            .unwrap_or_default();
        if !title.is_empty() && href.starts_with("http") {
            results.push(SearchResult { title, url: href, snippet });
        }
        if results.len() >= count {
            break;
        }
    }
    results
}

// ─────────────────────────────────────────────────────────────────────────
// Bing (cn.bing.com) — best non-CJK recall; over-fetch 4x then score-rank.
// ─────────────────────────────────────────────────────────────────────────

async fn fetch_bing(client: &reqwest::Client, query: &str, count: usize) -> Vec<SearchResult> {
    let Ok(resp) = client
        .get("https://cn.bing.com/search")
        .query(&[("q", query), ("count", &format!("{}", count * 4)), ("mkt", "zh-CN")])
        .header("Accept-Language", "zh-CN,zh;q=0.9")
        .send()
        .await
    else {
        return Vec::new();
    };
    if !resp.status().is_success() {
        return Vec::new();
    }
    let Ok(html) = resp.text().await else {
        return Vec::new();
    };
    parse_bing(&html, query, count)
}

/// Domains that pollute general results (encyclopedia/topic pages etc.).
const BING_BLACKLIST: [&str; 5] = [
    "baike.baidu.com",
    "map.baidu.com",
    "zhihu.com/topic",
    "zhidao.baidu.com",
    "tieba.baidu.com",
];

fn parse_bing(html: &str, query: &str, count: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    for block in html.split("<li class=\"b_algo\"").skip(1) {
        let Some((title, href)) = first_anchor_in_h3(block) else {
            continue;
        };
        let snippet = first_p_text(block);
        if !title.is_empty() && href.starts_with("http") {
            results.push(SearchResult { title, url: href, snippet });
        }
    }
    // Score-rank: term hits in title/snippet boost, blacklisted domains sink.
    let terms: Vec<&str> = query.split_whitespace().filter(|t| !t.is_empty()).collect();
    let score = |r: &SearchResult| -> i32 {
        let mut s = 0;
        for t in &terms {
            if r.title.contains(t) {
                s += 3;
            }
            if r.snippet.contains(t) {
                s += 1;
            }
        }
        if BING_BLACKLIST.iter().any(|b| r.url.contains(b)) {
            s -= 10;
        }
        s
    };
    results.sort_by_key(|r| std::cmp::Reverse(score(r)));
    results.truncate(count);
    results
}

// ─────────────────────────────────────────────────────────────────────────
// Shared HTML helpers (tag-stripping extraction, shim `_strip_tags` port).
// ─────────────────────────────────────────────────────────────────────────

/// First `<h2|h3 …><a … href="…">TITLE</a></…>` inside `block`.
fn first_anchor_in_h3(block: &str) -> Option<(String, String)> {
    // Bing wraps result anchors in <h2>, 360/Sogou in <h3> — accept both.
    let h_start = ["<h2", "<h3"]
        .iter()
        .filter_map(|tag| block.find(tag))
        .min()?;
    let rest = &block[h_start..];
    let a_start = rest.find("<a ")? + h_start;
    let a_rest = &block[a_start..];
    let a_end = a_rest.find('>')?;
    let open_tag = &a_rest[..a_end];
    let href = extract_attr(open_tag, "href")?;
    let title_start = a_start + a_end + 1;
    let title_end = block[title_start..].find("</a>")? + title_start;
    Some((strip_tags(&block[title_start..title_end]), href))
}

/// Value of `name="…"`/`name='…'` in `tag_text` (an opening tag or block).
fn extract_attr(tag_text: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=");
    let idx = tag_text.find(&needle)?;
    let rest = &tag_text[idx + needle.len()..];
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let end = rest[1..].find(quote)? + 1;
    Some(rest[1..end].to_string())
}

/// Text of the first element carrying `class="…{klass}…"`, tag-stripped.
fn extract_class_text(block: &str, klass: &str) -> Option<String> {
    // Scan every tag; on a start tag whose class list contains `klass`,
    // collect literal text until the matching close of that element.
    let tag_re = regex::Regex::new(r#"</?([a-zA-Z][a-zA-Z0-9]*)[^>]*>"#).unwrap();
    let mut depth = 0usize;
    let mut open_tag_name = String::new();
    let mut capture = false;
    let mut buf = String::new();
    let mut cursor = 0usize;
    for caps in tag_re.captures_iter(block) {
        let m = caps.get(0).unwrap();
        if capture {
            buf.push_str(&block[cursor..m.start()]);
        }
        cursor = m.end();
        let tag = m.as_str();
        let name = caps
            .get(1)
            .map(|g| g.as_str())
            .unwrap_or_default()
            .to_lowercase();
        let is_close = tag.starts_with("</");
        if !capture {
            if !is_close {
                let classes = extract_attr(tag, "class").unwrap_or_default();
                if classes.split_whitespace().any(|c| c == klass) {
                    capture = true;
                    open_tag_name = name.clone();
                    depth = 1;
                }
            }
        } else if is_close && name == open_tag_name {
            depth -= 1;
            if depth == 0 {
                let text = strip_tags(&buf);
                return (!text.is_empty()).then_some(text);
            }
        } else if !is_close && name == open_tag_name {
            depth += 1;
        }
    }
    if capture {
        let text = strip_tags(&buf);
        return (!text.is_empty()).then_some(text);
    }
    None
}

/// First `<p …>…</p>` text in `block`, tag-stripped.
fn first_p_text(block: &str) -> String {
    let Some(p_start) = block.find("<p") else {
        return String::new();
    };
    let rest = &block[p_start..];
    let Some(open_end) = rest.find('>') else {
        return String::new();
    };
    let body = &rest[open_end + 1..];
    let Some(close) = body.find("</p>") else {
        return String::new();
    };
    strip_tags(&body[..close])
}

/// Port of the shim's `_strip_tags`: drop script/style, strip tags, decode a
/// few common entities, collapse whitespace.
pub(crate) fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut rest = html;
    // Fast-path: drop <script>/<style> blocks wholesale.
    while let Some(pos) = find_script_or_style(rest) {
        out.push_str(&rest[..pos.start]);
        rest = &rest[pos.end..];
    }
    out.push_str(rest);
    // Strip all remaining tags.
    let mut text = String::with_capacity(out.len() / 2);
    let mut in_tag = false;
    for ch in out.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    // Decode common entities (shim parity).
    for (ent, dec) in [
        ("&nbsp;", " "),
        ("&#160;", " "),
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
    ] {
        text = text.replace(ent, dec);
    }
    // Collapse runs of whitespace.
    let mut collapsed = String::with_capacity(text.len());
    let mut last_ws = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !last_ws {
                collapsed.push(' ');
            }
            last_ws = true;
        } else {
            collapsed.push(ch);
            last_ws = false;
        }
    }
    collapsed.trim().to_string()
}

struct Span {
    start: usize,
    end: usize,
}

fn find_script_or_style(html: &str) -> Option<Span> {
    let low = html.to_lowercase();
    let (open, tag_len) = if let Some(p) = low.find("<script") {
        (p, 7)
    } else if let Some(p) = low.find("<style") {
        (p, 6)
    } else {
        return None;
    };
    // Match the close tag in the same casing-agnostic buffer via offsets.
    let close = low[open..].find("</script>").map(|c| c + open).or_else(|| {
        low[open..].find("</style>").map(|c| c + open)
    })?;
    let close_end = close + if low[close..].starts_with("</script>") { 9 } else { 8 };
    let _ = tag_len;
    Some(Span { start: open, end: close_end })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_detection() {
        assert!(has_cjk("Rust 并发编程"));
        assert!(has_cjk("东京 天气"));
        assert!(!has_cjk("rust async programming"));
    }

    #[test]
    fn strip_tags_basics() {
        assert_eq!(strip_tags("<p>Hello <b>world</b></p>"), "Hello world");
        assert_eq!(strip_tags("a &amp; b&nbsp;&nbsp;c"), "a & b c");
        assert_eq!(
            strip_tags("<script>var x=1;</script><style>p{}</style>text"),
            "text"
        );
        assert_eq!(strip_tags("  multi   \n line  "), "multi line");
    }

    #[test]
    fn parse_so360_sample() {
        let html = r#"<ul><li class="res-list" data-mdurl="https://real.example/a">
            <h3><a href="https://tracker.so.com/link?x">Example A</a></h3>
            <p class="res-desc">关于 A 的摘要</p></li>
            <li class="res-list"><h3><a href="https://b.example/b">B result</a></h3></li></ul>"#;
        let rs = parse_so360(html, 5);
        assert_eq!(rs.len(), 2);
        assert_eq!(rs[0].url, "https://real.example/a"); // mdurl preferred
        assert_eq!(rs[0].title, "Example A");
        assert_eq!(rs[0].snippet, "关于 A 的摘要");
        assert_eq!(rs[1].url, "https://b.example/b");
    }

    #[test]
    fn parse_sogou_sample() {
        let html = r#"<div class="vrwrap"><h3><a href="/link?url=abc123">Sogou 标题</a></h3>
            <p class="space-txt">摘要文字</p></div>
            <div class="rb"><h3><a href="https://plain.example/x">Plain</a></h3></div>"#;
        let rs = parse_sogou(html, 5);
        assert_eq!(rs.len(), 2);
        assert_eq!(rs[0].url, "https://www.sogou.com/link?url=abc123");
        assert_eq!(rs[0].snippet, "摘要文字");
        assert_eq!(rs[1].url, "https://plain.example/x");
    }

    #[test]
    fn parse_bing_scores_and_blacklist() {
        let html = r#"<ol><li class="b_algo"><h2><a href="https://good.example/rust">rust guide</a></h2>
            <p>rust async runtime</p></li>
            <li class="b_algo"><h2><a href="https://baike.baidu.com/item/rust">rust 百科</a></h2>
            <p>rust 词条</p></li>
            <li class="b_algo"><h2><a href="https://other.example/unrelated">unrelated</a></h2>
            <p>nothing here</p></li></ol>"#;
        let rs = parse_bing(html, "rust async", 3);
        assert_eq!(rs.len(), 3);
        // Scoring: title+snippet hits rank first; blacklist sinks to last.
        assert_eq!(rs[0].url, "https://good.example/rust");
        assert_eq!(rs.last().unwrap().url, "https://baike.baidu.com/item/rust");
    }

    #[test]
    fn first_anchor_handles_nested_markup() {
        let block = r#"<h3 class="t"><a class="a" href='https://x.example/1'>Title <strong>bold</strong></a></h3>"#;
        let (title, href) = first_anchor_in_h3(block).unwrap();
        assert_eq!(title, "Title bold");
        assert_eq!(href, "https://x.example/1");
    }

    #[test]
    fn empty_html_yields_empty() {
        assert!(parse_so360("<html></html>", 5).is_empty());
        assert!(parse_sogou("", 5).is_empty());
        assert!(parse_bing("no results", "q", 5).is_empty());
    }

    #[test]
    fn format_results_renders_numbered_list() {
        let rs = vec![
            SearchResult {
                title: "Title A".into(),
                url: "https://a.example".into(),
                snippet: "snippet a".into(),
            },
            SearchResult {
                title: "Title B".into(),
                url: "https://b.example".into(),
                snippet: String::new(),
            },
        ];
        let out = format_results("q", &rs);
        assert!(out.starts_with("Web search results for \"q\":"));
        assert!(out.contains("[1] Title A\nURL: https://a.example\nSnippet: snippet a"));
        assert!(out.contains("[2] Title B\nURL: https://b.example\n"));
        assert!(!out.contains("Snippet: \n"));
        assert_eq!(format_results("q", &[]), "No results found for \"q\".");
    }
}
