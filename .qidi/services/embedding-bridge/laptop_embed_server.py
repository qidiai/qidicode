# -*- coding: utf-8 -*-
"""自研 Embedding 模型 · OpenAI 兼容服务模板（部署在笔记本上）

暴露 POST /v1/embeddings，供 QiDi Relay(8765) 转发调用。
qidi 发来的请求形如：
  {"model": "<config里 [memory.embedding].model>", "input": ["文本1", "文本2", ...], "dimensions": N}

启动（笔记本上）：
  pip install fastapi uvicorn
  python laptop_embed_server.py            # 默认 0.0.0.0:8800
  # 或: uvicorn laptop_embed_server:app --host 0.0.0.0 --port 8800

需要改的地方只有 load_model() 和 embed_texts() 两处（搜 TODO）。
"""
import os
import time
from typing import List, Optional

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel

# ============================ 配置区 ============================
PORT = int(os.environ.get("EMBED_PORT", "8800"))
MODEL_NAME = os.environ.get("EMBED_MODEL_NAME", "my-embed")   # 对外显示的模型名
DIMENSIONS = int(os.environ.get("EMBED_DIM", "1024"))          # TODO: 改成自研模型实际输出维度
NORMALIZE = True          # L2 归一化（余弦检索推荐开）
MAX_BATCH = 64            # 单次请求最大条数，超过报 400
# ================================================================

app = FastAPI(title="Laptop Embedding Server", version="1.0")
_model = None


def load_model():
    """TODO: 加载自研模型。按实际框架三选一（示例给全，留有用的删其余）。"""
    global _model
    if _model is not None:
        return _model

    # ---- 方案 A: PyTorch / transformers（CPU 或 CUDA）----
    # import torch
    # from transformers import AutoModel, AutoTokenizer
    # tok = AutoTokenizer.from_pretrained(MODEL_PATH)
    # net = AutoModel.from_pretrained(MODEL_PATH).eval()
    # _model = (tok, net)

    # ---- 方案 B: 昇腾 NPU（torch_npu，笔记本 Ascend 910）----
    # import torch
    # import torch_npu  # 导入即注册 NPU 后端
    # from transformers import AutoModel, AutoTokenizer
    # tok = AutoTokenizer.from_pretrained(MODEL_PATH)
    # net = AutoModel.from_pretrained(MODEL_PATH).eval().to("npu")
    # _model = (tok, net)

    # ---- 方案 C: 已有推理引擎/自定义加载 ----
    # _model = my_engine.load(MODEL_PATH)

    raise RuntimeError("load_model() 尚未实现：请填入自研模型的加载代码")


def embed_texts(texts: List[str]) -> List[List[float]]:
    """TODO: 批量推理，返回与 texts 等长的向量列表（每条长度 = DIMENSIONS）。"""
    tok, net = load_model()   # 按实际模型对象调整
    raise RuntimeError("embed_texts() 尚未实现：请填入推理代码")
    # transformers 常规写法示例：
    # import torch
    # with torch.no_grad():
    #     batch = tok(texts, padding=True, truncation=True, max_length=512, return_tensors="pt")
    #     out = net(**batch).last_hidden_state[:, 0]      # CLS pooling，按模型改
    #     vecs = out.float().cpu().tolist()
    # return vecs


def l2_normalize(vec: List[float]) -> List[float]:
    n = sum(x * x for x in vec) ** 0.5
    return [x / n for x in vec] if n > 0 else vec


class EmbeddingRequest(BaseModel):
    model: Optional[str] = None
    input: object = None          # str 或 List[str]（qidi 会发批量数组）
    dimensions: Optional[int] = None
    encoding_format: Optional[str] = None


@app.get("/health")
def health():
    return {"ok": True, "model": MODEL_NAME, "dimensions": DIMENSIONS}


@app.post("/v1/embeddings")
def embeddings(req: EmbeddingRequest):
    if req.input is None:
        raise HTTPException(400, "missing input")
    texts = [req.input] if isinstance(req.input, str) else list(req.input)
    if not texts:
        raise HTTPException(400, "empty input")
    if len(texts) > MAX_BATCH:
        raise HTTPException(400, f"batch too large: {len(texts)} > {MAX_BATCH}")

    t0 = time.time()
    vecs = embed_texts(texts)
    if len(vecs) != len(texts):
        raise HTTPException(500, "model returned mismatched count")
    if NORMALIZE:
        vecs = [l2_normalize(v) for v in vecs]

    data = [
        {"object": "embedding", "embedding": v, "index": i}
        for i, v in enumerate(vecs)
    ]
    return {
        "object": "list",
        "data": data,
        "model": req.model or MODEL_NAME,
        "usage": {
            "prompt_tokens": sum(len(t) for t in texts) // 4 + 1,
            "total_tokens": sum(len(t) for t in texts) // 4 + 1,
        },
        "_server_meta": {"dims": DIMENSIONS, "elapsed_ms": int((time.time() - t0) * 1000)},
    }


if __name__ == "__main__":
    import uvicorn
    load_model()  # 启动即加载，失败早暴露
    uvicorn.run(app, host="0.0.0.0", port=PORT)
