# -*- coding: utf-8 -*-
r"""cad2pdf.py — CAD 图纸转 PDF（DXF 优先保真流水线）。

保真阶梯:
  L1 原生发布(需 CAD COM)  — 本脚本仅探测并给出指引(不自动执行,避免 COM 挂死)
  L2 DXF 优先(默认)        — .dwg --ODA--> .dxf --cadcraft--> .pdf;.dxf 直达
  L3 DWG 直读              — cadcraft convert .dwg .pdf(保真低于 L2)
  L4 光栅兜底              — cadcraft 模型空间渲染 PNG -> pymupdf 拼 PDF

依赖: cadcraft-cli(artcraft bin 版本无关 glob)、ODAFileConverter(Program Files\ODA)、
      pymupdf;ezdxf 仅 --check 的布局空间检测需要。

用法:
  python cad2pdf.py <in.dwg|in.dxf> <out.pdf> [--level 2|3|4] [--version ACAD2018] [--check]
  python cad2pdf.py --batch <in_dir> <out_dir> [--level 2|3|4] [--check]
  python cad2pdf.py --check-only <out.pdf> [--source <in.dxf>]
  python cad2pdf.py --probe

退出码: 0=成功 1=转换失败 2=依赖缺失
"""
import argparse
import glob
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

try:
    sys.stdout = __import__("io").TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
except Exception:
    pass


# ---------------- 依赖解析 ----------------

def _version_key(name):
    return [int(x) for x in re.findall(r"\d+", name) or [0]]


def resolve_cadcraft():
    env = os.environ.get("CADCRAFT_CLI")
    if env and os.path.isfile(env):
        return env
    pats = [
        r"G:\程序开发\artcraft\bin\cadcraft-*-windows-x64-portable\cadcraft-cli.exe",
        os.path.join(os.path.expanduser("~"), "artcraft", "bin",
                     "cadcraft-*-windows-x64-portable", "cadcraft-cli.exe"),
    ]
    hits = []
    for pat in pats:
        hits.extend(glob.glob(pat))
    if not hits:
        return None
    return max(hits, key=lambda p: _version_key(os.path.basename(os.path.dirname(p))))


def resolve_oda():
    env = os.environ.get("ODA_CONVERTER")
    if env and os.path.isfile(env):
        return env
    pats = [
        r"C:\Program Files\ODA\ODAFileConverter*\ODAFileConverter.exe",
        r"C:\Program Files (x86)\ODA\ODAFileConverter*\ODAFileConverter.exe",
    ]
    hits = []
    for pat in pats:
        hits.extend(glob.glob(pat))
    if not hits:
        return None
    return max(hits, key=lambda p: _version_key(p))


# ---------------- 执行器 ----------------

def run(cmd, timeout=300):
    t0 = time.time()
    try:
        r = subprocess.run(cmd, capture_output=True, timeout=timeout)
        return {
            "rc": r.returncode,
            "stdout": (r.stdout or b"").decode("utf-8", "replace")[-800:],
            "stderr": (r.stderr or b"").decode("utf-8", "replace")[-800:],
            "ms": int((time.time() - t0) * 1000),
        }
    except subprocess.TimeoutExpired:
        return {"rc": "TIMEOUT", "stdout": "", "stderr": "timeout %ss" % timeout,
                "ms": int((time.time() - t0) * 1000)}
    except FileNotFoundError as e:
        return {"rc": "NOEXE", "stdout": "", "stderr": str(e),
                "ms": int((time.time() - t0) * 1000)}


# ---------------- 核心转换 ----------------

def dwg_to_dxf(dwg_path, oda_exe, out_dxf, version="ACAD2018", timeout=300):
    """ODA: 输入/输出各用独立临时目录(ODA 按目录批处理)。"""
    in_dir = tempfile.mkdtemp(prefix="cad2pdf_in_")
    out_dir = tempfile.mkdtemp(prefix="cad2pdf_out_")
    try:
        src = os.path.join(in_dir, os.path.basename(dwg_path))
        shutil.copyfile(dwg_path, src)
        # ODAFileConverter <inDir> <outDir> <ver> <type> <recurse> <audit> [filter]
        res = run([oda_exe, in_dir, out_dir, version, "DXF", "0", "1", "*.DWG"], timeout)
        produced = glob.glob(os.path.join(out_dir, "*.dxf")) + glob.glob(os.path.join(out_dir, "*.DXF"))
        if res["rc"] != 0 or not produced:
            return None, {"stage": "oda", "result": res, "produced": produced}
        shutil.copyfile(produced[0], out_dxf)
        return out_dxf, {"stage": "oda", "result": res, "out": out_dxf}
    finally:
        shutil.rmtree(in_dir, ignore_errors=True)
        shutil.rmtree(out_dir, ignore_errors=True)


def cadcraft_convert(src, dst, cadcraft_exe, timeout=300, window=None):
    cmd = [cadcraft_exe, "convert", src, dst]
    if window:
        cmd.extend(["--window", window])
    res = run(cmd, timeout)
    ok = res["rc"] == 0 and os.path.isfile(dst) and os.path.getsize(dst) > 0
    return ok, {"stage": "cadcraft", "result": res, "out": dst,
                "bytes": os.path.getsize(dst) if os.path.isfile(dst) else 0,
                "window": window}


def raster_fallback(src, dst, cadcraft_exe, timeout=300):
    """L4: 模型空间 PNG -> 单页 PDF(文字不可搜索,仅兜底)。"""
    tmp_png = os.path.join(tempfile.mkdtemp(prefix="cad2pdf_png_"), "page.png")
    ok, info = cadcraft_convert(src, tmp_png, cadcraft_exe, timeout)
    if not ok:
        return False, {"stage": "raster", "error": "PNG 渲染失败", "detail": info}
    try:
        import pymupdf
        doc = pymupdf.open()
        page = doc.new_page(width=1684, height=1191)  # A2 横向 @72dpi 近似
        page.insert_image(page.rect, filename=tmp_png)
        doc.save(dst)
        doc.close()
        return True, {"stage": "raster", "out": dst,
                      "bytes": os.path.getsize(dst) if os.path.isfile(dst) else 0}
    except Exception as e:
        return False, {"stage": "raster", "error": str(e)}
    finally:
        shutil.rmtree(os.path.dirname(tmp_png), ignore_errors=True)


# ---------------- 核验 ----------------

def check_pdf(pdf_path, source_dxf=None, full=False):
    """核验。默认带性能护栏:PDF>20MB 跳过 get_drawings(极慢),DXF>30MB 跳过 ezdxf 解析。"""
    out = {"ok": False, "file": pdf_path}
    try:
        import pymupdf
    except ImportError:
        out["error"] = "pymupdf 缺失"
        return out
    try:
        pdf_mb = os.path.getsize(pdf_path) / 1048576.0
        heavy_ok = full or pdf_mb <= 20
        doc = pymupdf.open(pdf_path)
        pages = doc.page_count
        paths = images = text_chars = 0
        for p in doc:
            if heavy_ok:
                paths += len(p.get_drawings())
            images += len(p.get_images(full=True))
            text_chars += len(p.get_text().strip())
        size0 = doc[0].rect if pages else None
        doc.close()
        out.update({
            "ok": True, "pages": pages, "images": images, "text_chars": text_chars,
            "size_mb": round(pdf_mb, 2),
            "page_size_pt": [round(size0.width, 1), round(size0.height, 1)] if size0 else None,
        })
        if heavy_ok:
            out["vector_paths"] = paths
            out["rasterized"] = (paths == 0 and images > 0)
        else:
            out["vector_paths"] = None
            out["rasterized"] = None
            out["check_note"] = "PDF>20MB: vector path 统计已跳过(性能护栏),可用 --full-check 强制"
        warnings = []
        if out.get("rasterized"):
            warnings.append("光栅化产物：文字不可搜索，建议 L1/L2 矢量路径")
        if text_chars < 20:
            warnings.append("文本层几乎为空：可能 SHX 字体缺失或图纸纯几何")
        if source_dxf and os.path.isfile(source_dxf):
            dxf_mb = os.path.getsize(source_dxf) / 1048576.0
            if dxf_mb > 30 and not full:
                out["paperspace_entities"] = None
                warnings.append("DXF>30MB: 布局空间检测已跳过(性能护栏),可用 --full-check 强制")
            else:
                try:
                    import ezdxf
                    d = ezdxf.readfile(source_dxf)
                    ps = 0
                    for name in d.layouts.names():
                        if name.lower() != "model":
                            ps += len(d.layouts.get(name))
                    if ps > 0:
                        warnings.append("源图含布局空间实体 %d 个：cadcraft 只渲染模型空间，正式交付建议 L1 原生发布或 --window 取景" % ps)
                    out["paperspace_entities"] = ps
                except Exception as e:
                    warnings.append("布局空间检测失败: %s" % e)
        if pdf_mb > 20:
            warnings.append("输出 PDF %.1fMB：整模型空间全量输出；用 --window x0,y0,x1,y1 裁剪取景可大幅缩小" % pdf_mb)
        out["warnings"] = warnings
    except Exception as e:
        out["error"] = str(e)
    return out


# ---------------- 级别 1 探测 ----------------

def probe_cad_com():
    found = []
    try:
        import win32com.client  # noqa: F401
        for prog in ("AutoCAD.Application", "ZWCAD.Application", "GstarCAD.Application"):
            try:
                app = win32com.client.Dispatch(prog)
                found.append(prog)
                try:
                    app.Quit()
                except Exception:
                    pass
            except Exception:
                pass
    except ImportError:
        return {"win32com": False, "cad": []}
    return {"win32com": True, "cad": found}


# ---------------- 主流程 ----------------

def convert_one(src, dst, level, oda_ver, cadcraft_exe, oda_exe, do_check, window=None, full_check=False):
    ext = os.path.splitext(src)[1].lower()
    report = {"ok": False, "level": level, "input": src, "output": dst}
    t0 = time.time()

    if level == 1:
        probe = probe_cad_com()
        report.update({"error": "L1 需 CAD COM 原生发布（本脚本不自动执行，避免 COM 挂死）",
                       "cad_probe": probe,
                       "hint": "用 AutoCAD/中望/浩辰 打开图纸 → 发布(PUBLISH)/打印到 PDF；或走 D:\\cad-mcp-pro"})
        return report

    if ext == ".dxf":
        ok, info = cadcraft_convert(src, dst, cadcraft_exe, window=window)
        report.update({"ok": ok, "detail": info})
        src_dxf = src
    elif ext == ".dwg":
        if level == 3:
            ok, info = cadcraft_convert(src, dst, cadcraft_exe, window=window)
            report.update({"ok": ok, "detail": info, "warning": "L3 DWG 直读保真低于 L2"})
            src_dxf = None
        else:  # L2 / L4 都先转 DXF
            if not oda_exe:
                report.update({"ok": False, "error": "ODAFileConverter 未找到（L2 需要）",
                               "hint": "设 env ODA_CONVERTER 或安装 ODA File Converter"})
                return report
            tmp_dxf = os.path.join(tempfile.mkdtemp(prefix="cad2pdf_dxf_"), "converted.dxf")
            got, oda_info = dwg_to_dxf(src, oda_exe, tmp_dxf, oda_ver)
            if not got:
                report.update({"ok": False, "error": "DWG→DXF 失败", "detail": oda_info})
                return report
            src_dxf = got
            if level == 4:
                ok, info = raster_fallback(got, dst, cadcraft_exe)
            else:
                ok, info = cadcraft_convert(got, dst, cadcraft_exe, window=window)
            report.update({"ok": ok, "detail": info, "oda": oda_info})
            shutil.rmtree(os.path.dirname(tmp_dxf), ignore_errors=True)
    else:
        report["error"] = "不支持的输入扩展名: %s" % ext
        return report

    report["ms"] = int((time.time() - t0) * 1000)
    if report["ok"] and do_check:
        report["check"] = check_pdf(dst, src_dxf, full=full_check)
    return report


def main():
    ap = argparse.ArgumentParser(description="CAD 图纸转 PDF（DXF 优先）")
    ap.add_argument("input", nargs="?", help="输入 .dwg/.dxf")
    ap.add_argument("output", nargs="?", help="输出 .pdf")
    ap.add_argument("--level", type=int, default=2, choices=[1, 2, 3, 4])
    ap.add_argument("--version", default="ACAD2018", help="ODA 输出 DXF 版本")
    ap.add_argument("--window", help="取景矩形 x0,y0,x1,y1（图纸单位，裁剪输出用）")
    ap.add_argument("--check", action="store_true", help="转换后执行完整性核验")
    ap.add_argument("--full-check", action="store_true", help="核验不做性能护栏（大文件慎用）")
    ap.add_argument("--check-only", metavar="PDF", help="只核验既有 PDF")
    ap.add_argument("--source", help="配合 --check-only 的源 DXF（布局空间检测）")
    ap.add_argument("--batch", nargs=2, metavar=("IN_DIR", "OUT_DIR"))
    ap.add_argument("--probe", action="store_true", help="探测依赖与 CAD COM")
    args = ap.parse_args()

    cadcraft_exe = resolve_cadcraft()
    oda_exe = resolve_oda()

    if args.probe:
        print(json.dumps({"cadcraft": cadcraft_exe, "oda": oda_exe,
                          "cad_com": probe_cad_com()}, ensure_ascii=False))
        return 0 if cadcraft_exe else 2

    if args.check_only:
        print(json.dumps(check_pdf(args.check_only, args.source, full=args.full_check), ensure_ascii=False))
        return 0

    if not cadcraft_exe:
        print(json.dumps({"ok": False, "error": "cadcraft-cli 未找到", "hint":
                          "设 env CADCRAFT_CLI 或安装到 G:\\程序开发\\artcraft\\bin"}, ensure_ascii=False))
        return 2

    if args.batch:
        in_dir, out_dir = args.batch
        os.makedirs(out_dir, exist_ok=True)
        files = []
        for ext in ("*.dwg", "*.DWG", "*.dxf", "*.DXF"):
            files.extend(glob.glob(os.path.join(in_dir, "**", ext), recursive=True))
        files = sorted(set(files))
        results = []
        for f in files:
            dst = os.path.join(out_dir, os.path.splitext(os.path.basename(f))[0] + ".pdf")
            results.append(convert_one(f, dst, args.level, args.version, cadcraft_exe, oda_exe,
                                       args.check, window=args.window, full_check=args.full_check))
        ok_n = sum(1 for r in results if r.get("ok"))
        print(json.dumps({"ok": ok_n == len(results), "total": len(results), "passed": ok_n,
                          "results": results}, ensure_ascii=False))
        return 0 if ok_n == len(results) else 1

    if not args.input or not args.output:
        ap.error("需要 <input> <output>，或 --batch，或 --check-only，或 --probe")

    rep = convert_one(args.input, args.output, args.level, args.version, cadcraft_exe, oda_exe,
                      args.check, window=args.window, full_check=args.full_check)
    print(json.dumps(rep, ensure_ascii=False))
    return 0 if rep.get("ok") else 1


if __name__ == "__main__":
    sys.exit(main())
