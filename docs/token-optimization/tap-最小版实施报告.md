> ⚠️ **R10 终态声明（2026-10-18，R10.5 收尾补记——阅读本报告前必读）**
>
> **本报告描述的是 R10 前的 tap A 形态（collect_prompt_tap/ChatStateEvent::PromptTap/393 passed），已被 R10 裁定 (a) 退役——终态为 tap B（payload_accounting.rs 1363 行，14 条 payload 测试+CI run #114 6/6 背书）——阅读本报告请以终态为准。**
>
> R10 变更面（供考古）：
> - **tap A 退役清单**：`collect_prompt_tap`（单遍 walk 采集段）、`system_and_memory_tokens`/`user_text_tokens`（采集段辅助函数）、`ChatStateEvent::PromptTap` 变体+`PromptTapSample`（15 字段）+`sections_sum()`（events.rs）、run_loop.rs 的 PromptTap 消费臂（`shell.prompt_tap` 日志）、tap A 专属测试 6 条。tap A 从未进提交史（仅工作树+patch 形态，见 §10 冲突实录），退役后工作树零残留；唯一残存件 `tap-最小版.patch` 已于 R10.5 销毁（防复活）。**保留件**：`estimate_tool_spec_tokens`（state.rs:52）、P2-6 aware 口径（acp_types.rs skills/mcp 两行）、`serialized_json_bytes`/`conversation_body_bytes`（转 pub）——均被 tap B 复用为共享生产口径，不在退役面。
> - **B2**：history 分区行补 TR 内联图 token（`estimate_image_tokens(tr.images.len())`，payload_accounting.rs ToolResult 臂）——修复对账分子系统性 −765×N 假阳性；回归测试 `tool_result_images_count_into_history_partition_tokens`。
> - **B3**：回退项（R10 复审裁定；实质内容见 R10 复审材料，本报告落盘时编码手无该材料访问面）。
> - 本报告全文保留供考古；文中行号/事实以 R10 终态（tap B）为准。

# tap 最小版实施报告(⑤′ 前置测量件,2026-10-17)

> 编码手实施报告。锚点依据:指挥官转达的 step-5 终审锚点更新(7 条)+《推进方案 v2.1 双模审计合成》+《PhaseC-R7R8 落点复核-step5》。所有行号以本报告落盘时工作树现锚为准(实测纠正了任务书与审计材料的行号漂移,见 §5)。

---

## 1. 交付范围(最小版,P1-1 裁决形态)

**唯一 choke point**:`build_conversation_request`(`crates/codegen/cf-chat-state/src/actor/request_builder.rs:135`)。tap 全部落在此函数内部——`ByteCounter`(447)与 `serialized_json_bytes`(463)为私有,只有此位置复用成立(cf-shell 侧不植入、不导出,锚点 2 钉死)。

**文件清单**:

| 文件 | 变更 |
|---|---|
| `cf-chat-state/src/actor/request_builder.rs` | tap 采集段:`collect_prompt_tap`(单遍 walk)+ `system_and_memory_tokens`(剥块防双算)+ `user_text_tokens`;`build_conversation_request` Step 4 前挂 `send_event(ChatStateEvent::PromptTap)`;测试 6 条 |
| `cf-chat-state/src/events.rs` | `PromptTapSample` 结构(15 字段)+ `ChatStateEvent::PromptTap` 变体 + `sections_sum()` 对账恒等式 |
| `cf-chat-state/src/actor/state.rs` | `estimate_tool_definition_tokens` 口径换 aware(P2-6);新增 `estimate_tool_spec_tokens`(choke point 收 `Vec<ToolSpec>` 平铺线) |
| `cf-shell/src/session/acp_types.rs` | 419/429 两行(skills/MCP)`estimate_tokens` → `estimate_tokens_aware`(P2-6) |
| `cf-shell/src/session/acp_session_impl/run_loop.rs` | `PromptTap` 消费臂 → `unified_log::info("shell.prompt_tap", …)`(搭 ImageBudget 同一便车) |

## 2. Section 分类学实现(choke point 实测截面)

| 截面 | 实现 | 判定 |
|---|---|---|
| 工具 defs | `tools` 参数按 `estimate_tool_spec_tokens` 求和 | **提交时 effective 全量**——实测确认 cf-shell turn.rs:1841-1850 在 `build_request` **之前** push `StructuredOutput` 合成工具进 `effective_tools` → 按参数计即全量,C4 残差泄漏不存在(测试 `tap_golden_fixed_input_sections` 钉死含 StructuredOutput 的 11-token golden) |
| memory | 提交 items 内首个 System 的 `<memory-context>` 块(`system_and_memory_tokens` 剥出) | **单一真源裁定**:参数路径不另计(注入/持久化后块必然在 items)。与 R7R8 建议的"系统提示行排除块"一致,并把"入口按参数计"收敛为"注入后块计"——恒不双算,测试 `tap_memory_block_is_not_double_counted` 断言 `memory+system == aware(整串)` |
| AGENTS.md | User items 且 `synthetic_reason == ProjectInstructions` | 专用 item 变体,零嗅探 |
| 技能与 MCP | User items 且 `synthetic_reason == SystemReminder`(整桶一行) | snapshot 级子行(Skills/MCP servers)已有现成管道 `/context` `usage_categories`(session_setup.rs:669)——不重复建设;桶内细分留待全量版 |
| 系统提示(静态) | System items 剥 memory 块后 aware | — |
| **会话历史(合计)** | 五变体桶之和(P1-1 硬要求,Phase C 灰度槽位归因基础) | `history_user/assistant/tool_result/backend_tool_call/reasoning` |
| 会话历史分桶 | **第三零件**:与 `estimate_item_tokens`(state.rs:55)同一套变体分类学与估算原语,单遍 walk 分桶求和,未新写第二套遍历 | Reasoning 加密 blob 保持 bytes/4 特殊率(与既有口径一致) |
| 近 5 条 tool results | 尾随窗口 `TAP_RECENT_TOOL_RESULTS_WINDOW=5`,含于 TR 桶 | 测试钉 6 条→recent=尾 5 |
| attachments | User `ContentPart::Image` + **ToolResultItem.images**(实测发现:read_file 图片会内联进 TR,漏计即入残差) | 765/张 |
| 未归因残差 | choke point 内白名单归类完备 → 结构性恒 0;真残差在消费侧对账(`Σsections vs provider prompt_tokens`) | 残差告警语义按 R7R8 P1-3:告警对象=对账差额,残差行只监控增长率——消费侧实现留待全量版 |

## 3. 口径统一声明(锚点 5)

- **tap 对账一律 `estimate_tokens_aware`**(ascii/4 + ceil(CJK×3/5) + other/4,`cf-token-estimation/src/lib.rs:85`)。
- 三套估算收敛后:工具 defs 估算器内部换 aware(它是 **/context 域与 tap 的共同源**——session_setup.rs:621 同函数,一处改动两域统一);`/context` 两行(acp_types.rs:419/429)同步换 aware。
- golden 值变化属裁定内行为:19 字节整串 4 → 分段 1+2+0=3(分段截断),state.rs 测试期望同步修正并注释缘由。**注意**:per-field 分段求和与整串 bytes/4 有 ≤段数−1 token 的截断差——对账契约选"共享同一估算器"优先于绝对精度(注释已写入 `estimate_tool_definition_tokens`)。
- `estimate_tool_definitions_tokens` 实测无 tap 外新消费者(caliber_tests 与 dynamic_tools 测试均为同函数自比,口径切换不破绿)。

## 4. P1-2 前缀铁律执法(并入 tap 验收断言)

最小不变量集以测试形态执法(`request_builder.rs` test mod):

1. **纯函数性**:`tap_is_deterministic_across_calls_prefix_iron_law`——同一 (items, tools) 两次 collect 逐字段相等 ⇒ 时间戳/nonce/任何顺序抖动源一泄漏即红。
2. **对账恒等**:全部 tap 测试断言 `sections_sum() == total_tokens`(构造保证 + 测试钉死)。
3. **防双算**:memory 块剥离后 `memory + system == aware(整串)`。

既有 `build_conversation_request` 前缀稳定性测试(tests.rs:4504 起)继续独立覆盖请求本体。

## 5. 七处 aux 留白显式枚举(锚点 4,实测现锚)

**以下自建 LLM 调用全部绕过 `build_conversation_request`,tap 不覆盖,不能默认被主回路代表**(行号漂移实测纠正;recap 实测 **4 处**,较任务书多 1):

| # | 路径 | 现锚(实测) | 任务书锚 | 备注 |
|---|---|---|---|---|
| 1 | permission classifier | `sampler_turn.rs:585`(自建 ConversationRequest,603 发出) | 583 | 漂移 +2 |
| 2 | laziness classifier | `laziness.rs:544` | 577 | 漂移 −33 |
| 3 | memory_dream | `memory_dream.rs:355 / 465 / 731` | 347/407/707 | 3 处 |
| 4 | recap | `recap.rs:108 / 246 / 513 / 646` | 27/199/495 | **实测 4 处,646 为任务书外新发现** |
| 5 | reasoning_pregenerate | 触发线 `reasoning_pregenerate.rs:108`(≥35% 水位);实际 LLM 调用经 `reasoning_summarizer.rs:292` | 138 | 调用链已下沉 summarizer,锚随链迁移 |
| 6 | image-describe | `sampler_turn.rs:626` 一带 doc(630/659 现锚) | 626 | ✓ |
| 7 | 子代理 spawn 组装 | install_system_prompt+replace_conversation 直接装配,F2R 后基线技能清单注入在 spawn.rs:1011 | — | 非采样路径;子代理采样走同一 choke point(机械覆盖),见 §6 |

**战场候选(顺带发现,记入报告)**:permission classifier 在 auto 权限模式下**每次权限判定一次全价 LLM 调用**(sampler_turn.rs:585 一带),长会话累积可观——建议进入战场候选清单,等 tap 常驻数据给出占比证据后排期。

## 6. session-kind 标签(P1-4)——最小版留白声明

事件带 `conv_id` + `req_id`(每次 build 唯一),消费侧可据此区分会话;**真 `session_kind`(main/subagent)字段未进最小版**——`ChatStateActor` 无此信息,传入需跨 crate 改 spawn 链路。留白后果:子代理与主回路的 tap 数字在 unified log 中按 conv_id 关联,自动聚合分析需先 join 会话表。全量版补。

## 7. ②金丝雀状态(锚点 7)

**待跑**。金丝雀(工具 announce 路径漂移巡检)按 v2.1 排序前置于 tap 开表——本报告交付时未收到金丝雀已跑的通知,tap 数据在金丝雀跑完前**只入库不开表**。编码手侧不越权代跑(测量之前先验尺,验尺不是编码手的活)。

## 8. 开销与验收线自评

- **延迟**:`collect_prompt_tap` = 单遍 items walk + 一遍 tools 求和,纯字符类计数;每轮 `build_conversation_request` 本就在跑更贵的 `conversation_body_bytes`(JSON 转义下界扫描)——新增亚毫秒级,≪1% 验收线。
- **载荷**:tap 事件只走 `ChatStateEvent` 通道(unified log 本地落盘),**请求零新增字节**。
- **第二真源禁令**:tap 事件仅观测;不进 auto-compact 阈值、不进限额、不进计费(events.rs 契约注释钉死)。
- **采样策略**:最小版全量发(ImageBudget 先例同构);1% 采样/dev flag 留待全量版治理(v2.1 原案)。

## 9. 治理(M9 最小集)

- **golden 钉死**:固定输入→期望截面值测试 6 条(tap_golden_* / tap_memory_* / tap_recent_* / tap_deterministic_* / tap_classifies_*)。
- **新 section 准入规则**:新截面必须 ①有专用判定字段(嗅探例外需审计签字)②进 `PromptTapSample` + `sections_sum()` ③进 run_loop 日志行 ④补 golden——四件缺一不入。
- **残差语义**:choke point 内白名单归类完备(结构性 0);消费侧对账差额告警 + 残差增长率监控为全量版交付项。

## 10. 工作树并发冲突实录(⚠️ 指挥官必读,2026-10-18 02:26-02:51)

**实测事件**:本实施期间,另一编码手的 payload accounting 工作(payload_accounting.rs,untracked 新文件+acp_session.rs/session_setup.rs/turn.rs/session_compact.rs 的修改)在同一工作树活跃进行。**02:45-02:50 窗口内,tap 已完成并验证过的 3 个文件(events.rs/request_builder.rs/run_loop.rs)被对方 lost-update 式写回还原**(对方基于旧快照写回,抹掉 tap 全部改动;state.rs 与 acp_types.rs 因对方未触碰而存活)。

- 时间戳证据:events.rs mtime 02:48:48、request_builder.rs 02:50:24(均非本席位写入);test_cfs3(02:51)回到 387 基线全过=旧版
- **处置**:全部修改已重放(05:5x 完成重放,内容含全部修复),并持久化为 `docs/token-optimization/tap-最小版.patch`(35.6KB,`git apply` 即可恢复)
- **两个工作面零文件交集**(tap:events/state/request_builder/acp_types/run_loop vs 对方:payload_accounting/acp_session/session_setup/turn/session_compact)——但 request_builder.rs 是双方共同读取面(serialized_json_bytes),对方写回机制是粗粒度全文件覆盖,无避让
- **请求指挥官裁决**:(a)tap 与 payload-accounting 两编码手**分 worktree 隔离**(isolation="worktree",代价=冷构建);或 (b)串行化(等 payload_accounting 收尾后 tap 再动 run_loop/request_builder);或 (c)至少知会对方会话勿全文件写回共享文件。本席位在裁决前**暂停一切工作树写入**,成果以 patch 形态待命

## 11. 待办与移交

**验证结果(2026-10-18 03:26-03:44 全链实测)**:
- `cargo check -p cf-chat-state` ✅ exit=0
- `cargo test -p cf-chat-state --lib` ✅ **393 passed / 0 failed**(基线 387+tap 新增 6,全绿:golden/对账恒等/防双算/近5窗口/P1-2 纯函数性/C4 边界/合成分流)
- `cargo check -p cf-shell` ✅ exit=0(run_loop 消费臂+acp_types 口径随整 crate 通过;对方 payload_accounting 已收尾,两工作面共存)
- `cargo check -p cf-pager` ✅ exit=0(P2-6 消费方)
- patch 持久化:`docs/token-optimization/tap-最小版.patch`(5 文件,691 行,`git apply` 可恢复)

1. 金丝雀跑完后,tap 数据开表(①灰度实测窗口)。
2. 全量版清单:session_kind 标签、消费侧对账告警、技能/MCP snapshot 子行进事件、1% 采样、成本口径(¥ 换算,R7R8 P2-9 明示缺席)。
