# DeepSeek DeepInfer / deepseek-recipe 吸收分析（2026-10-20）

> **来源**：DeepSeek 公众号《DeepSeek 开源推理框架：走向真正普惠的未来》（2026-10-20）+ 两个官方仓库的本地实读。
> **本地快照**：`target/tmp/deepinfer-study/`（浅克隆，`cargo clean` 会销毁；复现命令见 §12）。
> **定位**：本仓库是**客户端/代理运行时**（Rust，`cf-*`），不是推理服务端。因此本文把结论分成两栏：**能直接吸收的代码/契约**，与**只能吸收的工程经验**。不做"看起来相关就照抄"的堆料。

---

## 0. 结论（TL;DR）

1. **两个仓库的价值极不对称**：`deepseek-recipe`（Rust，MIT，86 个 `.rs`）与我们的技术栈同构，可**直接吸收**；`DeepInfer`（Python+CUDA/Ascend，Apache-2.0，vLLM 衍生）是**服务端**框架，代码不可用，但它的 Q&A 文档是我们理解"上游成本/缓存/延迟从哪来"的一手资料。
2. **最值的吸收点是 1 个**：`deepseek-recipe` 提供**官方 tokenizer + special-token 感知编码**，恰好补上 `docs/token-optimization/方案.md` 里 **P0-1「真 BPE tokenizer 接入真实判定链」** 缺的"真值源"——而该方案当时自陈"仓内无 BPE 可对照，幅度为经验值"。
3. **第二价值点**：recipe 把 **`max_tokens` 的解析契约**（思考/非思考、effort 档、上下文裁剪、溢出报错）写成了代码，可与我们的输出预算逻辑逐条对齐，减少"上游静默截断"类事故。
4. **明确不吸收**：结构化输出（xgrammar）、流式解析状态机、多协议转换——我们已有对应实现（`StructuredOutput` 工具 + `stream/{chat_completions,messages,responses}.rs`），重复引入只增依赖。
5. **一个通用安全教训可立刻进错题本**：`DEEPINFER_XGRAMMAR_CACHE_LIMIT_BYTES` 注释里点名 CVE-2025-32381——**schema/grammar 编译缓存不设上限 = 无界增长 DoS**。我们任何缓存（prompt 缓存、hash 去重表、MCP 工具清单缓存）都该照此体检。

**吸收矩阵（先看这张表）**

| 对象 | 判定 | 理由 | 批次 |
|---|---|---|---|
| recipe 的 `tokenizer.json`（V4 / V4.1，各 6.6MB）+ `PromptTokenizer` | ✅ **吸收** | 唯一真值源，MIT 可分发，直击 P0-1 | A（P0） |
| recipe 的 `resolve_max_tokens` 契约 | ✅ **吸收（契约，不引依赖）** | 8192/65536/131072 三段规则 + 上下文裁剪 | B（P1） |
| recipe 的 `special_ranges` 思路 | ✅ 吸收（设计） | 用区间而非字符串匹配区分控制 token 与普通文本 | A 附带 |
| recipe 的 `with_model_alias` | ✅ 吸收（设计） | 与我们 `${slug}-222` 别名体系同类问题 | B 附带 |
| recipe 的协议转换 / 流式解析 | ⛔ 不吸收 | 方向相反（服务端 vs 客户端），已有实现 | — |
| DeepInfer 代码（调度/MoE/EPLB/KV） | ⛔ 不吸收 | 需 SM100 / CANN 9.2 / 多机集群 | — |
| DeepInfer Q&A 的运营数值 | 👀 观察 | 成本锚点、缓存 TTL、命中规则 | D |
| DeepInfer 的缓存限容教训 | ✅ 吸收（纪律） | CVE 级 DoS 模式 | 立即 |

---

## 1. 事实快照（实测，非二手转述）

| 项 | DeepInfer | deepseek-recipe |
|---|---|---|
| 提交 | `8b16149`，2026-10-20 | `935817c`，2026-10-20 |
| 规模 | 341 文件 / 5.4MB（py 270 / hpp 18 / cpp 15） | 139 文件 / 13.8MB（rs 86，828KB；json 12.9MB 为两份 tokenizer） |
| 许可证 | Apache-2.0（源文件带 `Derived from vLLM; modified by DeepSeek.`） | MIT（内置 tokenizer 同为 MIT，`Copyright (c) 2023 DeepSeek`） |
| 语言/工具链 | Python 3.12、CUDA 13、`torch==2.13.0+cu132`、`flash-attn-4==4.0.0b34`、CANN 9.2 | Rust edition 2024，workspace resolver 3 |
| 硬件 | NVIDIA SM100 / 华为昇腾 950 | 无（纯 CPU 库） |
| 形态 | 离线推理服务（在线服务组件未开源：KV 系统、P/D 分离、跨副本 LB） | 库 + Python bindings + 两个 mock server 示例 |

---

## 2. DeepInfer 解剖（只记"能解释上游行为"的部分）

### 2.1 模块平面

| 层 | 位置 | 关键内容 |
|---|---|---|
| 引擎 | `deepinfer/engine/llm_engine.py`（vLLM 衍生）、`engine_type.py` | `NORMAL` / `PROMPT` / `COMPLETION` 三型（P/D 分离的进程形态）；异步（zero-overhead）调度仅 `NORMAL`/`COMPLETION` 默认开 |
| 调度器 | `csrc/engine/scheduler.hpp`（C++，66KB，pybind 绑到 Python） | `SchedulerOutputs{scheduled_seqs, seqlens_q, sum_seqlen_qxk, num_cached_tokens_gpu, clone_states, clone_blocks, zero_states}`；`GPU_CACHE_PREFILL_WATERMARK=0.9` |
| Worker | `deepinfer/worker/{model_runner,cuda_graph_runner,dflash_cuda_graph_runner}.py` | CUDA Graph 按 batch 桶（`CUDAGRAPH_BATCH_SIZE_INTERVAL=8`） |
| 算子 | `deepinfer/model_executor/layers/`（102 文件）+ `csrc/` | FlashMLA / DeepGEMM / DeepEP / TileKernels / DeepSelect + TileLang 自定义 |
| MoE 负载均衡 | `deepinfer/model_executor/eplb/{policy,shuffle}.py` | 冗余专家 + 周期重排 |
| 结构化输出 | `deepinfer/structure_output/{compiler,matcher,so_matcher_builder}.py` | xgrammar 编译成自动机 → 每步出 mask |
| 昇腾 | `deepinfer/npu_ops/`、`requirements-ascend.txt` | 平台抽象 `DEEPINFER_ACCELERATOR=ascend` |

### 2.2 十个特性（README 自陈，均已开源）

优化算子与 sampler 运行时、Attention DP / 均衡 CP、**MoE EP + EPLB**、**DSpark 置信度调度验证**、**SWA bounded replay**、异步调度、**batch-invariant 推理**、**确定性采样**、结构化输出。

调参入口集中在 `deepinfer/environ.py`（一个 `EnvVar` 描述符类，首次访问惰性求值 + `rearm()` 重读）——**这份文件本身就是一份"推理服务全部可调面"的清单**，值得作为我们读上游性能问题的索引。

### 2.3 Q&A 文档里对我们有解释力的事实

| 事实 | 为什么对我们有用 |
|---|---|
| 单集群 **155 万亿 tokens/天**；**每卡每小时 38 美金（谷价）** | 上游定价的成本锚点。我们做模型路由/成本估算时有量级参照 |
| 线上并行：Prefill `EP2~8/DP2~8`，Decode `EP64~128/DP64~128`（Wide-EP）；P:D ≈ 1:1；高效最小单元 64P+64D | 解释"为什么长上下文贵、为什么 decode 吃满带宽" |
| KVCache 保留 **agent 流量 3 天 / chatbot 7 天**；3 天 TTL 使 chatbot 命中率降 0.5~1% | **前缀稳定性 = 命中率 = 成本**。我们的系统提示词/工具定义顺序一旦抖动，直接打掉上游缓存命中 |
| 命中长度由 **Block KVCache** 决定；最后一块是否重算由 **SWA KVCache** 决定；最大命中 ≤ L−134（窗口 128 + 投机回退 6） | 解释"为什么同一会话里追加内容时，只有末尾 134 token 需重算"——**长会话续写的前缀命中几乎免费的前提，是前文一字不改** |
| SWA KVCache 只在 **128 token 整数倍**位置存储；chunked prefill 中途不存 | 解释"为何我们改历史中段会让整段缓存失效" |
| DSpark 变长验证：draft 5 个，动态裁剪到 3.2~4.4，接受长度基本不变，**+4~7% 吞吐** | 投机解码的边际收益结构；只作为认知 |
| batch-invariant（`DEEPINFER_BATCH_INVARIANT=1`）+ 确定性采样（固定 `seq_seed`） | "同输入同输出"要靠框架级保证，不是 prompt 能求来的 |
| 结构化输出编译与推理解耦，缓存有容量上限（CVE-2025-32381） | 缓存限容纪律（见 §9） |

---

## 3. deepseek-recipe 解剖（真正可吸收的部分）

### 3.1 crate 布局

| crate | 职责 | 依赖重量 |
|---|---|---|
| `deepseek-recipe-core` | 对话/消息/图片/工具类型、special token 定义 | 极轻（serde + 宏） |
| `deepseek-recipe-encoding` | V4 / V4.1 prompt 渲染 + token 编码 | `tokenizers 0.23.2` + `aho-corasick` |
| `deepseek-recipe` | 三种协议（Chat Completions / Responses / Messages）请求转换、流式响应、输出解析、结构化输出选型 | 中（jsonschema、tokio-stream、base64…） |
| `deepseek-recipe-image` | 图片拉取 + V4.1 预处理 | **重**：`opencv 0.93` |
| `deepseek-recipe-macros` | `prompt!` 宏 | 编译期 |
| `deepseek-recipe-python` | PyO3 绑定 | — |

**只取 encoding + core 时，唯一的新增重量级依赖是 `tokenizers`**（默认走 `onig` 原生 C 构建；本机 MSVC 可编，但会引入一个需要 cc 的构建依赖）。

### 3.2 我们真正需要的两个 API

```rust
// 精确计数所需的最小面
let tokenizer = Tokenizer::from_file("tokenizer.json")?;
let encoding = DeepseekV41Encoding::new().with_tokenizer(PromptTokenizer::new(&tokenizer));
let ids: Vec<u32> = encoding.encode(&conversation)?;   // 渲染 + 分词一体
```

- **`PromptTokenizer` 的核心设计（值得抄，不只是用）**：它持有两份词表——**去掉 added vocabulary 的 plain tokenizer** 与 **special token 的 id 映射**；渲染时记录 `special_ranges`（显式控制 token 的字节区间），编码时把区间之间的普通文本**批量并行**编码、区间本身直接映射为 special token id。
  - 安全语义：用户消息里写出 `<｜User｜>` 这类**拼写**，是普通文本 → 编成普通 token，**不会变成控制 token**。这是 prompt injection 的一道结构性防线，不是靠过滤字符串。
- **`render_conversation` 返回 `RenderedPrompt{prompt, special_ranges, image_sources}`**：把"文本"和"哪些区间是控制符"一起交出去，调用方自己分词时也不会错。

### 3.3 `max_tokens` 契约（可直接对齐）

`ConversationRequest::resolve_max_tokens(prompt_tokens, context_limit)`：

| 场景 | 默认 |
|---|---|
| 非思考模式 | 8192 |
| 思考模式 | 65536 |
| 思考模式 + `reasoning_effort=max` | 131072 |

规则：请求显式指定的 `max_tokens` **保持不变**；未指定时按上表取默认，并**裁剪到 prompt 之后剩余上下文以内**；若 `prompt_tokens + max_tokens > context_limit` → 返回 `ConversionError`（**显式报错，不静默截断**）。

另一个小机制：`ConversionOptions::with_model_alias` —— 官方只认 `DeepSeek-V4-Pro` / `DeepSeek-V4.1-Flash`，`deepseek-flash` 这类名字必须显式注册别名。**这和我们 `${slug}-222` 的目录别名是同一类问题**，值得对照我们的模型解析是否也是"白名单 + 显式别名"。

### 3.4 内置 tokenizer 的出处与可信度

| 文件 | 上游 | 本地改动 |
|---|---|---|
| `static/tokenizers/v4/tokenizer.json`（6.63MB） | `DeepSeek-V4-Pro` | 见 `v4/README.md` |
| `static/tokenizers/v41/tokenizer.json`（6.63MB） | `DeepSeek-V4-Flash-Vision-Exp` @`6821d6a` | **仅一处**：id `129264` 由 `<｜deepseek_image｜>`（special=false）改为 `<｜image｜>`（special=true）；其余条目与比对修订一致；附 SHA-256 |

出处、校验和、语义差异三件套齐全——**这正是我们错题本里"词表/索引含义类改动必须钉住出处"的正面样本**，可作为 vendor 第三方资产的登记模板。

---

## 4. 与本仓库现状的对照（含 file:line）

| 我们的现状 | 证据 | 与本次吸收的关系 |
|---|---|---|
| token 计数是启发式：`BYTES_PER_TOKEN=4`、CJK `3/5`、图片 `765` | `crates/codegen/cf-token-estimation/src/lib.rs:10,17,31,32,36` | **A 批主战场**。该文件注释自陈 "Exposed as a constant so the ratio can be re-tuned against a real tokenizer without touching call sites"——吸收点已被预留 |
| 中文估算误差"仓内无 BPE 可对照，幅度为经验值" | `docs/token-optimization/方案.md:69` | recipe 正好补上"可对照的真值" |
| P0-1 已立项：真 BPE tokenizer 接入真实判定链（增量部分误差 40% → <5%） | `docs/token-optimization/方案.md:98` | **本文是 P0-1 的数据源与实现路线补强** |
| ⚠️ 潜伏 seam 陷阱：`ItemTokenCounter` seam 的生产消费者不存在 | 同上 `:101`（审计 F1 勘误） | 吸收时**禁止**把 tokenizer 塞进该 seam，否则"零收益" |
| 无 `tokenizers` 依赖 | `git grep` 全仓 `*.toml` 零命中 | 新增依赖需过构建/体积评审 |
| 流式解析已有三协议实现 | `cf-sampler/src/stream/{chat_completions,messages,responses}.rs` | ⛔ 不需要 recipe 的解析层 |
| 结构化输出已有 `StructuredOutput` 合成工具 + 参数校验重试 | `cf-shell/src/session/acp_session_impl/turn.rs:7-18,1564-1605` | ⛔ 不需要 xgrammar 路线 |
| 输出预算散落在业务点上（`Some(1024)`、`Some(50)`、`Some(8192)`…） | `cf-shell/.../memory_dream.rs:736`、`recap.rs:518`、`replay.rs:723` | **B 批**：缺一个统一的"主对话 max_tokens 解析" |

---

## 5. A 批（P0）：精确 token 计数接入

### 5.1 目标与验收

| 项 | 现状 | 目标 |
|---|---|---|
| 中文文本增量估算误差 | ~30–50%（保守高估） | **≤5%** |
| 压缩触发点（85% 阈值）实际落点 | 80–84% | 84–85.5% |
| 是否影响 wire 实测基数 | 不影响（`total_tokens` 仍以 usage 为准） | 保持一致，**只替换增量项** |
| 失败降级 | — | tokenizer 缺失/模型不支持 → **静默回退启发式**（不得 panic、不得阻塞） |

### 5.2 实现要点

1. **不要改 `cf-token-estimation` 的纯函数签名**：新增 `TokenCounter` 抽象（`estimate(&str) -> u64` 的对象安全 trait），启发式是默认实现，精确实现是可选 feature（建议 feature 名 `exact-tokenizer`，默认关）。
2. **tokenizer 分发按模型**，不按仓库：一张 `model_slug → tokenizer.json 路径/哈希` 的注册表；**缺失即回退**。DeepSeek V4/V4.1 直接用 recipe 的两份（必须记录 SHA-256）；其他厂商走各自 HF `tokenizer.json`（有则用，无则回退）。
3. **只对"增量"生效**：按 `方案.md:101` 的勘误，落点是 `get_estimated_total_tokens()` 的增量项，**不是** `ItemTokenCounter` seam（该 seam 无生产消费者）。
4. **缓存与预算**：编码是 CPU 密集，长文本（工具结果）单次可达数十 KB。必须：
   - 按 `(model, content_hash)` 做**有界** LRU（默认 4096 条 / 64MB 上限，照 §9 纪律）；
   - 放在已有异步路径上，禁止在 turn 关键路径同步阻塞（长内容用 `spawn_blocking`）。
5. **体积**：两份 tokenizer 各 6.6MB。建议**不进二进制**（`include_bytes!` 会撑大 exe），改为首次使用时按需放到用户缓存目录（`cf-dirs` 现有约定），或按"该模型被实际使用"懒加载。取舍见 §11 待决 1。
6. **special token 处理**：我们的场景是**给已存在的消息计数**，不做 prompt 渲染，所以只需 plain 文本编码；但若同一段文本里出现 `<｜User｜>` 拼写，**必须按普通文本编码**（这正是 `PromptTokenizer` 的语义）——用一个测试钉死。

### 5.3 必须有的测试

1. **真值对照**：录制一组真实会话（含中文、代码、混合、emoji、超长工具结果），比对 `tokenizer` 计数 vs 上游 `usage.prompt_tokens`，误差落盘为回归基线。
2. **回退路径**：tokenizer 文件缺失 / 加载失败 / 未知模型 → 返回启发式值且**不 panic**。
3. **CJK 边界**：`estimate_tokens_aware` 的 per-segment ceil 语义不得因接入而回退（现有注释 `lib.rs:70-79` 明确记载了分段取整差异）。
4. **缓存有界**：插入超过上限时 LRU 淘汰，内存不增长（可直接断言容量）。
5. **注入语义**：内容含 `<｜User｜>` / `<｜image｜>` 拼写 → 编码结果不含对应 special id。

### 5.4 工时

| 工作 | 估时 |
|---|---|
| 抽象 + feature + 分发注册表 + 回退 | 0.5–1 天 |
| tokenizer 资产落位（下载/校验/懒加载/离线降级） | 0.5 天 |
| 有界缓存 + 异步接线 | 0.5 天 |
| 真值对照基线与 5 类测试 | 1 天 |
| 合计 | **2.5–3 天**（不含审计与返工） |

---

## 6. B 批（P1）：`max_tokens` 契约对齐

**问题**：我们对主对话的输出预算没有统一解析——散落为业务常量（`recap.rs:518` 的 50、`memory_dream.rs:736` 的 1024、`replay.rs:723` 的 8192），且**看不到"请求值 vs 上下文剩余"的裁剪与溢出规则**。

**方案**：新建一个纯函数（建议放 `cf-sampling-types` 或 `cf-token-estimation` 旁边）：

```
resolve_max_tokens(requested: Option<u32>, thinking: bool, effort: Effort, prompt_tokens: u64, context_limit: u64)
  -> Result<u32, Overflow>   // 裁剪 + 溢出显式报错
```

默认档位对齐 recipe（8192 / 65536 / 131072）。**不要顺手改已有业务点的常量**——先接线主对话路径，业务子调用（recap/memory_dream）保持现状，避免一次改动搅动多处行为。

配套：`guidance` —— 溢出时报错文案应给出"prompt_tokens / max_tokens / context_limit"三元组，便于用户自救（当前静默截断类的报错信息不足）。

**工时**：1 天（含测试）；**风险**：中（改变实际下发参数，需灰度或 env 开关）。

---

## 7. C 批（P2）：前缀稳定性与注入防线（设计吸收，无代码依赖）

1. **前缀稳定性 = 上游缓存命中 = 成本**。Q&A 已量化：TTL 3 天，命中长度由 Block KVCache 决定，仅末尾 134 token 需重算。对我们的直接含义：
   - 系统提示词、工具定义、MCP 清单的**顺序与内容必须逐轮稳定**；任何"按 map 遍历顺序渲染"的实现都是成本炸弹；
   - 已有一个好范例：`turn_base_tool_specs` 与 `drop_web_search_for_backend_search` 的口径统一（`方案.md:99`）——继续沿这条线，把"工具定义序列化是否稳定"变成一条可测断言。
2. **注入防线的结构性做法**：如果我们将来引入本地 prompt 渲染（例如自建服务端或本地模型），必须采用 recipe 的 `special_ranges` 语义——**控制 token 只能由模板插入，不能由内容匹配生成**。
3. **别名显式化**：模型名解析走"白名单 + 显式别名注册"，不做模糊匹配（`with_model_alias` 的语义）。

---

## 8. D 批（观察）：可写进成本模型的锚点

| 锚点 | 数值 | 用途 |
|---|---|---|
| 单集群吞吐 | 155 万亿 tokens/天 | 上游容量量级 |
| 每卡每小时收入（谷价） | 38 美金 | 反推 token 单价的上限锚 |
| P:D 卡比 | ≈1:1；最小高效单元 64P+64D | 解释长上下文/高并发定价结构 |
| KV 保留 | agent 3 天 / chatbot 7 天 | 我们"隔天续聊"的命中预期 |
| SWA store 单序列 | 20 层 × 134 token × 528 B = 1,415,040 B | 解释"为什么缓存容量不是免费" |
| 投机解码收益 | 变长验证 +4~7% 吞吐 | 上游降本节奏 |
| EPLB 不均衡度 | moe_lb 1.045–1.090 | 服务端 5–9% 的固有损耗 |

这些只用于**判断与解释**，不进代码。

---

## 9. 立即吸收的一条纪律：缓存必须限容（CVE 级教训）

`deepinfer/structure_output/compiler.py:26-31` 的注释直接点名：xgrammar 缓存若不设 `cache_limit_bytes`，**随不同 schema 无界增长 → OOM（DoS 向量，CVE-2025-32381）**；代码默认给了 10GB 上限，并在配置为 0 时打 warning。

**对照自查**（本仓库需逐项确认，本文不预设结论）：
- 工具结果/文件内容的 hash 去重表；
- `codebase-graph` 索引、`mem` 记忆检索的向量/文本缓存；
- MCP 工具清单与 schema 缓存；
- 任何 `HashMap<String, X>` 且 key 来自外部输入（URL、路径、schema、模型名）的结构。

**判据**：key 可否由外部输入无限生成？能 → 必须有容量上限 + LRU + 上限命中时的行为定义（拒绝/降级/直通）。

---

## 10. 不吸收清单（附理由，防止后续重复讨论）

| 项 | 理由 |
|---|---|
| DeepInfer 全部 Python/C++ 代码 | 需 SM100 / Ascend 950 / CANN 9.2 / 多机 NCCL；我们是 CPU 侧客户端 |
| KV 系统 / P-D 分离 / 跨副本 LB | **官方未开源**（README 明示"not included in this release"），Q&A 只有设计描述 |
| xgrammar 结构化输出 | 我们走"合成工具 + 参数校验 + 重试"路线（`turn.rs:1573-1605`），语义不同且更好调试 |
| recipe 的协议转换与流式解析 | 方向是"token 流 → 协议事件"（服务端）；我们是"协议事件 → 客户端 UI" |
| recipe 的 `deepseek-recipe-image`（OpenCV） | 引入 `opencv 0.93` 原生依赖，收益不抵成本 |
| DeepInfer 的 TileLang 算子 / Mega MoE | 无 GPU 推理场景 |

---

## 11. 待决问题（需主理人定）

1. **tokenizer 资产的交付形态**：内嵌（exe +13MB，离线可用）vs 按需下载（+网络依赖、需校验和）vs 仅开发期使用（生产仍启发式，只做校准）。**建议先做"开发期校准 + 生产按需懒加载"**，把内嵌留到确有必要时。
2. **是否引入 `tokenizers` 依赖**：它带 `onig` 原生 C 构建（需 cc；MSVC 可用）。若只是"偶尔校准"，可以只在 `tools/` 下做一个一次性校验工具，**不进产品依赖树**。
3. **B 批的默认档位是否照抄 8192/65536/131072**：我们的模型目录里有非 DeepSeek 模型，照抄可能不合适；建议按"厂商默认 + 可配置覆盖"落地。
4. **A 批是否要先跑一次"误差实测"再决定投入**：低成本方案——用 recipe 的 tokenizer 对 20 段真实会话做离线对照，出一页误差报告（半天），再决定要不要进产品。**推荐先做这一步**。

---

## 12. 附录

### 12.1 复现（本机实测可用）

```powershell
# github 需首尔 SOCKS5（AGENTS.md §10）
git clone --depth 1 https://github.com/deepseek-ai/DeepInfer.git        target/tmp/deepinfer-study/DeepInfer
git clone --depth 1 https://github.com/deepseek-ai/deepseek-recipe.git  target/tmp/deepinfer-study/deepseek-recipe
```

### 12.2 关键文件索引

| 文件 | 为什么读它 |
|---|---|
| `DeepInfer/README.md` | 特性清单与"未开源"边界 |
| `DeepInfer/deepinfer/environ.py` | 全部可调面（惰性 EnvVar 描述符，可作我们的配置层参照） |
| `DeepInfer/examples/README.md` | benchmark 口径、PPL 基线、PD 解耦测量方法 |
| `DeepInfer/doc/*Q&A.pdf`（14 页） | 部署/缓存/负载均衡/投机解码的一手数值 |
| `DeepInfer/deepinfer/structure_output/compiler.py:26-31` | 缓存限容与 CVE-2025-32381 |
| `DeepInfer/csrc/engine/scheduler.hpp:39-80` | 调度器输出结构与引擎三型 |
| `deepseek-recipe/README.zh.md` | 能力面（三种协议、思考模式、结构化输出选型） |
| `deepseek-recipe/docs/tokenizer.zh.md` | 编码/解码契约与 `resolve_max_tokens` |
| `deepseek-recipe/deepseek-recipe-encoding/src/tokenizer.rs` | `PromptTokenizer` / `special_ranges` 实现（A 批核心参考） |
| `deepseek-recipe/deepseek-recipe-encoding/src/v4/dsv41.rs` | V4.1 渲染细节（含 `Reasoning Effort: N (range 1-100…)` 模板） |
| `deepseek-recipe/deepseek-recipe-core/src/special_tokens.rs` | 全部 special token 拼写 |
| `deepseek-recipe/static/tokenizers/{v4,v41}/README.md` | 资产出处/校验和/差异登记模板 |

### 12.3 与既有文档的关系

- **补强**：`docs/token-optimization/方案.md` 的 P0-1（本文提供真值源与实现路线）。
- **独立**：`docs/absorption/adm-absorption-plan-v3.md`（ADM 竞品吸收）——两者无重叠，ADM 是产品形态互鉴，本文是上游技术栈吸收。
