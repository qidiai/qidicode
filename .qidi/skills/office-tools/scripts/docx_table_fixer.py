#!/usr/bin/env python
"""
docx_table_fixer.py — Word文档表格格式通用修复工具

用法:
  python docx_table_fixer.py <docx文件路径> [输出路径]

功能:
  1. 表格居中对齐
  2. 表格宽度=100%页面可用宽度
  3. 表格左缩进=0
  4. 表格布局=fixed
  5. 表头行跨页重复
  6. 单元格段落行距=单倍、无段前段后
  7. 单元格段落缩进全部清零(6种属性)
  8. 移除cantSplit(防止大行被截断)
  9. 列宽均分

适用场景:
  - python-docx生成的docx表格显示异常
  - 表格内容被截断/溢出
  - 表格未居中/偏移
  - 暗标/标书格式合规修复
"""

import sys
import os

def fix_docx_tables(input_path, output_path=None):
    sys.stdout.reconfigure(encoding='utf-8')
    from docx import Document
    from docx.shared import Pt, Cm
    from docx.enum.table import WD_TABLE_ALIGNMENT
    from docx.oxml.ns import qn
    from docx.oxml import OxmlElement

    if output_path is None:
        base, ext = os.path.splitext(input_path)
        output_path = f"{base}_表格修复{ext}"

    doc = Document(input_path)

    # 计算页面可用宽度
    section = doc.sections[0]
    page_w = section.page_width
    left_m = section.left_margin
    right_m = section.right_margin
    avail_emu = page_w - left_m - right_m
    avail_cm = avail_emu / 360000
    avail_twips = int(avail_emu / 635)
    print(f"页面可用宽度: {avail_cm:.2f}cm ({avail_twips} twips)")

    stats = {
        'tables': 0,
        'alignment': 0,
        'width': 0,
        'indent': 0,
        'layout': 0,
        'header': 0,
        'cell_spacing': 0,
        'cell_indent': 0,
        'cantSplit_removed': 0,
        'col_width': 0,
    }

    for ti, table in enumerate(doc.tables):
        stats['tables'] += 1
        tbl = table._tbl
        tblPr = tbl.find(qn('w:tblPr'))
        if tblPr is None:
            tblPr = OxmlElement('w:tblPr')
            tbl.insert(0, tblPr)

        # 1. 表格居中
        table.alignment = WD_TABLE_ALIGNMENT.CENTER
        jc = tblPr.find(qn('w:jc'))
        if jc is None:
            jc = OxmlElement('w:jc')
            tblPr.append(jc)
        jc.set(qn('w:val'), 'center')
        stats['alignment'] += 1

        # 2. 表格宽度=100%
        tblW = tblPr.find(qn('w:tblW'))
        if tblW is None:
            tblW = OxmlElement('w:tblW')
            tblPr.append(tblW)
        tblW.set(qn('w:type'), 'pct')
        tblW.set(qn('w:w'), '5000')
        stats['width'] += 1

        # 3. 表格左缩进=0
        tblInd = tblPr.find(qn('w:tblInd'))
        if tblInd is None:
            tblInd = OxmlElement('w:tblInd')
            tblPr.append(tblInd)
        tblInd.set(qn('w:w'), '0')
        tblInd.set(qn('w:type'), 'dxa')
        stats['indent'] += 1

        # 4. 表格布局=fixed
        tblLayout = tblPr.find(qn('w:tblLayout'))
        if tblLayout is None:
            tblLayout = OxmlElement('w:tblLayout')
            tblPr.append(tblLayout)
        tblLayout.set(qn('w:type'), 'fixed')
        stats['layout'] += 1

        for ri, row in enumerate(table.rows):
            tr = row._tr
            trPr = tr.find(qn('w:trPr'))

            # 5. 表头行跨页重复(仅第一行)
            if ri == 0:
                if trPr is None:
                    trPr = OxmlElement('w:trPr')
                    tr.insert(0, trPr)
                tblHeader = trPr.find(qn('w:tblHeader'))
                if tblHeader is None:
                    tblHeader = OxmlElement('w:tblHeader')
                    trPr.append(tblHeader)
                stats['header'] += 1

            # 8. 移除cantSplit
            if trPr is not None:
                cantSplit = trPr.find(qn('w:cantSplit'))
                if cantSplit is not None:
                    trPr.remove(cantSplit)
                    stats['cantSplit_removed'] += 1

            # 6+7. 单元格段落修复
            for cell in row.cells:
                for para in cell.paragraphs:
                    pPr = para._element.find(qn('w:pPr'))
                    if pPr is None:
                        pPr = OxmlElement('w:pPr')
                        para._element.insert(0, pPr)

                    # 6. 行距=单倍、无段前段后
                    spacing = pPr.find(qn('w:spacing'))
                    if spacing is None:
                        spacing = OxmlElement('w:spacing')
                        pPr.append(spacing)
                    spacing.set(qn('w:before'), '0')
                    spacing.set(qn('w:after'), '0')
                    spacing.set(qn('w:line'), '240')
                    spacing.set(qn('w:lineRule'), 'auto')
                    stats['cell_spacing'] += 1

                    # 7. 缩进全部清零
                    ind = pPr.find(qn('w:ind'))
                    if ind is None:
                        ind = OxmlElement('w:ind')
                        pPr.append(ind)
                    ind.set(qn('w:left'), '0')
                    ind.set(qn('w:leftChars'), '0')
                    ind.set(qn('w:right'), '0')
                    ind.set(qn('w:rightChars'), '0')
                    ind.set(qn('w:firstLine'), '0')
                    ind.set(qn('w:firstLineChars'), '0')
                    ind.set(qn('w:hanging'), '0')
                    ind.set(qn('w:hangingChars'), '0')
                    stats['cell_indent'] += 1

        # 9. 列宽均分
        num_cols = len(table.columns)
        if num_cols > 0:
            col_width_twips = avail_twips // num_cols
            for col in table.columns:
                col.width = col_width_twips * 635
            stats['col_width'] += 1

    doc.save(output_path)

    print(f"\n=== 修复完成 ===")
    print(f"处理表格数: {stats['tables']}")
    print(f"居中对齐: {stats['alignment']}")
    print(f"宽度100%: {stats['width']}")
    print(f"左缩进清零: {stats['indent']}")
    print(f"布局fixed: {stats['layout']}")
    print(f"表头重复: {stats['header']}")
    print(f"单元格行距修复: {stats['cell_spacing']}")
    print(f"单元格缩进清零: {stats['cell_indent']}")
    print(f"移除cantSplit: {stats['cantSplit_removed']}")
    print(f"列宽均分: {stats['col_width']}")
    print(f"\n输出: {output_path}")
    return output_path


if __name__ == '__main__':
    if len(sys.argv) < 2:
        print("用法: python docx_table_fixer.py <docx文件路径> [输出路径]")
        sys.exit(1)
    
    input_path = sys.argv[1]
    output_path = sys.argv[2] if len(sys.argv) > 2 else None
    fix_docx_tables(input_path, output_path)
