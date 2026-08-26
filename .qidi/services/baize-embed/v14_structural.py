# -*- coding: utf-8 -*-
"""
v14_structural.py — V14 结构修复: 上下文调制写入 (Delta-Write Pool)
====================================================================
根源诊断结论 (root_cause.py, 2026-08-24):
  V10 是字符袋模型, cos 与字符重叠率 Spearman=+0.810;
  加字(不/的/吧)cos≈0.96 不变方向, 换字才变 — 否定恰是"加字"操作, 故天生色盲.

结构修复: 写入不再独立, 依赖当前池状态 —
  V10: write_t = tanh(W·x_t)                     [各字独立加法]
  V14: write_t = tanh(W·x_t + U·pool_{t-1})      [看语境再写入]
  U 零初始化 ⇒ 严格恒等 V10 (5e-7 级).

训练 (审计后校准配方): 蒸馏 + 轻规则 (目标 cos=+0.80, 即同义带下方一点, 对齐 bge-m3 分离度 +0.05)

实验组:
  S0 V10 基线
  S1 V14 恒等验证 (U=0)
  S2 V14 + 纯蒸馏          (结构无信号能否自发学否定? 预期不能)
  S3 V14 + 蒸馏+轻规则     (主实验: 结构修复+校准目标)
  S5 V11门控 + 蒸馏+轻规则 (对照: 无新结构, 同训练 — 归因结构贡献)
判据 (审计后口径):
  主: 分离度 = 同义均值 − 否定均值, 目标带 [+0.03, +0.15] (bge-m3=+0.050)
  官方压制率 = P(否定cos < 同义均值) (bge-m3=91.7%)
  守门: 总分 ≥0.65, 同义/反义/跨域不明显塌
  参考: 否定ρ (噪声指标, 仅参考); 训练后字袋相关性 (Spearman(cos,字符重叠) 是否从 0.810 下降)
"""
import argparse
import importlib.util as ilu
import json
import math
import os
import random
import time

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F

HERE = os.path.dirname(os.path.abspath(__file__))
V10_DIR = HERE
TEACHER_DIR = r"F:\代码\白泽档案\03-训练数据\teacher_v10"
OUT_DIR = os.path.join(HERE, "v14_output")

SEED = 42
CHAR_MOD = 4096
MAX_LEN = 96

torch.manual_seed(SEED)
random.seed(SEED)
np.random.seed(SEED)
DEVICE = "cuda" if torch.cuda.is_available() else "cpu"

import sys
sys.path.insert(0, V10_DIR)
from train_pure_pool import BaizeEmbeddingV10, Config  # noqa: E402

NEG_ID = ord("不") % CHAR_MOD

# ════════════════════════════════════════════════════════
# V14: Delta-Write 池 (上下文调制写入)
# ════════════════════════════════════════════════════════
class DeltaWrite(nn.Module):
    """U: sp_dim→sp_dim, 零初始化. write_t = tanh(W·x_t + U·pool_{t-1})."""

    def __init__(self, sp_dim):
        super().__init__()
        self.U = nn.Linear(sp_dim, sp_dim, bias=False)
        nn.init.zeros_(self.U.weight)


def layer_forward_delta(layer, dws, x):
    """V10 层前向的 Delta-Write 版. U=0 时与 V10 闭式解严格一致."""
    write_proj = layer.write_gate(x)              # [bs, seq, sp]
    bs, seq, sp = write_proj.shape
    decays = (layer.fast_decay, layer.medium_decay, layer.slow_decay)
    pools = [torch.zeros(bs, sp, device=x.device) for _ in range(3)]
    for t in range(seq):
        for i in range(3):
            w = torch.tanh(write_proj[:, t] + dws[i].U(pools[i]))
            pools[i] = decays[i] * pools[i] + (1 - decays[i]) * w
    fast_r = layer.fast_read(pools[0])
    med_r = layer.medium_read(pools[1])
    slow_r = layer.slow_read(pools[2])
    concat = torch.cat([fast_r, med_r, slow_r], dim=-1)
    weights = F.softmax(layer.fusion_gate(concat), dim=-1)
    return (weights[:, 0:1] * fast_r + weights[:, 1:2] * med_r + weights[:, 2:3] * slow_r)


class BaizeEmbeddingV14(nn.Module):
    """V10 骨干 + 每层每速一个 Delta-Write U 矩阵 (6×3×24×24 = 10.4K 新参数)."""

    def __init__(self, v10):
        super().__init__()
        self.v10 = v10
        cfg = v10.config
        self.dw_layers = nn.ModuleList()
        for _ in range(cfg.num_layers):
            self.dw_layers.append(nn.ModuleList([
                DeltaWrite(cfg.sp_dim) for _ in range(3)]))

    def forward(self, tokens):
        cfg = self.v10.config
        x = self.v10.embedding(tokens)
        residual = x.mean(dim=1)
        current = layer_forward_delta(self.v10.pool_stack.layers[0], self.dw_layers[0], x)
        for i in range(1, cfg.num_layers):
            skip = residual * torch.sigmoid(self.v10.pool_stack.skip_gates[i])
            pseudo_seq = x + current.unsqueeze(1)
            current = layer_forward_delta(self.v10.pool_stack.layers[i], self.dw_layers[i], pseudo_seq) + skip
        compressed = torch.tanh(self.v10.bottleneck(current))
        out = self.v10.output_proj(compressed)
        out = self.v10.output_norm(out)
        return F.normalize(out, p=2, dim=-1)


# ════════════════════════════════════════════════════════
# V11 门控 (S5 对照组用)
# ════════════════════════════════════════════════════════
class GatedPoolLayer(nn.Module):
    def __init__(self, embed_dim, sp_dim, base):
        super().__init__()
        self.gate = nn.Linear(embed_dim, sp_dim, bias=True)
        nn.init.zeros_(self.gate.weight)
        nn.init.constant_(self.gate.bias, math.log(base / (1.0 - base)))

    def lambda_features(self, inp):
        return torch.sigmoid(self.gate(inp))


def layer_forward_gated(layer, gates, x):
    write_vals = torch.tanh(layer.write_gate(x))
    bs, seq, sp = write_vals.shape
    lams = [g.lambda_features(x) for g in gates]
    pools = [torch.zeros(bs, sp, device=x.device) for _ in range(3)]
    for t in range(seq):
        for i in range(3):
            pools[i] = lams[i][:, t] * pools[i] + (1 - lams[i][:, t]) * write_vals[:, t]
    fast_r = layer.fast_read(pools[0])
    med_r = layer.medium_read(pools[1])
    slow_r = layer.slow_read(pools[2])
    concat = torch.cat([fast_r, med_r, slow_r], dim=-1)
    weights = F.softmax(layer.fusion_gate(concat), dim=-1)
    return (weights[:, 0:1] * fast_r + weights[:, 1:2] * med_r + weights[:, 2:3] * slow_r)


class BaizeEmbeddingGated(nn.Module):
    def __init__(self, v10):
        super().__init__()
        self.v10 = v10
        cfg = v10.config
        self.gate_layers = nn.ModuleList()
        for _ in range(cfg.num_layers):
            self.gate_layers.append(nn.ModuleList([
                GatedPoolLayer(cfg.embed_dim, cfg.sp_dim, cfg.fast_decay),
                GatedPoolLayer(cfg.embed_dim, cfg.sp_dim, cfg.medium_decay),
                GatedPoolLayer(cfg.embed_dim, cfg.sp_dim, cfg.slow_decay),
            ]))

    def forward(self, tokens):
        cfg = self.v10.config
        x = self.v10.embedding(tokens)
        residual = x.mean(dim=1)
        current = layer_forward_gated(self.v10.pool_stack.layers[0], self.gate_layers[0], x)
        for i in range(1, cfg.num_layers):
            skip = residual * torch.sigmoid(self.v10.pool_stack.skip_gates[i])
            pseudo_seq = x + current.unsqueeze(1)
            current = layer_forward_gated(self.v10.pool_stack.layers[i], self.gate_layers[i], pseudo_seq) + skip
        compressed = torch.tanh(self.v10.bottleneck(current))
        out = self.v10.output_proj(compressed)
        out = self.v10.output_norm(out)
        return F.normalize(out, p=2, dim=-1)


# ════════════════════════════════════════════════════════
# 规则词表 (审计清洗版: 剔除泄漏对 安静/喧闹; 补动词类 6 对)
# ════════════════════════════════════════════════════════
TRAIN_ANT = [
    ("强大", "弱小"), ("干净", "肮脏"), ("简单", "复杂"),
    ("善良", "邪恶"), ("认真", "马虎"), ("清楚", "模糊"),
    ("整齐", "杂乱"), ("新鲜", "腐烂"), ("平坦", "崎岖"),
    ("柔软", "坚硬"), ("明亮", "昏暗"),
    # 动词类 (基准否定对含动词, 补覆盖)
    ("接受", "拒绝"), ("支持", "反对"), ("相信", "怀疑"),
    ("表扬", "批评"), ("奖励", "惩罚"), ("保存", "删除"),
]
TRAIN_SENT = [
    "他的态度很{w}", "这个房间很{w}", "那件衣服很{w}",
    "她的性格很{w}", "这条道路很{w}", "这份材料很{w}",
]
SENT_WORDS = ["强大", "干净", "简单", "认真"]   # 安静→认真 (去泄漏)

RULE_TARGET = 0.80   # 校准目标: 同义带下方一点 (审计: bge-m3 分离度 +0.05)


def text_ids(text: str):
    ids = [min(ord(c) % CHAR_MOD, CHAR_MOD - 1) for c in text[:MAX_LEN]]
    ids += [0] * (MAX_LEN - len(ids))
    return ids


def make_rule_pairs():
    """轻规则: (不X, X, 目标cos=0.80). 不再推反义词(过冲根源之一)."""
    pairs = []
    for w, ant in TRAIN_ANT:
        pairs.append((f"不{w}", w, RULE_TARGET))
    for tmpl in TRAIN_SENT:
        for w in SENT_WORDS:
            a = tmpl.format(w=w)
            b = a.replace(w, f"不{w}", 1)
            pairs.append((a, b, RULE_TARGET))
    return pairs


# ════════════════════════════════════════════════════════
# 评测 (审计后口径)
# ════════════════════════════════════════════════════════
def load_bench_pairs():
    spec = ilu.spec_from_file_location(
        "sts_bench_mod", os.path.join(V10_DIR, "sts_benchmark.py"))
    mod = ilu.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod._RAW_PAIRS


def spearman(x, y):
    def rank(a):
        a = np.asarray(a, dtype=np.float64)
        order = np.argsort(a, kind="mergesort")
        ranks = np.empty(len(a), dtype=np.float64)
        i = 0
        while i < len(a):
            j = i
            while j + 1 < len(a) and a[order[j + 1]] == a[order[j]]:
                j += 1
            avg = (i + j) / 2 + 1
            for k in range(i, j + 1):
                ranks[order[k]] = avg
            i = j + 1
        return ranks
    rx, ry = rank(x), rank(y)
    rx, ry = rx - rx.mean(), ry - ry.mean()
    denom = math.sqrt((rx ** 2).sum() * (ry ** 2).sum())
    if denom < 1e-12:
        return 0.0
    return float((rx * ry).sum() / denom)


def cos_np(a, b):
    return float(np.dot(a, b) / (np.linalg.norm(a) * np.linalg.norm(b) + 1e-12))


CATS = ["synonym", "antonym", "negation", "degree", "cross_domain"]


def encode_texts(model_fn, texts, bs=64):
    embs = []
    for i in range(0, len(texts), bs):
        chunk = texts[i:i + bs]
        ids = torch.tensor([text_ids(t) for t in chunk], dtype=torch.long, device=DEVICE)
        with torch.no_grad():
            e = model_fn(ids).detach().cpu().numpy()
        embs.append(e)
    return np.concatenate(embs, axis=0)


def evaluate_on_bench(model_fn, name, pairs):
    texts = sorted({p[1] for p in pairs} | {p[2] for p in pairs})
    emb = {t: e for t, e in zip(texts, encode_texts(model_fn, texts))}
    per_cat, cos_cat = {}, {}
    for cat in CATS:
        sub = [p for p in pairs if p[0] == cat]
        sims = [cos_np(emb[p[1]], emb[p[2]]) for p in sub]
        labels = [p[3] for p in sub]
        per_cat[cat] = spearman(sims, labels)
        cos_cat[cat] = float(np.mean(sims))
    neg_pairs = [p for p in pairs if p[0] == "negation"]
    # 官方口径: 压制 = 否定cos < 同义均值
    syn_mean = cos_cat["synonym"]
    official_supp = float(np.mean([cos_np(emb[p[1]], emb[p[2]]) < syn_mean for p in neg_pairs]))
    separation = syn_mean - cos_cat["negation"]
    all_sims = [cos_np(emb[p[1]], emb[p[2]]) for p in pairs]
    all_labels = [p[3] for p in pairs]
    return {
        "name": name,
        "spearman_per_cat": per_cat,
        "spearman_total": spearman(all_sims, all_labels),
        "cos_mean_per_cat": cos_cat,
        "official_suppression": official_supp,
        "separation": separation,
    }


def diag_charbag(model_fn, name, log):
    """训练后字袋诊断: Spearman(cos, 字符重叠) 是否从 0.810 下降."""
    tests = [
        ("喜欢", "不喜欢"), ("喜欢", "喜欢的"), ("喜欢", "喜欢吧"),
        ("喜欢", "喜欢呢"), ("喜欢", "讨厌"), ("喜欢", "喜爱"),
        ("今天天气真好", "今天天气不好"), ("今天天气真好", "今天天气真好呀"),
        ("今天天气真好", "今天天气真差"), ("开心", "不开心"),
        ("开心", "开心啊"), ("开心", "悲伤"), ("开心", "快乐"),
    ]
    texts = sorted({a for a, _ in tests} | {b for _, b in tests})
    emb = {t: e for t, e in zip(texts, encode_texts(model_fn, texts))}
    ovs, css = [], []
    for a, b in tests:
        sa, sb = set(a), set(b)
        ovs.append(len(sa & sb) / max(len(sa | sb), 1))
        css.append(cos_np(emb[a], emb[b]))
    r = spearman(ovs, css)
    add_cos = np.mean([c for (a, b), c in zip(tests, css) if b.startswith(a[:1]) and len(b) > len(a)])
    log(f"  [字袋诊断:{name}] Spearman(cos,字符重叠)={r:+.3f} (V10基线 +0.810)")
    log(f"    加字组: " + " ".join(f"{b[:6]}:{c:.3f}" for (a, b), c in zip(tests, css) if len(b) > len(a) or len(a) > len(b)))
    return r


# ════════════════════════════════════════════════════════
# 训练: 只训新增参数 (蒸馏 + 轻规则)
# ════════════════════════════════════════════════════════
def load_distill_rows(n=2000):
    path = os.path.join(TEACHER_DIR, "teacher_vectors_v10_retokenized.jsonl")
    rows = []
    with open(path, "r", encoding="utf-8") as f:
        for i, line in enumerate(f):
            if i >= n:
                break
            try:
                r = json.loads(line)
            except Exception:
                continue
            text = r.get("text", "")
            t = r.get("teacher_embedding")
            if t is None or not text:
                continue
            ids = [min(ord(c) % CHAR_MOD, CHAR_MOD - 1) for c in text[:MAX_LEN]]
            rows.append((ids, np.asarray(t, dtype=np.float16)))
    return rows


def train_new_params(model, new_params, distill_rows, rule_pairs, log,
                     epochs=10, lr=1e-3, bs=32, lam_distill=1.0, lam_rule=0.3, tag="s"):
    model = model.to(DEVICE)
    for p in model.v10.parameters():
        p.requires_grad_(False)
    for p in new_params:
        p.requires_grad_(True)
    opt = torch.optim.AdamW(new_params, lr=lr, weight_decay=1e-5)

    d_items = distill_rows
    pad = lambda ids: ids + [0] * (MAX_LEN - len(ids))
    r_A = torch.tensor([pad(text_ids(a)) for a, _, _ in rule_pairs], device=DEVICE)
    r_B = torch.tensor([pad(text_ids(b)) for _, b, _ in rule_pairs], device=DEVICE)
    r_T = torch.tensor([t for _, _, t in rule_pairs], dtype=torch.float32, device=DEVICE)
    log(f"  蒸馏 {len(d_items)} 行 | 规则 {len(rule_pairs)} 对(目标{RULE_TARGET}) | "
        f"λd={lam_distill} λr={lam_rule} | 新参数 {sum(p.numel() for p in new_params)/1e3:.1f}K")

    def rule_loss():
        ca = model(r_A)
        cb = model(r_B)
        return (((ca * cb).sum(dim=-1) - r_T) ** 2).mean()

    for ep in range(epochs):
        model.train()
        idx = list(range(len(d_items)))
        random.Random(SEED + ep).shuffle(idx)
        tot, nb, r_sum = 0.0, 0, 0.0
        t0 = time.time()
        for i in range(0, len(idx), bs):
            opt.zero_grad()
            loss = torch.tensor(0.0, device=DEVICE)
            batch = [d_items[j] for j in idx[i:i + bs]]
            if batch and lam_distill > 0:
                ids = torch.tensor([pad(a) for a, _ in batch], device=DEVICE)
                tgt_v = torch.tensor(np.stack([t for _, t in batch]).astype(np.float32), device=DEVICE)
                out = model(ids)
                tn = F.normalize(tgt_v, p=2, dim=-1)
                loss = loss + lam_distill * (1.0 - (out * tn).sum(-1).mean())
            if lam_rule > 0:
                rl = rule_loss()
                loss = loss + lam_rule * rl
                r_sum += rl.item()
            if loss.requires_grad:
                loss.backward()
                torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
                opt.step()
            tot += loss.item()
            nb += 1
        log(f"  [ep{ep+1}] loss={tot/max(nb,1):.4f} rule={r_sum/max(nb,1):.4f} ({time.time()-t0:.0f}s)")
    model.eval()
    return model


# ════════════════════════════════════════════════════════
# 主流程
# ════════════════════════════════════════════════════════
def load_v10():
    cfg = Config()
    with open(os.path.join(V10_DIR, "embedding_model.json"), "r", encoding="utf-8") as f:
        jdata = json.load(f)
    for k, v in (jdata.get("config") or {}).items():
        setattr(cfg, k, v)
    model = BaizeEmbeddingV10(cfg)
    sd = torch.load(os.path.join(V10_DIR, "best_model.pt"),
                    map_location="cpu", weights_only=False)
    model.load_state_dict(sd, strict=False)
    return model.to(DEVICE).eval()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--quick", action="store_true")
    ap.add_argument("--distill-n", type=int, default=2000)
    ap.add_argument("--epochs", type=int, default=10)
    args = ap.parse_args()

    os.makedirs(OUT_DIR, exist_ok=True)
    log_path = os.path.join(OUT_DIR, "V14_log.txt")
    result_path = os.path.join(OUT_DIR, "V14_result.json")

    def log(msg):
        line = f"[{time.strftime('%H:%M:%S')}] {msg}"
        print(line, flush=True)
        with open(log_path, "a", encoding="utf-8") as f:
            f.write(line + "\n")

    log(f"device={DEVICE} seed={SEED} 规则目标cos={RULE_TARGET}")
    bench_pairs = load_bench_pairs()
    rule_pairs = make_rule_pairs()
    log(f"benchmark 54 对 | 规则对 {len(rule_pairs)} (词级+句级, 已去泄漏/补动词)")

    def report(tag, r):
        log(f"{tag} 分离度={r['separation']:+.3f} 官方压制={r['official_suppression']:.1%} "
            f"否定cos={r['cos_mean_per_cat']['negation']:.3f} | 总分={r['spearman_total']:.4f} "
            f"(同义{r['spearman_per_cat']['synonym']:+.3f} 反义{r['spearman_per_cat']['antonym']:+.3f} "
            f"程度{r['spearman_per_cat']['degree']:+.3f} 跨域{r['spearman_per_cat']['cross_domain']:+.3f} "
            f"否定ρ{r['spearman_per_cat']['negation']:+.3f}*)")

    results = {}

    def save():
        with open(result_path, "w", encoding="utf-8") as f:
            json.dump(results, f, ensure_ascii=False, indent=1)

    # 恢复已完成组
    if os.path.exists(result_path):
        try:
            with open(result_path, encoding="utf-8") as f:
                results = json.load(f)
            log(f"已有结果组: {list(results.keys())}")
        except Exception:
            results = {}

    # S0 基线
    if "S0" not in results:
        v10 = load_v10()
        r0 = evaluate_on_bench(v10, "S0-V10基线", bench_pairs)
        report("S0", r0)
        results["S0"] = r0
        save()
    else:
        v10 = load_v10()
        report("S0", results["S0"])

    if args.quick:
        save()
        return

    distill_rows = load_distill_rows(args.distill_n)
    log(f"蒸馏数据: {len(distill_rows)} 行")

    # S1 恒等验证
    if "S1" not in results:
        v14 = BaizeEmbeddingV14(load_v10()).to(DEVICE).eval()
        probe = torch.tensor([text_ids("今天天气真好")], device=DEVICE)
        with torch.no_grad():
            d = (v14(probe) - v10(probe)).abs().max().item()
        log(f"S1 恒等验证 max|Δ|={d:.2e} (要求<1e-5)")
        assert d < 1e-5, "恒等失败!"
        results["S1_identity"] = d
        save()

    def run_group(key, build_model, get_params, lam_d, lam_r, label):
        if key in results:
            log(f"══ {label} 已完成, 跳过 ══")
            return None
        log(f"══ {label} ══")
        model = build_model()
        model = train_new_params(model, get_params(model), distill_rows, rule_pairs, log,
                                 epochs=args.epochs, lam_distill=lam_d, lam_rule=lam_r, tag=key)
        r = evaluate_on_bench(model, label, bench_pairs)
        report(key, r)
        results[key] = r
        save()
        torch.save({"model": model.state_dict()}, os.path.join(OUT_DIR, f"v14_{key}.pt"))
        diag_charbag(model, label, log)
        del model
        torch.cuda.empty_cache()
        import gc; gc.collect()
        return r

    # S2: V14 + 纯蒸馏
    run_group("S2", lambda: BaizeEmbeddingV14(load_v10()).to(DEVICE).eval(),
              lambda m: list(m.dw_layers.parameters()), 1.0, 0.0, "S2-V14纯蒸馏")

    # S3: V14 + 蒸馏 + 轻规则 (主实验)
    run_group("S3", lambda: BaizeEmbeddingV14(load_v10()).to(DEVICE).eval(),
              lambda m: list(m.dw_layers.parameters()), 1.0, 0.3, "S3-V14结构+轻规则")

    # S5: V11门控 + 蒸馏 + 轻规则 (对照, 无新结构)
    run_group("S5", lambda: BaizeEmbeddingGated(load_v10()).to(DEVICE).eval(),
              lambda m: list(m.gate_layers.parameters()), 1.0, 0.3, "S5-门控+轻规则(对照)")

    log("完成")


if __name__ == "__main__":
    main()
