#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""标书DOCX审计脚本 — 合肥AI评标v3方法论的可执行实现。

用法:
    python audit_docx.py <docx路径>                    # 全量审计（结构+关键词+AI信号）
    python audit_docx.py <docx路径> --ai-signals        # 仅去AI化检查
    python audit_docx.py <docx路径> --keywords 针对性 可行性 语言精练度
    python audit_docx.py <docx路径> --subsections       # 子节字数统计
    python audit_docx.py <docx路径> --page-count        # WPS COM 统计实际页数

输出: JSON 报告（stdout），供代理直接读取。
依赖: pip install python-docx pymupdf
"""

import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path

try:
    from docx import Document
except ImportError:
    print("ERROR: python-docx 未安装。pip install python-docx", file=sys.stderr)
    sys.exit(2)

# 评分标准三维度关键词（合肥AI评标核心）
CORE_KEYWORDS = ["针对性", "可行性", "语言精练度"]
BONUS_KEYWORDS = ["结合本工程特点", "科学", "合理"]

# AI 术语黑名单
AI_TERMS = ["赋能", "抓手", "闭环", "颗粒度", "对齐", "拉通", "底层逻辑",
            "组合拳", "生态", "中台", "打法", "方法论沉淀", "复用赋能"]

# 高频机械词
MECHANICAL_WORDS = ["确保", "落实", "保证"]


def extract_paragraphs(doc):
    """提取全部段落文本（含标题层级）。"""
    result = []
    for p in doc.paragraphs:
        text = p.text.strip()
        if not text:
            continue
        style = p.style.name if p.style else ""
        level = 0
        m = re.match(r"Heading (\d)", style)
        if m:
            level = int(m.group(1))
        # 兼容中文样式名
        elif "标题" in style:
            m2 = re.search(r"(\d)", style)
            level = int(m2.group(1)) if m2 else 1
        # 编号式标题（如 4.1 xxx）
        elif re.match(r"^\d+\.\d+(\.\d+)?\s*\S", text) and len(text) < 40:
            level = 2 if text.count(".") == 1 else 3
        elif re.match(r"^\d+(\.\d+)?\s+\S", text) and len(text) < 35:
            level = 1
        result.append({"text": text, "level": level, "style": style})
    return result


def extract_tables(doc):
    """提取全部表格：数量、行数、文本样本。"""
    tables = []
    for i, t in enumerate(doc.tables):
        rows = len(t.rows)
        cols = len(t.columns)
        header = " | ".join(c.text.strip() for c in t.rows[0].cells)[:100] if rows else ""
        tables.append({"index": i + 1, "rows": rows, "cols": cols, "header": header})
    return tables


def count_images(doc):
    """统计内嵌图片数量。"""
    from docx.parts.image import ImagePart
    n = 0
    for rel in doc.part.rels.values():
        if isinstance(rel.target_part, ImagePart):
            n += 1
    return n


def keyword_check(paragraphs, keywords):
    """三维度关键词命中统计：全文计数 + 出现的段落定位。"""
    full_text = "\n".join(p["text"] for p in paragraphs)
    result = {}
    for kw in keywords:
        positions = []
        for i, p in enumerate(paragraphs):
            if kw in p["text"]:
                positions.append({"para_index": i, "snippet": p["text"][:60]})
        result[kw] = {"count": full_text.count(kw), "positions": positions[:10]}
    total_core = sum(result[kw]["count"] for kw in keywords if kw in keywords)
    return {"keywords": result, "core_total": total_core}


def ai_signal_check(paragraphs):
    """去AI化检查：段内编号密度/高频词密度/AI术语。"""
    full_text = "\n".join(p["text"] for p in paragraphs)
    total_chars = len(re.sub(r"\s", "", full_text))
    if total_chars == 0:
        return {"error": "空文档"}

    # 段内编号：段首不是编号但段中含 (1)(2)①②③ 或 "一是...二是..." 等列举式
    inner_numbering = 0
    numbered_paras = []
    for p in paragraphs:
        # 段首编号（如 "4.1.2" 或 "(一)"）不算段内编号
        body = re.sub(r"^\s*[\d.]+[、.\s]|^\s*[（(][一二三四五六七八九十\d]+[)）]", "", p["text"])
        matches = re.findall(r"[（(]\d+[)）]|[①②③④⑤⑥⑦⑧⑨⑩]|一是|二是|三是|四是|五是", body)
        if matches:
            inner_numbering += len(matches)
            numbered_paras.append(p["text"][:50])

    # 高频词密度（每千字）
    word_counts = {w: full_text.count(w) for w in MECHANICAL_WORDS}
    high_freq_permille = sum(word_counts.values()) / total_chars * 1000

    # AI 术语
    ai_term_hits = {t: full_text.count(t) for t in AI_TERMS if t in full_text}

    # 判定
    numbering_ok = inner_numbering < 20
    freq_ok = high_freq_permille < 0.5
    terms_ok = len(ai_term_hits) == 0

    return {
        "total_chars": total_chars,
        "inner_numbering_count": inner_numbering,
        "inner_numbering_ok": numbering_ok,
        "inner_numbering_samples": numbered_paras[:5],
        "mechanical_word_counts": word_counts,
        "high_freq_density_permille": round(high_freq_permille, 2),
        "high_freq_ok": freq_ok,
        "ai_terms_found": ai_term_hits,
        "ai_terms_ok": terms_ok,
        "deai_verdict": "PASS" if (numbering_ok and freq_ok and terms_ok) else "FAIL",
    }


def subsection_stats(paragraphs):
    """子节级字数统计：≥200字充分 / <200偏薄 / =0缺失。"""
    sections = []  # [{title, level, chars}]
    current = None
    for p in paragraphs:
        if p["level"] >= 2:  # 二级及以下标题开新子节
            current = {"title": p["text"][:50], "level": p["level"], "chars": 0,
                       "has_table": False}
            sections.append(current)
        elif current is not None:
            current["chars"] += len(re.sub(r"\s", "", p["text"]))
        elif p["level"] == 1:
            current = {"title": p["text"][:50], "level": p["level"], "chars": 0,
                       "has_table": False}
            sections.append(current)
    deep = [s for s in sections if s["chars"] >= 200]
    thin = [s for s in sections if 0 < s["chars"] < 200]
    missing = [s for s in sections if s["chars"] == 0]
    return {
        "subsection_count": len(sections),
        "deep_count": len(deep),
        "deep_ratio": round(len(deep) / len(sections) * 100, 1) if sections else 0,
        "thin": [{"title": s["title"], "chars": s["chars"]} for s in thin][:20],
        "missing": [{"title": s["title"]} for s in missing][:20],
    }


def estimate_score(kw_result, ai_result, table_count, image_count):
    """v3 预估公式（详见 references/calibration.md）。"""
    score = 75
    core_total = kw_result.get("core_total", 0)
    if core_total >= 5:
        score += 3
    elif core_total >= 1:
        score += 1
    if ai_result.get("deai_verdict") == "PASS":
        score += 2
    if 30 <= table_count <= 44 and 9 <= image_count <= 12:
        score += 2
    # 闭合链与论证句式需人工/代理判定，此处不计入；评委折扣按村庄改造小项目默认 -6
    score -= 6
    return {
        "base": 75, "keyword_bonus": (3 if core_total >= 5 else 1 if core_total >= 1 else 0),
        "deai_bonus": 2 if ai_result.get("deai_verdict") == "PASS" else 0,
        "chart_bonus": 2 if (30 <= table_count <= 44 and 9 <= image_count <= 12) else 0,
        "judge_discount": -6,
        "closure_bonus_manual": "需人工判定 四大闭合链+论证句式（各+2）",
        "point_estimate": score,
        "interval": [max(60, score), score + 5],
        "note": "评委打分均值机制，非AI直接出分；只报区间不报点值，下限为公式值。",
    }


def wps_page_count(docx_path):
    """WPS COM 统计实际页数（本机 Word COM 不稳，KWPS 实测可用）。"""
    try:
        import win32com.client  # 需 pip install pywin32
        w = win32com.client.Dispatch("KWPS.Application")
        w.Visible = False
        doc = w.Documents.Open(str(Path(docx_path).resolve()), ReadOnly=True)
        pages = doc.ComputeStatistics(2)
        doc.Close(False)
        w.Quit()
        return {"pages": pages, "method": "KWPS COM ComputeStatistics(2)"}
    except Exception as e:
        return {"error": f"WPS COM 不可用: {e}",
                "fallback": "pip install pywin32，且本机装有 WPS"}


def main():
    ap = argparse.ArgumentParser(description="标书DOCX审计（合肥AI评标v3）")
    ap.add_argument("docx", help="DOCX 文件路径")
    ap.add_argument("--ai-signals", action="store_true", help="仅去AI化检查")
    ap.add_argument("--keywords", nargs="*", default=None,
                    help="自定义关键词（默认三维度+加分词）")
    ap.add_argument("--subsections", action="store_true", help="仅子节字数统计")
    ap.add_argument("--page-count", action="store_true", help="WPS COM 实际页数")
    args = ap.parse_args()

    path = Path(args.docx)
    if not path.exists():
        print(json.dumps({"error": f"文件不存在: {path}"}, ensure_ascii=False))
        sys.exit(1)

    doc = Document(str(path))
    paragraphs = extract_paragraphs(doc)
    tables = extract_tables(doc)
    images = count_images(doc)

    report = {"file": str(path), "paragraph_count": len(paragraphs),
              "table_count": len(tables), "image_count": images}

    if args.page_count:
        report["page_info"] = wps_page_count(str(path))

    if args.ai_signals:
        report["ai_signals"] = ai_signal_check(paragraphs)
        print(json.dumps(report, ensure_ascii=False, indent=2))
        return

    if args.subsections:
        report["subsections"] = subsection_stats(paragraphs)
        print(json.dumps(report, ensure_ascii=False, indent=2))
        return

    # 全量审计
    kws = args.keywords if args.keywords else CORE_KEYWORDS + BONUS_KEYWORDS
    kw_result = keyword_check(paragraphs, kws)
    ai_result = ai_signal_check(paragraphs)
    report.update({
        "keyword_check": kw_result,
        "ai_signals": ai_result,
        "estimate": estimate_score(kw_result, ai_result, len(tables), images),
        "tables_detail": tables[:10],
        "next_steps": [
            "Step5 数据一致性核查需结合清单Excel（xlrd解析E.1分部分项sheet）",
            "四大闭合链校核需提取设备表/劳动力计划/材料计划后反算（references/closure-chains.md）",
            "论证句式检查需代理人工审阅段落",
        ],
    })
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
