# 真 BPE tokenizer：误差实测与接入方案（2026-10-20，v2 审计修订版）

> **背景**：`docs/absorption/deepseek-deepinfer-absorption.md` §11 待决 4 提出"先做半天误差实测再决定投入"。本文是这次实测的结果，以及经**双席审计**（Qwen3.8-Flash / GLM-5.3）返工后的方案。
> **一句话结论**：**实测推翻了原方案的前提**——`cf-token-estimation` 不是"高估中文"，而是**整体低估约 18%**（分层语料 token 加权 −18.1%；双尾语料 −23.2%）；39/40 会话同向。主因是 **ASCII/code/JSON 被 bytes÷4 系统性算少**，不是中文。
> **语料**：`G:\qidicode` 会话库。分层语料 40 会话 / 6,376 项 / 3,906,947 真值 token；双尾语料 40 会话 / 15,071 项 / 5,738,879。
> **复现**：`target/tmp/token-calib/`（脚本 + 语料 + 报告；`cargo clean` 会销毁，重跑见 §7）。

---

## 0. 结论

| # | 结论 | 证据 |
|---|---|---|
| 1 | **方向性错误**：启发式当**低估**（token 加权 ratio 0.819 分层 / 0.768 双尾），不是"保守高估"。安全方向反了 | §2.1 |
| 2 | **不是中文问题**：中文项在 aware 版下已接近真值（reasoning/中文散文 −2.5%~−8%），ASCII 侧才是大头——ASCII 是 token 量的主体（>3/4），且**纯 ASCII 实测 3.10 chars/token ≠ 4** | §2.3 / §2.8 |
| 3 | **会话级几乎全部同向**：分层语料 39/40 会话低估，中位 −22.6%，最差 −30.0%（仅 1 个 +1.5%） | §2.4 |
| 4 | **抽样方式影响幅度**：双尾抽样（最大 20+最小 20）把低估放大 ~5pp；分层抽样 −18.1%。**结论以分层为准** | §2.7 |
| 5 | **常数重调可消掉绝大部分聚合偏置**：`bytes÷3.4 + 0.80/字` → 分层 ratio **0.984**（双尾 0.934，两套都不高估）。草案的 `bytes÷3.1` 在分层语料上过校正到 1.062 | §2.6 / §2.7 |
| 6 | **两常数不可分离识别**（审计 F4）：唯一能干净识别的只有"纯 ASCII 项 = 3.10 chars/token"；混排项内 ASCII 只有 2.28 chars/token。**单对常数只能修聚合偏置，不能修逐项精度（MAPE 停在 16%）** | §2.8 |
| 7 | **精确 tokenizer 仍是终点**，它买的是逐项精确（MAPE 16%→0） | §4 |
| 8 | **wire usage 三向校验做不了**（turn/prompt 不 1:1、多调用累加、历史可变），本文不拿它当证据 | §2.5 |

---

## 1. 方法

### 1.1 对照物

| 角色 | 实现 | 可信度依据 |
|---|---|---|
| **被测量（现状）** | `cf-token-estimation::estimate_tokens_aware` 的 Python 逐行复刻（`lib.rs:96-104`：ASCII 1/4、CJK `ceil(3c/5)`、其余字节 1/4；per-segment 取整后求和） | 逐分支比对：CJK 三区间、ASCII 计数单位、other 字节、ceil 取整、求和式全部一致（审计两席独立复核） |
| **真值** | `deepseek-recipe` 官方 `static/tokenizers/v41/tokenizer.json`（MIT，`Copyright (c) 2023 DeepSeek`）经 `tokenizers 0.23.2` 编码，`add_special_tokens=False` | 官方随模型分发；控制样本落在已知区间（§2.3）；JSON 实测为 DeepSeek 系 BPE 词表 |

### 1.2 语料构造

对每个会话的 `chat_history.jsonl`，按 `estimate_item_tokens`（`state.rs:68-105`）的口径抽取"会被计数的文本段"：

| item 类型 | 计入的文本（与 Rust 一致） |
|---|---|
| `system` / `user` / `assistant` | `content`（user 取 text parts）+ 每个 `tool_calls[].arguments` |
| `tool_result` | `content` |
| `reasoning` | **`summary` parts + `content` blocks，用 `\n` 合并为一个串**（`conversation.rs:1248-1261` `reasoning_item_text`），与产品同构 |

**不计**：系统提示词模板、工具定义、图片（图片按 765 常量，属独立口径）、`reasoning.encrypted_content`（base64，产品侧按 bytes/4 计，语料中未出现）。

**两套抽样**：
- **双尾**（`build_corpus.py`）：体积最大 20 + 最小 20 —— 偏斜，仅作对照；
- **分层**（`build_corpus_strat.py`）：1307 个合格会话按体积排序后等距取 40 —— **代表性，结论以此为准**。

### 1.3 指标

- **ratio = Σh/Σe**：决定压缩闸门何时触发（闸门对 Σ 判定）。
- **bias（逐项均值）**：`mean((h−e)/e)`，反映"逐项典型偏差"，**与 ratio 不是一回事**（审计 F2）。
- **MAPE**：逐项绝对误差均值，反映逐项精度。
- 本文正文凡说"少算 x%"，一律指 `1−ratio`（聚合口径）。

---

## 2. 实测结果

### 2.1 总体

| 指标 | 双尾语料 | **分层语料（代表性）** |
|---|---|---|
| 项数 | 15,071 | 6,376 |
| 启发式合计 | 4,406,903 | 3,199,646 |
| 真值合计 | 5,738,879 | 3,906,947 |
| **聚合比** | 0.768 | **0.819** |
| **聚合偏差（1−ratio）** | −23.2% | **−18.1%** |
| bias（逐项均值） | −22.4% | −17.2% |
| MAPE / p90 / max | 23.8% / 41.9% / 246.2% | 21.0% / 38.0% / 118.6% |

> 读法：**Σ 层面少算约 18%**。闸门按估计值判定 → 真实用量到 94% 左右才触发（增量占比 50% 时），留给溢出的余量很小，只剩 preflight 兜底。

### 2.2 按消息类型（分层语料）

| 类型 | 项数 | ratio | bias（逐项均值） | MAPE |
|---|---|---|---|---|
| assistant | 2,129 | 0.741 | −25.2% | 25.3% |
| tool_result | 2,397 | **0.820** | −20.3% | 24.2% |
| user | 182 | 0.749 | −15.5% | 17.1% |
| reasoning | 1,628 | **0.902** | −2.5% | 11.1% |
| system | 40 | 0.880 | −8.9% | 13.1% |

> **`tool_result` 是最要紧的一类**（它是"上次响应以来增量"的主体，直接喂闸门）：聚合口径少算 18%。
> **reasoning 反而最准**（−2.5%）：它是中文密集、字数少的文本，aware 版的 CJK 比率对它接近正确——这进一步说明"问题在 ASCII/code"。
> 双尾语料同表（供对照）：assistant 0.684 / tool_result 0.742 / user 0.768 / reasoning 0.850 / system 0.933。

### 2.3 按文本类别 + 控制样本

| 类别（分层） | 项数 | ratio | bias | MAPE |
|---|---|---|---|---|
| ascii（几乎纯 ASCII） | 3,774 | 0.818 | −16.8% | 21.5% |
| mixed（中英混排） | 1,558 | 0.758 | −23.2% | 23.6% |
| code | 998 | 0.922 | −9.3% | 14.6% |
| cjk（纯中文） | 46 | 0.753 | −20.4% | 20.8% |

控制样本（证明 tokenizer 行为正常，n 已标注）：

| 样本 | n（字符） | chars/token | 备注 |
|---|---|---|---|
| 英文散文 | 326 | 5.34 | 该词表对英文压缩率高 |
| Python 代码 | 340 | 4.36 | |
| JSON 工具输出 | 168 | 2.85 | 标点密集 |
| Markdown 混排 | 82 | 2.10 | |
| 中文散文 | 109 | 1.76（=0.569 token/字） | 落在 0.5–0.6 带内 |

> 关键解读：**bytes÷4 ≡ 4.0 chars/token**。而**纯 ASCII 消息实测 3.10 chars/token**（§2.8，n=1,211 项），JSON/markdown 只有 2.1–2.9。我们的 ASCII 主体不是散文，是工具输出与代码——这是 −18% 的主因。

### 2.4 会话级分布（分层语料，40 会话）

| 指标 | 值 |
|---|---|
| 会话级误差均值 | −20.3% |
| 中位数 | −22.6% |
| 区间 | −30.0% ~ **+1.5%** |
| 方向一致性 | **39/40 低估**（1 个微正） |

> 双尾语料为 40/40 全负、中位 −21.5%、最差 −36.3%——差异来自抽到的会话形态，见 §2.7。

### 2.5 为什么没有用 wire usage 做三向校验（方法学说明）

设计过 `turn_completed.usage.inputTokens` vs `exact(history)` 的残差校验，实测**数据模型不支持**：

1. `prompt_index` 只标在 user 项上，与 `turn_completed` 数不 1:1（子代理/后台任务/续跑都产生 turn）；
2. `inputTokens` 在 `modelCalls>1` 时是一个 turn 内多次调用的**累加**；
3. 输出侧同样对不齐（`outputTokens` 与本 turn 新增 assistant 文本的比值在 0.27~15.7 间乱跳）；
4. **更根本**：工具结果老化/压缩会改写历史（`方案.md` 的 PruningConfig），使"turn 时点的 history"从当前 jsonl **不可恢复**（审计 F8 补充）。

**处置**：不把 wire 校验写进结论。本文只依赖"文本级真值"，这恰好也是 P0-1 需要的东西（偏差只作用于增量）。若要 wire 级校准，产品侧需记录每次采样的 prompt 组成快照（独立可观测性需求）。

### 2.6 常数网格（C：精确 tokenizer 为上限）

分层语料：

| bytes/token \ CJK | 0.55 | 0.60 | 0.64 | 0.70 | 0.75 |
|---|---|---|---|---|---|
| 3.0 | 20.2/1.04 | 20.0/1.05 | 19.9/1.06 | 19.7/1.07 | 19.6/1.08 |
| 3.2 | 18.2/0.98 | 17.9/0.99 | 17.6/1.00 | 17.3/1.01 | 17.1/1.02 |
| 3.3 | 17.8/0.95 | 17.5/0.96 | 17.2/0.97 | 16.8/0.99 | 16.6/1.00 |
| 3.5 | 17.8/0.91 | 17.4/0.92 | 17.1/0.93 | 16.6/0.94 | 16.3/0.95 |
| 4.0 | 21.4/0.81 | 20.9/0.82 | 20.5/0.83 | 20.0/0.84 | 19.6/0.85 |

（格式：MAPE / 聚合比。现状 A = 4.0/0.60 → 20.9% / 0.819；最优点 `grid.py` 给出 **3.4 / 0.80 → 16.0% / 0.984**。）

双尾语料最优点：3.1 / 0.80 → 15.6% / 0.999。

**读数**：
- 单靠调 CJK 比率，ratio 最多从 0.819 拉到 ~0.85——**中文比率不是主因**；
- 单靠调 ASCII 除数，ratio 可到 ~0.98–1.00——**这才是主因**；
- 两者联合调才能到 0.98，但 **MAPE 只从 21% 降到 16%**——因为两常数在混排项上互相补偿（§2.8）。

### 2.7 抽样方式的影响（审计 R1）

| 指标 | 双尾（最大 20+最小 20） | **分层（按体积等距）** |
|---|---|---|
| 聚合比 | 0.768 | **0.819** |
| bias（逐项均值） | −22.4% | −17.2% |
| MAPE | 23.8% | 21.0% |
| 会话级 40/40 负偏？ | 是 | **否（39/40）** |
| 最优点 | 3.1 / 0.80 | **3.4 / 0.80** |

**结论**：双尾抽样把低估放大了约 5pp，并把最优常数从 3.4 拉到 3.1（过校正方向）。**M1 的常数与夹具必须基于分层语料。**

**候选常数稳健性**（关键决策表）：

| 常数 | 双尾 ratio / MAPE | 分层 ratio / MAPE | 判定 |
|---|---|---|---|
| 现状 4.0 / 0.60 | 0.768 / 23.8% | 0.819 / 20.9% | 两套都低估 |
| 草案 3.1 / 0.80 | 0.999 / 15.6% | **1.062** / 18.0% | 双尾完美、分层过校正 → 不可外推 |
| **采用 3.4 / 0.80** | 0.934 / 14.4% | **0.984** / 16.0% | **两套都不高估**，代表性语料近完美 → 采用 |

### 2.8 分离识别：为什么"一个数"修不好（审计 F4）

把两常数从混排文本里一起拟合，它们会互相吸收（ASCII 少算由 CJK 多算补偿）。做分离识别：

| 子集 | 识别结果 | 含义 |
|---|---|---|
| **纯 ASCII 项**（n=1,211，≥200 字符，无 CJK） | **3.10 chars/token**（逐项：p10 2.46 / 中位 3.22 / p90 4.11） | 英语类文本的真实速率 = bytes÷3.1 |
| **含 CJK 项内的 ASCII**（>30% CJK 的项，n=263） | **2.28 chars/token** | 夹在中文里的英文/符号**更贵** |
| 含 CJK 项内的中文 | 0.702 token/字（分层）/ 0.808（双尾） | 与纯中文控制样本 0.569 不一致（混排拉高） |
| 四参数（ascii, cjk, other, 截距）最小二乘 | MAPE 41.5%（分层） | 截距模型被大项主导，小项误差反而爆掉 |

**结论**：
1. **"ASCII 每 3.1 字符一个 token"只在纯英文语境成立**；我们的语料以 markdown/JSON/日志为主，混排段更贵，所以聚合层的最优值是 3.4 而不是 3.1。
2. 因此 `bytes÷3.4 + 0.80` 是**对本次语料分布的补偿式拟合**，不是任何单一文体的物理标定——**它能把聚合比修到 0.98，但修不了逐项精度（MAPE 16%）**。
3. 要真正修逐项精度，只有精确 tokenizer（M2）。

---

## 3. 偏置如何传导到 85% 闸门

闸门只对"增量"用估算（`total_tokens` 是 wire 实测；`estimated_tokens_since_model` 是估算，`mutations.rs:138/173` 为 `+=` 累加，非整段重估）。设增量占比 `I`，聚合比 `r`：

```
真实触发点 ≈ 0.85 / (1 − I·(1−r))
```

| 增量占比 I | 现状双尾 (r=0.768) | 现状分层 (r=0.819) | 重调后 (r=0.984) |
|---|---|---|---|
| 10% | 87.0% | 86.6% | 85.1% |
| 20% | 89.1% | 88.2% | 85.3% |
| 30% | 91.4% | 89.9% | 85.4% |
| 40% | 93.7% | 91.6% | 85.5% |
| 50% | 96.2% | 93.5% | 85.7% |

> 结论：现状在"大工具输出"场景（I≈40–50%）会触发在 92–96% 窗口，逼近上限；重调后偏差 ≤0.7pp。

---

## 4. 方案

### 4.1 目标 / 非目标

**目标**：压缩闸门、`/context` 展示、成本预估的"估算 token"在**聚合层**与真值偏差 ≤3%。
**非目标**：不做 prompt 渲染计数（模板/工具定义/system 前缀由 wire usage 提供）；不追求逐项精确（那需要 M2）。

### 4.2 三方案与推荐

| 方案 | 做法 | 成本 | 收益 | 风险 |
|---|---|---|---|---|
| **M1 常数重调** | ASCII 有理数速率 `10/34`（bytes÷3.4）、CJK `4/5`（0.80）+ 分层锁定语料回归测试 | 0.5 天 | 聚合偏差 −18.1% → −1.6%（分层） | 极低（纯数值）；对非 DeepSeek 词表未标定 |
| **M2 精确 tokenizer** | `TokenCounter` trait + `exact-tokenizer` feature + 按模型注册表 + 有界缓存 + 异步 | 2.5–3 天 | MAPE 16% → 0 | 中：依赖/资产/体积/首启延迟 |
| **M3 仅开发期校准** | harness 留作工具，生产仍启发式 | 0 | 无（只知道真相） | 无 |

**推荐：M1 立即做，M2 排 P0-1 正式落地。**

### 4.3 M1 细节

1. **不要直接把 `BYTES_PER_TOKEN` 从 4 改成 3.4**——该常量通过 `estimate_tokens()`（`lib.rs:17`）与 `truncate.rs:190-197` 被多处生产路径消费：`state.rs:39`（工具定义）、`state.rs:104`（reasoning base64）、`payload_accounting.rs:598/603`（hosted/residual 计量）、`web_fetch/overflow.rs:174`（抓取预算）、`read_file/mod.rs:477`（读文件预算）。改为引入**独立的有理数速率**：
   ```
   ASCII_TOKENS_NUM / ASCII_TOKENS_DEN = 10 / 34   // ≈ 0.294 token/byte ≈ bytes÷3.4
   CJK_TOKENS_NUM / CJK_TOKENS_DEN     = 4 / 5     // 0.80 token/字
   ```
   整数运算：`ceil(ascii_bytes*10/34)`、`ceil(cjk*4/5)`；`BYTES_PER_TOKEN=4` 原样保留给上述几条路径。
2. **`estimate_chars` 会失去互逆性**（审计 F5）：`tokens×4` 与新速率 3.4 差 +17.6%；`overflow.rs:174` 用 `estimate_chars(context_window)` 反推字符预算，M1 后同预算实际可塞入更多 token（方向=溢出）。短期在该处显式降额，或让 `estimate_chars` 跟随新速率并统一测试。
3. 更新被锁定的单测（`lib.rs:203-208` 锁 3/5）**与 `lib.rs:24-30`/`:59-65` 的 doc comment**（那里仍写"对中文高估 30–50%"，M1 后即相反）。
4. **回归夹具必须分层**：~200 条覆盖"5 类内容 × 大/中/小会话规模"，断言 `Σh/Σe ∈ [0.95, 1.05]`。用双尾语料做夹具等于拟合了一个偏斜分布（§2.7）。
5. `/context` 文案注明"基于 DeepSeek V4.1 词表、分层语料标定（聚合层）"。
6. **发布说明**：典型会话的压缩点从"约 89%"回到 85% 附近；超长工具输出会话会**更早**压缩（这是修正，不是回归）。

### 4.4 M2 细节（若要落地）

1. `trait TokenCounter { fn count(&self, text: &str) -> u64; }`；默认=新常数启发式；`exact-tokenizer` feature 下为 `BpeCounter`。
2. **注册表** `model_family → tokenizer 资产 + sha256`；DeepSeek V4/V4.1 用 recipe 的两份（MIT，保留 copyright），其他厂商留空 → 回退启发式。
3. **落点**：只替换增量折算。**不要**塞 `ItemTokenCounter` seam（`方案.md:101`：该 seam 无生产消费者）。
4. **缓存**：`(family, content_hash)` 有界 LRU（≤64MB / ≤4096 条），key 可被外部输入无限生成，必须限容。
5. **异步**：大文本 `spawn_blocking`，不得阻塞 turn 关键路径。
6. **资产不进二进制**（`include_bytes!` 会让 exe +13MB）；首次使用落 `cf-dirs` 缓存并校验 sha256，失败即回退。
7. **开关**：`QIDI_EXACT_TOKENIZER=0` 一键回退 M1 启发式。

### 4.5 测试计划

1. 常数锁定：纯 ASCII / 纯中文 / 混排 / emoji / 全角标点（现有 10 个单测更新期望）。
2. 聚合比回归：分层夹具 `Σh/Σe ∈ [0.95,1.05]`。
3. 回退：tokenizer 缺失/损坏/未知模型 → 启发式值，不 panic。
4. 缓存有界：超限淘汰，内存不增长。
5. 注入语义：内容含 `<｜User｜>` 拼写不得产生 special token id。
6. per-segment 取整语义不回退（`lib.rs:70-79`）；reasoning 的"合并单串后再 ceil"语义必须与 `reasoning_item_text` 一致（审计 F1 的教训）。

### 4.6 分期

| 期 | 内容 | 工时 | 验收 |
|---|---|---|---|
| M1 | 常数 + 有理数速率 + `estimate_chars` 处置 + 注释/单测 | 0.5 天 | 分层夹具 ratio ∈ [0.95,1.05]；`cargo test -p cf-token-estimation` 绿 |
| M1.5 | 分层夹具入库（脱敏、<100KB） | 0.25 天 | CI 可跑 |
| M2 | trait + feature + 注册表 + 缓存 + 异步 | 2.5–3 天 | 开关/回退/缓存边界全绿 |
| M2.5 | 其他厂商词表标定 | 1–2 天/家 | 逐家报告 |

---

## 5. 风险与未决

| # | 风险 | 处置 |
|---|---|---|
| 1 | **只标定 DeepSeek 词表**；GLM/Kimi/Qwen 未测，常数可能反向 | M1 取"宁可高估"；M2 按家族回退；上线前各跑一次 harness |
| 2 | **补偿式拟合**（审计 F4）：3.4/0.80 只对本次语料分布成立，逐项精度仍在 16% | 夹具按"内容类别 × 会话规模"分层；对外只声称"聚合层"精度 |
| 3 | **`estimate_chars` 失配**（审计 F5）：+17.6%，web_fetch 预算方向变危险 | 同期处置，见 §4.3-2 |
| 4 | **抽样偏差**（审计 F1/R1）：双尾放大 5pp；微型重复会话稀释独立性 | 已改分层；`build_corpus` 增内容哈希去重 |
| 5 | **语料口径偏离**（审计 F1）：reasoning 曾按"summary or content"二选一，与 `reasoning_item_text`（合并）不符 → v2 已修，方**差最大的一类**正是它（修正前后 reasoning ratio 0.824→0.902） | 已修；夹具需固化该语义 |
| 6 | wire 校准做不了（§2.5） | 需产品侧记录 prompt 组成快照 |
| 7 | 常数改动改变 `/context` 与压缩观感 | 发布说明 + 灰度 |
| 8 | 许可证 | MIT，保留 `Copyright (c) 2023 DeepSeek` 与 sha256 登记 |
| 9 | 偏差掩盖其它问题 | 把"估算 vs wire 差值"打进 metrics |

---

## 6. 与既有文档的关系

- **勘误**：`docs/token-optimization/方案.md` §5-1「bytes/4 对中文高估 30–50%」——纯中文文本上成立（0.75 vs 0.569），但**真实工作量上不成立**：ASCII/code/JSON 是主体，整体低估约 18%。已在原文加勘误指针（v2 口径）。
- **补强**：同文件 P0-1——本文给出真值源、落点、常数与顺序。
- **联动**：`docs/absorption/deepseek-deepinfer-absorption.md` A 批——本文是其"先测半天"的产出。

---

## 7. 附录：复现

```powershell
# 0) 官方 tokenizer（MIT）
git clone --depth 1 https://github.com/deepseek-ai/deepseek-recipe.git target/tmp/deepinfer-study/deepseek-recipe
Copy-Item target/tmp/deepinfer-study/deepseek-recipe/static/tokenizers/v41/tokenizer.json target/tmp/token-calib/tokenizer_v41.json

# 1) 正确性验证前置（审计 F7）
python control.py tokenizer_v41.json

# 2) 语料（分层为准）
python build_corpus.py corpus2.jsonl 40                # 双尾（对照）
python build_corpus_strat.py corpus_strat2.jsonl 40    # 分层（代表）
python measure.py corpus_strat2.jsonl tokenizer_v41.json calib2_strat.json
python grid.py corpus_strat2.jsonl tokenizer_v41.json
python separable2.py corpus_strat2.jsonl tokenizer_v41.json   # 分离识别
python session_stats.py calib2_strat.json
```

| 脚本 | 作用 |
|---|---|
| `build_corpus.py` / `build_corpus_strat.py` | 双尾 / 分层抽样（口径镜像 `estimate_item_tokens`） |
| `measure.py` | 启发式复刻 + 真值 + 分组统计 |
| `grid.py` | 常数网格搜索 |
| `separable2.py` | ASCII/CJK 分离识别 + 截距模型 |
| `control.py` | tokenizer 健全性控制样本 |
| `session_stats.py` | 会话级分布 |
| `validate_wire.py` / `validate_output.py` | wire 校验尝试（**结论：不可用**） |

---

## 8. 审计与修订记录

| 轮次 | 席位 | 结论 | 处置 |
|---|---|---|---|
| v1 | 指挥官实测 | — | 双尾语料，结论 bytes÷3.1 |
| R1 | 席位 B（codely-core = GLM-5.3） | ACCEPT WITH NITS；R-006/R-017/R-015 FAIL | 采纳 F1（抽样偏差）、F3（数字与表冲突）、F5（estimate_chars）、F9（注释）、F10（口径） |
| R2 | 席位 A（Qwen3.8-Flash） | ACCEPT WITH NITS（附硬条件） | 采纳 **F1'（reasoning 口径 bug，已修并重跑全部数字）**、F4（分离识别，新增 §2.8 并降级常数声称为"聚合层补偿"）、F3'（§3 表重算）、F7（ASCII 占比/速率改为可复算值）、F5'（`estimate_chars` 注释与调用方不符） |

**两席一致的核心判定**：
1. 低估方向与"非中文主因"成立，可复现；
2. 常数**不能**当作物理标定值，只能当聚合层补偿；
3. M1 先行合理，但落盘前必须完成口径修正（已做）与分层夹具（M1.5）。

> 原始报告：`target/tmp/audit-token-calib/seatA-report.md`、`seatB-report.md`；正式归档见文档私有库 `审计评审/2026-10-20-token-calib.md`。
