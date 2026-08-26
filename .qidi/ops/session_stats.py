# -*- coding: utf-8 -*-
"""Session jsonl stats: date distribution + archive candidates (>N days)."""
import os, sys, time
sys.stdout.reconfigure(encoding="utf-8")

BASE = r".qidi/sessions"
files = []
for dirpath, dirnames, filenames in os.walk(BASE, onerror=lambda e: None):
    for fn in filenames:
        if fn.endswith(".jsonl"):
            p = os.path.join(dirpath, fn)
            try:
                st = os.stat(p)
                files.append((p, st.st_mtime, st.st_size))
            except OSError:
                pass

now = time.time()
by_day = {}
for p, mt, sz in files:
    day = time.strftime("%m-%d", time.localtime(mt))
    by_day.setdefault(day, [0, 0])
    by_day[day][0] += 1
    by_day[day][1] += sz

print(f"total jsonl: {len(files)}, {sum(s for _,_,s in files)/1e6:.1f} MB")
print("--- by date ---")
for day in sorted(by_day):
    c, s = by_day[day]
    print(f"  {day}  {c:4d} files  {s/1e6:8.1f} MB")

for threshold_days in (30, 14, 7):
    cut = now - threshold_days * 86400
    old = [(p, s) for p, mt, s in files if mt < cut]
    if old:
        print(f"--- older than {threshold_days}d: {len(old)} files, {sum(s for _,s in old)/1e6:.1f} MB ---")
    else:
        print(f"--- older than {threshold_days}d: none ---")
