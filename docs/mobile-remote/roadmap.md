# qidicode 手机遥控端 Roadmap（深度适配计划）

> 2026-10-17 立档。基线：手机端 T1/T2（ACP over WebSocket 瘦客户端 + Android WebView 壳 + bin/remote 运维套件）+ 界面重构（设计系统/双主题/Profile/PWA）+ 后端优化（进行中）。
> 原则：**手机端 = 桌面端能力子集投影**；权限模型与桌面一致（只读自动批准、写/执行必确认）；会话/记忆/项目状态实时一致；新协议一律走 `qidi/*` 命名空间扩展方法。

## 能力现状（2026-10-17）

| 层 | 已有 | 缺口 |
|---|---|---|
| 连接 | URL+secret 手工连接、Profile 保存、扫码入口（URL 解析降级） | 真扫码配对（依赖后端 QR，开发中） |
| 会话 | session/new、断线重连、会话信息头 | 会话列表/恢复（`qidi/session/list` 开发中）、多会话切换 |
| 消息 | Markdown/思考折叠/工具卡片 diff/Plan 时间线 | token/费用显示（usage 通道开发中）、模型显示与切换（set_model 透传待验） |
| 安全 | secret 鉴权、常数时间比较、手机端只读默认 | secret 熵强化、限流、威胁模型文档（开发中） |
| 运维 | serve/tunnel/watchdog/wss_test | healthz 探活（开发中） |
| 分发 | file:// 直开 + Android WebView APK（CI 构建） | PWA 安装（需 http(s) 源，见待决事项） |

## P0 — 本轮交付（进行中）

- [x] 界面重构：设计系统、深浅双主题、Profile、会话页升级、PWA 分层、39 单测（前端路已完成）
- [ ] 后端：`qidi/session/list` + `session/load` 透传、半开连接检出、反向请求断线取消、多客户端守卫
- [ ] 后端：secret 熵强化、限流、QR 配对（三层降级）、/healthz、watchdog 联动
- [ ] 前端清标注：协议就绪后移除对应"待后端"占位标注（模型切换/会话列表/token 费用/扫码连接）

## P1 — 能力补全（下一迭代）

- **静态服务决策**：serve Router 目前只有 `GET /ws`。方案 A：serve 内嵌 mobile/ 静态资源（`GET /` 直接返回 App，手机开浏览器即用，部署最简）；方案 B：独立静态托管（nginx/CDN）。倾向 A，需评估二进制体积与安全面（静态资源不泄露 serve 内部状态）。**待用户拍板**
- 文件树 + diff 查看（读文件工具透传 + 前端树组件）
- 斜杠命令面板（桌面端命令清单透传 + 前端执行）
- `@文件`引用（前端选文件 → prompt 注入路径）
- 模型切换点亮（set_model 透传验证后）
- token/费用显示点亮（usage 通道就绪后）

## P2 — 通道扩展

- 语音输入（对接 QidiWork 远程接入方案的语音链路）
- 微信遥控通道（技能已入库：微信传文件 + 遥控服务；与手机 App 双通道并存）
- PWA 推送（权限请求/回合完成的系统级通知，依赖 P1 静态服务）

## P3 — 深度适配

- 多 PC 管理（Profile 升级为设备中心：在线状态/会话数/快捷切换）
- 子代理状态透传（Task 工具的后台代理进度推送到手机）
- memory 搜索入口（手机端检索桌面记忆库）
- MCP 状态面板（已连服务器/工具数/健康态）

## 协议扩展登记（qidi/* 命名空间）

| 方法 | 方向 | 状态 | schema |
|---|---|---|---|
| `qidi/session/list` | 客户端→agent | 开发中 | `{sessions: [{id, title, cwd, updatedAt, messageCount}]}` |
| `qidi/usage` | agent→客户端（ext_notification） | 开发中（若 SessionUpdate 无 usage 变体则启用） | `{inputTokens, outputTokens, costUsd, model}` |
| `session/load`、`session/set_model` | 双向 | ACP 原生已存在，透输即可 | 见 acp crate |

## 红线（全周期有效）

1. 默认绑定 `127.0.0.1:2419` 不得修改；`0.0.0.0` 仅限用户显式 `--bind`
2. 断线处理（取消/排队/重放）不得改变权限判定结果
3. 手机端权限模型与桌面一致：只读工具自动批准，写/执行必人工确认
4. secret-in-URL 是已知泄漏面：威胁模型文档必须随 QR 功能同步交付，二维码=凭证
5. 新默认值/端口/超时单一来源，禁止多处硬编码（R-001/R-007）
