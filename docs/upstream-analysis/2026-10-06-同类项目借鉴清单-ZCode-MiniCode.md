# 同类项目借鉴清单：ZCode 与 MiniCode（v3，双审终裁版）

> 版本链：v1（explore 侦察+指挥官提炼）→ v2（GLM 审计修 5 处 P0）→ **v3（DeepSeek 复审 + GLM 终裁，2026-10-06）**
> 审计链：GLM 一审（外部 A-/内部 C+）→ DeepSeek 复审（抓 1 P0+2 P1+2 P2，其中 D-1 为 v2 自我覆辙）→ GLM 终裁（五项全成立，D-1 源头追至 v1 审计自身）
> 定位声明（AGENTS.md §11-13⑤）：上游与同类项目均**非合并目标**——全部条目为**仅设计借鉴**，实现以 qidicode 自有设计为准。
> 证据分级：★★★多源/★★双源/★单源/❓未核实（速写表同）

## 项目速写（v3：MiniCode 双仓库分列）

| | ZCode（zai-org/ZCode）✅已核实 | MiniCode-A：JiayuXu0/MiniCode | MiniCode-B：LiuMengxuan04/MiniCode |
|---|---|---|---|
| 身份 | 智谱官方 ADE，2026-09-21 开源（313MB 风波后：约 3 万文件 RepoWiki 上传、股价三日跌逾 16%、v3.14.0 移除链路）✅多源 | "30 天复刻 Claude Code" 教学系列原作（博客园 JiayuXu 系列） | 同名独立项目（**非 fork**，双侧 fork=False；晚 87 天创建，"受系列启发"为猜测性标注） |
| 技术栈 | TypeScript ✅（GitHub API） | **Go** + Fantasy SDK + Bubble Tea ✅（71★；2026-01-05→01-13 活跃 8 天即停更） | **TypeScript**（多语言 TS/Python/Rust 版，1130★，2026-09 仍活跃） |
| 参考定位 | 架构与机制参照 | **最小循环契约标本**（stop_reason 单循环） | 活跃多语言实现（行为参照） |

## 借鉴清单（v3）

| # | 设计点 | 来源与源流 | 适用场景（v3 修正） | 成本 | 证据 |
|---|---|---|---|---|---|
| 1 | **工具结果老化体系的既有能力披露 + 三点真增量**。**既有基线（仓库已建，勿再发明）**：`PruningConfig` 双模老化（soft_trim 尺寸档 4000/1500 + **hard_clear TTL 默认 10 回合、enabled 默认 true**，`cf-chat-state/src/types.rs:69-94`，用户可配 `[compaction.pruning]`）；`hard_clear_replacement()` **保留 spill 指针**（request_builder.rs:35-46，commit 09474065 08-23）；双触发（prune_conversation >50% + prune_retained_conversation 每用户回合）；5 个行为测试；用户文档 `13-memory.md:414-425`。机制源流：Claude Code Microcompact（canonical 拆解）→ZCode 同类实现（microcompact.ts 实勘） | CC→ZCode | **真增量（代码实证，GLM 终裁）**：**A** 回溯 handle 泛化——`"full output at:"` 指针仅 cf-tools **bash 系**产生，read_file/grep/subagent 结果被清后成盲 placeholder，无运行时回溯；**B** reasoning 块老化——prune 范围仅 ToolResult，Reasoning 只被测量从不被清理（mutations.rs:302-305），CC Microcompact 语义含历史 thinking 清理；**C** 缓存感知淘汰——hard-clear 每过年龄边界改请求内容=**破前缀缓存**，与 #2 张力**同根**（CC 的 cached_microcompact 是已知解；无此原语则应批量对齐压缩段边界而非逐回合零碎清） | 中 | ★★ |
| 2 | 缓存友好上下文组装：前缀稳定换 KV cache（**机制 ★★★** 多源；数字 ★ **单源官方宣传**：98.1% 命中/有效 token +30%，出处链=智谱官方口径→AIbase 报道→chinaz 聚合转载 2026-08-11；处方立于机制+自家遥测） | ZCode | 与 P0-2 张力成立（自证）：cached_read 计数已在 sampler/signals/headless/OTel **全链**，缺独立**命中率(ratio)指标**——分子分母现成，零成本增量；与 #1-C 联合设计（同根张力） | 中 | 机制★★★/数字★ |
| 3 | 回合严格状态流+全链路归因事件（turn-loop.ts 实勘证实） | ZCode | A-1 归因哲学的推广 | 低 | ★★ |
| 4 | 端点级上下文窗口降档 | ZCode（❓第三方 400K 未核实） | SamplingConfig 加 per-provider 窗口上限 | 低 | ★ |
| 5 | **Goal 模式的验收命令驱动**（target-completion-verification.ts 实勘） | ZCode | **v3 修正**：仓库已有完整内建（goal_tracker.rs 状态机 Idle/Planning/Executing + **5 个暂停变体**——机制侧 is_paused() 实现+测试名双证；⚠️ 源码注释 goal_tracker.rs:53 / agent.rs:291/:317 三处误写 "six" 系笔误，已列修正待办）+ verifier-skeptic + 四套 e2e。真增量=**verifier 从"子代理评审制"扩为"验收命令驱动"**（终止=命令绿门） | 低 | ★★★ |
| 6 | 闲时任务（低峰调度+独立额度） | ZCode | 过夜任务队列化 | 中 | ★★ |
| 7 | 多模型网关+路由级凭证作用域 | ZCode | A-1 已做代次绑定，补 per-provider 隔离+端点健康遥测 | 低-中 | ★★ |
| 8 | **固定长任务基准集（通过率口径）** | 基准生态（terminal-bench-2-verified 的 zai-org 归属 ❓ 维持未核实） | 已有绿门+约 195 个 PTY 行为级 e2e（实测 178 test 属性+leader/auto/xtversion），**缺的是固定长任务通过率基准**（5-10 个+每版本跑） | 中-高 | ★★ |
| 9 | 数据边界红线（313MB 反面教材：一切上传 opt-in、可见、限额、文档化） | ZCode 教训 | 出站数据审计清单——企业内网部署卖点；cf-mixpanel 默认关死✅（internal_defaults 全 None+false、TelemetryMode::default=Disabled） | 低 | ★★★ |
| 10 | **最小循环契约测试** | **MiniCode-A：JiayuXu0**（Go 极简教学定位） | cf-shell 加"loop 不变量"车道：重构不得破坏 `tool_use→执行→回填 / end_turn→停` | 低 | ★★ |
| 11 | Typed tool trait+静态 manifest | MiniCode-A / LiuMengxuan04-B | P0-2 接口基础（注意 #2 张力：裁剪走追加段） | 低 | ★ |
| 12 | 子代理分型 | ZCode | 与 §9 乐团"分型维度同构"（非完全同构）；可收增量=分组面板 UX+子代理上下文硬预算 | 低 | ★★ |
| 13 | 多端一核边界 | ZCode | 对照 cf-* 分层自查 | 0 | ★★ |

## 与在途工作的勾连（v3）

- **P0-2 第二步**：#2 张力用自家 cached_read 遥测自证基线；**#1-C 与 #2 同根**——动态裁剪与 hard-clear 老化都改变请求内容，统一在"缓存段稳定"设计下考虑（合并为一条设计约束）
- **token 优化方案 §7**：#1 真增量=A/B/C（非"建 TTL"——已建）；方案文档本未提及 pruning 机制，同批补注
- **#5**：Goal 验收命令驱动化=verifier 调绿门命令（与 check-work/auto-bug-fixer 接口点）
- **#8**：与 green-gates 记忆基线合并演进

## 审计链教训（合入 RUBRICS 的三条）

1. **R-002 升级**：「我们缺 X」类缺席断言必须扫**概念的本地同义词域**（本案例：找"老化清理"只扫了 spill/ChatReducer/compact，漏了 prune/hard_clear——机制以不同名字已存在 44 天）
2. **R-007 双向适用**：数字与机制侧同源（is_paused() 实现与测试名，而非注释字面量——源码注释自身笔误会经"源码→审计→文档"两级传播污染）
3. **内部断言独立验证不可豁免**：两轮复审全部反复出错的位置清一色是内部"缺 X"断言，外部事实零被推翻——连审计者本人也栽在自家 v1 误判上

## 补侦待办（路径已实勘修正）

`gh api repos/zai-org/ZCode/contents/apps/zcode-cli/packages/core/src/compact`（根级 core/ 会 404）

## 源码注释修正待办（防笔误第三次传播）

goal_tracker.rs:53、agent.rs:291/:317 三处 "six"→"five"（纯注释零语义；已随本 v3 commit 一并修正，见 commit 信息）
