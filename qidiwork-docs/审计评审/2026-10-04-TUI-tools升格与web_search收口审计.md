# 审计报告：P0-2 工具裁剪第一步（TUI --tools 升格 + web_search 收口）

- **日期**：2026-10-04
- **审计对象**：工作树未提交改动（基线 HEAD `f49f731`），11 文件 +208/−26 + 1 新测试文件
  - **D 通道**：ConnectFlags +tools/disallowed_tools（`cf-pager/src/acp/mod.rs`）、TUI 分支填充（`app/mod.rs`）、`apply_cli_tool_overrides` helper（复用 `CliAgentOverrides` 语义，不新增第三套）、leader 模式显式告警拒绝、headless 写路径零改动、`parse_comma_list` 升 pub(crate)、文档 14-headless-mode.md 更新
  - **A 收口**：`drop_web_search_for_backend_search` SSOT（`sampler_turn.rs`），turn_base_tool_specs / build_session_info / **compaction.rs:939（规格外第三处，编码手主动收敛）** 三处共用；新增口径一致测试（caliber_tests 2 个）+ 单元测试 + D 通道测试
- **实施**：编码手 ling-3.1-flash（4.2h，276 tool calls；规格依据 `docs/token-optimization/方案.md` §7 P0-2 + 侦察报告）
- **审计方式**：双模型交叉（audit-rubrics 流程）
  - 审计 A：GLM-5.3-222（9789s，注入 R-001/R-003/R-005/R-008/R-009）
  - 审计 B：DeepSeek-V4.1-Flash-222（4400s，注入 R-001/R-003/R-005/R-009/R-014）
- **环境披露**：sccache 服务反复崩（审计侧 `SCCACHE_DISABLE=1` 绕开）；cf-pager G 盘冷链 >90 分钟（pdb 1.6GB 链接瓶颈）→ 已回写 AGENTS.md §12

---

## 一、双审计一致结论（零冲突）

**阻塞合入：无，建议放行。**

| 维度 | 判定 | 关键证据 |
|---|---|---|
| D 通道端到端 | ✅ 链路完整自洽（A 逐环验证） | clap 可达（非子命令专属）→ 单一解析器 → connect() 收口（resolve_runtime_fields 之后/spawn 之前）→ `apply_to_definition` 直接替换（幂等，重建不叠加）→ builder 过滤（denylist/allowlist/SearchTool+UseTool 恒保留）→ 子代理 session-clamp；与 permission_rules 分层正交无冲突 |
| A 收口行为等价 | ✅ 三处**逐 token 等价**（B 主攻） | 谓词 `!use_backend_search \|\| name != "web_search"` 三处原内联逐字符一致；精确匹配（非前缀）；x_search 为 hosted tool 无本地 function 变体（grep 零命中）；compaction 侧输入同源（`prepare_tool_definitions`）、use_backend_search 谓词字面一致、false 时恒保留；`compaction.rs:187` two_pass 早已间接走共享函数——无第四处漏网 |
| /context 口径 | ✅ 无新漂移窗口 | 不靠对象共享靠 drop 谓词同一（重构后唯一）；两侧各自 fresh 调 `prepare_tool_definitions_inner`，count/tokens 经同一 drop 必然相等 |
| headless 回归红线 | ✅ 零触碰（A 确证） | 写路径 :909-924 逐行核对为既有代码；两路径互斥无双重应用；parse_comma_list 可见性升级无副作用 |
| R-001 ConnectFlags | ✅ 14 处构造点全查 | 10 字面量（9 test+1 生产，生产构造编译器强制必填已补）+4 `::default()`；`unsupported_leader_flags_detects_all` 既有测试未被放宽（R-005 双重核查） |
| 静默吞参排查（R-009 特查） | ✅ 无静默路径 | 非 leader TUI 消费 ✓/leader stderr 告警 ✓/headless 消费 ✓/ACP meta 无旁路 ✓；空串→None→不告警不生效语义自洽 |
| TUI 零影响 | ✅ | 无参数→None→`if let Some` 跳过；与 headless 汇入同一 builder，SearchTool/UseTool 强制保留与 `unresolved` 整体保留逻辑两模式无不对称 |

## 二、预先存在失败判定（R-005 重点复核）

| 套件 | 编码手声称 | 审计实测 | 判定 |
|---|---|---|---|
| cf-shell | 5537+1 failed（预先存在） | A：**5538/0 全绿**（更强，flaky 未复现）；B：5538/0 + stash 基线 5534+1（该失败为时序 flaky：2s 上限内含 chat-state 往返+JWT 刷新+TCP） | ✅ 声称保守成立，实际更好 |
| cf-pager | 6968+53 failed（预先存在） | 双方完全复现 6968/53；失败全名单（toast 字形 ✓/⚠/!、TUI 渲染块字符）与改动面**零交集**；7021=7018+3 算术闭合；A 抽查 3 个失败确证字形/渲染环境性质；B stash 基线失败集完全一致 | ✅ 成立 |

## 三、分歧与差异发现

- **无 ❌ 级冲突**。两审计对 R-003 判定粒度略异（B 报 3 处裸断言、A 全量清点约 17 处）——按 A 的全量口径采纳。
- **B 独家（F1，中，pre-existing）**：`recap.rs:79-80`（/btw 侧问）是**第 4 个 request 构建者**，未走 web_search drop 且从不设 hosted_tools——backend search 下 /btw 仍发本地 web_search，与 turn 口径不一致。属既有分歧非本次引入；处置：本轮在 SSOT doc 注释中显式声明例外（btw 提示词已声明 NO tools，实际影响低），后续可收口。
- **A 独家**：cf-shell 实测全绿（见上）；`unused_mut` ×2（新引入，caliber_tests）；leader 拒绝面覆盖不对称（`--effort`/`--todo-gate`/`--laziness-debug-log` 沉默 vs `--tools` 告警——既有问题被本次放大感知）；cf-pager G 盘冷链实测（→AGENTS.md §12 已回写）。

## 四、Rubric 触发汇总

| 条目 | 判定 |
|---|---|
| R-001 | ✅ 触发完成（14 构造点全查，Default 兼容验证） |
| R-003 | ⚠️ 触发违规（非阻塞）：约 17 处新断言裸（caliber_tests 的 count/tokens 为关键自证点）→ 返工补消息 |
| R-005 | ✅ 触发通过（既有测试零放宽；fixture 重构为委托；53 失败与 diff 零交集三重证据） |
| R-008 | ✅ 触发通过（apply 时序与注释一致；event_loop "in every mode" 一处措辞过强，返工收窄） |
| R-009 | ✅ 触发完成（无静默路径、无假阴性；B 弱触发对放开类变更对称核查通过） |
| R-014 | ✅ 已执行（B 按格式产出） |

## 五、返工与闭环（ling → 指挥官质检）

返工项（两审计合并，全部卫生级）：
1. R-003 补消息（优先 caliber_tests count/tokens）→ 2. 修 2 处 `unused_mut` → 3. 文档两行（leader 例外 + "in every mode" 收窄）→ 4. recap 例外在 SSOT doc 声明（不收口保持行为）

遗留后补（不阻塞）：F1 recap 实际收口、F4 `name_override` 重命名 web_search 的漏 drop 加固（pre-existing latent）、leader 拒绝面补全对称（既有）。

**结论**：✅ 可合入。

---

适用条目：R-001, R-003, R-005, R-008, R-009（审计 A）；R-001, R-003, R-005, R-009, R-014（审计 B）
