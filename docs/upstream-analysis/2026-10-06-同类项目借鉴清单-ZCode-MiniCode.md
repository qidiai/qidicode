# 同类项目借鉴清单：ZCode 与 MiniCode（v2，审计修订版）

> 侦察日期：2026-10-06（explore 子代理，web_search 多源交叉）｜**GLM-5.3 独立审计修订 2026-10-06**（外部事实经 web_search 复核 + GitHub API 直读 ZCode 仓库树实勘；内部声称经全仓 grep + 亲读——v1 的 5 处 P0 错误已按审计处方修正）
> 定位声明（AGENTS.md §11-13⑤）：上游与同类项目均**非合并目标**——全部条目为**仅设计借鉴**，实现以 qidicode 自有设计为准。
> 证据分级：★★★多源/★★双源/★单源/❓未核实（v2 起速写表也带分级）

## 项目速写（v2：未验事实已标 ❓）

| | ZCode（zai-org/ZCode）✅已核实 | MiniCode（LiuMengxuan04/MiniCode）|
|---|---|---|
| 身份 | 智谱官方 ADE（"GLM-5.3 官方 Harness"），2026-09-21 开源（313MB 隐私风波后：约 3 万文件"代码库索引/RepoWiki"上传、股价三日跌逾 16%、v3.14.0 移除链路）✅多源 | "30 天复刻 Claude Code" 教学项目（博客园 JiayuXu 系列；Go + Fantasy SDK + Bubble Tea ✅；600+ star ✅单源；**原仓库归属 JiayuXu0 与 fork 关系 ❓未核实**） |
| 技术栈 | TypeScript ✅（GitHub API language 字段） | Go ✅ |
| 架构核心 | "多端一核" monorepo：apps/zcode-cli + packages/desktop + **packages/web**（内核 core 嵌于 `apps/zcode-cli/packages/core/`，**根级 core/ 不存在**）✅实勘 | model-tool-loop 极简单循环（stop_reason 驱动 ❓细节未直读） |
| 子代理 | `core/src/subagent/` 存在 ✅结构级；Explore/general-purpose 具体型名 ❓ | 无 |

## 借鉴清单（v2，按价值排序）

| # | 设计点 | 来源与源流 | 适用场景（v2 修正） | 成本 | 证据 |
|---|---|---|---|---|---|
| 1 | **工具输出级微压缩（历史 TTL 老化）**：Auto-Compact 之外的清理层——**机制 canonical 源流是 Claude Code 的 Microcompact**（多源拆解文），ZCode 属同类实现（`apps/zcode-cli/packages/core/src/compact/` 的 microcompact.ts 已实勘证实） | Claude Code→ZCode | 我们已有：**cf-shell/session/compaction\* + cf-chat-state 压缩管线**（全局压缩）+ **truncate.rs 即时限长**（与历史老化不同层）+ **spill 指针回溯 handle**（request_builder.rs:22-31，"full output at: 检索指针"）。真增量=**在既有 spill handle 惯例之上加历史工具结果 TTL 淘汰**（复用指针，非新发明） | 中 | ★★ |
| 2 | **缓存友好上下文组装**：前缀稳定换 KV cache（**官方口径**命中 98.1%、有效 token +30%——chinaz 2026-08-11 转载 ZCode 升级公告，**宣传数字非第三方实测**；处方应立在机制+自家遥测上） | ZCode | **与 P0-2 直接张力成立**（机制自洽）：动态裁工具清单击穿前缀缓存 → core-toolset 固定为前缀段、on-demand 放追加段。遥测现状：**cached_read 计数已在 sampler/messages、signals、headless JSON、OTel 全链存在**——缺的只是独立**命中率(ratio)指标**，分子分母现成，零成本增量 | 中 | ★★★ |
| 3 | 回合严格状态流+全链路归因事件（`.../runtime/methods/turn-loop.ts` 实勘证实） | ZCode | A-1 bearer 归因哲学的推广：每阶段 emit 带 trace id 结构化事件 | 低 | ★★ |
| 4 | 端点级上下文窗口降档 | ZCode（❓第三方 400K 档未核实） | SamplingConfig 加 per-provider 窗口上限，防小窗端点静默截断 | 低 | ★ |
| 5 | **Goal 模式的验收命令驱动**（ZCode 侧 `target-completion-verification.ts`/`target-continuation-loop.ts` 实勘证实） | ZCode | **v2 修正**：我们**已有完整内建 Goal 基础设施**（goal_tracker.rs 状态机 Idle/Planning/Executing+六暂停态、goal_orchestrator.rs、verifier-skeptic 子代理验证、四套 goal e2e）——真增量收窄为：**verifier 从"子代理评审制"扩为"验收命令驱动"**（终止=命令绿门而非评审意见） | 低 | ★★★ |
| 6 | 闲时任务（低峰调度+独立额度） | ZCode | 过夜任务人肉脚本化 → 任务队列+低峰调度 | 中 | ★★ |
| 7 | 多模型网关+路由级凭证作用域 | ZCode | A-1 已做"凭证代次绑定请求"，补 per-provider 隔离+端点健康遥测 | 低-中 | ★★ |
| 8 | **固定长任务基准集（通过率口径）** | ZCode/基准生态（terminal-bench-2-verified 的 **zai-org 归属 ❓未核实**——terminal-bench 系第三方学术基准生态） | **v2 修正**：我们已有绿门+约 200 个 PTY 行为级 e2e（pty_e2e 等）——**不缺行为回归，缺的是固定长任务通过率基准**（harness 级退化短冒烟测不出）。先做 5-10 个固定长任务+每版本跑 | 中-高 | ★★ |
| 9 | **数据边界红线**（313MB 反面教材：一切上传 opt-in、可见、限额、文档化） | ZCode 教训 | 出站数据审计清单（哪些功能出网/传什么/多大/能否关）——企业内网部署**卖点**；cf-mixpanel 默认关死（cf-telemetry config internal_defaults 全 None+false，TelemetryMode::default=Disabled）✅已核实 | 低 | ★★★ |
| 10 | **最小循环契约测试**（MiniCode 单循环为规范标本） | MiniCode | cf-shell 加"loop 不变量"车道：重构不得破坏 `tool_use→执行→回填 / end_turn→停` | 低 | ★★ |
| 11 | Typed tool trait+静态 manifest | MiniCode | P0-2 接口基础（注意 #2 张力：裁剪走追加段） | 低 | ★ |
| 12 | 子代理分型 | ZCode | **v2 修正**：与我们 §9 乐团"分型维度同构"（只读/可写），非完全同构（我们是 4+ 角色编排体系）；可收增量=分组任务面板 UX + 子代理上下文硬预算 | 低 | ★★ |
| 13 | 多端一核边界（运行时不被端绑架） | ZCode | 对照 cf-* 分层自查 | 0 | ★★ |

## 与在途工作的直接勾连（v2）

- **P0-2 第二步**：#2 张力成立但需自证——用自家 cached_read 遥测数据（全链现成）跑基线对比，验证"前缀段+追加段"处方，不引宣传数字当论据
- **token 优化方案 §7**：#1 修正后表述=「既有 spill handle + 历史工具结果 TTL 老化」增量，并入 P1 候选
- **#5 修正后**：Goal 验收命令驱动化是与 check-work/auto-bug-fixer 最自然的接口点（verifier 调绿门命令）
- **#8 修正后**：基准集建议与"green-gates 记忆基线"合并演进（每版本记录基准通过率）

## 补侦待办（v2：路径已实勘修正）

`gh api repos/zai-org/ZCode/contents/apps/zcode-cli/packages/core/src/compact` ——注意真前缀是 `apps/zcode-cli/packages/core/src/`（**根级 core/ 会 404**，v1 待办照抄执行会踩坑，本审计已实测）
