# AGENTS.md — qidicode 工作区约定（每次会话与子代理必读）

> 本文件是本仓库的硬性工作约定。任何会话、任何子代理在创建文件前必须先读此文件。
> 经 Step5 + K3 双审计修订（2026-09-25，修订版 r2）。本仓库为生产工具，本文件有错会系统性误导所有会话——发现本文件与代码库事实不符时，**先修本文件再继续**。

---

## 1. 文件放置总则（硬规则）

**🚫 禁止在仓库根新建任何散文件或散目录。**

根目录白名单：`README.md`、`AGENTS.md`、`Cargo.toml`、`Cargo.lock`、`.gitignore`、`build_release.ps1` 及既有目录（`crates/`、`docs/`、`qidiwork-docs/`、`third_party/`、`tools/`、`bin/`、`prod/` 等）。
**白名单之外的一切根级条目均为历史遗留**（见 §6）——不要读它们的结构做参照，不要往里写。
不确定放哪 → **先问用户，不要猜根目录**。

---

## 2. 编程项目（代码 / 测试 / 构建产物 / 技术文档）

| 内容 | 位置 | 说明 |
|---|---|---|
| 源码 / 单元测试 | `crates/<crate>/src/`、`.../tests/` | 严格遵循现有 workspace 结构，不新造顶层目录 |
| 代码文档 / 技术设计 / 代码评审报告 | `docs/<主题>/` | 例如 `docs/memory-optimization/方案.md` |
| 构建产物 | `target/` | 由 cargo 管理，**禁止手工往里塞任何自造文件** |
| 临时脚本 / 日志 / 沙箱 / 一次性验证 | `target/tmp/<主题>/` 或系统 `%TEMP%` | `cargo clean` 会回收 |

**归属判定**：既涉代码又涉业务的文档——代码评审/技术设计归 `docs/`（权威源），面向业务的交付物归 `qidiwork-docs/`（快照）。`docs/` 与 `qidiwork-docs/` 现有 4 个同名文件，更新时以 `docs/` 版本为权威源并注明。

---

## 3. 办公项目（方案 / 标书 / 审计 / 资料 / 纪要 等非代码交付物）

统一根：**`qidiwork-docs/`**。一个项目/主题一个子目录，同项目所有文件（含中间稿）都进同一子目录。

| 类型 | 位置 | 命名规范 |
|---|---|---|
| 方案 / 标书 / 投标文件 | `qidiwork-docs/方案标书/<项目名>/` | `YYYY-MM-DD-名称.docx/.pdf` |
| 审计 / 评审 / 验证报告 | `qidiwork-docs/审计评审/` | `YYYY-MM-DD-主题-类别.md` |
| 资料 / 参考文献 / 素材 | `qidiwork-docs/资料/<主题>/` | 自描述名 |
| 会话结论 / 交接纪要 | `qidiwork-docs/会话纪要/` | `YYYY-MM-DD-主题.md` |

**与现状的衔接**：上表子目录为新增规范，现存 20 个平铺 .md（旧命名 `审计-日期-主题.md` 为历史格式）**不强制迁移**；新文件一律按上表落位。跨会话继续的任务先找已有子目录，不要另开。

---

## 4. 临时文件

一次性脚本、探针、日志 → `target/tmp/<主题>/` 或系统 `%TEMP%`。需要保留的验证证据按 §2/§3 归位后，临时原件可删。**绝不进仓库根、绝不进 qidiwork-docs/。**

---

## 5. 记忆与交接

- 跨会话要记住的结论/偏好 → `/remember` 或 `/flush`（记忆存于 `C:\Users\ASUS\.qidi\memory\`，与仓库无关，所有项目共享）
- 大型任务的方案与最终结论 → 除 memory 外，同时落 `qidiwork-docs/会话纪要/`
- 换会话/新开会话前若有未沉淀结论 → 先 `/flush`
- **自动兜底**：compact 出错前会自动落盘（`pre_compact_on_error-*.md`），`/flush` 是主动版
- **关键安心条款**：`cargo clean` / `git clean` / 仓库清理**物理隔离**于 `C:\Users\ASUS\.qidi\`（记忆、会话、技能、配置都在 C 盘，不受任何仓库清理影响）
- 测试绿门基线见记忆：`reference-qidicode-green-gates`（当前：cf-memory 315 / cf-shell 5533 / cf-pager 7018 / workspace check 0 errors）——先对照基线再判断"是不是我跑挂了"

---

## 6. 历史遗留与 target 风险

- 根目录除 §1 白名单外均为历史遗留：`tmp_relay/`、`9.22方案/`、`review_tmp/`、`audit-*`、`_show_pages.py`、`gen_*.py`、`patch*.py`、`tender_full.txt`、`temp_*.png`、`build_log*.txt`、`MEMORY_SCOPE.md` 等——**只出不进**
- ⚠️ `target/debug/` 下混有大量历史用户文件（`_archive/`、`staging/`、`sagent/`、`quant_deploy_*.zip`、`少儿英语小程序/` 等）——**都在 `cargo clean` 的销毁半径内**。重要文件绝不寄居 target；clean 前先确认

---

## 7. 模型与推理强度

| 操作 | 命令 |
|---|---|
| 切换模型 | `/model <模型名> [effort]`（下拉中也可选强度） |
| 当前会话推理强度 | `/effort <low\|medium\|high\|xhigh> [--save]`（`--save` 写入 `[models].default_reasoning_effort`） |
| 启动全局覆盖 | `qidi --reasoning-effort <level>`（等价别名 `--effort`） |
| 查看当前 | `/session-info`、`/context` |

- `max` 是 `xhigh` 的 **CLI 别名**；但技能/代理 frontmatter 的 `Effort` 枚举中 `Max` 是**独立值**——两层语义不同，写技能时勿混
- 仅对支持推理的模型生效（模型目录 meta `supportsReasoningEffort` 门控）；`/effort` 默认会话级，`--save` 才持久化
- 默认值即 `Some(0.35)`（对齐 `memory_search` 工具路径；`None` 仅显式配置时出现，回退历史 0.0）

## 8. Skills（技能）

- 触发：`/<技能名>` 或自然语言触发词（`SkillInfo.when_to_use`）；技能库 `C:\Users\ASUS\.qidi\skills\`
- 技能 frontmatter 可带 `effort:`（枚举含独立 `Max`，见 §7 分层）与 `model:` 覆盖
- 创建/修改技能用内置 `/create-skill`
- 常用：办公全家桶（office-tools / bid-* / seal-extractor / pdf-to-word-ocr）、编程流程（check-work / auto-bug-fixer / python-refactoring）、编排（dev-orchestra / find-skills）、安全（pre-commit-secret-check / secret-scan / check-secrets）

## 9. 子代理编排（乐团模式，实测有效）

| 角色 | 模型 / 类型 | 用途 |
|---|---|---|
| 编码手 | `spawn_subagent(model="DeepSeek-V4.1-Flash-222", capability_mode="all")` | 按规格书写码+自测；**并行前提：任务文件零交集**（同 crate 不同文件可并行，同 crate 高内聚改动合并给一人） |
| 审计 | `GLM-5.3-222` / `step-5-preview` / `KIMI-K3-222`（`resume_from` 续会话保留上下文） | 双模型交叉审计；结论分歧时以代码证据裁决 |
| 只读探查 | `subagent_type="explore"` / `plan` | 只读侦察 |
| 隔离 | `isolation="worktree"` | 需要隔离时用（有冷构建代价） |

铁律：指挥官不写码（写规格/审计/质检/提交）；编码手与子代理**禁止 commit**；每项修复必须有对应测试。

## 10. Git 提交与推送（生产纪律）

| 项 | 约定 |
|---|---|
| 远程 | `origin`（github）、`gitee`、`upstream`（本地基线 `C:\Users\ASUS\grok-build`） |
| 默认推送 | **双推 `origin` + `gitee`**；worktree 分支用 `git push <remote> feat/xxx:main` 直推 main |
| 提交权 | 编码手/子代理禁止 commit；指挥官质检通过后统一提交 |
| 提交信息 | `<类型>: <摘要>` + 正文列要点（orchestra 战报格式，见 git log） |
| 密钥扫描 | ⚠️ **本仓库无任何 git 钩子兜底**（`.git/hooks/` 仅 post-commit Qoder tracker）——提交前**必须手动**跑 `pre-commit-secret-check` / `secret-scan` / `check-secrets` 技能 |

## 11. 环境硬约束（本机实测，新会话必读）

1. **本机无 `rg`**；内置 grep 工具在本环境**常返回空（不可靠，多轮审计实测）**→ 用 `git grep` 或 `read_file`
2. **PowerShell 不支持 `&&`**（用 `;`）；控制台 GBK 中文乱码 → `Get-Content -Encoding UTF8`
3. **禁止递归扫描**：`target/`、`mem-log/`、`tmp_relay/`、`9.22方案/scripts`、`C:\Users\ASUS\.qidi\sessions\`（1.9 万文件）——都会卡死
4. `Format-Table` 输出有缓冲错位——重要判断以具体值/行号为准，勿凭表格位置
5. `read_file` 只能读 workspace 内文件（C 盘日志用终端 `-Encoding UTF8` 读）
6. 机器上有 25+ 常驻 python/node 进程（用户其它会话的活）——**绝不擅自杀进程**
7. `G:\quant-platform\` 是 G 盘根的**独立项目**（另一会话在用）：勿动其文件、勿杀其进程；本仓库 target 下**没有**它的构建产物
8. 后台任务输出常延迟/丢失——**以日志文件为准**：运维日志落点 `G:\qidi-clean.log`、`G:\qidi-release-build*.log`；exe 备份落点 `G:\qidi-exe-backup\`

## 12. 构建与测试

| 操作 | 命令 | 耗时/备注 |
|---|---|---|
| 出产品 exe | `cargo build --release -p cf-pager-bin --bin qidi -j 4` | 冷 30-90 分钟；有缓存快；**运行中的 qidi.exe 无法覆盖 → 先改名旧 exe（Windows 允许改名不允许覆盖）** |
| cf-memory 测试 | `cargo test -p cf-memory --lib` | ~2-10 分钟 |
| **cf-shell 测试** | `$env:CARGO_TARGET_DIR='C:\qidi-cfshell-test'; $env:CARGO_PROFILE_DEV_DEBUG='0'; cargo test -p cf-shell --lib -j 4` | **热缓存全量 ~2.5 分钟**；`DEBUG=0`+串行是内存 OOM 的解药（本机 16GB，cf-shell 测试二进制直跑必 OOM） |
| cf-pager 测试 | `cargo test -p cf-pager --lib -j 4` | 冷链 15-25 分钟 |
| 快速全局校验 | `cargo check --workspace` | 0 errors 为过 |
| 清理 | `cargo clean` | 先备份 `target/release/qidi.exe` 到 `G:\qidi-exe-backup\`；**注意 §6 target/debug 历史文件雷** |

注意：cf-shell 的 **test profile** 在 G 盘 target 直跑会 OOM——一律走上面的 C 盘热 target 方案。