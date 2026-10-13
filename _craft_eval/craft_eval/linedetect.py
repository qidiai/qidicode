# -*- coding: utf-8 -*-
"""linedetect.py — 表格线判据的公共算法（T2 规格书 Part A）。

提供四组纯函数（无 COM、无 CLI）：
  1. 页对齐      : pdf_page_texts / ngrams / jaccard / align_pages
  2. 页面渲染    : render_page_png (pymupdf, 192dpi 默认)
  3. 线段检测    : detect_segments (PIL, 灰度阈值 + run 长度, 写死参数)
  4. 覆盖率      : match_segments (同向 / 中心距 / 长度比)

参数全部为规格书写死的常量（见下方 block）。
"""
import re

# ---- 规格书 Part A 写死参数 ------------------------------------------------ #
DARK_THRESHOLD = 128          # 灰度 <128 为暗
MIN_H_FRAC = 0.08             # 横向线 = 行内连续暗 run >= 页宽 8%
MIN_V_FRAC = 0.08             # 纵向线 = 列内连续暗 run >= 页高 8%
CENTER_TOL_FRAC = 0.025       # 中心距 <= 2.5% 页尺寸
LENGTH_RATIO_MIN = 0.6        # 长度比 >= 0.6
NGRAM_N = 4                   # 4-gram
ALIGN_THRESHOLD = 0.35        # Jaccard >= 0.35 算对齐成功
DEFAULT_DPI = 192             # 渲染对照 192dpi

_WS_RE = re.compile(r"\s")


# ---- 通用 ------------------------------------------------------------------ #
def norm_ws(text):
    """去空白（与 metrics.norm 同规则，此处独立以免跨模块耦合）。"""
    return _WS_RE.sub("", text or "")


def ngrams(text, n=NGRAM_N):
    """字符 n-gram 集合。空串 → 空集；短于 n → {text}。"""
    if not text:
        return set()
    if len(text) < n:
        return {text}
    return {text[i:i + n] for i in range(len(text) - n + 1)}


def jaccard(a, b):
    """集合 Jaccard。双空 → 1.0，单空 → 0.0。"""
    if not a and not b:
        return 1.0
    if not a or not b:
        return 0.0
    inter = len(a & b)
    union = len(a | b)
    return inter / union if union else 0.0


# ---- 1. PDF 文本 / 页对齐 -------------------------------------------------- #
def pdf_page_texts(pdf_path):
    """返回每页 get_text() 去空白后的 list[str]。"""
    import fitz
    doc = fitz.open(pdf_path)
    try:
        return [norm_ws(p.get_text()) for p in doc]
    finally:
        doc.close()


def align_pages(wc_texts, wps_texts, threshold=ALIGN_THRESHOLD):
    """每个 wc 页取 4-gram Jaccard 最优 wps 页。

    返回 list（与 wc_texts 等长），元素 {"wps": idx|None, "score": float}。
    score >= threshold 且存在候选 → wps 记其索引，否则 None（unaligned）。
    """
    wps_grams = [ngrams(t) for t in wps_texts]
    out = []
    for wt in wc_texts:
        wg = ngrams(wt)
        best_i = None
        best = 0.0
        for i, g in enumerate(wps_grams):
            s = jaccard(wg, g)
            if s > best:
                best = s
                best_i = i
        if best >= threshold and best_i is not None:
            out.append({"wps": best_i, "score": round(best, 4)})
        else:
            out.append({"wps": None, "score": round(best, 4)})
    return out


# ---- 2. 页面渲染 ----------------------------------------------------------- #
def render_page_png(pdf_path, page_index, out_png, dpi=DEFAULT_DPI):
    """把 pdf 的第 page_index 页(0基)渲染为 PNG，返回页面尺寸(px)。

    返回 {"w_px","h_px","w_pt","h_pt","dpi"}。
    """
    import fitz
    doc = fitz.open(pdf_path)
    try:
        page = doc[page_index]
        zoom = dpi / 72.0
        pix = page.get_pixmap(matrix=fitz.Matrix(zoom, zoom))
        # 确保父目录存在由调用方负责
        pix.save(out_png)
        rect = page.rect
        return {
            "w_px": pix.width, "h_px": pix.height,
            "w_pt": round(rect.width, 2), "h_pt": round(rect.height, 2),
            "dpi": dpi,
        }
    finally:
        doc.close()


# ---- 3. 线段检测 ----------------------------------------------------------- #
def _scan_runs(data, width, height, min_len):
    """在 width×height 的逐像素字节里扫描每行的连续暗 run。

    暗像素字节为 0x00（point() 已二值化）。返回 [(row, start, end), ...]。
    """
    pat = re.compile(rb"\x00{%d,}" % int(min_len))
    runs = []
    for y in range(height):
        row = data[y * width:(y + 1) * width]
        for m in pat.finditer(row):
            runs.append((y, m.start(), m.end() - 1))
    return runs


def _merge_runs(runs):
    """把相邻行/列上水平重叠的 run 合并为一条线段。

    输出 [{"c0","c1","len","center0","center1","span"}, ...]
    其中 0=沿 run 方向的坐标，1=跨行方向的坐标。
    """
    from collections import defaultdict
    runs.sort()
    clusters = []
    by_last = defaultdict(list)
    for (y, s, e) in runs:
        chosen = None
        for ci in by_last.get(y - 1, ()):
            cs, ce = clusters[ci]["rep"]
            ov = min(ce, e) - max(cs, s) + 1
            shorter = min(ce - cs + 1, e - s + 1)
            if ov >= 0.5 * shorter:
                chosen = ci
                break
        if chosen is None:
            clusters.append({"y0": y, "y1": y, "runs": [(s, e)], "rep": (s, e)})
            by_last[y].append(len(clusters) - 1)
        else:
            c = clusters[chosen]
            c["y1"] = y
            c["runs"].append((s, e))
            c["rep"] = (s, e)
            by_last[y].append(chosen)

    segs = []
    for c in clusters:
        ss = sorted(s for s, _ in c["runs"])
        ee = sorted(e for _, e in c["runs"])
        s = ss[len(ss) // 2]
        e = ee[len(ee) // 2]
        segs.append({
            "c0": s, "c1": e, "len": e - s + 1,
            "center0": (s + e) / 2.0,
            "center1": (c["y0"] + c["y1"]) / 2.0,
            "span": c["y1"] - c["y0"] + 1,
        })
    return segs


def detect_segments(img):
    """对 PIL.Image 做横/纵线段检测。

    返回 {"h":[seg...], "v":[seg...]}，坐标单位像素：
      h 段: {"x0","x1","len","cx","cy"}   (cx=中心x, cy=中心y)
      v 段: {"y0","y1","len","cx","cy"}
    """
    from PIL import Image
    g = img.convert("L").point(lambda p: 0 if p < DARK_THRESHOLD else 255)
    w, h = g.size
    data = g.tobytes()
    min_h = max(1, int(w * MIN_H_FRAC))
    min_v = max(1, int(h * MIN_V_FRAC))

    h_raw = _scan_runs(data, w, h, min_h)
    h_segs = []
    for s in _merge_runs(h_raw):
        h_segs.append({
            "x0": s["c0"], "x1": s["c1"], "len": s["len"],
            "cx": s["center0"], "cy": s["center1"], "thick": s["span"],
        })

    gt = g.transpose(Image.TRANSPOSE)
    v_data = gt.tobytes()
    # 转置后：row = 原列 x，run 的 start/end = 原行 y
    v_raw = _scan_runs(v_data, h, w, min_v)
    v_segs = []
    for s in _merge_runs(v_raw):
        v_segs.append({
            "y0": s["c0"], "y1": s["c1"], "len": s["len"],
            "cx": s["center1"], "cy": s["center0"], "thick": s["span"],
        })
    return {"h": h_segs, "v": v_segs}


def detect_segments_file(png_path):
    from PIL import Image
    with Image.open(png_path) as im:
        im.load()
        return detect_segments(im)


def segment_count(segs):
    return len(segs["h"]) + len(segs["v"])


# ---- 4. 覆盖率 ------------------------------------------------------------- #
def match_segments(wps_segs, wc_segs, page_w, page_h):
    """WPS 每条线段在 wc 侧找匹配（同向；中心距<=2.5%页尺寸；长度比>=0.6）。

    返回 {"total": n, "matched": m, "coverage": m/n|None, "misses": [wps_seg...]}。
    page_w/page_h 为像素尺寸（双侧同 dpi 渲染）。
    """
    tol_x = CENTER_TOL_FRAC * page_w
    tol_y = CENTER_TOL_FRAC * page_h
    total = len(wps_segs)
    matched = 0
    used = set()          # 已匹配的 wc 段索引（同向列表内）
    misses = []
    for wse in wps_segs:
        hit = False
        for i, cse in enumerate(wc_segs):
            if i in used:
                continue
            if abs(wse["cx"] - cse["cx"]) <= tol_x and \
               abs(wse["cy"] - cse["cy"]) <= tol_y:
                ratio = min(wse["len"], cse["len"]) / max(wse["len"], cse["len"])
                if ratio >= LENGTH_RATIO_MIN:
                    hit = True
                    used.add(i)
                    break
        if hit:
            matched += 1
        else:
            misses.append(wse)
    coverage = (matched / total) if total else None
    return {"total": total, "matched": matched,
            "coverage": coverage, "misses": misses}


def page_coverage(wps_segs_all, wc_segs_all, page_w, page_h):
    """对 h+v 全量线段做覆盖率（分母 = WPS 全部线段）。"""
    res = {}
    tot = 0
    mt = 0
    misses = []
    for orient in ("h", "v"):
        r = match_segments(wps_segs_all[orient], wc_segs_all[orient],
                           page_w, page_h)
        tot += r["total"]
        mt += r["matched"]
        misses += [dict(o=orient, **m) for m in r["misses"]]
    cov = (mt / tot) if tot else None
    return {"total": tot, "matched": mt, "coverage": cov, "misses": misses}
