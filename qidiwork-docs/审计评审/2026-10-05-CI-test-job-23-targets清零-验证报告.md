# CI test job 23 失败 targets 清零 — 验证报告

- 日期：2026-10-05
- 范围：CI test job（windows-latest）暴露的 **23 个失败 targets / 186 panic**（全部存量欠账——这些 crate 的测试从未进过绿门）
- 基线 commit：`e83f28cd`（unix 缺口 + PTY 修复已合入）
- CI 日志：`G:\qidicode\target\tmp\test_job.log`（3.4MB，test@windows-latest 全量输出）
- 结论：**22/23 targets 代码/测试侧修复后本机全绿**；**1 个 target（cf-pager-pty-harness 3 个测试二进制）确认为环境限制（CI-TEST-DEBT-01）**，经 ci.yml `--exclude` 兜底；cf-pager --lib 的 54 字形失败经 ci.yml 环境变量 `QIDI_FORCE_LEGACY_CONSOLE=0` 修复（设计好的逃生舱），cf-pager **保留**在测试范围内。
- 禁止项遵守：未 commit / 未 push；`target/release/qidi.exe` 未动。

---

## 1. 分诊表（23 targets → 类别 → 根因组）

分诊方法：逐个 `cargo test -p <crate> [--test <target>]`（C 盘热 target 方案）本机复现；本机绿而 CI 挂的从 test_job.log 提取 CI 侧具体失败输出定根因。

| # | CI 失败 target | 类 | 根因组 | 本机复现 | 处置 |
|---|---|---|---|---|---|
| 1 | cf-computer-hub-mcp-adapter --lib（2 panic） | 1 真失败 | G1 | 2 failed ✓ | 测试侧 |
| 2 | cf-tool-protocol --lib + 3 test binaries（4 targets / 24 panic） | 1 | G2 | 4 targets 挂 ✓ | 测试侧（fixture 污染回退；代码侧零改动） |
| 3 | cf-paths --lib（2 panic） | 1 | G3 | 2 failed ✓ | 测试侧 |
| 4 | cf-hooks --lib（16 panic） | 1 | G16（4）+ G17（12） | 16 failed ✓ | 代码侧 + 测试侧 |
| 5 | cf-hooks --test integration（4 panic） | 1 | G17 | 4 failed ✓ | 测试侧（`#[cfg(unix)]`） |
| 6 | cf-pager --lib（59 panic = 5 NotFound + 54 字形） | 1（5）+ 2（54） | G18 + G19 | 59 failed ✓ | 测试侧（5）+ CI env（54） |
| 7 | cf-pager-pty-harness --test plan_approval_resume | 3 环境限制 | CI-TEST-DEBT-01 | ConPTY 挂（复现为挂死非 panic） | ci.yml exclude |
| 8 | cf-pager-pty-harness --test scroll_correctness_ptyctl | 3 | CI-TEST-DEBT-01 | 同上 | ci.yml exclude |
| 9 | cf-pager-pty-harness --test scroll_matrix_curated | 3 | CI-TEST-DEBT-01 | 同上 | ci.yml exclude |
| 10 | cf-paths --lib — 见 #3（重复计数排除） | — | — | — | — |
| 11 | cf-plugin-marketplace --lib（5 panic） | 1 | G4 | 5 failed ✓ | 代码侧 + 测试侧 + **代码侧追加（--no-hooks 真 bug）** |
| 12 | cf-sandbox --lib（1 panic） | 1 | G5 | 1 failed ✓ | 测试侧 |
| 13 | cf-sandbox --test windows_job_object（2 panic） | 1 | G6 | 2 failed ✓ | 测试侧 |
| 14 | cf-shell --lib（2 panic） | 1 | G7 | 本机 5538 绿（CI 挂因 `/tmp` fixture 在 Windows 不存在——本机 `G:\tmp` 碰巧存在掩盖了它，见 G7 注） | 测试侧 |
| 15 | cf-shell --test test_model_base_url_override（1 panic） | 1 | G8 | 1 failed ✓ | 测试侧 |
| 16 | cf-shell-base --lib（1 panic） | 1 | G9 | 1 failed ✓ | 测试侧 + manifest（自 dev-dep） |
| 17 | cf-tools --lib（43 panic = 42 rg 依赖 + 1 schema） | 1 | G10（42）+ G11（1） | 43 failed ✓ | 测试侧（可用性门控 + 键序归一化） |
| 18 | cf-tools --test path_suggestions_production（1 panic） | 1 | G12 | 1 failed ✓ | 代码侧 |
| 19 | cf-workspace --lib（1 panic） | 1 | G13 | 本机绿（CI 的 8.3 短路径 TEMP 触发）→ 实测归 1（根因在代码契约，非环境） | 测试侧 |
| 20 | cf-workspace --doc（1 panic） | 1 | G14 | doctest 挂 ✓ | 测试侧 |
| 21 | cf-fast-worktree --lib（3 panic） | 1 | G15 | 本机绿（CI 的 8.3 短路径 TEMP 触发）→ 同 #19 归类 | 代码侧 |
| 22 | cf-tool-protocol 见 #2 | — | — | — | — |
| 23 | cf-sandbox 见 #12/#13 | — | — | — | — |

> 去重说明：任务书给的 cargo 汇总名单中 cf-paths / cf-tool-protocol / cf-sandbox 各出现两次（lib 与 test 目标分列），上表按唯一 target 展开为 23 行；实际唯一失败 target 为 18 个（含 pty-harness 3 个）。

**类别统计**：类 1（真失败/本机可复现或根因在代码契约）15 个 target → 全部修复；类 2（CI 特异）0 个（cf-pager 字形 54 例实为类 1——本机以 unicode 控制台跑同样失败，只是本机默认 console 是 UTF-8 才侥幸绿；CI 的 OEM 代码页暴露之）；类 3（环境限制）1 个 crate / 3 个 target → exclude。

---

## 2. 根因组清单与修复方式（含 R-005 正当性论证）

裁决原则（R-005）：先判定**代码 bug**（实现错）还是**测试过期**（契约/环境变后测试没更新），修复对应侧；区分"修正"（恢复正确语义）与"迁就"（改动语义以通过测试）。

### G1 — MCP args schema-validation-null（cf-computer-hub-mcp-adapter --lib，2 例）
- 现象：`args failed schema validation: expected object but got null`（bridge.rs:623/658）。
- 根因：测试构造 `Value::Null` 作为 tool args；MCP 公共层先做 schema 校验（input schema `type: object`），Null 在到达被测的错误响应/传输错误路径之前就被拒。
- 修复：**测试侧**——`Value::Object(Default::default())`（空对象满足 schema）。
- 论证：**修正**。被测路径是"校验之后的错误处理"，测试前提（args 通过校验）才是真语义；不是迁就——没有放宽任何校验。

### G2 — tool-ID 名字空间 fixture 的双冒号污染（cf-tool-protocol 4 targets，24 例）
- 现象：identifier_validation / jsonrpc_envelope / serde_roundtrip / tool_id_derivation 四个测试二进制的 panic（:15/:84/:119/:127 等）。
- 根因（审计裁定，2026-10-05 返工）：测试 fixture 的 `cf_tools::read_file` 双冒号拼写是 **e9bf8c18（模型 base_url 特性提交）批量替换（210 插入/209 删除）产生的污染产物**，不是协议合法拼写。三重证据：①`ids.rs:214` 文档与 `registration.rs:34` 派生逻辑均为**单冒号** `{namespace}:{name}`；②上游 SMB 基线 `xai-tool-protocol/src/ids.rs` 校验器与 fork 版**逐字节相同**（单冒号），其 fixture 用 `GrokBuild:read_file`；③`CHANGELOG.md:39` 明确称 `cf_tools::run_terminal_cmd` 为 "stale legacy id"、`QidiBuild:run_terminal_cmd` 为 "the real"。
- 修复：**测试侧**——三个测试文件（`identifier_validation.rs` / `jsonrpc_envelope.rs` / `serde_roundtrip.rs`）共 18 处 `cf_tools::` 字面量回退为单冒号 `cf_tools:`（对齐 `QidiBuild:read_file` 契约，逐处核对测试意图不变）；`tool_id_derivation.rs` 两处期望改为文档化契约 `QidiBuild:read_file`（审计判定为正确修正，保留）。**代码侧零改动**——最初放宽 `is_well_formed_tool_id` 接受 `::` 分支的改动被审计判定为"迁就"并已回退，`ids.rs` 恢复为上游基线逐字节版本（`git diff` 对 HEAD 为空）。
- 论证：**修正**——校验器拒绝双冒号拼写是正确行为（协议只定义单冒号名字空间形式）；回退污染 fixture 是恢复测试的原始意图（验证名字空间形式），而非放宽校验以通过测试。下游安全：cf-shell/cf-tools 等 crate 的 `cf_tools::` 出现均为 Rust 模块路径（`pub use cf_tools::types::…`），与 ToolId 字面量无关；e83f28cd CI 历史证明这些 crate 在严格单冒号校验下无 id 校验类失败。

### G3 — 平台相关绝对路径 fixture（cf-paths --lib，2 例）
- 根因：`RelPathBuf::new("/absolute/path")` 在 Windows 上不是绝对路径（`Path::is_absolute` 平台相关），拒绝路径未触达。
- 修复：**测试侧**——`#[cfg(unix)] "/absolute/path"` / `#[cfg(windows)] r"C:\absolute\path"`。
- 论证：**修正**——用平台自身的绝对拼写作同一语义断言。

### G4 — plugin-marketplace URL 白名单与缓存机制测试错层（cf-plugin-marketplace --lib，5 例）
- CI 实测 5 例：`cache_lease_blocks_concurrent_reclone_during_scan`（`assert err.contains("cache lock timeout")` 挂）+ 4 例 `"blocked plugin clone URL 'C:\Users\RUNNER~1\...'"`。
- 根因分两层：
  1. **锁错误文案平台差**（1 例）：Windows 上 `acquire_cache_lock` 的二次获取经 LockFileEx 失败映射不同，快速失败报 "failed to lock cache" 而非轮询到截止报 "cache lock timeout"。
  2. **机制测试穿过策略层**（4 例）：TTL/reclone/fetch-head 机制测试调公共入口 `sync_source_cache`/`force_sync_source_cache`，而入口的 URL 白名单（仅 https/ssh，本地路径拒绝）是**正确且应fail-fast的安全策略**——测试用本地 tempdir 当"远端"，被策略层正当拒绝。
- 修复：**测试侧**——锁断言平台容忍（两文案任一 + `#[cfg(unix)]` 守住截止等待断言）；4 个机制测试改基到内部 `sync_cache_locked`（机制层），白名单本身由新单测 `sync_source_cache_rejects_local_path_url_at_entry` + `validate_clone_url_blocks_file_and_local_paths` 在入口层覆盖。
- 论证：**修正**——策略在入口强制且独立测试，机制测试不再伪造违反策略的输入；未放宽任何策略。
- **追加代码侧修复（真 bug，见 §3）**：机制测试改基后暴露 `clone_with_cli` 的 `git clone --no-hooks` 是无效选项。

### G5 — bwrap 测试的 `/tmp` 硬编码（cf-sandbox --lib，1 例）
- 根因：`bwrap_reexec_command(&["/tmp"], ...)` 前提是"/tmp 存在"；Windows 上 `Path::new("/tmp")` 按当前盘解析（`C:\tmp`/`G:\tmp`），干净 runner 上不存在 → 非 Linux 旧分支正确跳过 → 断言挂。
- 修复：**测试侧**——`std::env::temp_dir()`（全平台存在）。
- 论证：**修正**——测试前提"存在的路径"用平台无关方式构造；生产分支行为未动。

### G6 — Windows Job-Object 契约误读（cf-sandbox --test windows_job_object，2 例）
- 根因：测试断言 `mgr.is_applied()` 为 true；但 Windows 侧 `apply()` **故意永不置 applied**（lib.rs:269-278 SECURITY 注释——Job Object 只是启动能力探针，没有进程真正 attach，伪造 applied 会让 `is_active()`/`should_auto_allow_bash()` 报告不存在的隔离，YOLO bash 自动放行）。
- 修复：**测试侧**——断言 Windows 契约：`!mgr.is_applied()`；`readonly_profile_restricts_child_network` 断言 `!mgr.is_applied() && !mgr.restrict_child_network() && cf_sandbox::should_restrict_child_network()`（进程级子网限制在 ReadOnly profile 下确实武装）。
- 论证：**修正**——测试原来断言的是 Unix/Landlock 语义套到 Windows 上；新断言钉住安全契约本身。

### G7 — subagent cwd fixture 用 `/tmp`（cf-shell --lib，2 例）
- 根因：`request.cwd = Some("/tmp")`——cwd 校验要求真实存在的目录，Windows 上 `/tmp` 不存在（本机 `G:\tmp` 碰巧存在所以本机绿、CI 挂——根因仍是 fixture 非平台无关）。
- 修复：**测试侧**——`std::env::temp_dir()`。
- 论证：**修正**。

### G8 — 模型目录碰撞组包含内置 slug 碰撞项（cf-shell --test test_model_base_url_override，1 例）
- 根因：测试断言碰撞组恰为 `["agnes-2.0-flash", "agnes-2-0-flash"]`；内置 `agnes-2.0-flash-222` 的路由 slug 是 `agnes-2.0-flash`，经 slug 变量正当进入同一碰撞组。
- 修复：**测试侧**——成员资格 + 相对目录序断言（取代精确相等）。
- 论证：**修正**——碰撞检测的真实契约是"同 slug 的键互为碰撞"，精确相等把契约写窄了；未放宽检测本身。

### G9 — `is_grok_process` 测试前提随改名失效（cf-shell-base --lib，1 例）
- 根因：测试断言"本进程是 grok 进程"——xai→cf 改名后测试二进制名不再含 grok，正向用例前提失效。另有 `cargo test -p cf-shell-base` 独立编译时 test-helpers feature 未启用（cfg(test) 不向依赖传播 feature）。
- 修复：**测试侧**——用 grok 命名的探针副本（`--list` 即列出测试并退出）验证正向用例 + 不可能 PID 负向用例；**manifest 侧**——cf-shell-base 自 dev-dep（`cf-shell-base = { path = ".", features = ["test-helpers"] }`，cargo 惯用法）。
- 论证：**修正**——正向语义（图像名/cmdline 含 grok → true）被真正执行，而非删除用例。

### G10 — rg 依赖测试在无 rg 环境（cf-tools --lib，42 例）
- 根因：opencode glob（12）/ opencode grep（14）/ qidi_build grep（16）共 42 个测试经工具实现 shell out 到 `rg`；CI Windows runner 不预装 rg（build.rs 在 Windows 跳过 rg 自动捆绑），本机同样无 rg（AGENTS.md §11.1）→ "Error running glob: program not found"。
- 修复：**测试侧**——新增 `ripgrep_available()` 探针（`rg --version` 探测），42 个测试入口门控：`if !ripgrep_available() { eprintln!("skip: ripgrep (rg) binary not available"); return; }`；qidi_build grep 的两处卡片切片前加 `assert!(card_lines.len() >= 3)` 防错误卡片下的减溢出。
- 论证：**修正（可用性门控）**——与仓内既有惯例一致（cf-plugin-marketplace 的 `if !git_available() { return; }`）；rg 是生产依赖，测试在有 rg 的机器上仍真实执行；CI 上干净跳过而非假绿（有 stdout skip 日志）。未改任何工具实现。

### G11 — tool_meta schema 键序在 preserve_order 统一化下翻转（cf-tools --lib，1 例）
- 根因：`tool_meta_schema_is_up_to_date` 比较生成 schema 与 checked-in 文件的 pretty 字符串。workspace 构建（`cargo test --workspace`）经 feature 统一化把 cf-shell/cf-shared/cf-sampling-types/cf-codebase-graph 启用的 `serde_json/preserve_order` 带给 cf-tools → `serde_json::Map` 变为 IndexMap（插入序）；checked-in 文件由单包构建（BTreeMap，字母序）生成 → 内容完全相同、键序不同 → 字符串不等。`-p cf-tools` 单跑无 preserve_order → 本机绿。
- 修复：**测试侧**——新增 `sorted_json_value()` 递归键序归一化，比较两侧均经归一化（JSON 对象键序无语义）；`UPDATE_TOOL_META_SCHEMA=1` 写路径同样归一化（文件在两种构建模式下字节稳定）。
- 论证：**修正**——过期性检查比较的是 schema 内容而非 map 迭代序；未放宽内容比对（键、值、数组序均仍严格相等）。

### G12 — display 路径 join 混入平台分隔符（cf-tools --test path_suggestions_production，1 例）
- 根因：worktree remap 后的 `display_cwd` 是面向模型的 Unix 拼法（`/home/user/project`），`display_cwd.join(rel)` 在 Windows 拼出 `/home/user/project\src`。
- 修复：**代码侧**——新 `join_display_path()` 按 base 自身分隔符惯例拼接（含 `/` 用 `/`，否则 `\`）+ 新单测 `join_display_path_keeps_unix_spelling_on_windows`。
- 论证：**修正**——生产代码对 display-space 路径的拼接保持其拼写约定；原生 Windows display 路径行为不变（新单测钉住）。

### G13 — foreign-session 扫描双拼写的契约（cf-workspace --lib，1 例）
- CI 实测：`normalized_cwd_uses_ordinary_windows_spelling`，left=`C:\Users\RUNNER~1\...` right=`C:\Users\runneradmin\...`。
- 根因：`scan_with` **按设计**扫描两种拼写（canonical + 原始输入，二者不同时）——CI 的 TEMP 是 8.3 短形，二者不同，scanner 闭包合法地收到任一拼写；测试断言闭包只收到 canonical 形。
- 修复：**测试侧**——构造 spellings 向量（canonical + 不同时的 raw），断言收到的拼写 ∈ spellings 且非 `\\?\` 前缀。
- 论证：**修正**——测试原来只覆盖单拼写情形；新断言钉住"双拼写皆可、无扩展前缀"的真实契约。

### G14 — doctest 的 `crate::` 导入（cf-workspace --doc，1 例）
- 根因：doctest 根是 doctest 自身，`use crate::session::git::normalize_repo_url;` 解析不到（E0433）；库内文档应为外部消费者视角。
- 修复：**测试侧**——`use cf_workspace::session::git::normalize_repo_url;`。
- 论证：**修正**。

### G15 — worktree DB 注册非 canonical 路径（cf-fast-worktree --lib，3 例）
- CI 实测：`WorktreeDb::get` 查不到已注册 worktree（`normalized_cwd...`/discovery/auto_gc 三处调用点）。
- 根因：`WorktreeDb::get(path)` 用 `dunce::canonicalize` 规范化查找键（文档化契约，db/mod.rs:280-286），而 `rebuild_worktree_db` 注册的是**原始发现路径**；CI 的 TEMP 是 8.3 短形（`RUNNER~1`），注册的短形路径永远无法被 canonical 化的 `get()` 解析回。
- 修复：**代码侧**——`rebuild_worktree_db` 注册前 `wt.path = dunce::canonicalize(&wt.path).unwrap_or_else(|_| wt.path.clone())`（一处修复覆盖 discovery.rs:294 与 auto_gc.rs:1421/1658 三个调用点）。
- 论证：**修正**——使注册键符合 `get()` 的文档化契约；本地无短/长形差异时行为不变（canonicalize 幂等）。

### G16 — hook matcher 的 claude 别名委托 stub（cf-hooks --lib，4 例）
- 根因：`cf-tools/src/types/tool.rs` 的 `claude_names_for`/`qidi_names_for` 是 stub：返回**Claude 模型名**（`claude-3-5-sonnet-...`）而非工具别名（`Bash`/`Read`/...），且忽略入参 → 任何针对外部别名的 regex matcher 静默匹配不到 Grok 工具；matcher.rs 的 `.iter().any()` 随返回类型改迭代器一并调整。
- 修复：**代码侧**——委托到共享 vendor-compat 表 `claude_alias::{claude_names_for, grok_names_for}`（返回 `impl Iterator<Item=&'static str>`）。
- 论证：**修正**——恢复"按 vendor 别名反查"的真实语义（与 `kind_for` 同表同因）；不是迁就——stub 返回的内容在语义上就是错的。

### G17 — Unix shell 语义测试在 Windows（cf-hooks --lib 12 例 + --test integration 4 例）
- 根因两类：
  1. runner/dispatcher 的 fixture 以 `/tmp` 作 `workspace_root`（hook 以此为 cwd spawn，Windows CreateProcess 报 os error 267）；
  2. 12 个测试的 fixture 是 `#!/bin/sh` 脚本/`${VAR}`/`$QIDI_HOOK_EVENT`/`cat > file`/`grep -q`/`printenv` 等 bash 语义——检测到的 Windows shell（PowerShell）不提供。
- 修复：**测试侧**——fixture 的 cwd/workspace_root 改 `std::env::temp_dir()`（`RunContext<'static>` 用 `Box::leak` 满足生命周期）；纯 sh 语义测试加 `#[cfg(unix)]` + 文档注释（含"Windows 等价覆盖是已知缺口"的诚实标注）。
- 论证：**修正**——`#[cfg(unix)]` 是仓内既定模式（cf-shell-base/cf-hooks 既有测试同法）；Windows 侧不假装覆盖 shell 语义。

### G18 — foreign-resume fixture 的 `/tmp` 不可 canonicalize（cf-pager --lib，5 例 NotFound）
- CI 实测：`app_view.rs:6566` 等 `dunce::canonicalize(...).unwrap()` 抛 NotFound。
- 根因：共享 fixture 的不透明 `/tmp` cwd 在 Windows 按盘解析（CI 的 `D:\tmp` 不存在）；生产路径对 app cwd 做真实 canonicalize（effects/mod.rs:636-641 `.ok()` 优雅失败），测试用了 `.unwrap()` 的错误形状且前提不成立。
- 修复：**测试侧**——5 处 fixture 点 `app.cwd`/`launch.cwd`/`stale.cwd = std::env::temp_dir()`。
- 论证：**修正**——fixture 给真实可 canonicalize 的目录；生产行为未动。

### G19 — legacy console 字形回退在 CI（cf-pager --lib，54 例）
- CI 实测：`left: "! Always-approve ON..."` vs `right: "⚠ ..."`、`assertion failed: toast.contains('✓')` 等 54 例。
- 根因：`is_legacy_windows_console()` 的 `#[cfg(test)]` 默认非 legacy 分支**只作用于 cf-pager-render 自身测试二进制**；cf-pager 把 cf-pager-render 作为普通依赖编译，跑真实主机探测——CI runner 的控制台输出代码页是 legacy OEM（非 UTF-8）→ 判定 legacy → 所有钉死 fancy 字形的断言拿到 ASCII 回退（√/x/!）。本机默认 console 为 UTF-8 才侥幸绿（属类 1：换 legacy 代码页的本机同样会挂）。
- 修复：**CI env**——test job 设 `QIDI_FORCE_LEGACY_CONSOLE: "0"`（glyphs.rs 设计的逃生舱，`=0` 强制非 legacy）。cf-pager-render 自身测试不受影响（其 `#[cfg(test)]` 默认即非 legacy 且尊重 override——已实测带 env 跑绿）；仓内无任何测试在运行时设置该变量（git grep 实证），无冲突。生产行为不变（未设 = 正常探测）。
- 论证：**修正**——恢复测试二进制的既定语义（"toast and chrome wording assertions stay byte-stable across developer hosts"）到 CI runner；不是迁就——生产探测逻辑零改动。

### CI-TEST-DEBT-01 — cf-pager-pty-harness（3 targets）
- CI 实测：三个未 `#[ignore]` 的测试二进制全部 `timed out after 15-20s waiting for text: "Quit"` 且 **screen contents 为空**（pager 在 pseudo-terminal 内零输出）；本机 ConPTY 同样挂死（非 panic）。
- 根因：harness 用 portable-pty（Windows=ConPTY）起真 PTY + 屏幕抓取；伪控制台需要交互式控制台会话，CI 服务账户与本机无头调用都不提供。改名遗留（`qidi-code`→`qidi` 等 4 处）已在 e83f28cd 修复（binary 现在能解析），但 harness 本体从未在任何可测机器上绿过。
- 处置：ci.yml `--exclude cf-pager-pty-harness` + 详细注释（证据、e83f28cd 引用、报告 §3 引用）。待交互式控制台能力的 runner 可用后再立项清账。

---

## 3. 追加代码侧修复：`git clone --no-hooks` 无效选项（cf-plugin-marketplace）

G4 的机制测试改基 `sync_cache_locked` 后暴露：local-path remote 时 libgit2 拒绝 shallow（实测 `git2 clone failed: shallow fetch is not supported by the local transport; class=Net (12)`）→ 走 CLI 回退 → `git clone --depth 1 --no-hooks` 报 `error: unknown option 'no-hooks'`（git 2.53.0.windows.2 usage dump 实证：clone 无此选项）→ **CLI 回退在一切 git 版本上必然失败**（死代码）。

- 修复：**代码侧**——`clone_with_cli` 移除 `--no-hooks`（注释记录实证与理由）。
- 论证：**修正**——恢复文档化的"Fallback to git CLI"契约；该选项从未生效，移除无行为回归；git2 失败时（如 local 传输不支持 shallow）CLI 回退现在真正可用。生产收益：任何真实 git2 失败场景（异常服务器配置等）有了可用回退。
- 附带：`cache_lease_blocks_concurrent_reclone_during_scan` 的 `let start = Instant::now();` 移入 `#[cfg(unix)]`（消除 Windows 侧 unused-variable 警告）。

---

## 4. CI 排除清单最终形态（ci.yml diff 说明）

`.github/workflows/ci.yml` test job "Run tests" 步骤：

```diff
       - name: Run tests
-        run: cargo test --workspace --no-fail-fast
+        # Exclusion register (CI test job, 2026-10-05 triage):
+        #
+        # CI-TEST-DEBT-01  cf-pager-pty-harness (all 3 un-ignored test
+        #   binaries: plan_approval_resume, scroll_correctness_ptyctl,
+        #   scroll_matrix_curated). ...（证据：CI 日志 15-20s 超时 + screen
+        #   contents 空；本机 ConPTY 挂死；从未在任何机器绿过；e83f28cd
+        #   已修 binary 解析；本体需交互式控制台 runner；详见
+        #   qidiwork-docs/审计评审/2026-10-05-unix交叉编译缺口清除与PTY-CI侦察-验证报告.md §3）
+        run: cargo test --workspace --no-fail-fast --exclude cf-pager-pty-harness
         env:
           RUST_TEST_THREADS: "2"
           CARGO_PROFILE_TEST_DEBUG: "0"
+          # （54 字形失败的说明 + QIDI_FORCE_LEGACY_CONSOLE 设计逃生舱注释）
+          QIDI_FORCE_LEGACY_CONSOLE: "0"
```

排除清单（终态）：
- `--exclude cf-pager-pty-harness` — CI-TEST-DEBT-01（类 3 环境限制，注释含证据链与欠账编号）
- **无其他排除**——其余 22 个 target 全部修复后进绿门（cf-pager 保留，G19 由 env 修复）

---

## 5. 验证证据

### 5.1 本机分 crate 测试结果（C 盘热 target：`CARGO_TARGET_DIR=C:\qidi-ci-triage`、`CARGO_PROFILE_{DEV,TEST}_DEBUG=0`、`RUSTC_WRAPPER=''`、`CARGO_BUILD_JOBS=4`）

| crate / target | 结果 |
|---|---|
| cf-computer-hub-mcp-adapter --lib | `test result: ok. 14 passed; 0 failed`（+ 1 ignored 目标） |
| cf-tool-protocol（lib + 4 test targets） | lib `87 passed`；targets `17 / 14 / 73 / 7 passed`，全 0 failed（G2 审计返工后复测同数：`cargo test -p cf-tool-protocol` 全 5 targets 绿、TEST_EXIT=0；`cargo check -p cf-tool-protocol --all-targets` CHECK_EXIT=0——toolprotocol-rework.log） |
| cf-paths --lib | `7 passed; 0 failed` |
| cf-hooks --lib | `210 passed; 0 failed`（+ 12 / 1 两附属目标） |
| cf-hooks --test integration | `12 passed; 0 failed` |
| cf-shell-base --features test-helpers --lib | `60 passed; 0 failed` |
| cf-plugin-marketplace --lib | `133 passed; 0 failed`（--no-hooks 修复后；含 3 个新策略单测） |
| cf-shell --lib | `test result: ok. 5538 passed; 0 failed; 7 ignored`（其余 36 个附属目标全 ok；exit=0） |
| cf-shell --test test_model_base_url_override | `7 passed; 0 failed` |
| cf-workspace --lib | 并发负载下 `1380 passed; 1 failed`（唯一失败 `two_phase_drain_waits_for_producer_then_drains_queue`，handle.rs:9389）——**与本次改动无关的既有负载敏感测试**：handle.rs 未触；CI 同测 `... ok`（test_job.log 2026-10-05T07:12:47Z）；两次本地失败均在双 cargo 构建并发（8 rustc 进程）期间；**CI 同款条件隔离复跑 ×2（RUST_TEST_THREADS=2、零并发）：两次均 `1381 passed; 0 failed; 1 ignored`，exit=0**（workspace-lib-isolated.log）。定性：生产者经句柄真实时钟运行时 sleep（`start_paused` 只停测试运行时时钟），重负载下真实时钟生产者被饿死；CI 无并发编译，稳定绿 |
| cf-workspace --doc | `test result: ok. 1 passed; 0 failed; 3 ignored`，DOC_EXIT=0（G14 doctest 修复后实测，workspace-doc.log） |
| cf-fast-worktree --lib | `163 passed; 0 failed; 2 ignored` |
| cf-sandbox --lib | `15 passed; 0 failed` |
| cf-sandbox --test windows_job_object | `3 passed; 0 failed` |
| cf-tools --lib | `2605 passed; 0 failed; 6 ignored` |
| cf-tools --lib --features serde_json/preserve_order（workspace 模式忠实复现 G11） | `2605 passed; 0 failed; 6 ignored` —— G11 键序归一化在 preserve_order 下经验证 |
| cf-tools --test path_suggestions_production | `18 passed; 0 failed` |
| cf-pager --lib | 默认环境（后台任务非 UTF-8 控制台）`6968 passed; 53 failed` —— 53 例与 CI 的 54 例字形失败**同根因同族**（G19；差集 1 例 `entry_renderer::background_block_gutter_uses_block_background_fill` 为 CI 特有环境维度）；**`QIDI_FORCE_LEGACY_CONSOLE=0` 下重跑：`7021 passed; 0 failed; 6 ignored`，PAGER_ENV0_EXIT=0**（pager-env0.log）——G19 的 CI env 修复在 cf-pager 本体实证有效（6968+53=7021） |
| cf-pager-render --lib（QIDI_FORCE_LEGACY_CONSOLE=0 下，env 交互验证） | `928 passed; 0 failed; 2 ignored`，exit=0 |

### 5.2 双向 check

- Windows：`cargo check --workspace --locked`（`RUSTC_WRAPPER=''`、`CARGO_BUILD_JOBS=4`）→ **`Finished dev profile in 72m 38s`，WIN_CHECK_EXIT=0，零 `^error`**（win-check-final.log）。输出中 11 条 `warning: unused` 经逐标识符（`HANDLE`/`normalized_for_match`/`is_hard_cleared`/`IMAGE_GEN_TOOL_NAME`/`IMAGE_TO_VIDEO_TOOL_NAME` 等）词边界核验**全部位于本次未触碰的文件**（既有欠账）；本任务改动的 28 个源文件零警告。
- WSL（Ubuntu 原生，CI 同构）：`cargo check --workspace --locked`（`/home/xcm/.cargo/bin/cargo`，`CARGO_TARGET_DIR=/home/xcm/qidicode-target`）→ **`Finished dev profile in 11m 30s`，WSL_CHECK_EXIT=0，零 error**（wsl-check-full.log）。输出中警告（cf-tools web_search f32 回退、cf-fast-worktree/git/mod.rs `normalized_for_match`、cf-workspace/discovery.rs elided-lifetime、cf-chat-state `is_hard_cleared`、cf-shell 2 处、cf-pager billing/imagine 系）**全部位于本次未触碰的文件**（既有欠账，与 Windows 侧 11 条 unused 互为正交清单）。Linux 原生编译通过 = CI 同构证据。

---

## 6. 绿门基线更新建议（新基线数字）

更新记忆 `reference-qidicode-green-gates`：各 crate 修复后通过数见 §5.1；CI test job 终态命令为
`cargo test --workspace --no-fail-fast --exclude cf-pager-pty-harness`（env：`RUST_TEST_THREADS=2`、`CARGO_PROFILE_TEST_DEBUG=0`、`QIDI_FORCE_LEGACY_CONSOLE=0`）。

**建议新基线（本机 C 盘热 target 实测）**：cf-computer-hub-mcp-adapter 14 / cf-tool-protocol 87+17+14+73+7 / cf-paths 7 / cf-hooks 210+12+1 / cf-hooks integration 12 / cf-shell-base 60 / cf-plugin-marketplace 133 / cf-shell 5538（+36 附属目标全绿）/ test_model_base_url_override 7 / cf-workspace 1381+doc 1 / cf-fast-worktree 163 / cf-sandbox 15 / windows_job_object 3 / cf-tools 2605（preserve_order 下同）/ path_suggestions 18 / **cf-pager 7021（env=0 下；旧基线"6968+53 字形失败"作废——CI env 修复后全绿）** / cf-pager-render 928。cf-pager-pty-harness 3 个二进制 exclude（CI-TEST-DEBT-01）。

**观察项（非欠账）**：`cf-workspace::handle::tests::two_phase_drain_waits_for_producer_then_drains_queue` 为既有负载敏感测试（CI 绿、CI 条件隔离复跑双绿、本地仅在高并发编译负载下偶发）；如未来在 CI 复现，候选加固是让 `spawn_producer` 的 sleep 也走 paused 时钟（当前 `start_paused` 只覆盖测试运行时）。

---

## 7. 改动文件清单（工作树，未提交）

CI 工作流（1 文件）：
- `.github/workflows/ci.yml`（G19 env + CI-TEST-DEBT-01 exclude）

manifest 侧（1 文件）：
- `crates/codegen/cf-shell-base/Cargo.toml`（G9 自 dev-dep）

代码侧源码（5 文件）：
- `crates/codegen/cf-tools/src/types/tool.rs`（G16）
- `crates/codegen/cf-hooks/src/matcher.rs`（G16 配套：返回类型改迭代器）
- `crates/codegen/cf-tools/src/util/path_suggestions.rs`（G12）
- `crates/codegen/cf-fast-worktree/src/discovery.rs`（G15）
- `crates/codegen/cf-plugin-marketplace/src/git.rs`（G4 追加 + §3 --no-hooks）

测试侧（24 文件）：
- `crates/common/cf-computer-hub-mcp-adapter/src/bridge.rs`（G1，测试模块内）
- `crates/common/cf-tool-protocol/tests/tool_id_derivation.rs`（G2，期望改 `QidiBuild:read_file`）
- `crates/common/cf-tool-protocol/tests/identifier_validation.rs`、`tests/jsonrpc_envelope.rs`、`tests/serde_roundtrip.rs`（G2 返工：18 处 `cf_tools::` 污染字面量回退单冒号）
- `crates/codegen/cf-paths/src/lib.rs`（G3，测试模块内）
- `crates/codegen/cf-hooks/src/runner/command.rs`、`src/dispatcher.rs`、`tests/integration.rs`（G17）
- `crates/codegen/cf-shell/src/agent/subagent/tests/mod.rs`（G7）、`tests/test_model_base_url_override.rs`（G8）
- `crates/codegen/cf-shell-base/src/util/mod.rs`（G9，测试模块内）
- `crates/codegen/cf-sandbox/src/lib.rs`（G5，测试模块内）、`tests/windows_job_object.rs`（G6）
- `crates/codegen/cf-workspace/src/foreign_sessions/mod.rs`（G13，测试模块内）、`src/session/git.rs`（G14，doctest）
- `crates/codegen/cf-tools/src/tool_taxonomy.rs`（G11，测试模块内）、`src/implementations/opencode/glob/mod.rs`、`.../opencode/grep/mod.rs`、`.../qidi_build/grep/mod.rs`、`.../qidi_build/grep/ripgrep.rs`（G10）
- `crates/codegen/cf-pager/src/app/app_view.rs`、`src/app/dispatch/tests/router.rs`、`src/app/dispatch/tests/task_result.rs`（G18）

改动总量：本任务 31 文件（`git diff --name-only` 实证；G2 返工后 `ids.rs` 与基线逐字节一致、不再计入）+ `Cargo.lock` +1 行——该行经实测为 **G9 的 cf-shell-base 自 dev-dep** 产生的 lock 边（`Cargo.lock:2825-2832`：cf-shell-base 自身依赖表加入 `"cf-shell-base"`），属本任务改动（2026-10-05 审计返工更正：原归因"任务 1 遗留的 cf-pager dev-dep 边"有误）。
