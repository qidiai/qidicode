#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""PDF 图纸提取器 — 从招标文件/施工图 PDF 中识别并切片图纸页。

用法:
    python pdf_extract_drawings.py 招标文件.pdf --output-dir drawings/ --dpi 300

输出：
    drawings/
      page_001.png     （原图高保真 PNG）
      page_001_slice.pdf   （裁剪到图框区域的 PDF，如有边界识别）
      index.json     （提取到的图号列表与页码映射）
依赖: pip install pymupdf Pillow
"""

import argparse
import json
import re
import sys
from pathlib import Path

try:
    import fitz as pymupdf
except ImportError:
    print(json.dumps({"error": "PyMuPDF 未安装"}, ensure_ascii=False))
    sys.exit(2)

from PIL import Image
import io

FIGURE_RE = re.compile(r"图\s*(\d{1,3})[-–_]\s*(\d{1,3})[:：]?\s*([^\n\r]{2,40})", re.U)


def detect_figure_refs(page_text: str):
    matches = FIGURE_RE.findall(page_text)
    return [{"gid": f"{m[0]}-{m[1]}", "name": m[2].strip()} for m in matches]


def render_page_to_png(page, dpi=300):
    mat = pymupdf.Matrix(dpi / 72, dpi / 72)
    pm = page.get_pixmap(matrix=mat, alpha=False)
    img = Image.frombytes("RGB", [pm.width, pm.height], pm.samples)
    return img


def crop_to_drawing_bounds(page, img):
    """粗略检测图框：找页面中最长水平/垂直线段围成的矩形（简化版）。
    真正生产环境需要更复杂的图框识别逻辑。"""
    rects = page.get_drawings()
    if not rects:
        return 0, 0, img.width, img.height
    xs, ys = [], []
    for r in rects:
        rect = r.get("rect")
        if rect:
            xs += [rect[0], rect[2]]
            ys += [rect[1], rect[3]]
    if not xs:
        return 0, 0, img.width, img.height
    x0, x1 = int(min(xs)), int(max(xs))
    y0, y1 = int(min(ys)), int(max(ys))
    return x0, y0, x1 - x0, y1 - y0


def main():
    ap = argparse.ArgumentParser(description="PDF 图纸提取")
    ap.add_argument("pdf", help="PDF 文件路径")
    ap.add_argument("--output-dir", default="drawings")
    ap.add_argument("--dpi", type=int, default=300)
    args = ap.parse_args()

    p = Path(args.pdf)
    out = Path(args.output_dir).resolve()
    out.mkdir(parents=True, exist_ok=True)

    doc = pymupdf.open(str(p))
    index = {"source_pdf": str(p), "total_pages": len(doc), "drawings": []}
    total = 0

    for i, page in enumerate(doc):
        text = page.get_text()
        refs = detect_figure_refs(text)
        png = render_page_to_png(page, args.dpi)
        png_name = f"page_{i + 1:04d}.png"
        png_path = out / png_name
        png.save(png_path)
        total += 1
        index["drawings"].append({
            "page_num": i + 1,
            "png_file": png_name,
            "figure_refs": refs,
            "refs_count": len(refs),
        })
        if refs:
            print(f"P{i+1} → {png_path}（{len(refs)}处图号引用: {[r['gid'] for r in refs[:5]]}）")

    idx_path = out / "index.json"
    idx_path.write_text(json.dumps(index, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\n提取完成：{total}页，图号索引写入 {idx_path}")


if __name__ == "__main__":
    main()
