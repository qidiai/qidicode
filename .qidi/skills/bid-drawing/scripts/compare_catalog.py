#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""目录-图纸交叉比对 + 图号归一化 — R3改造（v2方案2026-09-07）

k3审计M3落实：
- 图号归一化函数：全/半角、破折号五胞胎、"图"前缀、前导零（验收断言级别）
- 四清单比对：missing/extra/name_mismatch/duplicates
- catalog双格式：txt（一行"图号 图名"）或 index.json（pdf_extract_drawings产物）
- A12对齐：预留 `--- 表格 j (第 N 页) ---` 序列化块消费口
- name匹配：difflib.SequenceMatcher阈值0.6（标准库，零新依赖）

用法（一般由drawing_audit.py调用，也可独立跑）:
    python compare_catalog.py --catalog <清单.txt或index.json> --drawings <dxf_meta输出.json>
"""
import argparse
import difflib
import json
import re
import sys
import unicodedata
from pathlib import Path
from typing import Dict, List, Tuple, Optional

# ── 图号归一化（M3①：k3称"比对成败的生命线"） ──

DASH_CHARS = '-–—−－'  # 破折号五胞胎（原FIGURE_PATTERNS只覆盖3种）


def normalize_fig_id(raw: str) -> str:
    """图号归一化：全/半角→破折号统一→去"图"前缀→去前导零→去空白。

    归一化断言：normalize_fig_id("图２—０１") == normalize_fig_id("2-1") == "2-1"
    """
    if not raw:
        return ''
    s = str(raw).strip()
    # 全角→半角（unicodedata NFKC同时处理全角数字/字母）
    s = unicodedata.normalize('NFKC', s)
    # 破折号五胞胎→'-'
    for d in DASH_CHARS:
        if d != '-':
            s = s.replace(d, '-')
    # 去"图"/"张"前缀（含全角）
    s = re.sub(r'^[图張张]\s*', '', s)
    # 去内部空白
    s = re.sub(r'\s+', '', s)
    # 分解专业前缀+编号：建施-01 → 建施-1（前导零去除，前缀保留）
    m = re.match(r'^([A-Za-z\u4e00-\u9fff]+)-(\d+)(?:-(\d+))?$', s)
    if m:
        prefix, num1, num2 = m.group(1), m.group(2), m.group(3)
        n1 = str(int(num1))
        n2 = str(int(num2)) if num2 else None
        return f"{prefix}-{n1}" + (f"-{n2}" if n2 else '')
    # 纯数字对：2-1 / 02-01
    m = re.match(r'^(\d+)-(\d+)$', s)
    if m:
        return f"{int(m.group(1))}-{int(m.group(2))}"
    # 纯数字：001 → 1
    m = re.match(r'^(\d+)$', s)
    if m:
        return str(int(m.group(1)))
    return s.lower()


def parse_catalog(catalog_path: str) -> List[Dict[str, str]]:
    """解析图纸清单（txt或index.json双格式）。返回[{fig_id, name}]（原始未归一）。"""
    p = Path(catalog_path)
    items: List[Dict[str, str]] = []
    if p.suffix.lower() == '.json':
        data = json.loads(p.read_text(encoding='utf-8'))
        # pdf_extract_drawings.py的index.json格式
        if isinstance(data, dict):
            for fig in data.get('figure_refs', []):
                items.append({'fig_id': str(fig.get('fig_id', '')),
                              'name': str(fig.get('name', '') or '')})
        elif isinstance(data, list):
            for fig in data:
                if isinstance(fig, dict):
                    items.append({'fig_id': str(fig.get('fig_id', '')),
                                  'name': str(fig.get('name', '') or '')})
    else:
        # txt：一行一条 "图号[空白]图名"——单空格/多空格/tab均支持
        for line in p.read_text(encoding='utf-8', errors='replace').splitlines():
            line = line.strip()
            if not line or line.startswith('#'):
                continue
            # A12对齐：容忍 `--- 表格 j (第 N 页) ---` 序列化块行（跳过）
            if re.match(r'^---\s*表格', line):
                continue
            # 尝试按"图号后跟空白"切分：图号可能含专业前缀（建施-1）或纯数字对（2-1）或三位数（001）
            m = re.match(r'^((?:[A-Za-z\u4e00-\u9fff]+[-–—−－])?\s*[图圓]?\s*\d+(?:\s*[-–—−－]\s*\d+)?|[A-Za-z\u4e00-\u9fff]+\d+|图\d+)\s+(.+)$', line)
            if m:
                items.append({'fig_id': m.group(1).strip(), 'name': m.group(2).strip()})
            else:
                parts = re.split(r'[\t]+|\s{2,}', line, maxsplit=1)
                if len(parts) == 2:
                    items.append({'fig_id': parts[0].strip(), 'name': parts[1].strip()})
                else:
                    items.append({'fig_id': line, 'name': ''})
    return [x for x in items if x['fig_id']]


def load_drawing_metas(meta_json_path: str) -> List[Dict]:
    """读取 dxf_meta.py 的输出JSON。"""
    data = json.loads(Path(meta_json_path).read_text(encoding='utf-8'))
    return data.get('results', [])


def compare(catalog: List[Dict[str, str]], drawings: List[Dict],
            name_threshold: float = 0.6) -> Dict[str, list]:
    """四清单比对（M3②）。

    catalog: 招标图纸清单 [{fig_id, name}]
    drawings: dxf_meta输出 results [{fig_id, fig_name, ...}]
    """
    # 归一化索引
    cat_by_norm: Dict[str, Dict[str, str]] = {}
    for item in catalog:
        nid = normalize_fig_id(item['fig_id'])
        if nid:
            cat_by_norm.setdefault(nid, item)

    draw_by_norm: Dict[str, List[Dict]] = {}
    for d in drawings:
        raw = d.get('fig_id')
        if not raw:
            continue
        nid = normalize_fig_id(raw)
        if nid:
            draw_by_norm.setdefault(nid, []).append(d)

    missing = []       # 清单有图无纸
    extra = []         # 有图无清单
    name_mismatch = [] # 图号同图名差异大
    duplicates = []    # 同图号多张

    # missing
    for nid, item in cat_by_norm.items():
        if nid not in draw_by_norm:
            missing.append({'fig_id': item['fig_id'], 'name': item['name'],
                            'normalized': nid})

    # extra / duplicates / name_mismatch
    for nid, dlist in draw_by_norm.items():
        if nid not in cat_by_norm:
            extra.append({'fig_id': dlist[0].get('fig_id'), 'normalized': nid,
                          'name': dlist[0].get('fig_name'),
                          'file': dlist[0].get('file')})
        if len(dlist) > 1:
            duplicates.append({'fig_id': dlist[0].get('fig_id'), 'normalized': nid,
                               'count': len(dlist),
                               'files': [d.get('file') for d in dlist]})
        if nid in cat_by_norm:
            cat_name = cat_by_norm[nid].get('name', '') or ''
            draw_name = dlist[0].get('fig_name') or ''
            if cat_name and draw_name:
                ratio = difflib.SequenceMatcher(None, cat_name, draw_name).ratio()
                if ratio < name_threshold:
                    name_mismatch.append({'fig_id': cat_by_norm[nid]['fig_id'],
                                          'catalog_name': cat_name,
                                          'drawing_name': draw_name,
                                          'similarity': round(ratio, 2),
                                          'file': dlist[0].get('file')})

    return {'missing': missing, 'extra': extra,
            'name_mismatch': name_mismatch, 'duplicates': duplicates,
            'summary': {'catalog_count': len(cat_by_norm),
                        'drawing_count': sum(len(v) for v in draw_by_norm.values()),
                        'missing_count': len(missing), 'extra_count': len(extra),
                        'name_mismatch_count': len(name_mismatch),
                        'duplicates_count': len(duplicates)}}


def main():
    ap = argparse.ArgumentParser(description='目录-图纸交叉比对')
    ap.add_argument('--catalog', required=True, help='图纸清单（txt或index.json）')
    ap.add_argument('--drawings', required=True, help='dxf_meta输出JSON')
    ap.add_argument('--name-threshold', type=float, default=0.6)
    args = ap.parse_args()

    catalog = parse_catalog(args.catalog)
    drawings = load_drawing_metas(args.drawings)
    result = compare(catalog, drawings, args.name_threshold)
    print(json.dumps(result, ensure_ascii=False, indent=1))


if __name__ == '__main__':
    # 模块自检（M3①验收断言）
    assert normalize_fig_id('图２—０１') == normalize_fig_id('2-1') == '2-1', '归一化断言失败'
    assert normalize_fig_id('建施-01') == '建施-1', '前导零断言失败'
    assert normalize_fig_id('结施—２—１') == '结施-2-1', '专业前缀断言失败'
    main()
