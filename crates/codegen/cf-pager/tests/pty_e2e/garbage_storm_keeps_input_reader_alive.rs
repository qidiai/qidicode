// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// Input-reader resilience (reader-death regression): a sustained stream of
/// unparseable escape garbage (late startup probe replies, PTY desync noise)
/// must not permanently kill the input reader thread. The reader used to
/// exit after 50 consecutive crossterm read errors, leaving a live TUI with
/// dead keyboard+mouse and no self-healing; it now backs off exponentially
/// and keeps retrying. This test FAILS against the old exit-on-storm code:
/// after the garbage storm the typed marker never echoes because the reader
/// is gone. It also exercises the drain-at-startup path for garbage queued
/// before the reader spawns.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn garbage_storm_keeps_input_reader_alive() {
    let content = ContentController::start().await.expect("start content");
    content.set_response(format!("{MOCK_RESPONSE_SENTINEL} post-storm reply."));

    let binary = pager_binary().expect("resolve pager binary");
    let mut harness =
        PtyHarness::spawn_with_content(&binary, DEFAULT_ROWS, DEFAULT_COLS, &content, &[])
            .expect("spawn pager with content");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");

    // ── Garbage storm ────────────────────────────────────────────────────
    // One XTVERSION-reply-style DCS sequence per injection (ESC P ... ST).
    // crossterm 0.28 cannot parse DCS and returns an error per sequence, so
    // 80 units push the consecutive-error counter past the old 50-exit
    // threshold. Separate injections with a small settle keep each unit its
    // own read() round-trip instead of one coalesced buffer.
    const DCS_GARBAGE: &[u8] = b"\x1bP0;1|zzgarbagezz\x1b\\";
    for _ in 0..80 {
        harness.inject_keys(DCS_GARBAGE).expect("inject garbage");
        harness.update(Duration::from_millis(20));
    }

    // Let the storm drain and any reader backoff (max tier: ~6.4s) elapse.
    harness.update(Duration::from_secs(8));

    // ── The reader must still be alive: typed keys must echo ─────────────
    const TYPED: &str = "ZZSTORMSURVIVORZZ";
    harness.inject_keys(TYPED.as_bytes()).expect("type after storm");

    if harness
        .wait_for_text(TYPED, Duration::from_secs(15))
        .is_err()
    {
        panic!(
            "typed text did not echo after a garbage storm: the input reader \
             thread is dead (consecutive-error exit regression).\nscreen:\n{}",
            harness.screen_contents()
        );
    }

    harness.quit().expect("clean quit");
}
