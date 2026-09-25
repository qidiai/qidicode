#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
reverse_metrics.py — 高分方案反推量化指标提取器（bid-reverse 配套，第一方工具）
来源：2026-09-07 实测方法论（巢湖学院84.87 vs 中垾坝镇78.25 双真值卷对比）

用法:
  python reverse_metrics.py <方案文本.txt> [--strip-normal] [--score 84.87
      --score-source "评标结果一览表"] [--site-keywords 泮山苑,Ⅰ区,...] [-o out.json]

Step 0 得分门禁: 提供 --score 即 mode=verified（可产入库结论）；
  缺失则 mode=hypothesis_only —— 报告只可作"待验证假设"，禁止产出可入库写作规则。

纪律（SKILL.md 同款）:
  - 指标是"体检信号"不是"得分规则"——输出只给分水岭对照，不给达标线
  - 条件分级类特征标"可选特征"（84.87卷实证≈0）
"""
import argparse
import io
import json
import re
import sys
from pathlib import Path

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')

# ── 分水岭参照系（2026-09-07 双真值卷实测，非达标线，仅对照） ──────────────
REFERENCE = {
    '巢湖学院_84.87': {
        'chars_total': 50208, 'emdash_permille': 0.82, 'data_points_permille': 15.85,
        'thresholds_permille': 3.66, 'site_terms_permille': 4.18, 'acceptance_total': 161,
        'difficulty_defs': 5, 'countermeasures': 5, 'cross_refs': 12,
        'factor_annotations': 9, 'closed_loop_count': 7,
        'kw_zdt': 5, 'kw_kxx': 0, 'kw_yjld': 0,
    },
    '中垾坝镇_78.25': {
        'chars_total': 38928, 'emdash_permille': 0.23, 'data_points_permille': 23.12,
        'thresholds_permille': 8.27, 'site_terms_permille': 5.83, 'acceptance_total': 171,
        'difficulty_defs': 0, 'countermeasures': 2, 'cross_refs': 4,
        'factor_annotations': 0, 'closed_loop_count': 1,
        'kw_zdt': 0, 'kw_kxx': 0, 'kw_yjld': 0,
    },
    '_note': '分水岭含义：高分独有特征=难点三件套/因果链密度/评审因素标注/详见网状引用；'
             '数据点密度与阈值密度高≠高分（78.25卷反而更高——数字嵌在因果链里才有分）',
}

ACCEPT_KWS = ['验收', '检验', '检测', '试验', '复核', '查验', '旁站', '见证取样']
BANNED_KWS = ['我方', '我们', '尽力', '争取', '综上所述', '值得注意的是']


def load_text(path, strip_normal):
    raw = open(path, encoding='utf-8').read()
    if strip_normal:
        lines = [re.sub(r'^\[[^\]]*\]\s*', '', ln).strip() for ln in raw.split('\n')]
        raw = '\n'.join(ln for ln in lines if ln)
    return raw


def metrics(text, site_keywords):
    chars = len(re.sub(r'\s', '', text))
    m = {'chars_total': chars}
    # ①标题树骨架
    m['h1_count'] = len(re.findall(r'第[一二三四五六七八九十]+章', text))
    m['h2_count'] = len(re.findall(r'(?m)^\d+\.\d+\s*[^\d.]', text))
    m['h3_count'] = len(re.findall(r'(?m)^\d+\.\d+\.\d+\s', text))
    m['factor_annotations'] = len(re.findall(r'【对应评审因素[^】]*】', text))
    # ②编号重置（圈号使用分布——段内从①重置为铁律）
    circled = re.findall(r'[①②③④⑤⑥⑦⑧⑨⑩]', text)
    m['circled_total'] = len(circled)
    m['numbering_note'] = '圈号在各节内应从①重置；跨节连续递增=编号违规（人工核对）'
    # ③因果链密度（"——"原始出现）
    m['emdash_total'] = len(re.findall(r'——', text))
    m['emdash_permille'] = round(m['emdash_total'] / chars * 1000, 2) if chars else 0
    # ④数据注入密度
    dp = re.findall(r'\d[\d,，.]*\s*(?:㎡|m²|m³|m|km|吨|t|台|套|栋|人|天|日|万元|元|％|%|‰|℃|MPa|mm|cm|dB|级|间|处|次|组|遍|h|kW)', text)
    m['data_points_permille'] = round(len(dp) / chars * 1000, 2) if chars else 0
    th = re.findall(r'[≥≤＞＜><]\s*\d[\d.]*|不少于\d+|不超过\d+', text)
    m['thresholds_permille'] = round(len(th) / chars * 1000, 2) if chars else 0
    # ⑤验收闭环 + 网状引用
    m['acceptance_total'] = sum(text.count(k) for k in ACCEPT_KWS)
    m['unqualified_handling'] = len(re.findall(r'不合格[^。]{0,25}', text))
    m['cross_refs'] = len(re.findall(r'详见[\d\s、.和及至第章节]*', text))
    # ⑥禁用词 + 管理惯用语
    m['banned_hits'] = {k: text.count(k) for k in BANNED_KWS if text.count(k)}
    m['closed_loop_count'] = text.count('闭环')
    # ⑦难点三件套
    m['difficulty_defs'] = len(re.findall(r'难点[一二三四五六七八九十]：', text))
    m['countermeasures'] = text.count('应对措施')
    # ⑧三维度关键词
    m['kw_zdt'] = text.count('针对性')
    m['kw_kxx'] = text.count('可行性')
    m['kw_yjld'] = text.count('语言精练度')
    # 现场感（项目专名，可传词表）
    if site_keywords:
        pat = '|'.join(re.escape(k) for k in site_keywords)
        n = len(re.findall(pat, text))
        m['site_terms_permille'] = round(n / chars * 1000, 2) if chars else 0
    return m


def ridge_table(m):
    """分水岭对照表：当前样本 vs 两个真值卷参照（对照≠达标线）"""
    rows = [
        ('字符总量', 'chars_total', ''),
        ('因果链"——"密度‰', 'emdash_permille', '84.87卷≈0.82 vs 78.25卷0.23（3.5倍差）'),
        ('数据点密度‰', 'data_points_permille', '反转指标：78.25卷23.1更高——数字嵌在哪比多少重要'),
        ('阈值参数密度‰', 'thresholds_permille', '同上：堆砌阈值≠高分'),
        ('现场感密度‰', 'site_terms_permille', ''),
        ('验收动作总数', 'acceptance_total', '两卷相当（161 vs 171）——验收体系非分水岭'),
        ('难点N定义数', 'difficulty_defs', '分水岭：84.87卷5 vs 78.25卷0'),
        ('应对措施数', 'countermeasures', '分水岭：5 vs 2'),
        ('详见X.X网状引用', 'cross_refs', '分水岭：12 vs 4（3倍）'),
        ('评审因素标注', 'factor_annotations', '分水岭：9 vs 0'),
        ('"闭环"管理惯用语', 'closed_loop_count', '84.87卷7次仍最高分——语境豁免实证'),
        ('针对性/可行性/语言精练度', 'kw_zdt', f"{m.get('kw_zdt', 0)}/{m.get('kw_kxx', 0)}/{m.get('kw_yjld', 0)}"),
    ]
    out = []
    for label, key, note in rows:
        cur = m.get(key, '')
        ref1 = REFERENCE['巢湖学院_84.87'].get(key, '—')
        ref2 = REFERENCE['中垾坝镇_78.25'].get(key, '—')
        out.append({'指标': label, '本卷': cur, '84.87参照': ref1, '78.25参照': ref2, '分水岭注': note})
    return out


def main():
    ap = argparse.ArgumentParser(description='高分方案反推量化指标提取（bid-reverse）')
    ap.add_argument('text_file')
    ap.add_argument('--strip-normal', action='store_true', help='去除[Normal]等docx样式前缀')
    ap.add_argument('--score', type=float, default=None, help='Step 0: 实测得分声明')
    ap.add_argument('--score-source', default='', help='得分来源（评标结果一览表等）')
    ap.add_argument('--site-keywords', default='', help='项目专名词表（逗号分隔）')
    ap.add_argument('-o', '--output', default=None)
    args = ap.parse_args()

    text = load_text(args.text_file, args.strip_normal)
    site_kws = [k for k in args.site_keywords.split(',') if k]
    m = metrics(text, site_kws)

    mode = 'verified' if args.score is not None else 'hypothesis_only'
    result = {
        'step0': {
            'mode': mode,
            'declared_score': args.score,
            'score_source': args.score_source,
            'gate_rule': ('有实测得分声明：可产出"入库规则"级结论，每条须标注'
                          '[来源:样本名+实测分]') if mode == 'verified' else
                         ('无实测得分声明：本报告全部结论只可表述为"待验证假设"，'
                          '禁止产出可入库写作规则（防把失分特征当规则入库）'),
        },
        'metrics': m,
        'ridge_table': ridge_table(m),
        'optional_traits': {
            '条件分级': '可选特征——84.87真值卷实测分级表述≈0，不构成高分必要项',
        },
    }
    js = json.dumps(result, ensure_ascii=False, indent=1)
    print(js)
    if args.output:
        Path(args.output).write_text(js, encoding='utf-8')
        print(f'\n已保存: {args.output}', file=sys.stderr)


if __name__ == '__main__':
    main()
