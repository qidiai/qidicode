# -*- coding: utf-8 -*-
"""白泽 S5 Embedding 服务 —— OpenAI 兼容 /v1/embeddings（qidi 记忆向量检索专用）

架构位置（详见 .qidi/docs/embedding-routing.md）：
    qidi --(默认模型 glm-5.3)--> QiDi Relay :8765 --(/v1/embeddings 转发)--> 本服务 :8800

依赖：同目录的白泽引擎 6 文件（baize_vector.py + 权重），Python 3.10+ / torch。
输出：1024 维 L2 归一化向量（模型自带归一化，与 qidi dimensions=1024 零适配）。

接口：
    GET  /health                -> {"ok":true,"model":...,"dim":1024,...}
    POST /v1/embeddings         -> OpenAI 标准格式
         请求体 {"model":"baize-s5","input":["文本",...],"dimensions":1024}
         （dimensions 仅查询请求携带；非 1024 返回 400 暴露配置错误）

启动：python server.py（默认 127.0.0.1:8800；本服务仅本机中转访问，不对外）
重启：powershell -File restart_baize.ps1（WMI 拉起，脱离终端作业对象）
"""
import json
import logging
import os
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from logging.handlers import RotatingFileHandler

HERE = os.path.dirname(os.path.abspath(__file__))
PORT = int(os.environ.get("BAIZE_EMBED_PORT", "8800"))
HOST = "127.0.0.1"
DIMS = 1024
MAX_BATCH = 256          # 单请求最大条数（qidi 实测批量远小于此）
SERVER_VERSION = "1.0"

# ── 日志：轮转 10MB × 5（与 websearch-shim v2.2 同规格）──
logger = logging.getLogger("baize-embed")
logger.setLevel(logging.INFO)
_h = RotatingFileHandler(os.path.join(HERE, "baize_embed.log"),
                         maxBytes=10 * 1024 * 1024, backupCount=5, encoding="utf-8")
_h.setFormatter(logging.Formatter("%(asctime)s [%(levelname)s] %(message)s"))
logger.addHandler(_h)
logger.addHandler(logging.StreamHandler())

# ── 引擎（启动即加载，失败早暴露）──
ENGINE = None
ENGINE_ERR = None
_infer_lock = threading.Lock()   # 推理串行化：负载轻（CPU ~200条/s），简单可靠优先


def load_engine():
    global ENGINE, ENGINE_ERR
    try:
        from baize_vector import get_engine
        t0 = time.time()
        eng = get_engine()
        eng.embed("预热")          # 触发惰性加载 + 首次推理
        ENGINE = eng
        logger.info("engine loaded: %s (%.1fs)", eng.info().get("model"), time.time() - t0)
    except Exception as e:
        ENGINE_ERR = str(e)
        logger.exception("engine load FAILED: %s", e)


class H(BaseHTTPRequestHandler):
    server_version = "baize-embed/" + SERVER_VERSION

    def do_GET(self):
        if self.path.rstrip("/").endswith("/health"):
            if ENGINE is not None:
                info = ENGINE.info()
                self._send(200, {"ok": True, "version": SERVER_VERSION,
                                 "dim": info.get("dim"), "engine": info})
            else:
                self._send(503, {"ok": False, "error": ENGINE_ERR or "engine not loaded"})
        else:
            self._send(404, {"error": "not found"})

    def do_POST(self):
        n = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(n) if n else b""
        p = self.path.rstrip("/")
        if not (p.endswith("/embeddings")):
            self._send(404, {"error": "not found"})
            return
        if ENGINE is None:
            self._send(503, {"error": "engine not loaded: " + (ENGINE_ERR or "?")})
            return
        try:
            req = json.loads(raw) if raw else {}
        except Exception:
            self._send(400, {"error": {"message": "bad json", "type": "invalid_request_error"}})
            return
        inputs = req.get("input")
        if inputs is None or inputs == "" or inputs == []:
            self._send(400, {"error": {"message": "missing input", "type": "invalid_request_error"}})
            return
        texts = [inputs] if isinstance(inputs, str) else list(inputs)
        if len(texts) > MAX_BATCH:
            self._send(400, {"error": {"message": f"batch too large: {len(texts)} > {MAX_BATCH}",
                                       "type": "invalid_request_error"}})
            return
        dims = req.get("dimensions")
        if dims is not None and int(dims) != DIMS:
            # qidi 配置 dimensions 与模型不符：响亮报错，避免静默错配
            self._send(400, {"error": {
                "message": f"dimensions={dims} but baize-s5 outputs {DIMS}; "
                           f"fix [memory.embedding] dimensions in config.toml",
                "type": "invalid_request_error"}})
            return
        t0 = time.time()
        try:
            with _infer_lock:
                vecs = ENGINE.embed_batch(texts)
        except Exception as e:
            logger.exception("embed failed: %s", e)
            self._send(500, {"error": {"message": f"engine error: {e}", "type": "api_error"}})
            return
        elapsed = int((time.time() - t0) * 1000)
        logger.info("POST /v1/embeddings model=%s n=%d %dms",
                    req.get("model", "?"), len(texts), elapsed)
        data = [{"object": "embedding", "embedding": v, "index": i}
                for i, v in enumerate(vecs)]
        chars = sum(len(t) for t in texts)
        self._send(200, {
            "object": "list", "data": data,
            "model": req.get("model") or "baize-s5",
            "usage": {"prompt_tokens": chars // 4 + 1, "total_tokens": chars // 4 + 1},
        })

    def _send(self, code, obj):
        body = json.dumps(obj).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *a):   # 静默默认 stderr 访问日志（自管轮转日志）
        pass


if __name__ == "__main__":
    logger.info("=== baize-embed v%s starting on %s:%d ===", SERVER_VERSION, HOST, PORT)
    load_engine()
    ThreadingHTTPServer((HOST, PORT), H).serve_forever()
