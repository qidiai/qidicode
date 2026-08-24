//! Zero-copy reads of append-only JSONL files via memory mapping.
//!
//! `updates.jsonl` is append-only by contract (defensive-patterns §3.1), which
//! makes mmap safe for read paths: a concurrent appender can only *extend* the
//! file after the offset captured at map time, so the mapped window is a frozen
//! prefix of the log. Readers that need a stable view (replay, fork copy) map
//! once and never re-map.
//!
//! This mirrors the codex `rollout` crate's frozen-prefix discipline
//! (`ReverseJsonlScanner::new_at`) without pulling in the whole scanner: the
//! existing line filters (`filter_rewind_lines`, `prepare_replay_lines`) operate
//! on `&str` slices and stay untouched — mmap only removes the up-front
//! `read_to_string` copy of files that have grown to hundreds of MB.
//!
//! Windows note: the map is opened with `SHARE_MODE` allowing subsequent
//! writers (FILE_SHARE_READ | FILE_SHARE_WRITE), matching the append path's
//! `OpenOptions::append`. An exclusive writer would deadlock the resume path.

use std::fs::File;
use std::io;
use std::path::Path;

use memmap2::Mmap;

/// A memory-mapped, read-only view of an append-only JSONL file.
///
/// The view is a snapshot taken at map time: `len()` is the byte length frozen
/// when the file was mapped, and lines are only those fully contained in that
/// prefix. A torn trailing line (crashed append) is exposed as-is — callers
/// keep their existing corruption-tolerant skip logic.
pub(crate) struct JsonlMmapView {
    mmap: Mmap,
    frozen_len: u64,
}

impl JsonlMmapView {
    /// Map `path` for reading, freezing the current length.
    ///
    /// Returns `Ok(None)` when the file does not exist (callers treat that as
    /// "empty log", e.g. a fresh session) so this can replace the
    /// `if !path.exists() { return Ok(vec![]) }` dance in one call.
    pub(crate) fn open(path: &Path) -> io::Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let file = open_shared_read(path)?;
        // SAFETY: append-only contract (defensive-patterns §3.1) — nothing
        // mutates the mapped prefix; appends only extend past the mapped end.
        let mmap = unsafe { Mmap::map(&file)? };
        // Take the length FROM the map, not from a separate metadata() call:
        // an append racing between two stats would make the map longer than
        // frozen_len, and replay's delta path would re-send events after
        // frozen_len. mmap.len() is exactly the mapped window.
        let frozen_len = mmap.len() as u64;
        Ok(Some(Self { mmap, frozen_len }))
    }

    /// Byte length of the frozen prefix (== mapped window length).
    pub(crate) fn frozen_len(&self) -> u64 {
        self.frozen_len
    }

    /// The mapped bytes (the frozen prefix itself).
    pub(crate) fn bytes(&self) -> &[u8] {
        self.mmap.as_ref()
    }

    /// The mapped bytes as a `&str`, if the file is valid UTF-8.
    ///
    /// JSONL lines are UTF-8 JSON documents; the only realistic failure is a
    /// torn mid-codepoint append at the tail. Callers needing per-line
    /// tolerance (invalid bytes on one line must not hide the rest) should
    /// use [`lines`] instead, which skips undecodable lines individually.
    pub(crate) fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(self.bytes()).ok()
    }

    /// Lines of the frozen prefix, skipping empty/whitespace-only lines and
    /// lines that are not valid UTF-8.
    ///
    /// Per-line decoding matches the legacy byte-splitting readers: a torn
    /// or corrupt line is skipped without hiding the lines around it.
    /// Allocates only the line-slice vector (8 bytes per line), never the
    /// contents — the previous `read_to_string` path copied the whole file
    /// into the heap first.
    pub(crate) fn lines(&self) -> Vec<&str> {
        let bytes = self.bytes();
        let mut lines = Vec::new();
        let mut start = 0usize;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                push_if_valid(bytes, start, i, &mut lines);
                start = i + 1;
            }
        }
        // Trailing segment after the last '\n' (no terminator).
        push_if_valid(bytes, start, bytes.len(), &mut lines);
        lines
    }
}

/// Open a file for read while concurrent writers keep appending and other
/// handles may delete/rename it.
///
/// `std::fs::File::open` on Windows uses FILE_SHARE_READ | WRITE | DELETE by
/// default; we state all three explicitly so the mapping never blocks a
/// concurrent `delete_session` (`remove_dir_all`) for the seconds the map is
/// held during replay. Unix has no equivalent lock.
fn open_shared_read(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x1;
        const FILE_SHARE_WRITE: u32 = 0x2;
        const FILE_SHARE_DELETE: u32 = 0x4;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(path)
    }
    #[cfg(not(windows))]
    {
        std::fs::File::open(path)
    }
}

/// Append the byte range `[start, end)` to `lines` if it is non-empty,
/// non-whitespace, and valid UTF-8.
fn push_if_valid<'a>(bytes: &'a [u8], start: usize, end: usize, lines: &mut Vec<&'a str>) {
    if start >= end {
        return;
    }
    let Ok(line) = std::str::from_utf8(&bytes[start..end]) else {
        return;
    };
    if !line.trim().is_empty() {
        lines.push(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("jsonl-mmap-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_is_none() {
        let path = temp_dir().join("does-not-exist.jsonl");
        assert!(JsonlMmapView::open(&path).unwrap().is_none());
    }

    #[test]
    fn maps_lines_without_copying() {
        let path = temp_dir().join("maps-lines.jsonl");
        std::fs::write(&path, b"{\"a\":1}\n{\"a\":2}\n\n{\"a\":3}\n").unwrap();
        let view = JsonlMmapView::open(&path).unwrap().unwrap();
        let lines = view.lines();
        assert_eq!(lines, vec!["{\"a\":1}", "{\"a\":2}", "{\"a\":3}"]);
        // `{"a":1}\n{"a":2}\n\n{"a":3}\n` = 7+1+7+1+1+7+1 = 25 bytes; the
        // frozen length must equal the mapped window exactly (P1 audit fix:
        // taken from mmap.len(), not a second stat that can race appends).
        assert_eq!(view.frozen_len(), 25);
        assert_eq!(view.frozen_len() as usize, view.bytes().len());
    }

    #[test]
    fn empty_file_maps_to_no_lines() {
        let path = temp_dir().join("empty.jsonl");
        std::fs::write(&path, b"").unwrap();
        let view = JsonlMmapView::open(&path).unwrap().unwrap();
        assert!(view.lines().is_empty());
        assert_eq!(view.frozen_len(), 0);
    }

    #[test]
    fn torn_tail_line_is_exposed_not_hidden() {
        // A crashed append can leave a partial final line; readers must see
        // the complete lines plus the torn remainder, and reject it per-line.
        let path = temp_dir().join("torn.jsonl");
        std::fs::write(&path, b"{\"a\":1}\n{\"a\":2}\n{\"a\":3").unwrap();
        let view = JsonlMmapView::open(&path).unwrap().unwrap();
        let lines = view.lines();
        assert_eq!(lines, vec!["{\"a\":1}", "{\"a\":2}", "{\"a\":3"]);
        // The torn tail must fail JSON parsing, not crash the reader.
        assert!(serde_json::from_str::<serde_json::Value>(lines[2]).is_err());
    }

    #[test]
    fn invalid_utf8_line_is_skipped_but_neighbors_kept() {
        // Regression guard: one mid-file line with invalid UTF-8 bytes must
        // not hide the valid lines around it (legacy per-line behavior).
        let path = temp_dir().join("bad-utf8.jsonl");
        std::fs::write(&path, b"{\"a\":1}\n\xE2\x82\n{\"a\":3}\n").unwrap();
        let view = JsonlMmapView::open(&path).unwrap().unwrap();
        let lines = view.lines();
        assert_eq!(lines, vec!["{\"a\":1}", "{\"a\":3}"]);
    }
}
