# -*- coding: utf-8 -*-
"""sampling.py — 语料配额抽样 (T1 规格书 §1).

池与配额（排除规则优先）：
    A 标书本体 25  : 投标文件\\**\\*.docx，排除路径含 '业绩' 的、排除 >50MB
    B 业绩合同 10  : 投标文件\\业绩\\**\\*.docx，排除 >50MB
    C 审查/共享 10 : Desktop\\** ∪ 审核标书\\**，排除 >50MB
    D 大件冒烟 5   : 全库 >50MB 按大小降序取前 5

A/B/C 每池用 random.Random(seed).sample(pool_sorted, quota)；pool 先按路径排序保证确定性。
D 池按规格书为"按大小降序取前 5"（确定性）。
"""
import json
import os
import random

DEFAULT_CORPUS = r"G:\投标docx库"

A_QUOTA = 25
B_QUOTA = 10
C_QUOTA = 10
D_QUOTA = 5

MB = 1024 * 1024
BIG_BYTES = 50 * MB  # >50MB 视为大件

_MANIFEST_BASENAME = "manifest.json"


def _walk_docx(base):
    """返回 base 子树下全部 .docx 的 (abspath, size_bytes) 列表。"""
    out = []
    if not os.path.isdir(base):
        return out
    for dp, _dn, fn in os.walk(base):
        for f in fn:
            if f.lower().endswith(".docx"):
                p = os.path.join(dp, f)
                try:
                    size = os.path.getsize(p)
                except OSError:
                    continue
                out.append((p, size))
    return out


def pools(corpus_root=DEFAULT_CORPUS):
    """返回 (A, B, C, D) 四个池，每池为按路径排序的 [(path,size_bytes), ...]。

    A/B/C 已排除 >50MB；D 为全库 >50MB（按大小降序、路径升序并列）。
    """
    bid = os.path.join(corpus_root, "投标文件")
    perf_dir = os.path.join(bid, "业绩")

    a = [(p, s) for (p, s) in _walk_docx(bid)
         if "业绩" not in p and s <= BIG_BYTES]
    b = [(p, s) for (p, s) in _walk_docx(perf_dir) if s <= BIG_BYTES]

    c = []
    for base in (os.path.join(corpus_root, "Desktop"),
                 os.path.join(corpus_root, "审核标书")):
        c += [(p, s) for (p, s) in _walk_docx(base) if s <= BIG_BYTES]

    d = [(p, s) for (p, s) in _walk_docx(corpus_root) if s > BIG_BYTES]

    a.sort(key=lambda t: t[0])
    b.sort(key=lambda t: t[0])
    c.sort(key=lambda t: t[0])
    d.sort(key=lambda t: (-t[1], t[0]))
    return a, b, c, d


def build_manifest(corpus_root=DEFAULT_CORPUS, seed=20261012):
    """构建抽样清单，返回 list[dict]，条目 {"path","size_mb","tier"}。

    规格书 §1: `build_manifest(corpus_root, seed=20261012) -> list[dict]`
    """
    a, b, c, d = pools(corpus_root)
    rng = random.Random(seed)

    manifest = []
    for tier, pool, quota in (("A", a, A_QUOTA),
                              ("B", b, B_QUOTA),
                              ("C", c, C_QUOTA)):
        n = min(quota, len(pool))
        picked = rng.sample(pool, n) if n else []
        for path, size in picked:
            manifest.append(_entry(path, size, tier))

    # D 组：按大小降序取前 D_QUOTA（确定性，不用 rng）
    for path, size in d[:D_QUOTA]:
        manifest.append(_entry(path, size, "D"))

    return manifest


def _entry(path, size, tier):
    return {"path": path, "size_mb": round(size / MB, 3), "tier": tier}


def write_manifest(manifest, out_path):
    """把 manifest 写入 JSON 文件（UTF-8, ensure_ascii=False）。"""
    d = os.path.dirname(os.path.abspath(out_path))
    if d and not os.path.isdir(d):
        os.makedirs(d, exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as fh:
        json.dump(manifest, fh, ensure_ascii=False, indent=2)
    return out_path
