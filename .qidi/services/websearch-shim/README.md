# WebSearch Shim（qidi web_search 本地搜索代理）

把 qidi.exe 的 `web_search` 工具的「模型调用」偷换为**真实网络搜索 + RAG 注入 + 快模型总结**，
零搜索 API 成本实现带引用溯源的联网问答。当前版本 **v2.2**（2026-08-27）。

## 架构

```
qidi.exe ──POST /v1/responses──▶ shim.py (127.0.0.1:8766, Flask)
                                   ├─ 检测 tools[].type == "web_search"
                                   ├─ 三引擎并发竞速抓取（无 key，解析结果页 HTML）
                                   │    中文: 360 → 搜狗 → 必应（偏好序）
                                   │    英文: 必应 → 360 → 搜狗
                                   ├─ 结果注入为 user 消息（要求模型用 [n] 标注引用）
                                   └─ POST /v1/chat/completions ──▶ 127.0.0.1:8765 (LAN 代理)
                                        └─ 响应转回 Responses API 格式（含 url_citation 锚点）
```

## v2 特性

| 特性 | 说明 |
|------|------|
| 三引擎并发竞速 | 同时发起 360/搜狗/必应，取最快命中；更偏好引擎 2s 宽限内到达则替换。最坏延迟 45s → ~15s |
| count 透传 | 读工具定义的 `count`/`max_results`（含 `filters` 内）；`search_context_size` low/med/high → 3/5/8；夹取 1-10 |
| 引用锚点 | 解析正文 `[n]` 标注，`url_citation` 的 start/end_index 定位到真实偏移；未标注时退化为全量列表 |
| 查询缓存 | 120s TTL，键为 (query, count)；容量 256 惰性清理 |
| 日志轮转（v2.2） | RotatingFileHandler 10MB × 5 备份，shim.log 自动截断历史 |

## 文件

| 文件 | 用途 |
|------|------|
| `shim.py` | 主程序（唯一需要维护的文件） |
| `shim_v1_backup_20260826.py` | v1 完整备份（回滚用） |
| `restart_shim.ps1` | 重启脚本：杀旧进程 → WMI 拉新 → 健康验证（基于 $PSScriptRoot，可在任意目录执行） |
| `shim.log` | 运行时自动生成的全链路请求日志（UTF-8，10MB × 5 自动轮转） |

> 2026-08-27 迁移：本目录从 `.qidi/tmp/websearch-shim/` 迁至 `.qidi/services/websearch-shim/`（生产位）。
> 开发期调试脚本（`debug_*.py`、`check_log*.py`）已随 tmp 清理归档至 `_archive/2026-08-27/qidi-tmp/`。

## 运维

```powershell
# 健康检查（版本/缓存/命中统计）
Invoke-RestMethod http://127.0.0.1:8766/health

# 重启（WMI 启动以脱离终端 Job Object；可在任意目录执行）
powershell -ExecutionPolicy Bypass -File .qidi\services\websearch-shim\restart_shim.ps1

# 回滚到 v1
Copy-Item .qidi\services\websearch-shim\shim_v1_backup_20260826.py .qidi\services\websearch-shim\shim.py -Force
powershell -ExecutionPolicy Bypass -File .qidi\services\websearch-shim\restart_shim.ps1
```

开机自启：`启动qidi.bat` 通过健康检查按需拉起本 shim，无需手动干预。

## 已知限制

- HTML 正则解析依赖搜索引擎页面结构，改版即失效（三引擎互为备份缓解）
- 引用锚点仅取 `[n]` 首次出现位置；模型不标注时退化为无锚点全量列表
- 缓存无 single-flight：并发同查询会各自抓取一次（无害，last-write-wins）
- 搜索注入会丢弃原请求的 `instructions`/`temperature`/`top_p`（v1 起即如此，隔离总结行为）
- Flask dev server、无鉴权——仅绑定 127.0.0.1，勿暴露到公网
- 依赖：Python ≥3.9（`shutdown(cancel_futures=)`）、flask、httpx

## 排障速查

| 症状 | 原因/处理 |
|------|-----------|
| web_search 报 connection refused | shim 未运行 → 跑 restart_shim.ps1 |
| 重启后进程又消失 | 用了 Start-Process（Job Object 陪葬）→ 必须用 WMI 方式（脚本已内置） |
| 中文结果差 | 看日志 `engine=` 字段确认是否 360 胜出；360 被风控时自动落到搜狗/必应 |
| 引用锚点 0 条 | 模型未按 `[n]` 标注 → 已自动退化，检查上游模型输出格式 |
