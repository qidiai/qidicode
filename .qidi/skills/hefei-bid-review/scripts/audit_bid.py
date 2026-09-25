#!/usr/bin/env python3
"""
合肥技术标审计脚本 — 通用版
用法: python audit_bid.py <docx路径> [关键词JSON(保留参数,当前未使用)] [招标品牌对照txt]

输出结构化审计报告到 stdout。
"""
import sys
import json
import os
import io
import re

sys.stdout.reconfigure(encoding='utf-8')

try:
    from docx import Document
    from docx.shared import Pt
    from docx.oxml.ns import qn
except ImportError:
    print("ERROR: python-docx not installed. Run: pip install python-docx")
    sys.exit(1)


def _make_table_text_getter(doc):
    cache = []
    for t in doc.tables:
        for row in t.rows:
            for cell in row.cells:
                cache.append(cell.text)
    blob = '\n'.join(cache)
    return lambda: blob


def audit_docx(docx_path, keywords_file=None, brands_file=None):
    """执行技术标审计，返回结构化报告。"""
    if not os.path.exists(docx_path):
        print(f"ERROR: File not found: {docx_path}")
        sys.exit(1)

    doc = Document(docx_path)
    all_text = '\n'.join(p.text for p in doc.paragraphs)
    _tta = _make_table_text_getter(doc)
    table_text_all = _tta

    # Table text
    table_chars = 0
    for t in doc.tables:
        for row in t.rows:
            for cell in row.cells:
                for p in cell.paragraphs:
                    table_chars += len(p.text)

    para_chars = sum(len(p.text) for p in doc.paragraphs)
    total_chars = para_chars + table_chars

    print("=" * 70)
    print(f"合肥技术标审计报告")
    print(f"文件: {os.path.basename(docx_path)}")
    print(f"大小: {os.path.getsize(docx_path) / 1024:.1f} KB")
    print(f"段落数: {len(doc.paragraphs)} | 表格数: {len(doc.tables)}")
    print(f"段落字符: {para_chars} | 表格字符: {table_chars} | 总字符: {total_chars}")
    print("=" * 70)

    # --- 1. 图片检查 ---
    image_count = 0
    for rel in doc.part.rels:
        if 'image' in doc.part.rels[rel].reltype:
            image_count += 1
    print(f"\n{'='*20} 1. 图片检查 {'='*20}")
    print(f"图片数量: {image_count}")
    if image_count >= 2:
        print("  [OK] >= 2 张（横道图+总平面图）")
    elif image_count == 1:
        print("  [WARN] 仅 1 张，可能缺横道图或总平面图")
    else:
        print("  [FAIL] 无图片，AI 不可见但评委需要")

    # --- 2. 页码检查 ---
    has_page_number = False
    for section in doc.sections:
        footer_xml = section.footer._element.xml
        if 'PAGE' in footer_xml or 'fldChar' in footer_xml:
            has_page_number = True
    print(f"\n{'='*20} 2. 页码检查 {'='*20}")
    print(f"页码: {'有 [OK]' if has_page_number else '无 [FAIL]'}")

    # --- 3. 评分索引表 ---
    has_score_index = any('评分索引' in p.text or '评审因素' in p.text for p in doc.paragraphs)
    print(f"\n{'='*20} 3. 评分索引 {'='*20}")
    print(f"评分索引表: {'有 [OK]' if has_score_index else '无 [FAIL]'}")

    # --- 4. 评审因素标注 ---
    score_marks = all_text.count('评审因素')
    print(f"\n{'='*20} 4. 评审因素标注 {'='*20}")
    print(f"标注次数: {score_marks}")
    if score_marks >= 10:
        print("  [OK] 十项因素均有标注")
    elif score_marks >= 5:
        print("  [WARN] 部分因素未标注")
    else:
        print("  [FAIL] 评审因素标注不足")

    # --- 5. 空标题检查 ---
    print(f"\n{'='*20} 5. 空标题检查 {'='*20}")
    empty_headings = []
    for i, p in enumerate(doc.paragraphs):
        text = p.text.strip()
        style_name = p.style.name if p.style else ''
        is_heading = 'Heading' in style_name or 'heading' in style_name
        # Also check by formatting: bold + short text
        if not is_heading:
            continue
        if not text:
            continue
        # Check if next non-empty paragraph is also a heading (meaning this heading has no body)
        next_has_body = False
        for j in range(i + 1, min(i + 5, len(doc.paragraphs))):
            next_text = doc.paragraphs[j].text.strip()
            if not next_text:
                continue
            next_style = doc.paragraphs[j].style.name if doc.paragraphs[j].style else ''
            next_is_heading = 'Heading' in next_style or 'heading' in next_style
            if next_is_heading:
                break
            next_has_body = True
            break
        if not next_has_body:
            # Check if there's a table immediately after
            # (rough check: if the heading text mentions 表/table, it's probably ok)
            if '详见下表' in text or '见表' in text:
                continue
            empty_headings.append((i + 1, text[:50]))

    if empty_headings:
        print(f"空标题: {len(empty_headings)} 处")
        for line_num, text in empty_headings:
            print(f"  L{line_num}: {text}")
    else:
        print("空标题: 0 处 [OK]")

    # --- 6. 占位符检查 ---
    print(f"\n{'='*20} 6. 占位符检查 {'='*20}")
    placeholders = ['XXX', 'xxx', '×××', '（）', '()', '____', '____']
    found_placeholders = []
    for ph in placeholders:
        count = all_text.count(ph)
        if count > 0:
            found_placeholders.append((ph, count))
    if found_placeholders:
        print(f"[FAIL] 发现占位符:")
        for ph, count in found_placeholders:
            print(f"  '{ph}': {count} 处")
    else:
        print("[OK] 无占位符")

    # --- 7. 楼栋/项目编号 ---
    print(f"\n{'='*20} 7. 项目关键词 {'='*20}")
    # Auto-detect building numbers from text
    building_pattern = r'[东西南北]?\d+#'
    buildings_found = set(re.findall(building_pattern, all_text))
    if buildings_found:
        print(f"楼栋编号: {sorted(buildings_found)}")
    else:
        print("  [WARN] 未检测到楼栋编号")

    # --- 8. 工程量关键词 ---
    print(f"\n{'='*20} 8. 工程量关键词 {'='*20}")
    # Common construction material keywords
    material_keywords = [
        'JDG', 'BV-', 'YJV', 'PPR', 'PVC', 'DN20', 'DN25', 'DN32', 'DN50', 'DN75', 'DN110',
        '蹲便器', '消火栓', '钢塑复合管', '桥架', '配电箱', '灯具', '插座', '开关',
        '真石漆', '乳胶漆', '环氧', '防水', 'SBS', '保温', '脚手架',
        '混凝土', '钢筋', '砂浆', '水泥', '砂石'
    ]
    found_materials = [kw for kw in material_keywords if kw in all_text]
    missing_materials = [kw for kw in material_keywords if kw not in all_text]
    print(f"命中: {len(found_materials)}/{len(material_keywords)}")
    if missing_materials:
        print(f"  未命中: {', '.join(missing_materials)}")

    # --- 9. 材料品牌 ---
    print(f"\n{'='*20} 9. 材料品牌 {'='*20}")
    # 2026-09-17 修复：合肥 AI 评审比对的是【招标文件品牌参考表】而非通用品牌表。
    # 优先级：brands_file 参数（每行"类别|品牌1、品牌2"）> 内置通用词表（仅作无招标对照时的兜底）
    # K3 审计加固：路径不存在显式 WARN；UTF-8 BOM/GBK 编码兜底；空文件/异常行 WARN；品牌级参考输出
    brand_map = []
    if brands_file and not os.path.exists(brands_file):
        print(f"  [WARN] 指定的品牌对照文件不存在：{brands_file}，已回退通用词表（请检查路径）")
    if brands_file and os.path.exists(brands_file):
        try:
            with io.open(brands_file, encoding='utf-8-sig') as f:
                brand_lines = list(f)
        except UnicodeDecodeError:
            with io.open(brands_file, encoding='gbk') as f:
                brand_lines = list(f)
        for ln in brand_lines:
            ln = ln.strip().replace('｜', '|')
            if not ln or ln.startswith('#'):
                continue
            parts = ln.split('|', 1)
            if len(parts) != 2:
                print(f"  [WARN] 无竖线行已跳过：{ln[:40]}")
                continue
            cat = parts[0].strip()
            brands = [b.strip() for b in parts[1].split('、') if b.strip()]
            if '|' in parts[1]:
                print(f"  [WARN] 多竖线行，尾部已并入最后品牌：{ln[:40]}")
            if not brands:
                print(f"  [WARN] 空品牌列表行已跳过（不计入未响应）：{ln[:40]}")
                continue
            brand_map.append((cat, brands))
    if brand_map:
        hit, miss = [], []
        for cat, brands in brand_map:
            if any(b in all_text or b in table_text_all() for b in brands):
                hit.append(cat)
            else:
                miss.append(cat)
        print(f"招标品牌参考表逐类命中: {len(hit)}/{len(brand_map)} 类")
        brand_hit_cnt = sum(1 for _, bs in brand_map for b in bs if b in all_text or b in table_text_all())
        brand_total_cnt = sum(len(bs) for _, bs in brand_map)
        print(f"  品牌级命中(参考): {brand_hit_cnt}/{brand_total_cnt}")
        if miss:
            print(f"  [WARN] 未响应类别({len(miss)}): {'、'.join(miss)}")
        else:
            print("  [OK] 招标品牌逐类全响应（类别级）")
    elif brands_file and os.path.exists(brands_file):
        print("  [WARN] 品牌对照文件无有效数据行（期望：类别|品牌1、品牌2，半角竖线），已回退通用词表，请核查文件")
    if not brand_map:
        common_brands = [
            '海螺', '东方雨虹', '金牛', '伟星', '恒洁', '九牧', '箭牌', '多乐士', '立邦',
            '三棵树', '马可波罗', '东鹏', '远东', '宝胜', '正泰', '西门子', '公牛',
            '欧普', '雷士', '步阳', '凤铝', '兴发', '海康', '大华', '德力西', '施耐德'
        ]
        brand_found = [b for b in common_brands if b in all_text or b in table_text_all()]
        print(f"品牌覆盖(通用词表兜底,建议传入招标品牌文件): {len(brand_found)}/{len(common_brands)}")
        if brand_found:
            print(f"  已覆盖: {', '.join(brand_found)}")

    # --- 10. 格式检查 ---
    print(f"\n{'='*20} 10. 格式检查 {'='*20}")
    sec = doc.sections[0]
    top = sec.top_margin.cm if sec.top_margin else 0
    bottom = sec.bottom_margin.cm if sec.bottom_margin else 0
    left = sec.left_margin.cm if sec.left_margin else 0
    right = sec.right_margin.cm if sec.right_margin else 0
    print(f"页边距: 上={top:.2f}cm 下={bottom:.2f}cm 左={left:.2f}cm 右={right:.2f}cm")

    style = doc.styles['Normal']
    font_name = style.font.name or '未设置'
    font_size = style.font.size.pt if style.font.size else '未设置'
    print(f"正文字体: {font_name} {font_size}pt")

    pf = style.paragraph_format
    line_spacing = pf.line_spacing.pt if pf.line_spacing else '未设置'
    line_rule = pf.line_spacing_rule
    print(f"行距: {line_spacing}pt 规则={line_rule}")

    # Page size
    page_w = sec.page_width.cm if sec.page_width else 0
    page_h = sec.page_height.cm if sec.page_height else 0
    print(f"纸张: {page_w:.1f}cm × {page_h:.1f}cm", end="")
    if abs(page_w - 21.0) < 0.5 and abs(page_h - 29.7) < 0.5:
        print(" (A4) [OK]")
    else:
        print(" [WARN] 非A4")

    # --- 11. 页数估算 ---
    print(f"\n{'='*20} 11. 页数估算 {'='*20}")
    est_lines = 0
    for p in doc.paragraphs:
        text = p.text
        if not text:
            est_lines += 1
        else:
            chars = len(text)
            lines_needed = max(1, (chars + 27) // 28)
            est_lines += lines_needed
    for t in doc.tables:
        for row in t.rows:
            max_cell_lines = 0
            for cell in row.cells:
                cell_text = ''
                for p in cell.paragraphs:
                    cell_text += p.text
                lines = max(1, (len(cell_text) + 30) // 31)
                max_cell_lines = max(max_cell_lines, lines)
            est_lines += max_cell_lines
    est_pages = (est_lines + 29) // 30
    print(f"估算页数: {est_pages}")
    print(f"  (估算基于行数，实际页数受图片/表格分页影响，建议导出PDF后复核)")

    # --- 12. 套话检查 ---
    print(f"\n{'='*20} 12. 模板套话检查 {'='*20}")
    cliche_patterns = [
        '为实现', '提供有力保障', '提供保障', '确保实现', '为.目标的实现',
        '确保.顺利进行', '切实做好', '进一步加强', '全面提升', '严格落实'
    ]
    cliche_counts = {}
    for pattern in cliche_patterns:
        count = all_text.count(pattern)
        if count > 3:
            cliche_counts[pattern] = count
    if cliche_counts:
        print("[WARN] 重复套话:")
        for phrase, count in sorted(cliche_counts.items(), key=lambda x: -x[1]):
            print(f"  '{phrase}': {count} 次")
    else:
        print("[OK] 套话重复率低")

    # --- 13. 章节字数统计 ---
    print(f"\n{'='*20} 13. 章节字数统计 {'='*20}")
    # 识别策略一：Heading 1 样式（docx 规范文档首选，2026-09-17 修复：支持"第X章"形态）
    chapters = []
    current_chapter = None
    current_chars = 0
    for p in doc.paragraphs:
        style_name = (p.style.name if p.style else '') or ''
        text = p.text.strip()
        if (style_name or '').lower().replace(' ', '') in ('heading1', '标题1'):
            if current_chapter:
                chapters.append((current_chapter, current_chars))
            current_chapter = text[:40]
            current_chars = 0
        elif current_chapter is not None:
            current_chars += len(text)
    if current_chapter:
        chapters.append((current_chapter, current_chars))
    chapter_src = 'Heading1样式'
    # 识别策略二（兜底）：两阶段正则——先"第X章"（支持阿拉伯数字），无命中才启用"一、"形态（K3 审计加固：排除句号结尾正文句，防误切列表项）
    if not chapters:
        for pat in (r'^第[一二三四五六七八九十百0-9]+章', r'^[一二三四五六七八九十]+、'):
            chapter_pattern = re.compile(pat)
            chapters = []
            current_chapter = None
            current_chars = 0
            for p in doc.paragraphs:
                text = p.text.strip()
                if chapter_pattern.match(text) and len(text) < 50 and not text.endswith('。'):
                    if current_chapter:
                        chapters.append((current_chapter, current_chars))
                    current_chapter = text[:40]
                    current_chars = 0
                elif current_chapter is not None:
                    current_chars += len(text)
            if current_chapter:
                chapters.append((current_chapter, current_chars))
            if chapters:
                chapter_src = f'正则兜底({pat[:6]}...)'
                break

    if chapters:
        print(f"(识别方式: {chapter_src}，共 {len(chapters)} 章)")
        print(f"{'章节':<40} {'正文字数':>8} {'评级':>6}")
        print("-" * 60)
        total_chapter_chars = 0
        for title, chars in chapters:
            total_chapter_chars += chars
            if chars >= 1500:
                rating = "优秀"
            elif chars >= 800:
                rating = "良好"
            elif chars >= 500:
                rating = "一般"
            else:
                rating = "薄弱"
            print(f"{title:<40} {chars:>8} {rating:>6}")
        print("-" * 60)
        print(f"{'合计':<40} {total_chapter_chars:>8}")
    else:
        print("  [WARN] 未检测到章节结构")

    # --- 14. 废标风险 ---
    print(f"\n{'='*20} 14. 废标风险检查 {'='*20}")
    risk_keywords = {
        '投标保证金': '投标保证金',
        '履约保证金': '履约保证金',
        '签字': '签字',
        '盖章': '盖章',
        '项目经理': '项目经理',
    }
    for name, kw in risk_keywords.items():
        found = kw in all_text
        print(f"  {name}: {'提及 [OK]' if found else '未提及 [WARN]'}")

    # --- 15. 数据来源标注 ---
    print(f"\n{'='*20} 15. 数据来源标注 {'='*20}")
    source_keywords = ['数据来源', '工程量清单', '清单编码', '项目编码', '招标文件']
    source_found = [kw for kw in source_keywords if kw in all_text]
    if source_found:
        print(f"[OK] 检测到数据来源标注: {', '.join(source_found)}")
    else:
        print("[WARN] 未检测到数据来源标注，建议添加'数据来源：已标价工程量清单'等标注")

    # --- 16. 表间横向闭环校验（判据：AI 拉通查所有表，一处对不上=方案不可行）---
    print(f"\n{'='*20} 16. 表间横向闭环校验 {'='*20}")
    num_pat = re.compile(r'([\d,]+(?:\.[\d]+)?)\s*\uff08?(人|天|台|₳|≡|吨|吨t|≤t|≡t|即t|≤≡|t)）?')

    def table_num_cells(table):
        """提取表格中 (文本, 数值, 单位) 三元组"""
        out = []
        for row in table.rows:
            for cell in row.cells:
                t = cell.text.strip()
                m = re.match(r'^([\d,]+(?:\.[\d]+)?)\s*(人|天|台|₳|吨|t|m|≡|即)?$', t)
                if m:
                    val = float(m.group(1).replace(',', ''))
                    unit = m.group(2) or ''
                    out.append((t, val, unit))
        return out

    labor_tables, equip_tables, mat_tables, sched_tables = [], [], [], []
    for ti, t in enumerate(doc.tables):
        header = ''
        for row in t.rows[:2]:
            header += row.cells[0].text if row.cells else ''
        header = header.replace(' ', '')
        nums = table_num_cells(t)
        if not nums:
            continue
        if any(k in header for k in ['劳动力', '配工', '工种', '人数']):
            labor_tables.append((ti, nums, header[:20]))
        elif any(k in header for k in ['机械', '设备']):
            equip_tables.append((ti, nums, header[:20]))
        elif any(k in header for k in ['材料', '物资', '用量']):
            mat_tables.append((ti, nums, header[:20]))
        elif any(k in header for k in ['进度', '工期', '计划表']):
            sched_tables.append((ti, nums, header[:20]))

    closed_checks = []

    # 16a 劳动力曲线峰值与进度计划阶段数一致
    if labor_tables and sched_tables:
        closed_checks.append(('ok', '检出劳动力表/进度表，可交叉比对（人工复核阶段峰值与工期分段）'))
    # 16b 工期闭环：各阶段天数之和 vs 总工期
    sched_days = []
    for ti, nums, _ in sched_tables:
        for txt, val, unit in nums:
            if unit == '天':
                sched_days.append((ti, val))
    total_kw = re.search(r'总工期[^0-9]{0,10}([\d]+)\s*天', all_text)
    if total_kw and sched_days:
        total_days = float(total_kw.group(1))
        stage_days = [d for _, d in sched_days if d != total_days]
        if stage_days:
            s = sum(stage_days)
            # 阶段天数之和应≤总工期（含交叉作业时可大于，仅提示人工复核）
            if s > total_days * 1.8:
                closed_checks.append(('fail', f'各阶段天数合计 {s:.0f} 远超总工期 {total_days:.0f} 天，可能存在单位/口径错误'))
            else:
                closed_checks.append(('ok', f'阶段天数合计 {s:.0f} 天 vs 总工期 {total_days:.0f} 天，建议人工复核交叉作业重叠'))
    # 16c 设备产能 vs 工程量（正文关键量值与设备表台数同时存在时提示验证）
    for ti, nums, header in equip_tables:
        units = {u for _, _, u in nums}
        closed_checks.append(('info', f'表{ti+1}({header}) 含数值单元格 {len(nums)} 个，请人工核对台数×产能×工天≥对应工程量'))
    # 16d 警示未检出任何可比表
    if not (labor_tables or equip_tables or mat_tables or sched_tables):
        closed_checks.append(('warn', '未识别到可校验的配置类表格，表间闭环需全人工核查'))

    # 产出
    if not closed_checks:
        closed_checks.append(('ok', '未发现明显矛盾，建议按 data-verification-guide.md 逐项产量验证'))
    level_mark = {'ok': '[OK]', 'info': '[INFO]', 'warn': '[WARN]', 'fail': '[FAIL]'}
    for lv, msg in closed_checks:
        print(f"  {level_mark[lv]} {msg}")
    if any(lv == 'fail' for lv, _ in closed_checks):
        print("  → 判据：AI 拉通查所有表，一处对不上即可能判「方案不可行」，请优先整改")

    # --- 17. 态度词黑名单（重态度轻控制检测）---
    print(f"\n{'='*20} 17. 措施段态度词检测 {'='*20}")
    attitude_words = [
        '加强管理', '严格控制', '高度重视', '做好养护',
        '精心施工', '强化监督', '切实保障', '认真贯彻', '做到最佳'
    ]
    # 只在段落正文中检测（排除表格），段长度 > 40 视为措施段
    attitude_hits = {}
    for p in doc.paragraphs:
        txt = p.text.strip()
        if len(txt) < 40:
            continue
        for w in attitude_words:
            if w in txt:
                attitude_hits.setdefault(w, 0)
                attitude_hits[w] += 1
    if attitude_hits:
        total_hits = sum(attitude_hits.values())
        print(f"[WARN] 检出态度类表述 {total_hits} 处（重态度轻控制，AI 不予计分）:")
        for w, cnt in sorted(attitude_hits.items(), key=lambda x: -x[1]):
            print(f"  '{w}': {cnt} 处")
        print("  → 处方：参照 section-writing-templates.md 五步法替换为可验证写法（见正例：分层浇筑≤500mm/入模温度5-30℃/养护≥14天/测温点每200㎡）")
    else:
        print("[OK] 未检出态度类表述，措施段可验证度良好")

    # --- 总结 ---
    print(f"\n{'='*70}")
    print("审计完成")
    print(f"{'='*70}")


if __name__ == '__main__':
    if len(sys.argv) < 2:
        print("用法: python audit_bid.py <docx路径> [招标关键词json] [招标品牌对照txt]")
        print("示例: python audit_bid.py 技术标.docx x brands_46.txt")
        print("品牌对照文件格式: 每行 类别|品牌1、品牌2、品牌3 （对照招标品牌参考表）")
        sys.exit(1)

    docx_path = sys.argv[1]
    keywords_file = sys.argv[2] if len(sys.argv) > 2 else None
    brands_file = sys.argv[3] if len(sys.argv) > 3 else None
    audit_docx(docx_path, keywords_file, brands_file)
