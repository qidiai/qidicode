#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""md2docx_selftest.py —— md2docx v3.0 验收环（A0/A1/A2/A3/A4 全断言）

用法:
    python .qidi/skills/bid-write/scripts/md2docx_selftest.py [--verbose] [--keep]

行为:
    1. 逐份跑 fixtures/ 下 11 份 unit + 1 份 integration golden（+ 运行时构造的
       绝对路径 / URL 下载 / --margin / --line-spacing / 暗标 / 三线表 用例）
    2. 对产物 docx 做 A0/A1/A2/A3/A4 断言 + 11 类泄漏标记残留扫描（正文+单元格+页眉页脚）
    3. 每个 FAIL 打一行（fixture / 断言 / 期望 / 实际），末尾汇总 + 页数估算表
    4. 有 FAIL → rc=1；全绿 → rc=0

纪律：凡改动 md2docx.py，提交前必跑本脚本，FAIL 即停（见 SKILL.md Step 8）。
"""

import argparse
import math
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
HERE_STR = str(HERE)
if HERE_STR not in sys.path:
    sys.path.insert(0, HERE_STR)

import md2docx
from md2docx import (LEAK_PATTERNS, MAX_IMG_H_CM, MAX_IMG_W_CM, PAGE_H_TW, PAGE_W_TW,
                     TWIP_PER_CM, scan_leaks)

from docx import Document
from docx.oxml.ns import qn

FIXTURES = HERE / "fixtures"
MEDIA = FIXTURES / "media"
BODY_STYLE = "BidBody"
CELL_STYLE = "BidTableCell"
FIG_STYLE = "BidFigure"
CAP_STYLE = "BidCaption"
SHAPE_COL = "000000"
EMU_PER_CM = 360000

# 每份 fixture 的期望（据规格书 B 节设计，非按实跑结果回填）
#   tables/figures   —— 块级表数 / 带图号图数（单元格图不计入 figures，单列 cell_images）
#   prose            —— 是否含正文段落（决定是否断言 BidBody 段落存在）
EXPECT = {
    "integration_golden": {"rc": 0, "tables": 4, "figures": 3, "cell_images": 0,
                           "min_warn": 0, "missing": 0, "prose": True},
    "u01_leak_syntax": {"rc": 0, "tables": 1, "figures": 1, "cell_images": 0,
                        "min_warn": 0, "missing": 0, "prose": True},
    "u02_suspicious_table": {"rc": 0, "tables": 1, "figures": 0, "cell_images": 0,
                             "min_warn": 3, "missing": 0, "prose": True},
    "u03_escaped_pipe": {"rc": 0, "tables": 2, "figures": 0, "cell_images": 0,
                         "min_warn": 0, "missing": 0, "prose": True},
    "u04_grid_table": {"rc": 0, "tables": 2, "figures": 0, "cell_images": 0,
                       "min_warn": 0, "missing": 0, "prose": False},
    "u05_consecutive_tables": {"rc": 0, "tables": 6, "figures": 0, "cell_images": 0,
                               "min_warn": 0, "missing": 0, "prose": True},
    "u06_table_image": {"rc": 0, "tables": 2, "figures": 0, "cell_images": 2,
                        "min_warn": 1, "missing": 0, "prose": False},
    "u07_images": {"rc": 1, "tables": 0, "figures": 3, "cell_images": 0,
                   "min_warn": 0, "missing": 2, "prose": True},
    "u08_tall_image": {"rc": 0, "tables": 0, "figures": 1, "cell_images": 0,
                       "min_warn": 0, "missing": 0, "prose": True},
    "u09_small_image": {"rc": 0, "tables": 0, "figures": 1, "cell_images": 0,
                        "min_warn": 0, "missing": 0, "prose": True},
    "u10_fullwidth_entity_tasklist": {"rc": 0, "tables": 1, "figures": 0, "cell_images": 0,
                                      "min_warn": 0, "missing": 0, "prose": True},
    "u11_empty_lines": {"rc": 0, "tables": 0, "figures": 0, "cell_images": 0,
                        "min_warn": 0, "missing": 0, "prose": True},
}

DEFAULT_PG_MAR = {"top": 1440, "bottom": 1440, "left": 1797, "right": 1797}


# =============================================================================
# 断言记录
# =============================================================================

class Report(object):
    def __init__(self, verbose=False):
        self.verbose = verbose
        self.total = 0
        self.fails = []
        self.pages = []

    def check(self, fixture, assertion, ok, expected="", actual=""):
        self.total += 1
        if ok:
            if self.verbose:
                print(f"  PASS {fixture} | {assertion}")
            return True
        self.fails.append((fixture, assertion, expected, actual))
        print(f"FAIL {fixture} | {assertion} | 期望={expected} | 实际={actual}")
        return False

    def eq(self, fixture, assertion, actual, expected):
        return self.check(fixture, assertion, actual == expected, expected, actual)

    def le(self, fixture, assertion, actual, expected):
        return self.check(fixture, assertion, actual <= expected, f"<={expected}", actual)

    def ge(self, fixture, assertion, actual, expected):
        return self.check(fixture, assertion, actual >= expected, f">={expected}", actual)

    def between(self, fixture, assertion, actual, lo, hi):
        return self.check(fixture, assertion, lo <= actual <= hi, f"[{lo},{hi}]", actual)


# =============================================================================
# docx 读取工具
# =============================================================================

def _style_id(el):
    ps = el.find(qn("w:pPr"))
    if ps is None:
        return ""
    pstyle = ps.find(qn("w:pStyle"))
    return pstyle.get(qn("w:val")) if pstyle is not None else ""


def para_style_name(doc, p):
    try:
        return p.style.name
    except Exception:
        return ""


def iter_all_paragraphs(doc):
    """正文 + 表格单元格（递归）段落。"""
    for p in doc.paragraphs:
        yield p
    for table in doc.tables:
        for row in table.rows:
            for cell in row.cells:
                for p in cell.paragraphs:
                    yield p
                for sub in cell.tables:
                    for r2 in sub.rows:
                        for c2 in r2.cells:
                            for p2 in c2.paragraphs:
                                yield p2


def all_text(doc, include_headers=True):
    chunks = [p.text for p in iter_all_paragraphs(doc)]
    if include_headers:
        for sec in doc.sections:
            for part in (sec.header, sec.footer, sec.first_page_header,
                         sec.first_page_footer):
                if part is None:
                    continue
                try:
                    chunks.extend(p.text for p in part.paragraphs)
                    for t in part.tables:
                        for row in t.rows:
                            for cell in row.cells:
                                chunks.extend(p.text for p in cell.paragraphs)
                except Exception:
                    pass
    return "\n".join(chunks)


def ppr_of(el):
    return el.find(qn("w:pPr"))


def spacing_of(el):
    ppr = ppr_of(el)
    return None if ppr is None else ppr.find(qn("w:spacing"))


def ind_of(el):
    ppr = ppr_of(el)
    return None if ppr is None else ppr.find(qn("w:ind"))


def drawings_of(doc):
    out = []
    for dr in doc.element.body.iter(qn("w:drawing")):
        ext = dr.find(".//" + qn("wp:extent"))
        if ext is None:
            out.append((0.0, 0.0))
            continue
        out.append((int(ext.get("cx")) / EMU_PER_CM, int(ext.get("cy")) / EMU_PER_CM))
    return out


def style_el(doc, name):
    try:
        return doc.styles[name].element
    except KeyError:
        return None


def rpr_fonts(el):
    rpr = el.find(qn("w:rPr"))
    if rpr is None:
        return None
    return rpr.find(qn("w:rFonts"))


def el_color(el):
    rpr = el.find(qn("w:rPr"))
    if rpr is None:
        return None
    return rpr.find(qn("w:color"))


def el_sz(el):
    rpr = el.find(qn("w:rPr"))
    if rpr is None:
        return None
    return rpr.find(qn("w:sz"))


# =============================================================================
# A0 页面与文档属性
# =============================================================================

def check_a0(rep, name, doc, path, margins=DEFAULT_PG_MAR):
    sect = doc.sections[0]._sectPr
    pgsz = sect.find(qn("w:pgSz"))
    w = int(pgsz.get(qn("w:w"))) if pgsz is not None else 0
    h = int(pgsz.get(qn("w:h"))) if pgsz is not None else 0
    rep.eq(name, "A0 pgSz.w", w, PAGE_W_TW)
    rep.eq(name, "A0 pgSz.h", h, PAGE_H_TW)
    pgmar = sect.find(qn("w:pgMar"))
    for edge, want in margins.items():
        got = int(pgmar.get(qn(f"w:{edge}"))) if pgmar is not None else -1
        rep.eq(name, f"A0 pgMar.{edge}", got, want)
    # 页面几何一致性：版心宽 = 页宽 - 左右边距
    if pgmar is not None:
        content = PAGE_W_TW - int(pgmar.get(qn("w:left"))) - int(pgmar.get(qn("w:right")))
        rep.eq(name, "A0 版心宽(twip)", content,
               int(round((PAGE_W_TW / TWIP_PER_CM - margins["left"] / TWIP_PER_CM
                          - margins["right"] / TWIP_PER_CM) * TWIP_PER_CM)))


def check_anonymous(rep, path):
    name = "u01_leak_source→--anonymous"
    with zipfile.ZipFile(path) as z:
        core = z.read("docProps/core.xml").decode("utf-8")
        app = z.read("docProps/app.xml").decode("utf-8")
    rep.check(name, "暗标 core.xml 无 python-docx 痕迹", "python-docx" not in core,
              "0 次命中", "命中" if "python-docx" in core else "0 次命中")
    rep.check(name, "暗标 app.xml 无 python-docx 痕迹", "python-docx" not in app,
              "0 次命中", "命中" if "python-docx" in app else "0 次命中")
    creator = re.search(r"<dc:creator>(.*?)</dc:creator>", core, re.S)
    rep.eq(name, "暗标 dc:creator 为空", (creator.group(1) if creator else "").strip(), "")
    lastby = re.search(r"<cp:lastModifiedBy>(.*?)</cp:lastModifiedBy>", core, re.S)
    rep.eq(name, "暗标 cp:lastModifiedBy 为空",
           (lastby.group(1) if lastby else "").strip(), "")
    desc = re.search(r"<dc:description>(.*?)</dc:description>", core, re.S)
    rep.eq(name, "暗标 dc:description 为空", (desc.group(1) if desc else "").strip(), "")
    company = re.search(r"<Company>(.*?)</Company>", app, re.S)
    rep.eq(name, "暗标 app Company 置空", (company.group(1) if company else "").strip(), "")


# =============================================================================
# A1 语法层：泄漏扫描 / 结构
# =============================================================================

def check_leaks(rep, name, doc, src_text):
    text = all_text(doc)
    hits = scan_leaks(text)
    allowed_pipes = src_text.count("\\|")          # \| 属内容，渲染为字面竖线不算泄漏
    others = [(k, v) for k, v in hits if k != "pipe"]
    pipes = [v for k, v in hits if k == "pipe"]
    rep.check(name, "A1 泄漏标记残留(除转义竖线)==0", not others,
              "0 处", str(others[:5]) if others else "0 处")
    rep.check(name, "A1 竖线残留==源码转义竖线数", len(pipes) == allowed_pipes,
              f"{allowed_pipes} 处", f"{len(pipes)} 处")
    # 全角语法字符不得残留
    rep.check(name, "A1 全角语法字符(＃＊｜)无残留",
              not re.search(r"[＃＊｜]", text), "0 处",
              str(re.findall(r"[＃＊｜]", text)[:5]))


def check_a1_structure(rep, name, doc):
    # 任务列表/删除线/代码/链接形态
    text = all_text(doc)
    if "☐" in text or "☑" in text:
        rep.check(name, "A1 任务列表渲染为 ☐/☑", True, "-", "-")
    styles = [para_style_name(doc, p) for p in doc.paragraphs]
    headings = [s for s in styles if s.startswith("Heading")]
    rep.check(name, "A1 标题使用 Heading 1-3 样式",
              all(s in ("Heading 1", "Heading 2", "Heading 3") for s in headings),
              "仅 Heading 1-3", str(sorted(set(headings))))


# =============================================================================
# A2 表格
# =============================================================================

def check_a2(rep, name, doc, table_style="grid"):
    content_tw = None
    sect = doc.sections[0]._sectPr
    pgmar = sect.find(qn("w:pgMar"))
    if pgmar is not None:
        content_tw = PAGE_W_TW - int(pgmar.get(qn("w:left"))) - int(pgmar.get(qn("w:right")))
    for idx, table in enumerate(doc.tables, 1):
        tag = f"{name}#表{idx}"
        tbl = table._tbl
        tblPr = tbl.tblPr
        # 三件套
        layout = tblPr.find(qn("w:tblLayout"))
        rep.eq(tag, "A2 tblLayout=fixed",
               layout.get(qn("w:type")) if layout is not None else None, "fixed")
        tblW = tblPr.find(qn("w:tblW"))
        rep.eq(tag, "A2 tblW type=dxa",
               tblW.get(qn("w:type")) if tblW is not None else None, "dxa")
        grid = tbl.find(qn("w:tblGrid"))
        ncols = len(table.columns)
        ncol_grid = len(grid.findall(qn("w:gridCol"))) if grid is not None else 0
        rep.eq(tag, "A2 tblGrid 列数", ncol_grid, ncols)
        widths = [int(gc.get(qn("w:w"))) for gc in grid.findall(qn("w:gridCol"))] \
            if grid is not None else []
        total = sum(widths)
        rep.check(tag, "A2 列宽总和≈版心宽", content_tw is not None and abs(total - content_tw) <= 2,
                  content_tw, total)
        # 每行 tcW 与 gridCol 一致
        tc_mismatch = 0
        for row in table.rows:
            for j, cell in enumerate(row.cells):
                tcW = cell._tc.tcPr.find(qn("w:tcW")) if cell._tc.tcPr is not None else None
                if tcW is None or j >= len(widths) or int(tcW.get(qn("w:w"))) != widths[j]:
                    tc_mismatch += 1
        rep.eq(tag, "A2 单元格 tcW 与 tblGrid 一致(不符数)", tc_mismatch, 0)
        # 边框
        borders = tblPr.find(qn("w:tblBorders"))
        if table_style == "grid":
            ok = True
            detail = []
            for edge in ("top", "left", "bottom", "right", "insideH", "insideV"):
                el = borders.find(qn(f"w:{edge}")) if borders is not None else None
                good = (el is not None and el.get(qn("w:val")) == "single"
                        and el.get(qn("w:sz")) == "4")
                detail.append(f"{edge}={None if el is None else el.get(qn('w:val'))}")
                ok = ok and good
            rep.check(tag, "A2 全框线六边 single sz4", ok, "六边 single sz4", ",".join(detail))
        else:
            rep.check(tag, "A2 三线表表级无框线",
                      borders is not None and borders.find(qn("w:top")).get(qn("w:val")) == "none",
                      "none", borders.find(qn("w:top")).get(qn("w:val")) if borders is not None else None)
            hdr = table.rows[0]
            top = hdr.cells[0]._tc.tcPr.find(qn("w:tcBorders")).find(qn("w:top"))
            bot = hdr.cells[0]._tc.tcPr.find(qn("w:tcBorders")).find(qn("w:bottom"))
            rep.eq(tag, "A2 三线表表头顶 sz12", top.get(qn("w:sz")), "12")
            rep.eq(tag, "A2 三线表表头底 sz4", bot.get(qn("w:sz")), "4")
            lastc = table.rows[-1].cells[0]._tc.tcPr.find(qn("w:tcBorders"))
            rep.eq(tag, "A2 三线表末行底 sz12",
                   lastc.find(qn("w:bottom")).get(qn("w:sz")) if lastc is not None else None, "12")
        # 表头重复 + 不可断行
        tr0 = tbl.findall(qn("w:tr"))[0]
        trPr0 = tr0.find(qn("w:trPr"))
        rep.check(tag, "A2 表头行 w:tblHeader", trPr0 is not None and trPr0.find(qn("w:tblHeader")) is not None,
                  "<w:tblHeader/>", "缺失" if trPr0 is None else [c.tag.split('}')[1] for c in trPr0])
        nosplit = 0
        for tr in tbl.findall(qn("w:tr")):
            trPr = tr.find(qn("w:trPr"))
            if trPr is None or trPr.find(qn("w:cantSplit")) is None:
                nosplit += 1
        rep.eq(tag, "A2 全行 cantSplit(缺行数)", nosplit, 0)
        # 单元格样式
        bad = 0
        for row in table.rows:
            for cell in row.cells:
                for p in cell.paragraphs:
                    if para_style_name(doc, p) != CELL_STYLE and p.text.strip():
                        bad += 1
        rep.eq(tag, "A2 单元格段落样式==BidTableCell(不符数)", bad, 0)
        # 单元格内图片：不得超出所在列宽
        over = []
        for row in table.rows:
            for j, cell in enumerate(row.cells):
                tcW = cell._tc.tcPr.find(qn("w:tcW")) if cell._tc.tcPr is not None else None
                col_cm = int(tcW.get(qn("w:w"))) / TWIP_PER_CM if tcW is not None else 0
                for dr in cell._tc.iter(qn("w:drawing")):
                    ext = dr.find(".//" + qn("wp:extent"))
                    if ext is None:
                        continue
                    img_cm = int(ext.get("cx")) / EMU_PER_CM
                    if col_cm and img_cm > col_cm + 0.01:
                        over.append(round(img_cm, 2))
        rep.check(tag, "A2 单元格内图片不超列宽", not over, "≤列宽", str(over[:3]))


# =============================================================================
# A3 图片
# =============================================================================

def check_a3(rep, name, doc):
    imgs = drawings_of(doc)
    for i, (w, h) in enumerate(imgs, 1):
        rep.le(name, f"A3 图{i} 宽≤{MAX_IMG_W_CM}cm", round(w, 3), MAX_IMG_W_CM + 0.01)
        rep.le(name, f"A3 图{i} 高≤{MAX_IMG_H_CM}cm", round(h, 3), MAX_IMG_H_CM + 0.01)
    # 图段/题注段不得 exact 行距
    fig_pars = [p for p in doc.paragraphs if para_style_name(doc, p) == FIG_STYLE]
    cap_pars = [p for p in doc.paragraphs if para_style_name(doc, p) == CAP_STYLE]
    for label, pars in (("图段", fig_pars), ("题注段", cap_pars)):
        bad = []
        for p in pars:
            sp = spacing_of(p._p)
            rule = sp.get(qn("w:lineRule")) if sp is not None else None
            if rule == "exact":
                bad.append(p.text[:20])
        rep.check(name, f"A3 {label}无 exact 行距", not bad, "lineRule≠exact", str(bad[:3]))
    # 图段样式级定义也不得 exact
    st = style_el(doc, FIG_STYLE)
    sp = spacing_of(st) if st is not None else None
    rep.check(name, "A3 BidFigure 样式级 lineRule≠exact",
              sp is not None and sp.get(qn("w:lineRule")) != "exact",
              "auto", None if sp is None else sp.get(qn("w:lineRule")))
    # 题注格式「图X-Y」
    bad_cap = [p.text for p in cap_pars if not re.match(r"^图\d+-\d+", p.text.strip())]
    rep.check(name, "A3 题注格式 图{章号}-{序号}", not bad_cap, "图N-M", str(bad_cap[:3]))
    # 图号连续性（同章内递增）
    seen = {}
    ok = True
    for p in cap_pars:
        m = re.match(r"^图(\d+)-(\d+)", p.text.strip())
        if not m:
            continue
        chap, seq = int(m.group(1)), int(m.group(2))
        want = seen.get(chap, 0) + 1
        seen[chap] = seq
        ok = ok and seq == want
    rep.check(name, "A3 图号章内连续", ok, "1,2,3…", str(seen))


# =============================================================================
# A4 字体与样式族
# =============================================================================

def check_a4(rep, name, doc, line_spacing=22, expect_prose=True):
    line_tw = line_spacing * 20
    st = style_el(doc, BODY_STYLE)
    rep.check(name, "A4 存在 BidBody 样式", st is not None, "存在", "缺失")
    if st is not None:
        sp = spacing_of(st)
        rep.eq(name, "A4 BidBody w:line", sp.get(qn("w:line")) if sp is not None else None, str(line_tw))
        rep.eq(name, "A4 BidBody lineRule", sp.get(qn("w:lineRule")) if sp is not None else None, "exact")
        rep.eq(name, "A4 BidBody before", sp.get(qn("w:before")) if sp is not None else None, "0")
        rep.eq(name, "A4 BidBody after", sp.get(qn("w:after")) if sp is not None else None, "0")
        ind = ind_of(st)
        rep.eq(name, "A4 BidBody firstLineChars",
               ind.get(qn("w:firstLineChars")) if ind is not None else None, "200")
    # 正文段落风格：只允许 BidBody / 围栏代码用的 BidTableCell
    stray = [f"{para_style_name(doc, p)}:{p.text[:12]}" for p in doc.paragraphs
             if p.text.strip() and para_style_name(doc, p) not in
             (BODY_STYLE, CELL_STYLE, FIG_STYLE, CAP_STYLE,
              "Heading 1", "Heading 2", "Heading 3", "Normal")]
    rep.check(name, "A4 正文段落样式仅 BidBody/BidTableCell", not stray,
              "无其他样式", str(stray[:3]))
    body_pars = [p for p in doc.paragraphs if para_style_name(doc, p) == BODY_STYLE]
    if expect_prose:
        rep.ge(name, "A4 正文段落数(BidBody)", len(body_pars), 1)
    # 单元格样式
    cs = style_el(doc, CELL_STYLE)
    rep.check(name, "A4 存在 BidTableCell 样式", cs is not None, "存在", "缺失")
    if cs is not None:
        sz = el_sz(cs)
        rep.eq(name, "A4 BidTableCell 字号(半磅)", sz.get(qn("w:val")) if sz is not None else None, "21")
        sp = spacing_of(cs)
        rep.eq(name, "A4 BidTableCell 行距 single",
               sp.get(qn("w:lineRule")) if sp is not None else None, "auto")
        ind = ind_of(cs)
        rep.eq(name, "A4 BidTableCell 无首行缩进",
               ind.get(qn("w:firstLineChars")) if ind is not None else None, "0")
    # 图/题注样式
    fs = style_el(doc, FIG_STYLE)
    rep.check(name, "A4 存在 BidFigure 样式", fs is not None, "存在", "缺失")
    caps = style_el(doc, CAP_STYLE)
    rep.check(name, "A4 存在 BidCaption 样式", caps is not None, "存在", "缺失")
    if caps is not None:
        fonts = rpr_fonts(caps)
        rep.eq(name, "A4 BidCaption eastAsia 黑体",
               fonts.get(qn("w:eastAsia")) if fonts is not None else None, "黑体")
        sz = el_sz(caps)
        rep.eq(name, "A4 BidCaption 字号(半磅)", sz.get(qn("w:val")) if sz is not None else None, "21")
    # Normal
    normal = style_el(doc, "Normal")
    fonts = rpr_fonts(normal) if normal is not None else None
    rep.eq(name, "A4 Normal ascii", fonts.get(qn("w:ascii")) if fonts is not None else None,
           "Times New Roman")
    body_font = fonts.get(qn("w:eastAsia")) if fonts is not None else None
    rep.check(name, "A4 Normal eastAsia 为检测到的正文字体",
              body_font in ("仿宋", "仿宋_GB2312", "宋体"), "仿宋/仿宋_GB2312/宋体", body_font)
    rep.eq(name, "A4 Normal 字号(半磅)", el_sz(normal).get(qn("w:val")) if normal is not None and el_sz(normal) is not None else None, "24")
    rep.check(name, "A4 Normal 不设行距", spacing_of(normal) is None if normal is not None else False,
              "无 w:spacing", "有 w:spacing" if normal is not None and spacing_of(normal) is not None else "无")
    ind = ind_of(normal) if normal is not None else None
    rep.check(name, "A4 Normal 不设缩进", ind is None, "无 w:ind", "有 w:ind")
    # 默认字体真实存在
    present = md2docx.font_file_present("仿宋")
    rep.check(name, "A4 默认字体 仿宋 在本机存在", present is True, True, present)
    # Heading 1-3
    for lvl, half in ((1, "32"), (2, "28"), (3, "24")):
        h = style_el(doc, f"Heading {lvl}")
        rep.check(name, f"A4 存在 Heading {lvl}", h is not None, "存在", "缺失")
        if h is None:
            continue
        fonts = rpr_fonts(h)
        rep.eq(name, f"A4 Heading {lvl} eastAsia", fonts.get(qn("w:eastAsia")) if fonts is not None else None, "黑体")
        col = el_color(h)
        rep.eq(name, f"A4 Heading {lvl} 颜色", col.get(qn("w:val")) if col is not None else None, SHAPE_COL)
        rep.check(name, f"A4 Heading {lvl} 无主题色属性",
                  col is not None and col.get(qn("w:themeColor")) is None,
                  "无 themeColor", None if col is None else col.get(qn("w:themeColor")))
        rep.eq(name, f"A4 Heading {lvl} 字号(半磅)",
               el_sz(h).get(qn("w:val")) if el_sz(h) is not None else None, half)


# =============================================================================
# 页数估算
# =============================================================================

def estimate_pages(doc, name, content_tw):
    """行数×行高估算（12pt 仿宋，版心宽 14.66cm，正文 22 磅 exact）。"""
    usable_tw = PAGE_H_TW - DEFAULT_PG_MAR["top"] - DEFAULT_PG_MAR["bottom"]
    line_pt = 22.0
    char_w_cm = 12.0 / 72 * 2.54                       # 12pt 全角字宽
    chars_per_line = max(1, int((content_tw / TWIP_PER_CM) / char_w_cm))
    lines_per_page = max(1, int((usable_tw / 20.0) / line_pt))
    body_lines = 0
    body_chars = 0
    for p in doc.paragraphs:
        style = para_style_name(doc, p)
        text = p.text
        if style == FIG_STYLE:
            body_lines += 20                           # 单图约 20 行高
            continue
        if not text.strip():
            body_lines += 1 if style in ("Normal",) else 0
            continue
        n = max(1, math.ceil(len(text) / float(chars_per_line)))
        body_lines += n
        body_chars += len(text)
    table_lines = 0
    for t in doc.tables:
        for row in t.rows:
            longest = max((len(c.text) for c in row.cells), default=0)
            table_lines += max(1, math.ceil(longest / float(max(1, chars_per_line / 2))))
    total_lines = body_lines + table_lines
    pages = int(math.ceil(total_lines / float(lines_per_page)))
    return {"name": name, "chars": body_chars, "body_lines": body_lines,
            "table_lines": table_lines, "lines_per_page": lines_per_page,
            "est_pages": pages}


# =============================================================================
# 主流程
# =============================================================================

RUNTIME_MD = {
    # 运行时构造：绝对路径图片（A3 支持绝对路径）
    "rt_abs_path": (
        "# 绝对路径插图\n\n正文。\n\n![绝对路径图](__ABS_IMG__)\n",
        {"tables": 0, "figures": 1, "missing": 0},
    ),
    # 运行时构造：URL 图片 + --download-images（file:// 走下载通道，免联网）
    "rt_url_download": (
        "# URL 图片下载\n\n正文。\n\n![下载图](__FILE_URL__)\n",
        {"tables": 0, "figures": 1, "missing": 0},
    ),
}


def build_runtime_fixtures(tmpdir):
    abs_img = (MEDIA / "fig-small.png").resolve()
    file_url = abs_img.as_uri()
    out = {}
    for key, (tpl, expected) in RUNTIME_MD.items():
        text = tpl.replace("__ABS_IMG__", str(abs_img)).replace("__FILE_URL__", file_url)
        p = tmpdir / f"{key}.md"
        p.write_text(text, encoding="utf-8")
        out[key] = (p, expected)
    return out


def run_selftest(verbose=False, keep=False):
    rep = Report(verbose=verbose)
    tmpdir = Path(tempfile.mkdtemp(prefix="md2docx_selftest_"))
    outdir = tmpdir / "out"
    outdir.mkdir(parents=True, exist_ok=True)

    print("=" * 78)
    print(f"md2docx {md2docx.__version__} 自测 —— fixtures: {FIXTURES}")
    print(f"临时输出: {outdir}")
    print("=" * 78)

    # 前置：fixtures 完整性
    unit = sorted(FIXTURES.glob("u*.md"))
    rep.eq("fixtures", "unit md 份数", len(unit), 11)
    rep.check("fixtures", "integration golden 存在",
              (FIXTURES / "integration_golden.md").exists(), True, False)
    media_names = {"fig-normal.png", "图 中文 (A).png", "fig-small.png"}
    have = {p.name for p in MEDIA.glob("*.png")} if MEDIA.is_dir() else set()
    rep.check("fixtures", "图片 fixture 三态齐备", media_names <= have, sorted(media_names), sorted(have))

    # 扫描器自检（防"检查器静默失效"）：源码本身必须命中；11 类标记全覆盖
    src01 = (FIXTURES / "u01_leak_syntax.md").read_text(encoding="utf-8")
    hits01 = scan_leaks(src01)
    rep.ge("扫描器灵敏度", "u01 源码命中类别数", len({k for k, _ in hits01}), 9)
    rep.ge("扫描器灵敏度", "u01 源码命中次数", len(hits01), 30)
    union = set()
    for md in sorted(FIXTURES.glob("*.md")):
        union |= {k for k, _ in scan_leaks(md.read_text(encoding="utf-8"))}
    rep.eq("扫描器灵敏度", "11 类泄漏标记逐一被 fixture 覆盖",
           sorted(union), sorted(k for k, _ in LEAK_PATTERNS))

    results = {}
    all_md = sorted(FIXTURES.glob("*.md"))
    for md in all_md:
        name = md.stem
        exp = EXPECT.get(name)
        if exp is None:
            continue
        src = md.read_text(encoding="utf-8")
        out = outdir / f"{name}.docx"
        res = md2docx.convert(md, out, cover=False)
        results[name] = (out, res, src)
        print(f"[run] {name}: rc={res.rc} {res.summary()}")
        rep.eq(name, "rc", res.rc, exp["rc"])
        rep.eq(name, "表数", res.tables, exp["tables"])
        rep.eq(name, "图数", res.figures, exp["figures"])
        rep.ge(name, "告警数", len(res.warnings), exp["min_warn"])
        rep.eq(name, "缺图数", len(res.missing_images), exp["missing"])

    # integration golden：带封面 + 目录（A0/B 要求）
    golden = FIXTURES / "integration_golden.md"
    gold_out = outdir / "integration_golden_cover.docx"
    gold_res = md2docx.convert(golden, gold_out, project="XX新区综合服务楼施工总承包",
                               bidder="XX建设工程有限公司", cover=True)
    rep.eq("integration_golden(cover)", "rc", gold_res.rc, 0)
    gdoc = Document(str(gold_out))
    gtext = all_text(gdoc)
    rep.check("integration_golden(cover)", "封面含项目名",
              "XX新区综合服务楼施工总承包" in gtext, True, False)
    rep.check("integration_golden(cover)", "封面含投标人",
              "XX建设工程有限公司" in gtext, True, False)
    rep.check("integration_golden(cover)", "目录域 TOC 已插入",
              'TOC \\o "1-3"' in gdoc.element.xml, True, False)
    rep.ge("integration_golden(cover)", "三级标题齐备",
           sum(1 for p in gdoc.paragraphs if para_style_name(gdoc, p) == "Heading 3"), 1)
    # 跨页长表：行数 ≥ 40
    long_rows = max((len(t.rows) for t in gdoc.tables), default=0)
    rep.ge("integration_golden(cover)", "跨页长表行数", long_rows, 40)

    # 逐份 docx 断言
    for name, (out, res, src) in results.items():
        doc = Document(str(out))
        exp = EXPECT[name]
        content_tw = int(round((PAGE_W_TW / TWIP_PER_CM - 3.17 - 3.17) * TWIP_PER_CM))
        check_a0(rep, name, doc, out)
        check_leaks(rep, name, doc, src)
        check_a1_structure(rep, name, doc)
        check_a2(rep, name, doc)
        check_a3(rep, name, doc)
        check_a4(rep, name, doc, expect_prose=exp["prose"])
        rep.eq(name, "图片数==带号图+单元格图",
               len(drawings_of(doc)), res.figures + exp["cell_images"])
        rep.pages.append(estimate_pages(doc, name, content_tw))

    # —— A3 专项：竖长图 / 小图尺寸 ——
    tall = Document(str(results["u08_tall_image"][0]))
    tw, th = drawings_of(tall)[0]
    rep.between("u08_tall_image", "A3 竖长图 高≈20cm", round(th, 2), 19.5, 20.01)
    rep.le("u08_tall_image", "A3 竖长图 宽<=14.6cm", round(tw, 2), MAX_IMG_W_CM)
    small = Document(str(results["u09_small_image"][0]))
    sw, sh = drawings_of(small)[0]
    rep.between("u09_small_image", "A3 小图不放大(宽≈2.12cm)", round(sw, 2), 2.0, 2.3)

    # —— A3 专项：缺图 rc=1 + 清单含路径 ——
    u07_res = results["u07_images"][1]
    joined = " ".join(u07_res.missing_images)
    rep.eq("u07_images", "A3 缺图 rc", u07_res.rc, 1)
    rep.check("u07_images", "缺图清单含缺图路径", "not-exist-anywhere.png" in joined,
              "含 not-exist-anywhere.png", joined[:80])
    rep.check("u07_images", "缺图清单含 URL 项", "remote-not-downloaded.png" in joined,
              "含 remote-not-downloaded.png", joined[:80])
    rep.check("u07_images", "缺图占位文本出现在正文", "【缺图：" in all_text(Document(str(results["u07_images"][0]))),
              "【缺图：…】", "无")

    # —— A2 专项：u05 连续表不吞并（每个表行数：表一/表三/表四 各 3 行，其余 2 行） ——
    u05 = Document(str(results["u05_consecutive_tables"][0]))
    row_counts = [len(t.rows) for t in u05.tables]
    rep.eq("u05_consecutive_tables", "A2 连续表分表后行数", row_counts, [3, 2, 3, 2, 2, 2])
    rep.check("u05_consecutive_tables", "A2 无 '---' 泄漏进行", "---" not in all_text(u05),
              "无 ---", "有 ---")

    # —— A2 专项：u06 单元格图片 ——
    u06 = Document(str(results["u06_table_image"][0]))
    cell_draw = sum(1 for _ in u06.element.body.iter(qn("w:drawing")))
    rep.eq("u06_table_image", "A2 单元格内图片插入数", cell_draw, 2)
    rep.check("u06_table_image", "不支持形态降级为【图：…】", "【图：丙图】" in all_text(u06),
              "【图：丙图】", "无")

    # —— A2 专项：u02 疑似表格告警与清洗 ——
    u02 = Document(str(results["u02_suspicious_table"][0]))
    u02_text = all_text(u02)
    rep.check("u02_suspicious_table", "A2 疑似表格告警文案", any(
        md2docx.SUSPICIOUS_TABLE_MSG in w for w in results["u02_suspicious_table"][1].warnings),
        md2docx.SUSPICIOUS_TABLE_MSG, "无告警")
    rep.check("u02_suspicious_table", "A2 分隔行已删除", "---" not in u02_text, "无 ---", "有 ---")
    rep.check("u02_suspicious_table", "A2 表头数据保留可读文本",
              "序号" in u02_text and "甲项目" in u02_text, "含 序号/甲项目", u02_text[:40])

    # —— A0 专项：暗标 ——
    anon_out = outdir / "u01_anonymous.docx"
    md2docx.convert(FIXTURES / "u01_leak_syntax.md", anon_out, anonymous=True, cover=True)
    check_anonymous(rep, anon_out)

    # —— A0 专项：--margin 覆盖 ——
    margin_out = outdir / "u04_margin.docx"
    md2docx.convert(FIXTURES / "u04_grid_table.md", margin_out, cover=False,
                    margins=(2.0, 2.0, 2.0, 2.0))
    mdoc = Document(str(margin_out))
    check_a0(rep, "u04--margin 2,2,2,2", mdoc, margin_out,
             {"top": int(2 * TWIP_PER_CM), "bottom": int(2 * TWIP_PER_CM),
              "left": int(2 * TWIP_PER_CM), "right": int(2 * TWIP_PER_CM)})
    # 版心宽随之变化 → 列宽总和应等于新值
    new_content = PAGE_W_TW - 2 * int(2 * TWIP_PER_CM)
    widths = [int(gc.get(qn("w:w"))) for gc in mdoc.tables[0]._tbl.find(qn("w:tblGrid"))]
    rep.eq("u04--margin 2,2,2,2", "A2 列宽总跟随版心宽", sum(widths), new_content)

    # —— A2 专项：three-line ——
    tl_out = outdir / "u04_three_line.docx"
    md2docx.convert(FIXTURES / "u04_grid_table.md", tl_out, cover=False,
                    table_style="three-line")
    tldoc = Document(str(tl_out))
    check_a2(rep, "u04--table-style three-line", tldoc, table_style="three-line")

    # —— A4 专项：--line-spacing 28 ——
    ls_out = outdir / "u01_line28.docx"
    md2docx.convert(FIXTURES / "u01_leak_syntax.md", ls_out, cover=False, line_spacing=28)
    lsdoc = Document(str(ls_out))
    sp = spacing_of(style_el(lsdoc, BODY_STYLE))
    rep.eq("u01--line-spacing 28", "A4 BidBody w:line=560",
           sp.get(qn("w:line")) if sp is not None else None, "560")

    # —— 运行时 fixture：绝对路径 / URL 下载 ——
    rt = build_runtime_fixtures(tmpdir)
    for key, (path, expected) in rt.items():
        out = outdir / f"{key}.docx"
        res = md2docx.convert(path, out, cover=False, download_images=True)
        rep.eq(key, "rc", res.rc, 0)
        rep.eq(key, "图数", res.figures, expected["figures"])
        rep.eq(key, "缺图数", len(res.missing_images), expected["missing"])
        doc = Document(str(out))
        check_a3(rep, key, doc)

    # —— 旧 CLI 兼容（子进程实跑） ——
    script = HERE / "md2docx.py"
    cli_out = outdir / "cli_compat.docx"
    cmd = [sys.executable, str(script), str(golden), "--project", "CLI兼容项目",
           "--bidder", "CLI兼容公司", "-o", str(cli_out)]
    proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8",
                          errors="replace")
    rep.eq("旧CLI兼容", "rc", proc.returncode, 0)
    rep.check("旧CLI兼容", "输出 docx 存在", cli_out.exists(), True, False)
    if cli_out.exists():
        cdoc = Document(str(cli_out))
        check_a0(rep, "旧CLI兼容", cdoc, cli_out)
        check_a4(rep, "旧CLI兼容", cdoc)
        rep.check("旧CLI兼容", "封面含项目名", "CLI兼容项目" in all_text(cdoc), True, False)
    # 缺图 fixture 走 CLI → rc=1
    proc2 = subprocess.run([sys.executable, str(script), str(FIXTURES / "u07_images.md"),
                            "-o", str(outdir / "cli_missing.docx")],
                           capture_output=True, text=True, encoding="utf-8", errors="replace")
    rep.eq("旧CLI兼容", "缺图 rc=1", proc2.returncode, 1)
    rep.check("旧CLI兼容", "缺图清单打印到 stderr", "not-exist-anywhere.png" in proc2.stderr,
              "含路径", (proc2.stderr or "")[-80:])

    # —— 页数估算表 ——
    print()
    print("-" * 78)
    print("页数估算（行数×行高，参考值；准绳为 WPS COM ComputeStatistics(2)）")
    print("-" * 78)
    print(f"{'fixture':32s} {'字数':>7s} {'正文行':>7s} {'表行':>6s} {'每页行':>7s} {'估页':>5s}")
    for row in sorted(rep.pages, key=lambda r: -r["est_pages"]):
        print(f"{row['name']:32s} {row['chars']:7d} {row['body_lines']:7d} "
              f"{row['table_lines']:6d} {row['lines_per_page']:7d} {row['est_pages']:5d}")
    print("-" * 78)

    # —— 汇总 ——
    print()
    if rep.fails:
        print(f"结果: FAIL —— {len(rep.fails)} 项失败 / 共 {rep.total} 项断言")
        for fixture, assertion, expected, actual in rep.fails:
            print(f"  ✗ {fixture} | {assertion} | 期望={expected} | 实际={actual}")
        rc = 1
    else:
        print(f"结果: PASS —— {rep.total} 项断言全绿")
        rc = 0

    if keep:
        print(f"输出保留在: {outdir}")
    else:
        shutil.rmtree(tmpdir, ignore_errors=True)
    return rc


def main(argv=None):
    ap = argparse.ArgumentParser(description="md2docx v3.0 自测（验收环）")
    ap.add_argument("--verbose", action="store_true", help="打印每条 PASS")
    ap.add_argument("--keep", action="store_true", help="保留临时输出目录")
    args = ap.parse_args(argv)
    return run_selftest(verbose=args.verbose, keep=args.keep)


if __name__ == "__main__":
    sys.exit(main())
