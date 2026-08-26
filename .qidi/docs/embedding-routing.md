# qidi 向量检索路由机制——破解报告与自研模型接入方案

> 调查日期：2026-08-27 · 方法：伪造 OpenAI 兼容探针服务器 + `--debug-file` 日志抓取 + 二进制字符串分析
> 结论：**可以用自己的向量模型**，且已全程验证打通。

## 一、破解结论（已实测验证）

**embedding 客户端完全复用「默认模型」的 SamplerConfig（base_url + api_key）**，
`[memory.embedding]` 的配置只决定请求体内容，不决定发往哪里。

实测证据（探针服务器 127.0.0.1:8799 抓到的真实请求）：

```json
POST /v1/embeddings
{"model":"probe-embed","input":["<记忆块文本>..."]}

POST /v1/embeddings   ← 查询向量化（首轮注入触发）
{"model":"probe-embed","input":["请使用 memory_search 工具搜索关键词：websearch shim 位置"],"dimensions":64}
```

把默认模型切到探针后：32 个记忆文件全部成功向量化、`embedding batch failed` 从 30+ 次降为 **0 次**、
向量写入 index.sqlite（WAL 增长 4MB）、无 "falling back to FTS-only"。

### 路由规则表（全部实测）

| 配置项 | 是否影响 embedding 路由 | 实测证据 |
|---|---|---|
| `[models] default` 指向的模型条目的 base_url/api_key | ✅ **唯一决定因素** | 默认模型切到探针 → /embeddings 全部到达探针 |
| `models_base_url`（[endpoints]） | ❌ | 指向探针时只收到 /v1/models + /v1/responses（会话标题），无 /embeddings |
| `QIDI_XAI_API_BASE_URL` 环境变量 | ❌ | 设置后探针零命中（该变量属 xAI 云端客户端，管 OAuth 云调用） |
| `CF_API_BASE_URL` / `--xai-api-base-url`（agent 子命令） | ❌ | 同上，且只绑定在 agent 子命令上 |
| `[model."xxx"]` 注册与 `[memory.embedding].model` 同名的条目 | ❌ | 注册 probe-embed@8799 条目但默认模型仍是 glm-5.3 时，embedding 仍打 8765 |
| `[memory.embedding] model / dimensions` | ⚠️ 只进请求体 | model → 请求体 "model" 字段；dimensions → 查询请求的 "dimensions" 字段 |

### 其他已确认事实

- `provider` 字段**只支持 `"api"`**（二进制内嵌文档原话 "currently `"api"`"），无本地推理模式
  （fastembed/onnx/gguf 全部不存在于二进制中）。
- 无认证也能发请求：本机无 auth.json/auth.toml，embedding 请求照发（带默认模型的 Bearer key）。
- 失败自动降级：embedding 不可达/404 时静默回退 FTS-only 检索（WARN 日志），不影响使用。
- 维度自适应：配置 dimensions 与库内不一致时自动重建 chunks_vec（"embedding dimension mismatch,
  recreating chunks_vec"），换模型无需手工清库。
- 索引写入在会话启动后台执行（"MEMORY_REINDEX: background reindex"），每次会话增量补齐缺失向量。

## 二、为什么一直没通（历史故障根因）

默认模型 glm-5.3 → `http://127.0.0.1:8765/v1`（QiDi Relay 中转），而中转只有
`/v1/chat/completions` 等聊天路由，没有 `/v1/embeddings` → 每次会话启动 30+ 次
`404 Not Found`（Werkzeug 页面）→ 全部跳过 → 向量表 0 行 → 检索永远 FTS-only。

## 三、接入自研模型（笔记本）部署方案

架构（关键洞察：**中转是必经枢纽**，因为 embedding 跟着默认模型走）：

```
qidi (本机)
  └─ 默认模型 glm-5.3 → http://127.0.0.1:8765/v1 (QiDi Relay 中转)
       ├─ /v1/chat/completions → 各上游聊天模型（现状，不动）
       └─ /v1/embeddings       → 转发 → 笔记本 http://<笔记本IP>:<端口>/v1/embeddings（自研模型）
```

### 步骤 1：笔记本侧——OpenAI 兼容 embedding 服务

模板见 `.qidi/services/embedding-bridge/laptop_embed_server.py`（FastAPI 单文件），
加载自研模型后暴露 `/v1/embeddings`，局域网可访问。

### 步骤 2：中转侧——加 /v1/embeddings 转发路由

`G:\程序开发\本地局域网中转-v3\routes\proxy.py` 末尾追加（补丁全文见
`.qidi/services/embedding-bridge/README.md`），并在 config.json 加：

```json
"embedding_upstream": {
  "base_url": "http://<笔记本IP>:<端口>/v1",
  "api_key": "<笔记本服务的key，无鉴权可留空>"
}
```

### 步骤 3：qidi 侧——config.toml 只需两行

```toml
[memory.embedding]
model = "<传给笔记本的模型名，任意字符串>"
dimensions = <笔记本模型输出维度，必须一致>
```

无需配 base_url（自动跟默认模型走）。笔记本关机时自动降级 FTS-only，无副作用。

### 注意事项

- 若日后把默认模型从「指向中转的条目」改成直连云端（如 agnes），embedding 会跟着漂走
  （agnes 无 /embeddings → 静默回退 FTS）。**保持默认模型走中转**即可。
- 换新版本模型、维度变化时：改 dimensions 即可，qidi 自动重建向量表。
- 中转路由建议原样透传请求体（qidi 会带 dimensions 参数和批量 input 数组）。

## 四、探针工具存档

`_archive/2026-08-27/_tools/`：probe_embed_server.py（伪造服务器）、probe_restore_url.py、
probe_switch_default.py、binary_context*.py（二进制分析）、qidi_debug*.log（证据日志）、
probe_server.log（全部抓到的请求）。
