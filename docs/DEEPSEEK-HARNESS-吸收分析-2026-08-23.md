# DeepSeek Harness（dsh）吸收分析 — v2（审计修订版）

日期：2026-08-23
状态：**已经 KIMI-K3 独立审计并修订**；用户已批准执行，附加条件：自身不崩、可恢复（见第 5 节）
对象：deepseek-harness @ b150a55（v0.1.1-rc.2，DeepSeek 官方开源 agent harness）
本地克隆：`%TEMP%\ds-harness-research\deepseek-harness`
v1→v2 变更：修正 4 处事实错误（见 0.1）；重排优先级；补安全面；新增防自崩/恢复设计（第 5 节）

---

## 0. 事实基线（v2 修正后）

### 0.1 v1 被审计推翻的四个论断（保留记录，防止重蹈）

| v1 论断 | 实况 | 影响 |
|---|---|---|
| "没有任何记录能证明当初模型看到了什么" | **错**。`system_prompt.txt`（acp_session.rs:1326）、`prompt_context.json`（:1235，注释明言 deterministic re-rendering）、压缩工件 `compaction_requests/{id}.json`（compaction.rs:1996-2062）都已存在 | request_header 从"补空白"降为"把覆写式快照统一成事件序列"，价值仍在但更窄、更便宜 |
| "fork 是整目录复制" | **错**。`copy_session_data_sync`（storage/jsonl/mod.rs:682-790）是逐文件变换重写：summary 重建、updates 改 session id、cwd 变换、strip_reasoning | 回放车道 fixture 设计需考虑 fork 后代与源会话的 id 改写 |
| "select.rs 被 intra/inter/code 三种压缩共用" | **半错**。tool-pair-safe 属实，但唯一调用点是 intra_compaction；lib.rs 注释说的是 `append_reminder_block` | 无实质影响，修正认知 |
| 压缩括号卖点"杜绝虚假成功" | **论证错位**。现状 checkpoint 先于 replace 写入（compaction.rs:1567→1594），崩溃永不产生"声称完成但摘要缺失"。真实增量 = 崩溃可归因 + resume 主动重压缩 | 压缩 start 标记从 P1 降 P3，验收重写 |

### 0.2 修正后的事实清单

**已有的（不需要建）**：
- `updates.jsonl` 仅追加事实流（单会话生命周期内；fork 后代是变换重写，不继承源文件字节）。读取端 corruption-tolerant：坏行跳过 + warn（storage/jsonl/mod.rs:322-361）
- `chat_history.jsonl` 整文件原子重写（tmp+rename，含双写者竞态处理，acp_session.rs:1258-1311）
- tool-pair-safe 压缩选段（cf-compaction/select.rs，intra 使用）
- 压缩完成点标记 CompactionCheckpoint（先于替换写入）+ 压缩 LLM 请求完整工件落盘
- 系统提示词/提示上下文覆写式快照（system_prompt.txt / prompt_context.json）
- **ChatReducer**（remote/pull.rs:187-259）：从 updates.jsonl 重建 chat_history 的完整机制，含 CompactionCheckpoint 截断重置——"chat_history 可重建"已有现成实现，只差泛化
- CI：cargo-deny 阻断、test 阻断、fmt/clippy 有意 advisory（上游分叉 1266 fmt diff，保持 merge 能力）
- 24310 个测试、QIDI_HOME 环境变量隔离真实数据（cf-config/paths.rs:75-77）

**真正缺的（要补的）**：
- 请求信封是**覆写式单点快照**，非事件序列；无工具 schema 列表、无采样 config 记录、不在 updates.jsonl 内
- updates.jsonl 无格式版本标记
- 压缩无 start 标记（诊断归因缺失，非正确性缺失）
- 超大工具输出硬清空后模型永久失去检索能力
- 无端到端回放测试车道
- token 计量全靠启发式估算，无 usage 锚定校准

---

## 1. 吸收项（v2 优先级）

### P0 —— 基础防护（合计 1 天）

1. **模板加密漂移门禁 + 行尾三件套**：
   - encrypt_templates.py 改 `write_bytes` 固定 LF 输出（当前 `write_text` 文本模式 Win 产 CRLF / Linux 产 LF）
   - 新增 `.gitattributes` 钉住 `crates/codegen/cf-agent/templates/*.md` 与 `prompt_encrypted.rs` 的行尾（仓库当前无 .gitattributes，模板是 CRLF）
   - CI 门禁（跑脚本 + `git diff --exit-code`）**先限定 windows-latest**（与当前行尾字节一致的一侧）
2. **docs/defensive-patterns.md**：种子 6 条（dunce::canonicalize / Set-Content UTF-8 / curl 别名 / 正交结果独立上报 / lstat-then-unlink junction / 模板加密约束），每条注明来源。后续新规则从 postmortem 流程产出（见 P3-⑩）
3. **两项只读审计**：(a) timedOut/signal/exitCode 正交性（background_task.rs:49-55 已是独立字段，重点查 Job Object 终止路径是否可能 exit 0）；(b) Inbox 三动词语义对照（followup/steer/inject vs cf-interjection-core/cf-prompt-queue 的排队/注入边界）

### P1 —— 请求信封事件化（1-2 周）

4. **request_header 事件**：动机=覆写快照→事件序列 + 补工具 schema/config。
   - 落点：cf-shell 采样调用点，对 `build_conversation_request` 返回的 ConversationRequest（自带系统提示词+tools+model/temperature/top_p）算 sha256，与上次记录比对，变化则经 **`persist_xai_update_only`** 通道追加（updates.jsonl 的真实写入路径，updates.rs:648-651）；**cf-chat-state 的 ChatPersistence 三方法（只管 chat_history.jsonl）完全不涉及**
   - 第一阶段只落 hash + 工具名列表 + config；**全文落盘推迟**，须先过第 5.4 节同步面安全评估
5. **格式版本行**：必须包成合法 envelope（`{timestamp, method:"_xai/session/format_version", params:{version}}`），不能用裸 `{"type":...}`（会在 envelope 解析失败后当坏行跳过，每次加载产生 warn 噪音）。读取端：默认 version=0；未知版本 warn 一次继续读（corruption-tolerant 已有先例）。存量迁移无需处理：旧日志无版本行 = version 0
6. **验收**：同一会话 resume 后 request_header hash 与首跑一致或差异可解释；旧二进制读新日志行为不变（未知行跳过）。

### P2 —— 能力与测试（1-2 月）

7. **ChatReducer 泛化（v1 的 P3-10 提升）**：把 remote/pull.rs 的重建机制抽成可复用入口，debug 构建断言"chat_history 可从 updates + 压缩段归档重建"。这是全案新的性价比之王——机制已存在
8. **Spill 存储**：硬清空升级为 spill 定位符 + 首尾预览。落点 `~/.qidi/spill/<session-sha256>/`（**与 session 目录同级安全姿态**——现状 Windows 分支本就无 ACL，不假装达成 0700）；`create_new(true)` 排他创建；**生命周期必须随方案定**：随 delete_session 清理 + 启动孤儿清扫（spill 在 session 目录外，不清就是永久明文孤儿）。保存失败回退内联，不算 isError
9. **回放测试车道**：录制入口 = 带 key 真实跑一场会话后拷贝 updates.jsonl 作 fixture。断言对象是**派生 chat_history**（ConversationItem 序列化不含 envelope 噪声）；压缩触发点断言依赖 token 估算确定性（对估算算法版本敏感，写明这是特性也是维护成本）。fixture 需剥离 timestamp 噪声（envelope 自带，dsh 用"规范打包行"剥离，我们至少要归一化时间戳）
10. **token-meter usage 锚定**：用 provider 返回的真实 usage 校准 EstimatedItemTokenCounter 锚点（baseline.kind: usage|estimated）。依赖 P1-4 的 config 记录配套。压缩时机从纯启发式变为真实值锚定

### P3 —— 长尾

11. **压缩 start 标记**：仅诊断归因 + resume 触发重压缩（**不做"回滚"**——checkpoint 先于 replace，start 与 checkpoint 之间崩溃时磁盘本就一致）。补 dsh 的 end-seed 语义：前一生命周期遗留的未闭合 start 视为陈旧证据，忽略
12. **Agent Notes 精简版 + postmortem 合并**：notes 目录（Problem/Decision/Alternatives considered/Consequences 骨架）+ postmortem 三门槛（隐蔽/系统性/重新发现代价高才写）。种子从 PROJECT_REVIEW 和 git 历史提取

### 不吸收清单（v2 无变化，补两条标注）

- Cordis 插件树、100% 覆盖率门禁、三域事件模型重构、双语文档机器、Web UI 相关、Agent Teams/E2B/Typert/workflow、dsh 全量门禁集 —— 理由同 v1
- **标注已知未评估**：dsh 的 approval / permission-presets 子系统（我们有 permission/manager.rs 但未对照评估，留待后续）
- ~~Inbox 审计无归属~~ → 已归入 P0-3

---

## 5. 防自崩与恢复设计（用户附加条件，执行前置）

**原则：任何时刻，正在运行的旧二进制 + 已有的会话数据 = 永远可用的一层。我们所有改动在这层之上做加法。**

### 5.1 物理隔离（已核实）

- 运行中的 qidi 实例 = `G:\qidicode\target\debug\qidi_new.exe`（主检出 b7656cd 编译）。本 worktree 的任何改动**不影响运行中的二进制**，直到显式重新构建并替换
- 主检出不碰：所有工作在 worktree `2026-08-23-bf4509c1` 的专用分支 `feat/dsh-absorption` 上进行
- worktree 里发现的非本次改动（default_models.json + bak 文件，与主检出一致，系 qidi 模型同步机制产生）：**不提交、不回滚、不 git add -A**。每次提交只 add 明确的文件路径

### 5.2 数据隔离（已核实机制）

- 测试一律经 `QIDI_HOME` 环境变量指向临时目录（cf-config/paths.rs:75-77 支持；git_contention_e2e.rs 等已有先例）。真实 `~/.qidi/sessions` 永不被测试触碰
- P1 动手前对 `~/.qidi/sessions` 做一次快照备份（这是唯一一次整体备份；日常靠 5.3 的格式兼容）

### 5.3 格式演化铁律（防自崩的核心，每条 PR 检查）

1. **加法唯一**：updates.jsonl 只新增事件类型，永不改写/删除已有行；新事件的写入是"崩溃时最坏多一行截断"——读取端 corruption-tolerant 已有（坏行跳过+warn）
2. **旧读新必须无害**：旧二进制遇到新事件行 = 未知 method 跳过，行为不变（现有 fallback 链已保证，需为每个新事件加回归测试）
3. **新读旧必须无感**：新二进制读无版本行日志 = version 0 走原路径
4. **chat_history.jsonl 重写保持原子**（tmp+rename 现状保留）
5. **迁移即测试**：每个持久化 PR 附"加载旧格式 fixture"测试——启动时收割 2-3 个真实旧会话目录冻结为 fixture（含一个带压缩 checkpoint 的、一个 fork 过的）

### 5.4 安全面前置评估（P1-4 全文化、P2-8 spill 的闸门）

- 系统提示词含 memory 注入（用户私有内容）。已存在的上传面：turn 管道 gzip 上云（trace.rs:1004-1044，有开关）、subagent 场景 system_prompt.txt/prompt_context.json 直传 GCS（handle_request.rs:1674-1686）。request_header **全文化**前必须评估这些同步面；hash 先行正是为了绕开这个雷
- spill 文件含原始工具输出（可能带密钥）：清单独的上传面豁免 + 生命周期（5.5）
- **回滚预案**：任何格式改动若导致线上会话加载异常 → 回退到本分支前一提交重建二进制即可恢复（因为旧数据从未被改写，加法铁律保证旧二进制永远读得回）

### 5.5 执行纪律

- 每 P 级独立 commit，commit 信息列明触碰的持久化面
- 持久化相关 PR 合并前过三问：旧二进制读新日志？新二进制读旧日志？kill -9 在写入中途？——三问都有测试或论证
- 主检出 main 分支只通过显式 merge 更新，且 merge 后先跑一轮 `cargo test -p cf-shell`（持久化测试集中在该 crate）再使用新二进制

---

## 附：证据锚点（v2）

- 本仓：cf-config/paths.rs:75-77（QIDI_HOME）、cf-shell session/{acp_session.rs:1230-1334, compaction.rs:1567-1620/1996-2114, storage/jsonl/mod.rs:276-361/682-790}、remote/pull.rs:187-259（ChatReducer）、updates.rs:648-651（persist_xai_update_only）、trace.rs:1004-1044（上传面）、background_task.rs:49-55
- dsh：docs/subsystems/{session,compaction,spill,token-meter,invariants}.zh.md、docs/{testing,defensive-patterns}.md、docs/postmortem/README.zh.md、AGENTS.md
- v1 文档及 KIMI-K3 审计全文见会话记录（86 次工具调用取证）
