# P0-2 第二步 Phase A 实施批审计报告（grok-4.7 第六视角）

## 1. 符合性判定表（实施 vs 设计稿 v3.3）
- flag 极性（false=直通/true=no-op+warn）+ 单代码路径 ✅ — sampler_turn.rs `if self.dynamic_tools { warn! }` 后走同一直通链，与 v3.2 极性修正一致。
- 默认关 `unwrap_or(false)` ✅ — config.rs `resolve_dynamic_tools()` 落点正确。
- 全链接入 Features→resolve→SessionActor→spawn 快照 ✅ — `Features.dynamic_tools: Option<bool>` + spawn.rs 一次读取，链条完整。
- 36 变体穷尽 match 无 `_` 通配 ✅ — resident_set.rs `pipeline_segment` 36 臂显式列举，含编译强制语义。
- 分类 Resident 8/TailSlot 2/Meta 2/Imported 24 ✅ — 逐行比对 Q1 表 #1–#36 零偏差；`resident_side==12` 断言校验。
- glob 归 List ✅ — `list_kind_is_resident_covering_both_instances` 落实 Opus 勘误。
- 配置表承载（回退方案） ✅ — `RESIDENT_SEGMENT_KINDS` 与 match 一致性断言在位，meta 单列 `META_SEGMENT_KINDS`。
- declared_kind 源 = `ToolEntry.kind`（finalize + MCP 两处构造） ✅ — types.rs 两处均写 `declared_kind: entry.kind` / `: kind`，与 v3.3 源落定一致。
- tools_for_kind 双域（registry_id+client_name） ✅ — bridge + registry 双实现，单/多实例 + 空集断言齐全。
- C-1 空白名单≡未设置 ✅ — headless.rs 三拼写归一 → None。
- C-2 白名单 ⚠️ — 仅落 parse/thread 逐字半；「合法名生效 + 非法名 warn + 常驻段=白名单∪meta」半未落（实施自注为 Phase B）。
- C-4 meta/plan 恒驻（条件版） ✅ — 真 `AgentBuilder` 四情形矩阵。
- C-7 plan 恒等 ✅ — enter/exit 字节级断言。
- C-8b 漂移防线 ⚠️ — 正例全工具扫描在位；伪造负例（应红灯用例）缺失，FakeMcpTool.kind() 独立性未证。
- BatchedPruningDomain（Phase C C-2 提前声明） ⚠️ — 超 Phase A 范围条目 1–4，但属无害前置；与 Q4「C-2」编号撞名。

## 2. 验收质量判定（R-005）
- A-1 三口同源 ✅/⚠️ — specs/count/tokens 三向一致为真断言；compaction 口用 `prepare_tool_definitions()` 间接模拟，非真实 compaction 请求路径。
- A-3 字节级 no-op ✅ — `serde_json::to_string` 指纹严格相等，真断言字节；但基线锚为「同轮 bridge 直出」而非持久 golden，未来 bridge 层新增过滤不会触发红灯。
- A-4 会话内快照抗翻转 ✅/⚠️ — 抗翻转系结构自证（无重读路径）；未走 `spawn_session_actor` 端到端。
- C-7 plan 恒等 ✅、C-4 meta 恒驻 ✅ — 均真断言。
- 36 分类测试 ✅ — 逐变体迭代 + 槽位计数，强度充分。

## 3. CI 全绿盲区
- 命名域实跑：5 个 name_override 工具 `registry_id≠client_name` 未被 fixture 覆盖（全同域），C-5 双域差异无实跑背书。
- 多会话并发快照：a4 两 actor 串行 LocalSet，非真并发；并发一致语义未验。
- `hook_shell_command_argv` 运行时表现：属 Phase B 发布语义④，Phase A 无覆盖面。
- flag=true 的 `warn!` 内容/次数未被 tracing 断言，仅验不崩溃。
- 指纹稳定性依赖 `ToolDefinition` 序列化确定性（若含 HashMap 有 flaky 风险）。
- 漂移/伪造的负例（应红用例）缺失，C-8b 无法自证防线有效。

## 4. Q2.3 杀死判据前置核查
- 文档层 ✅ — v3.3 Q2.3-6 明确「全场景不盈利→B-2 整条砍掉，MVP 独立」，判据闸在位。
- 代码层 ⚠️ — Phase A 未留任何 B-2 判据挂钩（数据挂点 B-5/B-10 属 Phase B），设计稿亦未要求 Phase A 落；存在 Phase B 开工时遗漏风险。

## 5. 修订建议
阻塞（0）：无。
可后补：
1. C-2 补「合法名生效 + 非法名 warn + builder 常驻段」半（Phase B 前）。
2. C-8b 补伪造 kind() 负例 + 证 FakeMcpTool.kind() 独立于 declared_kind。
3. A-3 增持久 golden 快照，脱离「对 bridge 直出」的弱锚。
4. A-4 走 spawn 真路径或加字段只读性（once-set）断言。
5. fixture 增 name_override 双域用例（C-5 前置）。
6. 两个「C-2」编号去歧（Q4 vs Phase C 验收）。
7. warn 日志加 tracing 断言。

## 6. 终论
**可合入。** Phase A 骨架为默认关 + true=no-op+warn 纯空操作，无副作用面；36 变体穷尽、双域查询、declared_kind 源落定、C-1/C-4/C-7 主链全部符合 v3.3；A-1/A-3 为真断言。上述 7 条均为质量增强，不阻塞下一步；B-2 判据闸建议在 Phase B 开工前显式代码化防止口头依赖。

适用条目：R-006, R-005, R-009, R-012。