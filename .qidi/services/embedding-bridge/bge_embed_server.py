# -*- coding: utf-8 -*-
"""
bge-m3 embedding 服务（笔记本侧，OpenAI 兼容）
=================================================
依赖（笔记本上装一次）:
    pip install fastapi uvicorn sentence-transformers

模型: 笔记本上已有的 bge-m3（1024 维）。
若模型不在默认 HF 缓存里，改 MODEL_PATH 指向本地目录。

启动:  python bge_embed_server.py
端口:  0.0.0.0:8800（需防火墙放行 8800 入站）

自测:  curl http://127.0.0.1:8800/health
"""
import logging
from logging.handlers import RotatingFileHandler
from threading import Lock
import time

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field
from typing import List, Optional
import uvicorn

# ── 模型路径：笔记本上 bge-m3 所在目录，None = HF 缓存自动找 ──
MODEL_PATH = None
DIM = 1024

# ── 日志（与台式机 baize-embed 同规格：10MB × 5 轮转）──
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(message)s",
    handlers=[RotatingFileHandler("bge_embed.log", maxBytes=10 * 1024 * 1024,
                                  backupCount=5, encoding="utf-8")],
    force=True,
)
log = logging.getLogger("bge-embed")

app = FastAPI()
_engine = None
_infer_lock = Lock()

class EmbRequest(BaseModel):
    model: str
    input: List[str]
    dimensions: Optional[int] = None

def _load():
    global _engine
    from sentence_transformers import SentenceTransformer
    log.info("loading bge-m3 from %s", MODEL_PATH or "HF cache")
    t0 = time.time()
    m = SentenceTransformer(MODEL_PATH or "BAAI/bge-m3")
    log.info("model loaded in %.1fs, dim=%d", time.time() - t0, m.get_sentence_embedding_dimension())
    return m

@app.on_event("startup")
def startup():
    global _engine
    try:
        _engine = _load()
    except Exception:
        log.exception("engine load FAILED")
        raise

@app.get("/health")
def health():
    if _engine is None:
        return {"ok": False, "error": "engine not loaded"}
    return {"ok": True, "dim": DIM,
            "engine": {"model": "bge-m3", "dim": DIM}}

@app.post("/v1/embeddings")
def embeddings(req: EmbRequest):
    if _engine is None:
        raise HTTPException(503, "engine not loaded")
    if len(req.input) == 0:
        raise HTTPException(400, "empty input")
    if len(req.input) > 256:
        raise HTTPException(400, f"batch too large: {len(req.input)} > 256")
    if req.dimensions not in (None, DIM):
        raise HTTPException(400, f"dimensions must be {DIM}, got {req.dimensions}")
    t0 = time.time()
    with _infer_lock:
        vecs = _engine.encode(req.input, normalize_embeddings=True)
    dt = (time.time() - t0) * 1000
    log.info("POST /v1/embeddings model=%s n=%d %.0fms", req.model, len(req.input), dt)
    return {
        "object": "list",
        "model": req.model,
        "data": [{"object": "embedding", "index": i,
                  "embedding": [round(float(x), 8) for x in v]}
                 for i, v in enumerate(vecs)],
        "usage": {"prompt_tokens": sum(len(t) for t in req.input) // 4 + 1,
                  "total_tokens": sum(len(t) for t in req.input) // 4 + 1},
    }

if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=8800, log_level="warning")
