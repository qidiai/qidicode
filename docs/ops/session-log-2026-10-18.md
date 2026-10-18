# 2026-10-18 会话纪要：tap 冒烟首战 + 推进方案 v2.3 重排 + 交接

> 本会话（10-17 深夜 → 10-18 上午）完成 tap 载荷单价观测层交付+首份分账冒烟+方案 v2.3 重排。**用户出差中，回家后按本文档排期推进。**

## 一、tap 载荷单价观测层交付（v2.3 放行版）

### 交付物
- **代码**：`cf-shell/src/session/acp_session_impl/payload_accounting.rs`（1363 行 tap 主模块）+ `payload_accounting.rs:479/484/2139` 的 `actor_pruning_config`（R7 config 接线）
- **植入点**：`build_conversation_request`（cf-chat-state）内 ByteCounter 复用——五截面全结构化可分（零字节嗅探）
- **第三零件**：`estimate_item_tokens`（state.rs:51-93 复用——P1-1 分桶近乎免费）
- **测试**：14 条 payload 全绿（CI #114）+ cf-chat-state 387/0 + cf-shell R10 后 1457 串行 exit 0 + **cf-shell R10 并行 8 FAILED = resolve 系环境缺陷（moonshot 条目触发，零关——R11 待办）**

### 双审计闭环
- step-5 联审：NO-GO 3 阻塞（双 tap/B2 图片漏计/B3 口径泄露）→ R10 修复 → 复审 **GO 0 阻塞**
- GLM 联审：NO-GO 2 阻塞（双 tap 矛盾矩阵/TR 图片）→ 复审 **GO 0 阻塞**
- deepseek-flash 终审：0 阻塞+盲面 10 项+**hash 记账勘误**（3c1eccff 非 HEAD 祖先，镜像提交）

## 二、tap 冒烟首战（09:09 headless QIDI_PAYLOAD_TAP=full）

### 首份分账（model=step-5-preview，provider=22748 tokens）

| Section | est tokens | 占比 |
|---|---|---|
| builtinToolDefs | **8425** | **37.0%** |
| **agentsMd** | **7429** | **32.7%** |
| conversationHistory | 2203 | 9.7% |
| systemPromptStatic | 960 | 4.2% |
| skillListing（透镜） | 2054 | — |
| mcpServers（透镜） | **67** | — |
| 残差 | 1 | ~0 |

- **对账**：Σ=19018 vs provider=22748，drift=16.4%（>15% 告警正确触发——chat-template 开销）
- **铁律执法**：noTimestamp/noNonce 均 true
- **前缀哈希**：system/defs/agentsMd/skillListing 四段 sha256 输出

### 三个决策级发现
1. **MCP 投影降级**：43 个 MCP schema 实际只占 **67 token**（概要行模式天然生效）——P0-4 的"5-15K"假设被数据推翻——③降远期
2. **M2 AGENTS.md 分域价值实锤**：7429 token/32.7% 第二大头——M2 优先级升
3. **对账校准需求**：drift 16.4%=chat-template 开销——校准器优先级升（校准后 <5%）

## 三、DeepSeek Flash 演进史（官方文档实锤——用户给的链接）

来源：https://api-docs.deepseek.com/zh-cn/updates/ 全文抓取（docs/ops/deepseek-flash-evolution.md 归档）：

| 时间 | 事件 |
|---|---|
| 04-24 | V4 发布（Pro+Flash）；旧名 chat/reasoner 三个月停用 |
| 07-31 | V4-Flash 正式版 |
| 08-21 | V4-Flash-Vision-Exp |
| 08-13 | V4-Pro 正式版+峰谷定价 |
| **09-10** | **V4.1-Flash 发布——"模型名更改为 deepseek-flash"——旧名暂时路由后撤下** |
| **10-17** | **V4.1-Flash-1017 更新（帕累托改进：更快/更短/更低成本；针对 DeepSeek Harness/CodeBuddy/WorkBuddy 优化）** |

### API 名策略（官方口径）
- `deepseek-flash` = 官方恒定 API 名（滚动指针）；版本代号名不在承诺内——**引用版本代号 = 401 必然**（AGENTS 主力条目 401 根源官方实锤）
- 原生多模态（V4.1 系）= ling 下线后的视觉替代位；峰谷定价=闲时任务

**策略定案（用户确认）**：**主力编程模型改为 deepseek-flash**（config 已切——滚动指针自动跟进版本）

## 四、luyou 账号池分级模型（服务端日志实锤+用户澄清）

文档：docs/ops/luyou-account-pools.md

| 层 | 内容 |
|---|---|
| intern key 池（ocr-4 等） | 额度已废（用户告知） |
| codely free 组 | 只能调 GLM 系（codely-core=GLM/Z.ai 实锤）；K3 无权限 |
| **pro/订阅组**（'39640','58863'） | 全模型可调——K3 唯一供给 |

**诊断口诀**：K3 429=pro 配额；GLM 429=free 冷却；DeepSeek 系=独立上游恒稳；**修复点=luyou 凭据→组映射配置（把 spawn 平台 token 加 pro 组——用户可修）**

## 五、模型路由全景修正（用户纠正×3）

| 层 | 事实 |
|---|---|
| spawn 路由 | **走 luyou**（错误里 Request URL 实锤——非平台网关） |
| 差异根源 | **凭据→组映射**：spawn 平台 token→无 K3 组；auth.json key→有配额组 |
| 修复 | luyou 配置侧（用户运维可修——把 spawn token 加 pro 组） |

## 六、AGENTS.md 并发编辑竞态（两次实证）

| 时间 | 事件 |
|---|---|
| 21:46 | Python 插入 DeepSeek 401 勘误 |
| 21:49 | 另一会话大幅扩充（19.2→31.8KB）——我的行幸存（合并共存） |
| 23:56 | Python 插入 luyou 账号池模型——**写入被覆盖（git no changes）** |
| 处置 | **AGENTS.md 是多会话竞态战场**——沉淀改走独立文件（docs/ops/）+ 交接包携带全文——**极简会话串行写入** |

## 七、交接包（回家后的排期）

| # | 待办 | 规模 | 说明 |
|---|---|---|---|
| 1 | **M2 AGENTS.md 分域** | 0.5-1 天 | 7429 token/32.7% 的杠杆——走 .qidi/rules 既有缝 + domain: frontmatter + 域触发(Phase 2) |
| 2 | **对账校准器** | 0.5 天 | drift 16.4% → <5%（chat-template 开销建模）——tap 数据可作归因标准 |
| 3 | **①灰度实测开表** | 2 周 | ④/M2/校准落地后开表——7 指标+判据事前注册（step-5 表） |
| 4 | **R11 测试隔离** | 小 | isolate_auth_sources 补 config 隔离（moonshot 条目触发的 resolve 系 8 FAILED——几行） |
| 5 | **28vs30 差异** | 已闭 | disable-model-invocation 设计行为（零 bug） |
| 6 | **geo 项目** | 待定 | 用户自己写的——usage producer 已裁定不做；其余方向待点 |
| 7 | **P0-3 二期** | 视数据 | Essential 缺召回观察后决定匹配器 |
| 8 | **AGENTS.md luyou 模型写入** | 小 | docs/ops/luyou-account-pools.md 备着全文——串行写入 AGENTS.md §9/§11 |

## 八、提交链索引（10-17 深夜 → 10-18 全部）

`b9f6044a`（R10.5 透镜+patch+报告声明）→ `f6db59c2`（R9/R10+演进史，混合提交——内容全保）→ `89fad9de`（AGENTS 编码手切换，另一会话）→ **main=f6db59c2 已推**，CI 全绿 6/6×2（#114+38730694821）
