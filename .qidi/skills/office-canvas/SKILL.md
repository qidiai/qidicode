---
name: office-canvas
description: "画布式反馈闭环：用户在 Word/图片上直接改、直接加批注（WPS/Word 原生批注即可），本技能把批注/修改读出来转成结构化整改清单，驱动精修子代理逐条落地，改完验证批注已响应。适用于：方案/图纸生成后人工审核、标注错误位置、告诉 AI 哪里错了怎么改、类似 bid-perfect 精修循环的人工反馈入口。触发词：画布、标注反馈、批注修改、审核意见、改图、人工检查、精修反馈。"
---

# Office Canvas —— 画布式反馈闭环

把 bid-perfect 的"审计→精修→复审"循环补上**人工反馈入口**：
用户在 WPS/Word 里打开产物，直接改、直接写批注（"这里错了，支撑应为四角撑"），
保存后一句话触发，agent 读取全部批注与改动 → 结构化整改清单 → 精修落地 → 验证响应。

对标：豆包侧边工作台"点编辑改局部"、WorkBuddy editor_sdk 所见即所得。
实现：Word 原生批注（OOXML comments）+ PIL 图片标注 + WPS COM 实时打开。

## 三种反馈形态

| 形态 | 用户操作 | agent 动作 |
|---|---|---|
| **批注反馈**（主推） | 在 docx 里圈选文字加批注 | `collect` 读批注 → 整改清单 |
| **图片标注** | 看图说位置（"横道图第3周缺条"）或圈图 | PIL 在图上画框+序号生成标注图，对照改图脚本 |
| **直接改文件** | 用户自己改了 docx | `diff` 对比 agent 端留存的基线版 → 提炼用户改动意图 |

## 使用流程

```bash
S=~/.qidi/skills/office-canvas/scripts

# 1. 交付时: 打开产物并留存基线(供后续 diff 用户改动)
python $S/canvas.py open 方案.docx          # WPS 打开,所见即所得
python $S/canvas.py baseline 方案.docx      # 留存基线快照

# 2. 用户在 WPS 里加批注/修改 → 保存关闭(或直接保存)

# 3. 收集反馈:
python $S/canvas.py collect 方案.docx       # 读全部批注→整改清单(含定位)
python $S/canvas.py diff 方案.docx          # 对比基线→用户改动摘要
python $S/canvas.py mark 图.png --box 50,50,300,150 --label "1:缺少第3周条"  # 图片标注

# 4. 整改清单转精修任务(交 bid-perfect 精修子代理逐条落地,禁批量替换)
# 5. 改完验证:
python $S/canvas.py verify 方案.docx        # 批注是否全部响应(内容已改/已答复)
```

## 整改清单格式（collect 输出，喂给精修子代理）

```markdown
## 人工批注整改清单 (共 N 条)
| # | 批注人 | 定位 | 批注内容 | 引用原文 |
|---|---|---|---|---|
| 1 | ASUS | P012¶3 | 支撑形式应为四角撑 | "三角支撑稳定" |
...
每条必须按 bid-perfect Step2b 纪律执行：读原文→逐段改→回读确认→禁正则批量替换
```

## 与精修流程的衔接（bid-perfect 协议）

1. `collect` 输出的清单 = 精修子代理的任务项来源（与审计差距清单同格式合流）
2. 每条批注按"原文位置/原文内容/修改方向/预期效果"四要素拆解
3. 落地后 `verify` 检查批注锚定文本是否已修改；未改的退回重做
4. 全部响应后在 `progress.md` 记录：N 条批注 → 改了什么 → 原文→新文对照

## 依赖

- python-docx ≥1.1（comments part 读写，1.2.0 已验证）
- pywin32 + WPS（KWPS.Application，已验证）或 Word（Word.Application）
- PIL（图片标注）

## 边界

- 批注读取基于 OOXML zip 直读，WPS/Word 双端兼容
- 用户批注未保存/文件被占用时 `collect` 会提示，等保存后重试
- 图片"画布"当前是标注图+改图脚本两步走；WPS COM 内实时圈选标注属 P3 增强
