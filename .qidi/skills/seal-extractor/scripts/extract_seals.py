# -*- coding: utf-8 -*-
"""
extract_seals.py -- 从 PDF / 图片中自动检测并抠取红色公章，输出透明背景 PNG。
用法:
    python extract_seals.py 输入文件 [输入文件...] [-o 输出目录]
                           [--dpi 300] [--no-purify] [--keep-fingerprints] [--min-size 120]
结果: 输出目录中生成 <源文件名>_p<页码>_章<N>.png 与 _预览_白底合成.png,
      stdout 打印 JSON 汇总 (seals / skipped / errors)。
依赖: pymupdf pillow numpy opencv-python
"""
import argparse
import json
import sys
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

try:
    import numpy as np
    import cv2
    from PIL import Image, ImageDraw, ImageFont
except ImportError as e:
    print("缺少依赖: %s" % e, file=sys.stderr)
    print("请先执行: pip install pymupdf pillow numpy opencv-python", file=sys.stderr)
    sys.exit(2)

IMG_EXTS = {".png", ".jpg", ".jpeg", ".tif", ".tiff", ".bmp", ".webp"}


def log(msg):
    print(msg, file=sys.stderr)


# ---------------- 渲染 / 读取 ----------------
def render_pages(path, dpi):
    """返回 [(page_no, rgb HxWx3 uint8), ...]，page_no 从 1 开始。"""
    ext = path.suffix.lower()
    if ext == ".pdf":
        try:
            import fitz
        except ImportError:
            log("处理 PDF 需要 PyMuPDF: pip install pymupdf")
            sys.exit(2)
        doc = fitz.open(str(path))
        zoom = dpi / 72.0
        mat = fitz.Matrix(zoom, zoom)
        pages = []
        for i, page in enumerate(doc):
            pix = page.get_pixmap(matrix=mat, alpha=False)
            arr = np.frombuffer(pix.samples, dtype=np.uint8).reshape(pix.height, pix.width, pix.n)
            if pix.n == 1:
                arr = np.repeat(arr, 3, axis=2)
            elif pix.n >= 3:
                arr = arr[:, :, :3]
            pages.append((i + 1, arr.copy()))
        doc.close()
        return pages
    if ext in IMG_EXTS:
        im = Image.open(str(path)).convert("RGB")
        return [(1, np.asarray(im).copy())]
    raise ValueError("不支持的文件类型: %s (支持 PDF 与常见图片)" % ext)


# ---------------- 红色掩膜 ----------------
def red_mask(rgb):
    r = rgb[:, :, 0].astype(np.int16)
    g = rgb[:, :, 1].astype(np.int16)
    b = rgb[:, :, 2].astype(np.int16)
    d = r - np.maximum(g, b)
    mask = ((d > 35) & (r > 90)).astype(np.uint8)
    return mask, d


# ---------------- 候选检测 ----------------
def split_wide(sub, x0, y0, w, h):
    """过宽(>620px)组件在原始掩膜上找 >=40px 零列段切分(处理并排双章)。"""
    if w <= 620:
        return [(x0, y0, w, h)]
    zero = sub.sum(axis=0) == 0
    cuts, run = [], 0
    for j in range(w):
        if zero[j]:
            run += 1
        else:
            if run >= 40:
                cuts.append((j - run, j))
            run = 0
    if run >= 40:
        cuts.append((w - run, w))
    if not cuts:
        return [(x0, y0, w, h)]
    spans, prev = [], 0
    for s, e in cuts:
        spans.append((prev, s))
        prev = e
    spans.append((prev, w))
    out = []
    for a, bnd in spans:
        if bnd - a < 120:
            continue
        seg = sub[:, a:bnd]
        ys = np.where(seg.sum(axis=1) > 0)[0]
        xs = np.where(seg.sum(axis=0) > 0)[0]
        if len(ys) == 0 or len(xs) == 0:
            continue
        out.append((x0 + a + int(xs[0]), y0 + int(ys[0]),
                    int(xs[-1] - xs[0] + 1), int(ys[-1] - ys[0] + 1)))
    return out or [(x0, y0, w, h)]


def find_candidates(mask, min_size):
    """膨胀合并 -> 连通域 -> 切分过宽组件 -> [bbox(x,y,w,h)]"""
    kernel = cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (25, 25))
    dil = cv2.dilate(mask, kernel, iterations=2)
    n, _, stats, _ = cv2.connectedComponentsWithStats(dil, 8)
    boxes = []
    for i in range(1, n):
        x, y, w, h = (int(v) for v in stats[i][:4])
        if w < min_size or h < min_size:
            continue
        boxes.extend(split_wide(mask[y:y + h, x:x + w], x, y, w, h))
    return boxes


# ---------------- 公章 vs 手印 分类 ----------------
def looks_like_seal(sub):
    """圆章/方章的边框四角有像素；手印(实心椭圆)四角为空。"""
    h, w = sub.shape
    fill = float(sub.sum()) / float(w * h)
    if w >= 270:
        return True
    if w < 200 or fill <= 0.30:
        return False
    cw, ch = max(1, int(w * 0.22)), max(1, int(h * 0.22))
    corners = [sub[:ch, :cw], sub[:ch, w - cw:], sub[h - ch:, :cw], sub[h - ch:, w - cw:]]
    occ = sum(1 for c in corners if int(c.sum()) >= 8)
    return occ >= 3


# ---------------- 单章抠取 ----------------
def extract_one(rgb, d, mask, box, purify):
    x, y, w, h = box
    sub_rgb = rgb[y:y + h, x:x + w].astype(np.float32)
    sub_d = d[y:y + h, x:x + w].astype(np.float32)
    sub_m = mask[y:y + h, x:x + w]
    alpha = np.clip((sub_d - 20.0) / 50.0, 0.0, 1.0)
    alpha[sub_m == 0] = 0.0
    # 去杂点: 去掉 <30px 的孤立小块
    binm = (alpha > 0).astype(np.uint8)
    n, lab, stats, _ = cv2.connectedComponentsWithStats(binm, 8)
    keep = np.zeros_like(binm)
    for i in range(1, n):
        if stats[i, cv2.CC_STAT_AREA] >= 30:
            keep[lab == i] = 1
    alpha[keep == 0] = 0.0
    # 章主色: d>120 像素的中位数
    strong = sub_d > 120
    if int(strong.sum()) < 50:
        strong = sub_d > 60
    if int(strong.sum()) < 10:
        seal_color = np.array([220.0, 40.0, 40.0], dtype=np.float32)
    else:
        seal_color = np.median(sub_rgb[strong], axis=0).astype(np.float32)
    out = sub_rgb.copy()
    if purify:
        # 黑字净化: 暗红像素按比例拉回章主色
        sr = max(float(seal_color[0]), 1.0)
        contam = np.clip((sr - sub_rgb[:, :, 0]) / (sr * 0.55), 0.0, 1.0)[..., None]
        out = sub_rgb * (1.0 - contam) + seal_color[None, None, :] * contam
    # 半透明边缘收敛到章主色, 消除白边/杂色
    semi = (alpha > 0) & (alpha < 1)
    out[semi] = seal_color
    a8 = (alpha * 255).round().astype(np.uint8)
    rgba = np.dstack([np.clip(out, 0, 255).round().astype(np.uint8), a8])
    # 裁掉全透明边, 留 8px
    ys = np.where(a8.max(axis=1) > 0)[0]
    xs = np.where(a8.max(axis=0) > 0)[0]
    y1, y2 = max(0, int(ys[0]) - 8), min(h, int(ys[-1]) + 9)
    x1, x2 = max(0, int(xs[0]) - 8), min(w, int(xs[-1]) + 9)
    return rgba[y1:y2, x1:x2]


# ---------------- 单文件处理 ----------------
def process_file(path, outdir, dpi, purify, keep_fp, min_size):
    seals, skipped = [], []
    for pno, rgb in render_pages(path, dpi):
        mask, d = red_mask(rgb)
        boxes = find_candidates(mask, min_size)
        boxes.sort(key=lambda b: (b[1] // 150, b[0]))  # 阅读顺序
        idx = 0
        for box in boxes:
            x, y, w, h = box
            if not keep_fp and not looks_like_seal(mask[y:y + h, x:x + w]):
                skipped.append({"source": path.name, "page": pno,
                                "bbox": [x, y, w, h],
                                "reason": "未通过公章判定(疑似手印/噪声)"})
                continue
            idx += 1
            rgba = extract_one(rgb, d, mask, box, purify)
            name = "%s_p%d_章%d.png" % (path.stem, pno, idx)
            Image.fromarray(rgba, "RGBA").save(str(outdir / name))
            seals.append({"source": path.name, "page": pno, "png": name,
                          "size": [int(rgba.shape[1]), int(rgba.shape[0])],
                          "path": str(outdir / name)})
            log("  [抠取] %s 第%d页 章%d -> %dx%d" % (path.name, pno, idx, rgba.shape[1], rgba.shape[0]))
    return seals, skipped


# ---------------- 预览图 ----------------
def load_font(size):
    for c in [r"C:\Windows\Fonts\msyh.ttc", r"C:\Windows\Fonts\simhei.ttf",
              r"C:\Windows\Fonts\simsun.ttc"]:
        if Path(c).exists():
            try:
                return ImageFont.truetype(c, size)
            except Exception:
                pass
    return ImageFont.load_default()


def make_preview(png_paths, out_path):
    cell_w, cell_h, cols = 380, 420, 3
    rows = (len(png_paths) + cols - 1) // cols
    sheet = Image.new("RGB", (cols * cell_w, rows * cell_h), (255, 255, 255))
    draw = ImageDraw.Draw(sheet)
    font = load_font(20)
    for i, p in enumerate(png_paths):
        im = Image.open(str(p)).convert("RGBA")
        im.thumbnail((cell_w - 40, cell_h - 90), Image.LANCZOS)
        cx, cy = (i % cols) * cell_w, (i // cols) * cell_h
        sheet.paste(im, (cx + (cell_w - im.width) // 2,
                         cy + 10 + (cell_h - 90 - im.height) // 2), im)
        label = p.name
        try:
            tw = draw.textlength(label, font=font)
        except Exception:
            tw = len(label) * 10
        draw.text((cx + max(4, (cell_w - tw) / 2), cy + cell_h - 55),
                  label, fill=(60, 60, 60), font=font)
    sheet.save(str(out_path))
    return str(out_path)


# ---------------- 入口 ----------------
def main():
    ap = argparse.ArgumentParser(description="从 PDF/图片抠取红色公章 -> 透明背景 PNG")
    ap.add_argument("inputs", nargs="+", help="输入文件(PDF 或图片), 可多个")
    ap.add_argument("-o", "--outdir", default=None,
                    help="输出目录(默认: 第一个输入所在目录\\公章PNG)")
    ap.add_argument("--dpi", type=int, default=300, help="PDF 渲染 DPI (默认 300)")
    ap.add_argument("--no-purify", action="store_true", help="关闭黑字净化")
    ap.add_argument("--keep-fingerprints", action="store_true",
                    help="不区分公章/手印, 全部候选都导出")
    ap.add_argument("--min-size", type=int, default=120,
                    help="候选最小宽/高像素 (默认 120, 对应 300dpi)")
    args = ap.parse_args()

    first = Path(args.inputs[0])
    outdir = Path(args.outdir) if args.outdir else first.resolve().parent / "公章PNG"
    outdir.mkdir(parents=True, exist_ok=True)
    log("输出目录: %s" % outdir)

    all_seals, all_skipped, errors = [], [], []
    for raw in args.inputs:
        p = Path(raw)
        if not p.exists():
            errors.append("文件不存在: %s" % raw)
            continue
        log("处理: %s" % p.name)
        try:
            seals, skipped = process_file(p, outdir, args.dpi,
                                          not args.no_purify,
                                          args.keep_fingerprints, args.min_size)
            all_seals.extend(seals)
            all_skipped.extend(skipped)
        except Exception as e:
            errors.append("%s: %s" % (p.name, e))
            log("  [错误] %s" % e)

    preview = None
    if all_seals:
        try:
            preview = make_preview([Path(s["path"]) for s in all_seals],
                                   outdir / "_预览_白底合成.png")
        except Exception as e:
            errors.append("预览图生成失败: %s" % e)

    summary = {"ok": len(errors) == 0, "outdir": str(outdir),
               "seal_count": len(all_seals), "seals": all_seals,
               "skipped_count": len(all_skipped), "skipped": all_skipped,
               "preview": preview, "errors": errors}
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    sys.exit(0 if (all_seals or not errors) else 1)


if __name__ == "__main__":
    main()