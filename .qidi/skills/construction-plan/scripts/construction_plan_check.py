# -*- coding: utf-8 -*-
"""
construction_plan_check.py — 通用施工方案质量门
用法:
    python construction_plan_check.py <方案文本.md|txt> [--strict]
检查项:
  error  必备章节缺失 / 总工期数字前后矛盾 / 无进度计划章节
  warning 缺进度图或平面布置图引用 / 岗位不全 / 空洞表述 /
          危大特征无专项方案安排 / 缺质量或安全目标 / 残留占位符
退出码: 0=通过  1=存在 error（--strict 时存在 warning 也为 1）
"""
import sys, io, re

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')

# ---------- 必备章节（名称正则, 显示名） ----------
REQUIRED_SECTIONS = [
    (r'编制依据', '编制依据'),
    (r'工程概况|项目概况', '工程概况'),
    (r'施工部署|施工总体安排|总体部署', '施工部署'),
    (r'施工进度计划|进度计划|施工总进度', '施工进度计划'),
    (r'施工准备', '施工准备'),
    (r'资源配置|劳动力配置|资源配备|劳动力计划', '资源配置计划'),
    (r'施工方法|主要施工(方法|方案|工艺)|分部分项施工', '主要施工方法'),
    (r'质量保证|质量管理体系|质量控制', '质量保证措施'),
    (r'安全(保证|管理|文明|技术)|安全文明施工', '安全保证措施'),
    (r'文明施工|环境保护|绿色施工|扬尘', '文明施工/环保措施'),
    (r'应急预案|应急处置|应急响应', '应急预案'),
]

HOLLOW_WORDS = [
    '高度重视', '全面提升', '保驾护航', '多措并举', '扎实推进',
    '切实加强', '全力做好', '精雕细琢', '匠心打造', '万无一失',
    '世界先进', '一流水平', '凝心聚力', '砥砺前行',
]

DANGER_FEATURES = ['基坑', '高支模', '模板支撑', '脚手架', '起重吊装', '拆除工程', '暗挖', '顶管']


def main():
    if len(sys.argv) < 2:
        print('用法: python construction_plan_check.py <方案文本.md|txt> [--strict]')
        sys.exit(2)
    path = sys.argv[1]
    strict = '--strict' in sys.argv
    try:
        with open(path, 'r', encoding='utf-8') as f:
            text = f.read()
    except UnicodeDecodeError:
        with open(path, 'r', encoding='gbk', errors='ignore') as f:
            text = f.read()

    errors, warnings = [], []

    # 1. 必备章节
    missing = [disp for pat, disp in REQUIRED_SECTIONS if not re.search(pat, text)]
    if missing:
        errors.append(f'必备章节缺失: {", ".join(missing)}')

    # 2. 总工期一致性
    total_ctx = re.findall(r'总工期[^。；;\n]{0,25}?(\d+)\s*日?历?天', text)
    total_vals = sorted(set(int(x) for x in total_ctx))
    all_cal = sorted(set(int(x) for x in re.findall(r'(\d+)\s*日历天', text)))
    if len(total_vals) > 1:
        errors.append(f'总工期数字前后矛盾: 出现多个总工期值 {total_vals}')
    elif len(all_cal) > 1:
        warnings.append(f'"日历天"出现多个不同数值 {all_cal}——若含分项工期请在表述中限定范围，避免与总工期混淆')
    elif not all_cal:
        warnings.append('未检测到"XX日历天"工期表述，请确认工期目标已量化')

    # 3. 必备图表
    if not re.search(r'横道图|网络图|进度计划图|甘特图', text):
        warnings.append('未引用进度计划图（横道图/网络图）——进度必须配图')
    if not re.search(r'平面布置图|施工总平面|现场平面', text):
        warnings.append('未引用施工现场平面布置图')

    # 4. 岗位配置
    for post in ['项目经理', '技术负责人', '安全员', '质量员']:
        if post not in text:
            warnings.append(f'未配置"{post}"岗位——人员配置表不完整')

    # 5. 空洞表述
    hollow_hits = {w: text.count(w) for w in HOLLOW_WORDS if w in text}
    if hollow_hits:
        s = ", ".join(f'"{k}"x{v}' for k, v in sorted(hollow_hits.items(), key=lambda x: -x[1]))
        warnings.append(f'空洞表述命中（应改为"动作+环节+标准"）: {s}')

    # 6. 危大特征与专项方案
    danger = [d for d in DANGER_FEATURES if d in text]
    if danger and not re.search(r'专项施工方案|专项方案', text):
        warnings.append(f'检出危大特征（{", ".join(danger)}）但未见"专项施工方案"安排')
    if ('基坑' in text and re.search(r'(开挖深度|开挖)[^。；;\n]{0,15}[≥大于等于]\s*5\s*[m米]', text)) and '专家论证' not in text:
        warnings.append('检出开挖深度≥5m 基坑特征，但未见"专家论证"安排（超一定规模危大工程须论证）')

    # 7. 目标三件套
    if not re.search(r'质量目标', text):
        warnings.append('缺少"质量目标"量化表述')
    if not re.search(r'安全目标', text):
        warnings.append('缺少"安全目标"量化表述')

    # 8. 占位符残留
    ph = len(re.findall(r'【待补充|【占位|XXX|\[待填', text))
    if ph:
        warnings.append(f'残留占位符 {ph} 处——交付前必须全部处理')

    # ---------- 输出 ----------
    print('=' * 62)
    print('施工方案质量门检查报告')
    print('=' * 62)
    n_sec = len(REQUIRED_SECTIONS) - len(missing)
    print(f'文本长度: {len(text)} 字符 | 章节命中: {n_sec}/{len(REQUIRED_SECTIONS)}')
    if errors:
        print(f'\n[ERROR] {len(errors)} 项:')
        for e in errors:
            print(f'  ✗ {e}')
    if warnings:
        print(f'\n[WARNING] {len(warnings)} 项:')
        for w in warnings:
            print(f'  ⚠ {w}')
    if not errors and not warnings:
        print('\n全部检查通过 ✓')
    print('-' * 62)
    if errors or (strict and warnings):
        print(f'结论: 未通过（error={len(errors)}, warning={len(warnings)}{"，--strict 模式" if strict else ""}）')
        sys.exit(1)
    print(f'结论: 通过（error={len(errors)}, warning={len(warnings)}）')
    sys.exit(0)


if __name__ == '__main__':
    main()
