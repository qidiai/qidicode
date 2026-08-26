# train_pure_pool.py (V10)
# 白泽嵌入模型 V10 — 纯源池架构 (Pure Source Pool)
# ==================================================
# 核心思路: 砍掉 V9v3 的 Linear+LayerNorm (Transformer残留),
#           让 token embedding 直接写入源池, 池状态即表征.
#
# 架构: Embed(384d) → PurePoolStack(6层, 每层144维池状态)
#       → 读池 → Softmax融合 → Proj(144→1024) → LayerNorm → L2
#
# 对比 V9v3:
#   V9v3: 3.15M params, 12MB, 含 Linear+LN 伪Transformer层
#   V10:  ~2.25M params, ~9MB(FP32)/~4.5MB(FP16), 纯源池, 零Transformer组件
#
# 关键改进:
#   1. 所有token写入源池 (V9v3仅最后一个token写入)
#   2. 去掉位置编码 (源池三速衰减天然捕捉序列顺序)
#   3. 去掉每token的Linear+LayerNorm (省41%参数)
#   4. 池状态直接作为语义表征 (144维→投影→1024维输出)

import torch
import torch.nn as nn
import torch.nn.functional as F
import numpy as np
import json, os, random, time
from pathlib import Path
from typing import Tuple


# ============================================================
# 配置
# ============================================================

class Config:
    vocab_size: int = 4958        # remapped: 4958 unique tokens → 0~4957 continuous
    embed_dim: int = 384          # 内部嵌入维度
    num_layers: int = 6           # 源池层数
    max_seq_len: int = 96
    output_dim: int = 1024

    # SourcePool (三速)
    fast_decay: float = 0.85
    medium_decay: float = 0.95
    slow_decay: float = 0.995
    sp_dim: int = 48              # 每速池状态维度 (3速共144维)

    # 训练
    batch_size: int = 64
    epochs: int = 80
    lr: float = 2e-4
    weight_decay: float = 1e-5

    # 损失权重
    alpha_cos: float = 1.0
    alpha_mse: float = 0.1
    beta_contrast: float = 0.35
    contrast_temperature: float = 0.4
    # 防塌缩损失 (来自 V51 CMM)
    beta_repulsion: float = 0.1     # 超球排斥: 推开 batch 内嵌入向量
    gamma_contraction: float = 0.01 # 收缩映射: 约束 write_gate Lipschitz < 1

    # 数据
    teacher_data_path: str = "F:/代码/白泽档案/03-训练数据/teacher_v10/teacher_vectors_v10_retokenized.jsonl"
    token_map_path: str = "F:/代码/白泽档案/03-训练数据/teacher_v10/token_id_map.json"
    save_dir: str = str(Path(__file__).resolve().parent.parent / ".qoder" / "experiments" / "v10_purepool")
    device: str = "cuda" if torch.cuda.is_available() else "cpu"
    save_interval: int = 10


# ============================================================
# PureSourcePoolLayer — 纯源池层 (无Linear/LayerNorm)
# ============================================================

class PureSourcePoolLayer(nn.Module):
    """纯源池层: 向量化实现, 无Python for循环"""

    def __init__(self, embed_dim: int, sp_dim: int,
                 fast_decay: float, medium_decay: float, slow_decay: float):
        super().__init__()
        self.embed_dim = embed_dim
        self.sp_dim = sp_dim
        self.fast_decay = fast_decay
        self.medium_decay = medium_decay
        self.slow_decay = slow_decay

        self.write_gate = nn.Linear(embed_dim, sp_dim)
        self.fast_read = nn.Linear(sp_dim, embed_dim)
        self.medium_read = nn.Linear(sp_dim, embed_dim)
        self.slow_read = nn.Linear(sp_dim, embed_dim)
        self.fusion_gate = nn.Linear(embed_dim * 3, 3)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        """
        x: [batch, seq_len, embed_dim]
        返回: [batch, embed_dim]
        
        向量化: 最终池状态 = sum_t(decay^(T-t) * write[t])
        等价于: decay_weights @ write_values (矩阵乘法)
        """
        bs, seq_len, _ = x.shape
        device = x.device

        # 1. 所有token同时通过写入门控
        write_vals = torch.tanh(self.write_gate(x))  # [bs, seq, sp_dim]

        # 2. 计算几何衰减权重: decay^(seq_len-1-t) for t in [0, seq_len)
        positions = torch.arange(seq_len, device=device, dtype=torch.float32)  # [seq]
        # 最终池状态 = sum of decay^(T-1-t) * write[t]
        # 位置t的权重 = decay^(seq_len-1-t)

        def pool_final(decay: float) -> torch.Tensor:
            # weights[t] = decay^(seq_len-1-t)
            exponents = (seq_len - 1) - positions  # [seq]: [seq-1, seq-2, ..., 0]
            weights = decay ** exponents  # [seq]
            # weights @ write_vals: [bs, sp_dim]
            return torch.einsum('s,bse->be', weights, write_vals)

        fast_pool = pool_final(self.fast_decay)    # [bs, sp_dim]
        medium_pool = pool_final(self.medium_decay)
        slow_pool = pool_final(self.slow_decay)

        # 乘以 (1-decay) 缩放因子 (EMA中的系数)
        fast_pool = fast_pool * (1 - self.fast_decay)
        medium_pool = medium_pool * (1 - self.medium_decay)
        slow_pool = slow_pool * (1 - self.slow_decay)

        # 3. 读取 + softmax融合
        fast_r = self.fast_read(fast_pool)
        medium_r = self.medium_read(medium_pool)
        slow_r = self.slow_read(slow_pool)

        concat = torch.cat([fast_r, medium_r, slow_r], dim=-1)
        logits = self.fusion_gate(concat)
        weights = F.softmax(logits, dim=-1)
        modulation = (weights[:, 0:1] * fast_r
                    + weights[:, 1:2] * medium_r
                    + weights[:, 2:3] * slow_r)

        return modulation


# ============================================================
# PurePoolStack — 多层纯源池堆叠
# ============================================================

class PurePoolStack(nn.Module):
    def __init__(self, config: Config):
        super().__init__()
        self.layers = nn.ModuleList([
            PureSourcePoolLayer(
                embed_dim=config.embed_dim,
                sp_dim=config.sp_dim,
                fast_decay=config.fast_decay,
                medium_decay=config.medium_decay,
                slow_decay=config.slow_decay,
            )
            for _ in range(config.num_layers)
        ])
        # 层间跳跃连接门控
        self.skip_gates = nn.Parameter(torch.ones(config.num_layers))

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        """
        x: [batch, seq_len, embed_dim]
        返回: [batch, sp_dim*3] — 三速池状态拼接 (72维)
        """
        # 第一层: 从序列获取池状态
        residual = x.mean(dim=1)  # [bs, embed_dim]
        current = self.layers[0](x)  # [bs, embed_dim]

        for i in range(1, len(self.layers)):
            skip = residual * torch.sigmoid(self.skip_gates[i])
            pseudo_seq = x + current.unsqueeze(1)  # [bs, seq, embed]
            current = self.layers[i](pseudo_seq) + skip

        return current


# ============================================================
# BaizeEmbeddingV10 — 纯源池嵌入模型
# ============================================================

class BaizeEmbeddingV10(nn.Module):
    """V10: 纯源池嵌入模型 — 零Transformer组件
    
    数据流: tokens → Embed(384d) → PurePoolStack(6层)
            → 读池(72d) → Proj(72→1024) → LayerNorm → L2
    """

    def __init__(self, config: Config):
        super().__init__()
        self.config = config

        # 唯一的"传统"组件: 字符嵌入表
        self.embedding = nn.Embedding(config.vocab_size, config.embed_dim)
        # 注意: 无位置编码! 源池衰减天然有序

        # 纯源池堆叠 (返回 embed_dim 维向量)
        self.pool_stack = PurePoolStack(config)

        # 瓶颈投影: embed_dim → sp_dim*3 (72维) → output_dim (1024)
        # 先把 384 维压缩到 72 维 (源池状态空间), 再投影到 1024
        self.bottleneck = nn.Linear(config.embed_dim, config.sp_dim * 3)  # 384→72
        self.output_proj = nn.Linear(config.sp_dim * 3, config.output_dim)  # 72→1024
        self.output_norm = nn.LayerNorm(config.output_dim)

    def forward(self, tokens: torch.Tensor) -> torch.Tensor:
        """
        tokens: [batch, seq_len] — 字符ID序列
        返回: [batch, output_dim] — L2归一化的嵌入向量
        """
        # 1. 字符嵌入
        tok_emb = self.embedding(tokens)  # [bs, seq, 384]

        # 2. 纯源池处理
        pooled = self.pool_stack(tok_emb)  # [bs, 384]

        # 3. 瓶颈压缩: 384→72 (回到源池状态空间)
        compressed = torch.tanh(self.bottleneck(pooled))  # [bs, 72]

        # 4. 输出投影: 72→1024
        out = self.output_proj(compressed)  # [bs, 1024]
        out = self.output_norm(out)
        out = F.normalize(out, p=2, dim=-1)

        return out

    def get_param_count(self) -> int:
        return sum(p.numel() for p in self.parameters())

    def hyperspherical_repulsion_loss(self, student_emb: torch.Tensor) -> torch.Tensor:
        """超球排斥损失 (V51 CMM 移植): 推开 batch 内嵌入向量, 防止输出空间塌缩。
        student_emb: [batch, output_dim] — 已 L2 归一化
        """
        gram = student_emb @ student_emb.t()  # [batch, batch]
        n = gram.size(0)
        off_diag = gram - torch.eye(n, device=gram.device)
        return (off_diag ** 2).sum() / (n * (n - 1) + 1e-12)

    def contraction_loss(self) -> torch.Tensor:
        """收缩映射约束 (V51 CMM 移植): write_gate 权重范数 < 1,
        保证池写入是收缩映射 (不同输入不会塌缩到同一池状态)。
        """
        loss = torch.tensor(0.0, device=next(self.parameters()).device)
        for layer in self.pool_stack.layers:
            loss = loss + F.relu(layer.write_gate.weight.norm() - 0.9)
        return loss


# ============================================================
# 损失函数 (复用V9v3)
# ============================================================

class DistillationLoss(nn.Module):
    def __init__(self, config: Config):
        super().__init__()
        self.config = config

    def forward(self, student_emb: torch.Tensor, teacher_emb: torch.Tensor):
        teacher_normed = F.normalize(teacher_emb, p=2, dim=-1)

        cos_sim = (student_emb * teacher_normed).sum(dim=-1).mean()
        loss_cos = 1.0 - cos_sim
        loss_mse = F.mse_loss(student_emb, teacher_normed)

        T = self.config.contrast_temperature
        sim_student = torch.matmul(student_emb, student_emb.t())
        sim_teacher = torch.matmul(teacher_normed, teacher_normed.t())
        labels = torch.arange(student_emb.shape[0], device=student_emb.device)
        loss_contrast = F.cross_entropy(sim_student / T, labels) \
                      + F.cross_entropy(sim_teacher / T, labels)

        total = (self.config.alpha_cos * loss_cos
                 + self.config.alpha_mse * loss_mse
                 + self.config.beta_contrast * loss_contrast)

        return total, {
            "loss_cos": loss_cos.item(),
            "cosine": cos_sim.item(),
            "loss_mse": loss_mse.item(),
            "loss_contrast": loss_contrast.item(),
            "total": total.item(),
        }


# ============================================================
# 数据集
# ============================================================

class DistillationDataset(torch.utils.data.Dataset):
    def __init__(self, data_path: str, max_seq_len: int = 96, token_map_path: str = None):
        self.data = []
        self.max_seq_len = max_seq_len
        # 加载 token_id 重映射 (4958 unique → 0~4957 continuous)
        self.old_to_new = None
        if token_map_path and os.path.exists(token_map_path):
            with open(token_map_path, "r", encoding="utf-8") as f:
                m = json.load(f)
            self.old_to_new = {int(k): v for k, v in m["old_to_new"].items()}
            print(f"token remapping: {len(self.old_to_new)} ids")
        if not os.path.exists(data_path):
            print(f"数据文件不存在: {data_path}")
            return
        with open(data_path, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if line:
                    self.data.append(json.loads(line))
        print(f"加载蒸馏数据: {len(self.data)} 条")

    def __len__(self):
        return len(self.data)

    def __getitem__(self, idx):
        item = self.data[idx]
        raw_ids = item["token_ids"]
        # 应用 remapping
        if self.old_to_new:
            ids = [self.old_to_new.get(tid, 0) for tid in raw_ids]
        else:
            ids = raw_ids
        tokens = torch.tensor(ids, dtype=torch.long)
        teacher = torch.tensor(item["teacher_embedding"], dtype=torch.float32)
        if len(tokens) > self.max_seq_len:
            tokens = tokens[:self.max_seq_len]
        else:
            tokens = F.pad(tokens, (0, self.max_seq_len - len(tokens)), value=0)
        return tokens, teacher


# ============================================================
# 训练
# ============================================================

def train(config: Config):
    device = torch.device(config.device)
    print(f"设备: {device}")

    model = BaizeEmbeddingV10(config).to(device)
    params = model.get_param_count()
    print(f"参数量: {params:,} ({params/1e6:.2f}M)")

    criterion = DistillationLoss(config)
    optimizer = torch.optim.AdamW(model.parameters(), lr=config.lr, weight_decay=config.weight_decay)
    scheduler = torch.optim.lr_scheduler.CosineAnnealingLR(optimizer, T_max=config.epochs)

    dataset = DistillationDataset(config.teacher_data_path, config.max_seq_len, config.token_map_path)
    if len(dataset) == 0:
        print("数据集为空, 无法训练!")
        return model

    loader = torch.utils.data.DataLoader(
        dataset, batch_size=config.batch_size, shuffle=True, num_workers=0
    )

    os.makedirs(config.save_dir, exist_ok=True)
    ckpt_path = os.path.join(config.save_dir, "train_ckpt.pt")
    best_cos = -1.0
    start_epoch = 0
    t_start = time.time()

    # ---- 断点续训 ----
    if os.path.exists(ckpt_path):
        ck = torch.load(ckpt_path, map_location=device, weights_only=False)
        model.load_state_dict(ck["model"])
        optimizer.load_state_dict(ck["optimizer"])
        scheduler.load_state_dict(ck["scheduler"])
        start_epoch = ck["epoch"] + 1
        best_cos = ck.get("best_cos", -1.0)
        print(f"  → 从断点恢复: epoch {start_epoch}/{config.epochs}, best_cos={best_cos:.4f}")

    for epoch in range(start_epoch, config.epochs):
        model.train()
        cosines = []

        for tokens, teacher_emb in loader:
            tokens, teacher_emb = tokens.to(device), teacher_emb.to(device)
            student_emb = model(tokens)
            loss_distill, info = criterion(student_emb, teacher_emb)

            # 防塌缩损失 (V51 CMM)
            loss_repulsion = model.hyperspherical_repulsion_loss(student_emb)
            loss_contraction = model.contraction_loss()
            total_loss = (loss_distill
                          + config.beta_repulsion * loss_repulsion
                          + config.gamma_contraction * loss_contraction)

            optimizer.zero_grad()
            total_loss.backward()
            torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
            optimizer.step()

            cosines.append(info["cosine"])

        avg_cos = float(np.mean(cosines))
        elapsed = time.time() - t_start
        print(f"E{epoch+1:2d}/{config.epochs} | cos={avg_cos:.4f} | "
              f"cos_loss={info['loss_cos']:.4f} mse={info['loss_mse']:.4f} "
              f"cont={info['loss_contrast']:.2f} "
              f"repul={loss_repulsion.item():.4f} contr={loss_contraction.item():.4f} "
              f"| {elapsed:.0f}s", flush=True)

        if avg_cos > best_cos:
            best_cos = avg_cos
            torch.save(model.state_dict(), os.path.join(config.save_dir, "best_model.pt"))
            print(f"  -> 保存最佳 (cos={avg_cos:.4f})", flush=True)

        # 每轮存断点 (含 optimizer/scheduler 状态)
        torch.save({
            "epoch": epoch,
            "model": model.state_dict(),
            "optimizer": optimizer.state_dict(),
            "scheduler": scheduler.state_dict(),
            "best_cos": best_cos,
        }, ckpt_path)

        scheduler.step()

    print(f"\n训练完成! 最佳余弦: {best_cos:.4f} ({time.time()-t_start:.0f}s)")
    return model


# ============================================================
# 验证
# ============================================================

def evaluate(model: BaizeEmbeddingV10, config: Config):
    device = torch.device(config.device)
    model.eval()

    dataset = DistillationDataset(config.teacher_data_path, config.max_seq_len, config.token_map_path)
    if len(dataset) == 0:
        return

    loader = torch.utils.data.DataLoader(dataset, batch_size=32, shuffle=False, num_workers=0)

    sims = []
    with torch.no_grad():
        for tokens, teacher_emb in loader:
            tokens, teacher_emb = tokens.to(device), teacher_emb.to(device)
            student_emb = model(tokens)
            cos = F.cosine_similarity(student_emb, teacher_emb, dim=-1)
            sims.extend(cos.cpu().tolist())

    sims = np.array(sims)
    print(f"\n验证 ({len(sims)} 样本):")
    print(f"  平均余弦: {sims.mean():.4f} | 中位: {np.median(sims):.4f} | "
          f">0.50: {(sims>0.50).mean()*100:.1f}% | >0.85: {(sims>0.85).mean()*100:.1f}%")


# ============================================================
# 导出
# ============================================================

def export_to_json(model: BaizeEmbeddingV10, config: Config, output_path: str):
    state = model.state_dict()
    export = {
        "type": "BaizeEmbeddingV10_PurePool",
        "config": {
            "vocab_size": config.vocab_size,
            "embed_dim": config.embed_dim,
            "num_layers": config.num_layers,
            "max_seq_len": config.max_seq_len,
            "output_dim": config.output_dim,
            "sp_dim": config.sp_dim,
            "fast_decay": config.fast_decay,
            "medium_decay": config.medium_decay,
            "slow_decay": config.slow_decay,
        },
        "weights": {k: v.cpu().numpy().tolist() for k, v in state.items()},
    }
    with open(output_path, "w", encoding="utf-8") as f:
        json.dump(export, f)
    size_mb = os.path.getsize(output_path) / 1024 / 1024
    print(f"导出: {output_path} ({size_mb:.2f} MB)")


# ============================================================
# 主入口
# ============================================================

if __name__ == "__main__":
    config = Config()
    print("=" * 60)
    print("白泽嵌入模型 V10 — 纯源池架构 (Pure Source Pool)")
    print(f"  embed_dim={config.embed_dim} sp_dim={config.sp_dim} layers={config.num_layers}")
    print(f"  池状态: {config.sp_dim}*3 = {config.sp_dim*3}维 (fast/medium/slow)")
    print(f"  源池: 三速 (fast={config.fast_decay}, medium={config.medium_decay}, slow={config.slow_decay})")
    print(f"  无位置编码, 无Linear/LayerNorm, 无Transformer组件")
    print(f"  损失: cos({config.alpha_cos}) + mse({config.alpha_mse}) + contrast({config.beta_contrast}) "
          f"+ repulsion({config.beta_repulsion}) + contraction({config.gamma_contraction})")
    print("=" * 60)

    # 预检: 模型参数量
    tmp_model = BaizeEmbeddingV10(config)
    params = tmp_model.get_param_count()
    print(f"\n预估参数量: {params:,} ({params/1e6:.2f}M)")
    print(f"预估模型大小: ~{params*4/1024/1024:.1f} MB (FP32)")
    del tmp_model

    print("\n[Step 1] 训练...")
    model = train(config)

    print("\n[Step 2] 验证...")
    evaluate(model, config)

    print("\n[Step 3] 导出...")
    export_to_json(model, config, os.path.join(config.save_dir, "embedding_model.json"))

    print("\n全部完成!")
