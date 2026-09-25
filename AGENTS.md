# AGENTS.md — qidicode 工作区约定（每次会话与子代理必读）

> 本文件是本仓库的硬性工作约定。任何会话、任何子代理在创建文件前必须先读此文件。
> 最后更新：2026-09-25

---

## 1. 文件放置总则（硬规则）

**🚫 禁止在仓库根新建任何散文件或散目录。**

根目录只允许存在既有条目（`README.md`、`AGENTS.md`、`Cargo.toml`、`.gitignore`、`crates/`、`docs/`、`qidiwork-docs/` 等）。
不确定放哪 → **先问用户，不要猜根目录**。

---

## 2. 编程项目（代码 / 测试 / 构建产物 / 技术文档）

| 内容 | 位置 | 说明 |
|---|---|---|
| 源码 / 单元测试 | `crates/<crate>/src/` 、`crates/<crate>/src/.../tests/` | 严格遵循现有 workspace 结构，不新造顶层目录 |
| 代码文档 / 设计文档 / 审计报告（编程类） | `docs/<主题>/` | 例如 `docs/memory-optimization/方案.md` |
| 构建产物 | `target/` | 由 cargo 管理，**禁止手工往里塞任何自造文件** |
| 临时脚本 / 日志 / 沙箱 / 一次性验证 | `target/tmp/<主题>/` 或系统 `%TEMP%` | `cargo clean` 会回收 target/tmp，不污染仓库 |
| 集成/回归测试数据 | 跟随所属 crate 的 `tests/` 或 fixtures 目录 | 不放根目录 |

---

## 3. 办公项目（方案 / 标书 / 审计 / 资料 / 纪要 等非代码交付物）

统一根目录：**`qidiwork-docs/`**。一个项目/主题一个子目录，同项目所有文件（含中间稿）都进同一子目录。

| 类型 | 位置 | 命名规范 |
|---|---|---|
| 方案 / 标书 / 投标文件 | `qidiwork-docs/方案标书/<项目名>/` | `YYYY-MM-DD-名称.docx/.pdf` |
| 审计 / 评审 / 验证报告 | `qidiwork-docs/审计评审/` | `YYYY-MM-DD-主题-审计.md` |
| 资料 / 参考文献 / 素材 | `qidiwork-docs/资料/<主题>/` | 自描述名 |
| 会话结论 / 交接纪要 | `qidiwork-docs/会话纪要/` | `YYYY-MM-DD-主题.md` |
| 其它办公产物 | `qidiwork-docs/<类别>/<主题>/` | 先归类再落盘 |

规则：
1. **跨会话继续的任务**：先找 `qidiwork-docs/` 下已有的项目子目录接着写，**不要另开新目录**
2. 中间稿与成品同目录共存，用日期前缀区分版本，不删历史稿
3. 交付后的大文件（如已交付标书）不主动删除，由用户决定归档

---

## 4. 临时文件

- 一次性脚本、探针、日志 → `target/tmp/<主题>/` 或系统 `%TEMP%`，**绝不进仓库根、绝不进 qidiwork-docs/**
- 需要保留的验证证据（测试输出、报告）→ 按 §2 或 §3 归位后，临时原件可删

---

## 5. 记忆与交接

- 需要**跨会话记住**的结论/偏好 → `/remember` 或 `/flush`（记忆存于 `C:\Users\ASUS\.qidi\memory\`，与仓库无关，任何项目共享）
- 大型任务的方案与最终结论 → 除 memory 外，同时落 `qidiwork-docs/会话纪要/`
- 换会话/新开会话前，若有未沉淀结论 → 先 `/flush` 再走

---

## 6. 历史遗留说明

根目录现存的 `tmp_relay/`、`9.22方案/`、`audit-*.json`、`review_tmp/` 等为历史会话遗留：
- **不要往这些目录追加新文件**
- 新文件一律按 §2 / §3 落位
- 历史遗留的归档/清理由用户另行决定

---

## 7. 模型与推理强度

| 操作 | 命令 |
|---|---|
| 切换模型 | `/model <模型名> [effort]`（下拉中也可选强度） |
| 当前会话推理强度 | `/effort <low\|medium\|high\|xhigh> [--save]`（`--save` 写入 `[models].default_reasoning_effort`；`max` 是 `xhigh` 的别名） |
| 启动全局覆盖 | `qidi --reasoning-effort <level>` |
| 查看当前 | `/session-info`、`/context` |

注意：仅对支持推理的模型生效（服务端目录 `supportsReasoningEffort`）；`/effort` 默认会话级，`--save` 才持久化。

## 8. Skills（技能）

- 触发：`/<技能名>` 或自然语言触发词；技能库在 `C:\Users\ASUS\.qidi\skills\`
- 技能 frontmatter 可带 `effort:` 指定该技能的推理强度
- 创建/修改技能用内置 `/create-skill`
- 常用：办公全家桶（office-tools / bid-* / seal-extractor / pdf-to-word-ocr）、编程流程（check-work / auto-bug-fixer / python-refactoring）、编排（dev-orchestra / find-skills）

## 9. 子代理编排（乐团模式，实测有效）

| 角色 | 模型 / 类型 | 用途 |
|---|---|---|
| 编码手 | `spawn_subagent(model="DeepSeek-V4.1-Flash-222", capability_mode="all")` | 按规格书写码+自测；**并行前提：任务文件零交集**（同 crate 不同文件可并行，同 crate 高内聚改动合并给一人） |
| 审计 | `GLM-5.3-222` / `step-5-preview` / `KIMI-K3-222`（resume_from 续会话保留上下文） | 双模型交叉审计；结论分歧时以代码证据裁决 |
| 只读探查 | `subagent_type="explore"` / `plan` | 只读侦察 |
| 隔离 | `isolation="worktree"` | 需要隔离时用（有冷构建代价） |

铁律：指挥官不写码（写规格/审计/质检/提交）；编码手不 commit；每项修复必须有对应测试。

## 10. 环境硬约束（本机实测，新会话必读）

1. **本机无 `rg`**；内置 grep 工具在本环境**常返回空（不可靠）**→ 用 `git grep` 或 `read_file`
2. **PowerShell 不支持 `&&`**（用 `;`）；控制台 GBK 中文乱码 → `Get-Content -Encoding UTF8`
3. **禁止全仓递归扫描**：`target/`、`mem-log/`、`Library/` 会让命令卡死
4. `Format-Table` 输出有缓冲错位——重要判断以具体值/行号为准，勿凭表格位置
5. `read_file` 只能读 workspace 内的文件（session 日志等 C 盘文件用终端读）
6. 有 25+ 常驻 python/node 进程（用户其它会话的活）——**绝不擅自杀进程**；G 盘 `target/debug/quant-platform/` 可能被另一会话占用（文件锁）
7. pre-commit 钩子有密钥扫描；`Format-Table`/后台任务输出常延迟，以日志文件为准

## 11. 构建与测试

| 操作 | 命令 | 耗时/备注 |
|---|---|---|
| 出产品 exe | `cargo build --release -p cf-pager-bin --bin qidi -j 4` | 冷 30-90 分钟；有缓存快；**运行中的 qidi.exe 无法覆盖 → 先改名旧 exe（Windows 允许改名不允许覆盖）** |
| cf-memory 测试 | `cargo test -p cf-memory --lib` | ~2-10 分钟 |
| **cf-shell 测试** | `$env:CARGO_TARGET_DIR='C:\qidi-cfshell-test'; $env:CARGO_PROFILE_DEV_DEBUG='0'; cargo test -p cf-shell --lib -j 4` | **热缓存全量 ~2.5 分钟**；`DEBUG=0`+串行是内存 OOM 的解药（本机 16GB，cf-shell 测试二进制直跑必 OOM） |
| cf-pager 测试 | `cargo test -p cf-pager --lib -j 4` | 冷链 15-25 分钟 |
| 快速全局校验 | `cargo check --workspace` | 0 errors 为过 |
| 清理 | `cargo clean` | target 曾膨胀到 114GB，定期清；先备份 `target/release/qidi.exe` |

注意：cf-shell 的 **test profile** 在 G 盘 target 直跑会 OOM——一律走上面的 C 盘热 target 方案。
