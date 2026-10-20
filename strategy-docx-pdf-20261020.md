# 自主文档智能层 · 战略方案（ArtCraft 接入工程复盘后的转向）

> 2026-10-20 · 指挥官起草 · 待 glm-5.3 审计
> 背景：ArtCraft（storytold）接入工程 Phase 1 收官（W1-W9 全过，9 轮独立审计 58→96 分），cad2pdf 联合审计（glm-5.3 + step-5 双 63/100）确认其 alpha 质量边界清晰；本方案回答“接下来做我们自己的什么”。

## 一、ArtCraft 优势继承清单（全部有实证）

| 优势 | 我们的实证 | 继承方式 |
|---|---|---|
| MCP-first 架构 | 三件套 16/21/161 工具 stdio 握手全通 | 自建 docx MCP 用同一架构思想 |
| 命令即引擎（GUI=命令ID+JSON=CLI/MCP 同接口） | craft_bridge/bindery/gate 全部按此消费 | 整改引擎命令集按此设计 |
| headless 优先 | 无 GUI 依赖服务器可跑 | 产线集成前提 |
| docx 原生 OOXML 写入 | 保真 ~60% 但路线对 | 继续 python-docx，不重写版式 |
| 反向优势（坑位图） | 东亚排版 25%/SHX 丢弃/xref 丢失/几何漂移/PDF 文本层 88.5% | 铁律：不重写编辑器，成熟引擎上做智能层 |

## 二、开源 landscape（已扫，2026-10-20）

| 类别 | 项目 | 判断 |
|---|---|---|
| 完整套件 | **OnlyOffice Docs** | 开源版式保真最强 + Document Builder API，可自托管——最值得关注 |
| 完整套件 | LibreOffice | UNO 可替代 WPS COM，但挂起风险同源 |
| MCP-Word | Office-Word-MCP-Server | python-docx 包 MCP，简单稳定能力浅——自建 MCP 参考底样 |
| MCP-Excel | excel-mcp-server（openpyxl 系） | 同上 |
| PDF 工具 | **Stirling-PDF** | 50+ 工具自托管 web，可替换部分 fitz 脚本 |
| 排版引擎 | Typst（MIT） | PDF 文本层完美但不出 docx——仅内部预览件 |
| docx SDK | python-docx / docx4j / POI | docx4j track-changes 最全（疑难参考） |
| CAD→PDF | ODA 27.1.0（在用） | 免费路径已最优 |

**关键结论：开源界不存在版式级可编辑 PDF**（全是注释/表单/裁剪级）。“PDF 好修改”的工程正解 = **docx 真源 + PDF 锚点回写**，不是改 PDF。

## 三、用户需求解读

“必须要再 WPS 里面或者 Word，实际就是 PDF 里面好修改的东西”：
- 投标业务现实：docx（编制/整改工作面）+ PDF（交付/评版面）双格式；客户与评标专家活在 WPS/Word 和 PDF 里
- 交付物 PDF 天生不可改；“好修改”的工作面必须回到 docx 真源
- **结论：自研 = 让 docx 真源与 PDF 交付物之间改起来无缝，而不是替换任何一侧**

## 四、架构提案（三层）

```
┌─ 智能面（自研轻量 MCP，借 ArtCraft 架构思想，不重写引擎）
│    命令集：整改/修订/diff/门禁/渲染 —— 包住 python-docx + WPS COM
├─ 真源层（docx，WPS/Word 直接可编）
│    python-docx 生成 + 批注锚定整改 + 版本 diff + --author 修订审计链
│    + final_gate 质量门禁（已有：伪脱敏/xlsx 校验）
└─ 交付层（PDF）
     WPS COM 打印级（已验证最保真）→ fitz 后处理（书签/水签）
     + PDF 锚点映射：客户提修改 → 定位 docx 真源 → 改 → 重打印
```

## 五、资产收编（8 轮工单的零件全部入位）

| 已有资产 | 在新架构中的角色 |
|---|---|
| office-canvas 整改闭环（SKILL+canvas.py） | P0 整改引擎的雏形（去 COM 依赖后放大） |
| craft_verify（wordcraft CLI 自查） | 智能面可选加速器（冻结二进制，零成本） |
| final_gate（标书软件，W7a/b） | 真源层质量门禁（fitz 伪脱敏+gridcraft check 已在产线） |
| pdf_bindery / redact_audit | 交付层工具（pdfcraft 主 + pymupdf 兜底双引擎） |
| cad2pdf（待 SHX 补丁） | 图纸参考副本管线 |
| ArtCraft 冻结集成 | 退位为可选加速器，不在关键路径 |

## 六、建造顺序

| 阶段 | 内容 | 风险 |
|---|---|---|
| **P0** | docx 整改引擎：comment→fix→verify 闭环，去 WPS COM 依赖（纯 python-docx + fitz，COM 只留打印） | 低——office-canvas 已验证雏形 |
| **P1** | 自建 docx MCP server：命令集借 wordcraft 433 命令清单做需求参考，实现用 python-docx | 中——命令面设计工作量 |
| **P2** | PDF 锚点映射 + 交付后修改回流 | **高——docx↔PDF 位置映射是研究级难题**，需 P0/P1 稳定后验证 |

## 七、待审计要点（给 glm-5.3）

1. 战略方向：docx 真源 + PDF 锚点是否为“WPS/Word/PDF 好修改”需求的正解？
2. 架构风险：WPS COM 仍在打印关键路径（挂起血泪史）——可接受吗？LibreOffice/OnlyOffice 是否该替换？
3. P2 可行性：PDF 锚点映射的真实难度评估，是否有更廉价的替代（如 docx 段落 ID + PDF 书签双向表）？
4. 资产收编声明是否属实（各资产的角色是否被高估）？
5. 遗漏：Typst/OnlyOffice/Stirling 的引入点是否需要重新评估？
