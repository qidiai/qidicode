# baize-embed —— 白泽 S5 自研向量模型服务

qidi 记忆向量检索（hybrid: FTS5 + sqlite-vec）的 embedding 后端。
OpenAI 兼容 `/v1/embeddings`，监听 `127.0.0.1:8800`，1024 维 L2 归一化输出。

- 引擎：`baize_vector.py`（白泽 S5，~2.42M 参数，CPU 推理 ~200 条/s）
- 服务：`server.py`（纯 `http.server`，无第三方 web 框架依赖；引擎启动即加载，
  失败 fail-early；推理串行化 `_infer_lock`；日志 `baize_embed.log` 10MB×5 轮转）
- 重启：`restart_baize.ps1`（杀 8800 占用 → WMI 拉起 → 等健康检查通过）
- 自愈：`ops/ensure_services.ps1` 已纳入，随 启动qidi.bat 每次拉起

## ⚠️ 权重文件不入 git

`best_model.pt`、`baize_s5_release.pt`、`embedding_model.json`（合计 ~47MB）
已被 .gitignore 排除。clone 后需从成果包拷入本目录：

```
源：G:\实验测试\模型研究\白泽成果包_20260825\
拷：best_model.pt、baize_s5_release.pt、embedding_model.json → 本目录
```

拷完自测：`Invoke-RestMethod http://127.0.0.1:8800/health` 应返回 `"ok":true`。

`train_pure_pool.py`、`v14_structural.py` 为成果包内的训练/结构参考代码，随库保留。

## 路由链

```
qidi（默认模型 glm-5.3 → 8765 中转）
  └─ POST /v1/embeddings {"model":"baize-s5","input":[...],"dimensions":1024}
       └─ 中转 embedding_upstream 转发 → 本服务 :8800
```

机制详解：`.qidi/docs/embedding-routing.md`；运维手册：`.qidi/ops-README.md`。
