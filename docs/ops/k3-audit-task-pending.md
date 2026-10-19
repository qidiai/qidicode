# K3 审计任务书（2026-10-18 台式机执行版）

> **⚠️ 2026-10-19 更新：KIMI-K3-222 已退役（用户决定，当日 429 后未恢复）。本任务书原定执行席改为 `kimi-k2.7-code-highspeed-222`**（apigoto 独立 key 线，spawn/headless 双通道 2026-10-19 实测可用）；其余审计规格不变。若该 Phase C 终审已由其他批次覆盖，以最新会话纪要为准。

> 笔记本侧通道全封锁（luyou spawn 429/codely CLI 未装）——**此任务书供台式机侧执行 K3 审计**（codely CLI 通道或手动均可）。执行后把审计报告放回本目录即可。

## 审计对象

G:\qidicode 仓库（或台式机副本）HEAD 的 Phase C 部分——**R1-R8 全部返工后的最终态**（R9/R10 的修复也在——含 payload_accounting 1363 行+R10.5 收尾）。

审计范围限定 cf-chat-state 与 cf-shell 两个 crate 的 reasoning 摘要链+批量清链（约 23 文件，核心三个新文件：reasoning_summary.rs/reasoning_summarizer.rs/reasoning_pregenerate.rs——在 cf-chat-state/src/actor/ 与 cf-shell/src/session/ 下）。

## 背景与你的独特价值

前五轮审计（GLM-5.3 6 阻塞/kimi-k2.7 4 阻塞含测试序暗雷/Qoder 3 盲面阻塞/step-5 4 阻塞+落点复核 10 处补笔/deepseek-flash 0 阻塞终审）已全部闭环返工（R1-R8）。你的价值：**第六方盲面终审**——验证 R7/R8（Qoder/step-5 批次）的落地质量+挖前五审的集体盲区。

## 审计依据

设计稿 docs/token-optimization/P0-2第二步-动态工具管线设计稿.md 的 Phase C 节（v3.6：C-1 分区独立/C-1b never-bare-clear 三硬底/C-2 批量语义/C-3 双禁区/C-4 软指引/C-6 flag 回退/C-7 滞回）。

## 审计重点

1. **R7 批**（Qoder 三阻塞落地）：config 接线透传链（[compaction.pruning] → actor_pruning_config → batched_mode()，缺省关=C-6 零介入）/地板三防线（top-8 截断/480B 帽/80 字最小块门/60% 净收益熔断——边界值极端形态）/cleared==0 不置位（C-7 塌缩修复）/RAII guard/age 单源（GetHardClearAgeTurns）/滞回回收线+钳制/三判据测试
2. **R8 批**（step-5 四收口落地）：merge 回归测试（双指纹断言）/文案全模式为真/文档债三处/跨 run 5s 节流
3. **盲面维度**（前五审的集体盲区候选）：内存结构计数的线程安全/estimate_tokens_aware 复用偏差/golden 断言脆弱性/compaction 与 batched 折叠交互/子代理路径 session-kind 标签遗漏

## 纪律

- file:line + 代码引用；禁编译（CI 已 6/6 背书）；禁 commit
- 流程：先自由审查后对照（C-1b 三硬底/C-2/C-3/C-4/C-6/C-7）

## 输出

结论先行（可靠终态/需新返工+数）→ R7/R8 逐项落实判定 → 盲面新阻塞（若有）→ 一段话总评。报告放回本目录（qidiwork-docs 会话纪要同级或本文档追加）。
