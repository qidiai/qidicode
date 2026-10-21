# M1 修复方案：token 估算常数校正（v2，2026-10-21 审计返工后）

> **依据**：`docs/token-optimization/tokenizer-calibration-and-plan.md` v2（分层语料 40 会话 / 6,376 项 / 3.91M 真值 token，聚合比 0.819）
> **审计**：codely-core（GLM-5.3）判 **REJECT**，7 项返工全部为"验收标准与影响面清单"级；核心算术（8 个测试期望值、常数选择、floor 取整）手算全部正确。本 v2 已逐条修正，见 §11。
> **目标**：把上下文用量估算的**聚合偏差**从 −18.1% 修到 ±2% 内，且**不改变**任何其它消费 `BYTES_PER_TOKEN` / `estimate_tokens` / `estimate_chars` 的行为。
> **非目标**：逐项精度（MAPE 16%→0 需 M2）；多厂商词表标定（M2.5）；prompt 渲染计数。

---

## 0. 一句话

只改 `estimate_tokens_aware` 的两个比率：**ASCII bytes÷4 → bytes÷3.4（有理数 10/34，floor）**、**CJK 0.6 → 0.8（4/5，ceil）**；`other` 桶保持 `bytes/4`。配套更新 **5 个单测函数**、**4 处注释**、**1 个 cf-shell 测试断言**，新增一个隐私安全的校准回归夹具。

**上线后预期（实测复算，见 §6）**：分层语料聚合比 **0.9811**（−1.89%），MAPE 16.0%；双尾语料 0.9312（−6.88%）。

---

## 1. 改动清单（文件级）

| # | 文件 | 改动 | 风险 |
|---|---|---|---|
| C1 | `cf-token-estimation/src/lib.rs` | 新增 `ASCII_TOKEN_NUMERATOR=10` / `ASCII_TOKEN_DENOMINATOR=34`；`CJK_TOKEN_NUMERATOR` 3→4；`estimate_tokens_aware` ASCII 分支改有理数 floor，CJK 仍 ceil，`other` 仍 `/BYTES_PER_TOKEN` | 低（纯算术） |
| C2 | 同上 + `cf-chat-state/src/actor/state.rs:20` + `cf-shell/.../payload_accounting.rs:597` | 注释勘误 **5 处**：`lib.rs:24-30`、`:59-65`、**`:66-81`**（"pure-ASCII yields exactly the same value"恒等声称）、`state.rs:20`（"bytes/4 for ASCII, 3/5"）、`payload_accounting.rs:597`（"ASCII 恒等"） | 无 |
| C3 | `cf-token-estimation/src/lib.rs` 测试区 | 更新 **5 个**测试函数（不是 6 个，见 §3）+ 新增 1 个常数锁定测试 | 无 |
| C4 | `cf-token-estimation/tests/calibration.rs`（新建） | 校准回归：读统计夹具，断言聚合比与 MAPE | 低 |
| C5 | `cf-token-estimation/tests/data/calibration_stats.json`（新建，<60KB） | **只存每项三桶计数与真值 token 数，不含任何原文** | 无（隐私安全） |
| C6 | `cf-shell/src/inspect/mod.rs:2000-2003` | 断言 `approx_tokens < estimate_tokens(&zh)` **必挂**（新 aware 160 > bytes/4 150）→ 改为反向断言 + 更新注释里的算式 | 低 |
| C7 | `cf-chat-state/src/actor/tests.rs`（9 个断言的期望值） | 见 §3.2 精确清单 | 低 |

**明确不做**：

- `BYTES_PER_TOKEN` 常量本身**不动**（仍为 4）。直接消费点实测 4 处：`state.rs:39`、`state.rs:104`、`payload_accounting.rs:598/603`；经 `truncate.rs:190-197` 包装的间接消费：`read_file/mod.rs:477`、`web_fetch/overflow.rs:174`（`truncate` 是 wrapper，不直接引用常量）。
- `estimate_tokens()`（bytes/4）、`estimate_chars()`（×4）**不动**（见 §5）。
- **M1b（`state.rs:39` 工具定义切 aware）本单不做**——拆为独立小单，避免与本单的验收口径纠缠（审计 F5）。

---

## 2. 精确实现

```rust
/// ASCII 速率：10/34 ≈ 0.294 token/byte，即 3.4 chars/token。
/// 标定自分层语料（纯 ASCII 项 n=1211 实测 3.10；混排段更贵，聚合取 3.4）。
pub const ASCII_TOKEN_NUMERATOR: u64 = 10;
pub const ASCII_TOKEN_DENOMINATOR: u64 = 34;

/// CJK 速率：4/5 = 0.8 token/字（纯中文实测 0.569；混排补偿取 0.8）。
pub const CJK_TOKEN_NUMERATOR: u64 = 4;
pub const CJK_TOKEN_DENOMINATOR: u64 = 5;

pub fn estimate_tokens_aware(s: &str) -> u64 {
    // 分桶循环不变
    let ascii_tokens = ascii_len * ASCII_TOKEN_NUMERATOR / ASCII_TOKEN_DENOMINATOR; // floor
    let cjk_tokens   = (cjk_chars * CJK_TOKEN_NUMERATOR + (CJK_TOKEN_DENOMINATOR - 1))
                       / CJK_TOKEN_DENOMINATOR;                                    // ceil
    ascii_tokens + cjk_tokens + other_bytes / BYTES_PER_TOKEN                      // other 保持 /4
}
```

**口径必须与"复算脚本"完全一致**（审计 F2 的核心）：标定脚本 `grid.py:59` 当年写的是
`a // 3.4 + float_ceil(c*0.8) + o // 3.4`——**other 桶用了 3.4、CJK 用了浮点 ceil**。本实现是
`floor(a*10/34) + int_ceil(c*4/5) + o//4`。两者差异：
- other 桶分母 3.4 vs 4（本实现更保守，other 占比 <8%，影响 <0.3pp）；
- c 为 5 的倍数时浮点 ceil 可能比整数 ceil 大 1（影响 <0.05pp）。

因此**验收数字以本实现公式复算为准**（§6），不得拿 grid.py 的 0.984 当验收线。

---

## 3. 测试改动清单（逐条，已手算）

### 3.1 `cf-token-estimation/src/lib.rs`（5 个函数）

| 测试函数 | 输入 | 旧值 | **新值** | 备注 |
|---|---|---|---|---|
| `estimate_tokens_aware_matches_bytes_over_four_for_ascii` | `"hello world"` / `"x"*4000` | 2 / 1000 | **3 / 1176** | 测试名与语义已失效 → 改名 `..._ascii_uses_calibrated_rate` |
| `estimate_tokens_aware_chinese_sits_in_measured_band` | `"你"*100` | 60 | **80** | 区间改 `78..=82`；**`t < estimate_tokens(&s)` 必须翻转**（80 > 75） |
| `estimate_tokens_aware_mixed_ascii_and_cjk` | `"你好世界hello"` | 4 | **5** | `"ab你"`=1 不变 |
| `estimate_tokens_aware_counts_fullwidth_punctuation_as_cjk` | `"，。！"` | 2 | **3** | |
| `cjk_ratio_constants_expose_the_tunable` | — | 3/5 | **4/5** + 两个 ASCII 常量 | 改动闸门 |
| （新增）`calibrated_constants_pin_the_corpus_fit` | — | — | — | 锁 10/34、4/5，断言消息必须带实际值 |

不受影响（已逐条手算）：`empty`、`emoji`（1/2）、`cjk_block_boundaries`（2/0/1/1）、`punct_via_han_arm`（2）。

### 3.2 `cf-chat-state/src/actor/tests.rs`（实测 9 个断言 + 1 个测试改名）

| 位置 | 输入 | 旧 | **新** |
|---|---|---|---|
| `:370` | 100 CJK system | 60 | **80** |
| `:375` | `"你好世界hello"` user | 4 | **5** |
| `:379` | 同上 assistant | 4 | **5** |
| `:389` | `{"路径":"你好"}`（7 ASCII + 4 CJK） | 4 | **6** |
| `:393` | `"你好世界hello"` tool_result | 4 | **5** |
| `:421` | `expected=[27,28,23,13]` 纯 ASCII → `[6,7,5,3]` | — | **`[7,8,6,3]`** |
| `:410` | `estimated_tokens_resets_on_truncate` 语义（"ASCII matches legacy"） | — | **改名 + 改注释**，期望值同上 |
| `:453` / `:470` | multisegment `ab/cd/efgh` → 1 / 0 | — | **不变**（已手算），仅注释算式过时 |

`compaction_utils.rs:3554` 有 `est` 聚合：编码手必须 `git grep` 该文件的测试断言并确认是否硬编码旧值。

### 3.3 `cf-shell/src/inspect/mod.rs`

`:2000-2003` 断言方向必须翻转（新 aware 160 > bytes/4 150），并更新 `:1979` 注释里的算式（`ceil(200*3/5)=120` → `ceil(200*4/5)=160`）。

---

## 4. 校准回归夹具（C4/C5）

**为什么不是把语料原文塞进仓库**：会话含隐私，且 origin/gitee 均为 PUBLIC（AGENTS.md §10）。只存**脱敏统计**：

```json
{
  "source": "40 sessions, size-stratified; DeepSeek V4.1 tokenizer (deepseek-recipe@935817c, MIT)",
  "generated_by": "target/tmp/token-calib (not committed)",
  "items": [{"kind": "tool_result", "size": "large", "ascii": 8123, "cjk": 412, "other": 88, "exact": 3120}]
}
```

测试逻辑：

```rust
// 用与 estimate_tokens_aware 完全相同的算术复算 h（写死在测试里，不调实现）
// 断言 Σh/Σe ∈ [0.95, 1.05]；MAPE ≤ 17%；每条断言带实际值消息
```

- **分层轴**：`kind`（5 类）× 会话规模（大/中/小），每档 ≥8 条，共 200 条。
- **残余漏洞（审计 F9）**：夹具在测试内复写算术，若将来只改实现不改夹具，夹具无感 → 用 §3 的 5 个单测钉取整方向（`"hello world"=3` 钉 floor、`"，。！"=3` 钉 ceil）作为第二道锁。
- 生成脚本不入库；夹具 JSON 保留 `source` 溯源字段。

---

## 5. 已知偏差与刻意不做的事

| 项 | 现状 | 影响 | 处置 |
|---|---|---|---|
| `estimate_chars(tokens)=tokens×4` | 与新速率 3.4 差 +17.6% | `web_fetch/overflow.rs:174` 字符预算偏大 → 溢出方向 | **本单不动**；§7 登记；**M1c（0.25 天）**单独做（含 web_fetch 前后对比） |
| `state.rs:39` 工具定义仍 bytes/4 | 工具定义 ASCII JSON，低估约 15% | `/context` tools 行偏低 | **M1b 独立小单**（1 行 + 测试），不在本单 |
| `estimate_tokens()` 仍 bytes/4 | `read_file` 截断预算、`truncate` 用它 | 截断阈值偏松 | 与 M1c 一并评估 |
| 逐项 MAPE 仍 16% | 两常数在混排段互相补偿 | 单条消息可偏 20%+ | 对外只声称"聚合层"；根治靠 M2 |
| 非 DeepSeek 词表未标定 | GLM/Kimi/Qwen 可能反向 | 那些模型偏差未知 | 只声称 DeepSeek 系；M2.5 逐家标定 |

**行为变化面（审计 F4，全链实测登记）**——`estimate_tokens_aware` 有 **4 个文件 21 处调用**，估算升高后各面的方向：

| 消费方 | 位置 | 变化方向 |
|---|---|---|
| 会话估算/压缩闸门 | `state.rs:28,54-56,70,77,85,87,92,95,103` | 用量**升高** → 压缩**更早**（修正目标） |
| `/context` 明细 | `inspect/mod.rs:21,514` | 显示**升高**（更接近真值） |
| 载荷计量 / preflight 溢出 | `payload_accounting.rs:505,509,608,613,851,1185,1200` | 计量**升高**；preflight 判定**趋严**（更早报溢出，方向安全） |
| ACP tokens 上报 | `acp_types.rs:421,432` | 上报值**升高** |
| 子代理两段式预算 / recap 预算 | `two_pass.rs:130`、`session_recap.rs:136` | 预算**收紧**（派发与摘要更保守） |

---

## 6. 验收标准（全部可脚本判定）

1. `cargo test -p cf-token-estimation` 全绿（含新夹具测试）。
2. **夹具聚合比 ∈ [0.96, 1.02]**，MAPE ≤ 17%（实测期望 **0.9811 / 16.0%**；容差按"实现值 ±0.02"给，不用 grid.py 的 0.984）。
3. `cargo test -p cf-chat-state --lib` 全绿（含 §3.2 全部更新）。
4. **`cargo test -p cf-shell --lib inspect::` 全绿**（覆盖 `list_instructions_approx_tokens_is_cjk_aware`；按 AGENTS.md §12 的跳过清单跑法）。
5. `cargo check --workspace` 0 error。
6. 离线复算：用**与实现完全相同的整数算术**对 `target/tmp/token-calib/corpus_strat2.jsonl` 重算，聚合比须为 **0.9811 ± 0.005**（脚本：`impl_ratio.py`，已随本方案提供命令）。
7. 出 exe 后的观感验收：同一会话 `/context` 显示的百分比**上升**；**压缩触发点提前**（从约 89% 回到约 85%）——**方向是"提前"，不是"后移"**（审计 F1：原文写反）。

---

## 7. 风险与回滚

| 风险 | 说明 | 缓解 |
|---|---|---|
| 压缩更早触发 | 估算变准后同量内容显示更高 → 可能多一次摘要调用 | 修正方向；发布说明写清；观察一周 token 成本 |
| 连带测试面（实测） | `cf-chat-state` 9 个断言 + `cf-shell` 1 个断言 + `lib.rs` 5 个函数 | §3 已给逐个新值；§6-3/6-4 覆盖三个 crate |
| 用户观感"突然用得多了" | 显示口径变化 | 帮助文案注明"按 DeepSeek 词表标定" |
| preflight 更早报溢出 | 计量趋严 | 属安全方向；若误报，先查 `payload_accounting` 的 double-count |
| **回滚** | 纯常数 + 测试 + 新增夹具 | `git revert` 单提交即可；无数据迁移、无配置、无外部资产（`EstimatedItemTokenCounter` 是派生值，不持久化旧口径） |

---

## 8. 工时与顺序

| 步 | 内容 | 工时 |
|---|---|---|
| 1 | C1/C2（常量 + 5 处注释） | 1.5 h |
| 2 | C3 + C7（lib.rs 5 个测试 + chat-state 9 个断言） | 2 h |
| 3 | C6（cf-shell inspect 断言） | 0.5 h |
| 4 | C4/C5（夹具生成 + 测试） | 2 h |
| 5 | 验收 §6 全跑 | 1 h |
| 合计 | | **0.5 天** |

---

## 9. 交付物

- 1 个 commit：`fix(token): 按 DeepSeek V4.1 词表标定估算常数`
- 变更：`lib.rs`、`tests/calibration.rs`、`tests/data/calibration_stats.json`、`cf-chat-state/.../tests.rs`、`cf-shell/.../inspect/mod.rs`、两处注释文件
- 发布说明一段（§7）

---

## 10. 上游勘误与有意偏离（审计 F7）

1. **勘误 `tokenizer-calibration-and-plan.md` §4.3-1**：该处写 `ceil(ascii_bytes*10/34)`，与其自身标定脚本 `grid.py:59` 的 `a // bpt`（floor）矛盾——系上游笔误。**本方案按 floor 执行**（与标定一致），并同步勘误上游文档。
2. **有意偏离 `calibration` §5-3**（"estimate_chars 同期处置"）：本单推迟到 M1c。理由：改动会把 web_fetch 抓取预算整体缩小 15%，属**独立可观测的行为变更**，应与本单的"常数校正"分开验收，避免一次改动同时动两处语义。

---

## 11. 审计记录（codely-core / GLM-5.3，2026-10-21）

**判定：REJECT（返工后可直接派工）**。7 项返工与处置：

| # | 审计发现 | 处置 |
|---|---|---|
| 1 | §6 验收 6 方向写反（"后移" vs 实际"提前"） | 已改 §6-7 |
| 2 | §6-5 拿 grid.py 的 0.984 当验收线，但两公式的 `other` 分母（3.4 vs 4）与 ceil 实现不同，按字面必失败且诱发调参凑数 | 已改为**实现公式复算 0.9811**，并新增 §2 口径差异说明 |
| 3 | 影响面遗漏 `cf-shell`（`inspect` 断言必挂），验收只覆盖两个 crate | 已加 C6 与 §6-4 |
| 4 | `estimate_tokens_aware` 消费方登记不全（实测 4 文件 21 处） | 已加 §5 行为变化面表 |
| 5 | M1b "默认随本单"与 §9 "可选同单"矛盾 | 已明确**不在本单** |
| 6 | 注释范围不足（漏 `lib.rs:66-81`、`state.rs:20`、`payload_accounting.rs:597`） | 已扩到 5 处 |
| 7 | 未声明与上游 §4.3-1（ceil）与 §5-3（同期处置）的分歧 | 已加 §10 |

**核对通过的项**：8 个测试期望值手算全对、floor 与 grid.py 一致、`BYTES_PER_TOKEN` 不动的决策正确、回滚设计成立、§0 全部数字与上游文档同源。
