# 笔记本 bge-m3 服务交接卡（2026-08-27）

**台式机侧一切就绪，只差笔记本上把服务跑起来。**

## 现状

| 项 | 状态 |
|---|---|
| 台式机链路 | ✅ 中转 8765 `/v1/embeddings` 转发路由已上线 |
| 笔记本 IP | `192.168.1.88`（在线，今日仍在使用中转） |
| 笔记本 8800 端口 | ❌ 未开放 —— bge-m3 服务未启动 |
| 台式机当前 embedding | 白泽 S5 @ 127.0.0.1:8800（检索质量已实测不足，待切换） |

## 笔记本上要做的三步（一次性）

1. **拷文件**：把 `bge_embed_server.py`（本目录）拷到笔记本任意目录。
2. **装依赖 + 启动**：
   ```
   pip install fastapi uvicorn sentence-transformers
   python bge_embed_server.py
   ```
   模型默认从 HF 缓存找 bge-m3；若在别的目录，改文件顶部的 `MODEL_PATH`。
3. **防火墙放行 8800 入站**：
   ```
   netsh advfirewall firewall add rule name="bge-embed" dir=in action=allow protocol=TCP localport=8800
   ```

## 台式机侧我来做（笔记本 8800 通了之后）

```
中转 config.json: embedding_upstream.base_url
  http://127.0.0.1:8800/v1  →  http://192.168.1.88:8800/v1
```
一行改动 + 重启中转，下次 qidi 会话启动自动全量重索引（619 块，bge-m3 1024 维，
维度不变 chunks_vec 无需重建）。然后跑 `bench_retrieval.py` 对比 S5 基准。

## 兜底

笔记本关机/休眠时，qidi 自动回退 FTS-only 检索（已验证的降级行为），
正常对话不受影响 —— 与当初设计一致。

服务模板兼容白泽同款规格：/health 探活、维度强校验 400、批量上限 256、
推理锁串行、日志 10MB×5 轮转、fail-early 加载。
