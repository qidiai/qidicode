#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""run_w2.py — W2 试金石评估 harness 主入口 (T1 规格书 §5).

子命令（argparse）:
    --selftest   转调 selftest.main()，全过打印 SELFTEST OK 退出 0
    --probe      CLI exe 缺失 → {"error":"cli_missing",...} 退出码 2；
                 存在 → 取 A 池最小文件跑 info+text+render(第1页) 打印 JSON 退出 0
    --preflight  小(<1MB)/中(1-10MB)/大(10-50MB) 3 份全流程 + 45 份线性外推 ETA
    --run        manifest 全量串行执行，产出 records/report

大输出落盘：text 全文写 records JSON，终端只打摘要行。
"""
import argparse
import io
import json
import math
import os
import statistics
import sys
import time
from datetime import datetime

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

from craft_eval import metrics, sampling, truth  # noqa: E402
from craft_eval.cli import DEFAULT_EXE, WordcraftCLI  # noqa: E402

CORPUS = sampling.DEFAULT_CORPUS
SEED = 20261012

OUT_DIR = os.path.join(HERE, "out")
RECORDS_DIR = os.path.join(OUT_DIR, "records")
PAGES_DIR = os.path.join(OUT_DIR, "pages")
MANIFEST_PATH = os.path.join(OUT_DIR, "manifest.json")
REPORT_JSON = os.path.join(OUT_DIR, "report.json")
REPORT_MD = os.path.join(OUT_DIR, "report.md")

MB = 1024 * 1024
BIG_TIMEOUT = 600  # D 组 info 超时
STD_TIMEOUT = 180
TOTAL_FOR_ETA = 45  # A/B/C 全量


def setup_stdout():
    """任何 print 中文前把 stdout 切到 UTF-8（本机 PS5.1 GBK 坑，规格书约束）。

    幂等：已包装过则跳过（避免二次包同一 buffer 导致 GC 关管道）。
    """
    try:
        if getattr(sys.stdout, "_craft_utf8", False):
            return
        if sys.stdout is not None and hasattr(sys.stdout, "buffer"):
            w = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
            w._craft_utf8 = True
            sys.stdout = w
    except Exception:
        pass


def ensure_dirs():
    for d in (OUT_DIR, RECORDS_DIR, PAGES_DIR):
        os.makedirs(d, exist_ok=True)


def _safe_write_path(p):
    """写路径防护：只允许写 OUT_DIR 内（防语料误写事故重演）。"""
    out_root = os.path.abspath(OUT_DIR)
    ap = os.path.abspath(p)
    if not ap.startswith(out_root + os.sep):
        raise AssertionError("write outside out/ blocked: %s" % p)


def pages_to_render(pages):
    """第 1 / ceil(pages/2) / pages 三页（去重排序）。pages 未知 → [1]。"""
    try:
        p = int(pages)
    except (TypeError, ValueError):
        return [1]
    if p < 1:
        return [1]
    cand = {1, int(math.ceil(p / 2.0)), p}
    return sorted(x for x in cand if 1 <= x <= p) or [1]


# --------------------------------------------------------------------------- #
# 单文件执行
# --------------------------------------------------------------------------- #
def _verdict(rec, info, truth_error):
    rcs = [rec["rc_info"], rec["rc_text"], rec["rc_render"]]
    if any(rc == "TIMEOUT" for rc in rcs):
        return "timeout"
    if truth_error:
        return "truth_error"
    if any(rc == 101 for rc in rcs):
        return "panic"
    if info.get("parse_error"):
        return "parse_error"
    if rec["rc_text"] not in (0,) or (not rec["text"] and (rec["truth_norm"] or 0) > 0):
        return "fail_text"
    if rec["rc_render"] not in (0,):
        return "fail_render"
    return "ok"


def run_one(cli, entry, idx, d_smoke=False):
    """执行一份文件，返回 record dict（含全文 text）。"""
    rec = {
        "path": entry["path"], "tier": entry["tier"], "size_mb": entry["size_mb"],
        "rc_info": None, "rc_text": None, "rc_render": None, "pages": None,
        "ms_info": None, "ms_text": None, "ms_render": None,
        "truth_norm": None, "craft_norm": None, "completeness": None,
        "u_fffd": None, "cjk_truth": None, "cjk_craft": None, "verdict": None,
        "text": None,
    }

    ins = cli.info(entry["path"], timeout=BIG_TIMEOUT if d_smoke else STD_TIMEOUT)
    rec["rc_info"] = ins["rc"]
    rec["ms_info"] = ins["ms"]

    if d_smoke:
        # D 组只跑 info，不进统计
        rec["verdict"] = ("timeout" if ins["rc"] == "TIMEOUT"
                          else "panic" if ins["rc"] == 101
                          else "ok" if ins["rc"] == 0 else "fail_info")
        return rec

    pages = None
    if isinstance(ins.get("json"), dict):
        pages = ins["json"].get("pages")
    rec["pages"] = int(pages) if isinstance(pages, (int, float)) else pages

    # text
    tx = cli.text(entry["path"], timeout=STD_TIMEOUT)
    rec["rc_text"] = tx["rc"]
    rec["ms_text"] = tx["ms"]
    craft_text = tx["out"] or ""
    rec["text"] = craft_text

    # truth
    tr = truth.extract_truth(entry["path"])
    truth_error = tr["error"] is not None
    rec["truth_norm"] = tr["norm_chars"]
    rec["cjk_truth"] = metrics.cjk_ratio(tr["raw"])

    # metrics
    comp = metrics.completeness(craft_text, tr["norm_chars"])
    rec["craft_norm"] = comp["craft_norm"]
    rec["completeness"] = comp["ratio"]
    rec["u_fffd"] = metrics.u_fffd_count(craft_text)
    rec["cjk_craft"] = metrics.cjk_ratio(craft_text)

    # render 第 1 / ceil(pages/2) / pages
    plist = pages_to_render(pages)
    pdir = os.path.join(PAGES_DIR, str(idx))
    os.makedirs(pdir, exist_ok=True)
    rcs = []
    ms_sum = 0.0
    for pg in plist:
        png = os.path.join(pdir, "page_%d.png" % pg)
        _safe_write_path(png)
        r = cli.render(entry["path"], png, pg, scale=2.0, timeout=STD_TIMEOUT)
        rcs.append(r["rc"])
        ms_sum += r["ms"]
    rec["ms_render"] = round(ms_sum, 1)
    rec["rc_render"] = next((rc for rc in rcs if rc != 0), 0) if rcs else None

    rec["verdict"] = _verdict(rec, ins, truth_error)
    return rec


def write_record(idx, rec):
    path = os.path.join(RECORDS_DIR, "%d.json" % idx)
    _safe_write_path(path)
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(rec, fh, ensure_ascii=False, indent=2)
    return path


def summary_line(idx, total, rec):
    return ("[%d/%d] %s %s | rc(info/text/render)=%s/%s/%s | verdict=%s | "
            "comp=%s truth_norm=%s craft_norm=%s | fffd=%s | ms=%s/%s/%s") % (
        idx, total, rec["tier"], rec["path"],
        rec["rc_info"], rec["rc_text"], rec["rc_render"], rec["verdict"],
        _fmt(rec["completeness"]), rec["truth_norm"], rec["craft_norm"],
        rec["u_fffd"], rec["ms_info"], rec["ms_text"], rec["ms_render"])


def _fmt(x):
    if x is None:
        return "-"
    try:
        if math.isinf(x):
            return "inf"
    except TypeError:
        return str(x)
    return "%.4f" % x


# --------------------------------------------------------------------------- #
# 报告
# --------------------------------------------------------------------------- #
def _finite(vals):
    return [v for v in vals if v is not None and isinstance(v, (int, float))
            and not (isinstance(v, float) and math.isinf(v))]


def _tier_stats(records, tier):
    rows = [r for r in records if r["tier"] == tier]
    ratios = _finite([r["completeness"] for r in rows])
    u_total = sum((r["u_fffd"] or 0) for r in rows)
    failures = [{"path": r["path"], "verdict": r["verdict"]}
                for r in rows if r["verdict"] != "ok"]
    ms_info = [r["ms_info"] for r in rows if isinstance(r["ms_info"], (int, float))]
    ms_text = [r["ms_text"] for r in rows if isinstance(r["ms_text"], (int, float))]
    ms_render = [r["ms_render"] for r in rows if isinstance(r["ms_render"], (int, float))]
    rcs_info = sorted({str(r["rc_info"]) for r in rows})
    rcs_text = sorted({str(r["rc_text"]) for r in rows})
    rcs_render = sorted({str(r["rc_render"]) for r in rows})
    return {
        "n": len(rows),
        "ok": sum(1 for r in rows if r["verdict"] == "ok"),
        "completeness": {
            "mean": (statistics.mean(ratios) if ratios else None),
            "median": (statistics.median(ratios) if ratios else None),
            "min": (min(ratios) if ratios else None),
            "n_measured": len(ratios),
        },
        "u_fffd_total": u_total,
        "failures": failures,
        "ms": {
            "info_sum": round(sum(ms_info), 1),
            "text_sum": round(sum(ms_text), 1),
            "render_sum": round(sum(ms_render), 1),
        },
        "sources": {
            "info_cmd": "wordcraft-cli info <path>",
            "text_cmd": "wordcraft-cli text <path>",
            "render_cmd": "wordcraft-cli render <path> <out.png> --page <N> --scale 2.0",
            "rc_info_observed": rcs_info,
            "rc_text_observed": rcs_text,
            "rc_render_observed": rcs_render,
        },
    }


def build_report(records, manifest, cli, elapsed_ms):
    tiers = {t: _tier_stats(records, t) for t in ("A", "B", "C")}
    d_rows = [r for r in records if r["tier"] == "D"]
    report = {
        "generated_at": datetime.now().isoformat(timespec="seconds"),
        "cli_exe": cli.exe,
        "cli_exists": cli.exists(),
        "corpus": CORPUS,
        "seed": SEED,
        "manifest_total": len(manifest),
        "manifest_counts": {t: sum(1 for e in manifest if e["tier"] == t)
                            for t in ("A", "B", "C", "D")},
        "elapsed_ms": round(elapsed_ms, 1),
        "tiers": tiers,
        "D_smoke": [{"path": r["path"], "size_mb": r["size_mb"],
                     "rc_info": r["rc_info"], "ms_info": r["ms_info"],
                     "verdict": r["verdict"]} for r in d_rows],
    }
    return report


def write_report_md(report):
    L = []
    ap = L.append
    ap("# W2 试金石保真度回归报告")
    ap("")
    ap("- 生成时间: %s" % report["generated_at"])
    ap("- CLI: `%s` (exists=%s)" % (report["cli_exe"], report["cli_exists"]))
    ap("- 语料根: `%s`  seed=%s" % (report["corpus"], report["seed"]))
    ap("- manifest 总数: %d  A=%d B=%d C=%d D=%d  (来源: out/manifest.json)" % (
        report["manifest_total"],
        report["manifest_counts"]["A"], report["manifest_counts"]["B"],
        report["manifest_counts"]["C"], report["manifest_counts"]["D"]))
    ap("- 总耗时: %s ms (来源: time.monotonic 差分, run_w2 --run)" % report["elapsed_ms"])
    if not report["cli_exists"]:
        ap("")
        ap("> ⚠️ **CLI 缺失** (`%s` 不存在)：本次所有 info/text/render 记为 "
           "rc=`NOEXE`，A/B/C 记录 verdict=`fail_text`。真值(truth)由纯 Python "
           "从 docx 解出，仍有效。报告不代表 wordcraft 真实行为。" % report["cli_exe"])
    ap("")
    ap("## A/B/C 分层统计")
    ap("")
    ap("| tier | n | ok | 完整率均值 | 完整率中位 | 完整率最小 | n_measured | "
       "U+FFFD总数 | 失败数 | info_ms | text_ms | render_ms |")
    ap("|---|---|---|---|---|---|---|---|---|---|---|---|")
    for t in ("A", "B", "C"):
        s = report["tiers"][t]
        c = s["completeness"]
        ap("| %s | %d | %d | %s | %s | %s | %d | %d | %d | %s | %s | %s |" % (
            t, s["n"], s["ok"], _fmt(c["mean"]), _fmt(c["median"]), _fmt(c["min"]),
            c["n_measured"], s["u_fffd_total"], len(s["failures"]),
            s["ms"]["info_sum"], s["ms"]["text_sum"], s["ms"]["render_sum"]))
    ap("")
    ap("每个统计数字的来源（命令/rc/ms 字段）：")
    ap("")
    for t in ("A", "B", "C"):
        s = report["tiers"][t]
        src = s["sources"]
        ap("- **%s**: n/完整率/U+FFFD ← `%s` + `%s` 的 out/rc/ms；"
           "耗时 ← 命令 ms 字段求和；观测 rc: info=%s text=%s render=%s" % (
               t, src["text_cmd"], src["info_cmd"],
               src["rc_info_observed"], src["rc_text_observed"],
               src["rc_render_observed"]))
    ap("")
    ap("## 失败清单 (A/B/C)")
    ap("")
    any_fail = False
    for t in ("A", "B", "C"):
        for f in report["tiers"][t]["failures"]:
            any_fail = True
            ap("- [%s] `%s` → verdict=%s" % (t, f["path"], f["verdict"]))
    if not any_fail:
        ap("- (无)")
    ap("")
    ap("## D 组（大件冒烟，单列，不进统计）")
    ap("")
    ap("| path | size_mb | rc_info | ms_info | verdict |")
    ap("|---|---|---|---|---|")
    for it in report["D_smoke"]:
        ap("| `%s` | %s | %s | %s | %s |" % (
            it["path"], it["size_mb"], it["rc_info"], it["ms_info"], it["verdict"]))
    ap("")
    ap("来源: `wordcraft-cli info <path>` 单次调用 (timeout=600s) 的 rc/ms 字段；"
       "D 组仅 info，不含 text/render。")
    ap("")
    return "\n".join(L)


# --------------------------------------------------------------------------- #
# 子命令
# --------------------------------------------------------------------------- #
def cmd_selftest(_args):
    import selftest
    return selftest.main()


def cmd_probe(_args):
    cli = WordcraftCLI()
    if not cli.exists():
        print(json.dumps({"error": "cli_missing", "path": cli.exe},
                         ensure_ascii=False))
        return 2
    a, _b, _c, _d = sampling.pools(CORPUS)
    if not a:
        print(json.dumps({"error": "empty_pool_A", "corpus": CORPUS},
                         ensure_ascii=False))
        return 2
    path = min(a, key=lambda t: t[1])[0]  # A 池最小文件
    ensure_dirs()
    ins = cli.info(path, timeout=STD_TIMEOUT)
    tx = cli.text(path, timeout=STD_TIMEOUT)
    png = os.path.join(OUT_DIR, "_probe_page1.png")
    _safe_write_path(png)
    rd = cli.render(path, png, 1, scale=2.0, timeout=STD_TIMEOUT)
    out = {
        "path": path,
        "info": {"rc": ins["rc"], "json": ins["json"],
                 "parse_error": ins["parse_error"], "ms": ins["ms"]},
        "text": {"rc": tx["rc"], "len": len(tx["out"] or ""), "ms": tx["ms"]},
        "render": {"rc": rd["rc"], "png_bytes": rd["png_bytes"], "ms": rd["ms"]},
    }
    print(json.dumps(out, ensure_ascii=False, indent=2))
    return 0


def _pick_bands(pool):
    """从 pool[(path,size)] 里挑 小(<1MB)/中(1-10MB)/大(10-50MB) 各一份。"""
    small = mid = big = None
    by_small = sorted(pool, key=lambda t: t[1])
    by_path = sorted(pool, key=lambda t: t[0])
    for p, s in by_small:
        if s < 1 * MB:
            small = (p, s)
            break
    for p, s in by_path:
        if 1 * MB <= s < 10 * MB:
            mid = (p, s)
            break
    for p, s in by_path:
        if 10 * MB <= s <= 50 * MB:
            big = (p, s)
            break
    out = []
    for band, item in (("small", small), ("mid", mid), ("big", big)):
        if item:
            out.append({"band": band, "path": item[0], "size_mb": round(item[1] / MB, 3)})
    return out


def cmd_preflight(_args):
    cli = WordcraftCLI()
    a, b, c, _d = sampling.pools(CORPUS)
    pool = a + b + c
    picks = _pick_bands(pool)
    ensure_dirs()
    results = []
    total_ms = 0.0
    for item in picks:
        path = item["path"]
        t0 = time.monotonic()
        ins = cli.info(path, timeout=STD_TIMEOUT)
        tx = cli.text(path, timeout=STD_TIMEOUT)
        pages = None
        if isinstance(ins.get("json"), dict):
            pages = ins["json"].get("pages")
        plist = pages_to_render(pages)
        pdir = os.path.join(PAGES_DIR, "_preflight")
        os.makedirs(pdir, exist_ok=True)
        rsum = 0.0
        for pg in plist:
            png = os.path.join(pdir, "p%d.png" % pg)
            _safe_write_path(png)
            r = cli.render(path, png, pg, scale=2.0,
                           timeout=STD_TIMEOUT)
            rsum += r["ms"]
        wall = round((time.monotonic() - t0) * 1000, 1)
        total_ms += wall
        results.append({
            "band": item["band"], "path": path, "size_mb": item["size_mb"],
            "pages": pages, "render_pages": plist,
            "ms_info": ins["ms"], "ms_text": tx["ms"], "ms_render": round(rsum, 1),
            "ms_wall_total": wall,
            "rc_info": ins["rc"], "rc_text": tx["rc"],
        })
    mean_ms = (total_ms / len(results)) if results else 0.0
    out = {
        "cli_exe": cli.exe, "cli_exists": cli.exists(),
        "items": results,
        "mean_ms_per_file": round(mean_ms, 1),
        "eta_45_ms": round(mean_ms * TOTAL_FOR_ETA, 1),
        "eta_45_s": round(mean_ms * TOTAL_FOR_ETA / 1000.0, 1),
        "note": "eta_45 = mean_ms_per_file * 45 (A/B/C 线性外推)",
    }
    print(json.dumps(out, ensure_ascii=False, indent=2))
    return 0


def cmd_run(_args):
    ensure_dirs()
    manifest = sampling.build_manifest(CORPUS, seed=SEED)
    _safe_write_path(MANIFEST_PATH)
    sampling.write_manifest(manifest, MANIFEST_PATH)

    cli = WordcraftCLI()
    if not cli.exists():
        sys.stderr.write("[warn] CLI 缺失: %s — rc 将记 NOEXE\n" % cli.exe)

    total = len(manifest)
    records = []
    print("manifest: total=%d  %s" % (
        total, {t: sum(1 for e in manifest if e["tier"] == t)
                for t in ("A", "B", "C", "D")}))
    print("manifest -> %s" % MANIFEST_PATH)

    t0 = time.monotonic()
    for idx, entry in enumerate(manifest, start=1):
        d_smoke = entry["tier"] == "D"
        rec = run_one(cli, entry, idx, d_smoke=d_smoke)
        write_record(idx, rec)
        records.append(rec)
        print(summary_line(idx, total, rec))

    elapsed = (time.monotonic() - t0) * 1000.0
    report = build_report(records, manifest, cli, elapsed)
    _safe_write_path(REPORT_JSON)
    with open(REPORT_JSON, "w", encoding="utf-8") as fh:
        json.dump(report, fh, ensure_ascii=False, indent=2)
    _safe_write_path(REPORT_MD)
    with open(REPORT_MD, "w", encoding="utf-8") as fh:
        fh.write(write_report_md(report))
    print("report -> %s" % REPORT_JSON)
    print("report -> %s" % REPORT_MD)
    return 0


# --------------------------------------------------------------------------- #
def main(argv=None):
    setup_stdout()
    parser = argparse.ArgumentParser(
        prog="run_w2.py", description="W2 试金石评估 harness (T1)")
    parser.add_argument("--selftest", action="store_true", help="跑 selftest")
    parser.add_argument("--probe", action="store_true", help="探测 CLI")
    parser.add_argument("--preflight", action="store_true", help="小/中/大预检 + ETA")
    parser.add_argument("--run", action="store_true", help="全量串行执行")
    args = parser.parse_args(argv)

    if args.selftest:
        return cmd_selftest(args)
    if args.probe:
        return cmd_probe(args)
    if args.preflight:
        return cmd_preflight(args)
    if args.run:
        return cmd_run(args)

    parser.print_help()
    return 1


if __name__ == "__main__":
    sys.exit(main())
