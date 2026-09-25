---
name: office-tools
description: "办公文档全功能操作工具，覆盖 PDF/Word/Excel/PowerPoint。当用户需要读取、创建、编辑、合并、拆分、旋转、转换、填模板、从Markdown生成或操作办公文档(.pdf/.docx/.xlsx/.pptx)时使用。触发词：读取/创建/编辑/合并/拆分/转换/生成 PDF/Word/Excel/PPT/文档/表格/幻灯片/WPS文件, Markdown转Word, 套模板, 标书排版, 修订标记, 批注, 表单填写, OCR, 公式重算, 演示文稿生成。覆盖：CLI快速操作、Markdown转docx管道、模板填充、中文排版、OOXML底层修补、PDF表单填写、reportlab生成、PPT设计系统、Excel公式重算。"
---

# Office Tools

## 产物呈现（强制）

本技能任何产出交付文件（docx/xlsx/pptx/pdf/png 等）的任务：

1. 文件落盘成功后立即登记：
   `python ~/.qidi/skills/office-artifact/scripts/card.py add <产物路径> --skill office-tools --note <一句话说明>`
2. 任务最终回复前必须输出卡片总览：
   `python ~/.qidi/skills/office-artifact/scripts/card.py show`
3. 不许只回一行文件路径就结束；卡片失败则降级为 Markdown 表格列出全部产物。


一站式办公文档工具集，覆盖 PDF / Word / Excel / PowerPoint 全格式操作。

## 依赖

- Python 3.x + 已安装库: python-docx, openpyxl, python-pptx, pypdf, pdfplumber, fpdf
- 可选增强: pandoc (Markdown→docx), Node.js + docx (备选渲染), LibreOffice (预览/转换/重算), pdftoppm (页面渲染)

运行 `python scripts/doctor.py` 检测环境并获取安装提示。

> **Windows PowerShell 注意**: JSON 参数需用 `--%` 停止解析或转义引号。

---

## 快速操作（CLI 脚本）

### PDF (`scripts/pdf_ops.py`)

```bash
python scripts/pdf_ops.py extract-text input.pdf [--json]     # 提取文本
python scripts/pdf_ops.py merge a.pdf b.pdf -o merged.pdf     # 合并
python scripts/pdf_ops.py split input.pdf [--range 1-5]       # 拆分
python scripts/pdf_ops.py rotate input.pdf 90 -o rotated.pdf  # 旋转
python scripts/pdf_ops.py info input.pdf                      # 元信息
```

### Word (`scripts/word_ops.py`)

```bash
python scripts/word_ops.py read input.docx [--json]           # 读取
python scripts/word_ops.py create -o out.docx '[JSON]'         # 创建
python scripts/word_ops.py edit input.docx '{"old":"new"}'     # 查找替换
python scripts/word_ops.py info input.docx                    # 结构信息
```

Word 创建 section 字段:

| type | 必填 | 可选 |
|------|------|------|
| paragraph | text | style, bold, size |
| heading | text | level |
| bullet | text | - |
| number | text | - |
| table | data | table_style |

### Excel (`scripts/excel_ops.py`)

```bash
python scripts/excel_ops.py read input.xlsx [--sheet S1] [--json] [--data-only] [--limit 50]
python scripts/excel_ops.py create -o out.xlsx '[JSON]'
python scripts/excel_ops.py edit input.xlsx '[{"row":1,"col":1,"value":"v"}]' [--sheet S1]
python scripts/excel_ops.py info input.xlsx
```

### PowerPoint (`scripts/ppt_ops.py`)

```bash
python scripts/ppt_ops.py read input.pptx [--json]
python scripts/ppt_ops.py create -o out.pptx '[{"title":"T","subtitle":"S","layout":0}]'
python scripts/ppt_ops.py info input.pptx
```

### Word 表格格式修复 (`scripts/docx_table_fixer.py`)

修复 python-docx 生成的表格常见显示问题（内容截断、未居中、缩进偏移等）：

```bash
python scripts/docx_table_fixer.py input.docx [output.docx]
```

自动修复 9 类问题：
1. 表格居中对齐
2. 表格宽度=100%页面可用宽度
3. 表格左缩进=0
4. 表格布局=fixed（列宽不漂移）
5. 表头行跨页重复
6. 单元格段落行距=单倍、无段前段后（防止内容被截断）
7. 单元格段落缩进全部清零（6种属性：left/leftChars/right/rightChars/firstLine/firstLineChars）
8. 移除cantSplit（防止大行跨页时被截断）
9. 列宽均分

**适用场景**: python-docx 生成的 docx 表格显示异常、暗标/标书格式合规修复

---

## 高级操作

### Markdown → Word

先组合内容为干净的 Markdown（不含元指令），再渲染：

```bash
# 首选 pandoc 引擎
python scripts/md_to_docx.py content.md output.docx [--reference templates/report-standard.docx] [--toc]

# 无 pandoc 时用 Node 引擎
node scripts/md_to_docx.mjs content.md output.docx --cjk
```

详见 [references/docx-pipeline.md](references/docx-pipeline.md)。

**核心规则**: 用户的需求文件 ≠ Markdown 输入。必须先写一个新的 .md 只含最终内容，再渲染。

### Word 模板填充

```bash
# 占位符替换 ({{token}})
python scripts/fill_template.py templates/contract.docx output.docx \
  --set title="合同标题" --set party_a="甲方" --set date="2026-01-01"

# 参考样式文档 (pandoc reference doc)
python scripts/md_to_docx.py body.md output.docx --reference templates/memo.docx
```

模板详见 [references/docx-templates.md](references/docx-templates.md)。

### 中文排版 (CJK)

- Node 渲染: 加 `--cjk`
- docx-js: 导入 `scripts/styles/zh-cn.js`
- 规则: 正文 1.5 倍行距 + 首行缩进 2 字符；标题黑体无缩进；eastAsia 字体必须显式声明

### OOXML 底层操作

解包 → 编辑 XML → 重新打包：

```bash
python scripts/office/unpack.py doc.docx unpacked/      # 解包
# 编辑 unpacked/word/document.xml (用 Edit 工具)
python scripts/office/pack.py unpacked/ output.docx --original doc.docx  # 打包
python scripts/office/validate.py output.docx            # 验证
```

支持：修订标记 (tracked changes)、批注 (comments)、图片插入、XML 级编辑。

```bash
python scripts/comment.py unpacked/ 0 "批注内容"           # 添加批注
python scripts/accept_changes.py input.docx output.docx  # 接受所有修订
```

### PDF 高级操作

| 操作 | 方法 |
|------|------|
| 提取表格 | `pdfplumber` → `page.extract_tables()` |
| 生成 PDF | `reportlab` (Canvas / Platypus) |
| 水印 | `pypdf` → `page.merge_page(stamp)` |
| 加密 | `pypdf` → `output.encrypt("password")` |
| 提取图片 | `pdfimages -j input.pdf prefix` |
| OCR 扫描件 | `pytesseract` + `pdf2image` |
| 表单填写 | 见 [references/form-filling-guide.md](references/form-filling-guide.md) |

reportlab 注意: 不要用 Unicode 上下标字符，用 `<sub>`/`<super>` 标签。

### Excel 公式与重算

**关键规则: 所有计算值必须是 Excel 公式，不要用 Python 算好后硬编码。**

```python
ws['B10'] = '=SUM(B2:B9)'        # ✓ 正确
ws['B10'] = df['Sales'].sum()    # ✗ 错误：静态值
```

公式重算（需要 LibreOffice）：

```bash
python scripts/recalc.py output.xlsx
```

输出 JSON 包含公式总数、错误数及位置。详见 [references/xlsx-advanced.md](references/xlsx-advanced.md)。

财务模型颜色规范: 蓝色=输入, 黑色=公式, 绿色=跨表引用, 红色=外部链接, 黄色填充=需审核。

### PowerPoint 专业生成

五步流程: 收集素材 → 确认环境 → 写生成脚本 → 执行自检 → 交付。

设计原则: 一个主色 + 1-2 辅色 + 一个强调色；统一视觉母题；每文本框设字符预算防溢出。

```bash
python -m markitdown deck.pptx   # 验证内容
```

CJK 字体: 同时设 `font.name` 和 `<a:ea>` 东亚字体。详见 [references/python_pptx_recipes.md](references/python_pptx_recipes.md)。

### 格式转换

参见 [references/conversion.md](references/conversion.md)。docx→PDF 需 LibreOffice。

---

## 脚本一览

| 脚本 | 用途 |
|------|------|
| `pdf_ops.py` / `word_ops.py` / `excel_ops.py` / `ppt_ops.py` | CLI 快速操作 |
| `md_to_docx.py` | pandoc Markdown→docx |
| `md_to_docx.mjs` | Node 备选 Markdown→docx |
| `fill_template.py` | 占位符模板填充 |
| `doctor.py` | 环境检测 |
| `preview.py` | 渲染页面为图片 (需 LibreOffice) |
| `recalc.py` | Excel 公式重算 (需 LibreOffice) |
| `comment.py` | Word 批注 |
| `accept_changes.py` | 接受所有修订 |
| `office/unpack.py` | docx 解包 |
| `office/pack.py` | docx 打包 |
| `office/validate.py` | OOXML 验证 |
| `office/soffice.py` | LibreOffice 包装 |
| `fill_fillable_fields.py` | PDF 表单填写 |
| `extract_form_field_info.py` | PDF 表单字段提取 |

## 参考文档

| 文件 | 内容 |
|------|------|
| `references/conversion.md` | 格式转换指南 |
| `references/docx-pipeline.md` | Markdown→docx 映射与边界情况 |
| `references/docx-templates.md` | 模板制作与占位符规范 |
| `references/advanced-reference.md` | PDF 高级操作 (pypdfium2, pdf-lib) |
| `references/form-filling-guide.md` | PDF 表单填写流程 |
| `references/python_pptx_recipes.md` | PPT 制作配方 |
| `references/python_pptx_skeleton.py` | PPT 生成骨架 |
| `references/editing.md` | PPT 编辑流程 |
| `references/xlsx-advanced.md` | Excel 公式与财务模型规范 |
