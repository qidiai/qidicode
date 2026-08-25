//! Live smoke test for the native web search engine (not part of the unit
//! suite — run manually with `cargo test -p cf-tools --lib web_search -- --ignored --nocapture`).
//! Verifies the parsers against the real search-engine HTML of the day.

use cf_tools::implementations::web_search::engine::smoke_search;

#[tokio::test]
#[ignore]
async fn live_engine_smoke() {
    let client = cf_tools::implementations::web_search::engine::http_client().unwrap();
    let results = smoke_search(&client, "Rust programming language").await;
    println!("latin query: {} results", results.len());
    for r in results.iter().take(3) {
        println!("  {} | {}", r.title, r.url);
    }
    assert!(!results.is_empty(), "latin query should return results");

    let results = smoke_search(&client, "Rust 并发编程").await;
    println!("cjk query: {} results", results.len());
    for r in results.iter().take(3) {
        println!("  {} | {}", r.title, r.url);
    }
    assert!(!results.is_empty(), "cjk query should return results");
}
