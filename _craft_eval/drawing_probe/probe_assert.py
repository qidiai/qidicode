#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""断言环：对 out/ 里三个方案的 PNG 做**像素级字面锚 + OCR** 客观量化，不拿被测渲染器自己的常量当基准。

基准来源（全部外部）:
  1) fixture_manifest.json —— 合成语料的 DXF 坐标分区 + 期望文字（由 make_fixture.py 生成，与被测器无关）
  2) {方案}_empty.png —— 空图基线，量化各方案的"底噪"墨量
  3) {方案}_frame.png —— 只有外框的标定图，用它的墨迹包围盒把 DXF 坐标线性映射到像素（像素锚）
  4) RapidOCR（onnxruntime 中文模型）—— 判定"是不是真的成字"，方块/占位矩形一律算不命中

指标:
  * 非背景像素数（墨量）、墨量占比（对画布面积归一）
  * 分区墨量：几何区（LINE/LWPOLY/ARC/ELLIPSE/CIRCLE）、文字区、块与属性区
  * OCR 字符级召回率（对每个期望串分别算，中文/数字/字母同权）
  * 占位符特征：文字区里是否只出现 "Aa" 字面串（现状 Pillow 渲染器的签名）

用法: python probe_assert.py [--no-ocr]
产出: assert_results.json / crops/{方案}_{区}_{变体}.png
"""
from __future__ import annotations

import argparse
import io
import json
import os
import sys

import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "out")
CROPS = os.path.join(HERE, "crops")
METHODS = ["A", "B1", "B2", "C"]
METHOD_LABEL = {
    "A": "ezdxf matplotlib 后端",
    "B1": "cadcraft-cli L2（默认字体）",
    "B2": "cadcraft-cli L2 + FONTALT=SimHei（SHX→TTF）",
    "C": "现状 dwg_to_png.py Pillow 占位",
}
# 分区 -> 用哪个变体图做归因（该变体只含外框 + 这一类实体）
ZONE_VARIANT = {
    "Z_LINES": "geom", "Z_LWPOLY": "geom", "Z_ARC": "geom", "Z_ELLIPSE": "geom", "Z_CIRCLE": "geom",
    "Z_TEXT_CNHEI": "text_ttf", "Z_TEXT_CNFANG": "text_ttf", "Z_MTEXT_CN": "text_ttf",
    "Z_TEXT_SHX": "text_shx", "Z_MTEXT_SHX": "text_shx",
    "Z_BLOCK_FRAME": "block", "Z_SYMBOL": "block", "Z_ATTRIB_NAME": "block", "Z_ATTRIB_TITLE": "block",
    "Z_ATTRIB_NO": "block", "Z_ATTRIB_SCALE": "block", "Z_ATTRIB_DATE": "block",
    "Z_ATTRIB_SHX": "block",
}
INK_TAU = 40          # 与背景色的最大通道差阈值
OCR_MIN_H = 48        # 裁片文字太小时放大到该高度再 OCR


# ---------------------------------------------------------------- 像素工具
def load_rgb(path: str) -> np.ndarray:
    with Image.open(path) as im:
        return np.asarray(im.convert("RGB"), dtype=np.int16)


def modal_bg(arr: np.ndarray) -> np.ndarray:
    """空图基线的众数颜色 = 该方案的纸面色（A/C 白、B 深灰），不假设"非白"。"""
    flat = arr.reshape(-1, 3)
    keys = (flat[:, 0].astype(np.int64) << 16) + (flat[:, 1].astype(np.int64) << 8) + flat[:, 2]
    vals, counts = np.unique(keys, return_counts=True)
    k = vals[int(np.argmax(counts))]
    return np.array([(k >> 16) & 255, (k >> 8) & 255, k & 255], dtype=np.int16)


def ink_mask(arr: np.ndarray, bg: np.ndarray) -> np.ndarray:
    return np.max(np.abs(arr - bg[None, None, :]), axis=2) > INK_TAU


def ink_bbox(mask: np.ndarray):
    ys, xs = np.nonzero(mask)
    if len(xs) == 0:
        return None
    return int(xs.min()), int(ys.min()), int(xs.max()), int(ys.max())


def calibrate(arr: np.ndarray, bg: np.ndarray, rect_dxf):
    """逐图标定：用整图墨迹包围盒对齐图幅外框矩形（外部几何锚，与任何渲染器常量无关）。
    外框是图幅最外层的实体，因此它的墨迹包围盒 = DXF frame_rect 的投影。
    """
    mask = ink_mask(arr, bg)
    bb = ink_bbox(mask)
    if bb is None:
        return None
    px0, py0, px1, py1 = bb
    dx0, dy0, dx1, dy1 = rect_dxf
    return {"px0": px0, "py0": py0, "sx": (px1 - px0) / float(dx1 - dx0),
            "sy": (py1 - py0) / float(dy1 - dy0), "bbox": [px0, py0, px1, py1],
            "rect_dxf": list(rect_dxf), "img_wh": [int(mask.shape[1]), int(mask.shape[0])]}


def frame_edges_closed(arr: np.ndarray, bg: np.ndarray, aff: dict) -> dict:
    """图框四条边是否真的被画出来（现状渲染器不处理 LWPOLYLINE 的 closed 标志，会少一条边）。"""
    mask = ink_mask(arr, bg)
    px0, py0, px1, py1 = aff["bbox"]
    t = 3  # 允许 3px 线宽/抗锯齿容差
    H = py1 - py0
    W = px1 - px0

    def col_frac(x):
        seg = mask[py0:py1 + 1, max(0, x - t):x + t + 1]
        return float(seg.any(axis=1).mean()) if seg.size else 0.0

    def row_frac(y):
        seg = mask[max(0, y - t):y + t + 1, px0:px1 + 1]
        return float(seg.any(axis=0).mean()) if seg.size else 0.0

    return {"left": round(col_frac(px0), 3), "right": round(col_frac(px1), 3),
            "top": round(row_frac(py0), 3), "bottom": round(row_frac(py1), 3),
            "side_len_px": [W, H]}


def calibrate_file(png_path: str, rect_dxf, bg: np.ndarray):
    return calibrate(load_rgb(png_path), bg, rect_dxf)


def zone_crop(arr: np.ndarray, aff: dict, rect, pad_px: int = 3):
    """DXF 矩形 -> 像素裁片。DXF y 轴向上、图像 y 轴向下；aff['px0/py0'] 是外框左上角的像素位置。"""
    x0, y0, x1, y1 = rect
    dx0, dy0, dx1, dy1 = aff["rect_dxf"]
    px0 = int(round(aff["px0"] + (x0 - dx0) * aff["sx"])) - pad_px
    px1 = int(round(aff["px0"] + (x1 - dx0) * aff["sx"])) + pad_px
    py_top = int(round(aff["py0"] + (dy1 - y1) * aff["sy"])) - pad_px
    py_bot = int(round(aff["py0"] + (dy1 - y0) * aff["sy"])) + pad_px
    H, W = arr.shape[0], arr.shape[1]
    px0, px1 = max(0, px0), min(W, px1)
    py_top, py_bot = max(0, py_top), min(H, py_bot)
    if px1 <= px0 or py_bot <= py_top:
        return None
    return arr[py_top:py_bot, px0:px1]


# ---------------------------------------------------------------- OCR
class Ocr:
    def __init__(self, enabled: bool = True):
        self.engine = None
        self.error = None
        if not enabled:
            self.error = "NOT_MEASURED: --no-ocr"
            return
        try:
            from rapidocr_onnxruntime import RapidOCR

            self.engine = RapidOCR()
        except Exception as e:  # 依赖缺失就如实报
            self.error = f"NOT_MEASURED: OCR 依赖不可用 {type(e).__name__}: {e}"

    def read(self, crop: np.ndarray, bg: np.ndarray) -> list:
        """返回 [(box, text, score)]；自动把深色纸面反色成白底黑字。"""
        if self.engine is None:
            return []
        img = Image.fromarray(np.uint8(crop))
        if float(np.mean(bg)) < 128:            # 深色底 -> 反色
            img = Image.fromarray(255 - np.uint8(img))
        h = img.size[1]
        if h and h < OCR_MIN_H:                 # 小字放大，避免 OCR 漏检
            f = max(2, int(OCR_MIN_H / max(h, 1)) + 1)
            img = img.resize((img.size[0] * f, img.size[1] * f), Image.LANCZOS)
        try:
            res, _ = self.engine(np.asarray(img.convert("RGB")))
        except Exception as e:
            self.error = f"OCR 调用失败 {type(e).__name__}: {e}"
            return []
        return res or []


def char_recall(expected: str, ocr_text: str) -> float:
    """字符级召回：期望串里每个字符是否在 OCR 结果中出现（重复字符按出现次数计）。"""
    from collections import Counter

    e = Counter(ch for ch in expected if not ch.isspace())
    g = Counter(ch for ch in ocr_text if not ch.isspace())
    if not e:
        return 1.0
    hit = sum(min(c, g.get(ch, 0)) for ch, c in e.items())
    return hit / sum(e.values())


# ---------------------------------------------------------------- 主流程
def main():
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    ap = argparse.ArgumentParser()
    ap.add_argument("--no-ocr", action="store_true")
    args = ap.parse_args()

    man = json.load(io.open(os.path.join(HERE, "fixture_manifest.json"), encoding="utf-8"))
    zones, rect_frame = man["zones"], man["frame_rect"]
    os.makedirs(CROPS, exist_ok=True)
    ocr = Ocr(enabled=not args.no_ocr)

    results = {"meta": {"ink_tau": INK_TAU, "manifest_fonts": man["fonts_available"],
                        "ocr_error": ocr.error, "zone_variant": ZONE_VARIANT},
               "methods": {}}

    for m in METHODS:
        rec = {"label": METHOD_LABEL[m]}
        p_empty = os.path.join(OUT, f"{m}_empty.png")
        p_frame = os.path.join(OUT, f"{m}_frame.png")
        if not (os.path.isfile(p_empty) and os.path.isfile(p_frame)):
            rec["status"] = "NOT_MEASURED: 缺 empty/frame PNG（先跑 probe_render.py）"
            results["methods"][m] = rec
            continue
        arr_empty = load_rgb(p_empty)
        bg = modal_bg(arr_empty)
        rec["paper_bg_rgb"] = [int(v) for v in bg]
        rec["blank_ink_px"] = int(ink_mask(arr_empty, bg).sum())   # 空图底噪
        aff = calibrate_file(p_frame, rect_frame, bg)
        if aff is None:
            rec["status"] = "NOT_MEASURED: 标定失败（frame 图无墨迹）"
            results["methods"][m] = rec
            continue
        rec["calib"] = {"px_per_unit_x": round(aff["sx"], 4), "px_per_unit_y": round(aff["sy"], 4),
                        "frame_px_bbox": aff["bbox"], "canvas": aff["img_wh"]}
        rec["frame_edges_closed"] = frame_edges_closed(load_rgb(p_frame), bg, aff)

        cals, arrs = {}, {}
        for v in set(list(ZONE_VARIANT.values()) + ["full"]):
            pv = os.path.join(OUT, f"{m}_{v}.png")
            if not os.path.isfile(pv):
                continue
            a = load_rgb(pv)
            c = calibrate(a, bg, rect_frame)
            if c is None:
                continue
            cals[v], arrs[v] = aff, a          # 统一用 frame 变体的标定，偏差在下面如实报
            rec.setdefault("variant_bbox_check", {})[v] = {
                "ink_bbox": c["bbox"], "dev_vs_frame_px": [abs(x - y) for x, y in zip(c["bbox"], aff["bbox"])],
                "ink_px": int(ink_mask(a, bg).sum())}

        p_full = os.path.join(OUT, f"{m}_full.png")
        if os.path.isfile(p_full):
            arr_full = load_rgb(p_full)
            rec["full_ink_px"] = int(ink_mask(arr_full, bg).sum())
            rec["full_ink_ratio"] = round(rec["full_ink_px"] / float(arr_full.shape[0] * arr_full.shape[1]), 5)

        zones_out = {}
        texts_all = []
        for zn, zinfo in zones.items():
            variant = ZONE_VARIANT.get(zn, "full")
            png = os.path.join(OUT, f"{m}_{variant}.png")
            if not os.path.isfile(png):
                zones_out[zn] = {"status": "NOT_MEASURED: 缺 " + os.path.basename(png)}
                continue
            if variant not in cals:
                zones_out[zn] = {"status": "NOT_MEASURED: 该变体无法标定外框 " + os.path.basename(png)}
                continue
            crop = zone_crop(arrs[variant], cals[variant], zinfo["rect"])
            if crop is None:
                zones_out[zn] = {"status": "NOT_MEASURED: 裁片越界"}
                continue
            cm = ink_mask(crop.astype(np.int16), bg)
            z = {"variant": variant, "ink_px": int(cm.sum()),
                 "ink_ratio": round(float(cm.sum()) / float(cm.size), 5)}
            cpath = os.path.join(CROPS, f"{m}_{zn}_{variant}.png")
            Image.fromarray(np.uint8(crop)).save(cpath)
            z["crop"] = os.path.basename(cpath)
            exp = zinfo["expect"]
            if exp:
                hits = ocr.read(crop, bg)
                joined = " ".join(t for _, t, _ in hits)
                texts_all.append((m, zn, joined))
                z["ocr_raw"] = joined
                z["ocr_recall"] = {e: round(char_recall(e, joined), 3) for e in exp}
                z["ocr_recall_mean"] = round(
                    float(np.mean(list(z["ocr_recall"].values()))), 3) if z["ocr_recall"] else None
                z["placeholder_Aa"] = ("Aa" in joined)
            zones_out[zn] = z
        rec["zones"] = zones_out

        # 整图 OCR（外部字面锚，覆盖"块属性文字是否可见"）
        if ocr.engine is not None and os.path.isfile(p_full):
            full_hits = ocr.read(load_rgb(p_full).astype(np.uint8), bg)
            rec["ocr_full_text"] = " ".join(t for _, t, _ in full_hits)[:600]
        results["methods"][m] = rec

    with io.open(os.path.join(HERE, "assert_results.json"), "w", encoding="utf-8") as f:
        json.dump(results, f, ensure_ascii=False, indent=2)
    print("written assert_results.json")


if __name__ == "__main__":
    main()
