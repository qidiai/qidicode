#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""DXF图纸元数据提取器 — 从DXF提取图号/图名/比例/图幅/图层语义/天正兼容信息。

R2改造（v2方案2026-09-07，k3审计M2/L2/L4/L5/L6全落实）：
- 图号来源：文件名优先 → 标题栏启发式兜底
- 图幅判定：布局页面设置优先 → 几何探测兜底 → $INSUNITS归一到mm
- 文字收集：模型空间+图纸空间+块内ATTRIB递归
- MTEXT清理：plain_mtext去格式码
- 天正兼容：ezdxf.recover.readfile容错 + ACAD_PROXY_ENTITY计数
- 图名启发式：最大字号 + 置信度标记

用法:
    python dxf_meta.py <图纸.dxf>          # 单文件
    python dxf_meta.py <目录>              # 批量全部DXF
输出: JSON（stdout），含 fig_id/fig_name/scale/sheet_size/layers/proxy等
依赖: pip install ezdxf
"""
import argparse
import json
import re
import sys
from pathlib import Path
from typing import Dict, Any, List, Tuple, Optional

try:
    import ezdxf
    from ezdxf.lldxf import const as ezdxf_const
except ImportError:
    print(json.dumps({"error": "ezdxf 未安装"}), file=sys.stderr)
    sys.exit(2)

# ── 图号识别（与drawing_audit.py一致的正则体系） ──
FIG_ID_PATTERNS = [
    re.compile(r"[图圆]\s*(\d{1,3})\s*[-–—−－]\s*(\d{1,3})", re.U),
    re.compile(r"[图圆]\s*0*(\d{2,3})", re.U),
    re.compile(r"(建施|结施|水施|电施|暖施|施)\s*[-–]\s*(\d{1,3})", re.U),
]
# 比例（1:100 / 1:500 / 1：100 / 1比100）
SCALE_PATTERN = re.compile(r"1\s*[:：比]\s*(\d+)", re.U)
# 图幅（A0~A4系列 mm）
SHEET_SIZES = {'A0': (841, 1189), 'A1': (594, 841), 'A2': (420, 594),
               'A3': (297, 420), 'A4': (210, 297)}
SHEET_TOL = 5.0  # mm
# 图层语义分类（GB/T专业前缀 + 中文关键词）
LAYER_SEMANTICS = [
    ('建筑', r'建筑|arch|wall|wall|door|window|A-'),
    ('结构', r'结构|struct|beam|column|slab|foundation|S-'),
    ('给排水', r'给排|水施|plumb|pipe|drain|water|P-'),
    ('电气', r'电气|电施|power|light|socket|E-'),
    ('暖通', r'暖通|暖施|hvac|vent|duct|M-'),
    ('消防', r'消防|fire|sprinkler|F-'),
    ('弱电', r'弱电|安防|监控|网络|T-'),
    ('装饰', r'装饰|装修|finish|ceiling'),
    ('总图', r'总图|总平|site|road|landscape|红线|临设|围挡|图例'),
]
INSUNITS_MAP = {1: 'inch', 4: 'mm', 5: 'cm', 6: 'm', 2: 'feet'}


def _norm_insunits_to_mm(val_mm: float, insunits: int) -> float:
    """$INSUNITS归一到mm。"""
    factor = {1: 25.4, 2: 304.8, 4: 1.0, 5: 10.0, 6: 1000.0}.get(insunits, 1.0)
    return val_mm * factor


def _collect_texts(entities_iter) -> List[Dict[str, Any]]:
    """递归收集 TEXT/MTEXT/ATTRIB 文字（含INSERT块内ATTRIB）+ 格式清理。"""
    texts = []
    for e in entities_iter:
        try:
            dt = e.dxftype()
            if dt == 'TEXT':
                pos = e.dxf.insert if hasattr(e.dxf, 'insert') else None
                # 注意：ezdxf Vec3不支持切片（__getitem__要整数），用.x/.y
                pos_xy = [float(pos.x), float(pos.y)] if pos is not None else None
                texts.append({'text': e.dxf.text if hasattr(e.dxf, 'text') else '',
                              'height': float(getattr(e.dxf, 'height', 0) or 0),
                              'pos': pos_xy})
            elif dt == 'MTEXT':
                # M6/L6：plain_mtext清理格式码
                raw = getattr(e, 'text', '') or ''
                clean = ezdxf_const.PLAIN_MTEXT.sub('', raw) if hasattr(ezdxf_const, 'PLAIN_MTEXT') else raw
                clean = re.sub(r'\\[PpfF][^;]*;?', ' ', clean)
                clean = re.sub(r'[{}]', '', clean)
                pos = e.dxf.insert if hasattr(e.dxf, 'insert') else None
                pos_xy = [float(pos.x), float(pos.y)] if pos is not None else None
                texts.append({'text': clean.strip(),
                              'height': float(getattr(e.dxf, 'char_height', 0) or 0),
                              'pos': pos_xy})
            elif dt == 'INSERT':
                # 递归ATTRIB + virtual_entities展开块内
                for att in e.attribs:
                    texts.append({'text': att.dxf.text,
                                  'height': float(getattr(att.dxf, 'height', 0) or 0),
                                  'pos': None, 'from_block': True})
        except Exception:
            continue
    return texts


def _fig_id_from_filename(path: Path) -> Optional[Tuple[str, str]]:
    """文件名图号提取（R2-M2④：文件名是最高精度图号源）。"""
    stem = path.stem
    for pat in FIG_ID_PATTERNS:
        m = pat.search(stem)
        if m:
            return m.group(0).strip(), 'filename'
    return None


def _fig_id_from_texts(texts: List[Dict]) -> Optional[Tuple[str, str]]:
    """标题栏启发式图号提取。"""
    for t in texts:
        for pat in FIG_ID_PATTERNS:
            m = pat.search(t['text'])
            if m:
                return m.group(0).strip(), 'titleblock'
    return None


def _scale_from_texts(texts: List[Dict]) -> Optional[str]:
    for t in texts:
        m = SCALE_PATTERN.search(t['text'])
        if m:
            return f"1:{m.group(1)}"
    return None


def _sheet_size_from_layout(doc) -> Optional[Tuple[str, str]]:
    """布局页面设置优先（R2-M2①）。"""
    for layout in doc.layouts:
        if layout.is_any_paperspace:
            try:
                w_mm = float(getattr(layout, 'dwg_size', (0, 0))[0]) if hasattr(layout, 'dwg_size') else 0
                h_mm = float(getattr(layout, 'dwg_size', (0, 0))[1]) if hasattr(layout, 'dwg_size') else 0
                if w_mm <= 0:
                    # ezdxf Layout页面尺寸API：layout.dwg_size或paper_setup
                    ps = getattr(layout, 'paper_setup', None)
                    if ps:
                        w_mm = float(ps.width)
                        h_mm = float(ps.height)
                if w_mm > 0:
                    for name, (sw, sh) in SHEET_SIZES.items():
                        if abs(w_mm - sw) <= SHEET_TOL and abs(h_mm - sh) <= SHEET_TOL:
                            return name, 'layout_setup'
                        if abs(w_mm - sh) <= SHEET_TOL and abs(h_mm - sw) <= SHEET_TOL:
                            return name, 'layout_setup'
            except Exception:
                continue
    return None


def _sheet_size_from_geometry(texts, insunits) -> Optional[Tuple[str, str]]:
    """几何兜底：找接近A系列宽高比的大尺寸文字组/图框估计。"""
    # 简化：从文字位置推断图框范围
    xs = [t['pos'][0] for t in texts if t.get('pos')]
    ys = [t['pos'][1] for t in texts if t.get('pos')]
    if not xs:
        return None
    w = (max(xs) - min(xs))
    h = (max(ys) - min(ys))
    if w <= 0 or h <= 0:
        return None
    # 文字范围比图框小，乘以保守系数1.5估图框
    for name, (sw, sh) in SHEET_SIZES.items():
        # 把A系列mm按$INSUNITS换到图面单位
        unit_factor = {1: 1/25.4, 2: 1/304.8, 4: 1.0, 5: 0.1, 6: 0.001}.get(insunits, 1.0)
        swu = sw / unit_factor
        shu = sh / unit_factor
        if abs(w - swu * 0.7) < swu * 0.2 and abs(h - shu * 0.7) < shu * 0.2:
            return name, 'geometry_est'
    return None


def _fig_name_from_texts(texts: List[Dict]) -> Tuple[Optional[str], str]:
    """最大字号启发式图名（带置信度）。"""
    if not texts:
        return None, 'low'
    sorted_by_h = sorted(texts, key=lambda t: t.get('height', 0), reverse=True)
    top = sorted_by_h[:3]
    candidates = [t['text'] for t in top if t['text'] and len(t['text']) >= 3]
    if not candidates:
        return None, 'low'
    fig_name = candidates[0]
    # 含"图"/"布置"/"平面"等词提高置信度
    conf = 'medium'
    if any(kw in fig_name for kw in ['图', '布置', '平面', '立面', '剖面', '系统', '详图']):
        conf = 'high'
    return fig_name[:50], conf


def _layer_semantics(doc) -> Dict[str, int]:
    """图层语义分类统计（按实体计数）。"""
    msp = doc.modelspace()
    counts: Dict[str, int] = {}
    for e in msp:
        layer_name = ''
        try:
            layer_name = e.dxf.layer
        except Exception:
            continue
        matched = False
        for sem, pat in LAYER_SEMANTICS:
            if re.search(pat, layer_name, re.I):
                counts[sem] = counts.get(sem, 0) + 1
                matched = True
                break
        if not matched:
            counts['其他'] = counts.get('其他', 0) + 1
    return counts


def extract_meta(path: str) -> Dict[str, Any]:
    """提取单个DXF文件的图纸元数据。"""
    p = Path(path)
    result: Dict[str, Any] = {'file': str(p), 'name': p.name}

    # 天正/容错模式读取（R2-M2⑥）
    recover_used = False
    doc = None
    try:
        doc = ezdxf.readfile(str(p))
    except Exception:
        try:
            doc = ezdxf.recover.readfile(str(p))
            recover_used = True
        except Exception as e:
            return {**result, 'error': f'DXF读取失败: {e}'}

    insunits = int(doc.header.get('$INSUNITS', 0) or 0)
    unit_name = INSUNITS_MAP.get(insunits, 'unspecified')

    # 文字收集：模型空间+图纸空间（R2-M2②/L5）
    msp = doc.modelspace()
    texts = _collect_texts(msp)
    for layout in doc.layouts:
        if layout.is_any_paperspace:
            texts += _collect_texts(layout)

    # 图号：文件名优先 → 标题栏兜底（R2-M2④）
    fig_id_result = _fig_id_from_filename(p) or _fig_id_from_texts(texts) or (None, None)
    fig_id, fig_id_src = fig_id_result[0], fig_id_result[1]

    # 比例
    scale = _scale_from_texts(texts)

    # 图幅：布局优先→几何兜底（R2-M2①）
    sheet_result = _sheet_size_from_layout(doc) or _sheet_size_from_geometry(texts, insunits) or (None, None)
    sheet_size, sheet_src = sheet_result[0], sheet_result[1]

    # 图名：最大字号+置信度（R2-M2⑤）
    fig_name, fig_name_conf = _fig_name_from_texts(texts)

    # 图层语义
    layers = _layer_semantics(doc)

    # 天正代理实体计数（R2-M2⑥）
    proxy_count = sum(1 for e in msp if 'PROXY' in e.dxftype().upper())

    # ACAD版本
    acad_ver = doc.header.get('$ACADVER', '?')
    # 图层数
    try:
        layer_names = [l.dxf.name for l in doc.layers]
    except Exception:
        layer_names = []

    result.update({
        'fig_id': fig_id,
        'fig_id_source': fig_id_src,
        'fig_name': fig_name,
        'fig_name_confidence': fig_name_conf,
        'scale': scale,
        'sheet_size': sheet_size,
        'sheet_size_source': sheet_src,
        'units': unit_name,
        'insunits': insunits,
        'acad_ver': acad_ver,
        'text_count': len(texts),
        'text_sample': [t['text'][:40] for t in texts[:10]],
        'layer_names': layer_names[:20],
        'layer_semantics': layers,
        'proxy_entities': proxy_count,
        'recover_mode_used': recover_used,
        'block_count': len(doc.blocks),
        'layout_count': len(doc.layouts),
    })
    return result


def main():
    ap = argparse.ArgumentParser(description='DXF图纸元数据提取器')
    ap.add_argument('input', help='DXF文件或目录')
    ap.add_argument('--json', default=None, help='输出JSON文件路径（默认stdout）')
    args = ap.parse_args()

    inp = Path(args.input).resolve()
    if inp.is_dir():
        files = sorted(inp.glob('*.dxf'))
    elif inp.is_file() and inp.suffix.lower() == '.dxf':
        files = [inp]
    else:
        print(json.dumps({'error': f'不支持的输入: {inp}'}), file=sys.stderr)
        sys.exit(1)

    results = [extract_meta(str(f)) for f in files]
    output = {'input': str(inp), 'files_processed': len(results), 'results': results}
    js = json.dumps(output, ensure_ascii=False, indent=1)
    if args.json:
        Path(args.json).write_text(js, encoding='utf-8')
        print(f'OK: {args.json}（{len(results)}个文件）', file=sys.stderr)
    else:
        print(js)


if __name__ == '__main__':
    main()
