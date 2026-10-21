# Agent 能力提升方案（v4，2026-10-21 三席审计返工后）

> 来源：`docs/absorption/deepseek-deepinfer-absorption.md`（两仓库实读）+ 代码盘点
> 审计：**GLM-5.3（codely-core）** 判 ACCEPT WITH NITS（8 项）；**Qwen3.8-Flash** 判 REJECT（"结论对、证据全错"）。两席独立作业、结论高度收敛，本 v2 已逐条吸收。
> 定位：只写"让 agent 少犯错、少绕弯、更会自查"的东西；推理服务端能力不在本方案。

---

## 0. 先纠正两处（原草案的错误，两席一致指出）

| 原草案说法 | 实际 |
|---|---|
| "截断在 `retry.rs:424` 判 fatal" | `:424` 只是**人读文案**。产生点 `cf-sampler/src/actor/request_task.rs:654-658`（`stop_reason == Length` → `AttemptOutcome::Failed`）；不 failover 的判定在 `retry.rs:294`（`is_failover_error`，`:305` 把该类归入不 failover 分支）；对上层成终态在 `cf-shell/src/sampling/error.rs:108`。**术语也要改**：failover ≠ retry |
| "logprobs 字段全为 None ⇒ 没采集" | 那些 `None` 全在**响应结构体**与测试 mock 里。**请求侧根本没有这个字段**——`SamplingConfig`（`cf-sampling-types/src/types.rs:1069-1090`）里没有 `logprobs`。所以这是**建设**（3 天），不是接线（1 天） |
| "json_schema 主循环未用" | 不准确：**最终结构化输出路径在用**（`turn.rs:1773-1891`，含 schema violation 重试；e2e 在 `test_built_binary_e2e.rs:842-843`，**该 e2e 带 `#[ignore]`，需 `--ignored` 才跑**）。真正没用的只是**工具参数**路径 |

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
| 截断后续写 | **无** | `request_task.rs:654-658` → `error.rs:108` 终态 |
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

- **改动位置（三席修正）**：预算不在 retry 层。灌入点共 **4 处**：
  - chat-completions `client.rs:814-816`
  - Responses `client.rs:1137-1140`
  - 另一条 `client.rs:1846-1848`
  - **Anthropic Messages `client.rs:1510-1521`**（`apply_message_defaults`：`max_tokens==0` 时回填常量 `ANTHROPIC_DEFAULT_MAX_TOKENS`，因为 Messages 线该字段是必填 `u32`）
  源头是 `SamplerConfig.max_completion_tokens`（`config.rs:49/:53`，client 侧装配为 `ClientDefaults`，`client.rs:535`）。**另有运行时覆盖源**：`client.rs:233-256` 从响应头读 `x-grok-max-completion-tokens` / context-window 覆盖默认值——封顶公式要声明取哪一份。
- **起点与封顶**：以本次实际下发的值为准；`None` 时取该模型目录默认（**Messages 线是常量回填，不是目录默认**）；上限 = `context_window − 已占 prompt token − 安全余量`（**不是直接用 `context_window`**）；超上限被 provider 返回 400 时按失败处理并回落 Step 1。`context_window` 现服务于 auto-compact 阈值（`types.rs:1082`），二者不冲突：cap 保证单请求不超窗，只会让下一回合更早触发压缩。
- **止损（新增）**：`MaxTokensTruncation` 是**双语义**——`stream/messages.rs:385-390` 把"生成中撞到上下文窗口上限"也归入此类。**该类翻倍必无效**：决策前先判 `prompt_tokens + 首轮 completion ≥ context_window`，成立则**跳过翻倍直接落 Step 1**。翻倍后仍截断 → 不再翻第二次（一次为止），并在 `turn_completed` 记 `truncation_retry` 与作废的首轮输出 token（成本口径见 §6）。
- **必须同步的既有设计决策**（否则会被现有测试挡住）：`retry.rs:26-31` 的"by design 不重试"文档、`:489` 的透传分支、`:636` 的 `assert!(!is_failover_error(...))` 回归、`stream/messages.rs:459` 的 Anthropic 抛出点——这四处要一并改成"预算提升≠failover，允许一次"。**`cf-shell/src/sampling/error.rs` 不动**（Step 0 在 cf-sampler 内消化，二次失败按原样成终态；`error_kind` 标记必须原样保留给 Step 1）。

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
   **可执行规则（三席定稿）**：`StreamingTurnCapture` **只收文本/推理，从不含 tool 参数**（已实测），因此**"把 capture 文本灌回消息"结构上安全**；"不续写"的准确含义是**不得把原始 `response`/`tool_calls` 灌回消息**。这样既不需要检测截断点位置，也不会误伤可续写的文本截断。
   **代价（必须知道）**：工具参数截断走整请求重发 = 首轮输出 token 全废，同内容从头重生成（输出成本 ≈2×）；长文件写入被截断时是整个文件重写。该成本记入 §6。
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

> **前提更正（2026-10-21 用户指出）**：`always-approve` 是**用户自设**的模式，权限分类器在该模式下不运行**是设计如此**（`manager.rs:79`："Auto mode 与 yolo 运行时互斥"），**不是缺陷，也不该被"修复"**。M2a 因此**不得触碰权限判定**，也不得在 always-approve 下增加任何拦截或额外模型轮次。

**重新定位**：M2a 是**提示层纪律 + 可观测**，不是门禁。

1. **提示层**：在系统提示 / 高危工具描述里明确"破坏性操作前先说明影响面与回滚点"，让模型自己养成习惯——零额外调用、零拦截。
2. **可观测**：把本回合的破坏性动作（覆盖写、删除、`git reset --hard`、对外发送）汇总进回合摘要，事后可查、可复盘。
3. **不做**：不阻断、不降级、不覆盖用户的 `always-approve` 选择。

审计两席指出的"最高性价比"仍然成立——它只是**不通过改动权限系统**来实现。

### M2b logprob 门控反思（**实验项，3–5 天，带退出判据**）

- 判据：`mean_logprob < τ1` 或 `p10 < τ2` 或 `low_conf_ratio > θ` → 追加一次"逐条核对结论与证据"；
- **硬门槛（三席修正）**：40 条样本、阳性约 15 条时，AUC 的 95% CI 约 ±0.13（**该值为 Hanley–McNeil 闭式推导，非实测**）。改为：**bootstrap 置信区间上界 < 0.65 才砍**；并用 Hanley–McNeil 闭式 SE 交叉核对（percentile bootstrap 对 AUC 这类 U-统计量方差偏低）；**判据冻结前必须用 M0 基线对成功/失败例双向实测**。首选仍是"用生产 `turn_completed` 攒大样本标定，评测集只作复核"。
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

### 5.1 若只能先做一件（三席要求给出单一推荐）

**单人力 → 先做 M2a（提示层自查纪律 + 回合摘要，2 天）**：收益确定、零依赖、**不碰权限系统**（`always-approve` 是用户自设，不得覆盖）。

**多人力 → M0 与 Step 0/M2a 并行**，M0 作为方法论地基仍最先起跑。

原 §4 "评测集最先做"是**方法论默认**（没有度量就没有改进），但它是 3 天的间接收益；若只能投一个人，M2a 的直接收益更硬。两者不冲突，只是"先"的判据不同，这里写死避免执行时再纠结。

---

## 6. 风险

| 风险 | 处置 |
|---|---|
| 续写重复/跑偏 | 去重阈值 + 轮数上限 + 原文不覆盖（可回看） |
| 去重破坏前缀缓存 | 只在确实重复时裁剪；成本口径含缓存差异 |
| tool-call 参数截断 | 该情形**不续写**，走提预算重发 |
| logprob 与正确性弱相关 | 门槛：CI 上界 < 0.65 即砍 M2b（与 §3 同口径） |
| **截断重试的代价** | 翻倍重试会让首轮输出作废（输出成本接近 2×）；`prompt + 首轮 ≥ context_window` 时直接跳过翻倍；工具参数截断只能整请求重发，长文件写入成本翻倍——均记入 `turn_completed.truncation_retry` 便于事后核算 |
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

### 第四轮（v3 → v4，GLM-5.3 复审）

判定 **ACCEPT WITH NITS**：8 项返工 6 项到位、2 项半到位（缺可执行规则/成本口径）。另提 5 项，已全部吸收：

| # | 发现 | 处置 |
|---|---|---|
| NP-1 | **§6 残留 v2 旧门槛"AUC<0.6"**，与 §3 的 CI 上界口径矛盾 | 已改 §6 为同口径 |
| NP-2 | **Step 1/坑 1 缺可执行规则**：`error_kind` 单值，区分不了"文本截断 vs 工具参数截断" | 已改为"capture 只含文本、灌回结构安全；'不续写'仅指不得灌回原始 response/tool_calls" |
| NP-3 | **Step 0 收益高估**：`MaxTokensTruncation` 双语义，context-window-hit 类翻倍必无效 | 已加止损：先判 `prompt+首轮 ≥ context_window` 则跳过；并记作废成本 |
| NP-4 | 优先级信号分裂（§3"最高性价比"/§4"最先做"/§5"最早见效"） | 已加 §5.1 单一推荐 |
| NP-5 | 文档残留：标题 v2、`655-659`、turn.rs 区间两值、"SamplerDefaults" 类型名不存在 | 已全清（标题 v4、654-658、1773-1891、`SamplerConfig`/`ClientDefaults`） |

同轮修正：Step 0 灌入点由"三条 wire"更正为**四条**（补 Anthropic `client.rs:1510-1521`）+ 运行时覆盖源（`client.rs:233-256`）。
