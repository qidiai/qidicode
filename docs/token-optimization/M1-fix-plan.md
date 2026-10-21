# M1 修复方案：token 估算常数校正（v5，2026-10-21 四轮审计返工后）

> **状态**：**已定稿，未实施**。2026-10-21 用户决定：不单独编译，随下次改动一起出包；不新建分支（直接在当前分支提交）。缺陷记录见 `docs/fix-plans/2026-10-21-token-estimate-underreport.md`。
>
> **依据**：`docs/token-optimization/tokenizer-calibration-and-plan.md` v2（分层语料 40 会话 / 6,376 项 / 3.91M 真值 token，聚合比 0.819）
> **审计**：codely-core（GLM-5.3）**四轮**——v1 REJECT（7 项）、v2 REJECT（5 项）、v3 REJECT（1 项）、v4 REJECT（断言清单仍漏 ≥7 处，且**按符号名 grep 收口本身不完备**）。四轮均确认**核心设计、常数、公式、夹具、回滚无需改动**。本 v5 除补全清单外，**改了方法论**：把"穷举断言"降级为"已知清单 + 机械重算规则"，因为零符号测试无法被任何 grep 穷举（见 §3.0）。
> **目标**：聚合偏差 −18.1% → ±2%；**不改变** `BYTES_PER_TOKEN` / `estimate_tokens` / `estimate_chars` 的既有语义。
> **非目标**：逐项精度（MAPE 16%→0 需 M2）；多厂商词表（M2.5）；prompt 渲染计数。

---

## 0. 一句话

只改 `estimate_tokens_aware` 的两个比率：**ASCII bytes÷4 → 10/34（floor）**、**CJK 0.6 → 4/5（ceil）**，`other` 保持 `/4`。配套：**7 处生产注释 + 13 处测试内注释/消息**、**已定位 28 处测试断言（跨 5 个文件）+ 一条机械重算规则兜住剩余**、**1 个脱敏夹具**。

---

## 3.0 为什么给"规则"而不只是"清单"

四轮审计证明：**按符号名 grep 永远穷举不完**——有些测试正文里一个估算函数名都没有（`ChatState::new` 直接算、或纯字面量断言），任何名字模式都扫不到，却在新常数下必挂。

因此本方案的执行协议是**双闸**：

- **闸 1（已知清单）**：§3.1–§3.4 逐条给出行号与新值——24 处已实证，照改即可。
- **闸 2（机械规则 + 全量跑绿）**：`cargo test` 任何红灯，**必须**用下列规则重算并**在 commit/测试里写出推导**，禁止"试数到绿"：
  - ASCII：`floor(ascii_chars × 10 / 34)`
  - CJK：`ceil(cjk_chars × 4 / 5)`，即 `(c×4+4)/5`
  - other：`other_bytes / 4`（不变）
  - 逐段求和：每段独立取整后相加（**不合并**）
  - reseed：`round(base_estimate × pre_replace_total / estimate_at_last_response)`（f64 `.round()`，已由 `mutations.rs:665-668` 实证），并受 `min(pre_replace_total)` 封顶
  - 若某断言**无法**用上述规则推出新值（例如它断言的是"与 bytes/4 精确耦合的构造"），必须**改构造**（按新速率反推输入长度），而不是改期望值

**验收以 §6 的全量跑绿为准**；§3 清单是"应该改这些"，不是"只改这些"。

**上线预期（实现公式实测）**：分层语料 **0.9811**（−1.89%），双尾 0.9312（−6.88%），夹具（194 条）**0.9841 / MAPE 16.3%**。

---

## 1. 改动清单（文件级）

> **路径约定**：下文文件名均相对 `crates/codegen/`（如 `cf-chat-state/src/actor/state.rs` 实为 `crates/codegen/cf-chat-state/src/actor/state.rs`）。

| # | 文件 | 改动 | 风险 |
|---|---|---|---|
| C1 | `cf-token-estimation/src/lib.rs` | 新增 `ASCII_TOKEN_NUMERATOR=10` / `ASCII_TOKEN_DENOMINATOR=34`；`CJK_TOKEN_NUMERATOR` 3→4；ASCII 分支改有理数 floor，CJK 仍 ceil，`other` 仍 `/BYTES_PER_TOKEN` | 低 |
| C2 | 5 个文件 **7 处生产注释 + 13 处测试内注释/消息** | `lib.rs:24-30`、`:59-65`、`:66-81`、`state.rs:20`、**`state.rs:60-61`**、`payload_accounting.rs:597`、**`inspect/mod.rs:1970-1976`**；测试内：`lib.rs` ~:269/:272/:284、`tests.rs` :316/:355/:375/:470/:538、`state.rs:411-412/:498/:535/:542`、`payload_accounting.rs:17` | 无 |
| C3 | `lib.rs` 测试区 | 5 个测试函数更新 + 1 个新增锁定测试（§3.1） | 无 |
| C4 | `cf-chat-state/src/actor/state.rs`（**内嵌 `#[cfg(test)] mod tests`**） | **10 处断言** + 1 个测试改名（§3.2 A） | 低 |
| C5 | `cf-chat-state/src/actor/tests.rs` | **11 处断言**（§3.2 B；四轮审计逐步补齐） | 低 |
| C6 | `cf-shell/src/inspect/mod.rs:1978-2003` | 注释算式 + 断言方向翻转（§3.3） | 低 |
| C9 | `cf-shell/src/session/helpers/session_recap.rs:804` | **改构造**（filler 按新速率反推），非改期望值（§3.4） | 低 |
| C7 | `cf-token-estimation/tests/calibration.rs` + `tests/data/calibration_stats.json` | 校准回归（已试生成：194 条 / 15.2KB / 0.9841 / 16.3%） | 低 |
| C8 | `cf-token-estimation/Cargo.toml` | **`[dev-dependencies] serde_json`**（C7 解析夹具所需；该 crate 现无此依赖） | 无 |

**明确不做**：

- `BYTES_PER_TOKEN`（=4）**不动**。直接消费：`state.rs:39`、`state.rs:104`、`payload_accounting.rs:598`、`:603`；经 `truncate.rs:190-197` 包装的间接消费：`read_file/mod.rs:477`、`web_fetch/overflow.rs:174`。
- `estimate_tokens()`、`estimate_chars()` **不动**（§5）。
- **M1b（`state.rs:39` 切 aware）拆为独立小单**。

---

## 2. 精确实现

```rust
/// ASCII 速率：10/34 ≈ 0.294 token/byte，即 3.4 chars/token。
pub const ASCII_TOKEN_NUMERATOR: u64 = 10;
pub const ASCII_TOKEN_DENOMINATOR: u64 = 34;

/// CJK 速率：4/5 = 0.8 token/字。
pub const CJK_TOKEN_NUMERATOR: u64 = 4;
pub const CJK_TOKEN_DENOMINATOR: u64 = 5;

pub fn estimate_tokens_aware(s: &str) -> u64 {
    let ascii_tokens = ascii_len * ASCII_TOKEN_NUMERATOR / ASCII_TOKEN_DENOMINATOR; // floor
    let cjk_tokens   = (cjk_chars * CJK_TOKEN_NUMERATOR + (CJK_TOKEN_DENOMINATOR - 1))
                       / CJK_TOKEN_DENOMINATOR;                                    // ceil
    ascii_tokens + cjk_tokens + other_bytes / BYTES_PER_TOKEN                      // other 仍 /4
}
```

**与标定脚本的口径差（必须知道）**：`grid.py:59` 当年是 `a // 3.4 + float_ceil(c*0.8) + o // 3.4`——**other 桶也除了 3.4**，CJK 用浮点 ceil。本实现 `other/4`、整数 ceil，故实现值系统性略低于 grid 值（实测 0.984 → 0.9811，差 0.29pp）。**验收一律用实现公式复算，不用 0.984。**

---

## 3. 测试改动清单（逐条已手算）

### 3.1 `cf-token-estimation/src/lib.rs`（5 改 + 1 新）

| 测试函数 | 输入 | 旧 | **新** |
|---|---|---|---|
| `..._matches_bytes_over_four_for_ascii` → 改名 `..._ascii_uses_calibrated_rate` | `"hello world"` / `"x"*4000` | 2 / 1000 | **3 / 1176** |
| `..._chinese_sits_in_measured_band` → 改名 `..._cjk_uses_calibrated_rate` | `"你"*100` | 60 | **80**（区间 `78..=82`；`t < estimate_tokens(&s)` **翻转为** `t > estimate_tokens(&s)`，因为 80 > 75） |
| `..._mixed_ascii_and_cjk` | `"你好世界hello"` / `"ab你"` | 4 / 1 | **5 / 1** |
| `..._counts_fullwidth_punctuation_as_cjk` | `"，。！"` | 2 | **3** |
| `cjk_ratio_constants_expose_the_tunable` | — | 3/5 | **4/5 + 两个 ASCII 常量** |
| （新增）`calibrated_constants_pin_the_corpus_fit` | — | — | 锁 10/34、4/5，断言消息带实际值 |

不受影响（已手算）：`empty`、`emoji`(1/2)、`cjk_block_boundaries`(2/0/1/1)、`punct_via_han_arm`(2)。

### 3.2 A. `cf-chat-state/src/actor/state.rs`（内嵌测试模块，:340 起；**10 处**）

| 行 | 断言对象 | 旧 | **新** |
|---|---|---|---|
| `:370` | 100 CJK system | 60 | **80** |
| `:375` | `"你好世界hello"` user | 4 | **5** |
| `:379` | 同上 assistant | 4 | **5** |
| `:389` | `{"路径":"你好"}`（7 ASCII + 4 CJK） | 4 | **6**（70/34=2 + ceil(16/5)=4） |
| `:393` | `"你好世界hello"` tool_result | 4 | **5** |
| `:406` | `multi`（两段各 2 CJK） | 4 | **4 不变**，注释 `ceil(6/5)` 须改 `ceil(8/5)` |
| `:410` | `..._ascii_matches_legacy_bytes_over_four` | 名与语义失效 | **改名** `..._ascii_uses_calibrated_rate` |
| `:419-421` | `[27,28,23,13]` 纯 ASCII | `[6,7,5,3]` | **`[7,8,6,3]`** |
| `:453` / `:470` | multisegment `ab/cd/efgh` | 1 / 0 | **不变**（注释算式过时） |
| **`:506`** | `new_state_estimates_tokens_from_conversation`：4×`"x"*4000` 的 `ChatState::new` | `4_000` | **`4_704`**（4×1,176）；`:498` 注释"bytes/4=1000 each"同步改 |
| **`:512`** | `estimate_system_message_tokens(...)`，`"a"*400` | `100` | **`117`**（4,000/34） |
| **`:543` / `:544`** | `estimate_messages_tokens...`（4 项 / 3 项） | `4000` / `3000` | **`4_704`** / **`3_528`**；`:535/:542` 注释同步 |

### 3.2 B. `cf-chat-state/src/actor/tests.rs`（**11 处**；四轮审计逐步补齐）

| 行 | 场景 | 旧 | **新** |
|---|---|---|---|
| `:321` | 4000 字符 tool_result 增量 | `101_000` | **`101_176`**（40000/34=1176） |
| `:335` | 同上 + model response | `101_000` | **`101_176`** |
| `:357` | 4,000,027 ASCII（4MB 后台通知） | 区间 `1_099_000..=1_101_000` | **`1_276_478`**；区间改 `1_276_000..=1_277_000` |
| `:380` | `"u"*4000` user 推送 | 区间 `101_000..=101_010` | **`101_176`**；区间改 `101_100..=101_250` |
| `:420` | 同上 rewind 后 | `101_000` | **`101_176`** |
| **`:500`** | `compaction_reseed_...`：`"x"*160_000` 的 `estimate_conversation_tokens` | `40_000` | **`47_058`**（1,600,000/34） |
| **`:506`** | 压缩后 `"z"*12_000` | `3_000` | **`3_529`**（120,000/34） |
| **`:510`** | ratio-scaled reseed 结果 | `6_525` | **`6_524`**（`mutations.rs:665-668` 实证为 f64 `.round()`：3,529×87,000/47,058=6,524.35 → 6,524，**确定值**） |
| **`:512`** | `additive_model = compacted + (provider − at_response)` | `50_000` | **`43_471`**（3,529+39,942） |
| **`≈:657`** | `compaction_reseed_without_provider_count_matches_plain_estimate`：`"w"*8_000` | `2_000` | **`2_352`**（80,000/34） |
| **`≈:674`** | `non_compaction_replace_does_not_carry_overhead`：`"q"*4_000` | `1_000` | **`1_176`** |

> **为什么连 grep 都靠不住（四轮审计的教训）**：`:657`/`:674` 这类测试正文里**一个估算符号都没有**（直接 `ChatState::new` 或纯字面量断言），任何名字模式都扫不到；`:512` 走的是 `estimate_system_message_tokens`，也不在原四符号里。**所以：名字 grep 只能"尽量多捞"，真正的完备闸门是 §6 的全量跑绿 + §3.0 的机械重算规则。**
>
> 建议的捞取命令（补充用，非完备）：
> `git grep -nE "estimat|101_000|1_099_000|4_000" -- crates/codegen/cf-chat-state crates/codegen/cf-shell`
>
> **已验证不受影响、编码手勿改**：`:484/:487` 的 `51_000`（compacted==at_response，线性缩放不变）、`:536` 的 `5_500`（2,000/4,000 与 588/1,176 同为 0.5 比例，精确不动）、`:404` 的 `100_000`（no-bump 语义与内容大小无关）、`:545`（用 `.round()` 现场算，速率自消）。

### 3.4 `cf-shell/src/session/helpers/session_recap.rs`（**1 处**）

`:804`（`budget_threshold_boundary_selects_fast_vs_over_budget`）：断言 `estimate_conversation_tokens(&at) == snapshot_budget`，而 filler 是按 `"a".repeat(filler_tokens * 4)` 构造的——**与 bytes/4 精确耦合，M1 后必挂**。
**修法不是改期望值，而是改构造**（§3.0 规则）：`filler_chars = ceil((snapshot_budget − reasoning_tokens) × 34 / 10)`，**且 `:816` 的 over 用例必须同步反推**（它复用同一 `filler_tokens×4` 构造；只改 `:802` 会让边界语义被稀释）。数学上该反推**精确成立**：`floor(10·ceil(34X/10)/34) = X` 零残差，`over = N+4` 时 `floor ≥ X+1`，`:818` 的 `>` 仍严格成立（五轮审计已证，无需实跑试数）。

### 3.3 `cf-shell/src/inspect/mod.rs`

- `:1978-1979` 注释：`ceil(200*3/5)=120` → `ceil(200*4/5)=160`；
- `:2000-2003` 断言 `approx_tokens < estimate_tokens(&zh)`（160 < 150 为假）→ **翻转为 `>`**，消息改"aware 现高于旧 bytes/4 低估"。

---

## 4. 校准回归夹具（C7）

只存**脱敏计数**，不含原文（公开仓库隐私）：

```json
{"source":"40 sessions, size-stratified; DeepSeek V4.1 tokenizer (deepseek-recipe@935817c, MIT)",
 "generated_by":"target/tmp/token-calib/make_fixture.py",
 "note":"counts only: ascii/CJK chars, other bytes, exact=official tokenizer count",
 "items":[{"kind":"tool_result","size":"large","ascii":8123,"cjk":412,"other":88,"exact":3120}]}
```

- **分层**：`kind`(5) × 会话规模三分位(3) = 15 组，每组 ≥8 条；**已试生成 194 条 / 15.2KB**，种子固定（`random.Random(20261021)`）。
- **测试必须真正调到实现**（三轮审计 F-4）：从计数**重建字符串**（`"a"*ascii`、`"你"*cjk`、按 4 字节 emoji 拼 `other/4` 个），**调用 `estimate_tokens_aware`** 得到 h；期望值另用测试内独立手写的算术对 `exact` 求聚合比。`estimate_tokens_aware` 是 (ascii, cjk, other) 的纯函数，重建串与原串必然同值 → 既约束实现、又不是自舔测试。
- **§3.1 的 5 个单测钉取整方向**作为第二道锁。
- 生成脚本不入库；JSON 保留溯源字段。

---

## 5. 已知偏差与行为变化面

| 项 | 现状 | 影响 | 处置 |
|---|---|---|---|
| `estimate_chars ×4` | 与新速率 3.4 差 +17.6% | `web_fetch/overflow.rs:174` 字符预算偏大 → 溢出方向 | **本单不动**；**M1c（0.25 天）**单独做 |
| `state.rs:39` 工具定义 bytes/4 | 低估约 15% | `/context` tools 行偏低 | **M1b 独立小单** |
| `estimate_tokens()` bytes/4 | `read_file` 截断预算 | 阈值偏松 | 与 M1c 一并评估 |
| 逐项 MAPE 16% | 两常数混排互补 | 单条可偏 20%+ | 对外只声称"聚合层"；根治靠 M2 |
| 非 DeepSeek 词表 | GLM/Kimi/Qwen 未标定 | 方向未知 | 只声称 DeepSeek 系 |

**直接消费方（21 处，`git grep estimate_tokens_aware` 实测）**：

| 文件 | 行 | 方向 |
|---|---|---|
| `cf-chat-state/src/actor/state.rs` | 28,54,55,56,70,77,85,87,92,95,103（**11 处**） | 用量↑ → 压缩更早 |
| `cf-shell/src/inspect/mod.rs` | 514（+import :21） | `/context` 显示↑ |
| `cf-shell/.../payload_accounting.rs` | 505,509,608,613,851,1185,1200（**7 处**） | 计量↑、preflight 判**趋严** |
| `cf-shell/src/session/acp_types.rs` | 421,432（**2 处**） | ACP 上报↑ |

**间接消费方（经 `estimate_item_tokens` / `get_estimated_total_tokens`，本单同样受影响）**：
`mutations.rs:137,172`（`estimated_tokens_since_model` 累加——正是 preflight 的数据源）、`compaction_utils.rs:132,141,145,185`、`cf-shell/session/compaction.rs:307`、`payload_accounting.rs:515,521,538,547,561,1186`、`session_recap.rs:136,792,796`、`two_pass.rs:130`。方向全部同向（估算↑、判定趋严）。

---

## 6. 验收标准（全部可脚本判定）

1. `cargo test -p cf-token-estimation` 全绿（含夹具测试）。
2. **夹具聚合比 ∈ [0.97, 1.01]，MAPE ≤ 18%**（实测 **0.9841 / 16.3%**）。上界 1.01 与 §0「±2%」自洽，下界 0.97 留采样余量。
3. `cargo test -p cf-chat-state --lib` 全绿（含 §3.2 A **10 处** + B **11 处** = ≥21 处；若还有清单外红灯，按 §3.0 规则重算）。
4. `cargo test -p cf-shell --lib` 全绿（按 AGENTS.md §12 现行口径：**跳过 17 项**、`CARGO_PROFILE_TEST_DEBUG='0'` + C 盘热 target，基线 5630 passed / 0 failed / 7 ignored；含 §3.3 inspect 1 处 + §3.4 session_recap 1 处）。
5. `cargo check --workspace` 0 error。
6. 离线复算（**仅标定机可跑**，语料不入库）：`python target/tmp/token-calib/impl_ratio.py corpus_strat2.jsonl tokenizer_v41.json` → **0.9811 ± 0.005**。
7. 出 exe 后：同一会话 `/context` 百分比**上升**；压缩触发点**提前**（约 89% → 约 85%）。**方向是"提前"**。

> **闸 2 的范围声明（五轮审计建议）**：上述门只覆盖 3 个受影响 crate（cf-token-estimation / cf-chat-state / cf-shell）。**cf-pager 等其它 crate 与集成测试不在门内**（4+1 轮审计与 §5 消费方地图均未发现那里钉了估算值，但未穷举）。派工前加一条廉价兜底：
> `git grep -nE "estimate_tokens_aware|estimate_item_tokens|get_estimated_total_tokens|101_000|40_000|4_000" -- crates/codegen/cf-pager crates/codegen/cf-tools`（红线才处理，无命中即记录"已扫无命中"）。

---

## 7. 风险与回滚

| 风险 | 说明 | 缓解 |
|---|---|---|
| 压缩更早触发 | 估算变准 → 同量内容显示更高 → 可能多一次摘要 | 修正方向；发布说明；观察一周成本 |
| 连带断言（**已定位 28 处 / 5 文件，剩余靠全量跑绿 + 规则兜底**） | `lib.rs` 5(+1 新增) + `state.rs` 10 + `tests.rs` 11 + `inspect` 1 + `session_recap` 1 | §3 已给逐个新值；§3.0 双闸；§6-3/6-4 三 crate 覆盖 |
| 观感"用得变快了" | 显示口径变化 | 帮助文案注明 |
| preflight 更早报溢出 | 计量趋严 | 安全方向；误报先查 double-count |
| **回滚** | 纯常数 + 测试 + 夹具 | `git revert` 单提交；**已核实无持久化旧口径**（`estimated_tokens_since_model`/`total_tokens` 皆为内存派生，`ChatState::new` 从原文重算） |

---

## 8. 工时与顺序

| 步 | 内容 | 工时 |
|---|---|---|
| 1 | C1/C2（常量 + 7 处生产注释） | 2 h |
| 2 | C3 + C4 + C5（lib 5 + state 10 + tests 11 = 26 处） | 4 h |
| 3 | C6 + C9（inspect 断言 + session_recap 构造反推） | 1 h |
| 4 | C7 + C8（夹具生成 + 测试 + dev-dep） | 2.5 h |
| 5 | 验收 §6 全跑（含 cf-shell 全量）+ 清单外红灯按 §3.0 规则补 | 2.5 h |
| 合计 | | **≈1.5 天（12 h）** |

---

## 9. 交付物

1 个 commit：`fix(token): 按 DeepSeek V4.1 词表标定估算常数`
变更：`lib.rs`、`tests/calibration.rs`、`tests/data/calibration_stats.json`、**`Cargo.toml`（dev-dep serde_json）**、`cf-chat-state/{state.rs,tests.rs}`、`cf-shell/inspect/mod.rs`、`payload_accounting.rs`（注释）+ 发布说明。

---

## 10. 上游勘误与有意偏离

1. `tokenizer-calibration-and-plan.md` §4.3-1 原写 `ceil(ascii…)`，与 `grid.py:59` 的 floor 矛盾——**已勘误**（该文件 §4.3 现有 R3 勘误块，并注明验收以实现公式 0.9811 为准）。
2. **有意偏离** `calibration` §5-3（"estimate_chars 同期处置"）：推迟到 M1c。理由：改动使 web_fetch 抓取预算整体缩小 15%，属独立可观测行为变更，应与常数校正分开验收。

---

## 11. 审计记录（codely-core / GLM-5.3）

### 第 1 轮（v1）→ REJECT，7 项

验收方向词写反、验收线沿用 grid.py 的 0.984（口径不同）、漏 cf-shell 验收、消费方登记不全、M1b 归属矛盾、注释清单不足、未声明与上游偏离。→ v2 全部修正。

### 第 2 轮（v2）→ REJECT，5 项

| # | 发现 | 处置 |
|---|---|---|
| 1 | **`actor/tests.rs` 另有 5 处断言必挂**（4000 字符→1176，`101_000` 变 `101_176`；4MB 场景变 `1_276_478`，区间断言越界） | 已加 §3.2 B 精确清单 |
| 2 | **§3.2 表头文件写错**：370/375/379/389/393 实为 `state.rs` 内嵌测试，非 `tests.rs` | 已拆成 A（state.rs）/ B（tests.rs）两表 |
| 3 | 注释清单仍漏 `state.rs:60-61`、`inspect:1970-1976` | 已扩到 **7 处** |
| 4 | `state.rs:406` multi=4 值不变但注释过时 | 已单列 |
| 5 | 工时 0.5 天与分项和矛盾；C4 缺 `serde_json` dev-dep；§6-4 过滤器过窄；夹具门 [0.95,1.02] 宽于 §0 ±2% | 已改 **≈1 天**、补 C8、改全量 `--lib`、夹具门收紧到 **[0.97,1.01]** |

**两轮核对通过的项**：8+5 个测试期望值手算全对、常数溯源（n=1211 / 3.10 / 0.569）、floor 与 grid.py 一致、`BYTES_PER_TOKEN` 不动、回滚成立（无持久化旧口径）、夹具 194/15.2KB/0.9841/16.3% 独立复算精确命中。

### 第 3 轮（v3）→ REJECT，1 项（已修）

| # | 发现 | 处置 |
|---|---|---|
| 1 | **`tests.rs:500/506/510/512` 另有 4 处必挂**（`compaction_reseed_...`，走 `estimate_conversation_tokens`，v3 的 grep 模式扫不到）：47,058 / 3,529 / 6,524 / 43,471 | 已补 §3.2 B 四行 + 收口 grep 改为四个符号名、弃 `*/tests*` glob |

同轮 nits 一并处理：断言计数改 **21 处 / 4 文件**（F-2）、§6-4 跳过清单改 **17 项**（F-5）、夹具测试改为"重建串 + 调实现"（F-4）、测试内注释与断言消息的陈旧清单（F-3）随 §3 一并改。

### 第 4 轮（v4）→ REJECT，方法论级（已修）

| # | 发现 | 处置 |
|---|---|---|
| 1 | 上轮 4 行数值全对，`:510` 取整由 `mutations.rs:665-668` 实证为 **f64 `.round()`** → 6,524 确定 | 已删"若向上取整"的对冲句 |
| 2 | **仍漏 ≥7 处必挂**：`state.rs:506(4_704)/:512(117)/:543(4_704)/:544(3_528)`、`tests.rs≈:657(2_352)/≈:674(1_176)`、`session_recap.rs:804`（改构造） | 已全部补入 §3.2 A/B 与 §3.4 |
| 3 | **根本问题：符号名 grep 无法穷举**——零符号测试（`ChatState::new` 直接算、纯字面量断言）任何模式都扫不到 | **改方法论**：新增 §3.0「已知清单 + 机械重算规则 + 全量跑绿」双闸；明确"清单不是全集" |

计数最终口径：**已定位 28 处 / 5 文件**（lib 5 + state 10 + tests 11 + inspect 1 + session_recap 1），其余靠 §3.0 闸 2。

### 第 5 轮（v5）→ **ACCEPT WITH NITS**（定稿通过）

| # | 结论 | 处置 |
|---|---|---|
| 1 | §3.0 双闸成立：漏改从"审计期不可知"降级为"执行期必然可见的红灯" | 保留 |
| 2 | 6 条新数值独立手算**全部命中**；两条 `≈` 的确定行为 `:657` / `:674` | 已将 `≈` 坐实 |
| 3 | §3.4 反推公式被数学证明**精确成立**（`floor(10·ceil(34X/10)/34)=X`），强于方案自述 | 已改为"无需实跑试数" |
| 4 | nit：C2 计数 10 vs 枚举 13 | 已改 **13** |
| 5 | nit：表格路径缺 `crates/codegen/` 前缀 | 已加路径约定 |
| 6 | nit：`:816` 须与 `:802` 同步反推 | 已补 |
| 7 | 建议：闸 2 范围只覆盖 3 crate | 已加 §6 范围声明 + 兜底 grep |

**五轮合计**：核心设计（常数 10/34 + 4/5、floor/ceil 口径、`other/4`、`BYTES_PER_TOKEN` 不动、回滚）**自始至终未被要求修改**；被打回的 18 项全部集中在"断言清单 / 验收口径 / 计数"。
