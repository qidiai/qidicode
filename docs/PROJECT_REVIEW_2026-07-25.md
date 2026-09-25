# QIDI Code 项目整体审阅报告

日期：2026-07-25
范围：G:\qidicode 全仓（含工程配置、安全审计状态、辅助资产、版本管理）
方法：三视角并行探索（架构 / 质量安全 / 辅助资产）+ 逐项代码核实

---

## 1. 项目概况与架构分层

QIDI Code 是 grok-build 的 Rust 分叉，定位为原生 Windows 的 AI 编码 CLI。
- 规模：79 个 workspace members（含 4 个 third_party vendored 库），约 123 万行 Rust，edition 2024。
- 分层（依赖方向自上而下）：

| 层 | Crate | 职责 |
|---|---|---|
| 组合根 | `cf-pager-bin` | 二进制 `qidi`，main.rs 2935 行；大栈线程启动（debug 32MB）、IoC 钩子安装、子命令分发 |
| UI | `cf-pager`（+ minimal/render/pty-harness） | ratatui TUI 全部应用逻辑 |
| 运行时 | `cf-shell`（+ base/session-support） | Agent 运行时、leader/stdio/headless 入口 |
| Agent | `cf-agent`、`cf-agent-lifecycle`、`cf-subagent-resolution` | Agent 核心、生命周期、子 Agent |
| 基础 | `cf-tools`、`cf-workspace`、`cf-config`、`cf-auth`、`cf-secrets` 等 | 工具执行、权限、配置、凭据 |
| 协议 | `crates/common/*`（tool-protocol/runtime/types、compaction、circuit-breaker、computer-hub 等） | 跨领域共享 |
| 三方 | `third_party/`（dagre_rust、graphlib_rust、mermaid-to-svg、ordered_hashmap） | vendored |

- 已知过渡负担：
  - workspace.dependencies 保留旧名别名（`cf-config-types`、`cf-workspace-client`、`xai-token-estimation`、`cf-tools-api`），依赖图有重复节点。
  - clippy.toml 注释仍引用 xai-grok-* 旧路径（以代码为准）。
  - main.rs 单文件近 3000 行且头部 `#![allow(dead_code, ...)]` 宽松。

## 2. 工程规范与构建约束

- 构建：`$env:PROTOC = 'G:\qidicode\bin\bin\protoc.exe'`，`cargo check --workspace`；全量 debug 重建约 109 分钟。
- clippy 铁律：禁 `std::fs::canonicalize` / `tokio::fs::canonicalize`（Windows verbatim 路径问题），用 `dunce::canonicalize`。
- lints：unwrap/expect/panic/todo/dbg/print = warn。
- 镜像：rsproxy.cn（.cargo/config.toml）；Windows crt-static；musl 目标带 RELRO+noexecstack 加固。
- **模板加密约束**：`cf-agent/templates/*.md`（prompt.md / apply_patch_prompt.md / subagent_prompt.md）是 XOR 加密进 `src/prompt/prompt_encrypted.rs` 的；改模板后必须在 cf-agent 目录运行 `python scripts/encrypt_templates.py` 重新生成，否则运行时提示词与模板漂移。
- 文件编辑约定：PowerShell `Set-Content` 会损坏非 ASCII UTF-8 文件，须用 .NET `[System.IO.File]::WriteAllText`（UTF8Encoding(false)）或编辑器工具。

## 3. 安全审计状态

三轮审计（2026-07-21 → 07-23）：62 项发现，44 项已修复；2026-07-25 完成 P0-P3 收尾 + 冒烟测试，`cargo check --workspace` 0 errors 0 warnings（`_audit_check.log` 是修复前的陈旧快照，已列入后续任务重跑归档）。

剩余未修项（本次逐项核实，**5 项**，非记录中的 6 项）：

| 项 | 状态 | 代码位置 |
|---|---|---|
| Rate limiting | **已实现**（本次核实修正） | `cf-sampler/src/rate_limiter.rs` 完整 token-bucket + 测试；`lib.rs:28` 声明、`client.rs:287/545` 持有、`actor/request_task.rs:430` 每次出站请求前 `acquire()` |
| 静态加密（凭据 at-rest） | 零实现 | 无基础设施，需先定密钥来源与存储方案，单独立项 |
| HMAC | 零实现 | 同上，单独立项 |
| MCP tool override 收紧 | 有骨架 | `cf-agent/src/error.rs:15`（UnknownToolOverride）、`config.rs:1394`、`agent.rs:225` |
| Hooks 信任模型增强 | 有基础 | `cf-workspace/src/trust.rs`（TrustStore 单一权威，fail-closed）、`cf-hooks/src/trust.rs`（legacy 迁移）、bash deny-list 在 `cf-workspace/src/permission/manager.rs:320-354` |
| LLM truncation 统一策略 | 有骨架 | `cf-agent/src/builder.rs`（TruncationCfg 注入 MCP 路径）、`prompt/user_message.rs:37-56`（git status 截断） |

## 4. 品牌残留清单

**A. 可安全修改的文案（P1 批次一已完成，2026-07-25，提交见第 7 节）**
- ~~`cf-pager-bin/src/main.rs` L173、L1505、L1508、L1524：用户可见错误提示中的 "Grok"~~ → 已改为 QIDI Code。
- ~~`cf-shell/skills/create-skill/SKILL.md`："Create a new Grok skill" 等 5 处文案 + 4 处路径~~ → 文案已改；路径核实为**用户级 `~/.qidi`、项目级 `.grok`**（与代码行为一致，见下方备注）。
- ~~`cf-compaction/src/templates/compaction_developer_prompt.txt` 与 `compaction_user_prompt.txt`：各 5 处 Grok/xAI 身份（非仅首行）~~ → 已全部改为 QIDI Code（这两个 .txt 不在 cf-agent 加密管线内，直接改即可）。
- ~~`cf-pager/docs/custom-hooks.md`、`hooks-and-plugins.md`、`cf-hooks/examples/`（README + tool-logger.sh/session-log.sh）：用户级 `~/.grok` 路径~~ → 已改 `~/.qidi`；custom-hooks.md 中 4 个失效示例链接（`xai-grok-hooks` 旧 crate 名 + 错误相对深度）一并修复为 `cf-hooks`。

> **路径口径核实（重要）**：当前代码**用户级目录已迁 `~/.qidi`**（`cf-config/paths.rs` default_grok_home），但**项目级目录仍扫 `.grok/`**（`cf-tools/types/compat.rs:368` skills、`cf-shell/util/hooks.rs:91` hooks、`cf-workspace/project_config.rs:81` config.toml）。文档中项目级 `.grok/` 引用是正确的，未改动；项目级目录是否迁移 `.qidi/`（需兼容期双扫）单列评估。

**A2. 批次一之后新发现的同类残留（下一批处理）**
- `cf-shell/skills/help/SKILL.md`：多处 `~/.grok/docs`、`~/.grok/config.toml`——但 `cf-shell/bundle.rs:86` 文档解压根目录也仍是 `~/.grok`，文案与运行时行为纠缠，需连同 bundle.rs 一起改。
- `cf-pager/docs/user-guide/14-headless-mode.md`：`~/.grok` 只读挂载说明。
- `cf-pager/scripts/install.sh`、`install-enterprise.sh`：`~/.grok/auth.json`、`~/.grok/bin` 等（安装管线，需连同发布流程验证）。
- `cf-tools/THIRD_PARTY_NOTICES.md`：`~/.grok/vendor/`（需核对 vendor 解压代码实际路径）。

**B. 不动的 API 标识符（改动有兼容风险，单列评估）**
- `PluginOrigin::ProjectGrok/UserGrok`（serde 序列化兼容）。
- `cf-mermaid` 主题 `GrokDay/GrokNight`。
- `ClientType::GrokPager`、`XAI_*`/`GROK_*` 环境变量（README 明示向后兼容）。
- `cf_shell::util::grok_home` 模块名等内部标识符。

## 5. 辅助资产登记

- **全息星图**：hologram_engine 是独立的 MCP 服务，不属于本仓库。`.hologram/`（hologram.db 228MB、vectors.usearch 94MB、baseline.json 162MB）与 `hologram_graph.json`（138MB 符号级导出，58844 节点/182984 边）只是它在本工作区的缓存/导出物，数据可再生（MCP 切回工作区自动重建索引），代码零依赖。处理：全部 gitignore。
- 已知引擎侧 bug：`hologram_graph_files.json` 文件级聚合导出只有单节点「G」（疑似按「.」切分节点 id 时盘符归并），向引擎侧反馈，本仓库无动作。
- `.codely/`、`.codely-cli/`、`mem-log/`、`CODELY.md`：Codely 工具的运行痕迹与记忆文件，已 gitignore（不入库，本地保留）。
- `reference/`：外部克隆参考区，已忽略；`bin/`：vendored protoc 工具链。

## 6. CI 现状与缺口

现状（`.github/workflows/ci.yml`）：windows-latest；fmt --check、check --workspace、clippy -D warnings 阻断；test job `continue-on-error: true` 不阻断；rsproxy 镜像 + 三级缓存。
缺口：test 不阻断掩盖回归；无 Linux/macOS 矩阵；有 deny.toml 但 CI 未跑 cargo-deny；无模板加密一致性校验（templates/*.md 与 prompt_encrypted.rs 可能漂移）。

## 7. 版本管理状态与本次保全记录

审阅时发现的最高风险：仓库仅 2 个提交，**三轮安全修复、品牌改造、中文化共 359 项改动（262 修改/79 删除/18 未跟踪）全部未提交**；且无 origin 远程（仅本地 upstream 指向 C:/Users/ASUS/grok-build），存在整体丢失风险。

本次保全动作（2026-07-25）：
1. `.gitignore` 补齐：`.hologram/`、`hologram_graph*.json`、`mem-log/`、`.codely/`、`.codely-cli/`、`_audit_*.ps1`、`_audit_check.log`、`CODELY.md`（约 470MB 本地数据不再有误提交风险）。
2. 分三批提交（均 --signoff）：
   - `b343891` refactor: grok_build→qidi_build 重命名（129 文件，git 识别为 rename）、prompt 身份、TUI 双语视图。
   - `9ff5d50` security: 三轮审计修复（205 文件，+2636/-1175，含 rate_limiter、yolo.rs、windows_job_object 测试）。
   - `de8dfbd` chore: CI 工作流、CHANGELOG、README/SECURITY/CONTRIBUTING、.gitignore、清理 3 个 stray zip。
3. 提交后工作区干净（`git status` 0 项）。
注：批次边界按路径主题近似划分，单个中间提交不保证独立可编译；最终树与提交前工作区一致。

## 8. 后续任务清单（分优先级）

| 优先级 | 任务 | 说明 |
|---|---|---|
| P0 | 建立 origin 远程备份 | 平台与公开性由所有者决定；注意核心资产边界 |
| P1 | 品牌文案批次清理 | **批次一已完成**（第 4 节 A 栏全部）；批次二 = A2 栏新发现项（help/SKILL.md+bundle.rs、headless 文档、install 脚本、THIRD_PARTY_NOTICES） |
| P1 | 重跑 `_audit_build.ps1` 刷新基线 | 改用 UTF-8 输出（当前 log 是 UTF-16 乱码），归档新快照 |
| P2 | 剩余安全项 | LLM truncation 统一策略 → MCP override 收紧 → hooks 审计日志；HMAC/静态加密单独立项设计评审 |
| P2 | CI 收紧 | test 去 continue-on-error（或独立 allow-fail 报告 job）、加 cargo-deny、加模板加密一致性校验 |
| P3 | 引擎侧反馈 | hologram_graph_files.json 单节点「G」聚合 bug；本仓库无动作 |
| P3 | 架构减负 | 收敛旧名别名、拆分 main.rs 子命令分发、修正 clippy.toml 旧路径注释 |
