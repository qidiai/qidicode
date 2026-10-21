# Agent 能力提升方案（v2，2026-10-21 双席联合审计返工后）

> 来源：`docs/absorption/deepseek-deepinfer-absorption.md`（两仓库实读）+ 代码盘点
> 审计：**GLM-5.3（codely-core）** 判 ACCEPT WITH NITS（8 项）；**Qwen3.8-Flash** 判 REJECT（"结论对、证据全错"）。两席独立作业、结论高度收敛，本 v2 已逐条吸收。
> 定位：只写"让 agent 少犯错、少绕弯、更会自查"的东西；推理服务端能力不在本方案。

---

## 0. 先纠正两处（原草案的错误，两席一致指出）

| 原草案说法 | 实际 |
|---|---|
| "截断在 `retry.rs:424` 判 fatal" | `:424` 只是**人读文案**。产生点 `cf-sampler/src/actor/request_task.rs:654-658`（`stop_reason == Length` → `AttemptOutcome::Failed`）；不 failover 的判定在 `retry.rs:294`（`is_failover_error`，`:305` 把该类归入不 failover 分支）；对上层成终态在 `cf-shell/src/sampling/error.rs:108`。**术语也要改**：failover ≠ retry |
| "logprobs 字段全为 None ⇒ 没采集" | 那些 `None` 全在**响应结构体**与测试 mock 里。**请求侧根本没有这个字段**——`SamplingConfig`（`cf-sampling-types/src/types.rs:1069-1090`）里没有 `logprobs`。所以这是**建设**（3 天），不是接线（1 天） |
| "json_schema 主循环未用" | 不准确：**最终结构化输出路径在用**（`turn.rs:1773-1889`，含 schema violation 重试；e2e 在 `test_built_binary_e2e.rs:843`，**该 e2e 带 `#[ignore]`，需 `--ignored` 才跑**）。真正没用的只是**工具参数**路径 |

另外，"接续"我们确实已有两条（用户指出的没错）：

| 已有 | 位置 | 触发 |
|---|---|---|
| goal 续跑 nudge | `acp_session_impl/goal.rs:1937`（`prepare_goal_continuation`） | 目标未完成、回合结束 → 注入"继续" |
| 子代理 resume | `cf-agent/src/builder.rs:1222`（`resume_from` 参数声明；机制本体在会话恢复路径） | 失败/中断 → 继承 transcript |
| **缺：截断后续写** | —— | `MaxTokensTruncation` 直接终态 |

---

## 1. 现状盘点（逐条已核）

| 能力 | 现状 | 证据 |
|---|---|---|
| 截断后续写 | **无** | `request_task.rs:655-659` → `error.rs:108` 终态 |
| 半截文本 | **已有捕获**：`cf-shell/src/session/streaming_capture.rs`（`StreamingTurnCapture`，有字节上限） | 当前只导 trace，未回流给模型 |
| logprobs | **请求侧无字段**；响应类型有但恒 `None` | `SamplingConfig` 无该字段 |
| 结构化输出（最终） | **已有且带重试** | `turn.rs:1773-1891`；`test_built_binary_e2e.rs:843` |
| 工具参数校验 | **已有**：`validate_params_json`，错误含 category/field_path/expected/bad_value | `cf-tools/src/types/params_validation.rs:44` |
| 前缀缓存意识 | 有注释、无守卫 | `conversation.rs:52/85/1922/2176` |
| 评测集 | **无** | `git ls-files evals` 空 |

---

## 2. L1 —— 少犯错

### L1-a 截断处理（三步，按成本从低到高）

**Step 0（新增，最便宜，1–1.5 人日）**：`stop_reason == Length` 时把**本次实际下发的** `max_tokens` 翻倍重发一次，再失败才终态。

- **改动位置（复审修正）**：预算不在 retry 层——它由三条 wire 的请求构造点灌入（`cf-sampler/src/client.rs:815 / :1139 / :1847`，源头 `SamplerDefaults`，`config.rs:53`）。因此 Step 0 = 这三处 + 新增"预算提升重试"类别 + actor 里每请求预算可变。
- **起点与封顶**：以本次实际下发的值为准；`None` 时取该模型目录默认；上限 = `context_window − 已占 prompt token − 安全余量`（**不是直接用 `context_window`**）；超上限被 provider 返回 400 时按失败处理并回落 Step 1。
- **必须同步的既有设计决策**（否则会被现有测试挡住）：`retry.rs:26-31` 的"by design 不重试"文档、`:489` 的透传分支、`:636` 的 `assert!(!is_failover_error(...))` 回归、`stream/messages.rs:459` 的 Anthropic 抛出点——这四处要一并改成"预算提升≠failover，允许一次"。

**Step 1（主路径，1.5 天）**：通用兜底续写。

- **实现层级（复审修正）**：在 **cf-shell 会话层**，由 `error.rs:133-158` 已有的 `error_kind` / `stop_reason_for_turn_error` 标记驱动；**不要在 cf-sampler 的 actor 里做**——截断判定在 actor 内（`request_task.rs:654-658`），那里拿不到 session 状态，跨 crate 取文本也不成立。
- **文本来源**：`cf-shell/src/session/streaming_capture.rs`（`StreamingTurnCapture`，挂在 session 上）。注意其上限 `STREAMING_CAPTURE_MAX_BYTES = 8MB`，超限会**静默裁剪并置 `truncated`**——`truncated` 置位时**不做续写**，只走 Step 0。
- 追加为 `assistant` 消息 + 一条续写指令（"从断点继续，不重复已写内容，不重新开头"）。
- **去重**：拼接前按最长公共后缀/前缀裁剪（≥20 字符才裁）；
- **上限**：最多 3 轮、累计 ≤ context 25%；
- **可观测**：`turn_completed` 记 `continuations` / `continuation_reason`。

**Step 2（可选，需先建能力）**：原生 prefix completion。
- **前提不成立**：全仓 `git grep supports_prefix_completion` = 0，模型目录里只有 `supports_reasoning_effort`。要用必须先建 capability 元数据 **并逐中转实测透传**（luyou / senseny / codely 均走 OpenAI 兼容中转，官方 beta 参数未必透传）。
- 结论：**不作为主路径**；等 Step 0/1 落地并有数据后再评估。

**两个必须处理的坑**（审计指出）：

1. **工具参数中途截断**：半截 JSON 作为 assistant 消息追加 → 非法 `tool_calls` → 下一次请求大概率 400。
   **默认策略（复审定稿）：一律不续写，且无需检测**——`streaming_capture` 不捕获 tool 参数，截断路径又丢弃 `response`，做"检测"要先新建通道，成本不值。所以规则就是"tool 参数截断 → 走 Step 0 / 整请求重发"，**判定门槛降到最低**。
2. **前缀缓存口径**（原草案写反了）：**纯追加 assistant 消息是追加、前缀不变 → 缓存友好**；真正会 miss 的是**去重裁剪改写了已生成前缀**。所以去重只在"重复确实发生"时启用，且成本口径要把"放弃一次缓存命中"算进去。

**验收**：构造"生成 6000 字章节"的回归任务，比较 Step 0 / Step 1 / 现状的成稿完整率、总 token（含缓存命中差异）、轮数。

### L1-b logprobs 采集（**建设，3 天**）

- 请求侧：`SamplingConfig` 新增 `logprobs: Option<u8>`；
- 三种 wire 各写映射：chat-completions / responses / messages；
- 三处流式解析各自累积；
- 回合末聚合：`mean_logprob` / `p10_logprob` / `low_conf_token_ratio`，写进 `turn_completed.usage`；
- **已知缺口**：Anthropic Messages 线拿不到 token logprob → 结果是"部分模型有、部分没有"，标定样本减半。方案必须按"仅同模型内相对比较"使用。

### L1-c 工具参数结构化 —— **不做**（方案自判正确，补一条证据）

校验+重试已在 `StructuredOutput` 路径实证（`test_built_binary_e2e.rs:843`）；provider 原生约束解码不可控。**关闭本项**。

---

## 3. L2 —— 更会自查（拆两半，别再打包）

### M2a 规则型高风险自检（**立即可做，不依赖任何采集，2 天**）

触发（纯规则，不看 logprob）：即将 **写文件超过 N 行 / 删除 / 执行含 rm·reset·force / 对外发送** → 执行前强制一次"影响面 + 回滚点"自检。

这是审计两席共同指出的**最高性价比项**：原草案把它绑在"logprob 门控"后面，等于用未验证的假设推迟了一个已经确定有效的功能。

### M2b logprob 门控反思（**实验项，3–5 天，带退出判据**）

- 判据：`mean_logprob < τ1` 或 `p10 < τ2` 或 `low_conf_ratio > θ` → 追加一次"逐条核对结论与证据"；
- **硬门槛（复审修正）**：40 条样本、阳性约 15 条时，AUC 的 95% CI 约 ±0.13，0.6 这条线区分不了"无用"与"中等有用"。改为：**bootstrap 置信区间上界 < 0.65 才砍**；或用生产 `turn_completed` 攒大样本标定，评测集只作复核。
- 理由：LLM 的置信校准本身很差（错答常常同样自信），不设退出判据的话 3–5 天大概率沉没。

---

## 4. L3 —— 评测集（最先做）

**落位**：`tools/evals/`（**不能用 `evals/`**——AGENTS.md §1 根目录白名单没有它，第一天就会被纪律卡住）。

- 30–50 条起步，**只收可自动判定的**：编译通过 / 测试通过 / 文件内容匹配 / 输出结构校验；
- **不可判定的任务丢弃并记录占比**；若丢弃率 >50%，说明"能区分改动好坏"的宣称不成立，需要引入 LLM-judge 或 10% 人工抽评；
- 指标：通过率、平均 token、平均轮数、平均时延（**连续量比 30 条的成功率更能区分改动**——两席都指出 30–50 条的统计功效不足以判定成功率差异）；
- 基线必须记录在案，任何 L1/L2 改动前后各跑一次。

---

## 5. 里程碑（三条线并行，M2b 收口）

```
M0  评测集 + 基线（3 天）──────────────┐
M1a-Step0  预算翻倍重试（1–1.5 天）────┤   ← 最早见效，改 cf-sampler
M2a  规则型高风险自检（2 天）──────────┤   ← 改 cf-shell
M1a-Step1  通用兜底续写（1.5 天）──────┤   ← 改 cf-shell（与 M2a 同区，**串行或同一人**）
M1b  logprobs 采集（3 天）─────────────┴→ M2b 门控实验（CI 上界门槛）
```

依赖与并行（复审修正）：**M1-b 不依赖 M1-a**（原串行链画错）；**Step 0 与 M2a 可并行**（不同 crate）；但 **Step 1 与 M2a 撞车**——两者都改 cf-shell 的会话/回合装配（一个注入续写 assistant 消息，一个在执行前插自检钩子），按 AGENTS.md §9 应串行或交给同一人。

---

## 6. 风险

| 风险 | 处置 |
|---|---|
| 续写重复/跑偏 | 去重阈值 + 轮数上限 + 原文不覆盖（可回看） |
| 去重破坏前缀缓存 | 只在确实重复时裁剪；成本口径含缓存差异 |
| tool-call 参数截断 | 该情形**不续写**，走提预算重发 |
| logprob 与正确性弱相关 | AUC < 0.6 砍掉 M2b（硬门槛） |
| 评测集选择偏差 | 记录丢弃率；>50% 时改用 judge/人工抽评 |
| Anthropic 线无 logprob | 只做同模型内相对比较，不跨模型 |

---

## 7. 工时（人日 / 含编译等待的墙钟）

| 项 | 人日 | 墙钟 |
|---|---|---|
| M0 评测集 | 3 | 3–4 天（"改动前后各跑一次"的全量跑另计） |
| M1a-Step0 | **1–1.5**（原估 0.5 漏了三条 wire + 既有设计决策同步） | 1.5–2 天 |
| M1a-Step1 | 1.5 | 2 天 |
| M1b | 3 | 3–4 天（cf-sampling-types 系改动触发下游重编） |
| M2a | 2 | 2 天 |
| M2b | 3–5 | 4–6 天（含调参 A/B） |

---

## 8. 双席审计记录（2026-10-21）

| 席位 | 判定 | 主要意见 | 处置 |
|---|---|---|---|
| **GLM-5.3**（codely-core） | ACCEPT WITH NITS | ①prefix 能力标记不存在，首选路线应降级；②M2 拆分 + AUC 退出判据；③缓存风险漏了（且方向易搞反）；④评测集"全自动判定"会诱发选择偏差；⑤可复用 `streaming_capture` | 全部采纳（§2 L1-a Step 2、§3 M2a/M2b、§6、§4、§2 Step 1） |
| **Qwen3.8-Flash** | REJECT（"结论对、证据全错"） | ①`retry.rs:424` 是文案不是判定点；②logprobs 三条证据全在响应侧/测试，请求侧根本没字段；③`supports_prefix_completion` 不存在；④**tool-call 参数截断会 400**（原方案未处理）；⑤**遗漏 `max_tokens` 翻倍重试**；⑥`evals/` 违反根目录白名单；⑦30–50 条统计功效不足 | 全部采纳（§0 纠错表、§2 Step 0/坑 1、§4、§6） |

两席**独立收敛**的三点：prefix 不可作主路径、M2 必须拆、评测集必须最先做。

### 第三轮（v2 → v3，Qwen3.8-Flash 复审）

判定 **ACCEPT WITH NITS**：上轮 11 项全部实质改到位；另提 8 项，其中 3 项重要，已全部吸收：

| # | 发现 | 处置 |
|---|---|---|
| N-1 | Step 0 改动位置漏了预算真正所在（`client.rs:815/1139/1847`，actor 内无 `max_tokens`）→ 工时 0.5 → 1–1.5 人日 | 已改 §2 Step 0 位置与工时 |
| N-2 | Step 0 反转了一条成文设计（`retry.rs:26-31` by design、`:489`、`:636` 断言、`messages.rs:459`） | 已列入"必须同步的既有设计决策" |
| N-3 | "翻倍"起点未定义、封顶口径过粗 | 已改：以实际下发值为准；上限 = `context_window − prompt − 余量`；400 处置 |
| N-4 | Step 1 层级不成立（捕获在 cf-shell session，判定在 cf-sampler） | 已改：实现在 cf-shell 会话层，由 `error_kind` 驱动；`truncated` 置位则不续写 |
| N-6 | 坑 1 无法执行（无 tool 参数捕获、截断路径丢 response） | 已改为默认策略：**一律不续写、无需检测** |
| N-5 | 40 样本上 AUC 门槛不可靠 | 已改：bootstrap CI 上界 < 0.65 |
| N-7 | Step1 与 M2a 都改 cf-shell 会话装配，不能并行 | 已改里程碑：两者串行或同一人 |
| N-8 | 行号偏移（`654-658`、`turn.rs` 区间、e2e 带 `#[ignore]`） | 已修正 |
