# unix 交叉编译缺口清除 + PTY harness CI 侦察 — 验证报告

- 日期：2026-10-05
- 范围：CI ubuntu `cargo check --workspace --locked` 失败链（unix 分支从未编译过的缺口）+ 加严的 `--all-targets` 测试/示例代码缺口 + `cf-pager-pty-harness` 的 CI 适配侦察（不改码）
- 结论：**unix 缺口已清除（8 crate / 33 errors / 全为缺 import 或改名遗留，零降级零 stub）**；linux 侧 check 与 all-targets 均 0 errors；Windows 侧 check 0 errors；cf-shared 测试全绿。PTY 侦察定位到 4 处包/二进制改名遗留，推荐"修名 + CI 预构建"方案。
- 禁止项遵守：未 commit / 未 push；release 产物未动。
- 改动总量：11 文件，+40 / -10（含 Cargo.lock +1 行，见 §4）。

---

## 0. 验证方法（本机无 linux 交叉 C 编译器 → 改用 WSL 原生）

任务书步骤 1 的 `cargo check --workspace --locked --target x86_64-unknown-linux-gnu`（Windows 交叉）实测**不可行**：卡于 `aws-lc-sys v0.43.0` 的 build.rs——它经 cc-rs 调 `x86_64-linux-gnu-gcc` 编译 C/汇编，本机无该交叉 C 编译器（sccache 报 `cannot find binary path`；本机也无 zig/docker daemon/任何 gcc/clang）。这正是任务书预判的"build.rs 跑主机命令导致交叉失败"情形。

**替代方案（更优，与 CI 同构）**：本机 WSL2 已装 Ubuntu 发行版，在其中原生执行 `cargo check --workspace --locked`——与 CI ubuntu-latest 同一执行环境类别（原生 linux + gcc），且：

- 工具链：WSL 内经 rsproxy 装 rustup stable **1.99.0**（与 CI `dtolnay/rust-toolchain@stable` 同 channel；repo `rust-toolchain.toml` 未改）
- registry：`CARGO_HOME=/mnt/c/Users/ASUS/.cargo` 复用 Windows 侧已下载缓存（rsproxy 镜像配置来自 repo 已提交的 `.cargo/config.toml`，与 CI 一致）
- protoc：`apt install protobuf-compiler`（3.21.12）——`cf-proto-build` 的 build script 需要（`find_protoc()` 在非 GitHub Actions 环境缺 protoc 仅告警，但 cf-shell 等依赖生成代码，故安装以保证行为对齐 CI）
- 目标目录隔离：`CARGO_TARGET_DIR=/home/xcm/qidicode-target`（WSL 原生 ext4，避免与 Windows 侧 `target/` 的 host 产物互踩）
- `CARGO_BUILD_JOBS=4` 控内存

## 1. unix 缺口清单（crate → errors → 修复方式）

逐层收敛过程（每层修复后重跑 check）：
- **check-job 层**（`cargo check --workspace --locked`，CI check job 范围）：cf-shared 12 errors → cf-pager-render 3 errors + cf-shell 1 error → 0
- **all-targets 加严层**（`--all-targets`，含 lib test / example / bench 目标，CI test job 会编译的代码）：cf-mermaid 3 → cf-plugin-marketplace 6 → cf-fast-worktree 4 → cf-sandbox 3 → cf-pager 1 → 0

### 第一层：lib/bin（CI check job 直接失败链）

| # | crate | errors | 位置 | 修复方式 |
|---|---|---|---|---|
| 1 | cf-shared | 12 × E0433（`Command`/`Stdio` 不在作用域） | `src/clipboard.rs:1626,1628-1630,1747,1749-1751,1771,1773-1775`（`tool_available` / `run_pipe_in` / `run_capture_out_with_status`，均 `#[cfg(target_os = "linux")]`） | 在 `#[cfg(not(target_os = "macos"))] mod platform` 内补 `#[cfg(target_os = "linux")] use std::process::{Command, Stdio};`。macOS 模块（554 行）自洽、Windows 下这些函数被 cfg 掉——只有 linux 分支编译时暴露缺口 |
| 2 | cf-pager-render | 3 × E0425/E0433（`Duration` 不在作用域） | `src/terminal/probe.rs:21,51`（`LATE_REPLY_GRACE` / `read_tty_reply`，均 `#[cfg(unix)]`） | 补 `#[cfg(unix)] use std::time::Duration;`（gated 避免 Windows 侧 unused import 警告） |
| 3 | cf-shell | 1 × E0599（`Permissions::from_mode` 需 `PermissionsExt` trait 在作用域） | `src/session/acp_session.rs:1283`（`persist_chat_history_jsonl_sync` 的 `#[cfg(unix)]` 块） | 块内 `use std::os::unix::fs::DirBuilderExt;` 扩为 `use std::os::unix::fs::{DirBuilderExt, PermissionsExt};` |

### 第二层：测试/示例代码（--all-targets 加严发现；plain check 不编译这些目标）

| # | crate | errors | 位置 | 修复方式 |
|---|---|---|---|---|
| 4 | cf-mermaid | 3 × E0433（`Instant` 不在作用域） | `src/mmdc.rs:249`、`src/subprocess.rs:222,250`（`#[cfg(unix)]` 测试） | 各测试模块补 `#[cfg(unix)] use std::time::Instant;`（`Duration` 已经 `use super::*` 带入，只缺 `Instant`） |
| 5 | cf-plugin-marketplace | 6 × E0425（`dir`/`outside` 不在作用域） | `src/types.rs:240,243,255,258`（两个 symlink-escape 测试） | 测试体内变量原为 `_dir`/`_outside`（下划线仅为压 Windows 侧未使用告警），但 unix 块内以无下划线名引用→改名 `dir`/`outside` + 测试 fn 加 `#[allow(unused_variables)]` 兜住 Windows 侧 |
| 6 | cf-fast-worktree | 4（1 × E0433 `PathBuf` + 3 × E0433 `xai_tty_utils` 旧 crate 名） | `src/worktree/mod.rs:328`、`src/git/probe.rs:266,299,320`（均 `#[cfg(unix)]` 测试） | 测试函数内补 `use std::path::PathBuf;`；`xai_tty_utils::detach_std_command` → `cf_tty_utils::detach_std_command`（xai→cf 改名遗留，cf-tty-utils 已是依赖） |
| 7 | cf-sandbox | 3（E0425 `ProfileName` + 2 × E0433 `SandboxManager`） | `examples/sandbox_smoke_test.rs:35,41,61`（`#[cfg(unix)] fn main`） | example 顶部补 `#[cfg(unix)] use cf_sandbox::{ProfileName, SandboxManager};`（示例从未在 unix 编译过，import 整段缺失） |
| 8 | cf-pager | 1 × E0432（`cf_test_support` 未链接） | `src/app/leader_cluster/mod.rs:55`（`#[cfg(unix)]` 模块） | `Cargo.toml` [dev-dependencies] 补 `cf-test-support = { workspace = true }`——**dev-deps 区原有的描述性注释（"In-process leader-cluster harness..."）还在，依赖行本身在 xai→cf 改名中丢失**；leader_cluster 是 unix 模块，Windows 侧不编译故从未暴露 |

**共性**：全部属于任务书修复优先序的第一类"cfg 分支缺 import/缺 use → 补齐"（#5 是变量名、#6/#8 是改名遗留，同属"unix 分支从未编译"类）。
**降级/stub 决定：无。** 没有 crate 需要降级为 stub 或 cfg 门控跳过——所有缺口都是最小补齐，无功能符号丢失，消费方无需加门控。

### 修复 diff 概览（+40 / -10，11 文件；完整 diff 见 git working tree）

核心三处（第一层）：

```diff
--- a/crates/codegen/cf-shared/src/clipboard.rs
+++ b/crates/codegen/cf-shared/src/clipboard.rs
@@ mod platform {   // #[cfg(not(target_os = "macos"))]
     use super::ImageData;
+    // The Linux Wayland/X11 CLI fallbacks (wl-copy/xclip/xsel probes and
+    // payload pipes below) spawn subprocesses; import here, gated to linux,
+    // so the Windows build never sees an unused import.
+    #[cfg(target_os = "linux")]
+    use std::process::{Command, Stdio};

--- a/crates/codegen/cf-pager-render/src/terminal/probe.rs
+++ b/crates/codegen/cf-pager-render/src/terminal/probe.rs
 use std::io::Write;
+// The timed-read path below is unix-only (raw-fd poll); keep the
+// import gated so the Windows build never sees it unused.
+#[cfg(unix)]
+use std::time::Duration;

--- a/crates/codegen/cf-shell/src/session/acp_session.rs
+++ b/crates/codegen/cf-shell/src/session/acp_session.rs
-        use std::os::unix::fs::DirBuilderExt;
+        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
```

其余（第二层）均为同型小改：gated `use std::time::Instant;`（cf-mermaid ×2）、变量改名+allow（cf-plugin-marketplace）、`use std::path::PathBuf;` 与 `xai_tty_utils`→`cf_tty_utils`（cf-fast-worktree）、gated `use cf_sandbox::{ProfileName, SandboxManager};`（cf-sandbox example）、`cf-pager/Cargo.toml` +1 行 dev-dep。

## 2. 双向（三角）验证证据

**① linux 侧（WSL Ubuntu 原生，CI 同构）— `cargo check --workspace --locked`：**
```
    Checking cf-pager-minimal v0.1.0 (/mnt/g/qidicode/crates/codegen/cf-pager-minimal)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2m 31s
```
（`wsl-check-final.log`，`^error` 行计数 = 0）

**② linux 侧加严 — `cargo check --workspace --locked --all-targets`（含 lib test / example / bench）：**
```
warning: `cf-pager` (lib test) generated 8 warnings (7 duplicates) (run `cargo fix --lib -p cf-pager --tests` to apply 1 suggestion)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8m 57s
```
（`wsl-check-alltargets6.log`，`CARGO_EXIT=0`，`^error` 行计数 = 0；警告均为既有项：unused variable / future-incompatible float 字面量 / elided lifetimes，与本次改动零交集）

**③ Windows 侧 — `cargo check --workspace`**（全部修复落盘后复跑，含 Cargo.toml/lock 变更）：
```
    Checking cf-pager-minimal v0.1.0 (G:\qidicode\crates\codegen\cf-pager-minimal)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9m 51s
```
（`win-check2.log`，`WIN_CHECK2_EXIT=0`，`^error` 行计数 = 0——新增代码未破坏 Windows 编译）

**④ Windows 测试抽样 — `cargo test -p cf-shared --lib`**（C 盘热方案 `$env:CARGO_TARGET_DIR='C:\qidi-cfshell-test'; $env:CARGO_PROFILE_DEV_DEBUG='0'`）：
```
test result: ok. 94 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.05s
TEST_EXIT=0
```
（cf-shared 源码此后未再变动，结果持续有效）

## 3. PTY harness CI 适配侦察报告（不改码）

### 3.1 `pager_binary()` 定位逻辑

`crates/codegen/cf-pager-pty-harness/src/env.rs:75-99`，解析顺序：

1. **`PAGER_BINARY` env var**（CI/Bazel 覆盖）：不存在则 bail；相对路径经 `std::path::absolute()` 解析（portable-pty 按 PATH 而非 cwd 解析非绝对路径）
2. **`CARGO_BIN_EXE_qidi-code` env var**：cargo test 仅当测试目标依赖该 bin 所属 package 时自动设置——**harness 的 Cargo.toml 并未依赖 cf-pager-bin，且 bin 名是 `qidi` 而非 `qidi-code`，故此分支永不命中**
3. **fallback**：`target/debug/qidi-code{EXE_SUFFIX}`；缺失则在 workspace root 嵌套执行 `cargo build -p qidi-code-bin --bin pager`，成功后校验文件存在

### 3.2 CI 实测失败（job 111578943620，test@windows-latest，2026-10-05T01:52）

plain `cargo test --workspace --no-fail-fast` 下，harness 自身 3 个**未 `#[ignore]`** 的测试二进制全部失败，**同一根源**：

| 测试二进制 | 失败 | 报错 |
|---|---|---|
| `--test plan_approval_resume` | 1 test FAILED | `resolve pager binary → failed to build qidi-code (exit Some(101)) → error: package ID specification 'qidi-code-bin' did not match any packages` |
| `--test scroll_correctness_ptyctl` | 1 test FAILED | `Error: resolve pager binary` + 同上 package spec 错误 |
| `--test scroll_matrix_curated` | 8 tests panicked（`scroll_matrix_curated.rs:35`） | 同上（每个 cell 独立调 `pager_binary()`，各耗 ~1s 失败） |

**根因 = 包/二进制改名遗留**：composition-root 包已从 `qidi-code-bin` 改名 `cf-pager-bin`、bin 从 `qidi-code`/`pager` 改名 `qidi`（`crates/codegen/cf-pager-bin/Cargo.toml`：`name = "cf-pager-bin"`、`[[bin]] name = "qidi"`），而 `env.rs` 四处旧名未同步：

- `env.rs:29` `qidi-code{EXE_SUFFIX}` → 应为 `qidi{EXE_SUFFIX}`
- `env.rs:43` `-p qidi-code-bin` → 应为 `-p cf-pager-bin`
- `env.rs:44` `--bin pager` → 应为 `--bin qidi`
- `env.rs:90` `CARGO_BIN_EXE_qidi-code` → 应为 `CARGO_BIN_EXE_qidi`

**重要更正（实测事实 > 任务书描述）**：任务书称 PTY 失败为 "os error 267"。CI 日志实测：PTY harness 的失败是上述 `package ID specification did not match`（exit 101），**与 os error 267 无关**。os error 267 实际来自**另一个独立失败**——`cf-hooks`：`runner::command::tests`（4 个）与依赖真实 spawn 的 `dispatcher::tests`（9 个）在 windows-latest 上报 `failed to spawn command: The directory name is invalid. (os error 267)`（hook 命令以不存在的 cwd spawn），另有 `matcher::tests` 4 个纯 regex 断言失败。cf-hooks 与本次 unix 缺口无关（Windows 侧失败），建议单独立项。

### 3.3 这些测试/bench 能否在 CI 跑

**能，且设计意图就是在 CI 跑**：

- 测试用 `portable-pty`（Windows = ConPTY）起**真 PTY** + mock 推理服务器（`cf-test-support` 的 `ContentController`），不需要交互终端/真人
- `scroll_matrix_curated` 的 Cargo.toml 注释明示 "The curated tier also runs in CI"；`plan_approval_resume` 文档写明 "CI stages the pager binary via `PAGER_BINARY`"
- `#[ignore]` 的 `empty_enter_send_now` / `prompt_history_durable_quit` 不在 plain `cargo test` 范围（仅 `--ignored` 时跑），不受影响
- benches（`pty_bench` / `paste_latency`，`harness = false`）不被 `cargo test` 执行，不受影响
- 注：CI test job 的 `cargo test --workspace` 只构建 cf-pager-bin 的 **unittest** 产物（`target/debug/deps/qidi-*.exe`），无实证产出 plain bin（`target/debug/qidi.exe`）——所以即使修好名字，fallback 第 3 步在 CI 仍可能触发嵌套构建，需要 CI 显式预构建或设 `PAGER_BINARY`

### 3.4 方案对比与推荐

| 方案 | 内容 | 评价 |
|---|---|---|
| A. 环境守卫优雅跳过 | `pager_binary()` 改返回 `Option`，无二进制时 skip 测试 | **不推荐**：curated tier 故意不 `#[ignore]` 就是要在 CI 跑真二进制 e2e，跳过会掩盖回归；且不解决根因（嵌套 cargo build 旧包名） |
| B. 修 4 处旧名 + CI test job 预构建 | 修 `env.rs` 旧名；CI test job 在 `cargo test` 前加 `cargo build -p cf-pager-bin --bin qidi`（或设 `PAGER_BINARY` 指向该产物） | **推荐**：根因修复 + 确定性高，与既有 `PAGER_BINARY` 设计一致（该 env var 就是为 CI 覆盖而设） |
| C. 只修名、依赖测试内嵌套构建 | 仅修 4 处旧名 | 本地可跑，但 CI 上从测试内嵌套 `cargo build` 慢且脆弱（CI 日志中每次 fallback 耗时 ~9.65s 且失败）；不推荐作为唯一手段 |

**推荐 = B**。实施清单（留给后续任务，本次不改码）：

1. `cf-pager-pty-harness/src/env.rs`：4 处旧名同步（3.2 节列表）
2. `cf-test-support/src/env.rs:106` `grok_binary()`：同样的 `CARGO_BIN_EXE_qidi-code` 旧名遗留，一并修为 `CARGO_BIN_EXE_qidi`
3. `.github/workflows/ci.yml` test job：`cargo test` 前加 `cargo build -p cf-pager-bin --bin qidi`（windows-latest 与未来 linux test job 均适用）
4. 文档串（`env.rs:76`、`pty_e2e/mod.rs` 等）中的 `qidi-code` 旧名可顺手清理（低优先）

## 4. `--locked` 说明

**有一处牵动，已如实处理**：cf-pager 补缺失的 `cf-test-support` dev-dependency（缺口 #8）属于 manifest 变更，`Cargo.lock` 相应 +1 行（cf-pager 的 dependencies 列表加入 `"cf-test-support"`；cf-test-support 本就是 workspace 成员、已在 lock 中，仅新增一条依赖边，无版本解析变化）。处理流程：先跑一次 `cargo metadata` 更新 lock（`git diff Cargo.lock` 确认仅 1 行插入），此后所有验证均带 `--locked` 通过（证据 ①②）——lock 与 manifest 重新一致。其余 10 个文件均为纯源码改动，不牵动 lock。

## 5. 遗留观察（供后续任务参考，非本次范围）

- CI test job（windows-latest）除 PTY harness 外还有 cf-hooks（18 lib + 4 集成失败，os error 267 类）、bridge 测试（2 failed）、cf-shell/cf-pager 各若干失败（59/2 failed 类，字形/环境相关）——与本次 unix 缺口修复零交集（本次修复对 Windows 侧行为零影响，见 ②③ 验证）。
- WSL 内 `$HOME` 被 Windows 侧环境污染为 `C:UsersASUS`，WSL 命令需显式 `export HOME=/home/xcm`（复现命令见 §0）。

---

### 收尾复验记录

- 第 6 轮 all-targets（全部 8 处修复 + lock 更新后）：`CARGO_EXIT=0`，`^error` = 0，`Finished in 8m 57s`。
- Windows 侧复验（manifest/lock 变更后）：`WIN_CHECK2_EXIT=0`，`Finished in 9m 51s`。
- 收敛过程共 6 轮 linux check（基线 → 4 轮逐层修复 → 最终全量），每轮日志存 `target/tmp/unix-gap/wsl-check*.log`（临时证据，`cargo clean` 会回收；如需留档请移入本目录）。

---

## 6. PTY 方案 B 实施记录（2026-10-05 当日，任务 2）

按 §3.4 推荐方案 B 实施：**修 4 处改名遗留 + CI test job 预构建**。**未 commit、未 push**（按任务约束）。

### 6.1 改动点清单（文件:行）

**PTY 改名遗留（4 处旧名，5 个源文件）**：

| 文件 | 行 | 改动 |
|---|---|---|
| `crates/codegen/cf-pager-pty-harness/src/env.rs` | :29 | `local_pager_binary_path()`：`qidi-code{EXE_SUFFIX}` → `qidi{EXE_SUFFIX}` |
| 同上 | :43-45 | `ensure_local_pager_binary()` 嵌套 cargo build：`-p qidi-code-bin --bin pager` → `-p cf-pager-bin --bin qidi` |
| 同上 | :52 / :56 / :64 | 三处错误消息 `qidi-code` → `qidi` |
| 同上 | :75-78 | doc 注释同步（resolution order 说明） |
| 同上 | :91 | `pager_binary()`：`CARGO_BIN_EXE_qidi-code` → `CARGO_BIN_EXE_qidi` |
| `crates/codegen/cf-test-support/src/env.rs` | :69 | `local_grok_binary_path()`：`qidi-code{EXE_SUFFIX}` → `qidi{EXE_SUFFIX}` |
| 同上 | :80 | `ensure_local_grok_binary()`：`["build","-p","pager","--bin","pager"]` → `["build","-p","cf-pager-bin","--bin","qidi"]`（旧 spec 双重错误：包 `pager` 与 bin `pager` 均已不存在） |
| 同上 | :82 / :86 / :93 | 错误消息 `qidi-code` → `qidi` |
| 同上 | :98 | doc 注释同步 |
| 同上 | :106 | `grok_binary()`：`CARGO_BIN_EXE_qidi-code` → `CARGO_BIN_EXE_qidi` |
| `crates/codegen/cf-pager-pty-harness/src/bin/pty_scenario.rs` | :23 | doc `CARGO_BIN_EXE_qidi-code` → `CARGO_BIN_EXE_qidi` |
| `crates/codegen/cf-pager-pty-harness/src/bin/scroll_matrix.rs` | :56 | 同上 |
| `crates/codegen/cf-pager/tests/pty_e2e/mod.rs` | :1-25 | 文档块：`qidi-code` → `qidi`、`cargo test -p qidi-code` → `cargo test -p cf-pager`、删除已不存在的镜像引用 |

**CI 预构建**：

| 文件 | 行 | 改动 |
|---|---|---|
| `.github/workflows/ci.yml` | :122-137 | test job 新增 `Build pager binary for PTY tests` 步骤，在 `Run tests`（:139）前执行 `cargo build -p cf-pager-bin --bin qidi`，env `CARGO_PROFILE_DEV_DEBUG: "0"` |

`Cargo.lock` +1 行为任务 1 遗留的 cf-pager dev-dependency 边（§4 已述），非本次新增。

### 6.2 CI 预构建方案选择：**预构建步骤**（而非 PAGER_BINARY 环境变量）

理由：

1. **与 harness 解析链天然对齐**：`pager_binary()` 解析顺序第 3 档正是 `target/debug/qidi{EXE_SUFFIX}`（PAGER_BINARY / CARGO_BIN_EXE_qidi 均未设置时的 fallback）——预构建步骤恰好物化该路径，YAML 无需写 OS 相关后缀逻辑，未来加 linux test job 同样适用
2. **语义不混淆**：`PAGER_BINARY` 保留为 CI 下载 release artifact 后跑 `pty_e2e -- --ignored` 套件的专用覆盖门（该 env var 的设计初衷），不被预构建路径挪用
3. **廉价**：CI 已缓存 `target/`（key `${{ runner.os }}-cargo-test-${{ hashFiles('**/Cargo.lock') }}`），重复运行预构建近乎零成本
4. **确定性**：测试内嵌套 `cargo build` 在 CI 上慢且脆弱（CI 日志实测每次 fallback ~9.65s 且失败）；预构建在测试前一次性完成
5. **OOM 对齐**：`CARGO_PROFILE_DEV_DEBUG=0` 与 test job 既有 `CARGO_PROFILE_TEST_DEBUG=0`（:144）同属 16GB runner 的 OOM 解药；PTY 测试只 spawn 二进制，剥离 debuginfo 无影响

### 6.3 验证矩阵

| # | 验证门 | 命令 | 结果 |
|---|---|---|---|
| ① | Windows harness all-targets | `cargo check -p cf-pager-pty-harness --all-targets` | ✅ exit 0，`Finished \`dev\` profile ... in 1m 33s`（日志 `target/tmp/pty-b/win-pty-harness-check.log`） |
| ② | cf-test-support lib 测试（C 盘热方案） | `$env:CARGO_TARGET_DIR='C:\qidi-cfshell-test'; $env:CARGO_PROFILE_DEV_DEBUG='0'; cargo test -p cf-test-support --lib` | ✅ `test result: ok. 20 passed; 0 failed; 0 ignored`（日志 `target/tmp/pty-b/cf-test-support-test.log`） |
| ③ | WSL（CI 同构 linux）harness all-targets --locked | `CARGO_TARGET_DIR=/mnt/g/qidicode/target-wsl cargo check -p cf-pager-pty-harness --all-targets --locked` | ✅ `CARGO_EXIT=0`，`Finished ... in 2m 24s`（日志 `target/tmp/unix-gap/wsl-pty-harness-check.log`） |
| ④ | 全局 workspace --locked | `cargo check --workspace --locked` | ✅ `WS_CHECK_EXIT=0`，`Finished ... in 6m 20s`；警告均为既有类（unused import/variable，改动文件零警告）（日志 `target/tmp/pty-b/win-workspace-check-locked.log`） |
| ⑤ | qidi 二进制构建（物化 PTY fallback 路径） | `cargo build -p cf-pager-bin --bin qidi -j 4` | ✅ `BUILD_EXIT=0`，`Finished ... in 14m 05s`。首跑因共享 sccache server 在并发 rustc 负载下断连（`error reading compile response from server` / os error 10054，属本机 sccache 基础设施问题非代码）失败；`RUSTC_WRAPPER=''` 绕过 sccache 重试成功（cargo 指纹与 wrapper 无关，~250 个已编译依赖缓存保持有效）（日志 `target/tmp/pty-b/build-qidi.log`） |

### 6.4 PTY 本机实跑：修复有效性已证，单元执行受本机环境阻塞

- **修复有效性已证**：`pager_binary()` 两条解析路径均验证通过——`PAGER_BINARY` 覆盖指向 release 产物、fallback 解析 `target/debug/qidi.exe`（273MB debug 构建物）均能定位并成功 spawn 二进制；CI 原失败模式（`package ID specification 'qidi-code-bin' did not match any packages`，exit 101）已消除；tripwire 用例 `curated_cells_all_have_a_test ... ok`（harness 自身完好）。
- **单元执行阻塞**：`scroll_matrix_curated` 8/8 cells 均失败于 `welcome text: timed out after 20s waiting for text: "Quit"`（空屏、0 streams、20s 超时）；`plan_approval_resume` 同症状（15s 超时）。
- **根因定位（cdb 栈证据，日志 `target/tmp/pty-b/cdb-stack.txt`）**：被 spawn 的 qidi.exe 存活但 cpu=0、线程数=1、仅加载 4 个模块、未产生 `~/.qidi/debug` 日志——进程尚未进入 `main()`。cdb 附加抓栈显示主线程阻塞于**加载器期 KERNELBASE.dll 的 DLL_PROCESS_ATTACH**：
  ```
  ntdll!NtCreateFile
  KERNELBASE!ConsoleCreateConnectionObject
  KERNELBASE!ConsoleInitialize
  KERNELBASE!_KernelBaseBaseDllInitialize
  ntdll!LdrpInitializeProcess   ← 进程初始化期，main() 之前
  ```
  即本 headless 代理会话缺功能 console station，ConPTY 子进程在控制台基础设施初始化时挂起——机器/会话级环境问题，仓库内不可修复。
- **判别实验**：release 产物（`PAGER_BINARY=G:\qidicode\target\release\qidi.exe`）同症状失败 → 与 debug 构建无关、与本次改名修复无关。
- **结论**：按任务预案条款如实报告——**CI 将是首次真验证**。CI runner（windows-latest / 未来 linux test job）具备完整终端基础设施，且预构建步骤已物化二进制、harness 解析链已通。

### 6.5 工作树状态

17 files modified, +81/-36（任务 1 的 12 文件 + 任务 2 的 5 文件，含 Cargo.lock）。**未 commit、未 push**。
