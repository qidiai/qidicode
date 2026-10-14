# qidicode 手机端瘦客户端（ACP over WebSocket）

在**手机浏览器**里遥控电脑上正在运行的 qidicode：连接电脑上的远程 Agent 服务器，
用 ACP（Agent Client Protocol）v1 协议开会话、发消息、看流式输出、批工具权限。

纯 Vanilla JS，**零依赖 / 零 CDN / 零构建工具**，可从 `file://`、静态 `http://`
或 Android WebView 的 assets 直接打开。后续 T2 会把它包进 WebView 打成 APK。

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

本客户端是三个静态文件（`index.html` / `style.css` / `app.js`，本说明为 `README.md`），
任选一种方式：

1. **任一静态服务器**（仅在 http 下才自动推断地址）：把 `mobile/` 目录下文件放到任意静态
   Web 服务器（如 `python -m http.server`）根目录，手机浏览器访问
   `http://<电脑IP>:8000/` 即可；此时"服务器地址"会自动预填为
   `ws://<电脑IP>:8000/ws`，请**改成电脑上 qidi 的实际地址与端口**（如 `:2419`）。
   > 预填有守卫：**只在页面以 `http://` 打开时**才会预填 `ws://<页面host>/ws`；
   > 以 `https://` 或 `file://` 打开时"服务器地址"一律留空，需手填。
   > 另外，用了静态服务器就请确认它**不记录含 query 的 access log**（见第三节密钥警告）。
2. **直接发送文件**：把三个文件（至少 `index.html`/`style.css`/`app.js`）一起发到手机，
   用浏览器以 `file://` 打开 `index.html`。`file://` 下无法自动推断地址，
   "服务器地址"需手动填写 `ws://<电脑IP>:2419/ws`。

> 注意：用静态服务器托管页面时，页面端口（如 8000）与 WebSocket 端口（2419）
> 是两个不同端口，请按上面的说明分别填对。

打开后：填写**服务器地址**、**访问密钥（Secret）**、**项目路径（cwd，电脑上的绝对路径）**，
点"连接"。三项都会被记住（见下节安全说明），下次自动填充。

## 三、安全注意事项

- **明文 WebSocket，无 TLS**：协议是 `ws://` 而非 `wss://`，链路上的数据（含密钥与对话
  内容）**不加密**。请仅在可信局域网内使用，切勿在公网 / 公共 Wi-Fi 暴露 2419 端口。
- **⚠ 密钥走 URL query 参数，泄露面更大**：浏览器的 WebSocket 不能自定义请求头，所以本
  客户端必须把密钥放在 `?server-key=<secret>` 里，即
  `ws://<host>:2419/ws?server-key=<你的密钥>`。这带来两个额外风险：
  1. **密钥会出现在服务端（以及中间代理）的 HTTP access log 里**。因此**不要**把 `mobile/`
     目录托管在会记录 access log 的服务器上——即使那台服务器只是用来分发这三个静态文件，
     "页面地址"与"WS 地址"也会一起进日志。推荐直接用 `file://` 打开，或使用**关闭
     access log / 只记路径不记 query** 的托管方式。
  2. 密钥也会出现在浏览器历史记录、书签与开发者工具的网络面板中，**同机他人可见**。
     用完请在菜单点"断开连接"，并清除该站点数据。
- **PC 端 qidi 必须已登录**：远程 Agent 服务器只能"接管"一个已经登录的 qidi 主程序。
  若电脑上的 qidi 处于未登录状态，`session/new` 会返回 `auth_required`，客户端会弹出
  "PC 端 qidi 未登录，请先在电脑上完成登录"并退回连接页。请**先在电脑上完成登录**再连接。
- **密钥明文存 localStorage**：`qidiMobile.secret` 以明文保存在手机浏览器，
  同源脚本/被授权的调试工具可读取。这是为"免重复输入"做的 MVP 权衡；请避免在
  共享设备上保存密钥，用完可在菜单"断开连接"或清除浏览器数据。
- **访问范围**：`protocolVersion` 协商成功后的会话由服务端 Agent 执行，cwd 指向的目录会被
  Agent 读写。请确认密钥只发给可信的人，绑定 `0.0.0.0` 时尤其注意。
- **XSS 防护**：客户端对一切来自服务端与用户的字符串做 HTML 转义后再入 DOM（Markdown
  渲染器第一步即整体转义），链接仅允许 `http/https`。但**根本信任边界仍在服务端**。

## 四、已知 MVP 限制

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
- 仅实现受限 Markdown：**不支持**表格、脚注、嵌套结构、图片语法；代码块不做语法高亮。
- 服务端 `session/update` 中未知种类会被忽略（仅 `console.debug`），不会报错。
- 会话恢复依赖服务端 `agentCapabilities.loadSession`；为 `false` 时每次都新建会话。
- 未实现消息本地持久化；刷新页面即丢失当前会话视图（会话仍存活于服务端）。

## 五、本地存储键

| 键 | 内容 |
| --- | --- |
| `qidiMobile.url` | 服务器地址 |
| `qidiMobile.secret` | 访问密钥（明文，见上） |
| `qidiMobile.cwd` | 项目路径 |
| `qidiMobile.sessionId` | 当前会话 id（用于重连/重开时尝试恢复） |

## 六、协议速览（实现依据）

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
