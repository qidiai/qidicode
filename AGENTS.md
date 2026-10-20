# AGENTS.md — qidicode 工作区约定（每次会话与子代理必读）

> 本文件是本仓库的**工作约定与实测快照**（活文档，维护协议见 §0）。任何会话、任何子代理在创建文件前必须先读此文件。
> 经 Step5 + K3 双审计修订（2026-09-25，r2）。本仓库为生产工具——本文件有错会系统性误导所有会话：发现不符，**先修本文件再继续**。

---

## 0. 本文件是活文档（维护协议）

本文件记录的是**编写时点的实测快照**，不是永恒法律。环境会变，本文件必须跟着变。

**裁决顺序：实测事实 > 本文件 > 会话习惯。**
任何会话（含子代理）发现本文件与实际不符 → **应当立即修订本文件并 commit**，这优先于继续遵守过时条目。遵守一条已知过时的规则不是"守规矩"，是传错。

**必须触发修订的时机**：
- 硬件/环境变化：内存扩容（§11 的 cf-shell OOM 配方可能不再需要）、磁盘增减、安装了 `rg`、git 钩子增删（§10 密钥扫描条款随之更新）、工具链升级
- 实测推翻文件：命令行为、耗时数值、测试基线数字与文件描述矛盾 → 以实测为准并回写
- 模型 slug / 技能 / MCP 服务增删（§7/§8/§9）
- 远程仓库或推送策略变化（§10）
- 文件落位目录结构调整（§2/§3）
- 返工/审计闭环 → durable 教训合入 `docs/audit-rubrics/RUBRICS.md`（流程见 audit-rubrics 技能）

**修订方式**：改完即 commit，信息注明 `AGENTS.md 维护: <触发原因>`；涉及 §10 推送策略等生产纪律的**语义变化**，先经用户确认再落盘。

**最易过时的节**（阅读时带时效意识）：§10 环境硬约束、§11 耗时数值与测试基线、§7/§8/§9 的模型与技能清单。其余章节（文件落位、提交纪律）相对稳定。

---

## 1. 文件放置总则（硬规则）

**🚫 禁止在仓库根新建任何散文件或散目录。**

根目录白名单：`README.md`、`AGENTS.md`、`Cargo.toml`、`Cargo.lock`、`.gitignore`、`build_release.ps1` 及既有目录（`crates/`、`docs/`、`third_party/`、`tools/`、`bin/`、`prod/` 等）。
**白名单之外的一切根级条目均为历史遗留**（见 §6）——不要读它们的结构做参照，不要往里写。
不确定放哪 → **先问用户，不要猜根目录**。

> **2026-10-17 纪律修订（用户确认）**：`qidiwork-docs/` 已从白名单和本仓库**永久移除**（历史已重写清除）——本仓库是对外公开的代码库，**只放代码与 §2 定义的工程技术文档**；办公/研究/个人文档一律不进本仓库，落位见 §3。

---

## 2. 编程项目（代码 / 测试 / 构建产物 / 技术文档）

| 内容 | 位置 | 说明 |
|---|---|---|
| 源码 / 单元测试 | `crates/<crate>/src/`、`.../tests/` | 严格遵循现有 workspace 结构，不新造顶层目录 |
| 代码文档 / 技术设计 / 代码评审报告 | `docs/<主题>/` | 例如 `docs/memory-optimization/方案.md` |
| 构建产物 | `target/` | 由 cargo 管理，**禁止手工往里塞任何自造文件** |
| 临时脚本 / 日志 / 沙箱 / 一次性验证 | `target/tmp/<主题>/` 或系统 `%TEMP%` | `cargo clean` 会回收 |

**归属判定（2026-10-17 修订）**：`docs/` 仅收**与代码演进直接相关**的技术设计、代码评审、构建/工具链文档。**研究资料、标书、纪要、个人文档、业务交付物一律不得进 `docs/`，更不得进本仓库**——落位见 §3。`docs/` 与已迁出的 `qidiwork-docs/` 曾有 4 个同名文件，以私有库版本为快照。

---

**产品竞品分析与吸收方案**：docs/absorption/（ADM 竞品分析+吸收方案 v2 三模型审计整合定稿，2026-10-19；含 WeCom 消息桥 P0 规格、qidi-update 技能、纪律互鉴、开放问题）。

## 3. 办公项目（方案 / 标书 / 审计 / 资料 / 纪要 等非代码交付物）

**统一根：私有库 `github.com/qidiai/qidiwork-docs`（Private），本地工作副本 `G:\qidiwork-docs-local\qidiwork-docs\`（2026-10-17 勘误：原写 `G:\qidicode-docs-local\` 实测不存在，真实副本为带 `qidiwork-docs` 子目录的本路径）。**
一个项目/主题一个子目录，同项目所有文件（含中间稿）都进同一子目录。

| 类型 | 位置 | 命名规范 |
|---|---|---|
| 方案 / 标书 / 投标文件 | `方案标书/<项目名>/` | `YYYY-MM-DD-名称.docx/.pdf` |
| 审计 / 评审 / 验证报告 | `审计评审/` | `YYYY-MM-DD-主题-类别.md` |
| 资料 / 参考文献 / 素材 | `资料/<主题>/` | 自描述名 |
| 会话结论 / 交接纪要 | `会话纪要/` | `YYYY-MM-DD-主题.md` |

**铁律（2026-10-17 用户确认，源于公开仓库误暴露事故）**：
- **办公/研究/个人文档绝不进入本公开仓库**（曾因误判远程可见性，157 个文档文件在 GitHub/Gitee 双公开仓库暴露 3 周，已重写历史清除）
- 文档的 git 工作流在私有库进行：`cd G:\qidiwork-docs-local` 后常规 add/commit/push（remote 为私有库 origin）
- 文档**不参与**本仓库的双推纪律（§10）——两边是完全不同的仓库
- 本地副本未 clone 到位时，先在 `G:\qidiwork-docs-local` 执行 `git clone https://github.com/qidiai/qidiwork-docs.git`
- `.gitignore` 已永久屏蔽 `qidiwork-docs/` 与 `MEMORY_SCOPE.md`，误建该目录也不会被提交

---

## 4. 临时文件

一次性脚本、探针、日志 → `target/tmp/<主题>/` 或系统 `%TEMP%`。需要保留的验证证据按 §2/§3 归位后，临时原件可删。**绝不进仓库根、绝不进文档私有库（`G:\qidiwork-docs-local\`）的提交面。**

---

## 5. 记忆与交接

- 跨会话要记住的结论/偏好 → `/remember` 或 `/flush`（记忆存于 `C:\Users\ASUS\.qidi\memory\`，与仓库无关，所有项目共享）
- **文档及时更新（2026-10-04 主理人原则）：结论即时落文档（夜报/纪要/NAS 档案），不依赖会话记忆——记忆会错，文档不会。** 大型任务当日内完成《会话纪要》；审计/裁定/交付当日落 NAS 档案并登记索引
- 大型任务的方案与最终结论 → 除 memory 外，同时落文档私有库 `G:\qidiwork-docs-local\会话纪要\`（git 提交推私有库，见 §3）
- 换会话/新开会话前若有未沉淀结论 → 先 `/flush`
- **自动兜底**：compact 出错前会自动落盘（`pre_compact_on_error-*.md`），`/flush` 是主动版
- **关键安心条款**：`cargo clean` / `git clean` / 仓库清理**物理隔离**于 `C:\Users\ASUS\.qidi\`（记忆、会话、技能、配置都在 C 盘，不受任何仓库清理影响）
- 测试绿门基线见记忆：`reference-qidicode-green-gates`（**2026-10-19 批次2修复后实测：cf-tools 2674 passed / 0 failed / 7 ignored；cf-pager 7060 passed / 0 failed / 6 ignored；cf-shell 全量（跳 10 项既有环境失败，见 §12 行注）5630 passed，改动面过滤 `mvp_agent::` 185 passed / 1 failed（既有 flaky）**；`cargo check --workspace` 0 errors）——先对照基线再判断"是不是我跑挂了"

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
- 推理强度默认：`ReasoningEffort` 枚举 `#[default] Medium`（`cf-sampling-types/src/types.rs`）；`SamplingConfig.reasoning_effort` 字段默认 `None`（未显式设置时不带 effort 参数）
- （勘误 2026-10-04）`Some(0.35)` 是 `memory_search` 的 `min_score` 默认（`cf-config/src/types.rs:2189/2343`、`cf-memory/src/search.rs:926`），与推理强度无关——本节原误植该数字，勿再混入

## 8. Skills（技能）

- 触发：`/<技能名>` 或自然语言触发词（`SkillInfo.when_to_use`）；技能库 `C:\Users\ASUS\.qidi\skills\`
- **技能库资产状态（2026-10-19 批次3 清理后）**：398 个 SKILL.md（原 432）。已清：6 个 malformed frontmatter、29 组同 scope 重名（删 34 个捆绑副本+4 个改名消歧）、1 个 `Skill.md` 大小写、9 个 BOM、`.bak`/孤立 references 残留；验收五项（malformed/大小写/BOM/重名/总数对账）全过，Essential 投影 29 白名单无损。**回滚备份**：`target/tmp/fix-batch3/skills-backup/`（463 文件，cargo clean 前先转移）。已知残留（记录未处理）：10 个脚本类 BOM（.py/.ps1/.xsd，PowerShell 5.1 的 BOM 有解码语义待单独裁决）、16 个 .bak、`_skill_mirror/`、420→368 个技能缺 triggers/when_to_use（生态缺口）、`handdraw-style` 等 4 个改名的产品显示名待用户复核
- 技能 frontmatter 可带 `effort:`（枚举含独立 `Max`，见 §7 分层）与 `model:` 覆盖
- 创建/修改技能用内置 `/create-skill`
- **子代理技能面实测（2026-10-16，DeepSeek 子代理 72 次调用深挖；根因经 GLM+step5 双席独立裁决修正）**：**子代理不能调用技能——差两层**。**（勘误 2026-10-17 深夜：①可见层快照已被 F2R 返工推翻——代码实证 `inject_subagent_baseline_listing`（spawn.rs:98）在子代理真实初始化点 spawn.rs:1011 与 session_setup.rs:104 调用，配套测试 subagent_skill_listing_spawn_tests.rs；F2R=commit 432cb8ee"技能清单注入缝搬至子代理真实初始化点(B-1)"、F2S=996f69a4"危险技能门禁默认禁止"。故子代理现有的是 **Essential 投影的基线技能清单**（非 0、非全量）；**激活层三层已落地（2026-10-19 全面测试实证：子代理工具面含 `skill` 调用工具——`default_qidi_build_toolset()` 内 config.rs:303 接线，F1=commit 362eb07c，指挥官任务书无需自带技能要点；危险门禁 subagent_gate.rs 默认拒 qwen-image-lowvram/pdf-to-word-ocr/seal-extractor/bid-/**wecom-** 前缀（wecom- = 2026-10-20 v3 决策 1：堵子代理经 wecom-unified 直发微信、绕过 wecom-bridge daemon 审计的外泄面），先于投影作用于 listing 与运行时集）**。以下为 10-16 深挖时的原始快照，真因部分仍有效：）**①可见层（10-16 时点）：子代理技能清单注入 = **0**。真因（双席裁决，早版"账本继承"叙事已证伪）：**子代理 spawn 不经 initialize/rebuild 注入缝**（handle_request.rs 只发 CopyFile/Prompt，conversation 由 install_system_prompt+replace_conversation 直接装配），且 **finalize 预填 announced_names（startup 全量）**使 seed 的 pending 门（`has_skills && announced_names.is_empty()`）不放行 → 工具调用 drain 无货；②激活层：子代理工具面（`default_qidi_build_toolset()`）只有安装型 `deploy_skill`，技能调用工具 `OpenCodeSkillTool`（id=`"skill"`）只存在于 `opencode_toolset()` 未接入——**设计应支持、实现未接通（F1 已接线，commit 362eb07c，审计通过）**；③内容层兜底 ✅：`read_file` 出 workspace 边界被拒，但 `run_terminal_command`+`Get-Content -Encoding UTF8` 可读任意 SKILL.md。子代理需技能时由指挥官把 SKILL.md 要点写进任务书，或待 F2 返工（缝搬子代理真实初始化点）落地
- **bid-content-diff（标书内容比对，标书技能族配套）**：每一版修改前备份（`bid_backup.py`）、修改后比对（`bid_diff.py`：哪些修改了+有没有修改坏别的内容=数字口径漂移/表格行数/图片引用/禁用词/编号连续/关键数据消失/评审因素标注回归检查）、改坏一键回滚（`bid_restore.py`，回滚前自动安全备份）。**加载规则：仅随标书技能族（bid-write/construction-tender-proposal/hefei-bid-review/bid-drawing）加载；极简模式、非标书任务一律不加载**。铁律：不备份不改内容，每轮修改=备份→修改→比对→(SAFE/REVIEW/BROKEN处置)闭环。2026-10-17 创建并全链路沙箱验证（注入4类损坏全部精准检出）
- 常用：办公全家桶（office-tools / bid-* / seal-extractor / pdf-to-word-ocr）、编程流程（check-work / auto-bug-fixer / python-refactoring）、编排（dev-orchestra / find-skills）、安全（pre-commit-secret-check / secret-scan / check-secrets）、本地AI生图（qwen-image-lowvram：1660 Ti 低显存 Qwen-Image-2.1 + 标书效果图流水线，含 ComfyUI 部署/踩坑/PIL 叠字/元数据清零全套）
- **主会话 skill 可达性（2026-10-20 修复，FAIL-1）**：主会话默认 agent=`qidi-build-plan` → 预设 `cf-tools-plan` 原为**手抄工具清单**，F1（362eb07c）只给 `default_qidi_build_toolset()` / `opencode_toolset()` 加了 `OpenCode:skill`，plan 系预设因此从未拿到 skill 栈（`deploy_skill` + skill 调用），且还漏了 `wait_tasks`。现 `qidi_build_plan/plan_no_subagents/ask_user/orchestrator_toolset()` 一律**从 default 派生后 push 各自 delta**（`cf-agent/src/config.rs`），未来 default 新增工具自动共享。回归 `every_main_session_toolset_ships_skill_invocation` 覆盖四个主会话预设 + orchestrator。**新增预设时不要再手抄清单**。`qidi_build_concise_toolset()` 例外（SFT/RL 精确最小面，有意手写）。

## 9. 子代理编排（乐团模式，实测有效）

| 角色 | 模型 / 类型 | 用途 |
|---|---|---|
**⚠️ step-5（阶跃）订阅时限（2026-10-17 用户告知，勘误 v2：非 apigoto）**：step-5 订阅 **2026-10-20 过期**——step-5 系模型（step-5-preview-222/step-5-preview/opencode-go step-5-preview-free）届时失效或降级。222 池（DeepSeek/K3/GLM 系）不受影响。step-5 位替代：luyou sonnet-5-5（今日实测可用）/zen 免费位其他模型/kimi 系。| 编码手 | 主力 `spawn_subagent(model="deepseek-flash-222", capability_mode="all")`（2026-10-16 用户指令：编码手切 deepseek-flash——DeepSeek 官方 V4.1 Flash 的**正式调用名**，10-17 起 1017 版专为 WorkBuddy/CodeBuddy/Harness 编码框架优化；sensenova 托管免费层 RPM 限流**偶发 429，重试即可**；DeepSeek 系 slug 的 spawn 序列化畸形率≈0，天然避开 10-15 发现的 model 参数 bug）。前史：ling-3.1-flash 10-02~10-15（永久下线，多模态看图偶可特批）→ DeepSeek-V4.1-Flash-222 10-15~10-16（`deepseek-v4-pro-222` 保留为复杂/高风险升级位） | 按规格书写码+自测；**并行前提：任务文件零交集**（同 crate 不同文件可并行，同 crate 高内聚改动合并给一人） |
**?? luyou ????????2026-10-17 ?????????????**??????????**?????????????????**??? intern key ??ocr-4 ?????? 429?quota exceeded ? key??? codely ?? free ??**??? GLM ??codely-core?**?K3 ????free ?????? pro ? GLM ????? K3 ????? **pro/????'39640','58863'?????????K3 ?????**?? K3 ??? = pro/??????????? 10-20 ????????????? 429??? ?????**K3/kimi ? 429 = pro ?????????/????GLM ? 429 = free ????????? pro??DeepSeek ? = ?????deepseek-v4-pro ???**?

| 审计 | `GLM-5.3-222`（首席，最深）/ `claude-haiku-5-5-ago-222`（高频巡查官）/ `step-5-preview`（**2026-10-19 用户决定：KIMI-K3-222 退役；claude-haiku-5-5 上线测试通过，顶替审计/第二席**——qidi headless 通道 366s 完成 R-A 实现审计：REJECT + 9 重要 + 10 次要，file:line 抽查 5 处 4 坐实 1 部分失实、零造假，grep 故障自愈改 python 脚本；分工 = GLM 任高风险深审、haiku 任每 commit 例行巡查，快 2-3 倍且输出价便宜 34x）。**巡查二席：`Qwen/Qwen3.8-Flash-Next`（ModelScope 魔粒池，条目 `qwen3.8-flash-next-ms-222`）——2026-10-19 试毒通过转正：审“面板②”功能 ACCEPT WITH NITS，2 重要（J-1 徽章展开态被 60s 轮询打回、J-2 刷新模态 lost update）+ 8 次要全部实证（跑 pytest 验证、rubrics 3 PASS/1 FAIL/1 附条件）。分工：haiku 任上线门禁（严）、Qwen 任日常巡查（准+免费池），GLM 任首席深审**。`kimi-k2.7-code-highspeed-222` 降级备选；GLM 席位遇网关故障可换 `GLM-5.3-FLASH-222`（opencode/glm-5.3-flash，免费池，代价=禁 run_all/附件，无 resume） | 双模型交叉审计；结论分歧时以代码证据裁决 |
| 只读探查 | `subagent_type="explore"` / `plan`（只读侦察/方案规划；**2026-10-19 起可指定 `Qwen/Qwen3.8-Flash-Next`（modelscope 魔粒池，slug `qwen3.8-flash-next-ms-222`）——零成本白嫖池，6/6 推理基准支撑规划质量；重规划仍建议 GLM/haiku**） | 只读侦察/规划 |
| 隔离 | `isolation="worktree"` | 需要隔离时用（有冷构建代价） |


### 笔记本侧 AI 工具调用双路径（2026-10-20 用户裁定：**默认走本机**，两条路都留）

| 路径 | 用法 | 定位 |
|---|---|---|
| **① ai-bridge 本机链（默认）** | `ai-bridge__dispatch(tools=["qoder", "opencode-flash", "codely"])`（hub 9800，本机执行，autoFallback 自动接力） | **默认**。qoder=Qoder 订阅全家桶（DeepSeek V4 Pro/Qwen3.8 Max/GLM-5.3/Kimi K3/M3.1）；opencode-flash=deepseek-v4.1-flash 快档；codely=兜底。零 LAN 跳转、失败自动降级、2026-10-20 实测 3/3 通 |
| ② 台式机 relay 条目（备用） | config.toml 的 `qoder-*` 三条（经 192.168.1.222:8765 → 台式机 qoder2api-hub 8790） | 备用。仅在本机链整链失败或需台式机侧特有通道时用；受台式机开机/负载/SMB 状态制约（历史痛点：两台同时调用慢且易失败） |

**纪律**：派子代理/任务分发**默认走 ①**；仅 ① 整链失败（qoder/opencode-flash/codely 全挂）才降 ②。
**注意**：qodercli（网页版 CLI）与 Qoder 桌面 IDE 是**两个应用、登录态独立**——本机链的 qoder 工具依赖 qodercli 已登录（2026-10-20 已登，若报 Not logged in 则重新 `qodercli /login`）。

### 笔记本侧 AI 工具调用双路径（2026-10-20 用户裁定：**默认走本机**，两条路都留）

| 路径 | 用法 | 定位 |
|---|---|---|
| **① ai-bridge 本机链（默认）** | `ai-bridge__dispatch(tools=["qoder", "opencode-flash", "codely"])`（hub 9800，本机执行，autoFallback 自动接力） | **默认**。qoder=Qoder 订阅全家桶（DeepSeek V4 Pro/Qwen3.8 Max/GLM-5.3/Kimi K3/M3.1）；opencode-flash=deepseek-v4.1-flash 快档；codely=兜底。零 LAN 跳转、失败自动降级、2026-10-20 实测 3/3 通 |
| ② 台式机 relay 条目（备用） | config.toml 的 `qoder-*` 三条（经 192.168.1.222:8765 → 台式机 qoder2api-hub 8790） | 备用。仅在本机链整链失败或需台式机侧特有通道时用；受台式机开机/负载/SMB 状态制约（历史痛点：两台同时调用慢且易失败） |

**纪律**：派子代理/任务分发**默认走 ①**；仅 ① 整链失败（qoder/opencode-flash/codely 全挂）才降 ②。
**注意**：qodercli（网页版 CLI）与 Qoder 桌面 IDE 是**两个应用、登录态独立**——本机链的 qoder 工具依赖 qodercli 已登录（2026-10-20 已登，若报 Not logged in 则重新 `qodercli /login`）。
铁律：指挥官不写码（写规格/审计/质检/提交）；编码手与子代理**禁止 commit**；每项修复必须有对应测试；审计规格须按 **audit-rubrics 技能**执行（先自由审查、后对照清单，适用条目注入 ≤5，报告落文档私有库 `G:\qidiwork-docs-local\审计评审\`，见 §3），返工闭环后教训合入 `docs/audit-rubrics/RUBRICS.md`。

**子代理失败回报契约（2026-10-20 修复，B1）**：`insert_pending` 之后的失败**必须**走 `handle_request.rs::fail_post_pending`（`PendingGuard::set_error` + `send_failure`），不得只调 `send_failure`——只调它时后台调用方（oneshot 接收端早已丢弃）只能看到 `PendingGuard::drop` 的泛化兜底 `"Subagent failed during initialization"`，真实原因（resume 仍在运行/未找到/身份冲突、cwd 非法、源模型缺失、resume 拷贝中止）全丢。失败记录进 `completed` 快照 + **恰好 1 条** completion summary；**条数由 `surface_completion` 决定，`set_error` 只保证消息保真**（勿再把"防重复呈现"记在 `set_error` 账上）。回归：`post_pending_*_records_reason_{foreground,background}` 6 例（`tests/mod.rs`）。另：`is_valid_resume_id` 的哨兵容忍（只排除 `""`/`null`/`none`/`undefined`/空白）是**设计**，非标准 id 走 not-found 分支属受支持行为，**不要**改成显式报错。

**spawn 路由实测注意（2026-10-11 深挖）**：
- spawn 子代理的模型调用**与本机 config.toml / auth.json 解耦**（走平台内置目录+独立凭据，细节未全还原）：同模型手动直连 200 而 spawn 可能 401/503（实测：kimi-k2.6 手动 200 / spawn 503 `no usable credential for provider 'intern'`；kimi-k2.7-code 手动 200 / spawn 401 `Invalid token` from api.openai-next.com）——**派活前先 spawn 小任务验连通，勿以手动直连结果为准**
- **spawn_subagent 的 `model` 参数序列化 bug（2026-10-15 实测 10+ 连败）**：`model` 值的引号被序列化层稳定吞掉（`"model": KIMI-K3-222` 裸 token → Invalid JSON，改参数顺序/转义/换 slug 全无效；**不带 model 继承父模型可正常 spawn**）。**根治通道：qidi CLI headless——`qidi.exe --prompt-file <任务书> -m "<模型>" --permission-mode ... --cwd ... --output-format plain`**（同走云端中转，实测 KIMI-K3-222 审计 170s 成跑；编码手派工用 `--permission-mode bypassPermissions`+相应工具面）。**B5 门坑**：`bypassPermissions`（=yolo）无 OS 沙箱时 CLI 拒启——解法按安全等级选：`--yolo-tools <逗号白名单>`（收窄自动批准面，推荐）或 `--ack-no-sandbox`（全放行，慎用）或 `--sandbox <profile>`；**2026-10-19 补实测：本机 config.toml `[ui] permission_mode="always-approve"` 即触发 B5 门——headless 默认跑法必挂（0.5s 拒启），任何 headless 任务书都须带 `--yolo-tools` 收窄**；另注意 PowerShell `>` 重定向默认写 UTF-16，报告落盘后须转 UTF8 再读。~~指针式短 prompt 可降低序列化风险~~（**勘误 2026-10-16 三测：短报文不豁免、值加尾随空格强制引号也不豁免——与报文大小无关；slug 选择性实存（同会话 DeepSeek-V4.1-Flash-222 spawn 正常成跑）。再勘误（用户口径）：故障为**间歇性非确定性**——另会话 GLM-5.3-FLASH-222 第三次重试成功；机制=参数 JSON 由会话模型逐 token 采样生成（无 schema 约束解码），丢引号是概率事件，畸形率按（会话模型×slug）分布（KIMI-K3-222 极高、DeepSeek ≈0）；产品的错误回带闭环（原始参数+列号塞回上下文）使重试自带 in-context 修正机会——**重试本身是有效应急绕法**，根治=宽容解析层（2026-10-16 落地，见当日 commit）**）
- **2026-10-17 标书编写实测补记**：①DeepSeek-V4.1-Flash-222 走 luyou 池当日 401（upstream returned 401 / no account could serve it）6 连败，不带 model 参数（继承父模型）可正常 spawn；②**父模型 step-5-preview 对大输出必截断**（max_tokens_truncation）——凡要求子代理重写整章/整文件的任务 100% 失败，必须改小步模式（逐节 search_replace、单次输出≤600字），并防跑偏（有个例转去 web 浏览需 kill）；③**并行写章的跨章口径冲突是最大质量风险**（两章各自自洽但互相矛盾：工种人数/工期窗口/曲线峰值），指挥官合并后必须做全案数据对账；④50 页硬限制的容积手段：表格 9.5 磅+12 磅行距、去 cantSplit、标题单倍行距、封面目录合一（本次 76→53 页）

  - **DeepSeek 401 备用通道（2026-10-17 下午实测）**：luyou 池 401 时 deepseek-v4-1-flash-260910 同样 401；可用位为 deepseek-flash?**2026-10-17 ????????????????? API ?**??deepseek-flash ?????????????DeepSeek ???? docs/ops/deepseek-flash-evolution.md?（200 通过）与 deepseek-flash（200 通过）；config 中 model 字段应写 deepseek/deepseek-v4.1-flash。
- luyou/codely 池为**每日共享额度制**（用户口径）：429 = 当日配额被共享用户挤占/用尽，波动型限流（同分钟内小请求与大请求成败可并存），等窗口或次日
- **2026-10-19 模型可用性实测快照（headless CLI 通道、串行、单实例）**：GLM-5.3-222（9.2s）/ KIMI-K3-222（9.6s，**当日稍后 429 并退役，勿再用**）/ kimi-k2.7-code-highspeed-222（10.7s）/ step-5-preview-222（10.5s，⚠️订阅明日到期）**可用**。**当晚追况**：luyou 中转 GLM 家族全族 401（"no account could serve it"，GLM-5.3/GLM-5.3-Flash/intern-glm-5.3 均中招，账号池波动，非我方问题）；**claude-haiku-5-5（apigoto 通道 `claude-haiku-5-5-ago-222`）顶替审计/第二席**（用户指定，深测判真）；**qwen3.8-flash 上魔塔**（config 条目 `qwen3.8-flash-ms-222`=ModelScope `Qwen/Qwen3.8-Flash-Next`，PONG 10s）；deepseek-flash-222 **池级 429**（串行+退避 30s 仍限=当日配额耗尽，非瞬时并发；**下午经用户铺设 apigoto 路由 `deepseek-flash-ago-222` 恢复，spawn 路径不认 config 条目须走 headless**）；claude-sonnet-4-6-222 上游 401（luyou 无账号可服务）、gpt-5.5-222 上游 404（apigoto 不支持，本地 catalog stale）。**编排教训（今日实测）：并行派发 3 个同模型子代理瞬间击穿 tpm/rps 全灭（单会话 77K tokens）——同模型派工必须串行，严禁并行挖吞吐；429 均 ~10s 快速返回（池级拒答非挂起）**
- luyou→kimi-k3 流的 `finish_reason:""` 空串坑已修（commit `b333b253`：`empty_finish_reason_as_none` 双路径归一化+5 回归测试）——**旧 qidi.exe 进程 spawn K3 必炸**（unknown variant ''），换新版 exe 后消除
- **Kimi 系唯一在位线 = `kimi-k2.7-code-highspeed-222`**（apigoto 直连带独立 key；spawn 与 headless 双通道今日实测均可用，编码手/审计第二席两职均可担）——**KIMI-K3-222 已于 2026-10-19 退役（用户决定，当日 429 后未恢复），任何任务书/编排不得再派**；其历史坑（`finish_reason:""` 空串需新版 exe）随退役归档
- **headless 路由与"429=x.ai"假象实测（2026-10-16 深挖+两次勘误，RUST_LOG=debug 探针铁证）**：
  - **chat 路由遵守 config.toml `[model.*]`**——探针实测 `-m kimi-k3-sn-222` 实打实 POST `https://token.sensenova.cn/v1/chat/completions`（config.rs:3234 debug 行 base_url/has_api_key 可证）；早版"登录态平台旁路"结论**错误已勘误**（config.rs:4116 兜底仅对目录条目缺 base_url 时生效）
  - **"x.ai 配额"假象根源=自家 429 文案模板**：任何上游 429 经 `RATE_LIMITED_ERROR_CODE(-32003)` 契约抑制真实详情后显示硬编码 `RATE_LIMITED_USER_MESSAGE_API_KEY`（cf-shell/src/sampling/error.rs:27，grok 遗产，自称 "team's API rate limit...docs.x.ai"）——**sensenova 的 tpm/rpm 限流、luyou 池日配额尽都被渲染成"x.ai 配额耗尽"**；历史会话一切"x.ai 配额"定性须按此重解读（文案中性化+默认端点修复 2026-10-16 当日落地）
  - **真实配额事件（均非 x.ai）**：luyou/CodeLy 池 kimi 线日配额尽（K3#1 有输出后死、K3#2 一响应即死=波动型）；sensenova 直连线受账号自身 tpm/rpm 滑动窗限流，且大 prompt 触发 400 invalid_request（K3-sn 审计任务书被拒——该线暂不宜跑长审计）
  - **rebrand 漏网默认端点**：`CLI_CHAT_PROXY_BASE_URL_DEFAULT="https://cli-chat-proxy.grok.com/v1"`（config.rs:46）与 `XAI_API_BASE_URL_DEFAULT="https://api.x.ai/v1"`（config.rs:48）——无覆盖时启动期 aux（models fetch 等）真打 grok.com（缓存 origin mismatch 行铁证），非致命但方向错误；已改 `https://api.qidiai.ltd/v1`
  - **rebrand 漏网清算（2026-10-19 批次3）**：L1.5 清单**大部分已清**——`cf-env` cli_chat_proxy/asset_server（`cf-env/src/lib.rs:16` 区块）、`cf-workspace/src/handle.rs` fallback、`ASSET_SERVER_URL_DEFAULT`、billing UPSELL 全套（含 FREE_USAGE 文案）、`MANAGED_SECTION_CONNECTORS_URL`、app.rs 文案、B5 前缀 "grok:"、console.x.ai、install 脚本代理 URL、mcp_doctor、share/relay URL——全部改到 qidi 域。**`QIDI_COM_METHOD_ID` 已按迁移容忍改造**：新值 `qidi.com`（待用户最终确认）只写不读旧，`LEGACY_QIDI_COM_METHOD_ID="grok.com"` 只读不写新，`from_id` 双认+`is_qidi_com_method_id()` helper+钉常量防漂移测试（存量会话/老 peer 不掉线）。**WS relay 已切（2026-10-19 深挖后）**：`wss://ba.qidiai.ltd/ws` 是用户实测在跑的**手机遥控中继**（`agent headless` 模式，Bearer secret+JSON-RPC/ACP，配 ws://192.168.1.222:2419 局域网直连；握手/鉴权/协议全实证）——`cf-env` relay_ws_url+ws_origin 默认值已切到 ba，CLI flag 去 grok 化（`--qidi-ws-url/origin` 主名+`--grok-ws-*` visible alias），leader 派生与文档同步切主名。**仍留 grok 上游的**：gateway `/ws/gw/`（ba 实测 404、无 qidi 部署，待运营方部署后同方式切）、`leader/server.rs` `PROD_COMPUTER_HUB_URL`、`workspace_server.rs` computer-hub 默认值、`remote/client.rs` `QIDI_CODE_BACKEND_URL=code.grok.com`、`remote/chat_models_client.rs`——另一条后端链，需真实值后替换。**猜措值状态（2026-10-19 用户确认）**：`https://assets.qidiai.ltd` 与 `https://qidiai.ltd/superqidi` **已确认可用**（用户将注册 qidiai.ltd 域名，代码值已就位无需改）；auth 新 ID = **`qidiai.ltd`**（2026-10-19 用户确认 qidiai.ltd 为其域名后改定；legacy `grok.com` 仍双认，存量会话不掉线）

## 10. Git 提交与推送（生产纪律）

| 项 | 约定 |
|---|---|
| 远程 | `origin`（github）、`gitee`、`upstream`（**SMB 共享基线** `\\192.168.1.222\程序开发\grok-build-upstream`，2026-10-05 起；原本地路径 `C:\Users\ASUS\grok-build` 已不存在）。⚠️ **2026-10-17 API 实测：origin 与 gitee 均为 PUBLIC（用户确认软件对外公开即为此状态）**——文档私有库是另一个仓库 `github.com/qidiai/qidiwork-docs`（Private），勿混 |
| GitHub 网络分流 | **origin 推送走首尔 SSH 隧道**（2026-10-14 起）：git 已配域名级代理 `http.https://github.com.proxy = socks5://127.0.0.1:1080`——**仅 github.com 走首尔出口，gitee 等其他全部原网络直连**。隧道=`C:\qidi-proxy\seoul_socks5.py`（SSH 动态转发→首尔 VPS，源自 `\\192.168.1.222\程序开发\clash` 归档），**计划任务 `QidiSeoulProxy` 常驻**（登录自启+无执行时限；勿用 shell Start-Process 拉起——Job Object 会杀）。SOCKS5 1080 / PAC 1081。⚠️ **2026-10-19 实测：首尔代理当日两次掉线**（port 1080 TCP 不通→origin 推送失败）——**修复法：`Start-ScheduledTask -TaskName "QidiSeoulProxy"`**（计划任务本身 Ready，拉起后端口即恢复；进程跑在任务独立会话，本 shell 的 Get-Process 看不到但端口可验）；另注意：**并行会话会在同一分支落 commit**（今日实测到他人 commit a1c81e2f/e1adc824）——推送前 `git log` 核对，**勿 revert 他人提交** |
| 默认推送 | **双推 `origin` + `gitee`（仅限代码）**；worktree 分支用 `git push <remote> feat/xxx:main` 直推 main。**文档永不进本仓库双推面**（§3） |
| 可见性闸门（2026-10-17 事故后新增） | 每次会话首次推送前、以及任何远程配置变更后，跑一次 `curl.exe -s https://api.github.com/repos/qidiai/qidicode` 与 gitee 对应 API，确认可见性与预期一致（公开是预期状态；**若返回 private 或仓库消失，停止推送并报告**）。文档类内容一律推私有库，推完 `gh repo view qidiai/qidiwork-docs --json visibility` 验证 PRIVATE |
| 提交权 | 编码手/子代理禁止 commit；指挥官质检通过后统一提交 |
| 提交信息 | `<类型>: <摘要>` + 正文列要点（orchestra 战报格式，见 git log） |
| 密钥扫描 | ✅ **pre-commit 钩子已生效**（2026-09-30 实测：`git config core.hooksPath` 指向 `pre-commit-secret-check` 技能的 `scripts/hooks` 目录，提交时自动扫描，实测输出 `[Pre-commit] No secrets detected`）。⚠️ 注意 hooksPath 生效后 `.git/hooks/` 整体失效（含原 post-commit Qoder tracker）。敏感提交前仍建议手动复跑 `secret-scan` 技能兜底 |

## 11. 环境硬约束（本机实测，新会话必读）

1. **本机无 `rg`**；内置 grep 工具在本环境 **100% 返空**（2026-10-19 全面测试实测：`fn main` 内置 0 vs `git grep` 89；根因链：Windows 构建不捆绑 rg（cf-shell/build.rs:43）+ PATH 无 rg 回落裸名（ripgrep.rs:99））→ **一律用 `git grep`**。⚠️ 2026-10-19 已修"静默" half（commit 45828f22：spawn 失败错误写入 stdout，不再伪装成"没有匹配"）——**但本机仍无 rg，grep 依旧不可搜索，只是现在会明说**；rg 捆绑根治（F2b 启动期探针/Windows 解捆）在批次 2
2. **PowerShell 不支持 `&&`**（用 `;`）；控制台 GBK 中文乱码 → `Get-Content -Encoding UTF8`
3. **禁止递归扫描**：`target/`、`mem-log/`、`tmp_relay/`、`9.22方案/scripts`、`C:\Users\ASUS\.qidi\sessions\`（1.9 万文件）——都会卡死。**会话压缩阈值实测（2026-10-20）**：默认 **85%**（`cf-shell/src/util/config/resolve/compaction.rs:2` `DEFAULT_AUTO_COMPACT_THRESHOLD_PERCENT`），优先级 env `QIDI_AUTO_COMPACT_THRESHOLD_PERCENT` > 用户 config `[model.<id>].auto_compact_threshold_percent` / `[session]` > 服务端下发 > 默认；**1M 窗口模型要到 ~85 万 token 才触发压缩**——大窗口下"一天不压缩"属预期（实测 973K 会话确实在越阈后自动压缩）；嫌晚可设 env=60 提前触发；`[skills].subagent_denied_skills` 是**显式替换**非合并（写配置=整体取代默认集）（按需小范围扫可以，全量递归必卡）；**会话归档用 junction 模式**（2026-10-20 首用）：7 天未活动的会话 `Move-Item` 到 `E:\qidi-sessions-archive\<项目>\` + 原位 `mklink /J` 回填——C 盘字节真释放且选择器/`/resume` 无感；归档包带 README+catalog（convention 见 `G:\qidi-sessions-archive\2026-09-18\`）；反向=删 junction（`rmdir` 纯联接勿加 /s）+ move 回
4. `Format-Table` 输出有缓冲错位——重要判断以具体值/行号为准，勿凭表格位置
5. `read_file` 只能读 workspace 内文件（C 盘日志用终端 `-Encoding UTF8` 读）
6. 机器上有 25+ 常驻 python/node 进程（用户其它会话的活）——**绝不擅自杀进程**
7. `G:\quant-platform\` 是 G 盘根的**独立项目**（另一会话在用）：勿动其文件、勿杀其进程；本仓库 target 下**没有**它的构建产物
8. 后台任务输出常延迟/丢失——**以日志文件为准**：运维日志落点 `G:\qidi-clean.log`、`G:\qidi-release-build*.log`；exe 备份落点 `G:\qidi-exe-backup\`
9. **GPU 服务已不自启**（2026-09-30 起）：HunyuanOCR(`llama-server`@8899，~3.3GB 显存) 与 ComfyUI(@8188) 的开机 vbs、30 分钟保活计划任务（`qidicode-hyocr-keepalive` / `qidicode-comfyui-keepalive`）已全部禁用。**跑 OCR 类技能（pdf-to-word-ocr / seal-extractor / bid 文件解析）或出图前先确认端口在线**，离线则手动拉起（幂等可重复跑）：OCR → `powershell -File C:\hyocr\start_hyocr_cuda.ps1`；出图 → `powershell -File F:\AI\ComfyUI\start_comfyui.ps1`。OCR 用完跑 `C:\hyocr\stop_hyocr.ps1` 释放显存（脚本已修编码 bug，可用）
10. **远控唯一入口 = 自建 RustDesk**（2026-10-14 起）：ToDesk 5.0.2 与 AweSun(向日葵)16.6 已彻底卸载（服务/注册表卸载项/驱动 `OrayUSBVHCI.sys`+`OrayVGC.sys`/`Program Files\Oray` 与 x86 `SunLogin` 空壳目录全清，注册表项备份在 `target\tmp\uninstall-remotes\`）。RustDesk 1.5.0（`C:\Program Files\RustDesk`，服务 Automatic 常驻），自建服务器腾讯云首尔 `43.155.167.164`（中继 `:21117`，带 Key），台式机 ID `467704860`，接入说明见 `\\192.168.1.222\共享文件资料\RustDesk接入说明.txt`。旧 AweSun 时代 Run 项备份仍留 `%TEMP%\HKLM_Run_backup_20260930.reg`
11. **Intel 核显驱动** 2026-09-30 从 2019-11 版（26.20.100.7584）更新至 **31.0.101.2141**（微软更新目录 WHQL 包，修复 0x10E VidMm 蓝屏 `dxgmms2!VIDMM_CPU_HOST_APERTURE::MapRange`）。驱动库留有 31.0.101.2127 后备包。再遇蓝屏先查 `C:\Windows\Minidump\`；本机已装 WinDbg（`Get-AppxPackage Microsoft.WinDbg` 定位 cdb.exe），符号路径 `srv*C:\symbols*https://msdl.microsoft.com/download/symbols`
12. 【临时条目-完成后请删除】2026-09-30 深夜：Defender 隔离区 16.73GB 仍未清除——实测安全模式下 MsMpEng/WdFilter 依旧常驻，且进程级行为防护会拦截一切停服/强删命令（连 Stop-Service 都在终端层报拒绝访问）。safeboot 标志已清（重启回正常系统）。**终局方案已就位：`C:\purge_winre.cmd`**（纯ASCII、自定位盘符）——需 WinRE 环境执行：Shift+重启 → 疑难解答 → 高级选项 → 命令提示符 → 输入 `C:\purge_winre.cmd` → exit 回正常系统。⚠️ **防篡改保护仍处于关闭状态**，回正常系统后需在 Windows 安全中心手动重新打开。若读到本条时隔离区已清且防篡改已开，说明流程已完成——**请顺手删除本条并 commit**

## 12. 构建与测试

| 操作 | 命令 | 耗时/备注 |
|---|---|---|
| 出产品 exe | `cargo build --release -p cf-pager-bin --bin qidi -j 4` | 冷 30-90 分钟；有缓存快；**运行中的 qidi.exe 无法覆盖 → 先改名旧 exe（Windows 允许改名不允许覆盖）**。**2026-10-19 构建实录**：含当日全部修复的产品 exe 已出（`qidi 0.1.220-alpha.4 (35045d22)`，独立目录 `C:\qidi-release-build` 冷编 101 分钟+冒烟 PONG+构建区已清），**暂存于 `G:\qidi-exe-backup\qidi-20261019-35045d22.exe`** 待换装（换装四步：备份旧→改名旧→拷新→`--version` 验；新会话生效） |
| cf-memory 测试 | `cargo test -p cf-memory --lib` | ~2-10 分钟 |
| **cf-shell 测试** | `$env:CARGO_TARGET_DIR='C:\qidi-cfshell-test'; $env:CARGO_PROFILE_DEV_DEBUG='0'; cargo test -p cf-shell --lib -j 4` | **热缓存全量 ~2.5 分钟**；`DEBUG=0`+串行是内存 OOM 的解药（本机 16GB，cf-shell 测试二进制直跑必 OOM）。⚠️ **2026-10-19 实测：全量跑不绿属既有问题（非回归），共 10 项已知失败**——① `acp_session::cancel_running_task_tests::persist_ack_waits_for_disk_flush_before_success` 栈溢出（Windows 测试线程 2MB 栈）会**中断整个 harness**；② `auth_method::tests::enterprise_byok_config_does_not_require_login`、`agent::config::tests::has_own_credentials_guards_session_vs_external_key`、`cli_models::tests::resolve_{deployment_key,model_api_key_byok,model_credentials_uses_first_catalog_key,model_env_key_byok,not_authenticated,priority_byok_over_deployment}` 共 8 项因本机预置多枚 API key（ANTHROPIC/OPENAI/DEEPSEEK/MOONSHOT/ZHIPU/ARK/AGNES）必败（kimi-k3-moonshot 条目 env 探针翻转，测试自己的注释记载了该机制）；③ `drain_finished_thread_returns_immediately` 负载敏感 flaky。**跑法：按名称跳过这 10 项（全量基线 5630 passed/7 env-failed，批次2 QC 实测），或 `cargo test -p cf-shell --lib mvp_agent::` 过滤跑（基线 185/186）**。**2026-10-20 三修合一后实测**：总数 5654（+6 个 post-pending 契约测试），跳过 17 项（含 `cli_models::tests::resolve_` 全组）后 **5630 passed / 0 failed / 7 ignored**；`mvp_agent::` 过滤 **186/0** |
| cf-chat-state 测试 | `cargo test -p cf-chat-state --lib`（G 盘直跑）；历史注记：2026-10-04 曾报 G 盘链接确定性挂起（link.exe ~369MB 零 I/O 冻结，两次复现），**2026-10-09 复测未复现**——冷编 21m22s + link.exe 活跃推进（工作集 5.4GB，非冻结特征）+ 链接正常完成 + 测试 0.53s；若再现挂起，改用 C 盘热 target 方案（同 cf-shell 行） | G 盘冷编（禁 sccache）~21 分钟；基线 345 passed / 0 failed（2026-10-09，较 2026-10-04 基线 343 +2 = B1-6 新增 cache_read_ratio 两测试，数目自洽） |
| **cf-pager 测试** | ⚠️ G 盘冷链**严重偏离旧记载**（2026-10-04 实测 >90 分钟未完成，pdb 1.6GB 链接为瓶颈）→ **一律用 C 盘热 target 方案**（同 cf-shell 行） | C 盘热缓存 ~40 分钟；基线 **7055 passed + 6 ignored / 0 failed**（2026-10-19 批次1修复后实测，7061 总数；旧值 7033 作废）——**必须加 `$env:QIDI_FORCE_LEGACY_CONSOLE='0'`**：旧记载"53 预先存在字形失败"已作废（根因=legacy OEM 代码页遗留，非环境字体）；另注意既有 flaky：`needs_animation_gates_prompt_history_tick_delivery`（时序敏感，隔离重跑即过） |
| 快速全局校验 | `cargo check --workspace` | 0 errors 为过 |
| **macOS 构建** | `.github/workflows/ci.yml` 的 macos-build job（直推 main 自动触发，`git push origin feat/xxx:main`） | aarch64-apple-darwin 产物挂 artifact `qidi-macos-arm64`（~59MB）可下载；libc 依赖门教训（b507ae61）：依赖 cfg 不得比代码使用面窄（`cfg(unix)` ⊃ macos） |
| **ai-bridge 多工具桥** | Hub `http://127.0.0.1:9800`（`/api/dispatch` 等，鉴权 token=用户级环境变量 `AI_BRIDGE_TOKEN`） | claude-code 与 opencode 适配器已改 **stdin 传参**（2026-10-14——arg 模式多行长文本丢失实锤）；opencode 已注册免费模型 `opencode-go/deepseek-v4-flash`；开关状态在 switches.json（`/api/toggle`）；Hub 挂了用 WMI 拉起（`Win32_Process.Create`，脱离 Job Object）；**dispatch 是外部工具委派**（把任务交给 claude-code/opencode 等外部 CLI），**不属于 qidicode 子代理机制、不受 `MAX_SUBAGENT_DEPTH` 约束**——因此 max depth=1 拦得住原生 `spawn_subagent`，拦不住 MCP 派发；**勿把 dispatch 当作嵌套 spawn 通道**（无 depth 链、无父子会话生命周期、无子代理工具面投影）。⚠️ **空返回不等于成功**：dispatch 无内容返回时按失败处置并核对 `/api/logs`（2026-10-19 实测，Hub 侧尚无可区分的错误码） |
| **playwright（MCP 25 工具 + 内置 5 `browser_*`）** | 2026-10-19 实测：枚举自洽（25=宣称数）、内置 5 工具全链路（导航/快照/读/点击/输入提交/错误路径）全部达成、错误路径报错清晰 | **⚠️ 双上下文陷阱：MCP 面 `playwright__*` 与内置 `browser_*` 操作两个完全隔离的浏览器上下文**——一面导航另一面不可见，带登录态/页面状态的任务严禁混用（静默错乱）；跳转后首次调用偶发 `Cannot find context with specified id`（~25%，操作实际均成功，重试即过） |
| **P1 缺陷台账（2026-10-19 全面测试发现；批次1修复 commit `45828f22`+`69677775`）** | ✅已修：①grep 静默返空→错误可见化（本机仍无 rg，grep 依旧不可用但不再伪装"没有匹配"，继续用 `git grep`；捆绑根治在批次2；**2026-10-20 追加**：spawn 失败文本附带 `RG_UNAVAILABLE_NOTICE`——点明 ripgrep 缺失 + `git grep` 兜底，`tool_grep_spawn_failure_error_visible_in_stdout` 逐字钉住）②冒号族广告即败→registry 归一化查找（含 `tool_identity` 残肢，7 常量全部恢复可调用）④headless `--worktree` 静默失效→显式报错退出（**真正创建 worktree 在批次2**）⏳未修：③`capability_mode=read-only/execute` 不禁 write（builder.rs:755-761 过滤后重注入）——**修好前 read-only 子代理按"可写"对待，勿托付写敏感操作** | 规格与剩余批次（F4/F2b/F3根治/F6技能库/F7）见 `docs/fix-plans/2026-10-19-缺陷修复方案.md`；F8 Dashboard 代理类型选择已随批次1落地（Alt+A） |
| 清理 | `cargo clean` | 先备份 `target/release/qidi.exe` 到 `G:\qidi-exe-backup\`；**注意 §6 target/debug 历史文件雷** |

⚠️ **磁盘清理硬性规则（2026-10-10 事故后强制，对所有编码手通用）**：target/debug 下混有大量历史用户文件（`_archive/`、`staging/`、`sagent/`、`少儿英语小程序/`、`quant_deploy_*.zip` 等——2026-10-10 被 DeepSeek 按"清 debug"指令**整目录删除后丢失**，无备份，仅部分有外部副本）。**规则**：
1. **"清 X"类命令一律写为 `cargo clean -p <crate>`**（只删包内产物）——绝不写模糊的"清 debug 目录"
2. **删除任何目录前必须先 `Get-ChildItem` 列出内容确认无用户文件**——发现 §6 列名文件立即排除并上报指挥官
3. **编码手任务书模板**：磁盘清理指令必须附带排除清单（"清除 cargo 托管产物，保留 _archive/staging/sagent 等用户文件"）
4. **违反后果**：不可恢复丢失（Remove-Item -Force 不走回收站；HDD 未覆盖前 winfr 可部分捞回但不可依赖）

注意：cf-shell 的 **test profile** 在 G 盘 target 直跑会 OOM——一律走上面的 C 盘热 target 方案（**每次跑前手动设** `$env:CARGO_PROFILE_TEST_DEBUG='0'`——未持久化到用户环境变量，规范命令行见上表 cf-shell 行）。
⚠️ **C 盘热 target 会无界增长**（2026-10-06 实测：`C:\qidi-cfshell-test` 累积 **44GB** 构建产物把 C: 盘填满到 0 bytes，cf-shell 测试编译报 `rustc-LLVM ERROR: IO failure on output stream: no space on device`）→ 每次跑前 `Get-PSDrive C` 看余量，低于 ~20GB 先清该目录（纯构建产物，可整目录删；cf-shell 冷重建 ~2 分钟）。带 `CARGO_PROFILE_TEST_DEBUG='0'` 时增量产物很小，44GB 是历史 debug 全量残留。**2026-10-19 实测更新**：该目录达 **72.1GB**（构成：`debug/incremental/` **47.2GB ≈65%**、`debug/deps/` 23.3GB、`debug/build/` 2.5GB）——**`incremental/` 是增量编译缓存，删除后 cargo 自动重建且不动 deps 热缓存，是精准清理首选**（当日经 §6 雷区守卫扫描后删除，回收 45.5GB，C: 56.1→101.5GB；删除前必须复扫 `_archive/staging/sagent/quant_deploy*/少儿英语*/*.zip` 雷区，且确认无 cargo 在跑）。

13. **upstream（SMB 基线）使用注意**（2026-10-05 实测）：① `git fetch upstream` 会报 "shallow roots are not allowed to be updated"（浅历史纠缠），**读上游更新改用直读**：`git --git-dir='\\192.168.1.222\程序开发\grok-build-upstream\.git' log/diff ...`（SMB 上可执行，慢但可用）；② 上游是 monorepo 镜像（提交均称 "Synced from monorepo"，每周 1-3 次）；③ 上游 crate 命名为 `xai-grok-*`，我们 7-25 已 rebrand 为 `cf-*`——**路径全不同，直接 merge 不可行**，同步只能按主题 cherry-pick + 手工改名适配；④ fork（2026-07-19）以来从未同步过上游；⑤ **定位声明（2026-10-05 用户确认）：上游为低频情报参考源、非合并目标**——分叉已深且双向加速演进，仅安全类修复（RUSTSEC/依赖漏洞）值得定期扫一眼，功能类改进一律以我们自己的设计与实现为准
14. **sccache 会污染 target 缓存**（2026-10-07 实测）：sccache.exe 服务挂掉后，经它包装的 rustc 会产出**坏元数据产物**——后续编译报 `E0460 found possibly newer version of crate` 或直接 "process didn't exit successfully: sccache.exe..."（同一 target 内 release lib 与 test 二进制可同时中招，cf-shell 是高发点）。**解药：`$env:RUSTC_WRAPPER=''` 禁用后增量重编**（16 分钟级可过）；若增量仍 E0460 则 `cargo clean -p <crate>` 强制重编该 crate。多轮审计/长任务中 sccache 反复崩是本机常态（2026-10-06 起三次实测），长编译/测试任务建议起手就禁用