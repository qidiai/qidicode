# K3 审计任务书（存档，明早额度窗口执行）

> 2026-10-17 收官时存档。K3 通道（luyou KIMI-K3-222 spawn）当日 429/401 全封锁（pro 组配额耗尽）；历史规律：**额度窗口早晨恢复**（昨晨可用）。明早跑法：手动探针（deepseek 探针脚本同款，model=kimi-k3）→ 200 则 spawn KIMI-K3-222 执行下述任务书。

## 审计对象

G:\qidicode 仓库 HEAD（268a70e9+）的 Phase C 部分（commit 3c1eccff 的 cf-chat-state/cf-shell reasoning 链+批量清链）——**R1-R8 全部返工后的最终态**。

## 背景与你的独特价值

前五轮审计（GLM-5.3 6 阻塞/kimi-k2.7 4 阻塞含测试序暗雷/Qoder 3 盲面阻塞/step-5 4 阻塞+落点复核 10 处补笔/step-5 继承位 v2 评审）已全部闭环返工（R1-R8）。你的价值：**第六方盲面终审**——验证 R7/R8（Qoder/step-5 批次）的落地质量 + 挖前五审的集体盲区。

## 审计重点

1. **R7 批**（Qoder 三阻塞落地）：①config 接线透传链（[compaction.pruning] batched_pruning/low_watermark_percent → actor_pruning_config → batched_mode()，缺省关=C-6 零介入）②地板三防线（top-8 截断/480B 帽/80 字最小块门/60% 净收益熔断——边界值恰好的极端形态）③cleared==0 不置位（B-3 的 C-7 塌缩修复）④RAII guard（AtomicBool panic 泄漏）⑤age 单源（GetHardClearAgeTurns）⑥滞回回收线+钳制（Some(60) 静默失效修复）⑦三判据测试（删判据必红）
2. **R8 批**（step-5 四收口落地）：①merge 回归测试（双指纹断言——replace 回归必红的构造真实性）②文案全模式为真（updates.jsonl 无条件落盘）③文档债三处④跨 run 5s 节流
3. **盲面维度**（前五审的集体盲区候选）：内存结构计数的线程安全/estimate_tokens_aware 复用偏差/golden 断言的脆弱性/compaction 与 batched 的折叠交互/子代理路径的 session-kind 标签遗漏

## 纪律

- file:line + 代码引用；禁编译（禁 cargo——CI 已 6/6 背书）；禁 commit
- 流程：先自由审查后对照（C-1b 三硬底/C-2/C-3/C-4/C-6/C-7）

## 输出

结论先行（可靠终态/需新返工+数）→ R7/R8 逐项落实判定 → 盲面新阻塞（若有）→ 一段话总评
