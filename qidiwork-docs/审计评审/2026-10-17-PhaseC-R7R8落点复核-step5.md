# Phase C R7/R8 落点复核审计 + v2.3 放行版（step-5 独立审计，2026-10-17 深夜）

> 审计员：step-5（spawn 继承模型位，`01a14a58-67d8`——50 调用/13.7 分钟）。对象：v2.2 终版排序 + R7/R8 合入后的代码状态。**结论：v2.2 可靠度高（可实施），10 处修改（5 P1+5 P2），无返工级项，补齐后放行。**

## 一、R7/R8 已合入主线实锤

- commit `8f4a0961`（2026-10-15，946 insertions/20 files）——工作树零 diff；`batched_pruning` 缺省 false 有测试钉死（types.rs:154/410-411）；验证全绿
- Phase C 是"**已合入待翻旗**"状态——不占实施量；但**灰度槽位缺失是排期决策问题**（P1-1）

## 二、tap 工程地基实锤（v2.2 可行性的决定性证据）

- `estimate_tokens_aware`（cf-token-estimation/src/lib.rs:85）：`ascii/4 + ceil(cjk×3/5) + other/4`，纯函数单遍 O(n)，**已被生产消费**（state.rs:28-91/inspect:21-1997），6 测试全覆盖——**tap 直接复用零新建**
- `ByteCounter`+`serialized_json_bytes`（request_builder.rs:443-473）：热路径每轮已在跑
- `usage_categories` 仅 2 行（skills_listing+mcp_servers，session_setup.rs:669-682）——**补齐=Vec 扩行，管道全通**
- **choke point 五截面全结构化可分**（build_conversation_request:139）：tool defs=独立参数（须过 drop_web_search_for_backend_search+plan-mode 过滤的 effective_tools，sampler_turn.rs:612-621 先例）/记忆行=独立参数/AGENTS.md=专用 item `ConversationItem::project_instructions`（三注入点全带去重守卫，prompt_build.rs:70-77 结构谓词在位）/技能清单=SkillListingSnapshot/MCP=mcp_announcement_snapshot——**无任何截面需要字节嗅探**
- AGENTS.md 静动判定可靠（渲染缓存 builder.rs:1072 一次读取；专用 item+去重）；本轮实测字节基数 32093B（较 fact-check +638B——活文档持续增长=M2 论据）
- 记忆行落位规则：**入口按参数计（注入前）**，系统提示行排除 `<memory-context>` 块（注入后）——二选一须写入规格防双算破对账

## 三、放行前的 10 处补笔（5 P1+5 P2）

| 级 | # | 内容 |
|---|---|---|
| P1 | 1 | Phase C 灰度槽位：R7/R8 已合入（"待翻旗"）但 v2.2 序列无灰度槽位+最小 tap 无 history/TR 行→①开表无法归因 Phase C 节省。二选一：①的指标加 Phase C on/off 臂+最小 tap 补粗粒度"会话历史（合计）"行；或显式排到 tap 全量版后并写明理由 |
| P1 | 2 | 前缀铁律执法半无 owner：最小不变量集（禁时间戳/nonce/顺序抖动 3-4 条）并入 tap 验收线，或明写执法归④——不可静默悬空 |
| P1 | 3 | tap 对账阈值按最小版校准：告警对象=**对账差额（Σ+残差−provider_total）**（漂移/漏 section），残差本身只监控增长率（最小分类学下残差=长会话 80%+，挂"残差大"必成噪音） |
| P1 | 4 | 子代理路径覆盖+**session-kind 标签**：机械上同 choke point 可覆盖，但事件必须带 session-kind 标签（否则子代理全量 defs 污染主回路归因） |
| P1 | 5 | M4 B-2 PoC 五要件：legacy 块共存（resume 永不重注入=前缀稳定，resume 测试钉死）/`<memory-context>` 标签兼容/域派生信号=source path（memory_context.rs:30-60）/单次搜索+事后分组（免 N× 延迟）/总预算封顶（现 6×500≈3KB 基线） |
| P2 | 6 | /context 域行显式列入 tap 交付物（TokenUsageCategory→ContextInfo.usage_categories→LegendRow 管道全通，cf-pager context_info.rs:412） |
| P2 | 7 | M9 治理机制显式化：golden 钉死/残差单独一行+增长监控/新 section 准入规则 |
| P2 | 8 | **AGENTS.md §8 与 v2.1 P1 快照过时（时效翻案）**：F2/F2R/B单（2b7ee12d/432cb8ee/996f69a4，10-16 上午）已把子代理基线技能清单注入搬到 spawn.rs:98-115（`inject_subagent_baseline_listing`，Essential 投影缺省+F2S 拒绝门禁）——AGENTS.md §8"子代理技能清单注入=0"及 v2.1 P1"子代理技能节省恒为 0"**均被推翻**（子代理现有小的 Essential 清单）——按 §0 维护协议修订（主理人执行）；tap 子代理截面按新现实多计一行技能清单 |
| P2 | 9 | 成本口径缺席明示：token→¥ 换算未进最小版；①"计费不升"判据以 token 口径执行——明示或补价目表 |
| P2 | 10 | compaction 重宣路径明列 tap 分类学（compact_system_prompt 是静态串 agent.rs:117-119；重宣字节一致性由前缀哈希兜住；静态行叙事断点在此） |

## 四、总评

> v2.2 是高质量收敛：把上轮可编码部分全部落成具体工作项，tap 从"全量分类学"收缩为"最小版"是**与中段家具优化精确匹配的对症下刀而非缩水**——四截面在 choke point 全部结构化可分、两件现成零件（estimate_tokens_aware/ByteCounter）经代码实锤、usage_categories 管道全通，工程地基是实的。主要风险不在编码而在三处治理性缺口（Phase C 无槽位/前缀铁律执法半悬空/M4 PoC 无设计）——补齐 10 处（规格补笔级，无返工级）后，v2.2 即可作为实施基线放行。另请主理人修订 AGENTS.md §8 与 v2.1 P1 被 F2R 推翻的"子代理技能注入=0"快照（§0 实测事实优先）。
