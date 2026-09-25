---
name: pdf-to-word-ocr
agent_created: true
summary: 将纯扫描（图片型、无内嵌文字）的 PDF 通过 OCR 转换为可编辑的 Word .docx，支持中文，全程自包含。
description: >
  当用户需要把"扫描件"PDF（pypdf/pdfplumber 提取不到文字、chars=0）转成 Word 时使用。
  适用场景：招标文件、合同、证书等扫描 PDF。本机无 tesseract/poppler/paddleocr，
  因此采用 pymupdf 渲染 + rapidocr-onnxruntime 中文 OCR + python-docx 组装的纯 pip 方案。
---

# 扫描件 PDF → Word（OCR）转换

## 产物呈现（强制）

本技能任何产出交付文件（docx/xlsx/pptx/pdf/png 等）的任务：

1. 文件落盘成功后立即登记：
   `python ~/.qidi/skills/office-artifact/scripts/card.py add <产物路径> --skill pdf-to-word-ocr --note <一句话说明>`
2. 任务最终回复前必须输出卡片总览：
   `python ~/.qidi/skills/office-artifact/scripts/card.py show`
3. 不许只回一行文件路径就结束；卡片失败则降级为 Markdown 表格列出全部产物。


## 触发场景
用户说"把这个 PDF 转成 Word / docx"，且 PDF 是**扫描图片型**（提取文字为 0）。

## 先判断是否为扫描件
```python
from pdfplumber import open as plumber_open
with plumber_open(path) as pdf:
    pg = pdf.pages[0]
    print(len(pg.chars), len(pg.images))   # chars=0 且 images>0 → 扫描件，需 OCR
```
- 若 `chars` > 0：可直接用 `pdfplumber.extract_text()` / `extract_tables()` + python-docx，无需 OCR（更快更准）。
- 若 `chars` = 0：走下方 OCR 流程。

## 环境（系统 Python）
- 解释器：`python`（本机 3.12.x，实测已装齐依赖：pymupdf / rapidocr-onnxruntime / python-docx / pypdf / pdfplumber）
- 如新机缺依赖再装（**不要用裸 pip**，用 `python -m pip`）：
  `python -m pip install pymupdf rapidocr-onnxruntime python-docx pypdf pdfplumber`
- 注意：Git Bash 下路径分隔用 `/`。

## 流水线
1. **渲染**：`fitz.open(path)` → 逐页 `page.get_pixmap(dpi=200).save(png)`。（用 `len(doc)`，不要 `doc.pages`）
2. **OCR**：`from rapidocr_onnxruntime import RapidOCR; eng=RapidOCR()`，对每张 PNG 调用 `out,_ = eng(png)`。
   - 返回 `out` 为 `[(bbox, text, score), ...]`，已大致按阅读顺序（上→下、左→右）。
   - ⚠️ `score` 是**字符串**，打印/格式化必须用 `float(score)`，否则 `:.2f` 报错。
3. **组装 docx**：
   ```python
   from docx import Document
   d = Document()
   d.styles['Normal'].font.name = 'SimSun'   # 中文友好
   d.styles['Normal'].font.size = Pt(10.5)
   for page_lines in all_pages:
       # 除第 1 页外每页前先 d.add_page_break()
       for text, score in page_lines:
           d.add_paragraph(text)
   d.save(out_docx)
   ```

## 性能与耗时
- 中文 OCR 约 4–10 秒/页（CPU，200 DPI）。108 页 ≈ 12 分钟。
- 务必 `run_in_background` 跑全量，先对第 1 页做 pilot 验证识别质量与速度。

## 已知现象（向用户说明）
- 扫描件转 Word 本质是 OCR 文本，原 PDF 的**表格线、红色印章**会作为识别文本出现。
- 印章压字处（如封面公司名+红章重叠）识别可能有少量误差——属扫描件转换固有现象，成品为**可编辑** docx，便于用户校对。
- 输出体积远小于原扫描 PDF（文本 docx 几十 KB vs 原数十 MB）。

## 清理
- 渲染的临时 PNG 占空间（108 页约 26MB），转换完成后 `rm -rf` 临时目录，仅保留脚本与日志。
