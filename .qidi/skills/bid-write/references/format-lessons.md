# Word版式与格式踩坑手册（bid-write · 46轮投标文档迭代实录）

> 来源：蚌埠怀远疾控中心项目 v20→v43 共46轮迭代、5个模型盲审（GLM/DeepSeek/MiniMax/Claude/Agnes）
> 实测发现的真实渲染问题与修复方法。所有方法均经 Word COM / python-docx XML 双重验证。

---

## 一、行距（最高频翻车点）

### 1.1 铁律：永远直接写XML，不用python-docx的line_spacing属性

`paragraph_format.line_spacing = 22` 会写入EMU值（如279400），不同Word/WPS版本解析
不稳定；曾出现1005个段落被写成 `w:line="4" lineRule="auto"`（正常行距的1/60，
全文文字重叠成黑块，评委复核100%推翻AI分数）。

**唯一安全写法**（22磅固定值）：

```python
def set_spacing_exact22(p):
    spPr = p._p.get_or_add_pPr()
    sp = spPr.find(qn('w:spacing'))
    if sp is None:
        sp = OxmlElement('w:spacing'); spPr.append(sp)
    sp.set(qn('w:line'), '440')       # 22磅 = 440 twip（1磅=20twip）
    sp.set(qn('w:lineRule'), 'exact')
    sp.set(qn('w:before'), '0'); sp.set(qn('w:after'), '0')
```

验收标准：全文解析XML，所有段落必须 `line=440 + lineRule=exact`，零继承、零例外。
（正文+表格单元格分开统计，v43实测 294+808=1102段全部显式。）

### 1.2 图片段落必须清除exact行距（"行有空间却换行"的元凶）

含图片的段落若继承/被设为"固定值22磅"：图片被压进22磅高度 → 整图推到下一页 →
原位置留大片空白，肉眼即"行有空间就换行"。

```python
# 图片段落：删除w:spacing（单倍自动行距）+ 居中
spPr = p._p.get_or_add_pPr()
sp = spPr.find(qn('w:spacing'))
if sp is not None: spPr.remove(sp)
p.alignment = WD_ALIGN_PARAGRAPH.CENTER
```

### 1.3 表格内段落也要显式22磅

表格单元格段落默认继承表格样式，不显式设置时批注"行距22磅"无法自证。
804个单元格段落逐个写 `line=440/exact`。

### 1.4 标题行距与字号匹配

三号字（16pt节标题）配exact 22磅会裁字底部；节标题用 `560 twip = 28磅 exact`。

---

## 二、表格列宽（第二高频翻车点）

### 2.1 问题：python-docx建的表默认均分列宽

内容长的列（如"工程量描述"24.8cm宽文本）被塞进均分的8.5cm列 → 文字竖排成
"一条蛇"，评委第一眼即判排版不专业。

### 2.2 修复：按内容实测权重分配列宽

```python
def redistribute(t, avail_cm=17.0):
    """t: docx表格；avail_cm: 版心可用宽度(A4 21cm - 左右边距)"""
    W = qn('w:tcW'); G = qn('w:gridCol'); TW = qn('w:tblW'); L = qn('w:tblLayout')
    ncols = len(t.columns)
    # ① 权重 = 列内最长单元格字符宽（中文1.0/ASCII 0.55，超46截断防独占）
    weights = []
    for j in range(ncols):
        mx = 0
        for row in t.rows:
            txt = row.cells[j].text
            w = sum(1 if ord(ch)>127 else 0.55 for ch in txt)
            mx = max(mx, min(w, 46))
        weights.append(max(mx, 3))
    # ② 分配 + 每列保底2.0cm（序号列不挤没）
    avail_tw = int(avail_cm * 567)          # cm→twip
    raw = [avail_tw*w/sum(weights) for w in weights]
    MIN = int(2.0*567)
    # 迭代：低于保底的提到保底，从宽列按比例扣（0.9系数防负值）
    ...
    # ③ 三件套必须齐：tblLayout=fixed + tblW(dxa) + tblGrid/gridCol + 每行tcW
    tblPr = t._tbl.tblPr
    if tblPr.find(L) is None:
        el = OxmlElement('w:tblLayout'); tblPr.append(el)
    tblPr.find(L).set(qn('w:type'), 'fixed')
    # tblW必须写总宽dxa！只有fixed layout没有tblW → Word/WPS打开挂死（实测v40）
    tblW = tblPr.find(TW)
    if tblW is None:
        tblW = OxmlElement('w:tblW'); tblPr.append(tblW)
    tblW.set(qn('w:w'), str(avail_tw)); tblW.set(qn('w:type'), 'dxa')
```

### 2.3 验收：Word COM `Columns.Item(j).Width` 实测

python-docx写完后必须用Word COM回读验证：
`t.Columns.Item(j).Width / 28.35` → cm。渲染生效才算修复完成。
（v41实测：表2工程量列8.5→15cm、表4规格列3.4→7.9cm。）

---

## 三、字体声明（隐形翻车点）

### 3.1 问题：r.font.name只写ascii槽，中文fallback到主题字体

`r.font.name='SimSun'` 只设置 `w:ascii`；中文实际用的是 `w:eastAsia` 槽——
不显式写时继承文档主题字体，在不同机器打开显示不同。

### 3.2 修复：三槽全写

```python
def force_font(r, name='SimSun', size_pt=14):
    r.font.name = name; r.font.size = Pt(size_pt)
    rPr = r._element.get_or_add_rPr()
    rF = rPr.find(qn('w:rFonts'))
    if rF is None:
        rF = OxmlElement('w:rFonts'); rPr.insert(0, rF)
    for attr in ('w:eastAsia', 'w:ascii', 'w:hAnsi'):
        rF.set(qn(attr), name)
```

验收：全文XML扫描 `w:rFonts` 无 `w:eastAsia` 的run数量=0。
（v40实测：53个None字体run——全部是后期文本替换脚本新建的run漏写。）

### 3.3 教训：任何"文本替换"脚本创建的新run必须走force_font

批量替换（AI腔清理/数据修正）用 `p.text = new` 重写时run属性全丢；
正确姿势是保留首个run只改text，或替换后统一跑一遍force_font扫描。

---

## 四、图片与题注

### 4.1 图片宽度上限：版心17cm，插入用Pt(400)≈14.1cm留边

超17cm的图会把右边距顶穿。22in画布的CPM图插14cm后，7.8pt源字号实际渲染≈2pt
（0.7mm，打印不可读）——**画布尺寸与插入宽度的比例决定有效字号**，
脚本字号设计时按 `源pt × 插入cm/画布宽cm ≥ 2.5pt` 倒推。

### 4.2 图内禁止出现图号/题注（Word里另有正式题注段）

图内嵌"图8-2 xxx"再在Word里加题注段 = 题注重复同文（DeepSeek盲审抓扣）。
图内只放主标题（项目名/说明），图号交给Word题注段。

### 4.3 图内禁止作者注记

"（与图8-3同源）""（参数与表8-2逐格一致）"这类QA备注写进图 = 编辑痕迹泄入
正式图面。同源关系在Word正文读图说明里说，不画进图。

### 4.4 图片段落版式三件套

清除exact行距（1.2节）+ 居中 + 图后紧跟题注段（10.5pt宋体加粗居中）。

---

## 五、文字自证与AI指纹（盲审必抓）

### 5.1 禁用词清单（5模型交叉盲审累计命中）

| 类别 | 词汇/句式 | 来源模型 |
|---|---|---|
| AI术语 | 赋能/抓手/闭环/颗粒度/底层逻辑/组合拳 | GLM+DeepSeek |
| 自评套话 | "语言精练度满足评审要求""针对性强；具备可行性"三段式小结 | GLM终评 |
| QA腔 | "与表8-2完全一致""逐格一致""180日历天闭合"（图注/正文自我背书） | MiniMax+Claude |
| 编辑式自指 | "在第七章第四节已明确""按第一章第十三节执行"（跨章互指） | DeepSeek+MiniMax |
| 自证断言 | "经过验证具备可行性""可行性有保障""针对性直接来自" | DeepSeek |
| 破折号 | "——"（"XX——解释"是AI写作指纹，改冒号） | 用户+MiniMax |
| 作者注记 | 图内"（与图8-3同源）"等 | DeepSeek |

正确替代：客观陈述对应关系（"各工序时间见图8-1及表8-2"）；
亮点用数据支撑（"节约40工日≈机动班组1周投入"）而非"具备可行性"。

### 5.2 本章小结模式

三段式"本章小结。xxx针对性强；xxx具备可行性；语言精练度满足评审要求"
连续12章出现=AI拼装指纹（GLM直接扣6分）。删除小结、把三关键词自然融入
正文参数句（每章≤2处）。

### 5.3 章节重复检测

跨章复述同一内容（4.2 vs 4.4设备保养逐句重复；9.2/9.4/9.5/9.6扬尘监测4处
复述）=章节注水。写作前列章节内容矩阵，同一事实只在一节展开、他节引用节号。

---

## 六、数据口径一致性（盲审交叉核验必查）

### 6.1 同一数据在全文的每一处表述必须逐字可对齐

典型翻车（本项目实录）：劳动力峰值 表格213 vs 正文145；地下室完成 55 vs 58天；
桩基"9天有效"vs"20日历天"；周转3.5 vs 4次；混凝土120 vs 单层需求141m³；
塔吊"覆盖全部"vs R=35m几何只够西半侧；挖方2554-回填250-外运709=1596去向未交代。

**防漏方法**：修任何数据时用 `grep 全文计数`，目标值出现N处就改N处；
改完跑反向断言（旧值出现次数=0）。

### 6.2 数字要经得起算术复核

- 工日勾稽：Σ(阶段人数×天数) 与宣称总数逐项复算（24244=64×40+113×13+145×67+132×65+48×40）
- 损耗链：232×1.025=237.8≈238
- 资源量≥需求量：混凝土供应150 ≥ 单层浇筑141（保障小于需求是硬伤）
- 几何可验证的宣称慎写：塔吊R=35m"覆盖全部作业面"被Claude量图反驳（东南远角60m）
  → 诚实写"覆盖楼西侧主体作业面，远角汽车吊接驳+搬运车二次转运"

### 6.3 图内数据=脚本数据=表数据 三方一致

图由脚本生成时，图内标注值（S曲线里程碑%等）必须与正文表述同源——
图8-3写87%、图5-1右图写90%被Claude判"同文档两条曲线两个口径"。
修数据先改脚本再重渲，禁止直接改文不重渲图。

---

## 七、编辑痕迹清零（提交前终检清单）

| 检查项 | 方法 | 判定 |
|---|---|---|
| `**`/`###` md标记残留 | XML全文grep | =0 |
| `--`行（md表格分隔符未转换） | 表格单元格扫描 | =0 |
| `——`破折号 | 全文grep | =0 |
| 图内作者注记 | 图脚本grep"同源/一致/逐格" | =0 |
| 占位符未填 | grep"占位/TODO/[待补" | =0（交付版） |
| 编辑式自指 | grep"本章/本节/第一章第.节已明确" | 语义审查 |
| 跳号 | 章节号连续性扫描 | 无缺号 |
| 表引用与表号对应 | 表X-Y grep vs 实际表数 | 引用闭合 |
| 图读图说明 | 每图后紧跟题注+读图说明段 | 图数=说明数 |

---

## 八、盲审方法论（如何用多模型发现真问题）

1. **连续审同一文档的模型会产生盲区固化**：GLM连审5轮从82→97.8分，
   对自己v36引入的图例叠压视而不见；DeepSeek盲审第一轮就抓到10项。
2. **视觉验收不可省**：Claude无视觉，用"脚本源码+几何复算+像素实测"抓出
   大门20m标12m、堆场出圈、图例叠压326×20px——比目验更硬。
3. **换模型=换眼睛**：每完成一轮修复，换一个未参与过审计的新模型盲审。
4. **评分参考区间**（本项目收敛值）：GLM连审97.8（高估）、MiniMax/Agnes 95、
   DeepSeek 91.5、Claude实测87.5 → 真实落点≈中位93。
5. **报告落盘**：每轮审计报告存 `work/<项目>/vXX_评审报告.md` 供追溯。

---

## 九、Word COM导出PDF故障预案

`SaveAs2/ExportAsFixedFormat` 报 -2147352567 时的排查顺序：
1. 杀WINWORD/WPS进程重启COM（悬挂文档锁）
2. 检查docx XML合法性（python-docx能否打开）——tblLayout=fixed无tblW会挂死打开
3. 换WPS COM（Kwps.Application，注意DisplayAlerts=0）
4. LibreOffice headless（本机未装则跳过）
5. 都失败：跳过PDF预览，用Word COM `Columns.Item(j).Width` 回读验证排版即可

---

## 十、页数实测

Word COM：`word.ActiveWindow.View.Type = 3; doc.Repaginate();
doc.ComputeStatistics(2)`（必须先Repaginate，否则返回错误值如9）。
WPS COM同API但本机实测不准（返回9），以Word COM为准。

---

## 十一、修改操作规程（最高优先级·违反必出新bug）

> 46轮迭代最大教训：**每次修A坏B**。根因不是能力问题，是流程缺失——
> 批量replace只看匹配字符串不看上下文、修完不回归、不视觉确认。

### 11.1 五步强制流程（每次修改必须走完，缺一不可）

```
① 修改（单点，最小范围）
② 语法/编译验证（改脚本必须py_compile）
③ grep关联位置（目标词+同义词+引用它的图表/段落）
④ 全项回归（重跑格式终检脚本，30秒）
⑤ 视觉确认（Word COM截图或agnes子代理读图）
```

**五步全过才算完成。跳过任何一步=埋雷。**

### 11.2 禁止事项

| 禁止 | 原因 | 事故案例 |
|---|---|---|
| `p.text = new` 直接赋值 | 丢失所有run格式（字体/字号/加粗） | v40的53个None字体run |
| 批量replace不做py_compile | 脚本语法损坏 | fontsize=17.5.5级联污染5脚本 |
| 修数据不grep关联 | 图表/读图说明/其他表残留旧值 | 86根vs83根跨图矛盾3轮未清 |
| 修文字不看上下文标点 | 追加句产生。，/。；/。。残错 | v44注入副作用8处 |
| 修复后跳过回归 | 修A碰坏B无人发现 | 每轮审计都有新问题的根因 |
| 不打开Word看渲染 | 格式参数正确≠渲染正确 | 46轮0次人工目检 |

### 11.3 修复脚本模板（每次修改用这个骨架）

```python
# -*- coding: utf-8 -*-
"""修改描述"""
import sys, re
from docx import Document
from docx.shared import Pt
from docx.oxml.ns import qn
sys.stdout.reconfigure(encoding='utf-8')
DOCX = r'...'; OUT = DOCX  # 或新版本号
doc = Document(DOCX)

def set_para(p, t):
    """保留格式骨架的安全替换：清runs→写首run→强制三槽字体"""
    for r in p.runs: r.text = ''
    r0 = p.runs[0] if p.runs else p.add_run()
    r0.text = t
    r0.font.name = 'SimSun'; r0.font.size = Pt(14)
    r0._element.rPr.rFonts.set(qn('w:eastAsia'), 'SimSun')

# ① 修改
REPLS = [('旧文本', '新文本')]
count = 0
for p in doc.paragraphs:
    t0 = p.text; t = t0
    for o, nw in REPLS:
        if o in t: t = t.replace(o, nw)
    if t != t0:
        set_para(p, t); count += 1

# ② 标点自愈（追加句常见残错）
for p in doc.paragraphs:
    t = p.text
    t = re.sub(r'。\s*。', '。', t)
    t = re.sub(r'，\s*。', '。', t)
    if t.rstrip().endswith('，'): t = t.rstrip('，') + '。'
    if t != p.text: set_para(p, t)

# ③ 关联位置grep（替换涉及的每个关键词）
doc.save(OUT)
import zipfile
with zipfile.ZipFile(OUT) as z:
    text = re.sub(r'\s+',' ',re.sub('<[^>]+>',' ',z.read('word/document.xml').decode('utf-8'))).strip()
for kw in ['旧文本', '关联词1', '关联词2']:
    c = text.count(kw)
    expected = 0 if kw == '旧文本' else '>=1'
    print(f'  {kw}: {c} (期望{expected})', 'OK' if (c==0 if expected==0 else c>=1) else 'FAIL')

# ④ 回归：重跑格式终检（行距/字体/痕迹/数据口径）
# ⑤ 视觉：Word COM截图或派agnes子代理读图
```

### 11.4 视觉验证必须做（不是可选）

每次修改后用Word COM打开→Repaginate→用agnes子代理`read_file`读截图或直接读图PNG。
**文字有没有被线条穿过、图例有没有遮数据、底部说明有没有被裁切——只有眼睛能发现。**

### 11.5 图表修改的特殊回归

改图脚本后：py_compile→重渲→**用PIL读取PNG尺寸确认生成**→替换到docx→
派视觉子代理验收（agnes-2.5-flash，读图PNG回答OK/FAIL）。
改动数据（如桩数）必须同步改：正文+表+图脚本+读图说明+图内标注 五处。

---

## 十二、 fontsize级联污染（批量替换脚本的隐雷）

多轮批量replace中，`fontsize=8` → `fontsize=10.5` → `fontsize=11` 的链式替换
会产生 `fontsize=17.5.5` 这种非法值 → SyntaxError → 整个图脚本报废。

**防法**：批量改字号时用**单次正则整体替换**，不用链式replace：

```python
# 错误（级联）：逐个replace
t = t.replace('fontsize=6.2','fontsize=8.5')
t = t.replace('fontsize=8.5','fontsize=11')  # ← 8.5已被上一步改成11

# 正确（一次性）：正则映射
import re
FONT_MAP = {'6.2':'8.5', '7':'9.5', '8':'11'}
t = re.sub(r'fontsize=([\d.]+)',
           lambda m: f"fontsize={FONT_MAP.get(m.group(1), m.group(1))}", t)
```

**修法**：出现 `a.b.c` 三段式时，正则清洗 `re.findall(r'fontsize=(\d+\.\d+\.\d+)')`
取首段数字。修完必须py_compile验证。


---

## 十三、Word/WPS COM自动化规范（最高优先级）

> 来源：v56~v60期间连续COM报错（被呼叫方拒绝接收呼叫/文件锁定/对话框阻塞）。
> 用户被迫每次手动点击对话框，严重影响工作效率。

### 13.1 六条铁律

| # | 规则 | 原因 |
|---|---|---|
| 1 | 每次COM操作前先 Get-Process WINWORD \| Stop-Process -Force | 残留进程持有文件锁，导致后续打开卡死 |
| 2 | 用 DispatchEx 不用 Dispatch | Dispatch复用已有实例→上一个实例的对话框阻塞新调用 |
| 3 | 必须设 word.DisplayAlerts = 0 | 禁用字体替换/兼容性/修复等所有弹窗（COM中这些弹窗隐藏但阻塞进程） |
| 4 | word.Quit() 必须放 	ry...finally 块的 inally 中 | 前面报错时Quit不执行→进程残留→恶性循环 |
| 5 | 打开文件加 ReadOnly=True, AddToRecentFiles=False | 避免修改原文件+避免最近文件列表干扰 |
| 6 | doc.Close(False) 不保存（除非明确要保存） | 防止意外写入 |

### 13.2 标准安全模板

`python
import win32com.client
import pythoncom
pythoncom.CoInitialize()  # 多线程环境必须

# 先杀残留
os.system('taskkill /f /im WINWORD.EXE >nul 2>&1')

word = win32com.client.DispatchEx('Word.Application')
word.Visible = False
word.DisplayAlerts = 0  # 禁用所有对话框
try:
    doc = word.Documents.Open(path, ReadOnly=True, AddToRecentFiles=False)
    # ... 操作 ...
    doc.Close(SaveChanges=0)  # 0=不保存
finally:
    word.Quit()
`

### 13.3 WPS COM特别注意

- ProgID为 Kwps.Application（不是Word.Application）
- WPS的DisplayAlerts可能不支持数字枚举——用 word.DisplayAlerts = False
- WPS的PageNumbers.Add参数名可能与Word不同（本机测试返回9页为错误值）
- **优先使用Word COM**，WPS COM仅作备选

### 13.4 COM报错排查顺序

| 报错 | 原因 | 解决 |
|---|---|---|
| 被呼叫方拒绝接收呼叫 | COM进程忙/挂 | 杀进程重启 |
| 文件被锁定 | 上次进程残留 | 杀进程+删~\ |
| RPC服务器不可用 | COM连接断开 | 重新DispatchEx |
| 打开后卡死 | docx XML非法 | 用python-docx验证XML合法性 |
| SaveAs2失败 | Word内部错误 | 换ExportAsFixedFormat→换LibreOffice→跳过 |
| 弹对话框阻塞 | DisplayAlerts未设0 | 设0后重试 |

### 13.5 何时用COM vs python-docx

| 操作 | 推荐工具 | 原因 |
|---|---|---|
| 修改文字/格式/行距/列宽 | python-docx | XML级精确控制，无COM开销 |
| 页数统计 | Word COM | python-docx无法计算 |
| PDF导出 | Word COM | 唯一可靠方案（WPS不可靠） |
| 页码添加 | Word COM | python-docx footer API不完整 |
| 表格边框/底色 | python-docx | XML级精确控制 |
| 元数据修改 | zipfile直接操作XML | 最轻量 |
