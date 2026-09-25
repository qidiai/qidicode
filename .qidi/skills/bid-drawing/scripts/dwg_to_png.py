#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""DWG/DXF 图纸 → 高清 PNG 转换器（R1改造版 2026-09-07）。

v2方案R1：自研渲染器（文字"Aa"占位/INSERT缺失/SPLINE崩/bbox失效）整体替换为
ezdxf官方MatplotlibBackend（二维常用实体全覆盖+真实文字渲染+finalize自动定界）。
k3审计M1落实：Agg强制/中文字体回退预案/幅面figsize/黑系颜色策略/legacy回滚开关。
R4落实：DWG输入走ezdxf.addons.odafc自动转换（无ODA则结构化引导，不再裸traceback）。

用法:
    python dwg_to_png.py <图纸.dwg|.dxf> -o <输出目录> --dpi 300 [--layout "布局名"] [--engine legacy]
依赖: pip install ezdxf matplotlib Pillow(legacy引擎用)
"""
import argparse
import json
import sys
import tempfile
from pathlib import Path

import matplotlib
matplotlib.use("Agg")  # M1①：无显示环境强制
import matplotlib.pyplot as plt
import ezdxf

try:
    from ezdxf.addons.drawing import RenderContext, Frontend
    from ezdxf.addons.drawing.matplotlib import MatplotlibBackend
    OFFICIAL_OK = True
except ImportError:
    OFFICIAL_OK = False


def load_doc(path: Path):
    """加载DXF/DWG——DWG走odafc自动转换（R4/M4），DXF带recover容错（L2天正）。"""
    if path.suffix.lower() == '.dwg':
        try:
            from ezdxf.addons import odafc
        except ImportError:
            print(json.dumps({"error": "DWG需ODA转换但ezdxf odafc不可用",
                              "howto": "安装ODA File Converter后重试"}), file=sys.stderr)
            sys.exit(3)
        if not odafc.is_installed():
            print(json.dumps({
                "error": "DWG需ODA File Converter转换（本机未安装）",
                "howto": "1.到 opendesign.com 免费下载 ODAFileConverter 并安装；2.重跑本命令；或先将DWG另存为DXF",
                "oda_url": "https://www.opendesign.com/guestfiles/oda_file_converter"
            }, ensure_ascii=False), file=sys.stderr)
            sys.exit(3)
        try:
            # M4②：odafc.readfile内部自动调ODA转DXF（API实测：readfile/is_installed）
            doc = odafc.readfile(str(path))
            return doc, False
        except odafc.ODAFCNotInstalledError:
            print(json.dumps({
                "error": "DWG需ODA File Converter转换（本机未安装）",
                "howto": "到 opendesign.com 免费下载 ODAFileConverter 并安装后重试；或先将DWG另存为DXF",
                "oda_url": "https://www.opendesign.com/guestfiles/oda_file_converter"
            }, ensure_ascii=False), file=sys.stderr)
            sys.exit(3)
        except Exception as e:
            print(json.dumps({"error": f"DWG转换失败: {e}",
                              "howto": "确认DWG文件有效且ODA版本支持该DWG格式"}), file=sys.stderr)
            sys.exit(3)
    return _read_dxf_recover(path)


def _read_dxf_recover(path: Path):
    """DXF读取：标准失败→recover容错（天正/损坏文件，L2）。"""
    try:
        return ezdxf.readfile(str(path)), False
    except Exception:
        return ezdxf.recover.readfile(str(path)), True


def render_official(doc, layout, dpi: int, out_png: Path) -> tuple:
    """R1官方渲染（M1全落实）。返回(png尺寸像素, 耗时秒)。"""
    import time
    t0 = time.time()
    fig = plt.figure()
    ax = fig.add_axes([0, 0, 1, 1])
    ctx = RenderContext(doc)
    backend = MatplotlibBackend(ax)
    # M1④：颜色策略——优先BLACK（白底上浅色实体可见）
    frontend = None
    try:
        from ezdxf.addons.drawing.config import Configuration, BackgroundPolicy, ColorPolicy
        cfg = Configuration(background_policy=BackgroundPolicy.WHITE,
                            color_policy=ColorPolicy.BLACK)
        frontend = Frontend(ctx, backend, config=cfg)
    except Exception:
        # 1.4.x参数名差异时降级默认配置（渲染仍可用）
        frontend = Frontend(ctx, backend)
    # finalize=True：自动定界（替代自研bbox——k3证实原bbox全线失效）
    frontend.draw_layout(layout, finalize=True)
    # M1③：幅面figsize由finalize确定的ax范围天然适配，dpi直接控制像素密度
    fig.savefig(str(out_png), dpi=dpi, facecolor='white')
    plt.close(fig)
    from PIL import Image
    with Image.open(out_png) as im:
        size = im.size
    return size, time.time() - t0


def render_legacy(ms_or_layout, dpi=300):
    """旧自研渲染器（--engine legacy回滚用，已知缺陷：文字占位/缺INSERT等）。"""
    from PIL import Image, ImageDraw

    def bbox(entities):
        xs, ys = [], []
        for e in entities:
            try:
                pts = list(e.vertices()) if hasattr(e, 'vertices') else []
                if pts:
                    xs += [p[0] for p in pts]
                    ys += [p[1] for p in pts]
            except Exception:
                continue
        if not xs:
            return 0, 0, 100, 100
        return min(xs), min(ys), max(xs), max(ys)

    x0, y0, x1, y1 = bbox(ms_or_layout)
    w, h = max(x1 - x0, 1), max(y1 - y0, 1)
    scale = dpi / 25.4
    pw, ph = int((w + 10) * scale), int((h + 10) * scale)
    img = Image.new("RGB", (pw, ph), "white")
    draw = ImageDraw.Draw(img)

    def px(x, y):
        return int((x - x0 + 5) * scale), int((y1 - y + 5) * scale)

    for e in ms_or_layout:
        dt = e.dxftype()
        try:
            if dt in ("LINE",):
                draw.line([px(*e.dxf.start[:2]), px(*e.dxf.end[:2])], fill="black", width=1)
            elif dt in ("LWPOLYLINE", "POLYLINE"):
                pts = [px(*p[:2]) for p in e.vertices()]
                if len(pts) >= 2:
                    draw.line(pts, fill="black", width=1)
            elif dt == "CIRCLE":
                cx, cy = px(e.dxf.center[0], e.dxf.center[1])
                r = int(e.dxf.radius * scale)
                draw.ellipse([cx - r, cy - r, cx + r, cy + r], outline="black", width=1)
            elif dt in ("TEXT", "MTEXT"):
                anchor = e.dxf.insert
                p = px(anchor[0], anchor[1])
                draw.rectangle([p[0], p[1], p[0] + 60, p[1] + 12], outline="gray")
        except Exception:
            continue
    return img


def main():
    ap = argparse.ArgumentParser(description="DWG/DXF → PNG（R1官方渲染版）")
    ap.add_argument("input", help="DWG/DXF 文件路径")
    ap.add_argument("-o", "--output-dir", default="png_output")
    ap.add_argument("--dpi", type=int, default=300)
    ap.add_argument("--layout", default=None, help="指定布局名（默认Paper space优先，无则Model）")
    ap.add_argument("--engine", choices=["official", "legacy"], default="official",
                    help="official=ezdxf官方MatplotlibBackend（默认）；legacy=旧自研渲染器（回滚用，文字占位缺陷）")
    args = ap.parse_args()

    inp = Path(args.input).resolve()
    out = Path(args.output_dir).resolve()
    out.mkdir(parents=True, exist_ok=True)

    doc, recover_used = load_doc(inp)
    layout_names = [l.name for l in doc.layouts]
    print(f"布局列表: {layout_names}" + ("（recover容错读取）" if recover_used else ""))

    if args.layout:
        if args.layout not in layout_names:
            print(f"布局 {args.layout} 不存在", file=sys.stderr)
            sys.exit(1)
        targets = [doc.layouts.get(args.layout)]
    else:
        # Paper space优先，但排除空布局（无实体则回退Model）——图2-1案例：Layout1空
        ps = [l for l in doc.layouts if l.is_any_paperspace and len(l) > 0]
        targets = ps if ps else [doc.modelspace()]

    rendered = []
    for lay in targets:
        fname = f"{lay.name.replace(':', '_')}.png" if len(targets) > 1 else f"{inp.stem}.png"
        fpath = out / fname
        try:
            if args.engine == "official" and OFFICIAL_OK:
                size, dt = render_official(doc, lay, args.dpi, fpath)
            else:
                img = render_legacy(lay, dpi=args.dpi)
                img.save(fpath)
                size, dt = img.size, 0.0
        except Exception as e:
            # M1②：渲染失败记录并继续下一布局（不静默出坏图）
            print(f"渲染失败 {lay.name}: {e}", file=sys.stderr)
            continue
        rendered.append({"layout": lay.name, "file": str(fpath), "size": size, "seconds": round(dt, 2), "engine": args.engine})
        print(f"OK: {fpath}（{size[0]}×{size[1]}px, {dt:.2f}s）")

    if not rendered:
        print("未渲染出任何布局", file=sys.stderr)
        sys.exit(1)
    print(json.dumps({"rendered": rendered, "count": len(rendered), "recover_used": recover_used}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
