# ChatML 壳重评判决实验报告（Opus 终审行动 1 · 训练-评测格式错位裁决）

> 日期: 2026-10-07
> 执行: ling-3.1-flash（C 线编码手）
> 实验: 白泽 2.2 best（LoopedTransformerLM T0 120m_v1，val_ppl 7.30）× zh_v2 十条
> 判决假设: Opus 终审仲裁 Q2-b「训练全程 ChatML 字面壳、评测裸文本 → 模型从未在"裸输入→短答"分布上收到梯度」
> 命令通道: GitCode fengzii/baize-c-line cmd #4600（base64 部署 + ASCEND_RT_VISIBLE_DEVICES=0 运行）
> 产物: 容器 `/opt/atomgit/npu_m4/baize22_sft_v1/chatml_reeval.json`（meta+results+summary）
> 脚本: `target/tmp/c-line/patches/chatml_reeval.py`（v2，覆盖初版；部署快照 `assets/chatml_reeval.py`）

## 一、背景与判决带（Opus 终审仲裁原文）

责任分配（终审仲裁 Q1）：参数规模 120M 15% / 欠训 3.3 tok/param 30% / **训练-评测格式错位 30%** / replay 反向拉力 20% / im_end 模板化 5%。

Q2-b 判决带（本实验要回答的）：
- 跳升 ≥50%（zh 1.3→2.0+，或代码出现 1-3/10）→ **格式错位实锤，此前所有读数全部低估**，c) 上调 40%+
- 跳升 10-50% → 维持 30%
- 跳升 <10% → 格式错位出局，责任回填欠训+replay

命中信号（行动 1）：zh 1.3→2.0+、HumanEval 20 条 ≥1 条正确、短答密度跳升 ≥20pp。
（本报告覆盖 zh 10 条口径；HumanEval 20 条包壳为行动 1 第二半，待另跑。）

## 二、实验设计（双口径对照）

| 项 | 壳臂（本次新跑） | 裸臂（既有样本） |
|---|---|---|
| prompt 渲染 | `<\|im_start\|>user\n{prompt}<\|im_end\|>\n<\|im_start\|>assistant\n`（训练同款，逐字符） | `<\|im_start\|>{prompt}<\|im_end\|><\|im_start\|>`（eval_m4.render_eval_prompt，无 role 词无换行） |
| 数据源 | `/opt/atomgit/baize-c-line/assets/eval_zh_prompts_v2.json` | `/opt/atomgit/npu_m4/baize22_sft_v1/eval_zh_samples.json`（同 10 条按 id 对齐） |
| 解码 | greedy argmax，max_new=512，窗 768（左截断） | 同（历史 greedy） |
| 退出 | eos_id=2 或解码后 `<\|im_end\|>` 字面出现（增量解码，命中即停） | eos / max_new（`truncated_by_max_new` 字段） |

逐条对照指标：长度比 / 停止状态 / 前 80 字 / difflib SequenceMatcher 相似度 / 字符级与 4-gram repetition_ratio / 复读题面 / 尾部退化 / 结构分（所以·总结·markdown）/ 期望点命中。

判据汇总（verdict 门）：短答(<120 字) 数、自然停止数、结构分数、退化数。
**verdict 规则：短答 ≥5/10 或 自然停止 ≥6/10 → PASS（格式错位实锤）**。

固定四判据（苛刻硬判，映射 rubric 1/3/5 锚点，逻辑经 26 例单测）：
- 03 算术：列式 23+48 + 结果 71 → PASS(5)；仅其一 → PARTIAL(3)；错（69/70/81/166）→ FAIL(1)
- 05 桥字：单句且含"桥"且 ≤25 字 → PASS(5)；单句含桥 25-30 字 → PARTIAL(3)；多句/无桥 → FAIL(1)
- 06 排序：完全等于「橘子、苹果、香蕉」→ PASS(5)；顺序对但夹带 → PARTIAL(3)；错序 → FAIL(1)
- 08 拒答：声明无法获取实时/未来天气且无编造数值 → PASS(5)；含糊或先编后补 → PARTIAL(3)；编造当事实 → FAIL(1)

## 三、基线（2.2 裸口径，既有读数）

| 指标 | 2.2 裸口径 |
|---|---|
| val_ppl | 7.30 |
| 中文分（人工预评） | ~1.3 |
| 自然停止 | 4/10 |
| 短答密度 | 14.2%（~1.4/10） |
| HumanEval | 0/164（三代同） |
| 固定四判据（裸） | 05「桥上挂着一座桥」×8 语义崩坏；06 一句收尾但顺序错；03/04 数字错；08 两次编造天气 |

## 四、双口径对照表（10 条）

【待回传——agent 复活执行 cmd #4600 后填充】

| id | 域 | 裸长→壳长 | 裸停→壳停 | len_ratio | 相似度 | rep4 | 判词 | 内容判读（前 80 字） |
|---|---|---|---|---|---|---|---|---|

## 五、固定四判据（ChatML 壳版本逐条判）

【待回传】

| 项 | 壳版本判定 | 裸版本对照 | 位移 |
|---|---|---|---|
| 03 算术 | | 数字错 | |
| 05 桥字 | | 桥×8 崩坏 | |
| 06 排序 | | 顺序错 | |
| 08 拒答 | | 编造天气 | |

## 六、verdict 与结论

【待回传——填：短答 x/10、自然停止 x/10、结构分 x/10、均长壳 vs 裸、代理均分 vs 1.3、短答密度 vs 14.2%】

**VERDICT: 【PASS/FAIL】**（规则：短答 ≥5 或 自然停止 ≥6）

**格式错位【实锤/不实锤】**（裁决带：跳升 ≥50% 实锤 / 10-50% 维持 / <10% 出局；跳升 = 【壳代理均分 vs 裸 1.3】与【短答密度 vs 14.2%】）

对终审责任分配的影响：【c) 30%→40%+ / 维持 30% / 出局回填 b)+d)】

## 七、附注

- 通道状态：容器在 #4560 后重启，agent 未复活（无 ttyd shell 触发 ~/.bashrc 自愈钩子）；cmd #4570/#4580/#4590 被槽位覆盖未执行；本实验以 cmd #4600 重新武装，agent 复活即自动执行（deploy→run chip0→summary）。
- 复现：`cd /opt/atomgit/baize-c-line && ASCEND_RT_VISIBLE_DEVICES=0 python3 -X utf8 assets/chatml_reeval.py`（~15-35 分钟）。
- 初版 `chatml_reeval.py` 的 placeholder bug（`gen_ids = gen_ids + [nxt] if False else None`）已在 v2 修净；v2 另增：增量解码 im_end 即停（初版跑满 512 才切）、退化检测、repetition_ratio、difflib 相似度、固定四判据硬判、原子写 JSON。
