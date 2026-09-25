# Document Format Conversion

## docx/xlsx/pptx → PDF (需 LibreOffice)

LibreOffice CLI (`soffice`) 可实现无头转换：

```bash
soffice --headless --convert-to pdf --outdir <output_dir> <input_file>
```

示例：
```bash
# Word → PDF
soffice --headless --convert-to pdf --outdir ./output report.docx

# Excel → PDF
soffice --headless --convert-to pdf --outdir ./output data.xlsx

# PowerPoint → PDF
soffice --headless --convert-to pdf --outdir ./output slides.pptx
```

### 安装 LibreOffice

- Windows: 从 https://www.libreoffice.org/download/ 下载安装，确保 `soffice` 在 PATH 中
- 或指定完整路径: `"C:\Program Files\LibreOffice\program\soffice.exe" --headless ...`

## PDF → 文本

使用内置脚本：
```bash
python scripts/pdf_ops.py extract-text input.pdf
```

## PDF → Word (OCR 场景)

扫描版 PDF 无法直接提取文本，需要 OCR：
```bash
# 需安装: pip install pytesseract
# 需安装 Tesseract OCR 引擎
# 使用 pdf2image 将页面转为图片再 OCR
```

## 图片 → PDF

```python
from fpdf import FPDF
pdf = FPDF()
for img_path in image_list:
    pdf.add_page()
    pdf.image(img_path, x=0, y=0, w=210)  # A4 width
pdf.output("output.pdf")
```

## WPS 格式说明

- `.wps` (WPS Writer), `.et` (WPS Spreadsheets), `.dps` (WPS Presentation) — 旧格式
- 现代 WPS Office 默认使用 .docx/.xlsx/.pptx，与 Microsoft Office 完全兼容
- 旧 WPS 格式可在 WPS Office 中另存为 .docx/.xlsx/.pptx 后使用脚本处理
