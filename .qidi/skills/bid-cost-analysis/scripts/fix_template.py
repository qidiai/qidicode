"""
工程投标成本分析修正脚本模板
用法: python fix_template.py <成本分析.xlsx> <输出.xlsx>

依赖: openpyxl, xlrd
"""
import openpyxl, sys, os

def fix_excel(src_path, dst_path, corrections):
    """
    corrections = {
        '材料费分析表': {'D3': 6457.24, 'D4': 119357.87, ...},
        '措施费分析表': {
            'updates': {'D3': 32.72, 'D4': 50.62},
            'new_rows': [
                {'row': 6, 'A': 4, 'B': '赶工措施费', 'D': 8.92, 'E': '=C3*D6', 'F': '备注'},
                ...
            ],
            'unmerge': 'C3:C7',
            'remerge': 'C3:C8',
        },
        '管理人员工资表': {'E3': 5400, 'E4': 4500, ...},
        '成本分析汇总表': {'B14': '新名称', 'C14': 1789682, 'D14': 1789682},
    }
    """
    wb = openpyxl.load_workbook(src_path, data_only=False)

    for sheet_name, changes in corrections.items():
        ws = wb[sheet_name]

        # 处理合并单元格
        if 'unmerge' in changes:
            ws.unmerge_cells(changes['unmerge'])
        if 'remerge' in changes:
            ws.merge_cells(changes['remerge'])

        # 更新单元格值
        if 'updates' in changes:
            for cell, val in changes['updates'].items():
                ws[cell] = val

        # 新增行
        if 'new_rows' in changes:
            for row_data in changes['new_rows']:
                r = row_data.pop('row')
                for col, val in row_data.items():
                    ws[col + str(r)] = val

    wb.save(dst_path)
    print('修正完成: ' + dst_path)


def calc_result(old_cost, mat_change, cuo_change, ins_total, guifei_change,
                fixed_costs, markup=1.15, ctrl_price=0):
    """计算修正后成本/报价/利润/下浮率"""
    total_change = mat_change + cuo_change + ins_total + guifei_change
    new_cost = old_cost + total_change
    variable = new_cost - fixed_costs
    new_bid = variable * markup + fixed_costs
    profit = new_bid - new_cost
    profit_rate = profit / new_bid * 100 if new_bid else 0
    down_rate = (1 - new_bid / ctrl_price) * 100 if ctrl_price else 0

    print('修正前成本价: %.2f' % old_cost)
    print('修正后成本价: %.2f' % new_cost)
    print('修正后投标报价: %.2f' % new_bid)
    print('利润: %.2f (%.2f%%)' % (profit, profit_rate))
    if ctrl_price:
        print('下浮率: %.2f%%' % down_rate)
    return new_cost, new_bid, profit


if __name__ == '__main__':
    if len(sys.argv) < 3:
        print('用法: python fix_template.py <成本分析.xlsx> <输出.xlsx>')
        sys.exit(1)
    # 实际使用时根据审查结果填写corrections
    print('请根据审查结果填写corrections字典后运行')
