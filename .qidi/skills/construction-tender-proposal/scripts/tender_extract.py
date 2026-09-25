#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""tender_extract.py — 招标文件解析器（gov-procurement-proposal Step 1）

用法:
    python tender_extract.py <招标文件.pdf 或 .txt> [--json out.json]

从政府采购招标文件中自动提取:
  1. 项目类型判定（工程施工 / 货物 / 服务）
  2. 技术文件详细评审因素清单（①②③…）→ 方案章节骨架
  3. 编制硬格式要求（A4/行距/字体/字号/篇幅上限）
  4. 评审维度与 AI 类人评审信号
  5. 形式评审否决项红线
零第三方依赖（PDF 解析优先用 pymupdf，缺失则降级要求用户提供 txt）。
"""
import json
import re
import sys
from pathlib import Path

FACTOR_PAT = re.compile(r"(\d+)[）)\uFF09.、]\s*([^\d][^；;。]{1,30}?)(?=[；;。,，]?\s*(?:\d+[）)\uFF09.、]|$|一般得|注：))")
FORMAT_RULES = [
    ("纸张A4", r"A4"),
    ("行距固定22磅", r"(?:固定值\s*22|22\s*磅)"),
    ("页边距上2.5其余2.0", r"上\s*2\.5\s*厘米|上2\.5cm"),
    ("宋体", r"宋体"),
    ("标题三号", r"标题[：:]\s*三号"),
    ("正文四号", r"(?:其他|正文)[：:]?\s*四号"),
]
PAGE_LIMIT_PAT = re.compile(r"(?:不超过|最多不超过)\s*(\d+)\s*页")
TYPE_KEYWORDS = {
    "工程施工": ["施工招标", "施工组织设计", "工程量清单", "危大工程", "施工总平面"],
    "货物采购": ["采购需求", "技术参数", "货物清单", "供货", "质保期"],
    "服务采购": ["服务要求", "服务方案", "磋商文件", "咨询服务", "运维服务"],
}
REJECT_MARKS = ["投标报价", "投标人名称", "公司名称", "联系方式", "身份证", "手机号", "邮箱"]
DIMENSIONS = ["针对性", "可行性", "语言精练", "响应性"]


def pdf_to_text(path: Path) -> str:
    try:
        import pymupdf  # noqa: PLC0415
    except ImportError:
        sys.stderr.write("未安装 pymupdf，请先提供招标文件纯文本(.txt)\n")
        raise
    doc = pymupdf.open(str(path))
    pages = []
    for i in range(doc.page_count):
        pages.append(doc[i].get_text())
    return "\n".join(pages)


def clean(t: str) -> str:
    return re.sub(r"\s+", "", t)


def extract_factors(compact: str):
    """提取『包括但不限于以下内容』后的顺序编号因素清单（必须 1,2,3… 连续）。"""
    factors = []
    for m in re.finditer(r"包括但不限[于至]以下内容[:：]?", compact):
        seg = compact[m.end():m.end() + 500]
        found, n = [], 1
        while n <= 15:
            mm = re.search(rf"(?<!\d){n}[）)\uFF09.、]", seg)
            if not mm:
                break
            rest = seg[mm.end():]
            stop = re.search(r"(?<!\d)\d+[）)\uFF09.、]|一般得|注[：:]|本项|未提供|评分标准", rest)
            name = (rest[:stop.start()] if stop else rest[:60]).strip("。，；;:：、（）()")
            if len(name) < 2:
                break
            found.append(name)
            seg = rest[stop.start():] if stop else ""  # 保留下一个编号标记供下一轮匹配
            n += 1
        if len(found) >= 5:
            # 合同条款黑名单：排除误捕的"包括但不限于"合同段（乙方/承包人/费用结算类）
            contract_words = ("乙方", "承包人", "分包", "结算", "装表计量", "脚手架", "发包人")
            bad = sum(1 for f in found if any(w in f for w in contract_words))
            if bad >= 2:
                continue
            factors = found
            break
    return factors


def classify(compact: str, factors):
    scores = {k: sum(1 for kw in kws if kw in compact) for k, kws in TYPE_KEYWORDS.items()}
    ptype = max(scores, key=scores.get)
    if scores[ptype] == 0:
        ptype = "未知"
    variant = "通用"
    if ptype == "工程施工":
        has_danger = any("危大" in f for f in factors)
        has_plan = any("总平面" in f for f in factors)
        if len(factors) == 10 and has_danger and has_plan:
            variant = "十因素(含危大保障+总平面图)"
        elif len(factors) == 10 and has_plan:
            variant = "十因素(含总平面图)"
        elif len(factors) == 9:
            variant = "九因素(维修改造类)"
    return ptype, variant, scores


def main(argv):
    if not argv:
        print(__doc__)
        return 2
    args = [a for a in argv if not a.startswith("--")]
    src = Path(args[0])
    out_json = None
    if "--json" in argv:
        out_json = argv[argv.index("--json") + 1]

    text = pdf_to_text(src) if src.suffix.lower() == ".pdf" else src.read_text(encoding="utf-8-sig")
    compact = clean(text)

    factors = extract_factors(compact)
    ptype, variant, scores = classify(compact, factors)

    fmt = [name for name, pat in FORMAT_RULES if re.search(pat, compact)]
    page_lim = PAGE_LIMIT_PAT.search(compact)
    dims = [d for d in DIMENSIONS if d in compact]
    rejects = [r for r in REJECT_MARKS if r in compact]
    ai_review = bool(re.search(r"AI[“\"]?类人[”\"]?评审|大模型.{0,6}评审", compact))

    result = {
        "source": str(src),
        "项目类型": ptype,
        "类型得分": scores,
        "方案变体": variant,
        "评审因素": factors,
        "因素数量": len(factors),
        "编制硬格式": fmt,
        "篇幅上限_页": int(page_lim.group(1)) if page_lim else None,
        "评审维度": dims,
        "AI类人评审": ai_review,
        "否决项信号词": rejects,
        "章节骨架建议": [f"第{i+1}章 {f}" for i, f in enumerate(factors)] if factors else [],
    }

    print(json.dumps(result, ensure_ascii=False, indent=1))
    if out_json:
        Path(out_json).write_text(json.dumps(result, ensure_ascii=False, indent=1), encoding="utf-8")
    return 0 if factors else 1


if __name__ == "__main__":
    try:
        sys.stdout.reconfigure(encoding="utf-8")
    except Exception:  # noqa: BLE001
        pass
    sys.exit(main(sys.argv[1:]))
