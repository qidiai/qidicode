# qidicode 手机端瘦客户端（ACP over WebSocket）

在**手机浏览器**里遥控电脑上正在运行的 qidicode：连接电脑上的远程 Agent 服务器，
用 ACP（Agent Client Protocol）v1 协议开会话、发消息、看流式输出、批工具权限。

纯 Vanilla JS，**零依赖 / 零 CDN / 零构建工具**，可从 `file://`、静态 `http://`
或 Android WebView 的 assets 直接打开。后续 T2 会把它包进 WebView 打成 APK。

## 〇、文件构成（mobile-ui 重构版）

| 文件 | 作用 |
| --- | --- |
| `index.html` | 三视图 + 抽屉 + 两个 bottom sheet 的骨架（连接页 / 会话页 / 扫码 / 权限） |
| `style.css` | 设计系统：CSS 自定义属性令牌、深/浅双主题、移动端优先适配 |
| `app.js` | Part 1 纯函数核心 `QidiMobileCore`（UMD，可 `node --test` 直测）+ Part 2 DOM 应用 |
| `manifest.webmanifest` / `sw.js` / `icon.svg` / `icon-*.png` | PWA 分层（http(s) 渐进增强） |
| `test/*.test.js` | `node --test` 零依赖测试（markdown / diff / URL / profile / 主题 / 工具函数） |
| `README.md` | 本说明 |

## 一、在电脑上启动服务端

```bash
qidi agent serve --bind 0.0.0.0:2419 --secret <你的密钥>
```

要点：

- **必须用 `0.0.0.0`**：默认 `127.0.0.1:2419` 只能本机访问；`0.0.0.0:2419` 才允许
  局域网内其它设备（手机）连入。端口 `2419` 可自行更换。
- **`--secret` 建议自设 16 位以上随机串**（如 `openssl rand -hex 16` 的结果）。
  密钥就是访问口令，请勿用弱口令或默认值。
- 启动后 WebSocket 端点为：`ws://<电脑局域网IP>:2419/ws`
  （例如电脑 IP 是 `192.168.1.10`，则地址为 `ws://192.168.1.10:2419/ws`）。
- 手机和电脑需在**同一局域网**；若连不上，检查 Windows 防火墙是否放行了 2419 端口。

### serve 启动与权限须知

`qidi agent serve` 子命令只接受 `--bind` 与 `--secret`（或等价的 `QIDI_AGENT_SECRET`
环境变量）。父级的 yolo 旗标（`--yolo-tools` / `--ack-no-sandbox` 等）**必须写在子命令
`serve` 之前**，例如：

```bash
qidi agent --yolo-tools read_file,grep serve --bind 0.0.0.0:2419 --secret <token>
```

- **继承用户配置**：`serve` 会继承用户 `config.toml` 里已有的 yolo 设置。若 config 开启了
  **全量 yolo**，在没有 OS 沙箱的机器上启动会被**直接拦截**（拒绝启动），并提示需要
  `--ack-no-sandbox` / `--yolo-tools` 或启用沙箱。
- **只读工具通常不弹窗**：`list_dir` / `read_file` / `grep` 等**只读**工具即使在权限模式下
  也通常**自动批准**，不会向客户端发权限请求；**权限弹窗只针对写 / 执行类工具**。
- **对手机端的含义**：PC 端的 yolo 配置会**静默吞掉手机端的权限确认机会**（一律自动批准）。
  远程遥控时若想**逐项确认**，PC 端**不要开全量 yolo**；可用 `--yolo-tools read_file,grep`
  只对只读工具放行，把写 / 执行类留给手机端逐项批准。

## 二、如何用手机打开

### file://（核心通道，永远可用）

把 `index.html` / `style.css` / `app.js` 三个文件一起发到手机，用浏览器以 `file://`
打开 `index.html`。`file://` 下无法自动推断地址，"服务器地址"需手动填写
`ws://<电脑IP>:2419/ws`。**PWA 的 Service Worker 在 `file://` 下静默跳过**（协议探测），
不影响任何功能。

### http(s)（PWA 渐进增强）

把 `mobile/` 目录下文件放到任意静态 Web 服务器根目录，手机浏览器访问
`http://<电脑IP>:8000/`：

- 此时"服务器地址"会自动预填为 `ws://<电脑IP>:8000/ws`（**仅 `http:` 预填**；
  `https:` / `file:` 一律留空，需手填），请**改成电脑上 qidi 的实际地址与端口**（如 `:2419`）。
- 页面端口（如 8000）与 WebSocket 端口（2419）是**两个不同端口**，请分别填对。
- **Service Worker 自动注册**（仅 http/https），只缓存静态壳（index/style/app/manifest/
  图标），stale-while-revalidate；WS 连接、POST、跨源请求一律旁路。可"添加到主屏幕"
  以 standalone 方式启动。
- 用了静态服务器就请确认它**不记录含 query 的 access log**（见第三节密钥警告）。

## 三、界面结构（mobile-ui 重构版）

```
┌──────────────── 连接页 view-connect ────────────────┐
│ 品牌头图（logo + qidicode 手机遥控 + 副标题）            │
│ ┌ 最近会话 ┐ 本地记录，点击直连（恢复原会话）            │
│ ┌ 已保存的连接 ┐ Profile 列表：点击直连 / ✎编辑 / 🗑删除  │
│ ┌ 连接信息 ┐ 名称 + 地址 + 密钥 + cwd                  │
│   [连接] [保存配置]                                    │
│   [📷 扫码连接] ← 降级：粘贴 ws:// 链接自动解析填入      │
└──────────────────────────────────────────────────────┘
┌──────────────── 会话页 view-session ────────────────┐
│ 横幅（重连中/失败 + 重试按钮）                          │
│ 头部：●状态灯 标题 / cwd · sid · 模型(待后端) · ⋮菜单    │
│ 消息流：                                              │
│   用户气泡 / 助手气泡（完整 Markdown）                   │
│   思考块（默认折叠，可展开）                            │
│   工具卡：状态图标 + 名称 + 耗时；输入/输出/差异可折叠     │
│   计划卡：时间线（步骤 + 状态）                          │
│   空状态：新会话能力提示卡                              │
│ composer：[@ 文件] [/ 命令] [🖼 图片] [输入框] [发送/停止]│
└──────────────────────────────────────────────────────┘
┌──────────────── 抽屉 drawer（⋮ 菜单）────────────────┐
│ 会话信息（标题/cwd/sid）                               │
│ [新会话] [断开连接]                                    │
│ 最近会话列表（点击重连）                                │
│ 模型切换（待后端）/ 斜杠命令面板（待后端）                │
│ 主题：深色 / 浅色 / 跟随系统                            │
│ 清空本地数据 / 关于·版本                                │
└──────────────────────────────────────────────────────┘
```

### 主题机制

- **默认深色**；无记录时跟随 `prefers-color-scheme`；localStorage 记住手动选择。
- `index.html` head 内联脚本在首屏绘制前定 `data-theme`，无闪白/闪黑。
- 令牌体系见 `style.css` `:root`（深色）与 `:root[data-theme="light"]`（浅色）：
  颜色 / 间距 / 圆角 / 字号 / 阴影 / z-index 全阶。
- 连接状态灯：**绿=已连接 / 黄=连接中·重连中（脉冲） / 红=断开**。

### 移动端适配

- `safe-area-inset`（刘海/小白条）、`100dvh` + `visualViewport` 实测高度
  （`--app-h`，输入法弹起时视图收缩、composer 不被挡）。
- 所有可点元素 ≥ 44px；`interactive-widget=resizes-content`；横竖屏/矮屏有压缩样式；
  `prefers-reduced-motion` 降级动画。

## 四、占位功能清单（可见标注"待后端"，非假按钮）

| 入口 | 现状 |
| --- | --- |
| composer `@ 文件` / `/ 命令` / `🖼 图片` | 按钮可见（带"待后端"黄点），点击 toast 明示"待后端支持" |
| 抽屉 `模型切换` | 显示"默认 · 待后端"，点击 toast 明示 |
| 抽屉 `斜杠命令面板` | 显示"待后端"，点击 toast 明示 |
| 会话页头部 `模型名` | 占位文本"默认模型 · 待后端" |
| `扫码连接` | 按钮+说明；摄像头扫码**待后端**，当前降级为粘贴 `ws://` 链接自动解析（server-key/cwd） |

**没有语音按钮**（按审计建议直接隐藏，不做可点无响应的入口）。

## 五、安全注意事项

- **明文 WebSocket，无 TLS**：协议是 `ws://` 而非 `wss://`，链路上的数据（含密钥与对话
  内容）**不加密**。请仅在可信局域网内使用，切勿在公网 / 公共 Wi-Fi 暴露 2419 端口。
- **⚠ 密钥走 URL query 参数，泄露面更大**：浏览器的 WebSocket 不能自定义请求头，所以本
  客户端必须把密钥放在 `?server-key=<secret>` 里，即
  `ws://<host>:2419/ws?server-key=<你的密钥>`。这带来两个额外风险：
  1. **密钥会出现在服务端（以及中间代理）的 HTTP access log 里**。因此**不要**把 `mobile/`
     目录托管在会记录 access log 的服务器上——即使那台服务器只是用来分发这几个静态文件，
     "页面地址"与"WS 地址"也会一起进日志。推荐直接用 `file://` 打开，或使用**关闭
     access log / 只记路径不记 query** 的托管方式。
  2. 密钥也会出现在浏览器历史记录、书签与开发者工具的网络面板中，**同机他人可见**。
     用完请在菜单点"断开连接"，并清除该站点数据。
- **PC 端 qidi 必须已登录**：远程 Agent 服务器只能"接管"一个已经登录的 qidi 主程序。
  若电脑上的 qidi 处于未登录状态，`session/new` 会返回 `auth_required`，客户端会弹出
  "PC 端 qidi 未登录，请先在电脑上完成登录"并退回连接页。请**先在电脑上完成登录**再连接。
- **密钥明文存 localStorage**：`qidiMobile.secret` / `qidiMobile.profiles`（含 secret 字段）
  以明文保存在手机浏览器，同源脚本/被授权的调试工具可读取。这是为"免重复输入"做的 MVP
  权衡；请避免在共享设备上保存密钥，用完可"断开连接"、删除对应 Profile 或
  "清空本地数据"。
- **访问范围**：`protocolVersion` 协商成功后的会话由服务端 Agent 执行，cwd 指向的目录会被
  Agent 读写。请确认密钥只发给可信的人，绑定 `0.0.0.0` 时尤其注意。
- **XSS 防护**：客户端对一切来自服务端与用户的字符串做 HTML 转义后再入 DOM（Markdown
  渲染器对非代码文本整体转义；代码块经专用 tokenizer 逐段转义后高亮；diff/表格单元格同样
  转义），链接仅允许 `http/https`。但**根本信任边界仍在服务端**。

## 六、已知 MVP 限制

- **重连可恢复历史（v2 起）**：断线后客户端会自动重连（退避 1/2/4/8/16s，5 轮失败转手动
  重试）。重连成功会重新 `initialize`；若 localStorage 里有 `qidiMobile.sessionId` 且服务端
  支持 `agentCapabilities.loadSession`，则用 `session/load` 恢复会话，服务端的 **replay 历史
  会流式重建到消息区，不会清空已有内容**。只有"服务端已无可恢复的历史"（load 失败或
  `loadSession=false`）时才会新建会话并清空消息区。
- **⚠ 重复 `initialize` 的 auth 降级隐患（服务端行为，客户端只能规避）**：同一条连接上重复
  发送 `initialize` 会覆盖第一次协商出来的状态，可能把连接**降级**为未鉴权态；此后
  `session/new` 会返回 `auth_required`，客户端只能退回连接页让用户重新连接。本客户端
  保证**每条 WebSocket 只发一次 `initialize`**（重连走全新 socket），并已加 15 秒
  `initialize` 超时（超时即断开重连）；但如果后续改动引入"同一连接重发 initialize"，
  就会踩到这个坑。
- **明文 `ws://`，无 TLS**：无传输加密（同上）。
- **secret 明文存 localStorage 且出现在 URL query**：无加密、无过期机制（同上）。
- **Markdown 能力（自研渲染器，已支持）**：围栏代码块（含流式未闭合）+ 轻量关键字高亮
  （关键字/字符串/数字/注释/函数名/全大写常量六类，css/html 不着色）、行内 code、粗/斜/删
  除线、http(s) 链接、`#`~`####` 标题、水平线、有序/无序/**缩进嵌套**列表、引用块（可嵌
  套）、**表格**（含 `:---`/`:--:`/`---:` 对齐）、段落内 `<br>`。**不支持**：图片语法、
  脚注、任务列表、HTML 直通、单元格内转义竖线。
- **工具卡 diff**：输入/输出均为文本时做轻量 LCS 行 diff（单侧 >600 行降级为全删全增，
  防止大文件卡死 UI）。
- **会话恢复依赖服务端 `agentCapabilities.loadSession`**；为 `false` 时每次都新建会话。
- **未实现消息本地持久化**：刷新页面即丢失当前会话视图（会话仍存活于服务端；"最近会话"
  只存 profileId + sid + 标题 + 时间戳，不含消息内容）。
- 服务端 `session/update` 中未知种类会被忽略（仅 `console.debug`），不会报错。
- **占位功能**（见第四节清单）等后端支持。

## 七、本地存储键

| 键 | 内容 |
| --- | --- |
| `qidiMobile.url` / `qidiMobile.secret` / `qidiMobile.cwd` | v1 兼容键：上次表单值（输入即存、预填用） |
| `qidiMobile.sessionId` | 当前会话 id（用于重连/重开时尝试恢复） |
| `qidiMobile.profiles` | 连接 Profile 列表 `[{id,name,url,secret,cwd,createdAt,lastUsedAt}]`（上限 50，secret 明文） |
| `qidiMobile.recent` | 最近会话 `[{profileId,sid,title,ts}]`（上限 20，按时间倒序） |
| `qidiMobile.theme` | `dark` / `light` / `system`（缺省=跟随系统） |
| `qidiMobile.activeProfile` | 当前连接的 Profile id |

"清空本地数据"（抽屉 → 设置）会删除以上全部键。

## 八、协议速览（实现依据）

JSON-RPC 2.0 / camelCase；**1 个 WS text 帧 = 1 条完整消息**。鉴权用 query 参数
`?server-key=<secret>`（浏览器 WebSocket 不能自定义 header）：

`initialize`（`protocolVersion` 必须是**数字 1**，`clientCapabilities` 为空对象 `{}`，
不得声明 fs/terminal 能力）→ `session/load`（恢复，必须同时带 `sessionId`/`cwd`/`mcpServers`
三字段；应答里没有 `sessionId`，沿用请求时的值；replay 历史会先于应答到达）或
`session/new` → `session/prompt`（回合进行中收到 `session/update` 的 `user_message_chunk` /
`agent_message_chunk` / `agent_thought_chunk` / `tool_call` / `tool_call_update` / `plan` /
`session_info_update`；回合结束以 `session/prompt` 应答含 `stopReason` 或
`_x.ai/session_notification` 里的 `turn_completed` 为信号，先到为准、幂等）→ 取消用
`session/cancel`（通知，无应答）；服务端反向请求 `session/request_permission` 需按用户选择
回传 `selected`/`cancelled`（带原 id）；**带 `id` 的未识别方法一律回 `-32601`**（防止 agent
侧永久挂起）；菜单"断开连接"与"新会话"都发 `session/close`（成功失败均继续）。

## 九、测试与纯函数核心

`app.js` Part 1 是纯函数核心 `QidiMobileCore`（UMD：浏览器挂 `window`，node 下
`require("./app.js")` 直接导出），覆盖：`renderMarkdown` / `highlightCode` /
`escapeHtml` / `prettyInput` / `diffLines` / `renderLineDiffHtml` / `parseWsUrl` /
`buildWsUrl` / Profile 与最近会话序列化 / `resolveTheme` / `cwdTail`。

```bash
node --test "mobile/test/*.test.js"   # 39 个用例全绿（markdown / diff / url / profile / 主题 / 工具函数）
```

> 注：本机 Node v24 下 `node --test mobile/test`（目录形式）报 MODULE_NOT_FOUND，
> 须用 glob 形式（PowerShell 里记得给模式加引号，防止通配符被 shell 抢先展开）。

v1 全部界面功能在重构后**一条不少**保留（store/load/drop、断线重连流、权限队列、
回合状态机、工具卡、计划卡、菜单动作等，函数级对照见交付报告）。
