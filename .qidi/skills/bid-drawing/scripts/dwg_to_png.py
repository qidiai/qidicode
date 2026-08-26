#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""DWG/DXF 图纸 → 高清 PNG 转换器（按布局分页输出）。

用法:
    python dwg_to_png.py <图纸.dwg> -o <输出目录> --dpi 300 [--layout "布局名"]
依赖: pip install ezdxf Pillow
"""

import argparse
import sys
from pathlib import Path

try:
    import ezdxf
except ImportError:
    print("ERROR: ezdxf 未安装。pip install ezdxf", file=sys.stderr)
    sys.exit(2)

try:
    from PIL import Image, ImageDraw
except ImportError:
    print("ERROR: Pillow 未安装。pip install Pillow", file=sys.stderr)
    sys.exit(2)

DPI = 300


def bbox(entities):
    """计算实体包围盒。"""
    xs, ys = [], []
    for e in entities:
        try:
            pts = list(e.vertices()) if hasattr(e, 'vertices') else []
            if not pts:
                # 简化：用 bounding_box 属性
                bb = getattr(e, 'bounding_box', None)
                if bb:
                    xs += [bb[0], bb[2]]
                    ys += [bb[1], bb[3]]
                    continue
                continue
            xs += [p[0] for p in pts]
            ys += [p[1] for p in pts]
        except Exception:
            continue
    if not xs:
        return 0, 0, 100, 100
    return min(xs), min(ys), max(xs), max(ys)


def render_layout_to_png(ms_or_layout, dpi=DPI, padding=5):
    """把 Layout/ModelSpace 渲染成白色背景 PNG。"""
    x0, y0, x1, y1 = bbox(ms_or_layout)
    w = max(x1 - x0, 1)
    h = max(y1 - y0, 1)
    # 换算为像素（假设图纸单位为 mm，72 DPI 相当于 1 inch = 25.4mm）
    scale = dpi / 25.4  # 每 mm 的像素数
    pw = int((w + padding * 2) * scale)
    ph = int((h + padding * 2) * scale)
    img = Image.new("RGB", (pw, ph), "white")
    draw = ImageDraw.Draw(img)

    def pt_in_px(x, y):
        return (int((x - x0 + padding) * scale), int((y1 - y + padding) * scale))

    for entity in ms_or_layout:
        dxtype = entity.dxftype()
        try:
            if dxtype in ("LINE", "LWPOLYLINE", "POLYLINE", "SPLINE"):
                pts = list(entity.vertices()) if dxtype in ("LWPOLYLINE", "POLYLINE") else \
                      [(entity.start[:2], entity.end[:2])]
                if dxtype == "LWPOLYLINE" and hasattr(entity, 'vertices'):
                    pts = list(entity.vertices())
                elif dxtype == "LINE":
                    pts = [entity.start[:2], entity.end[:2]]
                elif dxtype in ("SPLINE",):
                    # 样条线简化：取控制点
                    pts = list(entity.control_points()) if hasattr(entity, 'control_points') else []
                if len(pts) >= 2:
                    pxs = [pt_in_px(p[0], p[1]) for p in pts]
                    draw.line(pxs, fill="black", width=1)
            elif dxtype == "CIRCLE":
                center = entity.center
                r = entity.radius
                cx, cy = pt_in_px(center[0], center[1])
                rr = int(r * scale)
                draw.ellipse([cx - rr, cy - rr, cx + rr, cy + rr], outline="black", width=1)
            elif dxtype == "TEXT" or dxtype == "MTEXT":
                try:
                    txt = entity.text
                    anchor = entity.dxf.insert if dxtype == "TEXT" else entity.dxf.insert
                    px, py = pt_in_px(anchor[0], anchor[1])
                    # 简化：文字用矩形占位（真正渲染文字需 font 安装）
                    draw.rectangle([px, py, px + 60, py + 12], outline="gray")
                    draw.text((px + 2, py), "Aa", fill="gray", font=None)
                except Exception:
                    pass
            elif dxtype == "POINT":
                p = entity.dxf.position[:2]
                px, py = pt_in_px(p[0], p[1])
                draw.ellipse([px - 2, py - 2, px + 2, py + 2], fill="black")
            # ARC/ELLIPSE/INSERT 等后续可补
        except Exception:
            pass
    return img


def main():
    ap = argparse.ArgumentParser(description="DWG/DXF → PNG 转换器")
    ap.add_argument("input", help="DWG/DXF 文件路径")
    ap.add_argument("-o", "--output-dir", default="png_output")
    ap.add_argument("--dpi", type=int, default=DPI)
    ap.add_argument("--layout", default=None,
                    help="指定布局名（默认出全部 Paper space 布局）")
    args = ap.parse_args()

    inp = Path(args.input).resolve()
    out = Path(args.output_dir).resolve()
    out.mkdir(parents=True, exist_ok=True)

    doc = ezdxf.readfile(str(inp))
    layout_names = [l.name for l in doc.layouts]
    print(f"DWG 布局列表: {layout_names}")

    rendered = []
    if args.layout:
        if args.layout not in layout_names:
            print(f"布局 {args.layout} 不存在，退出", file=sys.stderr)
            sys.exit(1)
        targets = [doc.layouts[args.layout]]
    else:
        # 默认先出所有 Paper space 布局，没有时再出 Model space
        ps = [l for l in doc.layouts if l.is_paper_space]
        targets = ps if ps else [doc.modelspace()]

    for i, lay in enumerate(targets):
        try:
            img = render_layout_to_png(lay, dpi=args.dpi)
        except Exception as e:
            print(f"渲染失败 {lay.name}: {e}", file=sys.stderr)
            continue
        fname = f"{lay.name.replace(':', '_')}.png" if len(targets) > 1 else "图纸.png"
        fpath = out / fname
        img.save(fpath)
        rendered.append({"layout": lay.name, "file": str(fpath), "size": img.size})
        print(f"OK: {fpath}（{img.size[0]}×{img.size[1]}px）")

    if not rendered:
        print("未渲染出任何布局", file=sys.stderr)
        sys.exit(1)
    print(json.dumps({"rendered": rendered, "count": len(rendered)}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    import json
    main()
