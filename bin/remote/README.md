# qidicode 手机遥控服务部署套件（bin/remote）

「手机 App → 公网 → 家里电脑 qidicode」的完整服务端部署物。

## 架构

```
手机App →(公网TLS)→ 首尔 nginx wss://ba.qidiai.ltd/ws
        →(nginx location = /ws 反代)→ 首尔 127.0.0.1:24190
        →(ssh -R 反向隧道)→ 家里电脑 qidi agent serve 0.0.0.0:2419
```

| 文件 | 作用 | 部署位置 |
|---|---|---|
| `qidi_serve.bat` | 家里电脑的 agent serve 守护 | 计划任务 `QidiServe`（开机自启） |
| `qidi_tunnel.bat` | 首尔反向隧道（循环重连） | 计划任务 `QidiTunnel`（开机自启） |
| `qidi_watchdog.bat` | 每 30 分钟巡检 `/healthz` 探活，失败则拉起对应组件 | 计划任务 `QidiWatchdog` |
| `wss_test.ps1` | 公网 wss + ACP 握手端到端自检 | 手动运行 |

手机端 = 仓库 `mobile/`（浏览器直接用）+ `android/`（WebView APK，GitHub Actions 自动打包）。

## 部署步骤（新机器）

1. **密钥**（绝不入库）：生成 24 位随机串，写入 `%USERPROFILE%\.qidi\remote_secret.txt`（单行明文）
2. **计划任务**（三个命令，管理员或当前用户均可）：
   ```bat
   schtasks /create /tn "QidiServe"   /tr "C:\部署目录\qidi_serve.bat"   /sc onlogon /rl limited /f
   schtasks /create /tn "QidiTunnel"  /tr "C:\部署目录\qidi_tunnel.bat"  /sc onlogon /rl limited /f
   schtasks /create /tn "QidiWatchdog" /tr "C:\部署目录\qidi_watchdog.bat" /sc minute /mo 30 /rl limited /f
   schtasks /run /tn "QidiServe" & schtasks /run /tn "QidiTunnel"
   ```
   建议把 bat 拷到**纯 ASCII 路径**（如 `C:\QidiRemote\`）——避开 GBK 控制台传参乱码
3. **首尔侧**（一次性）：nginx 站点加 `location = /ws` 反代（`proxy_pass http://127.0.0.1:24190` +
   WebSocket Upgrade 头 + `proxy_read_timeout 3600s`）；服务器 sshd 允许当前用户密钥登录
4. **验证**：`powershell -File wss_test.ps1` → 应输出 `E2E_RESULT: PASS`

## ⚠️ 两个血泪根因（复现过两次断服）

1. **qidi serve 脱离终端环境会阻塞在控制台 stdin**——计划任务/Start-Process 方式起服务时
   进程活着却永远绑不上端口。**修复：命令末尾 `< NUL`**（wrapper 管道 stdin 立即 EOF 所以
   交互式终端里发现不了这个坑）。
2. **长驻服务绝不能跑在 CLI 会话的后台任务里**——wrapper 有 10 小时 max_runtime 硬上限
   （`timeout: 0` 参数不能解除），到点强杀。必须用系统计划任务（无时长限制 + 开机自启）。

## 威胁模型（必读）

本套件把「手机 App → 公网 → 家里电脑 agent」串起来，安全边界就压在两处：**密钥**和
**网络暴露面**。上线前必须理解下面四条，否则等于把家里电脑的 agent 权限交出去。

1. **secret-in-URL 的泄漏面**：鉴权密钥作为 `?server-key=<secret>` query 参数随 URL 走。
   - 传输中：公网段在 TLS 内加密（nginx → 手机）；本地段走 loopback 不经过网络。
   - 静止中：**会写进 nginx access log、系统代理日志、浏览器历史、curl -v 输出、调试
     截图**。任何能读到这些地方的人都能拿到密钥 → 完整控制 agent。
   - 对策：不把带 key 的 URL 贴进聊天/issue/commit；反向代理侧对 `/ws` 关掉 query
     日志；密钥**绝不入库**（`remote_secret.txt` 在 `%USERPROFILE%\.qidi\`，git 之外）。

2. **LAN-only 假设**：`qidi agent serve` 默认绑 `127.0.0.1:2419`，**严禁改成 0.0.0.0 或
   局域网地址**。任何"手机直连家里电脑 IP"的方案都是把未鉴权面（或弱鉴权面）暴露到
   局域网/公网，属于越界。远程访问唯一通道是 ssh 反向隧道 + 首尔 nginx TLS 反代。

3. **QR = 凭证**：启动时打印的配对二维码内容 = 完整 ws URL + secret。任何看到该 QR 的
   人（摄像头、肩窥、截图转发、会议投屏）即等于拿到 agent 控制权。配对时保证终端不被
   录屏/拍照，配对完不传播；已泄漏就改 `remote_secret.txt` 并重启 serve 使旧 key 作废。

4. **一次密钥换全量控制**：密钥通过后即可 `session/new`、`session/prompt`、写/执行工具。
   只有只读工具默认自动批准（`--yolo-tools read_file,grep`），写/执行类仍需手机端逐次确认
   ——这层**人工确认是最后防线**，切勿为省事把 yolo 扩到写工具。

## 运维速查

```powershell
# 健康检查
netstat -an | findstr ":2419"                          # 本机 serve
ssh ubuntu@43.155.167.164 "ss -tln | grep 24190"       # 首尔隧道
powershell -File wss_test.ps1                          # 全链路

# 手动恢复（计划任务方式 serve 启动约需 25 秒，勿提前判死）
schtasks /run /tn "QidiServe"; schtasks /run /tn "QidiTunnel"
```

安全边界：wss 有 TLS；密钥走 URL query（TLS 内加密，但会进 nginx access log——自用服务器可接受）；
只读工具自动批准，写/执行类需手机端确认（serve 启动参数 `--yolo-tools read_file,grep`）。
