# -*- coding: utf-8 -*-
"""session_trim.py — tail-truncate oversized session jsonl files to reclaim disk.

Sessions root is derived from this file's location (.qidi/ops/ -> ../sessions/),
never from cwd. Scans updates.jsonl / events.jsonl / chat_history.jsonl under the
sessions tree, skipping compaction_checkpoints / rewind_points recovery dirs.
A file is a candidate only if BOTH: size > --max-mb (default 20MB) AND last
modified earlier than --idle-days (default 1d) -> active sessions never touched.

Default is dry-run (prints the candidate list, does nothing). With --execute each
candidate is first fully copied to _archive/sessions-trim/<rel>.<ts>.bak, then the
original is rewritten keeping only its tail --keep-lines lines.

Long paths: sessions contains URL-encoded paths >260 chars. Every open/os call
tries the plain path first and retries with the \\\\?\\ prefix on FileNotFoundError;
if that still fails the file is WARNed and skipped without aborting.
"""
import os, sys, time, argparse
from collections import deque

sys.stdout.reconfigure(encoding="utf-8")

TARGET_NAMES = ("updates.jsonl", "events.jsonl", "chat_history.jsonl")
EXCLUDE_DIRS = ("compaction_checkpoints", "rewind_points")
BACKUP_DIR = "sessions-trim"
CHUNK = 1 << 20  # 1MB copy chunks -> safe for 178MB files


def extended(p):
    """Return the \\\\?\\-prefixed absolute path for >260-char Windows paths."""
    if os.name != "nt" or p.startswith("\\\\?\\"):
        return p
    return "\\\\?\\" + os.path.abspath(p)


def open_any(p, *args, **kw):
    try:
        return open(p, *args, **kw)
    except FileNotFoundError:
        return open(extended(p), *args, **kw)


def stat_any(p):
    try:
        return os.stat(p)
    except FileNotFoundError:
        return os.stat(extended(p))


def makedirs_any(p):
    try:
        os.makedirs(p, exist_ok=True)
    except FileNotFoundError:
        os.makedirs(extended(p), exist_ok=True)


def remove_any(p):
    try:
        os.remove(p)
    except FileNotFoundError:
        os.remove(extended(p))


def replace_any(src, dst):
    try:
        os.replace(src, dst)
    except FileNotFoundError:
        os.replace(extended(src), extended(dst))


def copyfile_any(src, dst):
    """Streaming full copy with long-path fallback; returns bytes copied."""
    def _do(a, b):
        total = 0
        with open(a, "rb") as fin, open(b, "wb") as fout:
            while True:
                chunk = fin.read(CHUNK)
                if not chunk:
                    break
                fout.write(chunk)
                total += len(chunk)
        return total
    try:
        return _do(src, dst)
    except FileNotFoundError:
        return _do(extended(src), extended(dst))


def scan(root, max_bytes, cutoff):
    """Walk sessions, prune recovery dirs, return (scanned, candidates)."""
    scanned, candidates = 0, []
    for dirpath, dirnames, filenames in os.walk(root, onerror=lambda e: None):
        dirnames[:] = [d for d in dirnames if d not in EXCLUDE_DIRS]
        for fn in filenames:
            if fn not in TARGET_NAMES:
                continue
            p = os.path.join(dirpath, fn)
            scanned += 1
            try:
                st = stat_any(p)
            except OSError as e:
                print(f"WARN stat failed: {p} ({e})")
                continue
            if st.st_size > max_bytes and st.st_mtime < cutoff:
                candidates.append((p, st.st_size, st.st_mtime))
    candidates.sort(key=lambda t: t[1], reverse=True)
    return scanned, candidates


def tail_lines(path, keep):
    """Return the last `keep` raw byte lines (deque, memory-safe)."""
    dq = deque(maxlen=keep)
    with open_any(path, "rb") as f:
        for line in f:
            dq.append(line)
    return dq


def mb(n):
    return n / 1e6


def main():
    ap = argparse.ArgumentParser(
        description="Tail-truncate oversized session jsonl files (default dry-run).")
    ap.add_argument("--execute", action="store_true",
                    help="actually trim; default is dry-run only")
    ap.add_argument("--max-mb", type=float, default=20.0,
                    help="candidate if file larger than this many MB (default 20)")
    ap.add_argument("--keep-lines", type=int, default=2000,
                    help="keep this many tail lines when trimming (default 2000)")
    ap.add_argument("--idle-days", type=float, default=1.0,
                    help="candidate if mtime older than this many days (default 1)")
    args = ap.parse_args()

    ops_dir = os.path.dirname(os.path.abspath(__file__))
    root = os.path.normpath(os.path.join(ops_dir, "..", "sessions"))
    backup_root = os.path.normpath(os.path.join(ops_dir, "..", "..", "_archive", BACKUP_DIR))

    mode = "EXECUTE" if args.execute else "DRY-RUN"
    print("== session_trim ==")
    print(f"mode:         {mode}" + ("" if args.execute else "  (add --execute to trim)"))
    print(f"sessions:     {root}")
    print(f"backup root:  {backup_root}")
    print(f"max-mb={args.max_mb:g}  keep-lines={args.keep_lines}  idle-days={args.idle_days:g}")
    print()

    max_bytes = args.max_mb * 1024 * 1024
    cutoff = time.time() - args.idle_days * 86400

    scanned, candidates = scan(root, max_bytes, cutoff)
    print(f"scanned {scanned} target jsonl files; candidates: {len(candidates)}")
    print("-" * 78)
    for i, (p, sz, mt) in enumerate(candidates, 1):
        age = (time.time() - mt) / 86400
        print(f"[{i}] {p}")
        print(f"    size={mb(sz):9.1f} MB  age={age:6.1f} d  -> keep tail {args.keep_lines} lines")
    print("-" * 78)

    if not args.execute:
        print(f"[dry-run] nothing written. {len(candidates)} candidate(s) would be trimmed.")
        return

    # --execute path
    trimmed, skipped = 0, 0
    reclaimed = 0
    backup_start = time.time()
    backup_bytes = 0
    stamp = time.strftime("%Y%m%d-%H%M%S")

    for p, sz, mt in candidates:
        rel = os.path.relpath(p, root)
        backup_path = os.path.join(backup_root, rel + f".{stamp}.bak")
        try:
            makedirs_any(os.path.dirname(backup_path))
            backup_bytes += copyfile_any(p, backup_path)

            dq = tail_lines(p, args.keep_lines)
            new_bytes = b"".join(dq)
            tmp = p + ".trimtmp"
            try:
                with open_any(tmp, "wb") as f:
                    f.write(new_bytes)
                replace_any(tmp, p)
            finally:
                try:
                    remove_any(tmp)
                except FileNotFoundError:
                    pass
            st_new = stat_any(p).st_size
            reclaimed += sz - st_new
            trimmed += 1
            print(f"[trim] {p}")
            print(f"       size: {mb(sz):9.1f} MB -> {mb(st_new):9.1f} MB  (reclaim {mb(sz - st_new):8.1f} MB)")
            print(f"       backup: {backup_path}")
        except Exception as e:  # any failure -> WARN and move on, never abort
            skipped += 1
            print(f"WARN skip {p}: {type(e).__name__}: {e}")

    backup_elapsed = time.time() - backup_start
    print()
    print("== summary ==")
    print(f"scanned files: {scanned}")
    print(f"candidates:    {len(candidates)}")
    print(f"trimmed:       {trimmed}")
    print(f"skipped:       {skipped}")
    print(f"backup time:   {backup_elapsed:.2f} s ({backup_bytes / 1e6:.1f} MB copied)")
    print(f"reclaimed:     {mb(reclaimed):.1f} MB")


if __name__ == "__main__":
    main()
