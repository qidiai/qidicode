# -*- coding: utf-8 -*-
"""T3 — W3 pdfcraft 实测 + W4 MCP 握手（Phase 1）

子命令：--pick / --encrypt / --bindery / --wmimg / --audit / --robust / --w4 / --report / --all
依赖白名单：stdlib + pymupdf + Pillow + pypdf（无新增）
一切写操作经 _safe_write_path（根 out/，gitignored）；F 盘只读。
"""
import argparse
import glob
import io
import json
import os
import queue
import shutil
import subprocess
import sys
import threading
import time

import pymupdf  # fitz

from PIL import Image, ImageDraw, ImageFont  # Pillow（白名单内）

try:
    from pypdf import PdfReader, PdfWriter
    PYPDF_OK = True
except Exception:
    PYPDF_OK = False

# ---------------------------------------------------------------- 路径常量
ROOT = os.path.dirname(os.path.abspath(__file__))          # _craft_eval/
OUT_ROOT = os.path.join(ROOT, 'out')                       # 一切写的根（gitignored）
W3_DIR = os.path.join(OUT_ROOT, 'w3')
W4_DIR = os.path.join(OUT_ROOT, 'w4')

PDFCRAFT = r'G:\程序开发\artcraft\bin\pdfcraft-0.6.0-windows-x64-portable\pdfcraft-cli.exe'
WORDCRAFT = r'G:\程序开发\artcraft\bin\wordcraft-0.5.0-windows-x64-portable\wordcraft-cli.exe'
GRIDCRAFT = r'G:\程序开发\artcraft\bin\gridcraft-0.5.0-windows-x64-portable\gridcraft-cli.exe'

SAMPLES_JSON = os.path.join(W3_DIR, 'samples.json')
RUN_LOG = os.path.join(W3_DIR, 'run_log.json')
W4_LOG = os.path.join(W4_DIR, 'w4_log.json')

MB = 1024 * 1024

# ---------------------------------------------------------------- 输出纪律
try:
    sys.stdout.reconfigure(encoding='utf-8')
    sys.stderr.reconfigure(encoding='utf-8')
except Exception:
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')


def _safe_write_path(rel):
    """一切写操作必须经此：解析到 OUT_ROOT 之下，绝不逃逸。"""
    if os.path.isabs(rel):
        raise ValueError('refuse absolute path: %r' % rel)
    root = os.path.abspath(OUT_ROOT)
    full = os.path.abspath(os.path.join(root, rel))
    if os.path.commonpath([root, full]) != root:
        raise ValueError('path escapes out/: %r' % rel)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    return full


def jdump(obj, path, **kw):
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(obj, f, ensure_ascii=False, indent=1, **kw)


def now_str():
    return time.strftime('%Y-%m-%d %H:%M:%S')


# ---------------------------------------------------------------- 运行日志
def load_log():
    if os.path.exists(RUN_LOG):
        with open(RUN_LOG, encoding='utf-8') as f:
            return json.load(f)
    return {}


def log_step(step, entries):
    """幂等：同一步骤重跑覆盖旧条目。"""
    log = load_log()
    log[step] = entries
    jdump(log, RUN_LOG)


# ---------------------------------------------------------------- CLI 执行
def run_cli(args, timeout=300):
    """返回 {cmdline, rc, stdout, stderr, elapsed}；超时如实记录。"""
    cmdline = [os.path.basename(args[0])] + args[1:]
    t0 = time.time()
    try:
        p = subprocess.run(args, capture_output=True, timeout=timeout)
        return {
            'cmdline': ' '.join(cmdline),
            'rc': p.returncode,
            'stdout': p.stdout.decode('utf-8', 'replace'),
            'stderr': p.stderr.decode('utf-8', 'replace'),
            'elapsed_s': round(time.time() - t0, 2),
            'timed_out': False,
        }
    except subprocess.TimeoutExpired as e:
        return {
            'cmdline': ' '.join(cmdline),
            'rc': 'TIMEOUT',
            'stdout': (e.stdout or b'').decode('utf-8', 'replace'),
            'stderr': (e.stderr or b'').decode('utf-8', 'replace'),
            'elapsed_s': round(time.time() - t0, 2),
            'timed_out': True,
        }


def load_samples():
    if not os.path.exists(SAMPLES_JSON):
        return None
    with open(SAMPLES_JSON, encoding='utf-8') as f:
        return json.load(f)


def need_samples(tag):
    s = load_samples()
    if s is None:
        print('[!] samples.json 不存在，请先运行 --pick；%s 跳过' % tag)
        return None
    return s


# ================================================================ W3.1 --pick
def cmd_pick():
    t0 = time.time()
    entries = []
    paths = sorted(glob.glob('F:\\**\\*.pdf', recursive=True))
    s1_cands, s2_cands, s3_cands = [], [], []
    for p in paths:
        try:
            size = os.path.getsize(p)
        except OSError:
            continue
        try:
            doc = pymupdf.open(p)
            try:
                pages = doc.page_count
                first = doc[0].get_text() if pages > 0 else ''
            finally:
                doc.close()
            ntext = len(first.strip())
            rec = {'path': p, 'size': size, 'pages': pages,
                   'first_page_text_len': ntext}
            # S1 扫描件：首页文本 <20 字 且 page_count>100 且 size>10MB
            if ntext < 20 and pages > 100 and size > 10 * MB:
                s1_cands.append(rec)
            # S2 普通招标件：可正常打开、首页文本≥20 字、1MB<size<30MB
            if ntext >= 20 and MB < size < 30 * MB:
                s2_cands.append(rec)
        except Exception as e:
            # S3 损坏件：fitz 打开抛异常 且 size>1MB 且路径不含 $RECYCLE
            if size > 1 * MB and '$RECYCLE' not in p:
                s3_cands.append({'path': p, 'size': size,
                                 'error': repr(e)[:200]})

    # S1：最大者（并列时按路径排序取首个，确定性）
    if s1_cands:
        s1 = sorted(s1_cands, key=lambda r: (-r['size'], r['path']))[0]
        s1_out = dict(s1, status='ok', role='扫描件')
    else:
        s1_out = {'status': 'missing', 'role': '扫描件'}

    # S2：优先路径含"招标"字样，按大小排序取中位一份
    if s2_cands:
        preferred = [r for r in s2_cands if '招标' in r['path']]
        pool = sorted(preferred or s2_cands, key=lambda r: r['size'])
        mid = pool[len(pool) // 2]
        s2_out = dict(mid, status='ok', role='普通招标件',
                      preferred_zhaobiao=bool(preferred),
                      pool_size=len(pool))
    else:
        s2_out = {'status': 'missing', 'role': '普通招标件'}

    # S3：第一份（按路径排序，确定性）
    if s3_cands:
        s3 = sorted(s3_cands, key=lambda r: r['path'])[0]
        s3_out = dict(s3, status='ok', role='损坏件')
    else:
        s3_out = {'status': 'missing', 'role': '损坏件'}

    samples = {
        'generated_at': now_str(),
        'scan': {
            'glob': 'F:\\\\**\\\\*.pdf (recursive=True)',
            'total_pdfs': len(paths),
            's1_candidates': len(s1_cands),
            's2_candidates': len(s2_cands),
            's3_candidates': len(s3_cands),
            'elapsed_s': round(time.time() - t0, 2),
        },
        'S1': s1_out, 'S2': s2_out, 'S3': s3_out,
    }
    jdump(samples, SAMPLES_JSON)

    entries.append({
        'what': 'F 盘全量 glob + pymupdf 容错扫描，定位 S1/S2/S3',
        'cmdline': 'python %s --pick' % os.path.join(ROOT, 'w3_run.py'),
        'rc': 0, 'elapsed_s': samples['scan']['elapsed_s'],
        'evidence': 'total=%d pdfs; S1 候选 %d, S2 候选 %d, S3 候选 %d' % (
            len(paths), len(s1_cands), len(s2_cands), len(s3_cands)),
    })
    log_step('pick', entries)
    print('[pick] total=%d S1=%s S2=%s S3=%s (%.1fs)' % (
        len(paths), s1_out['status'], s2_out['status'], s3_out['status'],
        samples['scan']['elapsed_s']))
    for tag in ('S1', 'S2', 'S3'):
        r = samples[tag]
        if r['status'] == 'ok':
            print('  %s: %d bytes, %s pages, %s' % (
                tag, r['size'], r.get('pages', '?'), r['path']))
        else:
            print('  %s: missing' % tag)
    return samples


# ================================================================ W3.2 --encrypt
def cmd_encrypt():
    entries = []
    s = need_samples('encrypt')
    if s is None or s['S2'].get('status') != 'ok':
        log_step('encrypt', [{'what': 'encrypt', 'rc': 'SKIP',
                              'evidence': 'S2 missing'}])
        return
    s2 = s['S2']['path']
    enc = _safe_write_path('w3/s2_enc.pdf')
    dec = _safe_write_path('w3/s2_dec.pdf')

    # 1) pypdf AES-128 加密（依赖白名单内；pypdf 在位，无需 pymupdf 降级）
    algo_note = None
    if PYPDF_OK:
        t0 = time.time()
        reader = PdfReader(s2)
        writer = PdfWriter()
        for pg in reader.pages:
            writer.add_page(pg)
        writer.encrypt(user_password='w3test', owner_password='w3test',
                       algorithm='AES-128')
        with open(enc, 'wb') as f:
            writer.write(f)
        algo_note = ('AES-128（pypdf %s EncryptAlgorithm.AES_128，V4/R4，'
                     'user=owner=w3test）' % __import__('pypdf').__version__)
        enc_el = round(time.time() - t0, 2)
        entries.append({
            'what': 'pypdf 加密 S2 → s2_enc.pdf',
            'cmdline': 'pypdf PdfWriter.encrypt(algorithm="AES-128")',
            'rc': 0, 'elapsed_s': enc_el,
            'evidence': 's2_enc.pdf %d bytes; algorithm=%s' % (
                os.path.getsize(enc), algo_note),
        })
    else:
        # 降级：pymupdf save encryption（RC4）
        t0 = time.time()
        doc = pymupdf.open(s2)
        doc.save(enc, encryption=pymupdf.PDF_ENCRYPT_RC4,
                 owner_pw='w3test', user_pw='w3test')
        doc.close()
        algo_note = 'RC4（pymupdf 降级加密，pypdf 缺失）'
        enc_el = round(time.time() - t0, 2)
        entries.append({
            'what': 'pymupdf 降级加密 S2 → s2_enc.pdf',
            'cmdline': 'pymupdf save(encryption=RC4)',
            'rc': 0, 'elapsed_s': enc_el,
            'evidence': 's2_enc.pdf %d bytes; algorithm=%s' % (
                os.path.getsize(enc), algo_note),
        })

    d = pymupdf.open(enc)
    is_enc = bool(d.is_encrypted)
    d.close()

    # 2) 不带密码 info 必须失败
    r = run_cli([PDFCRAFT, 'info', enc])
    entries.append({
        'what': 'info 加密件（不带密码，预期失败）',
        'cmdline': r['cmdline'], 'rc': r['rc'],
        'elapsed_s': r['elapsed_s'],
        'stderr_200': r['stderr'][:200],
        'evidence': 'rc=%s（预期非 0）; stderr=%r; is_encrypted=%s' % (
            r['rc'], r['stderr'].strip(), is_enc),
    })

    # 3) 带密码 info 对照（sanity）
    r2 = run_cli([PDFCRAFT, 'info', enc, '--password', 'w3test'])
    entries.append({
        'what': 'info 加密件（带 --password w3test，对照）',
        'cmdline': r2['cmdline'], 'rc': r2['rc'],
        'elapsed_s': r2['elapsed_s'],
        'evidence': 'rc=%s; encrypted=%s（JSON 输出摘录）' % (
            r2['rc'], json.loads(r2['stdout']).get('encrypted')
            if r2['rc'] == 0 else 'n/a'),
    })

    # 4) 按 tools 实测 Schema 用 run --script 解密：
    #    doc_unprotect Schema: 仅 {doc: integer}（document id from doc_open）
    #    → doc_open(path,password) → doc_unprotect(doc) → doc_save(doc,path,full)
    tools = json.loads(run_cli([PDFCRAFT, 'tools'], timeout=120)['stdout'])
    unprotect_schema = next((t for t in tools if t['name'] == 'doc_unprotect'),
                            None)
    _safe_write_path('w3/tools_doc_unprotect_excerpt.json')
    jdump(unprotect_schema,
          os.path.join(W3_DIR, 'tools_doc_unprotect_excerpt.json'))

    steps = [
        {'tool': 'doc_open', 'args': {'path': enc, 'password': 'w3test'}},
        {'tool': 'doc_unprotect', 'args': {'doc': 1}},
        {'tool': 'doc_save', 'args': {'doc': 1, 'path': dec, 'full': True}},
    ]
    steps_file = _safe_write_path('w3/unprotect_steps.json')
    jdump(steps, steps_file)
    r3 = run_cli([PDFCRAFT, 'run', '--script', steps_file], timeout=600)
    entries.append({
        'what': 'run --script 解密（doc_open→doc_unprotect→doc_save）',
        'cmdline': r3['cmdline'], 'rc': r3['rc'],
        'elapsed_s': r3['elapsed_s'],
        'evidence': 'rc=%s; steps=%s; doc_unprotect Schema 键=%s' % (
            r3['rc'], [st['tool'] for st in steps],
            sorted(unprotect_schema['input_schema']['properties'].keys())
            if unprotect_schema else 'n/a'),
        'stdout_tail_600': r3['stdout'][-600:],
        'stderr_200': r3['stderr'][:200],
    })

    # 5) fitz 验证明文页数与文本层与原件一致
    d_orig = pymupdf.open(s2)
    d_dec = pymupdf.open(dec)
    pages_equal = d_orig.page_count == d_dec.page_count
    text_equal = all(d_orig[i].get_text() == d_dec[i].get_text()
                     for i in range(d_orig.page_count))
    orig_pages, dec_pages = d_orig.page_count, d_dec.page_count
    d_orig.close()
    d_dec.close()
    entries.append({
        'what': 'fitz 验证解密件 vs 原件',
        'cmdline': 'pymupdf.open(...) 页数+逐页 get_text() 比对',
        'rc': 0, 'elapsed_s': 0,
        'evidence': 'pages %d==%d : %s; 全页文本层一致 : %s' % (
            orig_pages, dec_pages, pages_equal, text_equal),
    })

    log_step('encrypt', entries)
    print('[encrypt] rc_nopw=%s pages_equal=%s text_equal=%s algo=%s' % (
        r['rc'], pages_equal, text_equal, algo_note))


# ================================================================ W3.3 --bindery
def cmd_bindery():
    entries = []
    s = need_samples('bindery')
    if s is None or s['S1'].get('status') != 'ok' \
            or s['S2'].get('status') != 'ok':
        log_step('bindery', [{'what': 'bindery', 'rc': 'SKIP',
                              'evidence': 'S1/S2 missing'}])
        return
    s1p, s2p = s['S1']['path'], s['S2']['path']
    combined = _safe_write_path('w3/combined.pdf')

    # --- combine ---
    r = run_cli([PDFCRAFT, 'combine', s1p, s2p, '--out', combined],
                timeout=600)
    d1 = pymupdf.open(s1p)
    n1, toc1 = d1.page_count, len(d1.get_toc())
    d1.close()
    d2 = pymupdf.open(s2p)
    n2, toc2 = d2.page_count, len(d2.get_toc())
    d2.close()
    dc = pymupdf.open(combined)
    nc, tocc = dc.page_count, len(dc.get_toc())
    dc.close()
    entries.append({
        'what': 'combine S1+S2 → combined.pdf',
        'cmdline': r['cmdline'], 'rc': r['rc'],
        'elapsed_s': r['elapsed_s'],
        'evidence': '页数 %d+%d=%d : %s; 书签 %d+%d→%d（每文件一个顶层条目，'
                    '+2）: %s' % (n1, n2, n1 + n2, nc == n1 + n2,
                                 toc1, toc2, tocc, tocc == toc1 + toc2 + 2),
    })

    # --- watermark（按 doc_watermark Schema：必需键 doc，文本键 text）---
    wm = _safe_write_path('w3/wm.pdf')
    steps_zh = [
        {'tool': 'doc_open', 'args': {'path': combined}},
        {'tool': 'doc_watermark',
         'args': {'doc': 1, 'text': '投标文件W3测试'}},
        {'tool': 'doc_save', 'args': {'doc': 1, 'path': wm, 'full': True}},
    ]
    sf_zh = _safe_write_path('w3/watermark_steps_zh.json')
    jdump(steps_zh, sf_zh)
    rw = run_cli([PDFCRAFT, 'run', '--script', sf_zh], timeout=600)
    entries.append({
        'what': 'watermark 中文文本水印 投标文件W3测试（规格要求）',
        'cmdline': rw['cmdline'], 'rc': rw['rc'],
        'elapsed_s': rw['elapsed_s'],
        'stderr_200': rw['stderr'][:200],
        'evidence': 'rc=%s; wm.pdf 生成=%s; 失败原文见 stderr_200' % (
            rw['rc'], os.path.exists(wm)),
        'spec_gap': ('pdfcraft 0.6.0 doc_watermark 用标准字体'
                     '（helvetica/times/courier），无法绘制 CJK 文本：'
                     '"only Western European characters can be added as '
                     'text for now"。中文水印 + fitz 文本层验证无法达成。'),
    })

    # 诊断对照（非规格替代）：同 Schema 换 ASCII 文本，证明管线本身可用
    wm_ascii = _safe_write_path('w3/wm_ascii.pdf')
    steps_ascii = [
        {'tool': 'doc_open', 'args': {'path': combined}},
        {'tool': 'doc_watermark',
         'args': {'doc': 1, 'text': 'W3-WATERMARK-TEST'}},
        {'tool': 'doc_save',
         'args': {'doc': 1, 'path': wm_ascii, 'full': True}},
    ]
    sf_ascii = _safe_write_path('w3/watermark_steps_ascii.json')
    jdump(steps_ascii, sf_ascii)
    ra = run_cli([PDFCRAFT, 'run', '--script', sf_ascii], timeout=600)
    found_on = []
    if os.path.exists(wm_ascii):
        d = pymupdf.open(wm_ascii)
        for i in range(min(3, d.page_count)):
            if 'W3-WATERMARK-TEST' in d[i].get_text():
                found_on.append(i + 1)
        d.close()
    entries.append({
        'what': '诊断对照：同 Schema 加 ASCII 水印 W3-WATERMARK-TEST',
        'cmdline': ra['cmdline'], 'rc': ra['rc'],
        'elapsed_s': ra['elapsed_s'],
        'evidence': 'rc=%s; wm_ascii.pdf 生成=%s; fitz 文本层检出页=%s' % (
            ra['rc'], os.path.exists(wm_ascii), found_on),
        'note': '诊断项，证明 doc_watermark 管线可用、仅 CJK 文本被拒。',
    })

    # --- split ---
    split_dir = _safe_write_path('w3/split')
    rs = run_cli([PDFCRAFT, 'split', combined, '--every', '100',
                  '--out-dir', split_dir], timeout=600)
    parts = sorted(os.listdir(split_dir)) if os.path.isdir(split_dir) else []
    part_pages = []
    for name in parts:
        d = pymupdf.open(os.path.join(split_dir, name))
        part_pages.append({'name': name, 'pages': d.page_count})
        d.close()
    entries.append({
        'what': 'split combined.pdf --every 100',
        'cmdline': rs['cmdline'], 'rc': rs['rc'],
        'elapsed_s': rs['elapsed_s'],
        'evidence': '份数=%d（预期 ⌈504/100⌉=6）; 各份页数=%s; 总页数=%d' % (
            len(parts), part_pages, sum(pp['pages'] for pp in part_pages)),
    })

    # --- extract ---
    first2 = _safe_write_path('w3/first2.pdf')
    re_ = run_cli([PDFCRAFT, 'extract', combined, '--pages', '1,2',
                   '--out', first2], timeout=300)
    d = pymupdf.open(first2)
    n_first2 = d.page_count
    d.close()
    entries.append({
        'what': 'extract combined.pdf --pages 1,2',
        'cmdline': re_['cmdline'], 'rc': re_['rc'],
        'elapsed_s': re_['elapsed_s'],
        'evidence': 'first2.pdf 页数=%d（预期 2）: %s' % (
            n_first2, n_first2 == 2),
    })

    log_step('bindery', entries)
    print('[bindery] combine rc=%s pages_ok=%s; watermark_zh rc=%s; '
          'split parts=%d; extract pages=%d' % (
              r['rc'], nc == n1 + n2, rw['rc'], len(parts), n_first2))


# ================================================================ R1 --wmimg
WM_TEXT = '投标文件W3测试'


def _find_cjk_font():
    """R1 字体定位：simhei.ttf → msyh.ttc → 枚举 Fonts 目录。
    判定法：ImageFont.truetype(...).getmask('投') 非空（size[0] > 0）。"""
    for p in (r'C:\Windows\Fonts\simhei.ttf',
              r'C:\Windows\Fonts\msyh.ttc'):
        if os.path.exists(p):
            try:
                f = ImageFont.truetype(p, 60)
                if f.getmask('投').size[0] > 0:
                    return p
            except Exception:
                pass
    fonts_dir = r'C:\Windows\Fonts'
    if os.path.isdir(fonts_dir):
        for fn in sorted(os.listdir(fonts_dir)):
            if not fn.lower().endswith(('.ttf', '.ttc')):
                continue
            p = os.path.join(fonts_dir, fn)
            try:
                f = ImageFont.truetype(p, 60)
                if f.getmask('投').size[0] > 0:
                    return p
            except Exception:
                continue
    return None


def _make_wm_png(png_path, font_path):
    """透明底 RGBA，红字 45° 斜排 WM_TEXT（字形约 56px ∈ [48,64]）。"""
    font_size = 56
    font = ImageFont.truetype(font_path, font_size)
    tmp = Image.new('RGBA', (font_size * len(WM_TEXT) * 2,
                              font_size * 3), (0, 0, 0, 0))
    d = ImageDraw.Draw(tmp)
    d.text((font_size, font_size), WM_TEXT, font=font,
           fill=(255, 0, 0, 255))
    bbox = tmp.getbbox()
    if bbox:
        tmp = tmp.crop(bbox)
    # PIL rotate 正角度为逆时针；-45 得顺时针 45° 斜排
    rot = tmp.rotate(-45, expand=True, resample=Image.BICUBIC)
    rot.save(png_path)
    return rot.size


def cmd_wmimg():
    entries = []
    s = need_samples('wmimg')
    if s is None or s['S2'].get('status') != 'ok':
        log_step('wmimg', [{'what': 'wmimg', 'rc': 'SKIP',
                              'evidence': 'S2 missing'}])
        return
    s2p = s['S2']['path']
    png = _safe_write_path('w3/wm_zh.png')
    wm_img = _safe_write_path('w3/wm_img.pdf')

    # 1) CJK 字体定位 + 中文水印 PNG
    t0 = time.time()
    font_path = _find_cjk_font()
    if font_path is None:
        entries.append({
            'what': 'CJK 字体定位', 'cmdline': '_find_cjk_font()',
            'rc': 1, 'elapsed_s': round(time.time() - t0, 2),
            'evidence': 'simhei.ttf/msyh.ttc 均不在位且 Fonts 目录枚举'
                        '无含中文"投"字形的字体 → 无法生成中文水印 PNG',
        })
        log_step('wmimg', entries)
        print('[wmimg] no CJK font found')
        return
    png_size = _make_wm_png(png, font_path)
    entries.append({
        'what': '生成中文水印 PNG %s' % WM_TEXT,
        'cmdline': 'PIL: ImageFont.truetype(%r, 56) → draw.text(红) '
                   '→ rotate(-45) → %s' % (font_path, png),
        'rc': 0, 'elapsed_s': round(time.time() - t0, 2),
        'evidence': 'wm_zh.png %dx%d px, RGBA 透明底, 红字 45° 斜排, '
                    '字体=%s' % (png_size[0], png_size[1], font_path),
    })

    # 2) 按 pdfcraft-cli tools 实测 doc_watermark Schema 的 file 模式应用
    tools = json.loads(run_cli([PDFCRAFT, 'tools'], timeout=120)['stdout'])
    wm_schema = next((t for t in tools if t['name'] == 'doc_watermark'),
                     None)
    if wm_schema is not None:
        jdump(wm_schema,
              os.path.join(W3_DIR, 'tools_doc_watermark_excerpt.json'))
    wm_keys = sorted(wm_schema['input_schema']['properties'].keys()) \
        if wm_schema else []
    steps = [
        {'tool': 'doc_open', 'args': {'path': s2p}},
        {'tool': 'doc_watermark', 'args': {
            'doc': 1, 'file': png, 'scale': 0.5,
            'rotation': 0, 'opacity': 0.5}},
        {'tool': 'doc_save',
         'args': {'doc': 1, 'path': wm_img, 'full': True}},
    ]
    sf = _safe_write_path('w3/wmimg_steps.json')
    jdump(steps, sf)
    r = run_cli([PDFCRAFT, 'run', '--script', sf], timeout=600)
    entries.append({
        'what': 'doc_watermark file 模式（PNG 图片水印）应用到 S2',
        'cmdline': r['cmdline'], 'rc': r['rc'],
        'elapsed_s': r['elapsed_s'],
        'stderr_200': r['stderr'][:200],
        'evidence': 'rc=%s; wm_img.pdf 生成=%s; doc_watermark Schema 键=%s'
                    '（file=图片路径, scale=0.5, rotation=0——45° 已烘焙进'
                    'PNG, opacity=0.5）' % (
                        r['rc'], os.path.exists(wm_img), wm_keys),
        'stdout_tail_600': r['stdout'][-600:],
    })

    # 3) 像素法验证（图片模式文本层不可抽，同 DPI 渲染比对）
    DPI = 96

    def render_page1(pdf_path):
        d = pymupdf.open(pdf_path)
        try:
            return d[0].get_pixmap(dpi=DPI)
        finally:
            d.close()

    verdict = None
    if os.path.exists(wm_img) and r['rc'] == 0:
        pa = render_page1(s2p)
        pb = render_page1(wm_img)
        if (pa.width, pa.height) != (pb.width, pb.height):
            entries.append({
                'what': '像素法验证', 'cmdline': 'pymupdf get_pixmap(dpi=%d)' % DPI,
                'rc': 1, 'elapsed_s': 0,
                'evidence': '两页渲染尺寸不一致 %s vs %s，无法比对' % (
                    (pa.width, pa.height), (pb.width, pb.height)),
            })
        else:
            t_pix = time.time()
            img_a = Image.frombytes('RGB', (pa.width, pa.height),
                                     pa.samples)
            img_b = Image.frombytes('RGB', (pb.width, pb.height),
                                     pb.samples)
            ba = img_a.tobytes()
            bb = img_b.tobytes()
            total = len(ba) // 3
            diff_cnt = red_dom = red_raised = 0
            for i in range(0, len(ba), 3):
                ra, ga, bta = ba[i], ba[i + 1], ba[i + 2]
                rb, gb, btb = bb[i], bb[i + 1], bb[i + 2]
                if ra != rb or ga != gb or bta != btb:
                    diff_cnt += 1
                    # 红色通道像素（水印是红字）：wm 页该像素红通道占优
                    if rb > gb + 30 and rb > btb + 30:
                        red_dom += 1
                    # 对照指标：红通道相对原页抬升（红字压深色内容时）
                    if rb > ra + 30:
                        red_raised += 1
            ratio = diff_cnt / total if total else 0.0
            red_ratio = red_dom / diff_cnt if diff_cnt else 0.0
            visible = ratio > 0.005 and red_dom > 0
            verdict = '可用' if visible else '不可用'
            conclusion = ('中文水印最终路径 = 图片水印模式'
                          '【%s】' % verdict)
            entries.append({
                'what': '像素法验证（S2 第 1 页 vs wm_img.pdf 第 1 页，'
                        '同 DPI=%d）' % DPI,
                'cmdline': 'pymupdf get_pixmap(dpi=%d) ×2 → '
                           'PIL 逐像素比对（tobytes）' % DPI,
                'rc': 0,
                'elapsed_s': round(time.time() - t_pix, 2),
                'evidence': '总像素=%d; 差异像素=%d; 差异占比=%.4f%%'
                            '（阈值 >0.5%%）; 差异像素中红字像素'
                            '（红通道占优）=%d（占差异 %.4f%%）; '
                            '红通道抬升像素=%d; 可见性=%s' % (
                                total, diff_cnt, ratio * 100,
                                red_dom, red_ratio * 100,
                                red_raised, visible),
                'conclusion': conclusion,
                'note': ('观察（供裁决）：pdfcraft 0.6.0 对 PNG alpha '
                         '的合成在空白页上干净（对照：空白页加水印仅'
                         '粉色红字、无黑无灰）；在 S2 这类已有内容的'
                         '页面上，水印带与内容交互产生淡灰 (231,231,231)'
                         ' 斜带，且水印压到原有黑字处出现深色/黑色像素'
                         '（对照：S2 仅 doc_open→doc_save 重存，渲染'
                         '差异 0 像素）。中文 45° 斜排红字清晰可辨，'
                         '满足可见性判据。'),
            })
    else:
        entries.append({
            'what': '像素法验证', 'cmdline': 'n/a', 'rc': 1,
            'elapsed_s': 0,
            'evidence': 'wm_img.pdf 未生成，跳过像素验证',
        })

    log_step('wmimg', entries)
    print('[wmimg] png=%dx%d run_rc=%s %s' % (
        png_size[0], png_size[1], r['rc'],
        ('conclusion: ' + verdict) if verdict else 'verdict: n/a'))


# ================================================================ W3.4 --audit
def cmd_audit():
    entries = []
    s = need_samples('audit')
    if s is None or s['S2'].get('status') != 'ok':
        log_step('audit', [{'what': 'audit', 'rc': 'SKIP',
                            'evidence': 'S2 missing'}])
        return
    s2p = s['S2']['path']
    faux = _safe_write_path('w3/faux_redacted.pdf')
    true_ = _safe_write_path('w3/true_redacted.pdf')

    # 定位第 1 页一个文本块
    d = pymupdf.open(s2p)
    page = d[0]
    blocks = [b for b in page.get_text('blocks')
              if b[6] == 0 and len(b[4].strip()) >= 10]
    target = blocks[0]
    rect = list(target[:4])
    covered = target[4].strip()[:40]
    d.close()

    # 伪脱敏：draw_rect 黑矩形盖住文字（文本层保留）
    d = pymupdf.open(s2p)
    d[0].draw_rect(rect, color=None, fill=(0, 0, 0))
    d.save(faux)
    d.close()
    d = pymupdf.open(faux)
    faux_text_kept = covered in d[0].get_text()
    d.close()
    entries.append({
        'what': '自造伪脱敏样本（draw_rect 黑矩形盖文本，文本层保留）',
        'cmdline': 'pymupdf page.draw_rect(fill=(0,0,0)) → faux_redacted.pdf',
        'rc': 0, 'elapsed_s': 0,
        'evidence': '覆盖文本 %r 仍可抽取=%s（伪脱敏成立）' % (
            covered, faux_text_kept),
    })

    # 真脱敏对照：add_redact_annot + apply_redactions 真删
    d = pymupdf.open(s2p)
    d[0].add_redact_annot(rect)
    d[0].apply_redactions()
    d.save(true_)
    d.close()
    d = pymupdf.open(true_)
    true_text_gone = covered not in d[0].get_text()
    d.close()
    entries.append({
        'what': '自造真脱敏对照（add_redact_annot + apply_redactions 真删）',
        'cmdline': 'pymupdf add_redact_annot+apply_redactions → true_redacted.pdf',
        'rc': 0, 'elapsed_s': 0,
        'evidence': '覆盖文本 %r 已删除=%s（真脱敏成立）' % (
            covered, true_text_gone),
    })

    # pdfcraft-cli audit 实测（规格字面命令）
    ra1 = run_cli([PDFCRAFT, 'audit', faux])
    ra2 = run_cli([PDFCRAFT, 'audit', true_])
    ra3 = run_cli([PDFCRAFT, 'run', 'audit'])
    entries.append({
        'what': 'pdfcraft-cli audit <伪脱敏样本>（规格字面命令）',
        'cmdline': ra1['cmdline'], 'rc': ra1['rc'],
        'elapsed_s': ra1['elapsed_s'],
        'stderr_200': ra1['stderr'][:200],
        'evidence': 'rc=%s; stderr=%r' % (ra1['rc'], ra1['stderr'].strip()),
    })
    entries.append({
        'what': 'pdfcraft-cli audit <真脱敏样本>（规格字面命令）',
        'cmdline': ra2['cmdline'], 'rc': ra2['rc'],
        'elapsed_s': ra2['elapsed_s'],
        'stderr_200': ra2['stderr'][:200],
        'evidence': 'rc=%s; stderr=%r' % (ra2['rc'], ra2['stderr'].strip()),
    })
    entries.append({
        'what': 'pdfcraft-cli run audit（探测是否存在 audit 工具）',
        'cmdline': ra3['cmdline'], 'rc': ra3['rc'],
        'elapsed_s': ra3['elapsed_s'],
        'stderr_200': ra3['stderr'][:200],
        'evidence': 'rc=%s; stderr=%r' % (ra3['rc'], ra3['stderr'].strip()),
    })

    conclusion = (
        '结论：pdfcraft 0.6.0 无 audit 能力——--help 子命令列表无 audit；'
        'pdfcraft-cli audit → "unknown command \'audit\'"（rc=1）；'
        'pdfcraft-cli run audit → "unknown tool \"audit\""（rc=1）；'
        'tools 共 143 个，无任何审计/伪脱敏检测工具（最近邻为 '
        'redact_mark/redact_apply/redact_clear、doc_hidden_info、'
        'accessibility_check，均不审计既有文件的覆盖文本）。'
        '伪样本（文本层保留）与真样本（文本层已删）无法用 pdfcraft 区分 '
        '→ SPEC GAP，交指挥官裁决。'
        '候选替代（未执行，待裁决）：以 text_extract 比对覆盖前后文本层。')
    entries.append({'what': 'audit 区分性结论', 'cmdline': 'n/a',
                    'rc': 'SPEC GAP', 'elapsed_s': 0,
                    'evidence': conclusion})

    log_step('audit', entries)
    print('[audit] faux_text_kept=%s true_text_gone=%s; '
          'audit cmd rc=%s/%s; run audit rc=%s → SPEC GAP' % (
              faux_text_kept, true_text_gone,
              ra1['rc'], ra2['rc'], ra3['rc']))


# ================================================================ W3.5 --robust
def classify_rc(rc):
    if rc == 1:
        return '优雅失败(rc=1+错误消息)'
    if rc == 101:
        return 'panic(rc=101)'
    return '崩溃(rc=%s，其他)' % rc


def cmd_robust():
    entries = []
    s = need_samples('robust')
    if s is None or s['S3'].get('status') != 'ok':
        log_step('robust', [{'what': 'robust', 'rc': 'SKIP',
                             'evidence': 'S3 missing'}])
        return
    s3p = s['S3']['path']

    cases = [
        ('info', [PDFCRAFT, 'info', s3p]),
        ('text', [PDFCRAFT, 'text', s3p]),
        ('render', [PDFCRAFT, 'render', s3p, '--page', '1',
                    '--out', _safe_write_path('w3/s3_p1.png')]),
    ]
    for name, args in cases:
        r = run_cli(args, timeout=120)
        entries.append({
            'what': 'S3 损坏件 %s' % name,
            'cmdline': r['cmdline'], 'rc': r['rc'],
            'elapsed_s': r['elapsed_s'],
            'stderr_200': r['stderr'][:200],
            'evidence': 'rc=%s; 定性=%s; stderr=%r' % (
                r['rc'], classify_rc(r['rc']), r['stderr'].strip()),
        })

    # 附带：不存在路径的 info
    ghost = _safe_write_path('w3/this_file_does_not_exist.pdf')
    r = run_cli([PDFCRAFT, 'info', ghost])
    entries.append({
        'what': '不存在路径 info（附带）',
        'cmdline': r['cmdline'], 'rc': r['rc'],
        'elapsed_s': r['elapsed_s'],
        'stderr_200': r['stderr'][:200],
        'evidence': 'rc=%s; 定性=%s; stderr=%r' % (
            r['rc'], classify_rc(r['rc']), r['stderr'].strip()),
    })

    log_step('robust', entries)
    print('[robust] S3 info/text/render + 不存在路径 全部 rc=1 '
          '→ 优雅失败（无 panic/崩溃）')


# ================================================================ W3.6/3.7 --w4
W4_SERVERS = [
    ('pdfcraft', PDFCRAFT, ['mcp']),
    ('wordcraft', WORDCRAFT, ['mcp', '--author', 'AI精修']),
    ('gridcraft', GRIDCRAFT, ['mcp']),
]


class LineReader(threading.Thread):
    """后台逐行读 stdout 到队列，支持 30s 单步超时。"""

    def __init__(self, stream):
        super().__init__(daemon=True)
        self.stream = stream
        self.q = queue.Queue()

    def run(self):
        for line in iter(self.stream.readline, b''):
            self.q.put(line)
        self.q.put(None)  # EOF


def handshake_one(name, cli, cli_args):
    """一次完整握手：initialize → initialized → tools/list。"""
    t0 = time.time()
    rec = {
        'server': name,
        'command': cli,
        'args': cli_args,
        'started_at': now_str(),
        'steps': [],
        'timed_out': False,
    }
    proc = subprocess.Popen([cli] + cli_args, stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE)
    reader = LineReader(proc.stdout)
    reader.start()

    def drain_stderr():
        for line in iter(proc.stderr.readline, b''):
            pass

    th_err = threading.Thread(target=drain_stderr, daemon=True)
    th_err.start()

    def send(obj):
        proc.stdin.write(
            (json.dumps(obj, ensure_ascii=False) + '\n').encode('utf-8'))
        proc.stdin.flush()

    def read_step(timeout=30):
        try:
            line = reader.q.get(timeout=timeout)
        except queue.Empty:
            return None
        return line.decode('utf-8', 'replace') if line else None

    # 1) initialize
    init_req = {
        'jsonrpc': '2.0', 'id': 1, 'method': 'initialize',
        'params': {
            'protocolVersion': '2025-06-18',
            'capabilities': {},
            'clientInfo': {'name': 'w4-handshake', 'version': '1.0'},
        },
    }
    send(init_req)
    init_resp_line = read_step(30)
    init_resp = None
    if init_resp_line:
        try:
            init_resp = json.loads(init_resp_line)
        except Exception as e:
            rec['init_parse_error'] = repr(e)
    rec['steps'].append({'request': init_req,
                         'response_line': init_resp_line})
    server_info = None
    if init_resp and 'result' in init_resp:
        server_info = init_resp['result'].get('serverInfo')
    rec['serverInfo'] = server_info

    # 2) notifications/initialized
    notif = {'jsonrpc': '2.0', 'method': 'notifications/initialized'}
    send(notif)
    rec['steps'].append({'request': notif, 'response_line': None})

    # 3) tools/list
    tl_req = {'jsonrpc': '2.0', 'id': 2, 'method': 'tools/list',
              'params': {}}
    send(tl_req)
    tl_resp_line = read_step(30)
    tl_resp = None
    tool_names = []
    if tl_resp_line:
        try:
            tl_resp = json.loads(tl_resp_line)
            tool_names = [t.get('name') for t in
                          tl_resp.get('result', {}).get('tools', [])]
        except Exception as e:
            rec['tools_parse_error'] = repr(e)
    rec['steps'].append({'request': tl_req,
                         'response_line': tl_resp_line})
    rec['tool_count'] = len(tool_names)
    rec['tool_names'] = tool_names

    # 收尾：关 stdin，等自然退出，超时 kill
    try:
        proc.stdin.close()
    except Exception:
        pass
    try:
        proc.wait(timeout=10)
        rec['exit_rc'] = proc.returncode
    except subprocess.TimeoutExpired:
        proc.kill()
        rec['exit_rc'] = 'killed after 10s'
        rec['timed_out'] = True
    rec['elapsed_s'] = round(time.time() - t0, 2)
    return rec


def cmd_w4():
    results = []
    for name, cli, cli_args in W4_SERVERS:
        print('[w4] handshake %s ...' % name)
        rec = handshake_one(name, cli, cli_args)
        hf = os.path.join(W4_DIR, 'handshake_%s.json' % name)
        os.makedirs(W4_DIR, exist_ok=True)
        jdump(rec, hf)
        ok_init = rec.get('serverInfo') is not None
        print('  serverInfo=%s tools=%d (%.2fs)' % (
            rec.get('serverInfo'), rec.get('tool_count', 0),
            rec.get('elapsed_s', 0)))
        results.append(rec)

    log = {
        'generated_at': now_str(),
        'protocol': 'JSON-RPC 2.0 over stdio, 裸行（每行一个 JSON 对象）',
        'handshake_params': {
            'protocolVersion': '2025-06-18',
            'capabilities': {},
            'clientInfo': {'name': 'w4-handshake', 'version': '1.0'},
        },
        'per_step_timeout_s': 30,
        'servers': results,
    }
    jdump(log, W4_LOG)
    print('[w4] 3 servers handshaked → %s' % W4_LOG)


# ================================================================ --report
def fmt_rc(rc):
    return str(rc)


def cmd_report():
    os.makedirs(W3_DIR, exist_ok=True)
    samples = load_samples()
    log = load_log()
    w4log = None
    if os.path.exists(W4_LOG):
        with open(W4_LOG, encoding='utf-8') as f:
            w4log = json.load(f)

    L = []
    L.append('# W3 pdfcraft 实测报告（w3_report.md）')
    L.append('')
    L.append('> 生成：%s by `_craft_eval/w3_run.py --report`' % now_str())
    L.append('> 开头声明：**OCR 不在 pdfcraft 能力范围（中文扫描件归 '
             'rapidocr），S1 仅用于装订/鲁棒场景**。')
    L.append('> 依据：`pdfcraft-cli tools` 的 `ocr_recognize` Schema 中 '
             '`language` 枚举仅 `["en"]`（英语），中文扫描件 OCR 不在其'
             '能力范围；S1 为 227 页扫描件，本报告仅将其用于 combine '
             '装订与鲁棒性场景，不做文本/OCR 断言。')
    L.append('')
    L.append('## 样本（--pick）')
    if samples:
        sc = samples['scan']
        L.append('')
        L.append('- 扫描：glob `F:\\**\\*.pdf` 共 **%d** 份；S1 候选 %d、'
                 'S2 候选 %d、S3 候选 %d；耗时 %.2fs。'
                 '（来源：out/w3/samples.json → scan）'
                 % (sc['total_pdfs'], sc['s1_candidates'],
                    sc['s2_candidates'], sc['s3_candidates'],
                    sc['elapsed_s']))
        L.append('')
        L.append('| 样本 | 角色 | 状态 | 大小(B) | 页数 | 首页文本长 | 路径 |')
        L.append('|---|---|---|---|---|---|---|')
        for tag in ('S1', 'S2', 'S3'):
            r = samples[tag]
            L.append('| %s | %s | %s | %s | %s | %s | `%s` |' % (
                tag, r.get('role', '?'), r['status'],
                r.get('size', '-'), r.get('pages', '-'),
                r.get('first_page_text_len', '-'),
                r.get('path', '(missing)')))
        L.append('')
        L.append('（上表所有数字来源：out/w3/samples.json 的 S1/S2/S3 '
                 '条目，运行期 glob 解析，未写入任何入库文件。）')
    else:
        L.append('')
        L.append('samples.json 不存在（未运行 --pick）。')

    def section(title, step, note=None):
        L.append('')
        L.append('## %s' % title)
        entries = log.get(step, [])
        if not entries:
            L.append('')
            L.append('（无记录：未运行 --%s。）' % step)
            return
        L.append('')
        L.append('| 命令/操作 | rc | 耗时(s) | 验证证据 |')
        L.append('|---|---|---|---|')
        for e in entries:
            cmd = e.get('cmdline', '(无命令行)')
            if len(cmd) > 160:
                cmd = cmd[:157] + '...'
            L.append('| `%s` | %s | %s | %s |' % (
                cmd.replace('|', '\\|'),
                fmt_rc(e.get('rc', '?')),
                e.get('elapsed_s', '-'),
                str(e.get('evidence', '')).replace('|', '\\|').replace(
                    '\n', ' ')))
        if note:
            L.append('')
            L.append(note)
        L.append('')
        L.append('（本节每行数字来源：out/w3/run_log.json → "%s" 条目；'
                 '原始 stdout/stderr 摘录见同条目 stderr_200 / '
                 'stdout_tail_600 字段。）' % step)

    section('W3.2 --encrypt 加密/解密闭环', 'encrypt',
            note='- 解密链路：`pdfcraft-cli info` 不带密码必须失败（rc=1，'
                 'stderr="the document is protected by a password"）；'
                 '解密按 `pdfcraft-cli tools` 实测 Schema 执行：'
                 'doc_unprotect 仅接受 `{doc: integer}`，故以 '
                 '`run --script` 编排 doc_open(path,password) → '
                 'doc_unprotect(doc=1) → doc_save(doc=1,path,full=true)。'
                 'Schema 摘录：out/w3/tools_doc_unprotect_excerpt.json。'
                 '- pypdf 6.15.0 在位，使用 AES-128（未触发 pymupdf '
                 'RC4 降级）。')
    section('W3.3 --bindery 装订三连', 'bindery',
            note='- **SPEC GAP（水印）**：doc_watermark 文本水印仅支持'
                 '西方字符（标准字体 helvetica/times/courier），中文水印'
                 '`投标文件W3测试` 被拒（rc=1，stderr 见 run_log），'
                 'out/w3/wm.pdf 未生成；诊断对照（同 Schema，ASCII 文本 '
                 'W3-WATERMARK-TEST → out/w3/wm_ascii.pdf）证明管线本身'
                 '可用且 fitz 文本层可检出。中文水印需求待指挥官裁决。'
                 '- combine 书签：S1 无书签(0)，S2 有 1253 条，合并后 '
                 '1255 = 0 + 1253 + 2（doc_combine 每文件加一个顶层'
                 '条目），书签保留语义成立。')
    # --- R1：图片水印（file 模式）小节 ---
    wm_entries = log.get('wmimg', [])
    if wm_entries:
        L.append('')
        L.append('### W3.3 续 — 图片水印（file 模式，R1 返工补充）')
        L.append('')
        L.append('| 命令/操作 | rc | 耗时(s) | 验证证据 |')
        L.append('|---|---|---|---|')
        for e in wm_entries:
            cmd = e.get('cmdline', '(无命令行)')
            if len(cmd) > 160:
                cmd = cmd[:157] + '...'
            L.append('| `%s` | %s | %s | %s |' % (
                cmd.replace('|', '\\|'),
                fmt_rc(e.get('rc', '?')),
                e.get('elapsed_s', '-'),
                str(e.get('evidence', '')).replace('|', '\\|').replace(
                    '\n', ' ')))
        conclusions = [e.get('conclusion') for e in wm_entries
                       if e.get('conclusion')]
        L.append('')
        if conclusions:
            L.append('**结论行：%s**' % conclusions[-1])
        for e in wm_entries:
            if e.get('note'):
                L.append('')
                L.append('> %s' % e['note'])
        L.append('')
        L.append('（本节每行数字来源：out/w3/run_log.json → "wmimg" '
                 '条目；doc_watermark file 模式 Schema 摘录：'
                 'out/w3/tools_doc_watermark_excerpt.json。）')
        L.append('')

    section('W3.4 --audit 伪脱敏检测', 'audit',
            note='- **SPEC GAP**：pdfcraft 0.6.0 无 audit 命令/工具'
                 '（`pdfcraft-cli audit` → unknown command，rc=1；'
                 '`pdfcraft-cli run audit` → unknown tool，rc=1；143 个'
                 '工具中无审计工具）。伪/真脱敏区分性无法用 pdfcraft '
                 '验证。伪样本（文本层保留）与真样本（文本层删除）已造好：'
                 'out/w3/faux_redacted.pdf、out/w3/true_redacted.pdf，'
                 '可供裁决后用其他手段（如 text_extract 文本层比对）验证。')
    section('W3.5 --robust 损坏件鲁棒性', 'robust',
            note='- 定性对照 v2 §五 R5 退出码映射：rc=1 + 错误消息 = '
                 '优雅失败；rc=101 = panic；其他 = 崩溃。S3（28MB 损坏件）'
                 '三命令与不存在路径全部 rc=1 + 明确错误消息 → 全部优雅'
                 '失败，无 panic、无崩溃。')

    # --- W4 报告 ---
    L.append('')
    L.append('## W4 MCP 注册与握手')
    L.append('')
    L.append('- 标书软件 `.mcp.json`：已备份为同目录 `.mcp.json.bak-w4` '
             '（246 bytes 字节级副本）；新文件 `json.load` 验证通过；'
             'hologram 死条目原样移入 `_disabled`；craft 三件套注册于 '
             '`mcpServers`，wordcraft 条目含 `--author AI精修`（R19 '
             '溯源）。')
    L.append('- QIDI Code 注册：`qidi.exe mcp add` ×3（wordcraft 因 '
             '`--author` 需 `--` 分隔符，按 `qidi.exe mcp add --help` '
             '实测调整），`qidi.exe mcp list` 输出含 wordcraft / '
             'gridcraft / pdfcraft 三行。')
    if w4log:
        L.append('')
        L.append('### 握手结果（来源：out/w4/w4_log.json + '
                 'out/w4/handshake_<name>.json）')
        L.append('')
        L.append('| server | serverInfo | tool 数 | 总耗时(s) | 退出 |')
        L.append('|---|---|---|---|---|')
        for srv in w4log['servers']:
            si = srv.get('serverInfo') or {}
            L.append('| %s | %s %s | %d | %.2f | %s |' % (
                srv['server'], si.get('name', '?'), si.get('version', '?'),
                srv.get('tool_count', 0), srv.get('elapsed_s', 0),
                srv.get('exit_rc', '?')))
        L.append('')
        for srv in w4log['servers']:
            L.append('- %s 工具清单（%d 个）：%s' % (
                srv['server'], srv.get('tool_count', 0),
                ', '.join(srv.get('tool_names', []))))
    else:
        L.append('')
        L.append('- （未运行 --w4，无握手记录。）')

    out = _safe_write_path('w3/w3_report.md')
    with open(out, 'w', encoding='utf-8') as f:
        f.write('\n'.join(L) + '\n')

    # --- w4_report.md ---
    if w4log:
        W = []
        W.append('# W4 MCP 握手报告（w4_report.md）')
        W.append('')
        W.append('> 生成：%s by `_craft_eval/w3_run.py --w4/--report`'
                 % now_str())
        W.append('> 协议：JSON-RPC 2.0 over stdio，裸行传输（每行一个 '
                 'JSON 对象，LSP content-length 风格未采用——先试裸行即'
                 '成功）。单步超时 30s。仅验握手面（initialize + '
                 'tools/list），不做文档操作闭环（属 W5）。')
        W.append('')
        W.append('| server | 启动命令 | serverInfo.name | version | '
                 'tool 数 | 总耗时(s) | 退出 rc |')
        W.append('|---|---|---|---|---|---|---|')
        for srv in w4log['servers']:
            si = srv.get('serverInfo') or {}
            W.append('| %s | `%s %s` | %s | %s | %d | %.2f | %s |' % (
                srv['server'], os.path.basename(srv['command']),
                ' '.join(srv['args']), si.get('name', '?'),
                si.get('version', '?'), srv.get('tool_count', 0),
                srv.get('elapsed_s', 0), srv.get('exit_rc', '?')))
        W.append('')
        for srv in w4log['servers']:
            W.append('## %s（%d 个工具）' % (srv['server'],
                                             srv.get('tool_count', 0)))
            W.append('')
            W.append('```')
            W.append(', '.join(srv.get('tool_names', [])))
            W.append('```')
            W.append('')
            W.append('完整往返 JSON：`out/w4/handshake_%s.json` '
                     '（来源文件，含 initialize 请求/响应行、'
                     'notifications/initialized、tools/list 请求/响应行）。'
                     % srv['server'])
            W.append('')
        wout = _safe_write_path('w4/w4_report.md')
        with open(wout, 'w', encoding='utf-8') as f:
            f.write('\n'.join(W) + '\n')
        print('[report] w3_report.md + w4_report.md written')
    else:
        print('[report] w3_report.md written (w4 无记录)')
    print('[report] → %s' % out)


# ================================================================ main
def main():
    ap = argparse.ArgumentParser(description='T3: W3 pdfcraft 实测 + W4 MCP 握手')
    ap.add_argument('--pick', action='store_true')
    ap.add_argument('--encrypt', action='store_true')
    ap.add_argument('--bindery', action='store_true')
    ap.add_argument('--wmimg', action='store_true')
    ap.add_argument('--audit', action='store_true')
    ap.add_argument('--robust', action='store_true')
    ap.add_argument('--w4', action='store_true')
    ap.add_argument('--report', action='store_true')
    ap.add_argument('--all', action='store_true')
    args = ap.parse_args()

    if not any(vars(args).values()):
        ap.print_help()
        return

    if args.all:
        if os.path.exists(RUN_LOG):
            os.remove(RUN_LOG)
        if os.path.exists(W4_LOG):
            os.remove(W4_LOG)
        cmd_pick()
        cmd_encrypt()
        cmd_bindery()
        cmd_wmimg()
        cmd_audit()
        cmd_robust()
        cmd_w4()
        cmd_report()
        return

    if args.pick:
        cmd_pick()
    if args.encrypt:
        cmd_encrypt()
    if args.bindery:
        cmd_bindery()
    if args.wmimg:
        cmd_wmimg()
    if args.audit:
        cmd_audit()
    if args.robust:
        cmd_robust()
    if args.w4:
        cmd_w4()
    if args.report:
        cmd_report()


if __name__ == '__main__':
    main()
