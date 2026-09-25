---
name: bid-drawing
description: >
  标书施工图纸处理与质量检查技能：解析/审查 CAD（.dwg/.dxf）文件、从招标文件 PDF
  提取施工图纸页、核对图号/图幅/目录一致性、转换高质量 PNG 插入 Word、标注缺图风险。
  当用户要求"审图纸"、"CAD 检查"、"图纸审查"、"dwg 转 pdf/png"、
  "目录与图纸对照"、"检查缺图"、"/bid-drawing"时使用。
metadata:
  short-description: "标书 CAD/PDF 图纸处理与质量审查"
---

# 标书施工图纸处理技能

## 产物呈现（强制）

本技能任何产出交付文件（docx/xlsx/pptx/pdf/png 等）的任务：

1. 文件落盘成功后立即登记：
   `python ~/.qidi/skills/office-artifact/scripts/card.py add <产物路径> --skill bid-drawing --note <一句话说明>`
2. 任务最终回复前必须输出卡片总览：
   `python ~/.qidi/skills/office-artifact/scripts/card.py show`
3. 不许只回一行文件路径就结束；卡片失败则降级为 Markdown 表格列出全部产物。


施工图是技术标评审的硬材料，缺图、图号不连续、目录与图不一致都会导致扣分甚至废标。
本技能覆盖四类能力：

1. **dwg/dxf 解析** — 用 ezdxf 提取实体、图层、图块、页面设置
2. **PDF 图纸识别** — 用 PyMuPDF 扫描招标 PDF 中的图纸页，转高清 PNG
3. **图纸目录一致性审查** — 对照图号、图幅、比例、内容描述
4. **缺图/冗余/重复审查** — 与招标文件图纸清单交叉校验

## 前置依赖

```bash
pip install ezdxf pymupdf Pillow python-docx matplotlib
```

DWG格式需 [ODA File Converter](https://www.opendesign.com/guestfiles/oda_file_converter)（免费），
未安装时工具会输出结构化引导；DXF无需任何额外软件。

## 使用流程

### 场景 1：审一份待投标的施工组织设计，核查附图质量

```bash
# 批量检查 DOCX 内嵌图纸和引用
python scripts/drawing_audit.py <投标书.docx> --project "<项目名称>"

# DXF 图纸元数据提取（R2新增：图号/图名/比例/图幅/图层语义/天正兼容）
python scripts/dxf_meta.py <图纸.dxf 或目录>

# 目录-图纸交叉比对（R3新增：四清单 missing/extra/name_mismatch/duplicates）
python scripts/compare_catalog.py --catalog <图纸清单.txt或index.json> --drawings <dxf_meta输出.json>
```

dxf_meta 输出JSON字段：
- `fig_id`（来源filename/titleblock）、`fig_name`（含置信度high/medium/low）
- `scale`（1:xxx）、`sheet_size`（A系列+来源layout_setup/geometry_est）
- `units`（$INSUNITS归一）、`layer_semantics`（GB专业分类实体占比）
- `proxy_entities`（天正代理实体数，>0提示人工核验）、`recover_mode_used`

compare_catalog 输出四清单（图号归一化：全/半角、破折号五胞胎、"图"前缀、前导零）：
- `missing[]` 清单有图无纸 / `extra[]` 有图无清单
- `name_mismatch[]` 图号同图名差异（相似度<0.6）
- `duplicates[]` 同图号多张
输出 JSON 报告，包含：
- 图号列表（含页码、图名、图幅、比例）
- 缺失图清单（与招标文件图纸清单对比）
- 重复/冗余图清单
- 图层命名规范性（`A-WALL`/`A-DIM` 等应遵循行业规范）
- 图块统计（柱/梁/板/门窗等）

### 场景 2：从招标文件 PDF 提取施工图纸页

```bash
python scripts/pdf_extract_drawings.py <招标文件.pdf> --output-dir drawings/
```

- 自动识别图纸页（页面上出现图纸边界/图框/图号）
- 每页转 300dpi PNG + 原始 PDF 切片
- 输出图号列表（供代理后续人工核对）

### 场景 3：DWG/DXF → 高清图 PNG（供插入 Word）

```bash
python scripts/dwg_to_png.py <图纸.dwg|.dxf> -o <输出目录> --dpi 300
```

- 渲染引擎（R1改造 2026-09-07）：默认 `official`（ezdxf官方MatplotlibBackend——
  真实文字渲染/全实体支持/finalize自动定界）；`--engine legacy` 回滚旧自研渲染器
  （已知缺陷：文字"Aa"占位/缺INSERT/SPLINE画不出）
- 布局选择：非空 Paper space 优先，全空则回退 Model space（空布局不出白图）
- 天正/损坏DXF自动走 `ezdxf.recover` 容错（输出 `recover_used: true` 提示人工复核）
- DWG输入：自动经 ODA File Converter 转换（未装则结构化引导，不再裸报错）

## 图纸质量铁律（合肥 AI 评标实战经验）

1. **图号连续**：从 1 到 N 不能缺（缺 1 张 = 被评"编制不完整"扣分项）
2. **图幅统一**：施工图一般 A1/A2，不能 A4 糊弄
3. **比例标注**：每张图必须有比例（如 1:100、1:50），缺比例直接扣分
4. **图名对应**：图名必须与招标文件图纸清单名称一致
5. **指北针/比例尺/图例**：三件套齐全
6. **CAD 源文件备份**：投标用 PDF 必须从 CAD 导出（别截图糊图）
7. **图纸与方案互引**：方案正文必须写"见图 X-X"，图中也要有方案引用点
8. **总张数合理**：一般市政类 20~40 张（徐老家 9 图偏少，评分受影响）

详见 references/drawing-checklist.md（完整审查项）和 references/cad-style-guide.md（行业规范）。
