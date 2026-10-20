# 联合审计报告 · cad2pdf 方案 · glm-5.3 + step-5-preview · 2026-10-20

> 两路独立审计（glm-5.3 / step-5-preview 官方直连），**独立收敛到同一结论：63/100 · 方案有阻断缺陷**
> 两路均确认：指挥官的 47MB/50s/8paths/text=0/护栏 等数字**全部真实复现，无造假**
> 失实不在数字，在 SKILL 对 SHX/--window 行为的技术表述

## 你最初的判断是对的：根因实锤

**"转PDF的图片识别不完整" = SHX 字体文字被 cadcraft 直接丢弃，不是回落。**
真实样本（13#楼33F给水，天正 t3）3,194 个 TEXT 中 **2,725 个（85.3%）使用 SHX 类样式**（SIMPLEX/GBCBIG/HZTXT/COMPLEX…）→ 输出 PDF 丢失轴号/尺寸标注/管径编号等核心文字。
两路受控实验一致：SHX 样式 → PDF 1,197B/1 路径/零字形；TTF 样式（simhei）→ 11,919B/转曲保留。
**修复路径已验证可行**：L2 在 ODA→DXF 后加 ezdxf 样式改写（SHX font/bigfont → simhei.ttf，清空 bigfont）再喂 cadcraft，<20 行，文字恢复。

## 两路共识缺陷清单

| # | 级 | 结论 | 证据 |
|---|---|---|---|
| 1 | **阻断（样本触发）** | SHX 文字 85.3% 丢弃；SKILL "回落/内容不丢" 证伪 | 3,194/2,725 统计 + 受控实验 |
| 2 | **阻断级风险** | xref 全链静默丢失、零检测（ODA 不绑定 + cadcraft 不解析 + 检查缺失）；`dwg_to_dxf` 只复制单文件 | 四重实证；样本 33 张 DWG 无 xref 痕迹（天正自包含），风险场景未触发 |
| 3 | 高 | 单页 fit 版式不可打印交付（全图塞一页 letter/A4，1:1575，无图幅/分页/打印比例） | 实测页面 792×612 / 841.9×595.3 浮动 |
| 4 | 高 | `--window` 是视口语义非裁剪：PDF 实测 **+20~23% 反增**，"大幅缩小"失实 | 47.2→55.4/59.1MB；paths 30859=30859 |
| 5 | 中 | DWG 输入时布局检测死代码（rmtree 早于 check → DXF 已删） | check JSON 无 paperspace 字段 |
| 6 | 中 | 坏文件 traceback 违反 JSON 契约；`--check-only` 退出码恒 0；probe 不查 ODA；300s 超时对 50MB+ 必超；DXF `--level 4` 静默变 L2 | 实测 |
| 7 | 低 | 临时目录错误路径泄漏；与 bid-drawing 触发词重叠；"通常布局空间"以偏概全 | 实测/文本 |

## 数字复核（两路一致）

47,234,133B（47.2MB 十进制）✓ · ODA 3.0–4.8s ✓ · cadcraft 35–38s ✓ · 总链 42–53s ✓ · 8 paths/0 images/text=0 ✓ · 性能护栏必要（get_drawings 实测 120s）✓

## 修复计划（两路建议合并）

**P0（立即）**：
1. SHX→TTF 样式改写（ezdxf，<20 行，已验证恢复文字）
2. 文档按实测改写：SHX 丢弃（非回落）、--window 视口语义、布局空间"通常"措辞、L4 示例（.pdf 输入不支持）

**P1**：
3. xref 检测（check_pdf 扫 xref 块 → 警告"建议绑定/eTransmit 后转换"）
4. 死代码/契约修复：rmtree 移 check 后、copyfile try/except、泄漏、probe 退出码、check-only 退出码、--timeout 参数化

**P2**：
5. 版式分页（利用 --window 内容流仍在的特性 + pymupdf set_cropbox 后处理，需验证）
6. 与 bid-drawing 互加排他触发指针

## 裁决

AUDIT RESULT: 方案有阻断缺陷（修复路径全部实测可行，非推倒重来）
