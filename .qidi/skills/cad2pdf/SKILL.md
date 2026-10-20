---
name: cad2pdf
description: >
  CAD 图纸转 PDF 技能（DXF 优先的高保真流水线）：DWG→DXF（ODA File Converter）→
  cadcraft DXF→PDF 矢量输出；含模型/布局空间识别、SHX 字体风险、矢量/光栅与文本层
  完整性核验。当用户要求"CAD转PDF"、"DWG转PDF"、"DXF转PDF"、"图纸转PDF"、
  "CADpdf"、"CAD2pdf"、"图纸格式转换"、"转出来的PDF不清晰/不完整"、"/cad2pdf"时使用。
metadata:
  short-description: "CAD 图纸转 PDF（DXF 优先保真流水线）"
---

# CAD 图纸转 PDF（DXF 优先）

## 先决：产物登记（强制）

交付任何产物文件（pdf/png/docx 等）后：

1. 文件落盘成功后立即登记：
   `python ~/.qidi/skills/office-artifact/scripts/card.py add <产物路径> --skill cad2pdf --note <一句话说明>`
2. 回复前回读当前任务产物卡片：
   `python ~/.qidi/skills/office-artifact/scripts/card.py show`
3. 回退：只有文件路径且不接受产物卡片失败，降级为 Markdown 列表列出全部产物。

## 为什么"转出来不完整"（先诊断再转）

常见根因，按出现频率：

1. **布局空间被忽略**：cadcraft 只渲染模型空间（model space）。施工图通常把图框/视口放在布局空间（paper space），直接转只得到裸图形。
2. **光栅化路径**：把图纸打印成图片再拼 PDF → 文字不可搜索、缩放模糊（OCR 才能读）。
3. **SHX 大字体缺失**：cadcraft 不读 SHX，文字回落 Noto/微软雅黑——内容可读但字形与出图不一致。
4. **DWG 直读保真低**：cadcraft 的 DWG 读取（acadrust）保真度低于其自研 DXF 解析器（R12–2018 全支持）。

## 保真阶梯（默认 L2）

| 级 | 路径 | 保真点 | 依赖 |
|---|---|---|---|
| **L1 原生发布** | AutoCAD/中望/浩辰 COM 发布 PDF | 布局空间、SHX、线宽、打印样式全保真；**正式交付首选** | 需本机 CAD + COM（可与 `D:\cad-mcp-pro` 生态配合） |
| **L2 DXF 优先（默认）** | DWG →ODA→ DXF →cadcraft→ PDF（或 DXF 直达） | 矢量、几何最完整、无 CAD 依赖；模型空间为准 | ODA File Converter + cadcraft-cli |
| L3 DWG 直读 | cadcraft DWG→PDF | 简单图可用，复杂图保真下降 | cadcraft-cli |
| L4 光栅兜底 | 渲染 PNG 拼 PDF | 仅当以上全不可用；文字不可搜索 | cadcraft-cli + pymupdf |

**默认执行 L2；`--check` 会自动检测布局空间实体，若存在则提示改用 L1。**

## 使用

```bash
# 单文件（默认 L2）
python .qidi/skills/cad2pdf/scripts/cad2pdf.py <输入.dwg|.dxf> <输出.pdf> --check

# 取景裁剪（整模型空间全量输出常有几十 MB —— 实测 4.6MB 施工图 → 47MB PDF）
python .../cad2pdf.py in.dwg out.pdf --window x0,y0,x1,y1 --check

# 显式指定级别
python .../cad2pdf.py in.dwg out.pdf --level 3      # DWG 直读
python .../cad2pdf.py in.pdf out.pdf --level 4      # 光栅兜底（输入须为 dwg/dxf）

# 批量（目录内全部 dwg/dxf）
python .../cad2pdf.py --batch <输入目录> <输出目录> --check

# 版本（ODA 输出 DXF 版本；cadcraft 支持 R12–2018）
python .../cad2pdf.py in.dwg out.pdf --version ACAD2018
```

## 核验（`--check` 自动执行，也可单独跑）

`python .../cad2pdf.py --check-only <输出.pdf> [--source <in.dxf>] [--full-check]`：

- **页数/图幅**：页数与图幅尺寸
- **矢量/光栅判定**：`page.get_drawings()` 路径数 vs 图像数——路径 0 且图像>0 即光栅化产物，警告
- **文本层抽查**：可抽取字符数（<20 字警告：可能 SHX 缺失或图纸全为几何）
- **布局空间检测**（输入为 dxf 时用 ezdxf）：paperspace 实体数 >0 → 提示该图应走 L1 或 --window 取景
- **大文件性能护栏（默认开启）**：PDF>20MB 跳过 get_drawings（47MB PDF 上此调用 >60 秒）、DXF>30MB 跳过 ezdxf 解析；`--full-check` 强制全量
- 输出 JSON 一行，供后续技能消费

## 已知边界（必须向用户说明）

- cadcraft 仅模型空间：布局出图的图纸 L2 会丢图框/视口 → L1
- SHX 不读：文字回落 Noto/雅黑，字形不同、内容不丢
- 线宽/CTB 打印样式不参与：L2 是屏幕矢量，非打印级
- DWG 写出保真 ~15%（本技能只读，不写 DWG）
- 大批量时 ODA 转换单文件约数秒；10MB DWG 全链路约 2 分钟量级

## 与相邻技能的分工

- **bid-drawing**：图纸审查、目录与图号核对、从 PDF 提取图纸页 → 本技能负责**格式转换与保真核验**
- **pdf_bindery / office-tools**：PDF 装订与办公文档转换，不处理 CAD 矢量
- **cad-mcp-pro（D:\cad-mcp-pro）**：需要编辑/全保真出图时走真 CAD 的 COM 通道（L1）
