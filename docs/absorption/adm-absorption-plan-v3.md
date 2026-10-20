# ADM 吸收方案 v3（三席审计裁决定稿，2026-10-20）

> ** lineage**：v1（初稿）→ 三模型审计（step5/qwen/haiku）→ v2（吸收 7 处修订）→ **双席复审计（A=qwen-3.8-flash-next，B=GLM-5.3）+ 裁决席（claude-haiku-5-5）→ 本 v3**。
> **最终判定（裁决席）**：合并修订后**可实施**——A 的“不可实施”与 B 的“修订后直接实施”各对一半；A 的 R1/R3 为补充性修订，R2 改变交付物形态。共 14 条修订（V3-1~V3-14）+ 4 项硬约束（新发现-1~4）。
> 三份审计报告：`target/tmp/audit-adm-v2-qwen/report.md`（A）、`target/tmp/audit-adm-v2-glm/report.md`（B）、`target/tmp/audit-adm-v2-haiku/report.md`（裁决）。
> v1/v2 保留作修订记录，不再维护。

---

## 一、硬约束（裁决席新发现，v3 的地基）

1. **产品自更新是 fail-closed 的**（`crates/codegen/cf-update/src/auto_update.rs:1205-1220`）：验签未配置时**直接拒绝安装**；`signing.rs:11-12` 的“smoke-test 模式”注释已过时。**结论：私钥就位前，任何“自动安装/自动换装”渠道都不可用；qidi-update 只能做“可回滚换装 SOP”（见 V3-2）。**
2. **subagent denylist 是 name 前缀匹配**（`subagent_gate.rs:19-20`）：denylist 加一条 `wecom` 前缀即可同时罩住 `wecom-unified` 与未来的 `wecom-bridge`（V3-3）。
3. **agent 侧端点必须二选一并明确**（v2 的悬置项）：v3 选定 **ACP stdio**（daemon 以 ACP 客户端经 stdio 驱动本地 agent）——比 `agent serve :2419` 少一个端口、少一份 secret 管理；`serve` 模式保留为局域网备选（文档注明）。
4. **席位标注规范**：凡引用审计结论，一律“席位+实际模型”双标注（v2 曾把 haiku 实际执笔的报告标为 GLM 席，造成归因混乱）。

## 二、v2 → v3 修订记录（14 条，裁决席合并去重）

| # | 修订 | 来源 |
|---|---|---|
| V3-1 | **方向 2 确认通道改复用产品原语**：daemon 作为 ACP 客户端，把 agent 的 `session/request_permission`（`cf-acp-lib/src/message.rs:135`、`gateway.rs:391`、`leader/server.rs:462`、`hub_permission.rs:320`）渲染成微信消息，用户回复后经 RPC 回应；**删除 `ok #N`/`no #N` 自造协议**（#N 不绑 toolCallId，有 confused-deputy 洞）；异常时默认拒绝（fail-closed） | A-P0-R1 + B-D1 |
| V3-2 | **qidi-update 重定位**：不写“下载+验 hash+安装”技能（与 `cf-update` 重叠且更弱，产品本就拒绝未验签安装）；改为“**cf-update 薄壳 + 可回滚换装 SOP**”：staging→备份旧件→替换→`--version` 自验→一键回滚；私钥就位前不含任何自动安装路径 | A-P0-R2 + B-前置修订 A + 新发现-1 |
| V3-3 | **denylist 补 `wecom` 前缀**（`subagent_gate.rs:35-40`，前缀匹配一条覆盖两技能）；同时明确：denylist 只管子代理调用，**daemon 自发消息本就不经 denylist**——“全遮断”是伪需求，围栏责任在 daemon 侧（V3-5/V3-6） | B-前置修订 B + 新发现-2 |
| V3-4 | **凭据表述改现实**：同 Windows 用户下 agent 的 `run_terminal_command` 与 wecom-cli 同权限，**“禁入 agent 可读目录”不可执行**；批 0 需二选一定案（推荐：全站专用部署 daemon），§3 相关段落改写为“同用户文件级可读 + 显式知悉” | A-P0-R3 + B-R4 |
| V3-5 | **daemon 全站专用部署**：与会话 owner 绑定（非 per-agent 常驻）；agent 侧只读（只查状态/历史，不经手发送凭据）；chatid 与会话 owner 分离 | A-R-1 |
| V3-6 | **消息来源强制校验**：发送路径必须回显已识别的来源标记；未识别/多候选（好友/群/企微成员同名）→ **拒绝（fail-closed）**；群消息映射到同一“授权请求者”元组 | A-R-2 + B-R3 |
| V3-7 | **白名单改为形式化四元组** `(chat_type, chatid, sender, action)`：支持单 userid 或 API 签发批量；覆盖群查询、2048 字节分片、媒体文件落沙箱路径 | B-R2 + A-P1-7 |
| V3-8 | **消息记录改为形式化四元组**（禁自由文本日志模板）；`get_msg_chat_list` 返回会话列表及字段，`chat_type` 必须可解析（群/单聊） | A-O-3 |
| V3-9 | **daemon 失败模式**：argv 全参 spawn（禁 shell 拼接）；crash-loop 退避；危险操作（发送/删除）显式允许位；日志转储；失败不影响 agent 主链路 | A-R-3/R-5/O-8 |
| V3-10 | **wecom-cli 引用钉死**：绝对路径+版本/hash 登记；**禁止 agent 替换 CLI 或重启 daemon**（改动走 SOP） | A-R-6 |
| V3-11 | **删除“模板卡片按钮 PoC”**（与 V3-1 的权限原语重复）；方向 3（公网回调）整体移入 backlog——references 实证仅支持轮询、无回调 API | A-R-7/O-6 |
| V3-12 | **工作量修正**：批 0 = 2-3 天（含 wecom-cli 凭据探测）；批 1 = 2-3 天；批 3（qidi-update，按 V3-2 的 SOP 形态）= 0.5 天 | A-O-13 + B-C20 |
| V3-13 | **证据口径修正**：(a) `skills.rs:946` → `subagent_gate.rs:35`（前者仅是测试）；(b) “39 references” → 37；(c) “有 chat 回调 API” → “仅有轮询，无回调 API”；(d) 审计报告归因按硬约束 4 双标注 | A-O-9~11 + B-C1/C14 |
| V3-14 | **顺手修 rebrand 漏网**：`crates/codegen/cf-pager/docs/user-guide/15-agent-mode.md:54` 的 `GROK_AGENT_SECRET` → `QIDI_AGENT_SECRET`（以 `cli.rs:343` 为准） | A-O-12 + 裁决实证 |

## 三、修订后的 P0：wecom-bridge v3 规格

**架构**：`wecom-cli`（外部 CLI，npm 全局，**批 0 先升 0.1.9→1.3.4 并回归 37 个 references**）↔ **全站专用常驻 daemon**（Node 脚本或 Rust 小 exe；保活参照 §11.9 计划任务先例；argv 全参 spawn+crash-loop 退避）↔ **本地 agent（ACP stdio，secret 自动生成）**。

**第一批（唯一承诺批，2-3 天）**：
1. 方向 1 结果推送：只推终态结果（中间进度合并）；四元组白名单 `(chat_type, chatid, sender, action)`；群/私聊分级（群默认只读）；来源校验+多候选拒绝（V3-6）；2048 字节分片；媒体落沙箱路径
2. 安全围栏：`qidi <指令>` 前缀协议；禁自由文本直通；长度/类型白名单；敏感操作二次确认；daemon 与 agent 同机部署、agent 只读（V3-5）；daemon 侧审计日志（四元组，V3-8）
3. denylist 补 `wecom`（V3-3）；wecom-cli 路径/版本钉死（V3-10）

**第二批（1 天）**：方向 2 = 复用 ACP `session/request_permission` 原语（V3-1）：daemon 收到权限请求→渲染微信消息（含 toolCallId 与操作摘要）→用户回复→RPC 回应；TTL 默认拒绝；#N 自造协议删除。

**P1（0.5 天）**：qidi-update = cf-update 薄壳+可回滚换装 SOP（V3-2），私钥就位前不含自动安装。

**P2（即时，0 代码）**：纪律三条进 AGENTS.md（“没写=不存在”明文化；外部组件版本登记；并行会话文件守卫 backlog 化）。

**Backlog**：方向 3 公网回调（先定中继形态：ba 信任根 vs 独立穿透 vs WeCom 智能机器人长连接）；fact-block/recall 借鉴（ADM `doc/admAgent-fact-block-and-recall-refs-plan.md`，与 compact 挂载点）；前端约定互鉴；agent 流量代理产品化。

## 四、风险清单（v3 修订后）

1. **prompt injection**（微信内容→agent）：前缀协议+禁读 workspace 外+回传过 secret-scan+（读+发送）组合审计告警；门禁落 daemon/agent 表面，**不依赖模型自律**（呼应 capability 泄漏教训）
2. **凭据同机可读**：降级表述+专用 daemon+agent 只读（V3-4/V3-5）；DPAPI/企业秘匫置列为后续增强
3. **wecom-cli 1.3.4 升级风险**：6 个 minor 的 breaking changes 未知——批 0 的 37 references 回归+授权态探测是硬前置，未完成不开工
4. **daemon 被注入/沦陷**：只读绑定、禁自替换 CLI、危险操作允许位（V3-9/V3-10）
5. **频率限制**：终态合并+退避+分片（WeCom 应用消息配额）

## 五、实施批次与门禁

| 批次 | 内容 | 工作量 | 前置 |
|---|---|---|---|
| 0 | wecom-cli 0.1.9→1.3.4 升级+37 references 回归；授权态探测；凭据托管方案定案（V3-4）；白名单四元组+self userid | 2-3 天 | 无 |
| 1 | daemon 骨架（Node）+方向 1+围栏全套（V3-5/6/7/8/9/10） | 2-3 天 | 批 0 |
| 2 | 方向 2：ACP 权限原语复用（V3-1） | 1 天 | 批 1 |
| 3 | qidi-update SOP（V3-2） | 0.5 天 | 无 |
| — | P2 纪律进 AGENTS.md；V3-14 rebrand 修复 | 即时 | 无 |
| **合计** | | **6-9 个工作日** | |

**验收门**：每批带测试（围栏单测+daemon 集成测试+E2E 冒烟）；`cargo check --workspace` 0 errors（涉及 Rust 侧时）；ci-baseline 全量不回归（Rust 改动时）。

## 六、决策定案（2026-10-20，GLM-5.3 决策分析+用户拍板）

1. **V3-3 denylist**：✅ **已定：加 `wecom-` 前缀**（GLM 推荐 A+用户拍板）。子代理禁直发微信，daemon 为唯一发送面；已落地（subagent_gate.rs：**2 个新测试+1 个扩展**，cf-tools 2685/0）。⚠️ **R1 语义更正（GLM 实施后审计）**：用户 config 的 `[skills].subagent_denied_skills` 是**显式替换**默认清单、不是合并（`explicit_override_replaces_the_default_set` 与 `explicit_empty_override_opens_the_gate` 两测试钉住）——用户写配置即整体取代默认集（wecom-/OCR/bid- 全换成用户清单），属知情强覆盖而非叠加纠偏。⚠️ **R5**：子代理会话内 discovery flush 曾不过门禁（pending 冲刷直接写 AvailableSkills），P1 修复中
2. **V3-4 凭据托管**：✅ **已定：全站专用部署 daemon**（GLM 推荐 A）。daemon 独占凭据生命周期、与会话 owner 绑定、agent 只读；同机同权限现实下可控变量=发送面数量（A=1 个审计咽喉 vs B=每会话一个）；独立 Windows 账户+DPAPI 为后续增强路径
3. **方向 3 中继形态**：✅ **已定：暂缓+第四方案 D**（GLM 推荐）。WeCom 实证仅支持轮询无推送 API——“中继”是伪问题；方向 3 起步 = 现有 daemon+wecom-cli 轮询收件（零新增基础设施）；确需秒级推送再上 A（ba 信任根加独立鉴权路由）；B/C 不推荐
