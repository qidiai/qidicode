#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""生成用于图纸渲染方案对比的**合成 DXF 语料**（全部写在 %TEMP%\\wb_drawing_probe\\，不碰任何仓库）。

产出（幂等，重复运行覆盖）:
  fx_empty.dxf     -- 无任何实体（空图基线，用来量化各渲染器的"底噪"墨量）
  fx_frame.dxf      -- 只有外框（像素仿射标定的锚点图，也是"无文字"对照）
  fx_geom.dxf       -- 外框 + LINE/LWPOLYLINE/ARC/ELLIPSE/CIRCLE（纯几何）
  fx_text_ttf.dxf   -- 外框 + TEXT/MTEXT（TTF 样式：黑体 simhei.ttf、仿宋 simfang.ttf）
  fx_text_shx.dxf   -- 外框 + TEXT/MTEXT（SHX 形字体样式 gben.shx + 大字体 chineset.shx，本机无该文件）
  fx_block.dxf      -- 外框 + 含 ATTRIB 的标题栏块 INSERT + 嵌套符号块 INSERT
  fx_full.dxf       -- 以上全部（主测图，耗时/内存以它为准）
  fixture_manifest.json -- 每个测试区的 DXF 坐标矩形 + 期望文字（断言脚本的外部基准，非渲染器自比）

图纸单位按 mm 处理，外框 400x320。所有实体颜色 ACI 7（白/黑随背景翻转），线型 continuous，
避免把"线型未加载"这类噪声混进对比。
"""
from __future__ import annotations

import io
import json
import os
import sys

import ezdxf

HERE = os.path.dirname(os.path.abspath(__file__))
SHEET_W, SHEET_H = 400.0, 320.0

# 各测试区（DXF 坐标，左下/右上）+ 期望出现的文字（OCR 外部基准）
ZONES = {
    "Z_LINES":        {"rect": [15, 220, 130, 310], "expect": []},
    "Z_LWPOLY":       {"rect": [135, 220, 255, 310], "expect": []},
    "Z_ARC":          {"rect": [260, 262, 322, 312], "expect": []},
    "Z_ELLIPSE":      {"rect": [326, 262, 388, 302], "expect": []},
    "Z_CIRCLE":       {"rect": [262, 220, 300, 258], "expect": []},
    "Z_TEXT_CNHEI":   {"rect": [18, 178, 195, 210], "expect": ["一层平面图"]},
    "Z_TEXT_CNFANG":  {"rect": [18, 152, 195, 176], "expect": ["仿宋标题栏"]},
    "Z_MTEXT_CN":     {"rect": [200, 150, 392, 214], "expect": ["标题栏", "±0.000", "比例"]},
    "Z_TEXT_SHX":     {"rect": [18, 122, 195, 148], "expect": ["GSHX-01", "轴线"]},
    "Z_MTEXT_SHX":    {"rect": [200, 104, 392, 150], "expect": ["SHX", "覆盖测试"]},
    "Z_SYMBOL":       {"rect": [305, 218, 388, 258], "expect": []},
    "Z_BLOCK_FRAME":  {"rect": [12, 12, 388, 92], "expect": []},
    "Z_ATTRIB_NAME":  {"rect": [20, 66, 190, 90], "expect": ["某某建筑工程有限公司"]},
    "Z_ATTRIB_TITLE": {"rect": [196, 66, 300, 90], "expect": ["给排水管网"]},
    "Z_ATTRIB_NO":    {"rect": [300, 66, 386, 90], "expect": ["建施-01"]},
    "Z_ATTRIB_SCALE": {"rect": [20, 40, 130, 62], "expect": ["1:500"]},
    "Z_ATTRIB_DATE":  {"rect": [136, 40, 250, 62], "expect": ["2026-10-22"]},
    "Z_ATTRIB_SHX":   {"rect": [256, 40, 386, 62], "expect": ["审 核"]},
}


def _new_doc():
    doc = ezdxf.new("R2010", setup=True)
    doc.header["$INSUNITS"] = 4  # mm
    doc.header["$EXTMIN"] = (0, 0, 0)
    doc.header["$EXTMAX"] = (SHEET_W, SHEET_H, 0)
    # 说明：ezdxf 不允许删除最后一个图纸空间布局；真实 ODA 转换产物同样带一个空 Layout1，
    # 保留 Model + Layout1(空) 正好复现生产脚本 dwg_to_png.py 默认调用的崩溃路径。
    for lay in ("GEO", "TXT", "TBL"):
        if not doc.layers.has_entry(lay):
            doc.layers.add(lay, color=7)
    # TTF 样式（本机 C:\\Windows\\Fonts 下真实存在）
    doc.styles.new("CNHEI", dxfattribs={"font": "simhei.ttf"})
    doc.styles.new("CNFANG", dxfattribs={"font": "simfang.ttf"})
    # SHX 形字体样式 + 大字体（本机不存在这两个 .shx 文件 -> 用来量"形字体缺失时的回退行为"）
    doc.styles.new("SHX1", dxfattribs={"font": "gben.shx", "bigfont": "chineset.shx"})
    return doc


def add_frame(msp):
    msp.add_lwpolyline(
        [(0, 0), (SHEET_W, 0), (SHEET_W, SHEET_H), (0, SHEET_H)],
        close=True, dxfattribs={"layer": "GEO"},
    )
    msp.add_lwpolyline(
        [(6, 6), (SHEET_W - 6, 6), (SHEET_W - 6, SHEET_H - 6), (6, SHEET_H - 6)],
        close=True, dxfattribs={"layer": "GEO"},
    )


def add_geom(msp):
    for i, (p1, p2) in enumerate(
        [((20, 225), (125, 305)), ((20, 305), (125, 225)), ((20, 265), (125, 265)), ((72, 225), (72, 305))]
    ):
        msp.add_line(p1, p2, dxfattribs={"layer": "GEO"})
    # LWPOLYLINE 带凸度（含弧段）+ 普通多段线
    msp.add_lwpolyline(
        [(140, 300), (250, 300), (250, 230), (140, 230)],
        close=True, dxfattribs={"layer": "GEO"},
    )
    msp.add_lwpolyline(
        [(145, 290, 0, 0, 0.5), (200, 290, 0, 0, 0.5), (245, 240, 0, 0, 0), (145, 240, 0, 0, 0)],
        close=True, dxfattribs={"layer": "GEO"},
    )
    msp.add_arc((291, 287), 25, 20, 300, dxfattribs={"layer": "GEO"})
    msp.add_arc((291, 287), 15, 200, 480, dxfattribs={"layer": "GEO"})
    msp.add_ellipse(
        (357, 282), major_axis=(30, 0, 0), ratio=0.55, dxfattribs={"layer": "GEO"}
    )
    msp.add_circle((281, 239), 14, dxfattribs={"layer": "GEO"})


def add_text_ttf(msp):
    msp.add_text(
        "一层平面图", dxfattribs={"style": "CNHEI", "insert": (20, 180), "height": 18, "layer": "TXT"}
    )
    msp.add_text(
        "仿宋标题栏", dxfattribs={"style": "CNFANG", "insert": (20, 154), "height": 16, "layer": "TXT"}
    )
    msp.add_mtext(
        "标题栏 ±0.000\\P比例 1:100 层高 3.6m",
        dxfattribs={"style": "CNHEI", "insert": (205, 210), "char_height": 11,
                    "layer": "TXT", "rect_width": 185},
    )


def add_text_shx(msp):
    msp.add_text(
        "GSHX-01 轴线", dxfattribs={"style": "SHX1", "insert": (20, 124), "height": 16, "layer": "TXT"}
    )
    msp.add_mtext(
        "SHX 覆盖测试 3.500",
        dxfattribs={"style": "SHX1", "insert": (205, 145), "char_height": 13,
                    "layer": "TXT", "rect_width": 120},
    )


def build_blocks(doc):
    sym = doc.blocks.new("SYMBOL")
    sym.add_circle((0, 0), 6, dxfattribs={"color": 7})
    sym.add_line((-6, 0), (6, 0), dxfattribs={"color": 7})
    sym.add_line((0, -6), (0, 6), dxfattribs={"color": 7})

    tbl = doc.blocks.new("TITLEBLOCK")
    w, h = 376.0, 80.0
    tbl.add_lwpolyline([(0, 0), (w, 0), (w, h), (0, h)], close=True)
    for y in (24, 48):
        tbl.add_line((0, y), (w, y))
    for x in (184, 288, 130, 250):
        tbl.add_line((x, 0), (x, 48))
    attdefs = [
        ("NAME", "某某建筑工程有限公司", "CNFANG", 10, (10, 60)),
        ("TITLE", "给排水管网", "CNHEI", 10, (196, 60)),
        ("NO", "建施-01", "SHX1", 10, (300, 60)),
        ("SCALE", "1:500", "CNHEI", 9, (10, 34)),
        ("DATE", "2026-10-22", "CNHEI", 9, (136, 34)),
        ("CHECK", "审 核", "SHX1", 9, (256, 34)),
    ]
    for tag, prompt, style, ht, ins in attdefs:
        tbl.add_attdef(tag, ins, dxfattribs={"height": ht, "style": style})
    # 嵌套块：标题栏里放一个符号
    tbl.add_blockref("SYMBOL", (360, 16))
    return tbl


def add_block(msp, tbl):
    msp.add_blockref("SYMBOL", (322, 240), dxfattribs={"layer": "GEO"})
    msp.add_blockref("SYMBOL", (362, 232), dxfattribs={"layer": "GEO"})
    ref = msp.add_blockref("TITLEBLOCK", (12, 12), dxfattribs={"layer": "TBL"})
    ref.add_auto_attribs(
        {
            "NAME": "某某建筑工程有限公司",
            "TITLE": "给排水管网",
            "NO": "建施-01",
            "SCALE": "1:500",
            "DATE": "2026-10-22",
            "CHECK": "审 核",
        }
    )


def build(which: str, path: str):
    doc = _new_doc()
    msp = doc.modelspace()
    if which != "empty":
        add_frame(msp)
    if which in ("geom", "full"):
        add_geom(msp)
    if which in ("text_ttf", "full"):
        add_text_ttf(msp)
    if which in ("text_shx", "full"):
        add_text_shx(msp)
    if which in ("block", "full"):
        tbl = build_blocks(doc)
        add_block(msp, tbl)
    doc.saveas(path)
    return path


def main():
    variants = ["empty", "frame", "geom", "text_ttf", "text_shx", "block", "full"]
    written = []
    for v in variants:
        p = os.path.join(HERE, f"fx_{v}.dxf")
        build(v, p)
        written.append(p)
    manifest = {
        "sheet": [SHEET_W, SHEET_H],
        "frame_rect": [0, 0, SHEET_W, SHEET_H],
        "units": "mm",
        "fonts_available": {
            "simhei.ttf": os.path.exists(r"C:\Windows\Fonts\simhei.ttf"),
            "simfang.ttf": os.path.exists(r"C:\Windows\Fonts\simfang.ttf"),
            "simsun.ttc": os.path.exists(r"C:\Windows\Fonts\simsun.ttc"),
            "gben.shx": os.path.exists(r"C:\Windows\Fonts\gben.shx"),
            "chineset.shx": os.path.exists(r"C:\Windows\Fonts\chineset.shx"),
        },
        "zones": ZONES,
        "variants": variants,
    }
    with io.open(os.path.join(HERE, "fixture_manifest.json"), "w", encoding="utf-8") as f:
        json.dump(manifest, f, ensure_ascii=False, indent=2)
    print("\n".join(written))


if __name__ == "__main__":
    main()
