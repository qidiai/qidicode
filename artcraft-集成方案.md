# ArtCraft（storytold）Craft 套件 · 本地仓库与工作流接入方案

> 生成日期：2026-10-12 · 代码位置：`G:\程序开发\artcraft\`（26 个仓库，浅克隆）
> 上游：https://github.com/storytold · 官网：https://getartcraft.com
> 许可：MIT OR Apache-2.0（商用无障碍）

---

## 一、仓库全景

### 12 个 Craft 应用（全部纯 Rust、alpha 阶段、内置 CLI + MCP server + Web/WASM 版）

| 仓库 | 对标 | 星数 | 就绪度(自评) | 对我们价值 |
|---|---|---|---|---|
| **wordcraft** | Word | 6.0k | ~60% | ★★★★★ 文档生成/校验核心 |
| **gridcraft** | Excel | 2.9k | ~50% | ★★★★☆ 表格计算/校验 |
| **pdfcraft** | Acrobat | 9.7k | 57%(全量)/94%(必备) | ★★★★★ PDF 装订/水印/表单 |
| **cadcraft** | AutoCAD | 2.7k | ~38% | ★★★☆☆ 图纸只读转档试点 |
| deckcraft | PowerPoint | 2.0k | ~55%(有Win崩溃bug) | ★★☆☆☆ 等beta |
| photocraft | Photoshop | 48k | early alpha | ★★☆☆☆ 图像批处理备用 |
| lightcraft | Lightroom | 10k | — | ☆ |
| filmcraft | Premiere | 10k | — | ☆ |
| vectorcraft | Illustrator | 7.1k | — | ☆ |
| effectcraft | After Effects | 5.2k | — | ☆ |
| designcraft | InDesign | 3k | — | ☆ |
| soundcraft | Pro Tools | 1.8k | pre-alpha ~40% | ☆ |

### 基础设施
- **craft-libs**：仅 craft-ui（egui 组件）+ craft-print（CUPS/Windows 打印）；格式库与 MCP 均为各应用私有 crate
- **craft-launcher**：应用商店式安装器（无 CLI 批量安装，部署走各应用 MSI）
- **artcraft**（主引擎）：Tauri+React 云端 AI 生成客户端（OmniGen API 62 模型），与 Craft 套件不共享代码
- **craft-fonts**：Noto Sans CJK SC 等 6 族字体，构建期可选嵌入

### 统一架构（所有应用一致）
```
分层 crate：格式解析(自研) → 引擎(一切皆命令) → CLI → MCP(stdio JSON-RPC) → egui UI(可替换) → web(wasm)
命令即 API：GUI 按钮 = 命令 ID + JSON 参数 = CLI/MCP 可调用的同一接口
双模式：headless（无窗口，服务器友好）/ --connect（桥接运行中 GUI，可截图点击）
```

---

## 二、各应用无人值守能力清单（与工作流直接相关）

### WordCraft（433 条命令，481/496 ribbon 覆盖 97%）
```powershell
wordcraft-cli convert in.docx out.pdf          # docx/doc(读)/odt/rtf/html/md/tex/txt/json/png/pdf 互转
wordcraft-cli info in.docx                     # 页数/字数/结构 → JSON
wordcraft-cli text in.docx                     # 纯文本抽取
wordcraft-cli render in.docx out.png --page 3  # 页面渲染成 PNG（视觉校验用）
wordcraft-cli run --file in.docx --cmd 'id={json}'... --save out.docx --author "AI精修"
wordcraft-cli mcp                              # MCP server（headless）
```
- MCP 工具：`execute`/`batch`/`open`/`save`(docx+pdf+png)/`type_text`/`select_text`/`inspect_document`(免截图回读校验)/`render_page`/`parity`
- **修订带作者署名**（`--author`）、批注、邮件合并（收件人可读 xlsx/CSV）
- 短板：东亚排版 25%、真实语料未验证、无原生打印、.doc 只读

### GridCraft（513/523 工作表函数 98%，24 个 MCP 工具）
```powershell
gridcraft-cli convert a.xlsx b.csv|json|html
gridcraft-cli eval '=PMT(0.05/12,60,10000)'    # 无头单公式求值
gridcraft-cli check dir\ --json                # ★计算正确性 oracle + xlsx 往返保真校验（可进 CI）
gridcraft-cli run --in a.xlsx --script steps.jsonl --out b.xlsx
gridcraft-cli mcp                              # 24 工具：write_range(=开头即公式)/read_range/insert_chart/create_table/sort...
```
- 独有：JSONL 脚本文件、`automate.recordActions` 宏录制（存入 xlsx）、MCP 可作库嵌入
- 短板：100MB 大文件打不开、图表往返丢失、VBA 不执行；输出一律 .xlsx

### PdfCraft（100+ 自动化工具）
```powershell
pdfcraft-cli combine a.pdf b.pdf --out merged.pdf   # 合并（书签/链接/表单/图层保留）
pdfcraft-cli split in.pdf --every 2 --out-dir d\
pdfcraft-cli extract in.pdf --pages 1,3 --out out.pdf
pdfcraft-cli edit in.pdf --out out.pdf --rotate 1:90 --delete 2
pdfcraft-cli run doc_watermark text="投标文件" ...  # 文本/图片/PDF页水印
pdfcraft-cli render in.pdf --page 1 --dpi 150 --out p.png
pdfcraft-cli text in.pdf                            # 阅读顺序文本提取
pdfcraft-cli audit in.pdf                           # 伪脱敏审计（CI 门禁）
pdfcraft-cli tools                                  # 列出全部工具及 JSON Schema
pdfcraft-cli mcp --compact                          # 省 token 模式
```
- 表单填写+制作（沙箱 JS）、PAdES 数字签名（RSA/ECDSA+时间戳+LTV）、RC4→AES-256 全解密、PDF/A-2b/3b preflight
- 短板：**OCR 仅英文**（中文扫描件继续走 rapidocr）、Word 导出丢版式、无 Office→PDF

### CADCraft（DXF 自研 R12–2018 全读 / DWG 经 acadrust R13–2018 读）
```powershell
cadcraft-cli convert 图纸.dwg 出图.pdf            # dwg→dxf/svg/png/pdf 一条命令，--window 取景
cadcraft-cli info 图纸.dxf                        # 结构 JSON
cadcraft-cli run 图纸.dwg --script-file batch.scr --save out.dxf --export out.pdf
cadcraft-cli mcp                                  # 工具：command_line/script/execute/query_entities/render
```
- **红线**：DWG 写出保真仅 ~15%（丢 proxy/ACIS/自定义对象）→ **只读+出图，绝不回存 DWG 交付**；SHX 大字体不读（回落 Noto/雅黑）；10MB DWG 转换 ~2 分钟；代码页 ANSI_936 支持良好
- CLI 零 GUI 依赖（纯 CPU），服务器/CI 可跑

---

## 三、接入我们工作流的路线图

### 现状对照（我们的相关 skill → 可替换/增强点）

| 我们的 skill | 现技术 | ArtCraft 接入点 |
|---|---|---|
| office-tools | python-docx/openpyxl + WPS COM | wordcraft-cli convert（docx→pdf 去 Office 依赖）、pdfcraft 装订全家桶 |
| bid-write / bid-creation | python-docx 生成 docx | wordcraft MCP（修订带作者、样式、TOC、表格、邮件合并） |
| bid-docx-fix | XML 级 hack | wordcraft `run --cmd` 命令化修改 + `inspect_document` 回读验证 |
| bid-perfect / office-canvas 精修闭环 | 人工批注→整改 | MCP `render_page` PNG 视觉校验 + `inspect_document` 结构校验 |
| bid-review | python-docx 抽取关键词 | wordcraft-cli text/inspect（更稳的结构抽取） |
| bid-diagram | matplotlib 出 PNG | 不变；插入 docx 仍走现有链 |
| bid-drawing | ezdxf（仅 DXF） | cadcraft-cli 补上 DWG 读取 + 出图预览（只读模式） |
| pdf-to-word-ocr | pymupdf + rapidocr（中文） | **不替换**（pdfcraft 无中文 OCR）；pdfcraft 补表单/签名/水印 |
| seal-extractor | OpenCV 抠章 | 不变；photocraft batch 可做备用图像批处理 |
| office-workbench | docx→HTML 预览 | 方案B：嵌入 wordcraft-web/gridcraft-web（官方 wasm 静态包）真编辑器 |

### Phase 0 · 本周可做（零风险，先跑起来）
1. 下载 3 个 Windows x64 portable zip（wordcraft/gridcraft/pdfcraft）解压到 `G:\程序开发\artcraft\bin\`，验证 CLI
2. 注册 MCP（`claude mcp add wordcraft -- wordcraft-cli mcp` 同理），跑通"打开→改→存→inspect 验证"最小闭环
3. 用 3-5 份**真实标书 docx** 跑 `wordcraft-cli render` + `text`，肉眼比对中文排版保真度（关键验证：仿宋/黑体/小四/行距/表格）

### Phase 1 · 两周内（进 skill 流水线）
1. **pdfcraft 接管 PDF 装订**：合并资质文件、加水印（`doc_watermark`）、页眉页脚、拆分章节，替换 pypdf 手搓段
2. **wordcraft render 进 bid-perfect**：每轮整改后渲染关键页 PNG 自查，替代部分 WPS COM 截图
3. **gridcraft check 进交付门禁**：商务标 xlsx 提交前跑计算正确性 + 往返保真校验
4. office-tools 增加 `craft_convert()` 包装（子进程调用 + 退出码检查），WPS COM 保留为打印级 PDF 的兜底

### Phase 2 · 试点观察（不进关键路径）
1. **cadcraft PoC**：拿 20-50 张真实投标图纸（含 SHX 中文标注、布局空间）跑 dwg→pdf/png，与 AutoCAD 打印并排比对；只读，不回写
2. **deckcraft**：等 beta（#40/#59 Windows 启动崩溃修复）再评估 PPT 路线
3. **wordcraft-web 嵌 office-workbench**：iframe 嵌入官方 wasm 包，做浏览器内真编辑预览
4. artcraft OmniGen API（62 模型）如需 AI 生图/生视频可作可选云端通道（按 credits 计费）

### 风险与对策（务必记住）
| 风险 | 对策 |
|---|---|
| 全家桶 alpha，docx 真实语料未验证 | wordcraft 只先做"转换/渲染/校验"，生成主力仍是 python-docx；跑分通过后再逐步交出编辑权 |
| 中文东亚排版 25% | 每版升级用固定标书样张集回归比对；打印级 PDF 暂留 WPS COM 兜底 |
| VBA 一律不执行、.doc 只读 | 存量含宏文档：wordcraft/deckcraft 透传保留宏字节，宏逻辑人工处理 |
| CADCraft DWG 写保真 15% | 铁律：只读+导出，回存 DWG 一律走 AutoCAD |
| 大 xlsx（>100MB）打不开 | 商务标超大表继续 openpyxl |
| 版本迭代极快（5天4版） | 固定版本号 + SHA256SUMS 校验后入 bin\，升级走手动回归 |

---

## 五、自有资产对照（2026-10-12 盘点：ArtCraft × 我们已有的东西）

### 资产清单
| 资产 | 位置 | 现状 |
|---|---|---|
| **cad-mcp / cad-mcp-pro**（自己二开） | `D:\cad-mcp`、`D:\cad-mcp-pro` | COM 驱动真实 AutoCAD/浩辰/中望：绘图/实体管理/图层/块插入/export_pdf/截图反馈(get_view)/中文NL命令 |
| **标书软件**（Flask/SQLite, waitress:5556） | `G:\程序开发\标书软件` | 招标采集（anhui collector）、知识库 RAG（tenders.db + v8/v10 修复后的向量检索）、plan_service 方案生成、bid_checker、**已有 .mcp.json** |
| **投标docx库** | `G:\投标docx库` | **1393 个真实标书 docx**（Word COM 跑批 1343 文件成果，8-22 看门狗血泪史） |
| **QIDI Code**（本 CLI，Rust，支持 MCP） | 本工作区 | craft 全家桶 MCP server 的天然宿主 |
| **AI 数据资产** | `G:\数据资产` | train.jsonl 975MB + curated v1 四件套（sft_tooluse 461 样本）+ data_refinery 每周跑 |
| **参考仓库库** | `G:\数据资产\reference` | 5 仓（fable5 提示词/system_prompts_leaks/oh-my-pi/Trace_Inversion/stolen-thoughts），训练栈 llama-factory |
| **Skills 家族** | `.qidi/skills` | office-tools(WPS COM)/pdf-to-word-ocr(rapidocr)/seal-extractor/office-workbench/office-canvas/bid-* |

### 五个交叉结论

**A. CAD 已成双保险格局**：自己搞的 cad-mcp-pro（COM+真AutoCAD）走"全保真编辑"路线；ArtCraft cadcraft 走"免安装 headless 读图/出图"路线。两者 MCP 工具面几乎同构（cad-mcp-pro 的 get_view 截图反馈与 cadcraft 的 render/screenshot 同源思想），**共存不冲突**：服务器/CI 用 cadcraft-cli 批量 dwg→pdf/png，人机协作精修用 cad-mcp-pro 驱动真 AutoCAD。

**A+. PDF 资产也是成体系的，pdfcraft 是"补缺"不是"替换"**：
我们已有的（标书软件 `services/` + skills）：
- `bid_parser.py`(40KB) + `tender_pdf_parser.py`：pdfplumber 招标文件解析（表格结构强项，保留）
- `doc_exporter.py`(46KB)：pymupdf 方案导出 PDF（保留）
- `ocr_service.py` + `pdf_ocr_rebuild.py`：PaddleOCR 扫描件（有 oneDNN 坑，现"拒收扫描件"策略）+ pdf-to-word-ocr skill(rapidocr)：**中文 OCR 保留，pdfcraft 无中文 OCR**
- `skill_assets/office-tools/fill_pdf_form_with_annotations.py`：PDF 表单填写（→ pdfcraft 表单引擎更强，可升级）
- 知识库 PDF 管线：import_kb_clean 384 篇入库，F 盘 35 个 PDF 实测（27 文本层/8 扫描件拒收）
pdfcraft 补我们**完全没有**的：
- 合并/拆分/提取页（书签/链接/表单/图层保留，替代 pypdf 手搓 final_pdf_split.py）
- **加水印/页眉页脚**（doc_watermark，投标文件刚需，现无）
- **PAdES 数字签名**（RSA/ECDSA+RFC3161 时间戳+LTV，现无；对外正式签章仍走 Epoint 客户端）
- **伪脱敏审计 audit**（防"假脱敏"泄密质检，现无，可进交付门禁）
- **加密解密 RC4→AES-256 全版本**（带密码招标文件，pypdf 处理有限）
- PDF/A-2b/3b preflight + 983 pdf.js 语料 0 崩溃健壮性

**B. 1393 docx 语料库 = wordcraft 的试金石**：wordcraft 官方自认 beta 门槛 #1 是"真实世界 docx 验证"且无语料——**我们有**。直接动作：从投标docx库抽 50 份多样本（房建/市政/改造、含表格/图片/页眉页脚/修订），跑 `wordcraft-cli render+text` 批量回归，产出保真度报告。此报告同时决定 Phase 1 能否放行。

**C. Word COM 看门狗的血泪坑，wordcraft 全绕开**：8-22 实锤的 COM 无超时挂死/并发互斥/RPC 通道死亡，在 wordcraft-cli（纯子进程+退出码+无COM）上不存在，且可并行。未来新增 .doc 老文档入库可用 wordcraft 替代 Word COM 跑批（它支持 .doc 只读）——但先过 B 的保真度验证。

**D. craft MCP × 标书软件 .mcp.json**：标书软件已接 MCP 生态，wordcraft/gridcraft/pdfcraft 三个 server 一行配置挂进去，其 ai_tender_extract / plan_service 即可调用 docx 编辑/表格计算/PDF 装订能力。

**E. SFT 数据飞轮新语料域**：craft 全家桶"一切皆命令"（wordcraft 433 / gridcraft 270 / deckcraft 226 条命令 + JSON Schema）是天然 tool-use 训练域，refinery 每周跑可自动积累 craft 工具调用轨迹，对接 llama-factory 训练栈。

### Phase 0 修订（结合自有资产）
1. 从 `G:\投标docx库` 抽样 50 份 → wordcraft-cli 批量 render/text 保真度回归（**核心验证**）
2. QIDI Code / 标书软件 `.mcp.json` 注册 wordcraft + pdfcraft MCP server
3. cad-mcp-pro 定位不变（编辑通道）；cadcraft 试点只读出图（需 SHX 中文图纸实测）

---

## 六、版本基线（2026-10-12 记录）
- wordcraft/gridcraft/deckcraft/cadcraft/soundcraft：**v0.5.0**（10-11 发布，v0.3.0 为 10-08）
- pdfcraft **v0.6.0**、photocraft **v0.7.0**、filmcraft **v0.6.0**、craft-launcher v0.3.2
- 发行包：每应用 24-27 个资产（msi/portable zip × x64/arm64/x86、AppImage/deb/rpm/flatpak、dmg、`*-web-*.zip`、SHA256SUMS.txt）
