#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""劳动力动态直方图生成器 — 分工种堆叠柱 + 总人数折线。

用法:
    python gen_labor.py labor.json -o 劳动力直方图.png

labor.json: {"project_name", "phases": [...], "workers": {工种: [各阶段人数]}}
数据须与劳动力闭合链一致（总工日与报价人工费偏差≤±5%）。
依赖: pip install matplotlib
"""

import argparse
import json
import sys
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

plt.rcParams["font.sans-serif"] = ["SimSun", "Microsoft YaHei"]
plt.rcParams["axes.unicode_minus"] = False

PALETTE = ["#54A0FF", "#FF6B6B", "#4ECDC4", "#FFEAA7", "#96CEB4", "#DDA0DD",
           "#87CEEB", "#FF9FF3", "#48DBFB", "#1DD1A1"]


def generate(data, out):
    phases = data["phases"]
    workers = data["workers"]
    names = list(workers.keys())
    matrix = np.array([workers[n] for n in names], dtype=float)  # 工种×阶段
    totals = matrix.sum(axis=0)
    peak = int(totals.max())
    avg = totals.mean()
    # 总工日（假设每阶段约30天）
    total_days = float(totals.sum() * 30)

    x = np.arange(len(phases))
    fig, ax = plt.subplots(figsize=(11, 6), dpi=150)
    bottom = np.zeros(len(phases))
    for i, n in enumerate(names):
        ax.bar(x, matrix[i], bottom=bottom, width=0.55,
               color=PALETTE[i % len(PALETTE)], label=n, edgecolor="white", lw=0.5)
        bottom += matrix[i]

    ax.plot(x, totals, "o-", color="#2F3542", lw=2, ms=6, label="总人数")
    for xi, t in zip(x, totals):
        ax.text(xi, t + peak * 0.03, f"{int(t)}人", ha="center", fontsize=9,
                color="#2F3542")

    ax.set_xticks(x)
    ax.set_xticklabels(phases, fontsize=10)
    ax.set_ylabel("人数", fontsize=10)
    ax.set_ylim(0, peak * 1.18)
    ax.set_title(f"{data.get('project_name', '')}劳动力动态配置图"
                 f"（峰值{peak}人/平均{avg:.0f}人/总工日约{total_days:,.0f}）",
                 fontsize=12.5, fontweight="bold")
    ax.grid(axis="y", ls="--", alpha=0.4)
    ax.legend(loc="upper left", fontsize=9, ncol=min(3, len(names)))

    plt.tight_layout()
    plt.savefig(out, bbox_inches="tight")
    plt.close(fig)
    print(f"OK: {out}（峰值{peak}人，平均{avg:.0f}人，总工日约{total_days:,.0f}）")
    print(f"校核提醒: 总工日约{total_days:,.0f}，与报价人工费反算工日偏差须≤±5%")


def main():
    ap = argparse.ArgumentParser(description="劳动力动态直方图生成")
    ap.add_argument("labor", help="labor.json 路径")
    ap.add_argument("-o", "--output", default="劳动力直方图.png")
    args = ap.parse_args()

    p = Path(args.labor)
    if not p.exists():
        print(f"文件不存在: {p}", file=sys.stderr)
        sys.exit(1)
    generate(json.loads(p.read_text(encoding="utf-8-sig")), args.output)


if __name__ == "__main__":
    main()
