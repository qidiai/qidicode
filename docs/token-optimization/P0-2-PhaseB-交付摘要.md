# P0-2 第二步 Phase B 交付摘要（返工后口径，2026-10-11）

> 本文件是 Phase B 实施批（基线 HEAD `b9fcde45`，工作树 delta）的**交付摘要**。
> 经 GLM 审计 3 项定向返工后**改写**：imported 生命周期四场景由「已实现行为」降级为
> **状态语义声明 + 结构性推断**，并把 resume 持久化 / flag 快照持久化 / `/clear`
> 生产接线显式列为 **flag=true 灰度放量的前置项**。
> 设计侧契约编号见 `P0-2第二步-动态工具管线设计稿.md` Q4 契约表 C-10/C-11（v3.5 勘误补登记）。

## 1. 已实现（代码 + 测试钉住）

- **三段投影**：`dynamic_tools=true` 时 `prepare_tool_definitions_inner`
  (`cf-shell/src/session/acp_session_impl/sampler_turn.rs`) 把内建 defs 按 kind 分流为
  常驻段（Resident + plan 尾槽 + meta 二元）与 import 池，投影 = 常驻段 ++ 按解锁时序
  append-only 追加的 imported（`cf-tools/src/registry/dynamic_tools.rs`）。
- **C-10 / C-11 契约**：常驻段是 flag=false 视图的确定性常驻子集（逐字节相等）；
  解锁只增不减、投影单调前缀增长。
- **B-12 三态门控**（`cf-tools/src/implementations/use_tool/mod.rs`）：hidden（注册面内 +
  不在本轮快照）→ relay 内层 dispatch；in-defs → corrective「call it directly」；
  **快照缺失 → fail-safe 维持 corrective，不默认放行**；注册面外的未注册名 → 通用错误，
  永不误走 hidden 判定。发布通道 = `SharedResources`（`update_resource` / `shared_resources`）。
- **B-0/B-1 搜索扩面**（`search_tool/mod.rs`）：在 MCP 索引之外把被藏内建工具
  （含完整 `input_schema` + 逐字节 `callable_name`）扩进结果，并记录解锁（append-only）。
- **backend-search 三口同滤**（返工 1）：`web_search` 剔除前移到切分输入，
  搜索面 / defs 快照 / 投影三口同时滤除（`backend_search_drops_tool` 单一判据）。

## 2. imported 生命周期四场景 —— 状态语义声明 + 结构性推断（**非生产接线**）

> **口径降级说明**：下列四项断言建立在 `DynamicToolState` 是 `Arc<Mutex<…>>`
> 克隆共享（`session.clone()`）这一**结构性事实**之上，属**语义声明 + 结构性推断**，
> **不是**已接线的生产行为；对应测试 `imported_lifecycle_state_semantics`
> (`dynamic_tools_phase_b_tests.rs`) 仅钉住状态类型的 clone/reset 语义。

| 场景 | 声明 | 性质 |
|---|---|---|
| resume | 语义：解锁状态应随会话持久化，恢复时投影重建为「常驻段 + 历史全部 imported」 | **结构性推断**（同 Arc 克隆读同一解锁列表）；持久化/恢复未接线 |
| fork（verbatim-fork） | 语义：子代理继承父解锁集与前缀 | **结构性推断**（同源克隆） |
| subagent | 语义：子代理从零开始、不继承父 imported | **结构性推断**（新建 `DynamicToolState::default()`） |
| compact / `/clear` | 语义：compact 保留解锁状态；`/clear` 清空（`reset()`） | **结构性推断**（clone / reset 单元语义） |

## 3. flag=true 灰度放量的前置项（未完成，放量前必须补齐）

- **resume 持久化（O-8）**：解锁状态 + flag 快照 + defs 快照段的**会话状态文件
  持久化与恢复**尚未接线——现状仅进程内 `Arc` 共享，进程重启 / resume 不保留。
  **放量前必须落地 O-8 存储点。**
- **flag 快照持久化**：`dynamic_tools` 的「会话启动一次读取」快照（设计稿 A-4）目前
  只在进程内；resume 应读回会话内快照而非重算配置（Q2.4 优先级表 + O-8 硬约束）。
- **`/clear` 生产接线**：`DynamicToolState::reset()` 已就绪并有单测，但从真实 `/clear`
  命令到该调用的**生产路径尚未接线**。

## 3.5 指挥官裁决记录（2026-10-11 K2.7 独立审计 → 返工闭环）

审计报告：`qidiwork-docs/审计评审/2026-10-11-P0-2-PhaseB-K2.7code独立审计.md`（阻塞 1 + 建议 6，返工后复核 7/7 过）。以下契约定夺生效：

- **C-9 已兑现（返工 B-1 关闭）**：cap-3 执法（第 4 次起跳过全部检索，返回全量隐藏列表 + 停止提示 + `search_only_terminated` 声明）+ refine 未命中兜底（双分支：无 MCP 索引 / 两源皆空）落地，配 2 个契约测试。
- **C-9 触发口径**：实现取「首次未命中即兜底」＝设计稿「refine ≥2 次」的**超集**——裁决接受（更早触发只增信息不减安全，防死循环目的更强）。
- **C-9 兜底不解锁**：fallback 返回全量列表但**不**记 unlock（保持隐藏投影的词元优化语义；模型经 use_tool relay 消费选中项）——裁决接受，测试已钉。
- **C-9 预算截断子句（v3.2「按 kind 聚簇截断」）**：本轮不实现，兜底场景罕见、全量体积可接受——裁决记录在案，后续如见实测体积问题再补。
- **C-6 字面收窄**：`re-run search_tool` 指引适用于 **relay 失败形态**与 **C-9 兜底文案**；unregistered 名（`ghost__tool` 类）维持裸 `Tool not found` 不包裹（既有测试钉住的行为）。C-6 原文「三形态均带指引」字面由后续设计稿勘误收窄至此口径。
- **S-3 warn 收紧**：B-12 fail-safe 的 warn 仅在「管线激活（`DynamicToolState` 在场）但快照缺失」的真异常态发射；flag=false 健康路径不打 warn（防生产默认态日志噪声），正反向测试各一。

## 4. 验证

- `cargo check --workspace --locked`：0 errors（Windows）
- WSL `cargo check --workspace --locked --all-targets`：0 errors
- `cargo test -p cf-tools --lib`：全绿（含 B-12 返工新增 2 测）/ `cargo test -p cf-shell --lib`：全绿（含返工 1 新增交互断言）
- **K2.7 审计返工验证（2026-10-12，commit `2b2d717e`）**：`cargo test -p cf-tools --lib` 2629 passed / 0 failed（含 C-9×2、S-3 warn×2 新测实跑确认）；cf-shell C 盘方案 5566 passed / 0 failed（S-1 registry_ids 断言与两处改名测试实跑确认）；K2.7 独立复核 7/7 过、S-3 观察项收紧后 0 残留阻塞
