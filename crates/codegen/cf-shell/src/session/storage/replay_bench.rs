//! Real-file smoke benchmark: full scan vs checkpoint replay on a real
//! updates.jsonl, asserting byte-for-byte equivalence.
//!
//! Run: cargo test -p cf-shell --lib storage::replay_bench -- --ignored --nocapture

use crate::session::storage::prepare_replay_lines;
use crate::session::storage::replay_checkpoint::{
    self, IncrementalReplayState, build_prepared_replay,
};

const TARGET: &str = r"C:\Users\ASUS\.qidi\sessions\G%3A%5Cqidicode%5Ctarget%5Cdebug\01a01e7b-1dc4-7fc3-934e-c5a87c19abd4\updates.jsonl";

#[test]
#[ignore]
fn full_scan_vs_checkpoint_on_real_file() {
    let view = crate::session::storage::jsonl_mmap::JsonlMmapView::open(std::path::Path::new(TARGET))
        .expect("map")
        .expect("exists");
    let contents = view.as_str().expect("utf8");
    let updates_path = std::path::Path::new(TARGET);

    // 1. Full scan (cold, no checkpoint).
    let t0 = std::time::Instant::now();
    let full = prepare_replay_lines(contents, None, None);
    let t_full = t0.elapsed();
    eprintln!(
        "full scan:        {:>10.1?}  lines={} total_live={} tokens={}",
        t_full,
        full.lines.len(),
        full.total_live,
        full.last_tokens
    );

    // 2. Build a checkpoint from a fresh incremental pass (what a refresh does).
    let t0 = std::time::Instant::now();
    let mut state = IncrementalReplayState::new();
    state.feed(contents);
    let cp = state.to_checkpoint(contents);
    let t_seed = t0.elapsed();
    eprintln!(
        "seed (full pass): {:>10.1?}  offset={} live_offsets={}",
        t_seed,
        cp.offset,
        cp.live_offsets.len()
    );

    // 3. Replay from the checkpoint (resume path).
    let t0 = std::time::Instant::now();
    let prepared = build_prepared_replay(contents, None, &cp);
    let t_cp = t0.elapsed();
    eprintln!("checkpoint replay: {:>10.1?}", t_cp);

    // Equivalence.
    assert_eq!(prepared.lines, full.lines, "line sets diverge");
    assert_eq!(prepared.mark_replay, full.mark_replay);
    assert_eq!(prepared.last_tokens, full.last_tokens);
    assert_eq!(prepared.max_event_seq, full.max_event_seq);
    assert_eq!(prepared.total_live, full.total_live);
    assert_eq!(
        prepared.unfinished_subagents, full.unfinished_subagents,
        "subagents diverge"
    );
    eprintln!(
        "equivalent: yes   speedup: {:.0}x (seed once, then every resume)",
        t_full.as_secs_f64() / t_cp.as_secs_f64()
    );

    // 4. Validate the checkpoint refresh path on the real file, without
    //    persisting anything into the user's session dir.
    let dir = std::env::temp_dir().join(format!(
        "replay-bench-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let copy = dir.join("updates.jsonl");
    // Hardlink instead of copy: 839 MB, same volume.
    std::fs::hard_link(TARGET, &copy)
        .or_else(|_| std::fs::copy(TARGET, &copy).map(|_| ()))
        .unwrap();
    let t0 = std::time::Instant::now();
    replay_checkpoint::refresh_checkpoint(&copy).unwrap();
    eprintln!("refresh_checkpoint: {:>9.1?}", t0.elapsed());
    let loaded = replay_checkpoint::load_validated_checkpoint(&copy, contents).expect("valid");
    let prepared2 = build_prepared_replay(contents, None, &loaded);
    assert_eq!(prepared2.lines, full.lines);
    assert_eq!(prepared2.last_tokens, full.last_tokens);
    eprintln!(
        "round trip through disk: ok (offset {}, {} live lines)",
        loaded.offset, loaded.live_offsets.len()
    );
    std::fs::remove_dir_all(&dir).ok();
}
