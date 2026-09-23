//! `memory doctor` — a read-only health scan of the memory index.
//!
//! [`diagnose`] inspects both the global and the workspace memory scope and
//! reports four classes of problems:
//!
//! 1. **Orphan chunks** — indexed paths whose source file no longer exists.
//! 2. **Stuck reindex claims** — a leftover `reindex_claim` lock value.
//! 3. **Stale / low-access chunks** — older than [`STALE_AGE_DAYS`] and never
//!    accessed.
//! 4. **Suspected redundant clusters** — chunk texts whose token-Jaccard
//!    similarity is at or above [`REDUNDANCY_THRESHOLD`].
//!
//! The scan is strictly read-only: it never creates `index.sqlite` (an absent
//! index yields an empty report) and never mutates the schema. Per-chunk facts
//! are read through a read-only SQLite connection; the two doctor helpers
//! prepared on [`crate::index::MemoryIndex`] (`get_reindex_claim`,
//! `all_indexed_paths`) are reused for the claim and orphan checks.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use cf_config::xai_grok_config_types::MemoryIndexConfig;

use crate::index::MemoryIndex;
use crate::mmr::{jaccard_similarity, tokenize};
use crate::storage::MemoryStorage;

/// A chunk older than this many days with zero recorded accesses is "stale".
const STALE_AGE_DAYS: i64 = 30;

/// Token-Jaccard similarity at/above which two chunks are "suspected" dupes.
const REDUNDANCY_THRESHOLD: f64 = 0.8;

/// Upper bound on chunks fed into the O(n²) redundancy scan.
const MAX_REDUNDANCY_CHUNKS: usize = 2000;

/// Embedding dimensions assumed when the index stores none.
const DEFAULT_EMBED_DIMENSIONS: usize = 1024;

const SECS_PER_DAY: i64 = 86_400;

/// Minimal per-chunk facts needed for the stale / redundancy checks.
#[derive(Debug, Clone)]
struct ChunkFact {
    id: String,
    text: String,
    access_count: i64,
    created_at: i64,
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
    /// Number of distinct indexed file paths.
    pub indexed_paths: usize,
    /// Indexed paths whose source file is gone.
    pub orphan_paths: Vec<String>,
    /// A non-empty leftover reindex claim, if any.
    pub reindex_claim: Option<String>,
    /// Count of stale / never-accessed chunks.
    pub stale_chunks: usize,
    /// Suspected redundant clusters.
    pub redundant_clusters: Vec<RedundantCluster>,
    /// True when the redundancy scan was capped by [`MAX_REDUNDANCY_CHUNKS`].
    pub redundancy_truncated: bool,
}

impl ScopeReport {
    /// A scope is "empty" when there is no index or no indexed chunks at all.
    fn is_empty(&self) -> bool {
        !self.index_present || self.indexed_paths == 0
    }
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

    if r.is_empty() {
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

    match &r.reindex_claim {
        Some(v) => out.push_str(&format!("  stuck reindex claim: {v}\n")),
        None => out.push_str("  stuck reindex claim: none\n"),
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
            "    note: only the first {MAX_REDUNDANCY_CHUNKS} chunks were compared.\n"
        ));
    }
    out.push_str("    hint: suspected duplicates only - review before merging.\n");

    out
}

/// Diagnose the memory index for both scopes. Read-only; never panics.
pub fn diagnose(storage: &MemoryStorage) -> DoctorReport {
    DoctorReport {
        global: diagnose_scope(storage, "global", storage.global_dir()),
        workspace: diagnose_scope(storage, "workspace", storage.workspace_dir()),
    }
}

fn diagnose_scope(storage: &MemoryStorage, scope: &str, dir: &Path) -> ScopeReport {
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

    // Read per-chunk facts + the stored embedding dimensions over a read-only
    // connection (never creates or alters the file).
    let (dims, chunks) = match read_readonly(&db_path) {
        Some(v) => v,
        None => return report,
    };

    // Reuse the doctor helpers on `MemoryIndex`. Opened only because the file
    // already exists; `dims` is passed so the dimension check is a no-op.
    if let Ok(index) = MemoryIndex::open_or_create(
        &db_path,
        storage.clone(),
        MemoryIndexConfig::default(),
        dims,
    ) {
        if let Ok(paths) = index.all_indexed_paths() {
            report.indexed_paths = paths.len();
            for path in paths {
                if !Path::new(&path).exists() {
                    report.orphan_paths.push(path);
                }
            }
        }
        let claim = index.get_reindex_claim();
        if !claim.trim().is_empty() {
            report.reindex_claim = Some(claim);
        }
    }

    let stale_cutoff = unix_now() - STALE_AGE_DAYS * SECS_PER_DAY;
    report.stale_chunks = chunks
        .iter()
        .filter(|c| c.created_at < stale_cutoff && c.access_count == 0)
        .count();

    let (clusters, truncated) = find_redundant_clusters(&chunks);
    report.redundant_clusters = clusters;
    report.redundancy_truncated = truncated;

    report
}

/// Read chunk-level facts and stored embedding dimensions via a read-only
/// connection. Returns `None` if the database cannot be opened/queried.
fn read_readonly(db_path: &Path) -> Option<(usize, Vec<ChunkFact>)> {
    let conn = cf_sqlite_journal::JournalMode::for_db_path(db_path)
        .open_readonly(db_path)
        .ok()?;

    let dims: Option<usize> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'embedding_dimensions'",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .and_then(|s| s.parse().ok());

    let mut stmt = conn
        .prepare("SELECT id, text, access_count, created_at FROM chunks")
        .ok()?;
    let chunks = stmt
        .query_map([], |row| {
            Ok(ChunkFact {
                id: row.get(0)?,
                text: row.get(1)?,
                access_count: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .ok()?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();

    Some((dims.unwrap_or(DEFAULT_EMBED_DIMENSIONS), chunks))
}

/// Group chunks into connected components whose pairwise token-Jaccard
/// similarity is at or above [`REDUNDANCY_THRESHOLD`].
///
/// Returns the clusters plus whether the scan was capped. Each cluster's
/// `similarity` is the highest pairwise similarity within it (always >= the
/// threshold for any reported cluster).
fn find_redundant_clusters(chunks: &[ChunkFact]) -> (Vec<RedundantCluster>, bool) {
    let truncated = chunks.len() > MAX_REDUNDANCY_CHUNKS;
    let considered = &chunks[..chunks.len().min(MAX_REDUNDANCY_CHUNKS)];

    // Lowercase once (mmr::tokenize expects pre-lowered input), then tokenize.
    let lowered: Vec<String> = considered.iter().map(|c| c.text.to_lowercase()).collect();
    let tokens: Vec<HashSet<&str>> = lowered.iter().map(|s| tokenize(s)).collect();

    let n = considered.len();
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
        let mut ids: Vec<String> = members.iter().map(|&i| considered[i].id.clone()).collect();
        ids.sort();
        clusters.push(RedundantCluster {
            similarity: max_sim,
            members: ids,
        });
    }
    clusters.sort_by(|a, b| a.members.first().cmp(&b.members.first()));

    (clusters, truncated)
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
    use crate::index::init_sqlite_vec;
    use tempfile::TempDir;

    fn test_storage(tmp: &TempDir) -> MemoryStorage {
        let global = tmp.path().join("memory");
        let workspace = global.join("test_ws");
        MemoryStorage::with_paths(global, workspace)
    }

    fn chunk_fact(id: &str, text: &str) -> ChunkFact {
        ChunkFact {
            id: id.to_string(),
            text: text.to_string(),
            access_count: 0,
            created_at: 0,
        }
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
            chunk_fact("a", "rust async programming patterns"),
            chunk_fact("b", "rust async programming patterns tutorial"),
            chunk_fact("c", "completely unrelated cooking recipe"),
        ];

        let (clusters, truncated) = find_redundant_clusters(&chunks);

        assert!(!truncated);
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
                indexed_paths: 4,
                orphan_paths: vec!["/mem/old.md".to_string()],
                reindex_claim: Some("4242:1700000000".to_string()),
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
  stuck reindex claim: 4242:1700000000
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
}
