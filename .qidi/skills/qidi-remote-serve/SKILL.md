---
name: qidi-remote-serve
description: >
  qidicode 手机遥控服务的运维手册：家里电脑 qidi agent serve + 首尔反向隧道 + nginx wss 反代
  的完整架构、Windows 计划任务部署细节、以及两个血泪根因（wrapper 10h max_runtime 上限、
  计划任务环境 qidi serve 阻塞在控制台 stdin 必须用 < NUL）。触发词："重启遥控服务"、
  "遥控服务断了"、"手机连不上"、"遥控服务体检"、"qidi serve 起不来"。
allowed-tools: Bash, Read, Write
---

# qidi 手机遥控服务运维手册

## 架构

```
手机App →(公网TLS)→ 首尔 nginx wss://ba.qidiai.ltd/ws
        →(location = /ws 反代)→ 首尔 127.0.0.1:24190
        →(ssh -R 反向隧道)→ 家里电脑 qidi serve 0.0.0.0:2419
```

- 密钥：`_orchestra\serve_secret.txt`（tNd3VQXF9qEYKR8f6AqAj6UM）
- nginx 配置：首尔 `/etc/nginx/sites-enabled/ba.qidiai.ltd`（location = /ws，带 Upgrade 头）
- Windows 计划任务：**QidiServe**（`C:\QidiRemote\qidi_serve.bat`）+ **QidiTunnel**（`C:\QidiRemote\qidi_tunnel.bat`，循环重连版），均 /sc onlogon 开机自启
- bat 必须以 **GBK(936)** 编码写（cmd 原生），中文路径用 **8.3 短路径**（FSO ShortPath 生成）

## 健康检查（一条龙）

```powershell
netstat -an | Select-String ':2419 .*LISTENING'          # 本机 serve
ssh ubuntu@43.155.167.164 "ss -tln | grep 24190"          # 首尔隧道
# wss 握手测试脚本: _orchestra\wss_test.ps1（应返回 PASS）
```

## 恢复命令

```powershell
schtasks /run /tn "QidiServe"; schtasks /run /tn "QidiTunnel"
# 注意：计划任务方式 serve 启动需 ~25 秒，别过早判定失败
```

## ⚠️ 两个血泪根因

1. **wrapper 后台任务有 10 小时 max_runtime 硬上限**（2026-10-15/16 两次断服的直接原因）。
   `timeout: 0` 参数**不能**解除该限制。长驻服务必须走计划任务（系统进程无此限制）。
2. **qidi serve 脱离 wrapper 终端时会阻塞在控制台 stdin**（等待输入，绑端口前死住），
   wrapper 的管道 stdin 立即 EOF 所以能过。**修复：bat 里命令末尾加 `< NUL`**
   （2026-10-16 实测：加 `< NUL` 后计划任务 serve 正常绑定端口）。
   已固化在 `C:\QidiRemote\qidi_serve.bat`。

## 其他坑

- 计划任务方式 serve 启动慢（~25s），别拿 5-12 秒的检查误判失败
- schtasks 的 /tr 参数路径放 `C:\QidiRemote\`（纯 ASCII，避开 GBK 控制台传参乱码）
- 服务依赖家里电脑开机；电脑重启后 onlogon 任务自动拉起两个组件
- 本会话若关闭：serve/隧道已解耦，**不受影响**（这就是计划任务方案的意义）
