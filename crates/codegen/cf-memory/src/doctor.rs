//! `memory doctor` — a read-only health scan of the memory index.
//!
//! [`diagnose`] inspects both the global and the workspace memory scope and
//! reports four classes of problems:
//!
//! 1. **Orphan chunks** — indexed paths whose source file no longer exists.
//! 2. **Stuck reindex claims** — a leftover `reindex_claim` lock value whose
//!    age exceeds [`STALE_CLAIM_SECS`].
//! 3. **Stale / low-access chunks** — older than [`STALE_AGE_DAYS`] and never
//!    accessed.
//! 4. **Suspected redundant clusters** — chunk texts whose token-Jaccard
//!    similarity is at or above [`REDUNDANCY_THRESHOLD`].
//!
//! The scan is strictly read-only: it never creates `index.sqlite` (an absent
//! index yields an empty report) and never mutates the schema. Every fact is
//! read through a read-only SQLite connection with plain `SELECT`s. The
//! reindex claim and the indexed paths are read directly from `meta` /
//! `chunks` (mirroring `MemoryIndex::get_reindex_claim` and
//! `MemoryIndex::all_indexed_paths`) instead of opening a read-write
//! `MemoryIndex` — that path runs schema DDL and could write
//! `embedding_dimensions` or drop `chunks_vec`.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use crate::mmr::{jaccard_similarity, tokenize};
use crate::storage::MemoryStorage;

/// A chunk older than this many days with zero recorded accesses is "stale".
const STALE_AGE_DAYS: i64 = 30;

/// Token-Jaccard similarity at/above which two chunks are "suspected" dupes.
const REDUNDANCY_THRESHOLD: f64 = 0.8;

/// Upper bound on chunks fed into the O(n²) redundancy scan.
const MAX_REDUNDANCY_CHUNKS: usize = 2000;

/// A `pid:unix_ts` reindex claim older than this many seconds is "stuck".
const STALE_CLAIM_SECS: i64 = 60;

const SECS_PER_DAY: i64 = 86_400;

/// Per-chunk facts needed for the redundancy check (carries `text`).
#[derive(Debug, Clone)]
struct RedundancyChunk {
    id: String,
    text: String,
}

/// Per-chunk metadata read *without* `text` — feeds the stale check and the
/// distinct-path (orphan) set.
#[derive(Debug, Clone)]
struct ChunkMeta {
    path: String,
    created_at: i64,
    access_count: i64,
}

/// A read-only snapshot of one scope's index.
#[derive(Debug, Default)]
struct IndexSnapshot {
    /// Distinct indexed file paths, sorted.
    indexed_paths: Vec<String>,
    /// Raw non-empty `reindex_claim` value, if any.
    reindex_claim: Option<String>,
    /// Age of the claim in seconds (`now - ts`), when the value parses.
    reindex_claim_age_secs: Option<i64>,
    /// Full chunk metadata (no `text`) — total count, stale and orphan inputs.
    chunk_meta: Vec<ChunkMeta>,
    /// Newest chunks (with `text`), capped at [`MAX_REDUNDANCY_CHUNKS`].
    redundancy_chunks: Vec<RedundancyChunk>,
}

/// A group of chunks that look like near-duplicates.
#[derive(Debug, Clone, PartialEq)]
pub struct RedundantCluster {
    /// Highest pairwise token-Jaccard similarity within the cluster.
    pub similarity: f64,
    /// Chunk IDs (format `path:index`), sorted.
    pub members: Vec<String>,
}

/// Per-scope diagnosis results.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScopeReport {
    /// `"global"` or `"workspace"`.
    pub scope: String,
    /// Whether `index.sqlite` exists for this scope.
    pub index_present: bool,
    /// When the index exists but could not be read, the reason. Distinguishes a
    /// read failure from a genuinely empty index.
    pub read_error: Option<String>,
    /// Number of distinct indexed file paths.
    pub indexed_paths: usize,
    /// Indexed paths whose source file is gone.
    pub orphan_paths: Vec<String>,
    /// A non-empty leftover reindex claim value, if any.
    pub reindex_claim: Option<String>,
    /// Age of the claim in seconds (`now - ts`) when it parses; `None` for a
    /// malformed claim.
    pub reindex_claim_age_secs: Option<i64>,
    /// Count of stale / never-accessed chunks.
    pub stale_chunks: usize,
    /// Suspected redundant clusters.
    pub redundant_clusters: Vec<RedundantCluster>,
    /// True when the redundancy scan was capped by [`MAX_REDUNDANCY_CHUNKS`].
    pub redundancy_truncated: bool,
}

/// Full `memory doctor` report across both scopes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DoctorReport {
    pub global: ScopeReport,
    pub workspace: ScopeReport,
}

impl DoctorReport {
    /// Render a dependency-free, plain-text report (no colors, no formatting).
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("Memory doctor report\n");
        out.push_str("====================\n");
        out.push_str(&render_scope(&self.global));
        out.push('\n');
        out.push_str(&render_scope(&self.workspace));
        out
    }
}

fn render_scope(r: &ScopeReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("[{}]\n", r.scope));

    // No index at all: nothing to check.
    if !r.index_present {
        out.push_str("  no index / empty\n");
        out.push_str("  hint: nothing to check for this scope.\n");
        return out;
    }

    // The index exists but could not be read — surface the reason instead of
    // misreporting it as "empty".
    if let Some(err) = &r.read_error {
        out.push_str(&format!("  error reading index: {err}\n"));
        out.push_str("  hint: the index exists but could not be read (corrupt or locked?).\n");
        return out;
    }

    // Present and readable, but no indexed chunks.
    if r.indexed_paths == 0 {
        out.push_str("  no index / empty\n");
        out.push_str("  hint: nothing to check for this scope.\n");
        return out;
    }

    out.push_str(&format!("  indexed paths: {}\n", r.indexed_paths));

    out.push_str(&format!("  orphan chunks: {}\n", r.orphan_paths.len()));
    for p in &r.orphan_paths {
        out.push_str(&format!("    - {p}\n"));
    }
    out.push_str("    hint: indexed files that no longer exist; a reindex will prune them.\n");

    match (&r.reindex_claim, r.reindex_claim_age_secs) {
        (None, _) => out.push_str("  stuck reindex claim: none\n"),
        (Some(v), Some(age)) if age > STALE_CLAIM_SECS => {
            out.push_str(&format!("  stuck reindex claim: {v} (age {age}s)\n"));
        }
        (Some(v), Some(age)) => {
            out.push_str(&format!("  active reindex claim: {v} (age {age}s)\n"));
        }
        (Some(v), None) => {
            out.push_str(&format!("  malformed reindex claim: {v}\n"));
        }
    }
    out.push_str("    hint: a leftover reindex lock resets after the stale threshold.\n");

    out.push_str(&format!(
        "  stale / low-access chunks (>{}d, 0 accesses): {}\n",
        STALE_AGE_DAYS, r.stale_chunks
    ));
    out.push_str("    hint: review whether these never-accessed chunks are still useful.\n");

    out.push_str(&format!(
        "  suspected redundant clusters (Jaccard >= {:.2}): {}\n",
        REDUNDANCY_THRESHOLD,
        r.redundant_clusters.len()
    ));
    for c in &r.redundant_clusters {
        out.push_str(&format!(
            "    - [{:.2}] {}\n",
            c.similarity,
            c.members.join(", ")
        ));
    }
    if r.redundancy_truncated {
        out.push_str(&format!(
            "    note: only the newest {MAX_REDUNDANCY_CHUNKS} chunks (by created_at desc) were compared.\n"
        ));
    }
    out.push_str("    hint: suspected duplicates only - review before merging.\n");

    out
}

/// Diagnose the memory index for both scopes. Read-only; never panics.
pub fn diagnose(storage: &MemoryStorage) -> DoctorReport {
    DoctorReport {
        global: diagnose_scope("global", storage.global_dir()),
        workspace: diagnose_scope("workspace", storage.workspace_dir()),
    }
}

fn diagnose_scope(scope: &str, dir: &Path) -> ScopeReport {
    let mut report = ScopeReport {
        scope: scope.to_string(),
        ..ScopeReport::default()
    };

    // Only ever look at an existing index — never create one.
    let db_path = dir.join("index.sqlite");
    if !db_path.exists() {
        return report;
    }
    report.index_present = true;

    let snap = match read_readonly(&db_path) {
        Ok(s) => s,
        Err(e) => {
            report.read_error = Some(e);
            return report;
        }
    };

    report.indexed_paths = snap.indexed_paths.len();
    for path in &snap.indexed_paths {
        if !Path::new(path).exists() {
            report.orphan_paths.push(path.clone());
        }
    }
    report.reindex_claim = snap.reindex_claim;
    report.reindex_claim_age_secs = snap.reindex_claim_age_secs;

    let stale_cutoff = unix_now() - STALE_AGE_DAYS * SECS_PER_DAY;
    report.stale_chunks = snap
        .chunk_meta
        .iter()
        .filter(|c| c.created_at < stale_cutoff && c.access_count == 0)
        .count();

    report.redundancy_truncated = snap.chunk_meta.len() > MAX_REDUNDANCY_CHUNKS;
    report.redundant_clusters = find_redundant_clusters(&snap.redundancy_chunks);

    report
}

/// Read a read-only snapshot of the index at `db_path`.
///
/// Returns `Err(reason)` when the database cannot be opened or a required
/// query fails — deliberately distinct from a successfully-read but empty
/// index, so the report never renders a read failure as "empty".
///
/// Two queries keep memory bounded and the redundancy subset reproducible:
/// - **A** (full, no `text`): `id, path, created_at, access_count` — the
///   distinct-path (orphan) set and the stale facts.
/// - **B** (with `text`, ordered + limited): the newest
///   [`MAX_REDUNDANCY_CHUNKS`] chunks for the redundancy scan.
fn read_readonly(db_path: &Path) -> Result<IndexSnapshot, String> {
    let conn = cf_sqlite_journal::JournalMode::for_db_path(db_path)
        .open_readonly(db_path)
        .map_err(|e| format!("cannot open index: {e}"))?;

    // Query A: full chunk metadata WITHOUT text — orphan (path) + stale.
    // `id` is selected for parity with the index schema but only `path`,
    // `created_at` and `access_count` are consumed here.
    let mut stmt = conn
        .prepare("SELECT id, path, created_at, access_count FROM chunks")
        .map_err(|e| format!("cannot read chunks: {e}"))?;
    let chunk_meta = stmt
        .query_map([], |row| {
            Ok(ChunkMeta {
                path: row.get(1)?,
                created_at: row.get(2)?,
                access_count: row.get(3)?,
            })
        })
        .map_err(|e| format!("cannot read chunks: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("cannot read chunks: {e}"))?;
    drop(stmt);

    // Distinct indexed paths (mirrors `MemoryIndex::all_indexed_paths`).
    let mut indexed_paths: Vec<String> = chunk_meta.iter().map(|c| c.path.clone()).collect();
    indexed_paths.sort();
    indexed_paths.dedup();

    // Reindex claim (mirrors `MemoryIndex::get_reindex_claim`).
    let reindex_claim: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'reindex_claim'",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .filter(|s| !s.trim().is_empty());
    let reindex_claim_age_secs = reindex_claim.as_deref().and_then(claim_age_secs);

    // Query B: newest chunks WITH text, ordered + limited — redundancy only.
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, text FROM chunks ORDER BY created_at DESC LIMIT {MAX_REDUNDANCY_CHUNKS}"
        ))
        .map_err(|e| format!("cannot read chunks: {e}"))?;
    let redundancy_chunks = stmt
        .query_map([], |row| {
            Ok(RedundancyChunk {
                id: row.get(0)?,
                text: row.get(1)?,
            })
        })
        .map_err(|e| format!("cannot read chunks: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("cannot read chunks: {e}"))?;

    Ok(IndexSnapshot {
        indexed_paths,
        reindex_claim,
        reindex_claim_age_secs,
        chunk_meta,
        redundancy_chunks,
    })
}

/// Parse a `pid:unix_ts` reindex claim and return its age in seconds.
///
/// Mirrors `MemoryIndex::try_claim_reindex`'s format (a numeric pid, then the
/// unix timestamp after the first `:`). Returns `None` for a malformed value.
fn claim_age_secs(claim: &str) -> Option<i64> {
    let (_, ts_str) = claim.split_once(':')?;
    let ts: i64 = ts_str.trim().parse().ok()?;
    Some(unix_now() - ts)
}

/// Group chunks into connected components whose pairwise token-Jaccard
/// similarity is at or above [`REDUNDANCY_THRESHOLD`].
///
/// The input is already capped (and ordered) by the caller; each cluster's
/// `similarity` is the highest pairwise similarity within it (always >= the
/// threshold for any reported cluster).
fn find_redundant_clusters(chunks: &[RedundancyChunk]) -> Vec<RedundantCluster> {
    // Lowercase once (mmr::tokenize expects pre-lowered input), then tokenize.
    let lowered: Vec<String> = chunks.iter().map(|c| c.text.to_lowercase()).collect();
    let tokens: Vec<HashSet<&str>> = lowered.iter().map(|s| tokenize(s)).collect();

    let n = chunks.len();
    let mut parent: Vec<usize> = (0..n).collect();

    for i in 0..n {
        if tokens[i].is_empty() {
            continue;
        }
        for j in (i + 1)..n {
            if tokens[j].is_empty() {
                continue;
            }
            if jaccard_similarity(&tokens[i], &tokens[j]) >= REDUNDANCY_THRESHOLD {
                union(&mut parent, i, j);
            }
        }
    }

    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }

    let mut clusters = Vec::new();
    for members in groups.values() {
        if members.len() < 2 {
            continue;
        }
        let mut max_sim = 0.0_f64;
        for (a, &i) in members.iter().enumerate() {
            for &j in &members[a + 1..] {
                let s = jaccard_similarity(&tokens[i], &tokens[j]);
                if s > max_sim {
                    max_sim = s;
                }
            }
        }
        let mut ids: Vec<String> = members.iter().map(|&i| chunks[i].id.clone()).collect();
        ids.sort();
        clusters.push(RedundantCluster {
            similarity: max_sim,
            members: ids,
        });
    }
    clusters.sort_by(|a, b| a.members.first().cmp(&b.members.first()));

    clusters
}

/// Union-find root lookup with path compression.
fn find(parent: &mut [usize], x: usize) -> usize {
    let mut root = x;
    while parent[root] != root {
        root = parent[root];
    }
    let mut cur = x;
    while parent[cur] != root {
        let next = parent[cur];
        parent[cur] = root;
        cur = next;
    }
    root
}

/// Union two elements (attaching the larger root under the smaller one).
fn union(parent: &mut [usize], a: usize, b: usize) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra != rb {
        parent[ra.max(rb)] = ra.min(rb);
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{init_sqlite_vec, MemoryIndex};
    use cf_config::xai_grok_config_types::MemoryIndexConfig;
    use tempfile::TempDir;

    fn test_storage(tmp: &TempDir) -> MemoryStorage {
        let global = tmp.path().join("memory");
        let workspace = global.join("test_ws");
        MemoryStorage::with_paths(global, workspace)
    }

    fn redundancy_chunk(id: &str, text: &str) -> RedundancyChunk {
        RedundancyChunk {
            id: id.to_string(),
            text: text.to_string(),
        }
    }

    /// Overwrite the `reindex_claim` meta value via a short-lived write
    /// connection (test setup only — `diagnose` itself is read-only).
    fn set_claim(db_path: &Path, value: &str) {
        let conn = cf_sqlite_journal::JournalMode::for_db_path(db_path)
            .open(db_path)
            .unwrap();
        conn.execute(
            "UPDATE meta SET value = ?1 WHERE key = 'reindex_claim'",
            rusqlite::params![value],
        )
        .unwrap();
    }

    /// Empty / nonexistent index must yield an empty report and never panic.
    #[test]
    fn empty_index_yields_empty_report_without_panic() {
        let tmp = TempDir::new().unwrap();
        let storage = test_storage(&tmp);

        let report = diagnose(&storage);

        assert!(!report.global.index_present);
        assert!(!report.workspace.index_present);
        assert!(report.global.orphan_paths.is_empty());
        assert!(report.workspace.orphan_paths.is_empty());
        assert_eq!(report.global.stale_chunks, 0);
        assert!(report.workspace.redundant_clusters.is_empty());

        let rendered = report.render();
        assert!(rendered.starts_with("Memory doctor report"));
        assert!(rendered.contains("no index / empty"));
    }

    /// A chunk whose source file was deleted must be reported as an orphan.
    #[test]
    fn detects_orphan_paths_for_deleted_files() {
        let tmp = TempDir::new().unwrap();
        let storage = test_storage(&tmp);
        let db_path = storage.workspace_dir().join("index.sqlite");
        std::fs::create_dir_all(storage.workspace_dir()).unwrap();

        init_sqlite_vec();
        let mut idx =
            MemoryIndex::open_or_create(&db_path, storage.clone(), MemoryIndexConfig::default(), 4)
                .unwrap();

        let live = tmp.path().join("live.md");
        let doomed = tmp.path().join("doomed.md");
        std::fs::write(&live, "# Live\n\nkept content.").unwrap();
        std::fs::write(&doomed, "# Doomed\n\ngone content.").unwrap();
        idx.reindex_file(&live, "workspace").unwrap();
        idx.reindex_file(&doomed, "workspace").unwrap();
        drop(idx);

        // Remove one source file → its chunks become orphans.
        std::fs::remove_file(&doomed).unwrap();

        let report = diagnose(&storage);
        assert!(report.workspace.index_present);
        assert!(
            report
                .workspace
                .orphan_paths
                .iter()
                .any(|p| p.ends_with("doomed.md")),
            "expected doomed.md in orphans, got {:?}",
            report.workspace.orphan_paths
        );
        assert!(
            !report
                .workspace
                .orphan_paths
                .iter()
                .any(|p| p.ends_with("live.md")),
            "live.md must not be reported as an orphan"
        );

        let rendered = report.render();
        assert!(rendered.contains("doomed.md"));
        assert!(rendered.contains("orphan chunks: 1"));
    }

    /// Near-identical chunk texts must be grouped into one suspected cluster.
    #[test]
    fn detects_redundant_clusters() {
        let chunks = vec![
            redundancy_chunk("a", "rust async programming patterns"),
            redundancy_chunk("b", "rust async programming patterns tutorial"),
            redundancy_chunk("c", "completely unrelated cooking recipe"),
        ];

        let clusters = find_redundant_clusters(&chunks);

        assert_eq!(clusters.len(), 1, "exactly one cluster expected");
        assert_eq!(clusters[0].members, vec!["a".to_string(), "b".to_string()]);
        assert!(clusters[0].similarity >= REDUNDANCY_THRESHOLD);
    }

    /// `render()` produces deterministic, plain-text output. Asserts the exact
    /// bytes so the report layout is pinned (this doubles as the sample).
    #[test]
    fn render_plain_text_layout() {
        let report = DoctorReport {
            global: ScopeReport {
                scope: "global".to_string(),
                index_present: true,
                read_error: None,
                indexed_paths: 4,
                orphan_paths: vec!["/mem/old.md".to_string()],
                reindex_claim: Some("4242:1700000000".to_string()),
                reindex_claim_age_secs: Some(120),
                stale_chunks: 2,
                redundant_clusters: vec![RedundantCluster {
                    similarity: 0.85,
                    members: vec!["a.md:0".to_string(), "a.md:1".to_string()],
                }],
                redundancy_truncated: false,
            },
            workspace: ScopeReport {
                scope: "workspace".to_string(),
                ..ScopeReport::default()
            },
        };

        let rendered = report.render();
        println!("{rendered}");

        let expected = "\
Memory doctor report
====================
[global]
  indexed paths: 4
  orphan chunks: 1
    - /mem/old.md
    hint: indexed files that no longer exist; a reindex will prune them.
  stuck reindex claim: 4242:1700000000 (age 120s)
    hint: a leftover reindex lock resets after the stale threshold.
  stale / low-access chunks (>30d, 0 accesses): 2
    hint: review whether these never-accessed chunks are still useful.
  suspected redundant clusters (Jaccard >= 0.80): 1
    - [0.85] a.md:0, a.md:1
    hint: suspected duplicates only - review before merging.

[workspace]
  no index / empty
  hint: nothing to check for this scope.
";
        assert_eq!(rendered, expected);
    }

    /// R2: `diagnose` must never write to the index. A read-write open would
    /// re-create the missing `embedding_dimensions` meta row and could touch
    /// the schema; here we remove that row, set a distinctive claim, run the
    /// scan, and assert both survived untouched (read back via a separate
    /// read-only connection).
    #[test]
    fn diagnose_never_writes_to_the_index() {
        let tmp = TempDir::new().unwrap();
        let storage = test_storage(&tmp);
        let db_path = storage.workspace_dir().join("index.sqlite");
        std::fs::create_dir_all(storage.workspace_dir()).unwrap();

        init_sqlite_vec();
        let idx =
            MemoryIndex::open_or_create(&db_path, storage.clone(), MemoryIndexConfig::default(), 4)
                .unwrap();
        // Simulate an index missing its dimension record, with a live claim.
        idx.db()
            .execute("DELETE FROM meta WHERE key = 'embedding_dimensions'", [])
            .unwrap();
        idx.db()
            .execute(
                "UPDATE meta SET value = '9999:1700000000' WHERE key = 'reindex_claim'",
                [],
            )
            .unwrap();
        drop(idx);

        let report = diagnose(&storage);
        assert!(report.workspace.index_present);
        assert!(report.workspace.read_error.is_none());

        // Read the meta table back through a fresh read-only connection so the
        // check itself cannot write.
        let conn = cf_sqlite_journal::JournalMode::for_db_path(&db_path)
            .open_readonly(&db_path)
            .unwrap();
        let dims: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'embedding_dimensions'",
                [],
                |r| r.get(0),
            )
            .ok();
        assert!(
            dims.is_none(),
            "diagnose must not write embedding_dimensions back (got {dims:?})"
        );
        let claim: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'reindex_claim'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            claim, "9999:1700000000",
            "diagnose must not touch the reindex claim"
        );
    }

    /// R3: a readable failure must be reported as an error, not "empty".
    #[test]
    fn read_failure_is_distinct_from_empty() {
        let tmp = TempDir::new().unwrap();
        let storage = test_storage(&tmp);
        let dir = storage.workspace_dir();
        std::fs::create_dir_all(dir).unwrap();
        // A file named index.sqlite that is not a SQLite database.
        std::fs::write(dir.join("index.sqlite"), b"not a sqlite database").unwrap();

        let report = diagnose(&storage);
        assert!(report.workspace.index_present);
        assert!(
            report.workspace.read_error.is_some(),
            "expected a read error, got {:?}",
            report.workspace.read_error
        );

        let rendered = report.render();
        println!("{rendered}");
        assert!(
            rendered.contains("error reading index"),
            "render must surface the read error, got:\n{rendered}"
        );
    }

    /// R3: a present but chunk-less index still renders as empty (not error).
    #[test]
    fn empty_but_present_index_renders_empty() {
        let tmp = TempDir::new().unwrap();
        let storage = test_storage(&tmp);
        let db_path = storage.workspace_dir().join("index.sqlite");
        std::fs::create_dir_all(storage.workspace_dir()).unwrap();

        init_sqlite_vec();
        drop(
            MemoryIndex::open_or_create(
                &db_path,
                storage.clone(),
                MemoryIndexConfig::default(),
                4,
            )
            .unwrap(),
        );

        let report = diagnose(&storage);
        assert!(report.workspace.index_present);
        assert!(report.workspace.read_error.is_none());
        assert_eq!(report.workspace.indexed_paths, 0);

        let rendered = render_scope(&report.workspace);
        assert!(rendered.contains("no index / empty"), "got:\n{rendered}");
        assert!(!rendered.contains("error reading index"));
    }

    /// R6: only a claim older than [`STALE_CLAIM_SECS`] is "stuck".
    #[test]
    fn claim_staleness_is_time_aware() {
        let tmp = TempDir::new().unwrap();
        let storage = test_storage(&tmp);
        let db_path = storage.workspace_dir().join("index.sqlite");
        std::fs::create_dir_all(storage.workspace_dir()).unwrap();

        init_sqlite_vec();
        let mut idx =
            MemoryIndex::open_or_create(&db_path, storage.clone(), MemoryIndexConfig::default(), 4)
                .unwrap();
        // One indexed chunk so the claim section is not short-circuited by the
        // "empty" branch.
        let file = tmp.path().join("note.md");
        std::fs::write(&file, "# Note\n\nsome content.").unwrap();
        idx.reindex_file(&file, "workspace").unwrap();
        drop(idx);

        // Fresh claim (10s old) → active, not stuck.
        set_claim(&db_path, &format!("1234:{}", unix_now() - 10));
        let report = diagnose(&storage);
        assert!(report.workspace.reindex_claim.is_some());
        let rendered = render_scope(&report.workspace);
        assert!(
            rendered.contains("active reindex claim"),
            "fresh claim must not be stuck, got:\n{rendered}"
        );
        assert!(!rendered.contains("stuck"));

        // Stale claim (300s old) → stuck, with value + age shown.
        set_claim(&db_path, &format!("1234:{}", unix_now() - 300));
        let report = diagnose(&storage);
        let rendered = render_scope(&report.workspace);
        assert!(
            rendered.contains("stuck reindex claim: 1234:"),
            "stale claim must be reported, got:\n{rendered}"
        );
        assert!(rendered.contains("age 3"), "age shown, got:\n{rendered}");
    }

    /// R6: an unparseable claim value is reported as malformed.
    #[test]
    fn malformed_claim_is_reported() {
        let tmp = TempDir::new().unwrap();
        let storage = test_storage(&tmp);
        let db_path = storage.workspace_dir().join("index.sqlite");
        std::fs::create_dir_all(storage.workspace_dir()).unwrap();

        init_sqlite_vec();
        let mut idx =
            MemoryIndex::open_or_create(&db_path, storage.clone(), MemoryIndexConfig::default(), 4)
                .unwrap();
        let file = tmp.path().join("note.md");
        std::fs::write(&file, "# Note\n\nsome content.").unwrap();
        idx.reindex_file(&file, "workspace").unwrap();
        drop(idx);

        set_claim(&db_path, "not-a-timestamp");
        let report = diagnose(&storage);
        assert_eq!(report.workspace.reindex_claim.as_deref(), Some("not-a-timestamp"));
        assert!(report.workspace.reindex_claim_age_secs.is_none());

        let rendered = render_scope(&report.workspace);
        assert!(rendered.contains("malformed"), "got:\n{rendered}");
        assert!(!rendered.contains("stuck"));
    }
}
