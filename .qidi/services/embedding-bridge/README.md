# Embedding Bridge —— 自研向量模型接入部署指南

> **状态（2026-08-27）：已在本机（台式机）落地** —— 白泽 S5 部署在
> `127.0.0.1:8800`（`.qidi/services/baize-embed/`），中转转发路由与
> config.toml 均已配置，E2E 验证通过（24/24 次 embedding 成功、32 文件重索引零失败）。
> 本文档保留下方「笔记本部署」流程：将来要迁到笔记本时，只需按步骤 1 在笔记本起服务，
> 然后把中转 config.json 的 `embedding_upstream.base_url` 改成笔记本地址即可，其余不动。

目标：让 qidi 的跨会话记忆向量检索（hybrid: FTS5 + sqlite-vec）用上自研 embedding 模型。

原理与实测结论见 `.qidi/docs/embedding-routing.md`。
**核心：embedding 请求跟着「默认模型」的 base_url 走 → 默认模型指向中转(8765) →
在中转加一条 /v1/embeddings 转发路由即可，qidi 侧零路由配置。**

## 架构

```
台式机 qidi ──(默认模型 glm-5.3)──> QiDi Relay :8765
                                      ├─ /v1/chat/completions  → 上游聊天模型（现状不动）
                                      └─ /v1/embeddings        → 笔记本 :8800（新增转发）
```

---

## 步骤 1 · 笔记本：跑 OpenAI 兼容 embedding 服务

1. 把本目录 `laptop_embed_server.py` 拷到笔记本。
2. 填两处 TODO：`load_model()`（加载自研模型）和 `embed_texts()`（批量推理），
   并把 `DIMENSIONS` 改成模型真实输出维度。
3. 启动：`python laptop_embed_server.py`（默认 0.0.0.0:8800）。
4. 自测：
   ```bash
   curl http://127.0.0.1:8800/v1/embeddings -H "Content-Type: application/json" \
        -d '{"model":"my-embed","input":["测试文本"]}'
   ```
   返回 `"data":[{"embedding":[...]}]` 即成功。
5. 防火墙放行 8800 端口（入站规则），记下笔记本局域网 IP（如 192.168.x.x）。

## 步骤 2 · 台式机中转：加 /v1/embeddings 转发路由

**文件：`G:\程序开发\本地局域网中转-v3\routes\proxy.py`**（在文件末尾追加）：

```python
# ===================== /v1/embeddings 转发（自研向量模型，2026-08-27） =====================
EMBED_CFG = load_config().get("embedding_upstream") or {}

@bp.route("/v1/embeddings", methods=["POST"])
@require_api_key
def v1_embeddings():
    """转发到笔记本自研 embedding 服务（qidi 记忆向量检索专用，笔记本离线时 qidi 自动降级 FTS）。"""
    if not EMBED_CFG.get("base_url"):
        return jsonify({"error": {"message": "embedding upstream not configured",
                                  "type": "invalid_request_error"}}), 404
    data = request.get_json(force=True, silent=True) or {}
    upstream = EMBED_CFG["base_url"].rstrip("/") + "/embeddings"
    headers = {"Content-Type": "application/json"}
    if EMBED_CFG.get("api_key"):
        headers["Authorization"] = "Bearer " + EMBED_CFG["api_key"]
    try:
        r = httpx.post(upstream, json=data, headers=headers,
                       timeout=EMBED_CFG.get("timeout", 60))
        return Response(r.content, status=r.status_code,
                        content_type="application/json")
    except Exception as e:
        log.warning("embedding upstream failed: %s", e)
        return jsonify({"error": {"message": f"embedding upstream failed: {e}",
                                  "type": "api_error"}}), 502
```

> 注意：`EMBED_CFG` 若想改 config.json 后免重启，把第一行挪进函数体调 `load_config()` 即可。

**文件：`G:\程序开发\本地局域网中转-v3\config.json`**（顶层加一段）：

```json
"embedding_upstream": {
    "base_url": "http://192.168.x.x:8800/v1",
    "api_key": "",
    "timeout": 60
}
```

重启中转（`启动代理-v3.bat` 或 ensure_services.ps1 自愈拉起），然后自测：

```bash
curl http://127.0.0.1:8765/v1/embeddings -H "Content-Type: application/json" \
     -d '{"model":"my-embed","input":["测试文本"]}'
```

## 步骤 3 · 台式机 qidi：config.toml 开启向量检索

`.qidi/config.toml` 末尾追加（**无需 base_url，自动跟默认模型走中转**）：

```toml
[memory.embedding]
model = "my-embed"      # 任意字符串，会原样透传到笔记本
dimensions = 1024       # 必须与笔记本模型输出维度一致
```

验证：跑一次 qidi（任意会话），debug 日志里应出现：
- `MEMORY_REINDEX: background reindex complete` 且**无** `embedding batch failed`
- memory_search 调用时**无** `falling back to FTS-only`
- `.qidi/memory/<工作区>/index.sqlite` 明显增长（向量写入）

中转侧 `proxy.log` / 笔记本服务日志能看到成批 `/v1/embeddings` 请求。

## 运维特性

| 场景 | 行为 |
|---|---|
| 笔记本关机/休眠 | qidi 每次会话 WARN 跳过向量写入，检索自动 FTS-only，**不影响正常使用** |
| 换新版本模型、维度变了 | 改 config.toml 的 dimensions → qidi 自动重建 chunks_vec（"embedding dimension mismatch, recreating"） |
| 默认模型换成非中转条目 | embedding 会跟着漂走（云端无 /embeddings → 静默 FTS）。**默认模型保持走中转** |
| 首次全量索引 | 600+ 记忆块会话启动时后台批量向量化，笔记本第一次会忙一阵 |

## 相关文件

- `laptop_embed_server.py` —— 笔记本侧服务模板（本目录）
- `.qidi/docs/embedding-routing.md` —— 路由机制破解报告（实测证据）
- `_archive/2026-08-27/_tools/` —— 探针工具与证据日志存档
