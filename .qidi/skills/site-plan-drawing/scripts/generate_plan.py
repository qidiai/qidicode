#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
施工总平面布置图生成器
用WPS COM Shapes API在Word文档中绘制文本框+直线+矩形布局。

用法：
  python generate_plan.py --output output.docx

依赖：
  - Windows系统
  - WPS Office 或 Microsoft Word
  - pywin32 (pip install pywin32)
"""

import argparse
import sys
import win32com.client


def generate_site_plan(output_path, roads=None, facilities=None):
    """生成施工总平面布置图Word文档。

    参数：
        output_path: 输出文件路径
        roads: 道路信息列表 [(名称, 全长, 特殊说明), ...]
        facilities: 设施信息列表 [(名称, 规格, 区域), ...]
    """

    if roads is None:
        roads = [
            ("回春路", "233m", ""),
            ("康乐路", "380m", "分3段交替施工"),
            ("富强路", "300m", "含玻纤格栅铺设区2005平方米"),
        ]

    if facilities is None:
        facilities = {
            "left_top": [("项目部\n办公区\n会议室\n资料室", 3.0, 2.5),
                         ("工人休息区\n饮水设施\n急救药箱", 3.0, 1.5),
                         ("安全宣讲台\n五牌一图\n公示栏", 3.0, 1.5)],
            "right_top": [("材料库房\n水泥库\n工具库", 3.5, 2.0),
                          ("机械停放场\n200平方米", 3.5, 1.5),
                          ("交通协管员\n2名/路段", 3.5, 1.5)],
            "mid_1": [("○临时用水\n接驳点", 2.0, 1.2),
                      ("◇总配电箱\n630kVA", 2.0, 1.2),
                      ("★消防器材\n灭火器10具", 2.0, 1.2),
                      ("△材料堆场\n300平方米", 2.0, 1.2),
                      ("机械停放场\n200平方米", 2.0, 1.2),
                      ("临时蓄水池\n10立方米", 2.0, 1.2),
                      ("配电房\n10平方米", 2.0, 1.2)],
            "mid_2": [("备用发电机\n50kW", 2.5, 1.2),
                      ("分配电箱\n3台", 2.5, 1.2),
                      ("排水沟\n300x300mm", 2.5, 1.2),
                      ("沉淀池\n5立方米", 2.5, 1.2),
                      ("电缆YJV-3x95\n+2x50 架空", 2.5, 1.2),
                      ("洗车装置\n三级沉淀", 2.0, 1.2)],
        }

    word = win32com.client.Dispatch('Word.Application')
    word.Visible = False
    doc = word.Documents.Add()

    # A4横向
    doc.PageSetup.Orientation = 1
    doc.PageSetup.PageWidth = word.CentimetersToPoints(29.7)
    doc.PageSetup.PageHeight = word.CentimetersToPoints(21)
    doc.PageSetup.LeftMargin = word.CentimetersToPoints(2)
    doc.PageSetup.RightMargin = word.CentimetersToPoints(2)
    doc.PageSetup.TopMargin = word.CentimetersToPoints(2)
    doc.PageSetup.BottomMargin = word.CentimetersToPoints(2)

    def cm2pt(cm):
        return word.CentimetersToPoints(cm)

    def add_textbox(left_cm, top_cm, width_cm, height_cm, text, font_sz=9):
        shape = doc.Shapes.AddTextbox(1, cm2pt(left_cm), cm2pt(top_cm),
                                     cm2pt(width_cm), cm2pt(height_cm))
        shape.Fill.Visible = False
        shape.Line.ForeColor.RGB = 0
        shape.Line.Weight = 0.5
        tf = shape.TextFrame
        tf.TextRange.Text = text
        tf.TextRange.Font.Size = font_sz
        tf.TextRange.Font.Name = '宋体'
        tf.TextRange.Font.Color = 0
        tf.TextRange.ParagraphFormat.Alignment = 2
        tf.VerticalAnchor = 3
        return shape

    def add_line(x1_cm, y1_cm, x2_cm, y2_cm, weight=0.5):
        shape = doc.Shapes.AddLine(cm2pt(x1_cm), cm2pt(y1_cm),
                                   cm2pt(x2_cm), cm2pt(y2_cm))
        shape.Line.ForeColor.RGB = 0
        shape.Line.Weight = weight
        return shape

    def add_rect(left_cm, top_cm, width_cm, height_cm, weight=0.5):
        shape = doc.Shapes.AddShape(1, cm2pt(left_cm), cm2pt(top_cm),
                                    cm2pt(width_cm), cm2pt(height_cm))
        shape.Fill.Visible = False
        shape.Line.ForeColor.RGB = 0
        shape.Line.Weight = weight
        return shape

    def add_title(text, font_sz=14):
        p = doc.Paragraphs.Add()
        r = p.Range
        r.Text = text
        r.Font.Size = font_sz
        r.Font.Name = '宋体'
        r.Font.Color = 0
        r.ParagraphFormat.Alignment = 1
        r.ParagraphFormat.LineSpacingRule = 4
        r.ParagraphFormat.LineSpacing = 28

    # ============================================================
    # 标题
    # ============================================================
    add_title('附表五：施工总平面图')
    add_title('扎赉诺尔区回春路、康乐路、富强路道路提升改造工程')
    add_title('施工总平面布置图')

    # ============================================================
    # 外框
    # ============================================================
    add_rect(1.0, 5.0, 25.7, 14.0, 0.5)

    # ============================================================
    # 左侧设施区（项目部等，独立于道路）
    # ============================================================
    left_y = 5.5
    for text, w, h in facilities.get("left_top", []):
        add_textbox(1.5, left_y, w, h, text, 9)
        left_y += h + 0.2

    # ============================================================
    # 右侧设施区（材料库房等）
    # ============================================================
    right_y = 5.5
    for text, w, h in facilities.get("right_top", []):
        add_textbox(22.5, right_y, w, h, text, 9)
        right_y += h + 0.2

    # ============================================================
    # 三条道路纵向排列
    # ============================================================
    road_y = 5.5
    road_spacing = 4.5  # 每条路占4.5cm高度

    for idx, (name, length, note) in enumerate(roads):
        # 道路标题
        title_text = f'{name}（全长约{length}'
        if note:
            title_text += f'，{note}'
        title_text += '）'
        add_textbox(5.5, road_y, 16.0, 0.8, title_text, 10)

        # 施工区域
        add_textbox(5.5, road_y + 1.0, 16.0, 1.5,
                   '施工区域（半幅封闭）  →← 半幅通行 →←  限速30km/h', 9)

        # 围挡线
        add_line(5.2, road_y, 5.2, road_y + 2.7, 1.0)
        add_line(21.8, road_y, 21.8, road_y + 2.7, 1.0)

        # 中间设施带
        mid_y = road_y + 3.0
        mid_key = f"mid_{idx + 1}"
        mid_facilities = facilities.get(mid_key, [])
        if mid_facilities:
            mid_x = 5.5
            for text, w, h in mid_facilities:
                add_textbox(mid_x, mid_y, w, h, text, 8)
                mid_x += w + 0.3

        road_y += road_spacing

    # ============================================================
    # 临时水电小方块
    # ============================================================
    water_elec_positions = [
        (4.3, 6.8, '电'), (4.3, 11.3, '水'), (4.3, 15.8, '电'),
        (22.0, 6.8, '水'), (22.0, 11.3, '电'), (22.0, 15.8, '水'),
    ]
    for x, y, label in water_elec_positions:
        add_textbox(x, y, 0.8, 0.8, label, 8)

    # ============================================================
    # 指北针
    # ============================================================
    add_textbox(24.5, 16.5, 1.5, 1.5, 'N\n↑', 10)

    # ============================================================
    # 图例
    # ============================================================
    legend_items = [
        ('━━━', '施工围挡', '○', '临时用水', '◇', '配电箱'),
        ('→←', '交通流向', '★', '消防器材', '△', '机械停放'),
        ('■', '施工区域', '⊕', '洗车装置', '□', '材料堆场'),
    ]
    legend_y = 17.5
    for row in legend_items:
        x = 1.5
        for i in range(0, len(row), 2):
            add_textbox(x, legend_y, 1.0, 0.8, row[i], 8)
            add_textbox(x + 1.3, legend_y, 2.0, 0.8, row[i + 1], 8)
            x += 4.0
        legend_y += 0.9

    # ============================================================
    # 保存
    # ============================================================
    doc.SaveAs(output_path, 16)
    doc.Close(False)
    try:
        word.Quit()
    except:
        pass
    print(f'文档已保存: {output_path}')


def main():
    parser = argparse.ArgumentParser(description='生成施工总平面布置图Word文档')
    parser.add_argument('--output', '-o', required=True, help='输出文件路径')
    args = parser.parse_args()
    generate_site_plan(args.output)


if __name__ == '__main__':
    main()
