"""
从清单控制价提取工程量汇总脚本
用法: python extract_qty.py <清单根目录> [--categories 钢筋,钢构,砼,砌块砖]

依赖: xlrd
"""
import xlrd, sys, os, argparse

CATEGORIES = {
    '钢筋': ['钢筋', 'HRB', 'HPB', '钢筋笼', '预埋铁件', '植筋', '钢筋网'],
    '钢构': ['钢柱', '钢梁', 'H型钢', '钢结构', '钢构件', '钢屋架', '钢支撑', '钢平台', '钢梯', '钢雨棚', '钢结构雨棚', '钢支撑、系杆'],
    '砼': ['商品混凝土', 'C15', 'C20', 'C25', 'C30', 'C35', 'C40', 'C45', '混凝土', '垫层', '满堂基础', '独立基础', '矩形柱', '矩形梁', '构造柱', '圈梁', '过梁', '直形墙', '平板', '设备基础', '带型基础', '基础梁', '微膨胀'],
    '砌块砖': ['砌块墙', '实心砖墙', '砖基础', 'ALC', '隔墙板', '空心砖'],
    '砂浆': ['预拌砂浆', 'M5', 'M7.5', 'M10', 'M15', 'M20', '干混', '水泥砂浆', '防水砂浆'],
    '防水': ['防水', '卷材', '涂膜', '聚氨酯', 'SBS', '聚合物水泥'],
    '保温': ['保温', '挤塑板', '聚苯板', '岩棉', '发泡'],
    '门窗': ['铝合金', '百叶窗', '卷帘门', '平开门', '防火门', '对讲门', '塑钢窗'],
    '桩基': ['管桩', '搅拌桩', '灌注桩', '预制桩', '接桩', '截桩', '填心', '桩尖'],
    '模板': ['模板', '脚手架'],
    '土方': ['挖土', '挖基坑', '回填', '平整场地', '运土方', '人工清底', '换填'],
}


def extract_from_dir(base_path, categories=None):
    """从清单目录提取所有分部分项工程量"""
    if categories:
        cats = {k: CATEGORIES[k] for k in categories if k in CATEGORIES}
    else:
        cats = CATEGORIES

    results = {}
    for cat in cats:
        results[cat] = {}

    for root, dirs, files in os.walk(base_path):
        for f in sorted(files):
            if not f.endswith('.xls'):
                continue
            if not any(kw in f for kw in ['土建工程', '安装工程', '绿化', '标牌', '道路', '围墙', '排水', '室外安装']):
                continue
            fp = os.path.join(root, f)
            try:
                wb = xlrd.open_workbook(fp)
                sheet_name = None
                for sn in wb.sheet_names():
                    if '分部分项' in sn:
                        sheet_name = sn
                        break
                if not sheet_name:
                    continue
                ws = wb.sheet_by_name(sheet_name)
                for r in range(ws.nrows):
                    name = str(ws.cell_value(r, 2))
                    unit = str(ws.cell_value(r, 4)).strip()
                    qty = ws.cell_value(r, 5)
                    price = ws.cell_value(r, 6)
                    total = ws.cell_value(r, 7)
                    if not isinstance(qty, (int, float)) or qty <= 0:
                        continue
                    if not isinstance(price, (int, float)) or price <= 0:
                        continue
                    for cat, keywords in cats.items():
                        if any(kw in name for kw in keywords):
                            if unit not in results[cat]:
                                results[cat][unit] = {'qty': 0, 'total': 0, 'count': 0}
                            results[cat][unit]['qty'] += qty
                            if isinstance(total, (int, float)):
                                results[cat][unit]['total'] += total
                            results[cat][unit]['count'] += 1
                            break
            except Exception as e:
                pass

    # 输出
    for cat in cats:
        if cat not in results or not results[cat]:
            continue
        print('\n=== %s ===' % cat)
        for unit, data in sorted(results[cat].items(), key=lambda x: -x[1]['total']):
            avg = data['total'] / data['qty'] if data['qty'] > 0 else 0
            print('  单位:%s | 总量:%.2f | 总额:%.2f | 均价:%.2f | 条目:%d' % (
                unit, data['qty'], data['total'], avg, data['count']))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='从清单控制价提取工程量')
    parser.add_argument('base_dir', help='清单控制价根目录')
    parser.add_argument('--categories', default=None, help='提取类别(逗号分隔): 钢筋,钢构,砼,砌块砖')
    args = parser.parse_args()

    cats = args.categories.split(',') if args.categories else None
    extract_from_dir(args.base_dir, cats)
