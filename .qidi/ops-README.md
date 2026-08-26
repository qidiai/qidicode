# qidicode-portable 运维手册（ops-README）

日常巡检、故障恢复、服务架构速查。面向未来的自己。

## 服务架构

```
qidi.exe (本目录)
   │  普通对话：直接走 config.toml 各模型端点
   │  web_search：POST /v1/responses
   │  记忆向量检索：POST /v1/embeddings（跟默认模型走 8765）
   ▼
websearch shim ── Flask, 127.0.0.1:8766 ── .qidi/services/websearch-shim/shim.py
   │  检测 tools[].type == "web_search" → 三引擎竞速抓真实搜索结果
   │  结果注入上下文 → 转发为 chat/completions
   ▼
QiDi Relay ── 多账户中转, 0.0.0.0:8765 ── G:\程序开发\本地局域网中转-v3
   │  （注意：在便携包【外】运行，py -3 main.py，多账户轮换 16 个模型）
   │  /v1/chat/completions → 上游账号池；/v1/embeddings → 转发到 ↓
   ▼
baize-embed ── 白泽 S5 自研向量模型, 127.0.0.1:8800 ── .qidi/services/baize-embed/server.py
   │  1024 维归一化向量（qidi 记忆 hybrid 检索：FTS5 + sqlite-vec）
   ▼
agnes / chipcloud / deepseek / intern-ai / stepfun 等上游
```

- **Relay（8765）**：上游中转。进程在外部目录，配置/日志/密钥都在那边。
- **Shim（8766）**：websearch 代理，`config.toml` 里 `[model.deepseek-v4-flash]`
  的 `base_url` 指向它，`models.web_search = "deepseek-v4-flash"` 完成接线。
- **Baize（8800）**：白泽 S5 向量服务（自研模型）。qidi 的记忆向量检索跟着
  **默认模型**走 8765，中转 `/v1/embeddings` 路由转发到本服务；模型权重与
  成果包源在 `G:\实验测试\模型研究\白泽成果包_20260825`。切笔记本部署只需改
  中转 config.json 的 `embedding_upstream.base_url`。
- **启动自愈**：`启动qidi.bat` 每次启动先跑 `ops/ensure_services.ps1`（WMI 拉起，
  进程不随终端 Job Object 陪葬），哪个服务没起就拉哪个。

## ops/ 脚本速查（.qidi/ops/）

| 脚本 | 用途 | 用法 |
|------|------|------|
| `ensure_services.ps1` | 自愈拉起 8765/8766/8800，全部已在跑则无操作 | `powershell -ExecutionPolicy Bypass -File .qidi\ops\ensure_services.ps1` |
| `status_check.ps1` | 一页快照：python 进程 / 端口 / shim health / relay 模型数 / shim.log 尾部 | `... -File .qidi\ops\status_check.ps1` |
| `check_models.ps1` | 探测 config.toml 全部端点（自动去重，密钥运行时读取，脚本内零密钥） | `... -File .qidi\ops\check_models.ps1` |
| `test_chat_models.ps1` | 逐模型真实对话冒烟（验证能生成，不只是列表） | `... -File .qidi\ops\test_chat_models.ps1 [-Filter glm]` |
| `test_chat.ps1` / `test_endpoints.ps1` | 单发对话 / 端点轻探测 | 同上 |
| `validate_config.py` | 校验 config.toml：死端口、fallback 悬空、旧 IP 残留 | `python .qidi\ops\validate_config.py` |
| `session_stats.py` | 会话目录体量/日期分布统计（归档决策依据） | `python .qidi\ops\session_stats.py` |

## shim 运维（websearch-shim）

```powershell
# 健康（版本/缓存/命中数）
Invoke-RestMethod http://127.0.0.1:8766/health

# 重启（杀旧→WMI拉新→验证；可在任意目录执行）
powershell -ExecutionPolicy Bypass -File .qidi\services\websearch-shim\restart_shim.ps1

# 回滚 v1
Copy-Item .qidi\services\websearch-shim\shim_v1_backup_20260826.py .qidi\services\websearch-shim\shim.py -Force
# 然后跑上面的 restart
```

版本历史：v1（基础转发+搜索注入）→ v2.1（三引擎竞速/锚点/缓存）→
**v2.2**（日志轮转 10MB×5；迁入 services/ 生产位）。详见 `services/websearch-shim/README.md`。

## baize-embed 运维（白泽 S5 向量服务）

```powershell
# 健康（引擎加载状态 + 维度 1024）
Invoke-RestMethod http://127.0.0.1:8800/health

# 重启（杀 8800 占用 → WMI 拉新 → 等模型加载完，约 10s）
powershell -ExecutionPolicy Bypass -File .qidi\services\baize-embed\restart_baize.ps1

# 直接冒烟（走中转 8765 全链路，应返回 1024 维向量）
Invoke-RestMethod http://127.0.0.1:8765/v1/embeddings -Method Post -Headers @{Authorization="Bearer <relay密钥>"} -ContentType 'application/json' -Body '{"model":"baize-s5","input":["测试"]}'
```

要点：
- **路由链**：qidi 记忆向量检索跟默认模型走 8765 → relay `/v1/embeddings` →
  本服务。中间任何一环挂掉，qidi 会**静默回退 FTS5 纯文本检索**（日志才可见），
  所以怀疑向量检索失效时先看 8800 health。
- **维度强校验**：请求 `dimensions != 1024` 直接 400，配置错了一眼可见，
  不会静默产出错误向量。
- **质量评估（08-27）**：S5 在真实记忆库上 hit@1/5=3/5，弱于词法基线（短文本对
  模型 vs 长文档检索），详见 `.qidi/docs/s5-retrieval-eval.md`；后端可插拔，
  换 bge-m3 等只需改 server.py 引擎加载，链路零改动。基准：
  `services/baize-embed/bench_retrieval.py`。
- **切换笔记本部署**：改中转 `config.json` 的 `embedding_upstream.base_url`
  即可，本目录无需动。FastAPI 模板见 `services/embedding-bridge/`。
- 日志 `baize_embed.log`（10MB×5 轮转）在服务目录内。

## 日志管理

| 日志 | 位置 | 轮转 |
|------|------|------|
| shim.log | `.qidi/services/websearch-shim/` | ✅ 自动（10MB × 5 备份，v2.2 起） |
| unified.jsonl | `.qidi/logs/` | ❌ 手动，目前 2MB，超 100MB 时可截断 |
| api-error.log | `.qidi/` | ❌ 手动，qidi 运行时产物 |
| relay 日志 | `G:\程序开发\本地局域网中转-v3\` | relay 侧自理 |

## 历史端口备忘

- **8790**（bridge）、**9800**（旧 glm 桥）：已于 2026-08-27 归档废弃
  （`_archive/2026-08-27/qidi-dirs/bridge/`）。status_check 仍观察这两个端口，
  出现监听反而是异常信号。

## 归档区（_archive/）

- `_archive/2026-08-26/`：早期根目录清理（78 项）
- `_archive/2026-08-27/`：P2/P3 大清理，**全部是移动而非删除**：
  - `sessions/` 94 项旧会话（920MB）
  - `qidi-root/` 105 个一次性脚本、`qidi-tmp/` 1404 项
  - `qidi-dirs/` bridge / build(旧 proxy-v2,v2.1) / backup_frontend / data-pipeline(152MB) / upload-proxy
  - `hcl-mirror/` ⚠️ huwang-cloud-link **比根目录原件新的工作快照**（含 08-20 的
    PHP/Vue 修改与 .env），勿当垃圾清理
  - `_tools/` 本次迁移所用脚本（含 websearch 端到端测试）
- 会话归档规则：最新文件早于 2 天前 → 归档；近期会话保留在 `.qidi/sessions/` 供 `/resume`

## Git 追踪策略

白名单模式（见根 `.gitignore`）：只追踪启动器 / docs / skills / ops / services /
bridge 已移除。**config.toml（含全部真实密钥）、sessions、memory、exe 均不入库**。
密钥只允许存在于 config.toml（未追踪）中 —— check_models.ps1 / test_chat_models.ps1
均在运行时读取，脚本本身保持零密钥（2026-08-27 已完成脱敏）。

## 常见故障

| 症状 | 处理 |
|------|------|
| web_search 报 connection refused | `restart_shim.ps1`；还不行跑 `ensure_services.ps1` 看是 relay 还是 shim 挂了 |
| 重启后进程消失 | 必须走 WMI 拉起（restart/ensure 脚本已内置），Start-Process 会随 Job Object 陪葬 |
| relay 429 | 多账户轮换的临时限流，重试即可；持续 429 去外部 relay 目录查账户状态 |
| 模型列表对但对话失败 | `test_chat_models.ps1 -Filter <名字>` 定位到具体模型 |
| .ps1 报"字符串缺少终止符" | 含中文的 ps1 必须存成 UTF-8 **带 BOM**（PS5.1 无 BOM 按 GBK 解析） |
