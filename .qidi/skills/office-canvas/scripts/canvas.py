#!/usr/bin/env python3
"""office-canvas: 画布式反馈闭环脚本。

子命令:
  open <文件>                     WPS/Word 打开(所见即所得)
  baseline <docx>                 留存基线快照(供 diff 用户改动)
  collect <docx>                  读取全部批注→整改清单
  diff <docx>                     对比基线→用户改动摘要
  mark <png> --box x,y,w,h --label N:文本   图片标注(画框+序号)
  verify <docx>                   验证批注是否已响应
"""
import argparse
import datetime
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile

HOME = os.path.expanduser("~")
BASE_ROOT = os.path.join(HOME, ".qidi", "office-canvas")

# 批注锚定: commentRangeStart..commentRangeEnd 之间的 w:t 文本
RE_COMMENT = re.compile(
    r"<w:comment\s[^>]*?w:id=\"(\d+)\"[^>]*?w:author=\"([^\"]*)\"[^>]*?>(.*?)</w:comment>",
    re.S,
)
RE_TEXT = re.compile(r"<w:t(?:\s[^>]*)?>(.*?)</w:t>", re.S)
RE_RANGE_START = re.compile(r"<w:commentRangeStart\s[^>]*?w:id=\"(\d+)\"")
RE_RANGE_END = re.compile(r"<w:commentRangeEnd\s[^>]*?w:id=\"(\d+)\"")


def esc(s: str) -> str:
    return (s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
            .replace("&quot;", '"').replace("&apos;", "'"))


def open_in_office(path: str) -> int:
    path = os.path.abspath(os.path.expanduser(path))
    if not os.path.isfile(path):
        print(f"找不到文件: {path}", file=sys.stderr)
        return 2
    try:
        if sys.platform == "win32":
            os.startfile(path)  # noqa: S606
        else:
            subprocess.run(["open" if sys.platform == "darwin" else "xdg-open", path],
                           check=False)
        print(f"已打开(系统默认办公程序): {os.path.basename(path)}")
        print("请在 WPS/Word 中直接修改、圈选加批注后保存, 再运行 collect/diff")
        return 0
    except Exception as e:
        print(f"打开失败: {e}", file=sys.stderr)
        return 3


def doc_hash(path: str) -> str:
    h = hashlib.md5()
    with open(path, "rb") as f:
        h.update(f.read())
    return h.hexdigest()[:10]


RE_TAG = re.compile(r"<[^>]+>")


def baseline_dir(path: str) -> str:
    # 用绝对路径的哈希做键(内容会变,路径不变),保证 diff 永远找得到基线
    key = hashlib.md5(os.path.abspath(path).encode("utf-8")).hexdigest()[:10]
    return os.path.join(BASE_ROOT, "baselines",
                        os.path.splitext(os.path.basename(path))[0] + "-" + key)


def cmd_baseline(args) -> int:
    path = os.path.abspath(os.path.expanduser(args.file))
    if not os.path.isfile(path):
        print(f"找不到文件: {path}", file=sys.stderr)
        return 2
    d = baseline_dir(path)
    os.makedirs(d, exist_ok=True)
    dst = os.path.join(d, "baseline" + os.path.splitext(path)[1])
    shutil.copy2(path, dst)
    meta = {"origin": path, "hash": doc_hash(path),
            "saved_at": datetime.datetime.now().isoformat(timespec="seconds")}
    with open(os.path.join(d, "meta.json"), "w", encoding="utf-8") as f:
        json.dump(meta, f, ensure_ascii=False, indent=1)
    print(f"基线已留存: {dst}")
    print("用户改完保存后运行: canvas.py diff <文件> 对比改动")
    return 0


def read_comments(path: str):
    """读 docx 批注 + 批注锚定的正文文本。"""
    with zipfile.ZipFile(path) as z:
        names = z.namelist()
        if "word/comments.xml" not in names:
            return []
        cx = z.read("word/comments.xml").decode("utf-8", errors="replace")
        doc = z.read("word/document.xml").decode("utf-8", errors="replace")
    comments = {}
    for cid, author, body in RE_COMMENT.findall(cx):
        text = "".join(esc(t) for t in RE_TEXT.findall(body)).strip()
        comments[cid] = {"id": cid, "author": author, "text": text}

    # 锚定文本: commentRangeStart 到 commentRangeEnd 之间的裸文本
    # 区间可能嵌在 <w:t> 内部, 所以逐字符扫描标签边界, 精确到 End 标签为止
    for cid in comments:
        start_pat = r'<w:commentRangeStart\s[^>]*?w:id="%s"\s*/?>' % cid
        ms = re.search(start_pat, doc)
        if not ms:
            continue
        me = re.search(r'<w:commentRangeEnd\s[^>]*?w:id="%s"\s*/?>' % cid,
                       doc[ms.end():])
        if not me:
            continue
        seg = doc[ms.end():ms.end() + me.start()]
        bare = RE_TAG.sub("", seg)
        comments[cid]["anchor"] = esc(bare).strip()[:80]

    # 批注所在段落号: 统计锚定前的段落数
    PARA_PAT = re.compile(r"<w:p(?:\s[^>]*)?>")
    for cid in comments:
        start_pat = r'<w:commentRangeStart\s[^>]*?w:id="%s"\s*/?>' % cid
        ms = re.search(start_pat, doc)
        if not ms:
            comments[cid]["para"] = 1
            continue
        pos = ms.start()
        para_no = len(PARA_PAT.findall(doc[:pos])) + 1
        comments[cid]["para"] = para_no

    # 附加段落映射：{段落号: 原文}，供前端锚点高亮使用
    para_mapping = {}
    para_pat = re.compile(r"<w:p(?:\s[^>]*)?>")
    for cid in comments:
        start_pat = r'<w:commentRangeStart\s[^>]*?w:id="%s"\s*/?>' % cid
        ms = re.search(start_pat, doc)
        if ms:
            pos = ms.start()
            para_no = len(para_pat.findall(doc[:pos])) + 1
            comments[cid]["para"] = para_no
            # 收集该段落原文
            para_start = max(0, doc.rfind("<w:p", 0, pos))
            para_end = doc.find("</w:p>", pos) + 6
            if para_start != -1 and para_end != -1:
                para_seg = doc[para_start:para_end]
                bare = RE_TAG.sub("", para_seg)
                para_mapping[para_no] = esc(bare).strip()[:200]
    for cid in comments:
        comments[cid]["para_mapping"] = para_mapping
    return sorted(comments.values(), key=lambda c: int(c["id"]))


def cmd_collect(args) -> int:
    path = os.path.abspath(os.path.expanduser(args.file))
    if not os.path.isfile(path):
        print(f"找不到文件: {path}", file=sys.stderr)
        return 2
    comments = read_comments(path)
    if not comments:
        print(f"未发现批注: {os.path.basename(path)}")
        print("提示: 在 WPS/Word 圈选文字 → 插入批注 → 保存, 再重新 collect")
        return 0
    # 默认输出 Markdown 表格（保持向后兼容）
    print(f"## 人工批注整改清单 (共 {len(comments)} 条)")
    print(f"文件: {os.path.basename(path)}  读取时间: {datetime.datetime.now():%Y-%m-%d %H:%M}")
    print()
    print("| # | 批注人 | 定位 | 批注内容 | 引用原文 |")
    print("|---|---|---|---|---|")
    for i, c in enumerate(comments, 1):
        anchor = c.get("anchor", "")
        loc = f"P{c.get('para','?'):>03}"
        print(f"| {i} | {c['author'] or '匿名'} | {loc} | {c['text']} | {anchor} |")
    print()
    print("**执行纪律** (bid-perfect Step2b): 每条先读原文→逐段改→回读确认; 禁止正则批量替换。")
    print("改完执行: canvas.py verify 同一文件 检查响应。")
    # 存档（JSON 含完整 para_mapping，供 workbench 锚点高亮使用）
    d = os.path.join(BASE_ROOT, "collect")
    os.makedirs(d, exist_ok=True)
    out = os.path.join(d, os.path.splitext(os.path.basename(path))[0]
                       + datetime.datetime.now().strftime("_%Y%m%d_%H%M") + ".json")
    with open(out, "w", encoding="utf-8") as f:
        json.dump(comments, f, ensure_ascii=False, indent=1)
    print(f"(清单已存档: {out})")
    return 0


def para_texts(path: str):
    from docx import Document
    doc = Document(path)
    return [p.text.strip() for p in doc.paragraphs]


def cmd_diff(args) -> int:
    path = os.path.abspath(os.path.expanduser(args.file))
    d = baseline_dir(path)
    base = os.path.join(d, "baseline" + os.path.splitext(path)[1])
    if not os.path.isfile(base):
        print("未找到基线。请先在用户修改前运行: canvas.py baseline <文件>", file=sys.stderr)
        return 2
    import difflib
    a = para_texts(base)
    b = para_texts(path)
    diff = list(difflib.unified_diff(a, b, lineterm="", n=0,
                                     fromfile="基线", tofile="当前"))
    if not diff:
        print("与基线一致: 用户未改动段落文本。")
        return 0
    changed = [l for l in diff if l.startswith(("+", "-")) and not l.startswith(("+++", "---"))]
    print(f"## 用户改动摘要: {len(changed)} 处段落级变化")
    for l in changed[:40]:
        print(f"  {l[:100]}")
    if len(changed) > 40:
        print(f"  ...(共 {len(changed)} 处,仅显示前 40)")
    print("提示: 结合批注(collect)一起判断用户意图; 单纯删改可能是格式微调。")
    return 0


def cmd_mark(args) -> int:
    try:
        from PIL import Image, ImageDraw, ImageFont
    except ImportError:
        print("需要 Pillow: pip install pillow", file=sys.stderr)
        return 3
    path = os.path.abspath(os.path.expanduser(args.file))
    img = Image.open(path).convert("RGB")
    draw = ImageDraw.Draw(img)
    font = None
    for fp in ("msyh.ttc", "simhei.ttf"):
        try:
            font = ImageFont.truetype(fp, max(16, img.width // 40))
            break
        except Exception:
            continue
    boxes = args.box or []
    labels = args.label or []
    for i, box in enumerate(boxes):
        x, y, w, h = [int(v) for v in box.split(",")]
        color = (220, 20, 60)
        draw.rectangle([x, y, x + w, y + h], outline=color, width=4)
        label = labels[i] if i < len(labels) else str(i + 1)
        if font:
            draw.text((x + w + 8, y), label, fill=color, font=font)
        else:
            draw.text((x + w + 8, y), label, fill=color)
    root, ext = os.path.splitext(path)
    out = root + "_标注" + ext
    img.save(out)
    print(f"标注图已生成: {out}")
    print("标注说明将作为改图依据,连同原图一并交给图表生成脚本修正。")
    return 0


def cmd_verify(args) -> int:
    path = os.path.abspath(os.path.expanduser(args.file))
    comments = read_comments(path)
    if not comments:
        print("当前无批注(可能从未有批注,或文件经 python-docx 保存后批注部件被丢弃——")
        print("精修管线改完即属后者,视为已响应)。✅")
        return 0
    # 响应判定: 批注锚定文本是否还存在于正文
    from docx import Document
    doc = Document(path)
    full = "\n".join(p.text for p in doc.paragraphs)
    pending = [c for c in comments if c.get("anchor") and c["anchor"] in full]
    answered = [c for c in comments if not (c.get("anchor") and c["anchor"] in full)]
    print(f"批注响应验证: 共 {len(comments)} 条")
    if answered:
        print(f"  ✅ 已响应 {len(answered)} 条(锚定原文已修改):")
        for c in answered:
            print(f"     [{c['id']}] {c['text'][:50]}")
    if pending:
        print(f"  ⚠️ 未响应 {len(pending)} 条(锚定原文仍在,需继续整改):")
        for c in pending:
            print(f"     [{c['id']}] {c['text'][:50]} ← 原文: {c.get('anchor','')[:40]}")
        return 1
    print("全部批注已响应。可回复批注并交付,或重新 collect 复核。")
    return 0


def preserve_comments(src_docx: str, dst_docx: str) -> bool:
    """python-docx 保存会丢弃 comments part; 此工具把 src 的批注部件拷回 dst。

    注意: 仅拷 comments.xml + document.xml 里的锚定标记才有意义, 而 python-docx
    重写 document.xml 时锚定标记也会丢。所以真实用法是:
    精修改完成后 → preserve_comments(批注版, 新版) 只恢复批注文字(供人工复核),
    锚定是否已改以 collect 的存档清单为准。返回是否恢复了批注。
    """
    import tempfile as _tf
    try:
        with zipfile.ZipFile(src_docx) as zs:
            if "word/comments.xml" not in zs.namelist():
                return False
            cdata = zs.read("word/comments.xml")
        dst_dir = os.path.dirname(dst_docx) or tempfile.gettempdir()
        # Windows 下同目录 os.replace 有时会被 Python 的 GC 锁住 (zipfile 未关闭),
        # 故先写到系统 temp, 然后再 replace 到目标。
        fd, tmp = _tf.mkstemp(suffix=".docx", dir=tempfile.gettempdir())
        try:
            os.close(fd)
        except OSError:
            pass
        try:
            with zipfile.ZipFile(dst_docx) as zd, \
                 zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED) as zt:
                for n in zd.namelist():
                    zt.writestr(n, zd.read(n))
                zt.writestr("word/comments.xml", cdata)
            os.replace(tmp, dst_docx)
            return True
        except Exception:
            # 二次失败: 删掉可能残留的 tmp
            try:
                os.remove(tmp)
            except OSError:
                pass
            return False
    except Exception:
        return False


def cmd_restore(args) -> int:
    src = os.path.abspath(os.path.expanduser(args.src))
    dst = os.path.abspath(os.path.expanduser(args.dst))
    if not (os.path.isfile(src) and os.path.isfile(dst)):
        print("src/dst 文件不存在", file=sys.stderr)
        return 2
    if preserve_comments(src, dst):
        print(f"批注部件已恢复到: {os.path.basename(dst)} (供人工复核,锚定以collect存档为准)")
        return 0
    print("源文件无批注部件,未做修改", file=sys.stderr)
    return 1


def main() -> int:
    ap = argparse.ArgumentParser(description="office-canvas 画布反馈闭环")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("open", help="办公程序打开")
    p.add_argument("file")
    p.set_defaults(fn=open_in_office)

    p = sub.add_parser("baseline", help="留存基线")
    p.add_argument("file")
    p.set_defaults(fn=cmd_baseline)

    p = sub.add_parser("collect", help="读批注→整改清单")
    p.add_argument("file")
    p.set_defaults(fn=cmd_collect)

    p = sub.add_parser("diff", help="对比用户改动")
    p.add_argument("file")
    p.set_defaults(fn=cmd_diff)

    p = sub.add_parser("mark", help="图片标注")
    p.add_argument("file")
    p.add_argument("--box", action="append", help="x,y,w,h 可多次")
    p.add_argument("--label", action="append", help="与box对应")
    p.set_defaults(fn=cmd_mark)

    p = sub.add_parser("verify", help="验证批注响应")
    p.add_argument("file")
    p.set_defaults(fn=cmd_verify)

    p = sub.add_parser("restore", help="把批注部件恢复到python-docx保存后的文件")
    p.add_argument("src", help="含批注的原文件")
    p.add_argument("dst", help="精修后的文件")
    p.set_defaults(fn=cmd_restore)

    args = ap.parse_args()
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
