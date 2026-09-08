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

# ── AI 术语语境豁免表（A5+A8 2026-09-07，均经用户明示批准）─────────────
# 治理规则（防黑名单名存实亡）：
#   1) A5"闭环"豁免：证据=巢湖学院84.87实测样本7处全为管理动作惯用语
#      （整改闭环×2/隐患闭环×2/回访闭环/完整闭环/24小时内闭环）；
#   2) A8"对齐/生态"豁免：证据=中垾坝镇78.25卷实测——"板缝对齐贯通"系
#      混凝土施工实义、"生态保护/生态红线控制"系环保监管术语；
#   3) 默认拒绝：未命中豁免语境的词照常计数（如"业务闭环""打造生态"）；
#   4) 新增豁免词或扩语境必须经用户明示批准，禁止自行扩充。
AI_TERM_EXEMPTIONS = {
    "闭环": [
        r"(?:整改|验收|投诉|问题|处置|管理|隐患|回访|复查)[^。；\n]{0,10}?闭环",
        r"完整闭环",
        r"闭环(?:管理|处置|台账|机制)",
    ],
    "对齐": [
        r"板缝对齐",
    ],
    "生态": [
        r"生态(?:保护|红线)",
    ],
}


def _count_ai_term_with_exemptions(full_text, term):
    """按语境豁免统计AI术语命中。返回 (有效命中数, 豁免明细dict)。

    无豁免表的词=裸计数；有豁免表的词逐处取上下文窗口（前14后8字），
    命中豁免语境则不计，豁免处留样可审计。
    """
    if term not in AI_TERM_EXEMPTIONS:
        return full_text.count(term), {}
    patterns = [re.compile(p) for p in AI_TERM_EXEMPTIONS[term]]
    total = exempted = 0
    samples = []
    for m in re.finditer(re.escape(term), full_text):
        total += 1
        window = full_text[max(0, m.start() - 14): m.end() + 8]
        if any(p.search(window) for p in patterns):
            exempted += 1
            if len(samples) < 5:
                samples.append(re.sub(r"\s+", " ", window)[:24])
    detail = {"total": total, "exempted": exempted, "samples": samples} if exempted else {}
    return total - exempted, detail

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

    # AI 术语（含语境豁免，A5 2026-09-07：管理惯用语"闭环"不计，默认拒绝未匹配语境）
    ai_term_hits = {}
    ai_terms_exempted = {}
    for t in AI_TERMS:
        n, detail = _count_ai_term_with_exemptions(full_text, t)
        if n:
            ai_term_hits[t] = n
        if detail:
            ai_terms_exempted[t] = detail

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
        "ai_terms_exempted": ai_terms_exempted,
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


def estimate_score(kw_result, ai_result, tables, image_count):
    """v3 预估公式（详见 references/calibration.md）。"""
    score = 75
    core_total = kw_result.get("core_total", 0)
    if core_total >= 5:
        score += 3
    elif core_total >= 1:
        score += 1
    if ai_result.get("deai_verdict") == "PASS":
        score += 2
    # 2026-08-31 修订：表格图片不设固定总数（历史 30~44表/12~25表 区间均已废除）。
    # 改按"按需+实义"判定：有表有图、且凑数小表占比不过半 → 计达标 +2；
    # 数量本身不是得分依据，硬堆砌表格没有意义。
    table_count = len(tables)
    small_tables = [t for t in tables if 0 < t.get("rows", 0) < 4]
    chart_ok = table_count > 0 and image_count > 0 and len(small_tables) * 2 <= table_count
    if chart_ok:
        score += 2
    # 闭合链与论证句式需人工/代理判定，此处不计入。
    # 评委折扣已于2026-09-01经庐江459家评标表实证废除（评委=基分±0.2微扰的确认器，
    # 见 SKILL.md Step 5.5/6）。A7 2026-09-07 清理残留：本函数不再扣减，与校准文档对齐。
    chart_warnings = []
    if table_count == 0:
        chart_warnings.append("全文无表：计划/配置/对照类数据建议按需补表，避免长段罗列")
    elif len(small_tables) * 2 > table_count:
        chart_warnings.append(f"凑数小表 {len(small_tables)}/{table_count}：疑似硬堆砌，建议删除或合并为正文")
    if image_count == 0:
        chart_warnings.append("全文无图：关键工艺/布置图建议按需补充")
    return {
        "base": 75, "keyword_bonus": (3 if core_total >= 5 else 1 if core_total >= 1 else 0),
        "deai_bonus": 2 if ai_result.get("deai_verdict") == "PASS" else 0,
        "chart_bonus": 2 if chart_ok else 0,
        "chart_principle": "不设固定总数，按内容需要提供；每表每图须承载真实信息并被正文引用",
        "chart_warnings": chart_warnings,
        "judge_discount": 0,
        "judge_discount_note": "评委折扣2026-09-01庐江459家评标表实证废除（详见SKILL.md Step 5.5）；A7 2026-09-07清理",
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
        "estimate": estimate_score(kw_result, ai_result, tables, images),
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


# ── V4 编号语境判断（2026-09-08 A23）──────────────────────────

def detect_custom_scoring(tender_text):
    """检测招标评分标准是否含定制①~⑦重难点清单（V4前置判断）。

    判定特征：
    - 正文含"①②③④⑤⑥⑦"编号的专业分组清单（如"建筑专业①~⑦"）
    - 存在"重难点分析"专项评分表述
    - 评审因素按专业分组（非通用10项）

    Returns: (bool is_custom, list evidence)
    """
    evidence = []
    # 特征1：专业前缀+①~⑦连续编号（如"建筑①~⑦""幕墙①~⑤"）
    pro_circled = re.findall(
        r'[（(]?(?:建筑|幕墙|照明|景观|装饰|机电|结构|给排水|暖通)[^。；\n]{0,8}'
        r'[①②③][^。；\n]{0,60}[④⑤⑥⑦]', tender_text)
    if pro_circled:
        evidence.append(f'专业分组①~⑦清单: {pro_circled[0][:50]}')
    # 特征2：重难点分析专项评分
    if re.search(r'重难点[^。\n]{0,20}(?:分析|评分|评审)', tender_text):
        evidence.append('重难点分析专项评分')
    # 特征3：圈号①-⑦密集出现（≥10个不同圈号在评分标准区域）
    circled_set = set(re.findall(r'[①②③④⑤⑥⑦]', tender_text))
    if len(circled_set) >= 5:
        evidence.append(f'评分标准含圈号编号: {len(circled_set)}个不同圈号')
    return (len(evidence) >= 2, evidence)  # 至少2个特征才判定制型


def judge_numbering_context(inner_count, tender_text, threshold=20):
    """V4编号语境判断：定制型+镜像响应=不扣分，否则按v3规则。

    Returns: (bool is_fail, str reason)
    """
    if inner_count < threshold:
        return False, f'编号{inner_count}个 < 阈值{threshold}'
    is_custom, evidence = detect_custom_scoring(tender_text)
    if is_custom:
        return False, (f'编号{inner_count}个≥阈值但为定制型项目镜像响应'
                       f'（证据: {"; ".join(evidence[:2])}）——不扣分')
    return True, f'编号{inner_count}个 ≥ 阈值{threshold}（通用型项目，v3规则）'
