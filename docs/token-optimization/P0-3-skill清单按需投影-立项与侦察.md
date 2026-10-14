# P0-3：skill 清单按需投影——立项与侦察报告

> 2026-10-14 立项。P0-2 三连击（工具定义投影 A/B → 成果压缩 C）之后的第三战场：**skill 清单的按需投影**。排期：Phase C 实施完成后启动。

## 一、问题定义

当前系统提示注入全部 ~300+ 个 skill 的完整描述（name + description + 绝对路径），每会话固定 **20-30K token 开场税**（≈200K 窗口的 10-15%）。NPU 适配类 ~200+ 个 skill 在编程会话中全程占位但零使用。

## 二、方案蓝图（三层加载）

| 层 | 内容 | 时机 | 预算 |
|---|---|---|---|
| **L0 索引层**（常驻） | 全部 skill 超短条目：name + 分类标签 + 触发词 | 会话开始 | ~10-15 token/条 |
| **L1 基线层**（常驻） | 编程核心白名单完整描述（check-work / auto-bug-fixer / python-refactoring / dev-orchestra / secret 类等） | 会话开始 | ~30-50 条 |
| **L2 按需层**（命中注入） | 其余 270+ skill 完整描述，**命中即注入且常驻**（单调追加，不删不换） | 用户输入触发词命中 | 按需 |

**双通道防漏召回**：被动触发词匹配 + 主动 skill 检索工具兜底。

**预期收益**：开场税 20-30K → ~3-5K，每会话净省 15-25K；skill 边际添加成本从线性涨变近零。

**缓存铁律**（Phase C 同款教训）：L2 命中必须作为会话中途 user message（system-reminder）注入，**绝不改系统提示/首条 user message 前缀**——prefix 单调增长保 KV 缓存。

## 三、代码锚点（2026-10-14 侦察实锚）

### 清单注入双路径（改造须同时覆盖）

| 路径 | 锚点 |
|---|---|
| 默认 harness（markdown）——**20-30K 开销主源** | `SkillManager::take_pending()`（`cf-tools/src/types/skill_discovery_tracker/mod.rs:412-450`）→ `render_listing()`（mod.rs:214-247）→ `format_announcement()`（listing.rs:610-633）→ `inject_baseline_skill_reminder()`（`cf-shell/src/session/acp_session_impl/session_setup.rs:71-90`）包 `<system-reminder>` 注入 |
| 自定义模板 harness（XML） | `UserMessageContext::render_skill_listing_xml()`（`cf-agent/src/prompt/user_message.rs:279-291`）→ `format_announcement_xml()`（listing.rs:565-607） |

### 关键结构

- `SkillInfo`（`cf-tools/src/implementations/skills/types.rs:40-131`）：已有 `when_to_use`/`enabled`/`paths`/`disable_model_invocation` 字段可复用
- `SkillScope` 优先级（types.rs:20-36）：Local > Repo > User > Server > Bundled > Plugin——现成分组维度
- **全文加载已就绪**：`load_skill_with_body()`（skill.rs:498-510）→ `build_skill_message()`（skill.rs:59）——L2 命中注入直接复用
- **`when_to_use` 现状：纯展示零匹配**（listing.rs:80-96 渲染 "Use when:" 行）——匹配器是全新能力，但 `extract_trigger_suffix()`（listing.rs:431-451）已提供现成触发短语来源

### 注入口与挂点

- 通用注入口：`push_system_reminder()`（`cf-shell/.../reminders.rs:288-299`）
- **现成"每轮按需注入"范式**：`maybe_inject_mcp_reminder()`（mcp.rs:589-620）——L2 匹配器照抄此模式
- 匹配器最佳挂点：`turn.rs:620-625`（每轮 reminder 注入块，user message 拼好、采样前）
- 惰性先例：`ConditionalSkills`（conditional.rs:30-48，基于 `paths:` glob 的**文件触碰**触发）——与触发词维度不同但管道同构，`activate_for_paths` 链路可直接借鉴

## 四、改动面预估

| 文件 | 改动 |
|---|---|
| `skill_discovery_tracker/mod.rs` | 核心：SkillManager 加 L0/L1/L2 分层字段；`take_pending()` 改分层投影；新增 `match_and_collect_triggered(user_input)` |
| `skill_discovery_tracker/listing.rs` | 渲染器分层（L0 仅名/L1 全描述/L2 命中全描述） |
| （新建）`skill_discovery_tracker/matcher.rs` | 触发词匹配器 |
| `acp_session_impl/turn.rs` | ~620 行插入 `maybe_inject_triggered_skills_reminder()` |
| `session_setup.rs` | 启动基线改 L0+L1 |
| `cf-agent/src/prompt/skills.rs` + `cf-config` | `SkillsConfig` 加 `l1_allowlist`/`l2_enabled` 配置 |

**分期**：先默认 harness（markdown），自定义模板 harness（XML）二期。

## 五、风险清单（实施前必读）

1. **测试锚定密集**：listing.rs 大量精确断言（`xml_verbatim_row_shape` 等）会被分层打破，需同步改写
2. **/context 与注入同源**：`listing_snapshot_matches_injected_baseline_reminder`（mod.rs:1548）强制两者逐字节一致——分层后 /context 记账须反映当前投影
3. **announced_names 持久化**：L2 命中态是新的会话态，resume 是否保留需决策
4. **compaction 后重宣**：`on_compaction()`（mod.rs:521）清空 announced_names——重发口径（L0+L1 还是含已命中 L2）需决策
5. **缓存稳定性**（铁律重申）：L2 只走会话中途 system-reminder
6. **与 ConditionalSkills 碰撞**：`take_unconditional` 不得把 L2 白名单外 skill 误判为无条件显示
7. **预算耦合**：`SKILL_BUDGET_CONTEXT_PERCENT=0.5`（listing.rs:11）的三层取舍影响 tier 降级
8. **双 harness 分支**：`use_xml_format` 分支两边都要生效

## 六、验收草案（v0，设计稿评审时定稿）

- V-1 开场 token：L0+L1 ≤ 5K（对照现状 20-30K）
- V-2 触发词命中注入延迟 ≤ 1 轮；命中后常驻（单调）
- V-3 现有 skill 相关测试改造后全绿（含 /context 同源断言）
- V-4 resume/compaction 边界行为有测试钉住
- V-5 双通道兜底：skill 检索工具可查全量索引

## 七、与 P0-2 的关系

同一理念（按需投影）跨域延伸：P0-2 管**工具定义**（defs schema），P0-3 管**skill 描述**（listing）——两者共享 prefix 单调增长铁律与"投影不是权限边界"的定位声明。P0-2 的 B-12/SharedResources 经验可复用于投影状态管理。
