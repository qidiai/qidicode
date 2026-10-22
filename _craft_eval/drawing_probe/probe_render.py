#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""三个 DXF→PNG 渲染方案的实测驱动（只读仓库，所有产物写 %TEMP%\\wb_drawing_probe\\out\\）。

方案:
  A   ezdxf 官方 matplotlib 后端（ezdxf.addons.drawing Frontend + MatplotlibBackend）
  B1  cadcraft-cli 0.5.0 `convert`（L2 路线：自带单笔画制图字体 + 系统 TTF 回退），默认环境
  B2  同上，但设 CADCRAFT_FONTALT / CADCRAFT_FONTFALLBACK = simhei.ttf（即 SHX→TTF 字体替换）
  C   现状：G:\\程序开发\\标书软件\\skill_assets\\bid-drawing\\scripts\\dwg_to_png.py（Pillow 占位渲染），
      按生产参数 --dpi 300 原样调用（子进程 cwd 指向本探针目录，绝不写仓库）

度量:
  * 每张图渲染耗时 time.monotonic()，fx_full 每方案跑 BENCH_REPS 次报中位数
  * 峰值内存：psutil 以 5ms 轮询 RSS（含子进程树），轮询采样可能低估瞬时尖峰
  * 产物 PNG / SVG / PDF 一律落 out/，文件名 {方案}_{变体}{后缀}

用法:
  python probe_render.py                # 渲染全部变体 + 对 fx_full 做基准
  python probe_render.py --variants     # 只渲染（快）
  python probe_render.py --bench        # 只跑基准
"""
from __future__ import annotations

import argparse
import json
import os
import statistics
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "out")

CADCRAFT_CLI = r"G:\程序开发\artcraft\bin\cadcraft-0.5.0-windows-x64-portable\cadcraft-cli.exe"
PROD_DWG2PNG = r"G:\程序开发\标书软件\skill_assets\bid-drawing\scripts\dwg_to_png.py"

VARIANTS = ["empty", "frame", "geom", "text_ttf", "text_shx", "block", "full"]
METHODS = ["A", "B1", "B2", "C"]
BENCH_REPS = 5
POLL_INTERVAL = 0.005


# ---------------------------------------------------------------- 内存采样
class PeakRSS:
    """以固定间隔轮询 RSS 求峰值（psutil）。非精确，但跨方案一致，可比较。"""

    def __init__(self, pid: int):
        self.pid = pid
        self.peak = 0
        self._stop = threading.Event()
        self._thr = None

    def _loop(self):
        import psutil

        try:
            proc = psutil.Process(self.pid)
        except psutil.Error:
            return
        while not self._stop.is_set():
            total = 0
            try:
                total = proc.memory_info().rss
                for ch in proc.children(recursive=True):
                    try:
                        total += ch.memory_info().rss
                    except psutil.Error:
                        pass
            except psutil.Error:
                break
            if total > self.peak:
                self.peak = total
            self._stop.wait(POLL_INTERVAL)

    def __enter__(self):
        self._thr = threading.Thread(target=self._loop, daemon=True)
        self._thr.start()
        return self

    def __exit__(self, *exc):
        self._stop.set()
        if self._thr:
            self._thr.join(timeout=2.0)


# ---------------------------------------------------------------- 方案 A
def render_A(dxf_path: str, png_path: str, width_px: int = 2400, height_px: int = 1600) -> dict:
    """ezdxf 官方 matplotlib 后端。画布固定 2400x1600（与 cadcraft PNG 默认同尺寸，便于横向比）。"""
    import matplotlib

    matplotlib.use("agg")
    import matplotlib.pyplot as plt
    import ezdxf
    from ezdxf.addons.drawing import Frontend, RenderContext
    from ezdxf.addons.drawing.config import BackgroundPolicy, Configuration
    from ezdxf.addons.drawing.matplotlib import MatplotlibBackend

    doc = ezdxf.readfile(dxf_path)
    msp = doc.modelspace()
    ctx = RenderContext(doc)
    cfg = Configuration(background_policy=BackgroundPolicy.CUSTOM, custom_bg_color="#ffffff")
    fig = plt.figure(figsize=(width_px / 100.0, height_px / 100.0), dpi=100)
    ax = fig.add_axes([0, 0, 1, 1])
    backend = MatplotlibBackend(ax, adjust_figure=False)
    Frontend(ctx, backend, config=cfg).draw_layout(msp, finalize=True)
    fig.savefig(png_path, dpi=100, facecolor="#ffffff")
    plt.close(fig)
    return {"canvas": [width_px, height_px]}


# ---------------------------------------------------------------- 方案 B
def run_subprocess(cmd, env=None, cwd=None):
    """返回 (耗时秒, 峰值RSS字节, returncode, stdout, stderr)。"""
    t0 = time.monotonic()
    proc = subprocess.Popen(
        cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, cwd=cwd, text=True,
        encoding="utf-8", errors="replace",
    )
    with PeakRSS(proc.pid) as sampler:
        out, err = proc.communicate()
    dt = time.monotonic() - t0
    return dt, sampler.peak, proc.returncode, out or "", err or ""


def render_B(dxf_path: str, png_path: str, fontalt: str | None = None) -> dict:
    env = os.environ.copy()
    if fontalt:
        env["CADCRAFT_FONTALT"] = fontalt
        env["CADCRAFT_FONTFALLBACK"] = fontalt
    cmd = [CADCRAFT_CLI, "convert", dxf_path, png_path]
    dt, rss, rc, out, err = run_subprocess(cmd, env=env, cwd=OUT)
    if rc != 0 or not os.path.isfile(png_path):
        raise RuntimeError(f"cadcraft-cli rc={rc} err={err[:400]} out={out[:200]}")
    return {"stdout": out.strip()[:200], "stderr": err.strip()[:400], "wall": dt, "peak_rss": rss}


# ---------------------------------------------------------------- 方案 C
def render_C(dxf_path: str, png_path: str, dpi: int = 300, mode: str = "direct") -> dict:
    """生产通道 dwg_to_png.py 的三种调用口径。

    mode="cli_default" -- 完全按生产调用（services/drawing_service.py:97 的 args）：
        python dwg_to_png.py IN.dxf -o OUT --dpi 300
    mode="cli_layout"  -- 加 --layout Model（脚本 help 里给的另一条路）
    mode="direct"      -- 绕过 main() 的两处崩溃，只读 import 它的 render_layout_to_png()
        直接对 modelspace 出图，用来量这个 Pillow 占位渲染器本身的像素能力。
        （子进程强制 PYTHONDONTWRITEBYTECODE=1，绝不在仓库里落 __pycache__）
    """
    env = os.environ.copy()
    env["PYTHONIOENCODING"] = "utf-8"          # 生产脚本 print 中文，Windows 控制台默认 GBK 会炸；仅影响子进程编码
    env["PYTHONDONTWRITEBYTECODE"] = "1"       # 防止 import 生产模块时在仓库写 __pycache__

    if mode in ("cli_default", "cli_layout"):
        work = os.path.join(OUT, "_c_tmp")
        os.makedirs(work, exist_ok=True)
        for f in os.listdir(work):
            os.remove(os.path.join(work, f))
        cmd = [sys.executable, PROD_DWG2PNG, dxf_path, "-o", work, "--dpi", str(dpi)]
        if mode == "cli_layout":
            cmd += ["--layout", "Model"]
        dt, rss, rc, out, err = run_subprocess(cmd, env=env, cwd=OUT)
        pngs = [f for f in os.listdir(work) if f.lower().endswith(".png")]
        if not pngs:
            raise RuntimeError(f"rc={rc} out={out[:200]} err_tail={err[-300:]}")
        src = os.path.join(work, sorted(pngs, key=lambda n: -os.path.getsize(os.path.join(work, n)))[0])
        if os.path.isfile(png_path):
            os.remove(png_path)
        os.replace(src, png_path)
        return {"rc": rc, "stdout": out.strip()[:300], "wall": dt, "peak_rss": rss, "files": pngs}

    cmd = [sys.executable, os.path.join(HERE, "probe_render.py"), "--worker-c", dxf_path, png_path, str(dpi)]
    dt, rss, rc, out, err = run_subprocess(cmd, env=env, cwd=OUT)
    if rc != 0 or not os.path.isfile(png_path):
        raise RuntimeError(f"rc={rc} out={out[:200]} err_tail={err[-300:]}")
    return {"rc": rc, "stdout": out.strip()[:300], "wall": dt, "peak_rss": rss}


def worker_C(dxf_path: str, png_path: str, dpi: int):
    """子进程入口：只读 import 生产模块的 render_layout_to_png() 对 modelspace 出图。"""
    sys.dont_write_bytecode = True
    import ezdxf

    sys.path.insert(0, os.path.dirname(PROD_DWG2PNG))
    import dwg_to_png as prod  # noqa: E402  只读导入，不写仓库

    doc = ezdxf.readfile(dxf_path)
    img = prod.render_layout_to_png(doc.modelspace(), dpi=dpi)
    img.save(png_path)


# ---------------------------------------------------------------- 驱动
def dispatch(method: str, dxf_path: str, png_path: str) -> dict:
    if method == "A":
        t0 = time.monotonic()
        with PeakRSS(os.getpid()) as s:
            info = render_A(dxf_path, png_path)
        info["wall"] = time.monotonic() - t0
        info["peak_rss"] = s.peak
        return info
    if method == "B1":
        return render_B(dxf_path, png_path, fontalt=None)
    if method == "B2":
        # 注意：只认字体名/裸文件名，传绝对路径会被静默忽略（实测与默认输出逐字节相同）
        return render_B(dxf_path, png_path, fontalt="simhei.ttf")
    if method == "C":
        return render_C(dxf_path, png_path, dpi=300, mode="direct")
    raise ValueError(method)


def image_size(path: str):
    from PIL import Image

    with Image.open(path) as im:
        return list(im.size)


def do_render():
    results = {}
    for m in METHODS:
        for v in VARIANTS:
            dxf = os.path.join(HERE, f"fx_{v}.dxf")
            png = os.path.join(OUT, f"{m}_{v}.png")
            if not os.path.isfile(dxf):
                results[f"{m}/{v}"] = {"status": "SKIP", "error": "fixture 缺失，先跑 make_fixture.py"}
                continue
            try:
                info = dispatch(m, dxf, png)
                rec = {"status": "OK", "png": png, "wall_s": round(float(info.get("wall", -1)), 3),
                       "peak_rss_mb": round(float(info.get("peak_rss", 0)) / 1048576.0, 1),
                       "px": image_size(png)}
                for k in ("stdout", "stderr", "rc", "files", "canvas"):
                    if info.get(k):
                        rec[k] = info[k]
            except Exception as e:
                rec = {"status": "FAIL", "error": f"{type(e).__name__}: {e}"}
            results[f"{m}/{v}"] = rec

    # 生产口径复现 1：完全按 services/drawing_service.py 的调用（不带 --layout）
    # 生产口径复现 2：带 --layout Model（脚本 help 里的另一条路）
    dxf = os.path.join(HERE, "fx_full.dxf")
    for tag, mode in (("生产默认调用(无 --layout)", "cli_default"), ("生产调用(--layout Model)", "cli_layout")):
        for v in ("full", "empty"):
            try:
                info = render_C(os.path.join(HERE, f"fx_{v}.dxf"),
                                os.path.join(OUT, f"C_{v}_climode.png"), mode=mode)
                results[f"C/{v} {tag}"] = {"status": "OK", "info": info}
            except Exception as e:
                results[f"C/{v} {tag}"] = {"status": "FAIL", "error": str(e)}
    return results


def do_bench():
    dxf = os.path.join(HERE, "fx_full.dxf")
    bench = {}
    for m in METHODS:
        walls, rss = [], []
        errs = []
        for i in range(BENCH_REPS):
            png = os.path.join(OUT, f"bench_{m}_{i}.png")
            try:
                info = dispatch(m, dxf, png)
                walls.append(float(info.get("wall", -1)))
                rss.append(float(info.get("peak_rss", 0)) / 1048576.0)
                if info.get("stderr"):
                    errs.append(str(info["stderr"])[:200])
            except Exception as e:
                errs.append(f"{type(e).__name__}: {e}")
            finally:
                if os.path.isfile(png):
                    os.remove(png)
        bench[m] = {
            "reps": BENCH_REPS, "ok": len(walls),
            "wall_median_s": round(statistics.median(walls), 3) if walls else None,
            "wall_min_s": round(min(walls), 3) if walls else None,
            "wall_max_s": round(max(walls), 3) if walls else None,
            "walls_all": [round(w, 3) for w in walls],
            "peak_rss_median_mb": round(statistics.median(rss), 1) if rss else None,
            "peak_rss_max_mb": round(max(rss), 1) if rss else None,
            "errors": errs[:3],
            "mode": "in-process 复用已加载模块（热路径）" if m == "A" else "子进程冷启动",
        }

    # A 的冷启动对照：与 B/C 同口径（每次新起 python 进程，含 import matplotlib 的开销）
    walls, rss, errs = [], [], []
    for i in range(BENCH_REPS):
        png = os.path.join(OUT, f"bench_Acold_{i}.png")
        dt, peak, rc, out, err = run_subprocess(
            [sys.executable, os.path.join(HERE, "probe_render.py"), "--worker-a", dxf, png],
            cwd=OUT,
        )
        if rc == 0 and os.path.isfile(png):
            walls.append(dt)
            rss.append(peak / 1048576.0)
        else:
            errs.append((err or out)[:200])
        if os.path.isfile(png):
            os.remove(png)
    bench["A_cold"] = {
        "reps": BENCH_REPS, "ok": len(walls),
        "wall_median_s": round(statistics.median(walls), 3) if walls else None,
        "wall_min_s": round(min(walls), 3) if walls else None,
        "wall_max_s": round(max(walls), 3) if walls else None,
        "walls_all": [round(w, 3) for w in walls],
        "peak_rss_median_mb": round(statistics.median(rss), 1) if rss else None,
        "peak_rss_max_mb": round(max(rss), 1) if rss else None,
        "errors": errs[:3],
        "mode": "子进程冷启动（含 import）",
    }
    return bench


def main():
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    ap = argparse.ArgumentParser()
    ap.add_argument("--variants", action="store_true", help="只渲染变体")
    ap.add_argument("--bench", action="store_true", help="只跑 fx_full 基准")
    ap.add_argument("--worker-a", nargs=2, metavar=("DXF", "PNG"), help="内部：冷启动渲染一次方案 A")
    ap.add_argument("--worker-c", nargs=3, metavar=("DXF", "PNG", "DPI"), help="内部：子进程调用生产渲染函数（方案 C）")
    args = ap.parse_args()

    if args.worker_a:
        render_A(args.worker_a[0], args.worker_a[1])
        return

    if args.worker_c:
        worker_C(args.worker_c[0], args.worker_c[1], int(args.worker_c[2]))
        return

    os.makedirs(OUT, exist_ok=True)
    run_all = not (args.variants or args.bench)
    payload = {}
    rpath = os.path.join(HERE, "render_results.json")
    if run_all or args.variants:
        payload["render"] = do_render()
    if run_all or args.bench:
        payload["bench"] = do_bench()
    with open(rpath, "w", encoding="utf-8") as f:
        json.dump(payload, f, ensure_ascii=False, indent=2)
    print(json.dumps(payload, ensure_ascii=False, indent=2)[:4000])


if __name__ == "__main__":
    main()
