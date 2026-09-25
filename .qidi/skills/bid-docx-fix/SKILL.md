---
name: bid-docx-fix
description: >
  标书DOCX修复技能：处理Word文档的XML级修改，包括表格插入、段落删除/插入、
  关键词格式统一（"关键词。"→"关键词："）、图片位置修正等。
  适用于整改标书格式、补充缺失内容、批量替换段落开头格式等操作。
  当用户要求"修复标书"、"补表格"、"改格式"、"13.4节"、"技术参数表"、
  "冒号格式"、"批量替换段落开头"时使用。
metadata:
  short-description: "标书DOCX XML级修复"
---

# 标书DOCX修复技能

## 产物呈现（强制）

本技能任何产出交付文件（docx/xlsx/pptx/pdf/png 等）的任务：

1. 文件落盘成功后立即登记：
   `python ~/.qidi/skills/office-artifact/scripts/card.py add <产物路径> --skill bid-docx-fix --note <一句话说明>`
2. 任务最终回复前必须输出卡片总览：
   `python ~/.qidi/skills/office-artifact/scripts/card.py show`
3. 不许只回一行文件路径就结束；卡片失败则降级为 Markdown 表格列出全部产物。


## 核心原则

**DOCX本质是ZIP，用python zipfile直接读写document.xml最可靠**（python-docx存在命名空间检测bug，不可靠）。

## 关键陷阱与教训

### 陷阱1：段落删除后索引失效
`re.finditer(r'<w:p(?: [^>]*)?>.*?</w:p>', doc_xml)` 提取的段落列表在删除后索引会偏移。
**必须每次删除后重新调用 finditer**，不能用旧的索引列表继续操作。

### 陷阱2：表格插入位置错误导致XML损坏
表格XML（`<w:tbl>...</w:tbl>`）不是`<w:p>`段落，不能混入段落列表的join操作。
**正确做法**：用字符串切片 `doc_xml[:insert_pos] + table_html + doc_xml[insert_pos:]` 插入。

### 陷阱3：第二步修改破坏第一步结果
错误模式：第一步修改doc_xml（删段落+插表），第二步重新提取所有段落再`''.join(new_paras)`——
表格HTML不在`<w:p>`里，join时丢失，导致整个document根标签和表格全部消失。
**正确做法**：第二步修改时**直接对doc_xml做字符串替换**，不重新构建整个doc_xml。
使用 `re.finditer` 从后往前遍历（`reversed()`），避免位置偏移。

### 陷阱4：正则中文字符问题
`\u3002`（。）、`\uff1a`（：）、`\u4e00-\u9fff`（中文）等Unicode转义在正则中需单独测试，
直接写中文句号可能导致正则引擎解析错误。

### 陷阱5：文件锁定
PowerShell脚本中`2>nul`在Windows环境下会引发编码错误。
文件被锁定时用 `os.rename()` 改名释放，或写入新文件名后再覆盖。

## 标准操作流程

### 1. 备份先行
```python
import shutil, os
BACKUP = r'...\原始文件.docx'
DST = r'...\修复后文件.docx'
shutil.copy2(BACKUP, DST)
```

### 2. 提取段落（带属性）
```python
import zipfile, re
from io import BytesIO

with zipfile.ZipFile(DST) as z:
    doc_xml = z.read('word/document.xml').decode('utf-8')

# 关键：用非贪婪匹配，正确处理带属性的<w:p w14:paraId="...">
paras = list(re.finditer(r'<w:p(?: [^>]*)?>.*?</w:p>', doc_xml, re.S))
```

### 3. 删除段落（从后往前）
```python
delete_indices = [755, 756, 757]  # 要删除的段落索引
for di in sorted(delete_indices, reverse=True):
    start = paras[di].start()
    end = paras[di].end()
    doc_xml = doc_xml[:start] + doc_xml[end:]
# 删除后必须重新提取段落列表！
paras = list(re.finditer(r'<w:p(?: [^>]*)?>.*?</w:p>', doc_xml, re.S))
```

### 4. 插入表格
```python
table_html = '''<w:p>表标题段落</w:p>
<w:tbl>...表格内容...</w:tbl>'''
insert_pos = paras[target_index].end()
doc_xml = doc_xml[:insert_pos] + table_html + doc_xml[insert_pos:]
```

### 5. 批量修改段落开头格式（从后往前）
```python
JUHAO = '\u3002'  # 。
COLON = '\uff1a'  # ：

paras2 = list(re.finditer(r'<w:p(?: [^>]*)?>.*?</w:p>', doc_xml, re.S))
for p in reversed(paras2):  # 从后往前，避免位置偏移
    para_xml = p.group(0)
    text = ''.join(re.findall(r'<w:t(?: [^>]*)?>([^<]*)</w:t>', para_xml))
    stripped = text.strip()
    m = re.match(r'^(.+?)' + JUHAO + r'(.+)$', stripped)
    if m and re.match(r'^[\u4e00-\u9fff]{2,15}$', m.group(1)):
        new_text = m.group(1) + COLON + m.group(2)
        start, end = p.start(), p.end()
        doc_xml = doc_xml[:start] + para_xml.replace(text, new_text, 1) + doc_xml[end:]
```

### 6. 保存
```python
with zipfile.ZipFile(DST, 'w', zipfile.ZIP_DEFLATED) as zf_out:
    zf_out.writestr('word/document.xml', doc_xml.encode('utf-8'))
```

## 验证清单

修复完成后必须检查：
- [ ] `len(doc_xml)` 合理（不应比原文小太多）
- [ ] `'<w:document' in doc_xml` 为True（根标签存在）
- [ ] `'<w:body>' in doc_xml` 为True（body标签存在）
- [ ] `doc_xml.count('<w:tbl>')` 等于预期表格数
- [ ] 目标段落/表格内容正确

## 常用表格模板

```python
rows = [
    ('1','类别A','参数1','值1','单位'),
    ('2','类别B','参数2','值2','单位'),
]

table_html = '<w:p><w:pPr><w:jc w:val="center"/><w:spacing w:line="440" w:lineRule="exact"/></w:pPr>'
table_html += '<w:r><w:rPr><w:rFonts w:ascii="宋体" w:hAnsi="宋体" w:eastAsia="宋体"/>'
table_html += '<w:sz w:val="28"/><w:szCs w:val="28"/></w:rPr><w:t>表X-Y 表题</w:t></w:r></w:p>'
table_html += '<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/>'
table_html += '<w:tblW w:w="9072" w:type="dxa"/>'
table_html += '<w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="000000"/>'
table_html += '<w:left w:val="single" w:sz="4" w:space="0" w:color="000000"/>'
table_html += '<w:bottom w:val="single" w:sz="4" w:space="0" w:color="000000"/>'
table_html += '<w:right w:val="single" w:sz="4" w:space="0" w:color="000000"/>'
table_html += '<w:insideH w:val="single" w:sz="4" w:space="0" w:color="000000"/>'
table_html += '<w:insideV w:val="single" w:sz="4" w:space="0" w:color="000000"/>'
table_html += '</w:tblBorders></w:tblPr>'
table_html += '<w:tblGrid><w:gridCol w:w="800"/><w:gridCol w:w="2000"/>'
table_html += '<w:gridCol w:w="2800"/><w:gridCol w:w="2072"/></w:tblGrid>'

for i,(seq,cat,name,val,unit) in enumerate(rows):
    fill='D9E2F3' if i==0 else 'FFFFFF'
    table_html += f'''<w:tr><w:trPr><w:height w:val="400" w:hRule="atLeast"/></w:trPr>
    <w:tc><w:tcPr><w:vAlign w:val="center"/><w:shd w:fill="{fill}"/></w:tcPr>
    <w:p><w:pPr><w:jc w:val="center"/><w:spacing w:line="440" w:lineRule="exact"/></w:pPr>
    <w:r><w:rPr><w:rFonts w:ascii="宋体" w:hAnsi="宋体" w:eastAsia="宋体"/>
    <w:sz w:val="28"/><w:szCs w:val="28"/></w:rPr><w:t>{seq}</w:t></w:r></w:p></w:tc>
    ...（其他列类似）'''
table_html += '</w:tbl>'
```

## 文件格式规范参考

- 四号字：`<w:sz w:val="28"/>`（半磅，28半磅=14pt）
- 三号字：`<w:sz w:val="32"/>`（32半磅=16pt）
- 行距22磅：`<w:spacing w:line="440" w:lineRule="exact"/>`（440 twips = 22pt）
- 页边距上2.5cm其余2.0cm：在sectPr中设置
- 首行缩进2字符：`<w:pPr><w:ind w:firstLineChars="200" w:firstLine="560"/></w:pPr>`
