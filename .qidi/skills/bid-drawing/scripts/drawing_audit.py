#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""图纸审查工具 — 从 DOCX/PDF/DWG 提取图号、图层、实体，输出 JSON 报告。

用法:
    # 审查 DOCX（提取内嵌图片 + 文本中的图号引用）
    python drawing_audit.py 投标文件.docx --project "XX项目"
    # 审查 DWG/DXF 目录
    python drawing_audit.py drawings/ --check-figures
    # 审查 PDF（识别图纸页）
    python drawing_audit.py 招标文件.pdf

输出: JSON 报告（stdout），供代理直接读取。
依赖: pip install ezdxf pymupdf Pillow python-docx
"""

import argparse
import json
import re
import sys
from pathlib import Path
from typing import List, Dict, Any

try:
    from docx import Document
    from docx.opc.constants import RELATIONSHIP_TYPE as RT
except ImportError:
    print(json.dumps({"error": "python-docx 未安装"}, ensure_ascii=False))
    sys.exit(2)

try:
    import fitz as pymupdf  # PyMuPDF
except ImportError:
    pymupdf = None


# 图号匹配正则（国内工程通用格式）
FIGURE_PATTERNS = [
    re.compile(r"图\s*(\d{1,3})[-–_]\s*(\d{1,3})[:：]?\s*([^\n\r]{2,40})", re.U),  # 图4-1 xxx
    re.compile(r"图\s*(\d+)[^\d]{1,6}([^\n\r]{2,30})", re.U),  # 图001 xxx
    re.compile(r"[Ff][Ii][Gg]\s*(\d+)", re.I),  # Fig.4-1
]

# 专业前缀（图号常见分类）
PROFESSION_PREFIX = re.compile(
    r"(建施|结施|水施|电施|暖施|消施|智施|绿施|道施|雨污)"
)


def extract_figures_from_text(text: str) -> List[Dict[str, Any]]:
    """从文本提取图号引用。"""
    found = []
    for i, m in enumerate(FIGURE_PATTERNS):
        for j, g in enumerate(m.findall(text)):
            if isinstance(g, tuple):
                gid = str(g[0]) + ("-" + g[1] if len(g) > 1 and g[1] else "")
                name = g[-1].strip().replace("*", "") if j == 0 else ""
            else:
                gid = str(g)
                name = ""
            if gid and len(gid) <= 20:
                found.append({"fig_id": gid, "name_hint": name,
                              "source": "text_match"})
    return found


def extract_figures_from_docx(docx_path: str) -> List[Dict[str, Any]]:
    """扫描 DOCX 正文 + 标题的图号引用，以及内嵌图片统计。"""
    doc = Document(docx_path)
    full_text = "\n".join(p.text for p in doc.paragraphs)
    figs = extract_figures_from_text(full_text)
    images = 0
    for rel in doc.part.rels.values():
        from docx.parts.image import ImagePart
        if isinstance(rel.target_part, ImagePart):
            images += 1
    return {"figures_found": figs[:30], "figure_count": len(figs),
            "embedded_images": images, "total_paragraphs": len(doc.paragraphs)}


def scan_pymupdf(pdf_path: str) -> Dict[str, Any]:
    """用 PyMuPDF 扫描 PDF：识别图纸页（有图框/边界）。"""
    if not pymupdf:
        return {"error": "PyMuPDF 未安装"}
    doc = pymupdf.open(pdf_path)
    pages = []
    for i, page in enumerate(doc):
        rects = page.get_drawings()
        lines = page.get_text("dict").get("blocks", [])
        has_frame = any(
            r.get("rect", [0, 0, 0, 0])[2] - r.get("rect", [0, 0, 0, 0])[0] > 150
            for r in rects
        )
        text = page.get_text()
        fig_refs = extract_figures_from_text(text)
        # 图号密度高 + 有绘图矩形 = 可能是图纸页
        is_drawing_like = len(rects) > 30 or (has_frame and len(fig_refs) >= 1)
        pages.append({
            "page_num": i + 1,
            "rect_count": len(rects),
            "lines_count": sum(len(bl.get("lines", [])) for bl in lines),
            "figure_refs": [f["fig_id"] for f in fig_refs][:5],
            "is_drawing_like": is_drawing_like,
            "page_size": page.rect,
        })
    drawing_pages = [p for p in pages if p["is_drawing_like"]]
    return {
        "pdf_path": pdf_path,
        "total_pages": len(pages),
        "drawing_like_pages": len(drawing_pages),
        "drawing_page_numbers": [p["page_num"] for p in drawing_pages],
        "sample_figures": list({f for p in drawing_pages for f in p["figure_refs"]}),
    }


def scan_dxf(path: str) -> Dict[str, Any]:
    """解析 DXF，提取布局、图层、块、实体统计。"""
    try:
        import ezdxf
    except ImportError:
        return {"error": "ezdxf 未安装"}
    try:
        doc = ezdxf.readfile(path)
    except Exception as e:
        return {"error": f"DXF 打开失败: {e}"}
    ms = doc.modelspace()
    layouts = list(doc.layouts)
    block_names = {b.name for b in doc.blocks}
    layer_names = set(ms.query("*"))
    layer_names.update(l.name for l in layouts)

    # 实体计数
    counts = {}
    for entity in ms:
        dtype = entity.dxftype()
        counts[dtype] = counts.get(dtype, 0) + 1
    # 图层规范检查
    bad_layers = [ln for ln in layer_names
                  if not re.match(r"^[A-Z][\w\-]+$", ln) and ln not in ("0", "DEFPOINTS", "*MODEL_SPACE")]
    # 页面设置（Paper space）
    layouts_info = []
    for l in layouts:
        if l.is_paper_space:
            layouts_info.append({
                "name": l.name,
                "size_mm": (l.width * 25.4, l.height * 25.4),
                "plot_device": getattr(l, "plot_device", ""),
            })
    return {
        "file": path,
        "acad_ver": doc.header.get("$ACADVER", "?"),
        "layout_count": len(layouts),
        "paper_space_layouts": layouts_info,
        "block_count": len(block_names),
        "layer_count": len(layer_names),
        "entity_counts": counts,
        "layer_issues": bad_layers[:10],
        "blocks_sample": list(block_names)[:20],
    }


def main():
    ap = argparse.ArgumentParser(description="图纸审查工具")
    ap.add_argument("input", help="DOCX/PDF/DWG/DXF 文件或目录路径")
    ap.add_argument("--check-figures", action="store_true",
                    help="DWG/DXF 时做图块/图层规范化检查")
    ap.add_argument("--project", default="", help="项目名称（输出用）")
    args = ap.parse_args()

    inp = Path(args.input).resolve()
    report = {"project": args.project, "input": str(inp), "entries": []}

    if inp.is_dir():
        candidates = list(inp.rglob("*")) + list(inp.rglob("*.*"))
        candidates = [p for p in candidates if p.suffix.lower() in (
            ".dwg", ".dxf", ".pdf", ".docx", ".doc")]
        for p in candidates:
            report["entries"].append(_process_one(p, args.check_figures))
    else:
        report["entries"].append(_process_one(inp, args.check_figures))

    print(json.dumps(report, ensure_ascii=False, indent=2))


def _process_one(path: Path, check: bool) -> Dict[str, Any]:
    s = path.suffix.lower()
    if s in (".docx", ".doc"):
        d = extract_figures_from_docx(str(path))
        d["file"] = str(path)
        return d
    if s == ".pdf":
        d = scan_pymupdf(str(path))
        return d
    if s in (".dwg", ".dxf"):
        try:
            d = scan_dxf(str(path))
        except Exception as e:
            d = {"error": f"处理失败: {e}"}
        d["file"] = str(path)
        return d
    return {"error": f"不支持格式: {s}", "file": str(path)}


if __name__ == "__main__":
    main()
