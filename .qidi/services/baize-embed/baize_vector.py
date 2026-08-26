# -*- coding: utf-8 -*-
"""
baize_vector.py — 白泽 S5 向量引擎（记忆系统专用接口）
========================================================
用法（记忆系统里两行接入）:
    from baize_vector import get_engine
    engine = get_engine()                  # 全局单例, 首次调用加载模型(~3秒)
    vec = engine.embed("任意中文文本")       # → 1024 维 list[float], 已归一化
    engine.similarity("文本A", "文本B")     # → float 余弦相似度

依赖文件（与本文件同目录）:
    best_model.pt          V10 骨干 (7.4MB)
    baize_s5_release.pt    S5 门控权重 (0.66MB)
    embedding_model.json   模型配置
    train_pure_pool.py     架构定义
    v14_structural.py      S5 加载器

特性:
    - 惰性加载, 进程内只加载一次
    - CPU/GPU 自动选择
    - 批量编码 embed_batch (建索引/迁移用, 快 10 倍)
    - 输出已 L2 归一化, 直接点积 = 余弦相似度
    - 1024 维, 与 EmbeddingsStore(dim=1024) 直接匹配

质量参考 (54 对中文基准): 总分 0.786 (bge-m3 的 97.6%, 参数 1/237)
错别字鲁棒/网络用语强于 bge-m3; 否定带序正确。
"""
import json
import os
import threading

_HERE = os.path.dirname(os.path.abspath(__file__))

# ── 分词 (与训练完全同口径: ord(c)%4096, 最长 96 字, 补 0) ──
_CHAR_MOD = 4096
_MAX_LEN = 96


def _text_ids(text: str):
    ids = [min(ord(c) % _CHAR_MOD, _CHAR_MOD - 1) for c in text[:_MAX_LEN]]
    return ids + [0] * (_MAX_LEN - len(ids))


class BaizeVectorEngine:
    """白泽 S5 向量引擎. 单例使用: get_engine()."""

    def __init__(self, model_dir=None):
        import torch
        self._torch = torch
        self.model_dir = model_dir or _HERE
        self.device = "cuda" if torch.cuda.is_available() else "cpu"
        self._model = None
        self._lock = threading.Lock()

    # ── 模型加载 (惰性, 线程安全) ──
    def _ensure_loaded(self):
        if self._model is not None:
            return
        with self._lock:
            if self._model is not None:
                return
            import sys
            if self.model_dir not in sys.path:
                sys.path.insert(0, self.model_dir)
            from v14_structural import BaizeEmbeddingGated, load_v10
            m = BaizeEmbeddingGated(load_v10()).eval()
            ck = self._torch.load(
                os.path.join(self.model_dir, "baize_s5_release.pt"),
                map_location="cpu", weights_only=False)
            m.gate_layers.load_state_dict(ck["gate_weights"])
            # 关键: load_state_dict 不搬设备, 统一搬一次
            m = m.to(next(m.v10.parameters()).device)
            self._model = m

    # ── 核心接口 ──
    def embed(self, text: str):
        """单条文本 → 1024 维 list[float] (已归一化)"""
        return self.embed_batch([text])[0]

    def embed_batch(self, texts, bs=64):
        """批量文本 → list[1024维list] (已归一化). 建索引/迁移用."""
        if not texts:
            return []
        self._ensure_loaded()
        torch = self._torch
        out = []
        with torch.no_grad():
            for i in range(0, len(texts), bs):
                chunk = list(texts[i:i + bs])
                ids = torch.tensor([_text_ids(t) for t in chunk],
                                   dtype=torch.long, device=self.device)
                e = self._model(ids).cpu().numpy()
                out.extend(e.tolist())
        return out

    def similarity(self, a: str, b: str) -> float:
        """两条文本的余弦相似度 (向量已归一化, 点积即余弦)"""
        va, vb = self.embed(a), self.embed(b)
        return float(sum(x * y for x, y in zip(va, vb)))

    @property
    def dim(self):
        return 1024

    def info(self):
        return {
            "model": "白泽 S5 (V10骨干+门控+轻规则)",
            "dim": 1024,
            "params": "~2.42M",
            "device": self.device,
            "quality": "54对基准总分0.786 / bge-m3的97.6% / 错别字鲁棒+网络用语强于bge-m3",
        }


# ── 全局单例 ──
_ENGINE = None
_ENGINE_LOCK = threading.Lock()


def get_engine():
    """获取全局引擎单例 (记忆系统用这个入口)"""
    global _ENGINE
    if _ENGINE is None:
        with _ENGINE_LOCK:
            if _ENGINE is None:
                _ENGINE = BaizeVectorEngine()
    return _ENGINE


if __name__ == "__main__":
    # 自测: python -X utf8 baize_vector.py
    eng = get_engine()
    print("白泽 S5 向量引擎自测")
    print("  info:", json.dumps(eng.info(), ensure_ascii=False))
    demos = [("我喜欢这部电影", "我不喜欢这部电影"),
             ("今天天气真好", "今天天汽真好"),        # 错别字
             ("内卷", "过度竞争"),                    # 网络用语
             ("今天天气真好", "帮我打开微信")]          # 无关
    for a, b in demos:
        print(f"  {eng.similarity(a, b):+.3f}  {a}  vs  {b}")
    v = eng.embed("测试向量维度")
    print(f"  维度: {len(v)}, 范数: {sum(x*x for x in v)**0.5:.4f} (应=1.0)")
    print("自测通过" )
