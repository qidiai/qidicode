#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Markdown → 投标 DOCX 转换器（自包含版，源自标书软件 doc_exporter 经验）。

特性:
- **加粗**标记解析为 bold run（直接塞入单 run 会显示星号——实测踩坑）
- 封面: 项目名26pt / 副标题"投标文件"18pt / 投标人 / 日期 / TOC自动目录域
- 三线表样式 + 长表行 cantSplit 防跨页断行
- 标题样式映射: # → 标题1, ## → 标题2, ### → 标题3
- 暗标模式: --anonymous 不插占位符/投标人信息

用法:
    python md2docx.py 方案.md --project "XX项目" --bidder "XX公司" -o 投标文件.docx
依赖: pip install python-docx
"""

import argparse
import re
import sys
from datetime import date
from pathlib import Path

try:
    from docx import Document
    from docx.shared import Pt, Cm
    from docx.enum.text import WD_ALIGN_PARAGRAPH
    from docx.enum.table import WD_TABLE_ALIGNMENT
    from docx.oxml.ns import qn
    from docx.oxml import OxmlElement
except ImportError:
    print("ERROR: python-docx 未安装。pip install python-docx", file=sys.stderr)
    sys.exit(2)


def set_font(run, cn_font="宋体", name_en="Times New Roman", size=None, bold=None):
    run.font.name = name_en
    run._element.rPr.rFonts.set(qn("w:eastAsia"), cn_font)
    if size:
        run.font.size = Pt(size)
    if bold is not None:
        run.font.bold = bold


def add_runs_with_bold(paragraph, text, size=12, cn_font="宋体"):
    """解析 **加粗** 标记拆分为多个 run——不能直接塞入单 run（星号会显示）。"""
    for i, seg in enumerate(re.split(r"(\*\*.*?\*\*)", text)):
        if not seg:
            continue
        if seg.startswith("**") and seg.endswith("**") and len(seg) > 4:
            run = paragraph.add_run(seg[2:-2])
            set_font(run, cn_font, size=size, bold=True)
        else:
            run = paragraph.add_run(seg)
            set_font(run, cn_font, size=size, bold=False)


def set_cell_border(cell, **kwargs):
    """设置单元格边框（三线表用）。"""
    tc = cell._tc
    tcPr = tc.get_or_add_tcPr()
    tcBorders = tcPr.find(qn("w:tcBorders"))
    if tcBorders is None:
        tcBorders = OxmlElement("w:tcBorders")
        tcPr.append(tcBorders)
    for edge in ("top", "bottom", "left", "right"):
        if edge in kwargs:
            el = tcBorders.find(qn(f"w:{edge}"))
            if el is None:
                el = OxmlElement(f"w:{edge}")
                tcBorders.append(el)
            for k, v in kwargs[edge].items():
                el.set(qn(f"w:{k}"), str(v))


def make_three_line_table(doc, header, rows):
    """三线表 + 行 cantSplit。"""
    table = doc.add_table(rows=len(rows) + 1, cols=len(header))
    table.alignment = WD_TABLE_ALIGNMENT.CENTER
    thin = {"sz": 4, "val": "single", "color": "000000"}
    thick = {"sz": 12, "val": "single", "color": "000000"}
    for j, text in enumerate(header):
        cell = table.rows[0].cells[j]
        cell.text = ""
        p = cell.paragraphs[0]
        p.alignment = WD_ALIGN_PARAGRAPH.CENTER
        add_runs_with_bold(p, str(text), size=10.5)
        set_cell_border(cell, top=thick, bottom=thin)
    for i, row in enumerate(rows):
        for j, text in enumerate(row):
            cell = table.rows[i + 1].cells[j]
            cell.text = ""
            p = cell.paragraphs[0]
            add_runs_with_bold(p, str(text), size=10.5)
            borders = {"bottom": thick} if i == len(rows) - 1 else {}
            if borders:
                set_cell_border(cell, **borders)
        # cantSplit 防跨页断行
        trPr = table.rows[i + 1]._tr.get_or_add_trPr()
        cant = OxmlElement("w:cantSplit")
        trPr.append(cant)
    return table


def add_toc_field(doc):
    """插入 TOC 自动目录域（打开后按 F9 更新）。"""
    p = doc.add_paragraph()
    run = p.add_run()
    fldChar = OxmlElement("w:fldChar")
    fldChar.set(qn("w:fldCharType"), "begin")
    instrText = OxmlElement("w:instrText")
    instrText.set(qn("xml:space"), "preserve")
    instrText.text = 'TOC \\o "1-3" \\h \\z \\u'
    fldChar2 = OxmlElement("w:fldChar")
    fldChar2.set(qn("w:fldCharType"), "end")
    run._r.append(fldChar)
    run._r.append(instrText)
    run._r.append(fldChar2)
    note = doc.add_paragraph()
    r = note.add_run("（目录域已插入，Word 中打开后全选→F9 更新页码）")
    set_font(r, size=9)
    r.font.color.rgb = None


def build_cover(doc, project, bidder, anonymous):
    if anonymous:
        return
    for _ in range(4):
        doc.add_paragraph()
    p = doc.add_paragraph()
    p.alignment = WD_ALIGN_PARAGRAPH.CENTER
    run = p.add_run(project)
    set_font(run, cn_font="黑体", size=26, bold=True)
    doc.add_paragraph()
    p = doc.add_paragraph()
    p.alignment = WD_ALIGN_PARAGRAPH.CENTER
    run = p.add_run("投 标 文 件")
    set_font(run, cn_font="黑体", size=18, bold=True)
    doc.add_paragraph()
    p = doc.add_paragraph()
    p.alignment = WD_ALIGN_PARAGRAPH.CENTER
    run = p.add_run(f"投标人：{bidder}")
    set_font(run, size=14)
    doc.add_paragraph()
    p = doc.add_paragraph()
    p.alignment = WD_ALIGN_PARAGRAPH.CENTER
    run = p.add_run(f"日期：{date.today().strftime('%Y年%m月%d日')}")
    set_font(run, size=14)
    doc.add_page_break()
    # 目录页
    h = doc.add_paragraph()
    h.alignment = WD_ALIGN_PARAGRAPH.CENTER
    run = h.add_run("目  录")
    set_font(run, cn_font="黑体", size=16, bold=True)
    add_toc_field(doc)
    doc.add_page_break()


def convert(md_path, out_path, project, bidder, anonymous):
    doc = Document()
    # 正文默认样式
    style = doc.styles["Normal"]
    style.font.name = "Times New Roman"
    style.font.size = Pt(12)
    style._element.rPr.rFonts.set(qn("w:eastAsia"), "宋体")

    build_cover(doc, project or "投标文件", bidder, anonymous)

    lines = Path(md_path).read_text(encoding="utf-8").splitlines()
    i = 0
    while i < len(lines):
        line = lines[i]
        stripped = line.strip()

        # 表格（| 分隔）
        if stripped.startswith("|") and i + 1 < len(lines) and \
                re.match(r"^\|[\s\-:|]+\|$", lines[i + 1].strip()):
            header = [c.strip() for c in stripped.strip("|").split("|")]
            rows = []
            i += 2
            while i < len(lines) and lines[i].strip().startswith("|"):
                rows.append([c.strip() for c in lines[i].strip().strip("|").split("|")])
                i += 1
            make_three_line_table(doc, header, rows)
            doc.add_paragraph()
            continue

        if stripped.startswith("### "):
            p = doc.add_paragraph(style="Heading 3")
            add_runs_with_bold(p, stripped[4:], size=12, cn_font="黑体")
        elif stripped.startswith("## "):
            p = doc.add_paragraph(style="Heading 2")
            add_runs_with_bold(p, stripped[3:], size=14, cn_font="黑体")
        elif stripped.startswith("# "):
            p = doc.add_paragraph(style="Heading 1")
            add_runs_with_bold(p, stripped[2:], size=16, cn_font="黑体")
        elif re.match(r"^\s*[-*]\s+", line):
            text = re.sub(r"^\s*[-*]\s+", "", line)
            p = doc.add_paragraph(style="List Bullet")
            add_runs_with_bold(p, text)
        elif re.match(r"^\s*\d+\.\s+", line):
            text = re.sub(r"^\s*\d+\.\s+", "", line)
            p = doc.add_paragraph(style="List Number")
            add_runs_with_bold(p, text)
        elif stripped:
            p = doc.add_paragraph()
            # 首行缩进2字符
            p.paragraph_format.first_line_indent = Cm(0.74)
            add_runs_with_bold(p, stripped)
        i += 1

    doc.save(str(out_path))
    print(f"OK: {out_path}")
    if not anonymous:
        print("提示: 占位符 [资质占位符：xxx] 等需线下确认后手动替换")


def main():
    ap = argparse.ArgumentParser(description="Markdown → 投标 DOCX")
    ap.add_argument("md", help="Markdown 文件路径")
    ap.add_argument("--project", default="", help="项目名称（封面用）")
    ap.add_argument("--bidder", default="", help="投标人名称（封面用）")
    ap.add_argument("-o", "--output", default=None, help="输出 DOCX 路径")
    ap.add_argument("--anonymous", action="store_true",
                    help="暗标模式：无封面投标人信息、无占位符")
    args = ap.parse_args()

    md = Path(args.md)
    if not md.exists():
        print(f"文件不存在: {md}", file=sys.stderr)
        sys.exit(1)
    out = Path(args.output) if args.output else md.with_suffix(".docx")
    convert(md, out, args.project, args.bidder, args.anonymous)


if __name__ == "__main__":
    main()
