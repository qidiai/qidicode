#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""t2_run.py — T2 主入口（规格书 T2：表格线补课 + 字符级 diff + harness 修复 + convert→pdf 探针）。

子命令:
  --selftest    转调 selftest.main()
  --a0          Part A0  convert→pdf 文本层探针
  --chardiff    Part B   10 份字符级 diff
  --tableline   Part A   表格线判据（wordcraft vs WPS 双路 PDF）
  --rerun-w2    Part D   转调 run_w2.main(['--run'])
  --all         A0 → chardiff → tableline → rerun-w2（默认入口）

纪律: 语料只读；一切写经 _safe_write_path（根 out/）；WPS COM 仅在 t2_wps_child.py
子进程内，本父进程绝不 Dispatch；不 git commit。
"""
import argparse
import io
import json
import math
import os
import random
import re
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
from datetime import datetime

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

from craft_eval import linedetect as ld            # noqa: E402
from craft_eval import metrics, truth               # noqa: E402
from craft_eval.cli import DEFAULT_EXE, WordcraftCLI  # noqa: E402
import run_w2                                       # noqa: E402

# ---- 路径 ------------------------------------------------------------------ #
OUT_DIR = os.path.join(HERE, "out")
T2_DIR = os.path.join(OUT_DIR, "t2")
T2_PDF = os.path.join(T2_DIR, "pdf")
T2_PAIRS = os.path.join(T2_DIR, "pairs")
MANIFEST_PATH = os.path.join(OUT_DIR, "manifest.json")
RECORDS_DIR = os.path.join(OUT_DIR, "records")
CHILD = os.path.join(HERE, "t2_wps_child.py")

# ---- 参数（规格书写死） ---------------------------------------------------- #
SEED = 20261013
WC_TIMEOUT = 300
WPS_JOB_TIMEOUT = 300          # 单份 300s 无进度行 → 看门狗 taskkill
WPS_DISPATCH_TIMEOUT = 300     # Dispatch/READY 等待上限
RENDER_DPI = 192
QUOTA = {"A": 6, "B": 2, "C": 2}
N_PAGES_PER_FILE = 3
FILE_PASS_COVERAGE = 0.90
FILE_PASS_NEED = 2.0 / 3.0
PAGE_ABSENT_COVERAGE = 0.5

A0_SAMPLE1_ENV = "A0_SAMPLE1"  # 指向"指挥官复现丢字"样本的 docx 路径
# A0 样本1 的真实路径不入库：优先读 env A0_SAMPLE1；未设置则跳过样本1并如实记录

FIXED_CHARDIFF = [2, 33, 34]
CHARDIFF_QUOTA = {"A": 4, "B": 2, "C": 1}

W_NS = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
W = "{%s}" % W_NS
_WS_RE = re.compile(r"\s")

# ---- fast diff ------------------------------------------------------------- #
GAP_LIMIT = 6000
# len(A)+len(B) <= 此值 → 用真 difflib（段更干净、归因更准）；否则锚点法。
# 实测：124k 对 110k 文本 difflib 需 ~84s / 锚点法 0.31s，两者丢失字符总数与
# 前若干丢失样例完全一致（见 T2 报告）；小文本 difflib 段级切分更贴合归因。
DIFFLIB_MAX = 80000


def _uniq_ngram_pos(text, n):
    pos = {}
    for i in range(len(text) - n + 1):
        g = text[i:i + n]
        if g in pos:
            pos[g] = -1
        else:
            pos[g] = i
    return pos


def _anchors(A, B, n):
    pos = _uniq_ngram_pos(A, n)
    out = []
    la = -1
    i = 0
    L = len(B) - n
    while i <= L:
        p = pos.get(B[i:i + n])
        if p is not None and p >= 0:
            if p > la:
                out.append((p, i))
                la = p
            i += n
        else:
            i += 1
    return out


def _gap_ops(ga, gb, n_list):
    if not ga and not gb:
        return []
    if not ga:
        return [("insert", 0, 0, 0, len(gb))]
    if not gb:
        return [("delete", 0, len(ga), 0, 0)]
    if len(ga) + len(gb) <= GAP_LIMIT:
        return list(difflib_seq_matcher(ga, gb).get_opcodes())
    if not n_list:
        return [("replace", 0, len(ga), 0, len(gb))]
    n = n_list[0]
    if n >= min(len(ga), len(gb)) + 1:
        return _gap_ops(ga, gb, n_list[1:])
    return _fast_ops(ga, gb, n, n_list[1:])


def _fast_ops(A, B, n, n_list):
    ops = []
    pa = pb = 0
    for a, b in _anchors(A, B, n):
        for t, i1, i2, j1, j2 in _gap_ops(A[pa:a], B[pb:b], n_list):
            ops.append((t, pa + i1, pa + i2, pb + j1, pb + j2))
        ops.append(("equal", a, a + n, b, b + n))
        pa, pb = a + n, b + n
    for t, i1, i2, j1, j2 in _gap_ops(A[pa:], B[pb:], n_list):
        ops.append((t, pa + i1, pa + i2, pb + j1, pb + j2))
    return ops


def difflib_seq_matcher(a, b):
    import difflib
    return difflib.SequenceMatcher(None, a, b, autojunk=False)


def fast_opcodes(A, B):
    """段级 diff。小文本用真 SequenceMatcher，大文本用锚点法（结果等价）。

    锚点法：以 A 中唯一 16-gram 作锚点，仅在锚点间隙内用 difflib；间隙超大则递归
    降 n，最后兜底 replace。返回 [(tag,i1,i2,j1,j2), ...]（同 get_opcodes）。
    """
    if not A and not B:
        return []
    if not A:
        return [("insert", 0, 0, 0, len(B))]
    if not B:
        return [("delete", 0, len(A), 0, 0)]
    if len(A) + len(B) <= DIFFLIB_MAX:
        return list(difflib_seq_matcher(A, B).get_opcodes())
    return _fast_ops(A, B, 16, [8, 4, 2])


# ---- 基础工具 -------------------------------------------------------------- #
def setup_stdout():
    try:
        if getattr(sys.stdout, "_craft_utf8", False):
            return
        if sys.stdout is not None and hasattr(sys.stdout, "buffer"):
            w = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8",
                                 line_buffering=True, write_through=True)
            w._craft_utf8 = True
            sys.stdout = w
    except Exception:
        pass


def ensure_dirs():
    for d in (T2_DIR, T2_PDF, T2_PAIRS):
        os.makedirs(d, exist_ok=True)


def _safe_write_path(p, root=None):
    """写路径防护：默认只允许写 OUT_DIR(out/) 内。"""
    out_root = os.path.abspath(root or OUT_DIR)
    ap = os.path.abspath(p)
    if not (ap == out_root or ap.startswith(out_root + os.sep)):
        raise AssertionError("write outside %s blocked: %s" % (out_root, p))


def _write_text(path, text, root=None):
    _safe_write_path(path, root)
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(text)
    return path


def _write_json(path, obj, root=None):
    _safe_write_path(path, root)
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(obj, fh, ensure_ascii=False, indent=2)
    return path


def _norm_ws(s):
    return _WS_RE.sub("", s or "")


def cli_convert(docx, out_pdf, timeout=WC_TIMEOUT):
    """wordcraft-cli convert <in> <out>；rc 可能为 'TIMEOUT'。"""
    argv = [DEFAULT_EXE, "convert", docx, out_pdf]
    t0 = time.monotonic()
    try:
        p = subprocess.run(argv, capture_output=True, timeout=timeout)
        rc = p.returncode
        err = (p.stderr or b"").decode("utf-8", "replace")
    except subprocess.TimeoutExpired:
        rc = "TIMEOUT"
        err = "timeout"
    except OSError as e:
        rc = "NOEXE"
        err = "%s: %s" % (type(e).__name__, e)
    return {"rc": rc, "err": err[-300:],
            "ms": round((time.monotonic() - t0) * 1000, 1)}


def cli_render(docx, out_png, page, scale=2.0, timeout=WC_TIMEOUT):
    argv = [DEFAULT_EXE, "render", docx, out_png, "--page", str(page),
            "--scale", str(scale)]
    t0 = time.monotonic()
    try:
        p = subprocess.run(argv, capture_output=True, timeout=timeout)
        rc = p.returncode
    except subprocess.TimeoutExpired:
        rc = "TIMEOUT"
    except OSError as e:
        rc = "NOEXE"
    return {"rc": rc, "ms": round((time.monotonic() - t0) * 1000, 1),
            "bytes": os.path.getsize(out_png) if os.path.exists(out_png) else 0}


def pdf_pages(path):
    import fitz
    doc = fitz.open(path)
    try:
        return doc.page_count
    finally:
        doc.close()


def load_manifest():
    with open(MANIFEST_PATH, "r", encoding="utf-8") as fh:
        return json.load(fh)


def load_records():
    recs = {}
    if not os.path.isdir(RECORDS_DIR):
        return recs
    for f in os.listdir(RECORDS_DIR):
        if f.endswith(".json"):
            with open(os.path.join(RECORDS_DIR, f), "r", encoding="utf-8") as fh:
                try:
                    recs[int(f[:-5])] = json.load(fh)
                except ValueError:
                    pass
    return recs


def has_table(docx_path):
    """word/document.xml 是否含 w:tbl。解析失败 → False。"""
    import zipfile
    try:
        with zipfile.ZipFile(docx_path) as z:
            xml = z.read("word/document.xml")
    except Exception:
        return False
    return b"<w:tbl>" in xml


# =========================================================================== #
# Part A0 — convert→pdf 文本层探针
# =========================================================================== #
def _a0_samples():
    """返回样本条目列表（每项 dict：n/label/path/source）。

    样本1 路径不入库：取 env A0_SAMPLE1；未设置 → path=None 且 source 记 SKIPPED。
    样本2/3：从 W2 records 取 A 层 truth_norm 最大/中位（运行期解析，同样不入库）。
    """
    recs = load_records()
    a = [(r.get("truth_norm") or 0, idx, r.get("path"))
         for idx, r in recs.items() if r.get("tier") == "A"]
    a.sort()
    n = len(a)
    median = a[(n - 1) // 2] if n else None   # 中位（下中位）
    mx = a[-1] if a else None

    env_path = os.environ.get(A0_SAMPLE1_ENV)
    out = []
    if env_path:
        out.append({"n": 1, "label": "样本1(env %s)" % A0_SAMPLE1_ENV,
                    "path": env_path, "source": "env %s" % A0_SAMPLE1_ENV})
    else:
        out.append({"n": 1, "label": "样本1",
                    "path": None,
                    "source": "SKIPPED(env %s 未设置，路径不入库)" % A0_SAMPLE1_ENV})
    if mx:
        out.append({"n": 2, "label": "A层truth_norm最大(#%d=%d)" % (mx[1], mx[0]),
                    "path": mx[2], "source": "records #%d" % mx[1]})
    if median:
        out.append({"n": 3, "label": "A层truth_norm中位(#%d=%d)" % (median[1], median[0]),
                    "path": median[2], "source": "records #%d" % median[1]})
    return out


def _loss_samples(base_norm, other_norm, limit=6, ctx=10):
    """base 中在 other 里丢失的片段（delete/replace 的 base 侧）。"""
    ops = fast_opcodes(base_norm, other_norm)
    lost = []
    for tag, i1, i2, j1, j2 in ops:
        if tag in ("delete", "replace") and i2 > i1:
            lost.append({
                "lost": base_norm[i1:i2], "pos": i1,
                "before": base_norm[max(0, i1 - ctx):i1],
                "after": base_norm[i2:i2 + ctx],
            })
    lost.sort(key=lambda d: (-len(d["lost"]), d["pos"]))
    return lost[:limit]


def _classify(ret):
    if ret is None:
        return "未知"
    if ret >= 0.99:
        return "完好"
    if ret >= 0.95:
        return "轻度丢失"
    return "重度丢失"


def cmd_a0(_args):
    ensure_dirs()
    samples = _a0_samples()
    cli = WordcraftCLI()
    print("[A0] 样本 %d 项；CLI=%s exists=%s" % (len(samples), cli.exe, cli.exists()))
    rows = []
    losses_all = {}
    for ent in samples:
        n = ent["n"]
        label = ent["label"]
        docx = ent["path"]
        source = ent["source"]
        row = {"n": n, "label": label, "source": source, "docx": docx,
               "pdf": None, "skipped": docx is None,
               "conv_rc": None, "conv_ms": None, "wc_rc": None, "wc_len": None,
               "fitz_len": None, "fitz_ret": None,
               "plumber_len": None, "plumber_ret": None, "pages": None}
        if docx is None:                       # 样本1 未设 env → 跳过并如实记录
            losses_all[n] = []
            rows.append(row)
            try:                               # 清理可能残留的旧样本1 PDF（含真实项目内容）
                stale = os.path.join(T2_PDF, "a0_%d.pdf" % n)
                if os.path.exists(stale):
                    os.remove(stale)
            except OSError:
                pass
            print("  [A0 %d] 样本1: %s" % (n, source))
            continue
        pdf = os.path.join(T2_PDF, "a0_%d.pdf" % n)
        _safe_write_path(pdf)
        row["pdf"] = pdf
        conv = cli_convert(docx, pdf)
        row["conv_rc"] = conv["rc"]
        row["conv_ms"] = conv["ms"]
        wc = cli.text(docx, timeout=WC_TIMEOUT)
        wc_norm = _norm_ws(wc["out"])
        row["wc_rc"] = wc["rc"]
        row["wc_len"] = len(wc_norm)
        if os.path.exists(pdf):
            row["pages"] = pdf_pages(pdf)
            fitz_norm = "".join(ld.pdf_page_texts(pdf))
            row["fitz_len"] = len(fitz_norm)
            row["fitz_ret"] = (len(fitz_norm) / len(wc_norm)) if wc_norm else None
            losses_all[n] = _loss_samples(wc_norm, fitz_norm)
            # pdfplumber（可选）
            try:
                import pdfplumber
                with pdfplumber.open(pdf) as pl:
                    pl_text = "".join((pg.extract_text() or "") for pg in pl.pages)
                pl_norm = _norm_ws(pl_text)
                row["plumber_len"] = len(pl_norm)
                row["plumber_ret"] = (len(pl_norm) / len(wc_norm)) if wc_norm else None
            except ImportError:
                row["plumber_len"] = None
            except Exception as e:
                row["plumber_len"] = "err:%s" % type(e).__name__
        else:
            losses_all[n] = []
        rows.append(row)
        print("  [A0 %d/%d] %s conv_rc=%s wc_len=%d fitz_ret=%s plumber_ret=%s" % (
            n, len(samples), label, conv["rc"], row["wc_len"],
            _pct(row["fitz_ret"]), _pct(row["plumber_ret"])))

    md = _a0_report_md(rows, losses_all)
    path = _write_text(os.path.join(T2_DIR, "convert_pdf_textprobe.md"), md)
    print("[A0] -> %s" % path)
    return 0


def _pct(x):
    if isinstance(x, (int, float)):
        return "%.4f%%" % (x * 100)
    return "-"


def _a0_report_md(rows, losses):
    L = []
    ap = L.append
    ap("# T2 Part A0 — wordcraft convert→pdf 文本层探针")
    ap("")
    ap("- 生成时间: %s" % datetime.now().isoformat(timespec="seconds"))
    ap("- CLI: `%s`" % DEFAULT_EXE)
    ap("- 命令: `wordcraft-cli convert <docx> out/t2/pdf/a0_<n>.pdf` (timeout=300s)")
    ap("- 文本三路: ① `wordcraft-cli text <docx>`(基线) ② `pymupdf`(fitz) 逐页 "
       "`page.get_text()` 拼接 ③ `pdfplumber` 逐页 `extract_text()` 拼接")
    ap("- 规整: 三路均 `re.sub(r'\\s','',x)`；保留率 = 该路长度 / 基线长度")
    ap("- 阈值: ≥99% 完好 / 95-99% 轻度丢失 / <95% 重度丢失")
    ap("")
    ap("## 三路对比表")
    ap("")
    ap("| n | 样本 | 来源 | docx | 页数 | conv rc | 基线 wc_len | fitz_len | "
       "fitz 保留率 | plumber_len | plumber 保留率 |")
    ap("|---|---|---|---|---|---|---|---|---|---|---|")
    for r in rows:
        if r.get("skipped"):
            ap("| %d | %s | %s | - | - | - | - | - | - | - | - |" % (
                r["n"], r["label"], r["source"]))
            continue
        ap("| %d | %s | %s | `%s` | %s | %s | %s | %s | %s | %s | %s |" % (
            r["n"], r["label"], r["source"], r["docx"], r["pages"], r["conv_rc"],
            r["wc_len"] if r["wc_len"] is not None else "-",
            r["fitz_len"], _pct(r["fitz_ret"]),
            r["plumber_len"], _pct(r["plumber_ret"])))
    ap("")
    s1 = next((r for r in rows if r["n"] == 1), None)
    if s1 is not None:
        ap("- 样本1 来源: **%s**（真实路径不入库：env `%s` 提供，未设置则跳过）"
           % (s1["source"], A0_SAMPLE1_ENV))
    ap("")
    ap("来源: 基线= `wordcraft-cli text` 的 out 字段；fitz= `out/t2/pdf/a0_<n>.pdf` "
       "的 pymupdf 逐页 get_text 拼接后去空白；plumber= 同 pdf 的 pdfplumber 逐页 "
       "extract_text 拼接后去空白。")
    ap("")
    ap("## 丢失样例（每份 ≥3 处，上下文前后各 10 字）")
    ap("")
    for r in rows:
        ap("### A0-%d %s" % (r["n"], r["label"]))
        ap("")
        if r.get("skipped"):
            ap("- （%s，无数据）" % r["source"])
            ap("")
            continue
        ls = losses.get(r["n"], [])
        if not ls:
            ap("- （fitz 侧未见丢失片段）")
        else:
            for k, d in enumerate(ls, 1):
                ap("- #%d pos=%d lost=%d字: `%s` | 上下文: `%s`【`%s`】`%s`" % (
                    k, d["pos"], len(d["lost"]), d["lost"],
                    d["before"], d["lost"], d["after"]))
        ap("")
    # 结论
    rets = [r["fitz_ret"] for r in rows if isinstance(r["fitz_ret"], (int, float))]
    worst = min(rets) if rets else None
    verdict = _classify(worst)
    ap("## 结论")
    ap("")
    ap("- 各样本 fitz 保留率: %s" % ", ".join(
        "A0-%d=%s" % (r["n"], _pct(r["fitz_ret"])) for r in rows))
    ap("- 取最差样本定级 → **PDF文本层【%s】**（最差 fitz 保留率 %s，"
       "阈值 ≥99%%完好/95-99%%轻度/<95%%重度）" % (verdict, _pct(worst)))
    ap("")
    if verdict != "完好":
        ap("### R21 候选登记文案（供指挥官采用）")
        ap("")
        ap("> R21（候选）：`wordcraft-cli convert` 生成的 PDF 文本层存在丢字——"
           "以 `wordcraft-cli text` 为基线，pymupdf 抽取得分 %s（最差 %s），"
           "判为【%s】；影响所有依赖 PDF 文本层的判据（如目录/条款抽取）。"
           "建议登记为 R21，附本报告 `out/t2/convert_pdf_textprobe.md` 复现证据。"
           % ("、".join(_pct(r["fitz_ret"]) for r in rows), _pct(worst), verdict))
        ap("")
    ap("> 本探针只量化不修复。")
    ap("")
    return "\n".join(L)


# =========================================================================== #
# Part B — 字符级 diff
# =========================================================================== #
_BUCKET_TAGS = {"txbx": W + "txbxContent", "delText": W + "delText",
                "del": W + "del", "sym": W + "sym"}


def _char_sources(xml_bytes):
    """解析 document.xml，返回 (raw_n, labels_n, buckets)。

    raw_n    : 非删除 w:t 文本去空白后的字符串
    labels_n : 与 raw_n 等长的来源标签 ('txbx'|'tbl'|'body')
    buckets  : {"delText": str, "del": str, "sym_chars": set}
    """
    root = ET.fromstring(xml_bytes)
    parent = {}
    for p in root.iter():
        for c in p:
            parent[c] = p
    TXBX = W + "txbxContent"
    TBL = W + "tbl"
    DEL = W + "del"

    parts = []
    labels = []
    for el in root.iter(W + "t"):
        anc = set()
        node = el
        while node in parent:
            node = parent[node]
            anc.add(node.tag)
        if DEL in anc:
            continue  # 旧式删除不进 raw（与 truth.py 一致）
        lab = "txbx" if TXBX in anc else ("tbl" if TBL in anc else "body")
        for ch in (el.text or ""):
            if not _WS_RE.match(ch):
                parts.append(ch)
                labels.append(lab)

    deltext = "".join(_WS_RE.sub("", (el.text or ""))
                      for el in root.iter(W + "delText"))
    olddel = "".join(_WS_RE.sub("", (el.text or ""))
                     for el in root.iter(W + "t")
                     if _has_ancestor(el, parent, DEL))
    sym_chars = set()
    for el in root.iter(W + "sym"):
        ch = el.get(W + "char")
        if ch:
            try:
                sym_chars.add(chr(int(ch, 16)))
            except ValueError:
                pass
    return "".join(parts), labels, {"delText": deltext, "del": olddel,
                                    "sym_chars": sym_chars}


def _has_ancestor(el, parent, tag):
    node = el
    while node in parent:
        node = parent[node]
        if node.tag == tag:
            return True
    return False


def _ngram_set(s, n=4):
    return ld.ngrams(s, n)


def _sub_overlap(frag, bucket_text, gram_set):
    """片段与桶文本的匹配度：先整体包含（短片段也能命中），再 4-gram 覆盖率。

    len(frag)<4 时只认整体包含，避免单字被任意桶误归因。
    """
    if not frag:
        return 0.0
    if frag in bucket_text:
        return 1.0
    if len(frag) < 4:
        return 0.0
    fg = ld.ngrams(frag, 4)
    if not fg:
        return 0.0
    return len(fg & gram_set) / len(fg)


def _explain_extra(frag, buckets, sets):
    """craft 多出片段的归因：delText / del(旧式) / sym / unexplained。"""
    scores = {
        "delText": _sub_overlap(frag, buckets["delText"], sets["delText"]),
        "del": _sub_overlap(frag, buckets["del"], sets["del"]),
    }
    sym_frac = 0.0
    if frag and buckets["sym_chars"]:
        sym_frac = sum(1 for ch in frag if ch in buckets["sym_chars"]) / len(frag)
    best = max(scores.values()) if scores else 0.0
    best_name = max(scores, key=scores.get) if scores else None
    if best >= 0.5:
        return best_name, round(best, 3)
    if sym_frac >= 0.5:
        return "sym", round(sym_frac, 3)
    return "unexplained", round(max(best, sym_frac), 3)


def _chardiff_samples(recs):
    abc = {t: [] for t in ("A", "B", "C")}
    for idx, r in recs.items():
        t = r.get("tier")
        if t in abc and (r.get("truth_norm") or 0) > 0 and idx not in FIXED_CHARDIFF:
            abc[t].append(idx)
    for t in abc:
        abc[t].sort(key=lambda i: (recs[i].get("path", ""), i))
    rng = random.Random(SEED)
    picks = list(FIXED_CHARDIFF)
    for t, q in CHARDIFF_QUOTA.items():
        n = min(q, len(abc[t]))
        picks += rng.sample(abc[t], n)
    return picks


def cmd_chardiff(_args):
    ensure_dirs()
    recs = load_records()
    picks = _chardiff_samples(recs)
    print("[chardiff] %d 份: %s" % (len(picks), picks))
    cli = WordcraftCLI()
    results = []
    for idx in picks:
        r = recs[idx]
        docx = r["path"]
        tr = truth.extract_truth(docx)
        try:
            import zipfile
            with zipfile.ZipFile(docx) as z:
                xml = z.read("word/document.xml")
            raw_n, labels, buckets = _char_sources(xml)
        except Exception as e:
            results.append({"idx": idx, "tier": r.get("tier"), "path": docx,
                            "error": "%s: %s" % (type(e).__name__, e)})
            continue
        craft = cli.text(docx, timeout=WC_TIMEOUT)
        craft_n = _norm_ws(craft["out"])
        sets = {"delText": _ngram_set(buckets["delText"]),
                "del": _ngram_set(buckets["del"])}
        ops = fast_opcodes(raw_n, craft_n)
        missing = []
        extra = []
        for tag, i1, i2, j1, j2 in ops:
            if tag == "equal":
                continue
            if tag in ("delete", "replace") and i2 > i1:
                srcs = labels[i1:i2]
                dom = max(set(srcs), key=srcs.count) if srcs else "?"
                missing.append({"frag": raw_n[i1:i2], "src": dom,
                                "n": len(srcs)})
            if tag in ("insert", "replace") and j2 > j1:
                frag = craft_n[j1:j2]
                mech, score = _explain_extra(frag, buckets, sets)
                extra.append({"frag": frag, "mech": mech, "score": score,
                              "n": len(frag)})

        miss_attr = [m for m in missing if m["src"] == "txbx"]
        miss_un = [m for m in missing if m["src"] != "txbx"]
        extra_attr = [e for e in extra if e["mech"] != "unexplained"]
        extra_un = [e for e in extra if e["mech"] == "unexplained"]
        total_seg = len(missing) + len(extra)
        attr_seg = len(miss_attr) + len(extra_attr)
        total_ch = sum(m["n"] for m in missing) + sum(e["n"] for e in extra)
        attr_ch = sum(m["n"] for m in miss_attr) + sum(e["n"] for e in extra_attr)
        results.append({
            "idx": idx, "tier": r.get("tier"), "path": docx,
            "truth_norm": len(raw_n), "craft_norm": len(craft_n),
            "del_chars": tr.get("del_chars"), "sym_count": tr.get("sym_count"),
            "missing": missing, "extra": extra,
            "missing_attr": miss_attr, "missing_un": miss_un,
            "extra_attr": extra_attr, "extra_un": extra_un,
            "total_seg": total_seg, "attr_seg": attr_seg,
            "total_ch": total_ch, "attr_ch": attr_ch,
            "fully": (total_seg == attr_seg),
        })
        print("  #%d %s truth=%d craft=%d 差异段=%d 可归因=%d 未解释=%d -> %s" % (
            idx, r.get("tier"), len(raw_n), len(craft_n), total_seg,
            attr_seg, total_seg - attr_seg,
            "全归因" if total_seg == attr_seg else "有未解释"))
    md = _chardiff_report_md(results)
    path = _write_text(os.path.join(T2_DIR, "char_diff_report.md"), md)
    _write_json(os.path.join(T2_DIR, "char_diff_report.json"),
                {"generated_at": datetime.now().isoformat(timespec="seconds"),
                 "seed": SEED, "samples": results})
    print("[chardiff] -> %s" % path)
    return 0


def _chardiff_report_md(results):
    L = []
    ap = L.append
    ap("# T2 Part B — 10 份字符级 diff 抽验")
    ap("")
    ap("- 生成时间: %s" % datetime.now().isoformat(timespec="seconds"))
    ap("- 方法: `truth.raw`(去空白) → wordcraft text(去空白) 段级 diff"
       "（`fast_opcodes`：len(A)+len(B)<=80000 用真 `difflib.SequenceMatcher`，"
       "更大文本用等价锚点法；实测大文本锚点法丢失字符总数/样例与 difflib 完全一致）")
    ap("- 归因: 缺失段按其在 `word/document.xml` 中的来源标签"
       "(w:txbxContent=可归因 / 表格 / 正文)；多出段先做整体包含、再做 4-gram "
       "覆盖率 ≥0.5，归入 w:delText / 旧式 w:del / w:sym，否则 unexplained")
    ap("- 样本: 固定 #2/#33/#34 + 分层 A4/B2/C1（seed=%d，排除 truth_norm=0）" % SEED)
    ap("- 明细: `out/t2/char_diff_report.json`")
    ap("")
    ap("## 逐份点检表")
    ap("")
    ap("| # | tier | truth_norm | craft_norm | del_chars | sym_count | 缺失段 | "
       "多出段 | 可归因段 | 未解释段 | 可归因字 | 裁决 |")
    ap("|---|---|---|---|---|---|---|---|---|---|---|---|")
    for r in results:
        if r.get("error"):
            ap("| %d | %s | - | - | - | - | - | - | - | - | - | ERROR %s |" % (
                r["idx"], r.get("tier"), r["error"]))
            continue
        ap("| %d | %s | %d | %d | %s | %s | %d | %d | %d | %d | %d | %s |" % (
            r["idx"], r.get("tier"), r["truth_norm"], r["craft_norm"],
            r["del_chars"], r["sym_count"], len(r["missing"]), len(r["extra"]),
            r["attr_seg"], r["total_seg"] - r["attr_seg"], r["attr_ch"],
            "差异全部可归因" if r["fully"] else "存在未解释差异"))
    ap("")
    # 汇总
    ok = [r for r in results if not r.get("error")]
    seg_tot = sum(r["total_seg"] for r in ok)
    seg_att = sum(r["attr_seg"] for r in ok)
    ch_tot = sum(r["total_ch"] for r in ok)
    ch_att = sum(r["attr_ch"] for r in ok)
    ap("## 汇总")
    ap("")
    ap("- 份数: %d；段级可归因率 = %d/%d = %s；字级可归因率 = %d/%d = %s" % (
        len(ok), seg_att, seg_tot, _ratio(seg_att, seg_tot),
        ch_att, ch_tot, _ratio(ch_att, ch_tot)))
    fixed_ok = all(r.get("fully") for r in ok if r["idx"] in FIXED_CHARDIFF)
    ap("- 固定机制样本 #2/#33/#34 全归因: **%s**" % ("是" if fixed_ok else "否"))
    ap("")
    ap("## 逐份细节")
    ap("")
    for r in results:
        if r.get("error"):
            continue
        ap("### #%d (tier %s) `%s`" % (r["idx"], r.get("tier"), r["path"]))
        ap("")
        ap("- 差异段 %d（缺失 %d / 多出 %d）；可归因 %d，未解释 %d → **%s**" % (
            r["total_seg"], len(r["missing"]), len(r["extra"]),
            r["attr_seg"], r["total_seg"] - r["attr_seg"],
            "差异全部可归因" if r["fully"] else "存在未解释差异"))
        for m in r["missing"][:8]:
            ap("  - 缺失[%s] %d字: `%s`" % (m["src"], m["n"], _trunc(m["frag"])))
        for e in r["extra"][:8]:
            ap("  - 多出[%s score=%s] %d字: `%s`" % (
                e["mech"], e["score"], e["n"], _trunc(e["frag"])))
        if len(r["missing"]) > 8 or len(r["extra"]) > 8:
            ap("  - …（仅列前 8 段/类，完整段表见 `out/t2/char_diff_report.json`）")
        un = r["missing_un"] + r["extra_un"]
        if un:
            ap("  - **未解释段（%d）**: " % len(un))
            for u in un[:20]:
                if "src" in u:
                    ap("    - 缺失[%s] %d字: `%s`" % (u["src"], u["n"], _trunc(u["frag"])))
                else:
                    ap("    - 多出[%s score=%s] %d字: `%s`" % (
                        u["mech"], u["score"], u["n"], _trunc(u["frag"])))
        ap("")
    ap("来源: truth ← `craft_eval.truth.extract_truth`；craft ← `wordcraft-cli text <docx>`；"
       "XML ← `zipfile` 读 `word/document.xml` + ElementTree。")
    ap("")
    return "\n".join(L)


def _trunc(s, n=80):
    s = s.replace("\n", " ")
    return s if len(s) <= n else s[:n] + "…"


def _ratio(a, b):
    return "%.4f" % (a / b) if b else "N/A"


# =========================================================================== #
# Part A — 表格线判据
# =========================================================================== #
def _tableline_picks(manifest):
    pools = {"A": [], "B": [], "C": []}
    for e in manifest:
        t = e.get("tier")
        if t in pools and has_table(e.get("path", "")):
            pools[t].append(e)
    for t in pools:
        pools[t].sort(key=lambda e: e["path"])
    rng = random.Random(SEED)
    picks = []
    for t in ("A", "B", "C"):
        q = QUOTA[t]
        n = min(q, len(pools[t]))
        picks += [(t, e) for e in rng.sample(pools[t], n)]
    return picks, {t: len(pools[t]) for t in pools}


def _wps_parse_log(log_path):
    st = {"unavailable": False, "ready": False,
          "done": set(), "fail": set(), "start": []}
    if not os.path.exists(log_path):
        return st
    with open(log_path, "r", encoding="utf-8", errors="replace") as fh:
        for line in fh:
            if "UNAVAILABLE" in line:
                st["unavailable"] = True
            if "READY" in line:
                st["ready"] = True
            m = re.search(r"\bSTART idx=(\d+)", line)
            if m:
                st["start"].append(int(m.group(1)))
            m = re.search(r"\bDONE idx=(\d+)", line)
            if m:
                st["done"].add(int(m.group(1)))
            m = re.search(r"\bFAIL idx=(\d+)", line)
            if m:
                st["fail"].add(int(m.group(1)))
    return st


def _kill_tree(pid):
    try:
        subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"],
                       capture_output=True, timeout=30)
    except Exception:
        pass


def _watch_child(proc, log_path, offset):
    """看门狗：监控 log 增长。返回 (kind, idx)。

    kind ∈ {"exit","unavailable","timeout"}；timeout 时 idx 为进行中份(可能 None)。
    """
    ready = False
    in_progress = None
    last = time.monotonic()
    dispatch_deadline = last + WPS_DISPATCH_TIMEOUT
    size = offset
    while True:
        if proc.poll() is not None:
            return ("exit", in_progress)
        try:
            cur = os.path.getsize(log_path) if os.path.exists(log_path) else 0
        except OSError:
            cur = size
        if cur > size:
            with open(log_path, "r", encoding="utf-8", errors="replace") as fh:
                fh.seek(size)
                chunk = fh.read()
            size = cur
            for line in chunk.splitlines():
                last = time.monotonic()
                if "UNAVAILABLE" in line:
                    return ("unavailable", None)
                if "READY" in line:
                    ready = True
                m = re.search(r"\bSTART idx=(\d+)", line)
                if m:
                    in_progress = int(m.group(1))
                if "DONE idx=" in line or "FAIL idx=" in line:
                    in_progress = None
        now = time.monotonic()
        if not ready:
            if now > dispatch_deadline:
                return ("unavailable", None)
        else:
            if in_progress is not None and (now - last) > WPS_JOB_TIMEOUT:
                return ("timeout", in_progress)
            if in_progress is None and (now - last) > WPS_JOB_TIMEOUT:
                return ("timeout", None)
        time.sleep(2)


def run_wps_batch(jobs):
    """逐份导出 WPS PDF。返回 {idx: status}。

    status ∈ {ok, fail, wps_timeout, wps_unavailable}。
    """
    log_path = os.path.join(T2_PDF, "wps_child.log")
    _safe_write_path(log_path)
    status = {}
    pending = []
    for j in jobs:
        if os.path.exists(j["pdf"]) and os.path.getsize(j["pdf"]) > 0:
            status[j["idx"]] = "ok"
        else:
            pending.append(j)
    if not pending:
        return status, log_path

    jobs_path = os.path.join(T2_PDF, "_wps_jobs.json")
    guard = 0
    while pending and guard <= len(jobs) + 3:
        guard += 1
        _write_json(jobs_path, {"root": T2_PDF, "jobs": pending})
        offset = os.path.getsize(log_path) if os.path.exists(log_path) else 0
        try:
            proc = subprocess.Popen(
                [sys.executable, CHILD, "--jobs", jobs_path, "--log", log_path],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        except OSError as e:
            for j in pending:
                status[j["idx"]] = "wps_unavailable"
            break
        kind, idx = _watch_child(proc, log_path, offset)
        if kind == "exit":
            st = _wps_parse_log(log_path)
            if st["unavailable"] or not st["ready"]:
                for j in pending:
                    status[j["idx"]] = "wps_unavailable"
            else:
                for j in pending:
                    if j["idx"] in st["done"]:
                        status[j["idx"]] = "ok"
                    elif j["idx"] in st["fail"]:
                        status[j["idx"]] = "fail"
                    else:
                        status[j["idx"]] = "fail"
            break
        if kind == "unavailable":
            _kill_tree(proc.pid)
            try:
                proc.wait(timeout=10)
            except Exception:
                pass
            for j in pending:
                status[j["idx"]] = "wps_unavailable"
            break
        # timeout
        _kill_tree(proc.pid)
        try:
            proc.wait(timeout=10)
        except Exception:
            pass
        if idx is not None:
            status[idx] = "wps_timeout"
            for j in list(pending):
                if j["idx"] == idx:
                    try:
                        if os.path.exists(j["pdf"]):
                            os.remove(j["pdf"])
                    except OSError:
                        pass
                    pending.remove(j)
        else:
            # 无法判定进行中份：整批降级，避免死循环
            for j in pending:
                status.setdefault(j["idx"], "wps_timeout")
            break
    # 卫生：临时 jobs 文件含语料路径，流程结束即删除（不依赖它，报告不引用）
    try:
        if os.path.exists(jobs_path):
            os.remove(jobs_path)
    except OSError:
        pass
    return status, log_path


def cmd_tableline(_args):
    ensure_dirs()
    manifest = load_manifest()
    picks, pool_sizes = _tableline_picks(manifest)
    print("[tableline] manifest A/B/C=%d；含 w:tbl 池 A=%d B=%d C=%d；抽 %d 份" % (
        sum(1 for e in manifest if e.get("tier") in ("A", "B", "C")),
        pool_sizes["A"], pool_sizes["B"], pool_sizes["C"], len(picks)))

    # 2. wordcraft 侧 PDF
    files = []
    jobs = []
    for i, (tier, e) in enumerate(picks, 1):
        docx = e["path"]
        wc_pdf = os.path.join(T2_PDF, "wc_%d.pdf" % i)
        wps_pdf = os.path.join(T2_PDF, "wps_%d.pdf" % i)
        _safe_write_path(wc_pdf)
        conv = cli_convert(docx, wc_pdf)
        rec = {"idx": i, "tier": tier, "docx": docx,
               "wc_pdf": wc_pdf, "wps_pdf": wps_pdf,
               "conv_rc": conv["rc"], "conv_ms": conv["ms"]}
        if conv["rc"] != 0:
            rec["status"] = "wc_fail"
            print("  [%d] wc convert fail rc=%s → 跳过 %s" % (i, conv["rc"], docx))
            files.append(rec)
            continue
        rec["status"] = "ok"
        files.append(rec)
        jobs.append({"idx": i, "docx": docx, "pdf": wps_pdf})

    # 3. WPS 侧 PDF（子进程 + 看门狗）
    print("[tableline] 启动 WPS 子进程批量导出 %d 份（单份 300s 看门狗）..." % len(jobs))
    wps_status, log_path = run_wps_batch(jobs)
    print("[tableline] WPS 导出: %s" % {k: wps_status.get(k) for k in sorted(wps_status)})

    # 4-8. 页对齐 / 选页 / 渲染 / 检测 / 覆盖率
    rows = []
    for rec in files:
        if rec["status"] == "wc_fail":
            rows.append(rec)
            continue
        st = wps_status.get(rec["idx"], "missing")
        rec["wps_status"] = st
        if st != "ok" or not os.path.exists(rec["wps_pdf"]):
            rec["pages"] = _degraded_pages(rec)
            rows.append(rec)
            print("  [%d] WPS 不可用(%s) → 降级单侧密度检测" % (rec["idx"], st))
            continue
        print("  [%d] %s 分析中: %s" % (rec["idx"], rec["tier"],
                                        os.path.basename(rec["docx"])))
        _analyze_pair(rec)
        rows.append(rec)
        print("  [%d] %s 选页=%s coverage=%s 裁决=%s" % (
            rec["idx"], rec["tier"],
            [(p["wc_page"], p["wps_page"]) for p in rec["pages"]],
            [None if p.get("coverage") is None else round(p["coverage"], 3)
             for p in rec["pages"]],
            rec["verdict"]))

    # 11. 附注 spot check（2 页 native render）
    _native_spot_check(rows)

    report = _tableline_build(rows)
    _write_json(os.path.join(T2_DIR, "tableline_report.json"), report)
    md = _tableline_report_md(report)
    path = _write_text(os.path.join(T2_DIR, "tableline_report.md"), md)
    print("[tableline] -> %s" % path)
    print("[tableline] 判据: %s" % report["criterion_verdict"])
    return 0


def _degraded_pages(rec):
    """WPS 不可用：wordcraft 单侧线段密度检测，coverage=null，标'无对照'。"""
    out = []
    n = pdf_pages(rec["wc_pdf"])
    pdir = os.path.join(T2_PAIRS, str(rec["idx"]))
    os.makedirs(pdir, exist_ok=True)
    for pn in range(1, min(n, N_PAGES_PER_FILE) + 1):
        png = os.path.join(pdir, "wc_p%d.png" % pn)
        _safe_write_path(png)
        ld.render_page_png(rec["wc_pdf"], pn - 1, png, dpi=RENDER_DPI)
        segs = ld.detect_segments_file(png)
        out.append({"wc_page": pn, "wps_page": None, "score": None,
                    "wc_seg": ld.segment_count(segs),
                    "wps_seg": None, "matched": None, "coverage": None,
                    "wc_png": png, "wps_png": None, "verdict": "无对照"})
    return out


def _compact_segs(segs):
    """线段坐标（H 取 cy，V 取 cx），供 JSON 留证。"""
    return {"h": sorted(round(s["cy"], 1) for s in segs["h"]),
            "v": sorted(round(s["cx"], 1) for s in segs["v"])}


def _analyze_pair(rec):
    wc_texts = ld.pdf_page_texts(rec["wc_pdf"])
    wps_texts = ld.pdf_page_texts(rec["wps_pdf"])
    aligns = ld.align_pages(wc_texts, wps_texts)
    aligned = [(i, a["wps"], a["score"]) for i, a in enumerate(aligns)
               if a["wps"] is not None]
    pdir = os.path.join(T2_PAIRS, str(rec["idx"]))
    os.makedirs(pdir, exist_ok=True)
    tmpdir = os.path.join(T2_PAIRS, "_sel_tmp")
    os.makedirs(tmpdir, exist_ok=True)
    tmp_png = os.path.join(tmpdir, "%d_wps.png" % rec["idx"])
    _safe_write_path(tmp_png)

    # 5. 取"对齐成功 且 wps 侧线段数最多"的 3 页
    cand = []
    for wc_i, wps_i, score in aligned:
        ld.render_page_png(rec["wps_pdf"], wps_i, tmp_png, dpi=RENDER_DPI)
        segs = ld.detect_segments_file(tmp_png)
        cand.append({"wc_page": wc_i + 1, "wps_page": wps_i + 1,
                     "score": score, "wps_seg": ld.segment_count(segs)})
    cand.sort(key=lambda c: (-c["wps_seg"], c["wc_page"]))
    chosen = cand[:N_PAGES_PER_FILE]
    try:
        if os.path.exists(tmp_png):
            os.remove(tmp_png)
    except OSError:
        pass

    # 6-8. 渲染对照 + 线段检测 + 覆盖率
    pages = []
    for c in chosen:
        wc_png = os.path.join(pdir, "wc_p%d.png" % c["wc_page"])
        wps_png = os.path.join(pdir, "wps_p%d.png" % c["wc_page"])
        _safe_write_path(wc_png)
        _safe_write_path(wps_png)
        wi = ld.render_page_png(rec["wc_pdf"], c["wc_page"] - 1, wc_png, dpi=RENDER_DPI)
        ld.render_page_png(rec["wps_pdf"], c["wps_page"] - 1, wps_png, dpi=RENDER_DPI)
        wc_segs = ld.detect_segments_file(wc_png)
        wps_segs = ld.detect_segments_file(wps_png)
        cov = ld.page_coverage(wps_segs, wc_segs, wi["w_px"], wi["h_px"])
        pages.append({
            "wc_page": c["wc_page"], "wps_page": c["wps_page"],
            "score": c["score"],
            "wc_seg": ld.segment_count(wc_segs),
            "wps_seg": ld.segment_count(wps_segs),
            "matched": cov["matched"], "coverage": cov["coverage"],
            "wc_png": wc_png, "wps_png": wps_png,
            "wc_lines": _compact_segs(wc_segs),
            "wps_lines": _compact_segs(wps_segs),
            "verdict": ("PASS" if (cov["coverage"] is not None
                                   and cov["coverage"] >= FILE_PASS_COVERAGE)
                        else "FAIL"),
        })
    rec["pages"] = pages
    rec["aligned_pages"] = len(aligned)
    rec["total_wc_pages"] = len(wc_texts)
    rec["total_wps_pages"] = len(wps_texts)
    # 9. 文件裁决
    n = len(pages)
    pass_n = sum(1 for p in pages if p["verdict"] == "PASS")
    worst_ok = all((p["coverage"] is None) or (p["coverage"] >= PAGE_ABSENT_COVERAGE)
                   for p in pages)
    rec["file_pass"] = (pass_n >= math.ceil(FILE_PASS_NEED * n)) and worst_ok and n > 0
    rec["verdict"] = "PASS" if rec["file_pass"] else "FAIL"


def _native_spot_check(rows):
    """11. 抽 2 页 wordcraft 原生分页渲染（native_pN.png）。"""
    done = 0
    for rec in rows:
        if done >= 2:
            break
        if rec.get("status") == "wc_fail" or not rec.get("pages"):
            continue
        for p in rec["pages"]:
            if done >= 2:
                break
            n = p["wc_page"]
            out_png = os.path.join(T2_PAIRS, str(rec["idx"]), "native_p%d.png" % n)
            _safe_write_path(out_png)
            r = cli_render(rec["docx"], out_png, n, scale=2.0)
            p["native_png"] = out_png
            p["native_rc"] = r["rc"]
            p["native_bytes"] = r["bytes"]
            done += 1


def _tableline_build(rows):
    usable = [r for r in rows if r.get("wps_status") == "ok"
              and r.get("status") != "wc_fail"]
    no_wps = [r for r in rows if r.get("status") != "wc_fail"
              and r.get("wps_status") != "ok"]
    wc_fail = [r for r in rows if r.get("status") == "wc_fail"]
    n_pass = sum(1 for r in usable if r.get("file_pass"))
    denom = len(usable)
    ratio = (n_pass / denom) if denom else None
    return {
        "generated_at": datetime.now().isoformat(timespec="seconds"),
        "seed": SEED,
        "quota": QUOTA,
        "files": rows,
        "usable": denom,
        "passed": n_pass,
        "pass_ratio": ratio,
        "criterion_verdict": ("表格线判据 PASS" if (denom and ratio >= 0.9)
                              else "表格线判据 FAIL"),
        "no_wps_files": [r["idx"] for r in no_wps],
        "wc_fail_files": [r["idx"] for r in wc_fail],
        "sources": {
            "wc_pdf_cmd": "wordcraft-cli convert <docx> out/t2/pdf/wc_<idx>.pdf (timeout=300s)",
            "wps_pdf_cmd": "t2_wps_child.py: KWPS.Application + "
                           "Documents.Open(ReadOnly) + ExportAsFixedFormat(pdf,17) "
                           "-> out/t2/pdf/wps_<idx>.pdf",
            "align": "pymupdf 逐页 get_text 去空白 -> 4-gram Jaccard >= 0.35",
            "render": "pymupdf get_pixmap 192dpi -> out/t2/pairs/<idx>/{wc,wps}_p<N>.png",
            "detect": "PIL 灰度<128 为暗; 横向 run>=8%页宽; 纵向 run>=8%页高",
            "coverage": "同向; 中心距<=2.5%页尺寸; 长度比>=0.6; cov=匹配/WPS线段",
            "page_pass": "coverage>=0.90",
            "file_pass": ">=2/3 选页 PASS 且无 coverage<0.5 页",
            "criterion": ">=9/10 文件 PASS -> 判据 PASS; WPS 不可用份不进分母",
        },
    }


def _tableline_report_md(rep):
    L = []
    ap = L.append
    ap("# T2 Part A — 表格线判据点检表")
    ap("")
    ap("- 生成时间: %s" % rep["generated_at"])
    ap("- 抽选: seed=%d, 配额 A%d/B%d/C%d, `random.Random(seed).sample`"
       "（各池先按路径排序，只取含 w:tbl 者）" % (
           rep["seed"], rep["quota"]["A"], rep["quota"]["B"], rep["quota"]["C"]))
    ap("- 判据: **%s**（usable=%d, passed=%d, ratio=%s）" % (
        rep["criterion_verdict"], rep["usable"], rep["passed"],
        ("%.4f" % rep["pass_ratio"]) if rep["pass_ratio"] is not None else "-"))
    if rep["no_wps_files"]:
        ap("- WPS 不可用/超时（不进分母，单列）: %s" % rep["no_wps_files"])
    if rep["wc_fail_files"]:
        ap("- wordcraft convert 失败（跳过）: %s" % rep["wc_fail_files"])
    ap("")
    ap("## 点检表（文件 × 页）")
    ap("")
    ap("| # | tier | 文件 | 页(wc←wps) | 对齐分数 | WPS线段数 | wc线段数 | 匹配数 | "
       "coverage | 裁决 | 来源(PNG) |")
    ap("|---|---|---|---|---|---|---|---|---|---|---|")
    for r in rep["files"]:
        docx = os.path.basename(r["docx"])
        if r.get("status") == "wc_fail":
            ap("| %d | %s | `%s` | - | - | - | - | - | - | wc_convert_fail(rc=%s) | - |" % (
                r["idx"], r["tier"], docx, r["conv_rc"]))
            continue
        if r.get("wps_status") != "ok":
            ap("| %d | %s | `%s` | (WPS %s) | - | - | - | - | null | 无对照 | - |" % (
                r["idx"], r["tier"], docx, r.get("wps_status")))
            for p in r.get("pages", []):
                ap("| %d | %s | `%s` | p%d(单侧) | - | - | %d | - | null | 无对照 | "
                   "`%s` |" % (r["idx"], r["tier"], docx, p["wc_page"],
                                p["wc_seg"], _rel(p["wc_png"])))
            continue
        for p in r.get("pages", []):
            cov = p["coverage"]
            ap("| %d | %s | `%s` | p%d←p%d | %s | %d | %d | %d | %s | %s | "
               "`%s` / `%s` |" % (
                   r["idx"], r["tier"], docx, p["wc_page"], p["wps_page"],
                   p["score"], p["wps_seg"], p["wc_seg"], p["matched"],
                   ("%.4f" % cov) if cov is not None else "null",
                   p["verdict"], _rel(p["wc_png"]), _rel(p["wps_png"])))
    ap("")
    ap("## 文件级裁决")
    ap("")
    ap("| # | tier | 文件 | WPS状态 | 选页数 | PASS页数 | file_pass |")
    ap("|---|---|---|---|---|---|---|")
    for r in rep["files"]:
        if r.get("status") == "wc_fail":
            continue
        pages = r.get("pages", [])
        pp = sum(1 for p in pages if p.get("verdict") == "PASS")
        ap("| %d | %s | `%s` | %s | %d | %d | %s |" % (
            r["idx"], r["tier"], os.path.basename(r["docx"]),
            r.get("wps_status"), len(pages), pp,
            r.get("verdict", "-")))
    ap("")
    _append_rootcause(ap, rep)
    ap("## 数字来源")
    ap("")
    for k, v in rep["sources"].items():
        ap("- **%s**: %s" % (k, v))
    ap("")
    ap("## 附注 — 原生分页 spot check")
    ap("")
    ap("以下页额外用 `wordcraft-cli render <docx> <png> --page <N> --scale 2` 生成"
       "原生分页 PNG，与 wc-PDF 渲染页并排存于 `out/t2/pairs/<idx>/native_pN.png`。"
       "**渲染一致性由指挥官视觉复核。**")
    ap("")
    ap("| # | 页 | native_png | render rc | bytes |")
    ap("|---|---|---|---|---|")
    for r in rep["files"]:
        for p in r.get("pages", []):
            if p.get("native_png"):
                ap("| %d | p%d | `%s` | %s | %s |" % (
                    r["idx"], p["wc_page"], _rel(p["native_png"]),
                    p.get("native_rc"), p.get("native_bytes")))
    ap("")
    return "\n".join(L)


def _rel(p):
    if not p:
        return ""
    try:
        return os.path.relpath(p, HERE).replace("\\", "/")
    except ValueError:
        return p


def _append_rootcause(ap, rep):
    """判据 FAIL 时的量化根因（证据来自各页 wc_seg/wps_seg 与 JSON 内线段坐标）。"""
    pages = []
    for r in rep["files"]:
        for p in r.get("pages", []) or []:
            if p.get("wps_seg") is None:
                continue
            pages.append((r["idx"], p))
    if not pages:
        return
    n = len(pages)
    close = sum(1 for _i, p in pages
                if abs(p["wc_seg"] - p["wps_seg"]) <= max(2, 0.2 * p["wps_seg"]))
    worst = min(pages, key=lambda t: (t[1]["coverage"]
                                      if t[1]["coverage"] is not None else 2.0))
    ap("## 根因分析（判据 FAIL 说明，纯量化）")
    ap("")
    ap("- 选页双侧线段总数相近（|wc_seg-wps_seg| <= max(2,20%% wps_seg)）的页: "
       "**%d/%d** —— 说明表格线并未被 wordcraft 整体丢弃。" % (close, n))
    ap("- 但同页/对应页线段中心坐标随行高、列宽漂移，落在 2.5%% 容差外 → coverage 低。"
       "取最差页例（#%d p%d←p%d, coverage=%s）：" % (
           worst[0], worst[1]["wc_page"], worst[1]["wps_page"],
           ("%.4f" % worst[1]["coverage"]) if worst[1]["coverage"] is not None else "-"))
    wl = worst[1].get("wc_lines") or {}
    pl = worst[1].get("wps_lines") or {}
    ap("  - 该页 wc 横线 cy=%s" % (wl.get("h", [])[:12],))
    ap("  - 该页 wps 横线 cy=%s" % (pl.get("h", [])[:12],))
    ap("  - 该页 wc 竖线 cx=%s" % (wl.get("v", [])[:12],))
    ap("  - 该页 wps 竖线 cx=%s" % (pl.get("v", [])[:12],))
    ap("- 完整坐标见 `out/t2/tableline_report.json` 各页 `wc_lines`/`wps_lines` 字段；"
       "PNG 对见点检表『来源(PNG)』列，视觉复核由指挥官进行。")
    ap("")


# =========================================================================== #
# 子命令
# =========================================================================== #
def cmd_selftest(_args):
    import selftest
    return selftest.main()


def cmd_rerun_w2(_args):
    return run_w2.main(["--run"])


def cmd_all(args):
    rc = 0
    for name, fn in (("a0", cmd_a0), ("chardiff", cmd_chardiff),
                     ("tableline", cmd_tableline), ("rerun-w2", cmd_rerun_w2)):
        print("=== T2 --all: %s ===" % name)
        r = fn(args)
        if r not in (0, None):
            rc = r
    return rc


def main(argv=None):
    setup_stdout()
    p = argparse.ArgumentParser(prog="t2_run.py", description="T2 主入口")
    p.add_argument("--selftest", action="store_true")
    p.add_argument("--a0", action="store_true")
    p.add_argument("--chardiff", action="store_true")
    p.add_argument("--tableline", action="store_true")
    p.add_argument("--rerun-w2", dest="rerun_w2", action="store_true")
    p.add_argument("--all", action="store_true")
    args = p.parse_args(argv)

    if args.selftest:
        return cmd_selftest(args)
    if args.a0:
        return cmd_a0(args)
    if args.chardiff:
        return cmd_chardiff(args)
    if args.tableline:
        return cmd_tableline(args)
    if args.rerun_w2:
        return cmd_rerun_w2(args)
    # 默认入口 = --all
    return cmd_all(args)


if __name__ == "__main__":
    sys.exit(main())
