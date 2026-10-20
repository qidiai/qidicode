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

## 六、建造顺序（glm-5.3 审计 82/100 后修正版 v2）

> 原版评级修正：P0“低风险”→“中低”；P2“研究级”→“工程级（文本锚定）”；新增 P0.5 与 P3

```
P0'  整改闭环产线化（原文 P0 同目标，三处修正）:
     ├─ verify 改存档清单驱动（collect JSON 为真值源）——消灭 H1 假阳性
     │    （原缺陷：python-docx 保存丢批注部件 → verify "视为已响应"直接✅，核心验证空转）
     ├─ 预埋段落稳定 ID/书签锚点（P2' 同套定位机制，防返工）
     └─ anchor 判定稳定化（替代 80 字截断子串匹配）
∥
P0.5 打印通道看门狗化（审计新增，与 P0' 并行）:
     隔离子进程 + 进度心跳 + taskkill /T /F 杀树（移植 craft_bridge.py:70-121 现成模式）
     + 降级链：WPS COM 主 → OnlyOffice Document Builder 兜底（标"预览版"）→ LibreOffice 末档
→
P1   自建 docx MCP（原文 P1，补纪律）:
     基础 CRUD 抄 Office-Word-MCP-Server 底样不重写；自研只做四类业务命令：
     批注 OOXML 深操作 / 整改闭环 / 门禁命令化 / PDF 交付联动
     + 补 OnlyOffice Builder 生成引擎落点结论（哪怕结论是"不用"）
→
P2'  PDF 锚点映射·文本锚定级（降级自"研究级"）:
     PDF 批注(annotation) → fitz 提取被批文本 → docx 段落模糊匹配（复用 P0' 锚点 ID）
     → 改 → 重打印；廉价兜底永远在：版本树 + canvas diff + 重打印（已有，不弃）
→
P3   备选通道试点（审计新增）: WPS JSA 打印宏绕 COM / Aspose.Words 评估 / Stirling 明确"不进产线"
```

## 七、审计放行条件（C1-C5，全部满足后 P0' 开工）

| # | 条件 | 状态 |
|---|---|---|
| C1 | verify 存档驱动重设计进入 P0 验收标准 | ✅ 已入 P0' |
| C2 | 段落稳定 ID 进 P0 设计文档 | ✅ 已入 P0' |
| C3 | 打印看门狗+降级链方案进入 P0.5 规格 | ✅ 已入 P0.5 |
| C4 | 方案补“跨仓库治理”一节（单一仓库+调用边界，禁止 fork） | ✅ 见下节 |
| C5 | Aspose/WPS JSA/docxtpl 三项评估结论写入方案（哪怕“不做”）；Stirling 收紧为“不进产线” | ✅ 见下节 |

## 八、跨仓库治理（C4）

**现状**：产线资产分裂两地——标书软件仓库（`G:\程序开发\标书软件`：final_gate/pdf_bindery/redact_audit/craft_bridge + Flask 产线）与 qidicode 工作区（skills/harness/审计链）。
**风险**：fork 即分叉；标书软件产线仍在演进（W7a/b 就在那边做的）。
**规则**：
1. **禁止 fork**——qidicode 侧不复制上述四件套的代码；需要其能力时走 HTTP/CLI 调用边界（标书软件 waitress:5556 或 `python -m` 入口）
2. 新智能层（P0'/P1）建在 qidicode 侧，对标书软件只读消费
3. 若未来合并为单一仓库，走正式迁移评审，不做静默分叉

## 九、备选引擎评估结论（C5，三项全部记录）

| 选项 | 结论 | 理由 |
|---|---|---|
| Aspose.Words / Spire.Doc（商业库） | **记录备用，不引入** | 工业级 docx→PDF 无 COM 挂起，可用许可费消灭打印风险；但当前 WPS COM+看门狗路径成本更低，商业化授权评估留到 P0.5 实测看门狗效果后 |
| WPS JS 宏（JSA 加载宏） | **P3 试点** | WPS 进程内 JS API 可绕 COM RPC 桥做打印自动化，独特路径；风险是 API 面窄+绑定 WPS |
| docxtpl（模板填充） | **不引入** | 产线 docx 生成已由 python-docx 主力承担且经 1393 份语料验证，模板填充场景未成刚需 |
| Stirling-PDF | **不进产线**（收紧） | fitz 脚本群极薄（101/305 行）、进程内、零运维、已过 9 轮审计；Stirling 是 Java/Docker web 服务，进产线=新故障域。仅作人工兜底工具箱 |
| Typst | **不引入交付层**（维持，论据修正） | 真正理由是单一真源原则（引入 Typst=第二真源，与 docx 真源策略冲突），非“不出 docx” |
| OnlyOffice Document Builder | **降级链第二档 + 生成引擎不替换**（python-docx 继续主力） | OOXML 原生保真强于 LibreOffice，但分页引擎非 WPS，不可作打印主通道；docx 生成替换 python-docx 无增益（进程外 JS 引擎集成链长+AGPLv3 商用评估） |

## 十、原待审计要点（T1-T5）审计结论存档

glm-5.3 独立审计 82/100（2026-10-20，34 次工具调用）：战略方向正解（T1 通过，附“P2 是客户反馈主通道”的优先级修正）、三层正交无循环依赖、自建 vs 轮子边界清醒。问题清单：H1 verify 假阳性（已入 P0'）/ H2 打印无看门狗（已入 P0.5）/ H3 跨仓库治理（已入 §八）/ M1-M3 风险评级修正（已入 v2）/ L1-L3 文书卫生（SKILL 漂移/docstring 过时/433 命令数为 0.5.0 时代口径——现 parity live 487/total 502，P1 需求参考时用实测值）。
