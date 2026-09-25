---
name: gantt-chart-drawing
description: 【内蒙古专用·2026-09-25用户裁定降格】在WPS Word文档中用python-docx绘制黑白施工进度横道图（甘特图）。"2磅黑色横道线、禁单元格底色填充"等规范源自内蒙古暗标项目的招标要求，不是通用暗标要求，仅当本项目招标文件明确要求黑白图时使用。其他项目（如南谯区城乡供水项目，已实证无黑白要求）一律改用 bid-diagram 技能的彩色横道图（matplotlib：主工序深蓝+平行工序青+关键条红描边）。当用户需要在Word中生成横道图、施工进度图、甘特图且招标文件有黑白要求时使用。输入为工序列表（名称、开始天数、结束天数）和总工期天数，输出为包含横道图表格的docx文件。
---

# 施工进度横道图绘制（内蒙古专用）

> **⚠️ 适用范围裁定（2026-09-25，用户指定）**：本技能的"2磅黑色横道线、禁底色填充"规范源自**内蒙古暗标项目**的招标要求。使用前必须先检索本项目招标文件原文：有黑白要求 → 用本技能；**无此要求的 → 用 bid-diagram 技能的彩色横道图**（matplotlib 彩色专业配色，参考南谯区附图9-1 实践：主工序深蓝/平行工序青+斜纹/关键条红描边）。

## 产物呈现（强制）

本技能任何产出交付文件（docx/xlsx/pptx/pdf/png 等）的任务：

1. 文件落盘成功后立即登记：
   `python ~/.qidi/skills/office-artifact/scripts/card.py add <产物路径> --skill gantt-chart-drawing --note <一句话说明>`
2. 任务最终回复前必须输出卡片总览：
   `python ~/.qidi/skills/office-artifact/scripts/card.py show`
3. 不许只回一行文件路径就结束；卡片失败则降级为 Markdown 表格列出全部产物。


## 核心原理

用**段落顶部边框**模拟横道线，而非WPS COM AddShape或单元格底色填充。原因：

- AddShape坐标计算不可靠（WPS COM `Information`属性返回值异常），横道线会飘到错误位置
- 单元格底色填充会被招标文件视为废标
- 段落顶部边框是python-docx原生XML，坐标由Word排版引擎自动计算，不会错位

### 连续性原理

横道线连续的关键：**单元格左右边距设为0 + 相邻单元格段落的顶部边框无缝衔接**。

一条工序的横道线跨越多个时间列时，每个单元格内的段落都设置2pt顶部边框，由于单元格边距为0，相邻段落的边框在单元格交界处无缝连接，形成一条完整的连续横线。

### 居中原理

横道线居中的关键：**段前间距 = 行高/2 - 边框厚度/2**。

段落行距设为极小值（1pt=20twips），段前间距根据行高动态计算，使顶部边框始终在行内垂直居中。用户调整行高后，段前间距需要重新计算。

### 精确定位原理

横道线起止精确到天：**段落左/右缩进 = 分数 × 列宽（twips）**。

例如工序从第5天开始（时间列0-10天），左缩进 = 0.5 × 列宽twips，使横道线从该列50%处开始。

## 使用步骤

1. 准备工序数据：`[(工序名称, 开始天数, 结束天数), ...]`
2. 调用 `scripts/generate_gantt.py` 生成docx文件
3. 用WPS打开检查横道线是否连续、居中

## 关键参数

| 参数 | 说明 | 默认值 |
|------|------|--------|
| 总工期 | 日历天数 | 180 |
| 时间列数 | 每10天一列 | 18 |
| 工序列宽 | 工序名称列宽度 | 5.0cm |
| 行高 | 数据行高度 | 1.0cm（28磅） |
| 横道线粗细 | 段落顶部边框 | 2pt (sz=16) |
| 表格边框 | 全表边框 | 0.5pt (sz=4) |
| 页面方向 | A4横向 | 29.7×21cm |
| 页边距 | 四边 | 2cm |

## 暗标格式要求（内蒙古暗标项目要求——非通用）

- 图表标题：黑色四号宋体(14pt)，两端对齐，缩进两字符
- 图表内文字：黑色五号宋体(10.5pt)，行距28磅
- 所有边框：0.5磅黑色单实直线
- 横道线：2磅线条
- WPS Word黑白绘制（内蒙古要求），不得手写手绘；无黑白要求的项目禁用本技能，改用 bid-diagram 彩色横道图
- 图表居中放置
- 颜色仅使用黑色（内蒙古要求）

## 脚本用法

```bash
python scripts/generate_gantt.py --output output.docx --tasks "施工准备,0,10" "测量放线,5,25" --total-days 180
```

也可在Python中直接import：

```python
import sys
sys.path.insert(0, 'scripts')
from generate_gantt import create_gantt, add_chart_title
from docx import Document
from docx.shared import Cm
from docx.enum.section import WD_ORIENT

doc = Document()
section = doc.sections[0]
section.orientation = WD_ORIENT.LANDSCAPE
section.page_width = Cm(29.7)
section.page_height = Cm(21)
section.top_margin = Cm(2)
section.bottom_margin = Cm(2)
section.left_margin = Cm(2)
section.right_margin = Cm(2)

tasks = [
    ("施工准备", 0, 10),
    ("测量放线", 5, 25),
    # ...
]
create_gantt(doc, tasks, total_days=180)
doc.save("output.docx")
```

## 常见陷阱

1. **不要用WPS COM AddShape画横道线**：`Information(7)`返回的Y坐标不可靠，横道线会飘到错误位置
2. **不要用单元格底色填充**：暗标评审中底色填充会被判废标
3. **单元格边距必须设为0**：否则相邻段落的边框会有间隙，横道线断开
4. **行高改变后段前间距需重新计算**：段前间距 = (行高twips - 边框厚度twips) / 2
5. **行距必须设为固定值**：不能用"单倍行距"等自动行距，否则段前间距计算不准
6. **行不可跨页**：设置`cantSplit`属性，避免表格行跨页导致横道线断裂
