#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""双代号网络图生成器 — 标书施工进度计划。

用法:
    python gen_network.py activities.json -o 网络图.png --title "施工网络计划图"

activities.json: [{"id", "name", "duration", "predecessors": []}]
自动计算最早/最迟时间与总时差，红色加粗标关键线路。
依赖: pip install matplotlib
"""

import argparse
import json
import sys
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import Circle, FancyArrowPatch

plt.rcParams["font.sans-serif"] = ["SimSun", "Microsoft YaHei"]
plt.rcParams["axes.unicode_minus"] = False

R = 0.35  # 节点半径


def topo_sort(acts):
    """拓扑排序（Kahn）。"""
    ids = {a["id"] for a in acts}
    indeg = {i: 0 for i in ids}
    for a in acts:
        for p in a.get("predecessors", []):
            indeg[a["id"]] += 1
    order, queue = [], [i for i, d in indeg.items() if d == 0]
    while queue:
        cur = queue.pop(0)
        order.append(cur)
        for a in acts:
            if cur in a.get("predecessors", []):
                indeg[a["id"]] -= 1
                if indeg[a["id"]] == 0:
                    queue.append(a["id"])
    if len(order) != len(ids):
        raise ValueError("活动存在环或前驱引用错误")
    return order


def compute_times(acts):
    """计算 ES/EF/LS/LF/TF 与关键线路。"""
    by_id = {a["id"]: a for a in acts}
    order = topo_sort(acts)
    for i in order:
        a = by_id[i]
        preds = [by_id[p] for p in a.get("predecessors", []) if p in by_id]
        a["ES"] = max((p["EF"] for p in preds), default=0)
        a["EF"] = a["ES"] + a.get("duration", 0)
    T = max(a["EF"] for a in acts)
    for i in reversed(order):
        a = by_id[i]
        succs = [b for b in acts if a["id"] in b.get("predecessors", [])]
        a["LF"] = min((b["LS"] for b in succs), default=T)
        a["LS"] = a["LF"] - a.get("duration", 0)
        a["TF"] = a["LS"] - a["ES"]
    critical = [a["id"] for a in acts if a["TF"] == 0]
    return T, critical, by_id


def layout(by_id, order):
    """分层布局：按 ES 值分列，同列纵向排开。"""
    cols = {}
    for i in order:
        a = by_id[i]
        cols.setdefault(a["ES"], []).append(i)
    pos = {}
    col_keys = sorted(cols)
    for ci, k in enumerate(col_keys):
        items = cols[k]
        for ri, aid in enumerate(items):
            x = ci * 3.0
            y = -ri * 1.4 + (len(items) - 1) * 0.7
            pos[aid] = (x, y)
    return pos


def generate(acts, out, title):
    order = topo_sort(acts)
    T, critical, by_id = compute_times(acts)
    pos = layout(by_id, order)

    # 节点：每个活动 → 箭线；构建事件节点（简化为活动起点=前驱终点）
    # 事件节点收集：每活动的 (前驱终点集, 自身终点)
    events = {}  # event_id -> (x, y)
    # 简化：每活动起点=其前驱（单一）位置，多前驱汇聚到该活动起点
    # 用活动的布局坐标画"箭线+中部标签"，首尾画小圆事件节点
    fig_w = max(10, (len(set(int(p[0]) for p in pos.values())) + 1) * 3)
    fig_h = max(5, (max(abs(p[1]) for p in pos.values()) + 1) * 1.6)
    fig, ax = plt.subplots(figsize=(fig_w, fig_h), dpi=150)

    for a in acts:
        x, y = pos[a["id"]]
        preds = [p for p in a.get("predecessors", []) if p in pos]
        for p in preds:
            px, py = pos[p]
            is_crit = a["id"] in critical and p in critical
            ax.annotate("", xy=(x, y), xytext=(px, py),
                        arrowprops=dict(
                            arrowstyle="-|>", lw=2.6 if is_crit else 1.2,
                            color="#E74C3C" if is_crit else "#576574",
                            shrinkA=6, shrinkB=6))
        # 无前驱：起点小节点
        if not preds:
            c = Circle((x - 0.6, y), 0.08, color="#2F3542", zorder=5)
            ax.add_patch(c)

    # 活动标签框
    for a in acts:
        x, y = pos[a["id"]]
        is_crit = a["id"] in critical
        label = f"{a['id']} {a['name']}\n({a.get('duration', 0)}d, TF={a['TF']})"
        ax.text(x, y, label, ha="center", va="center", fontsize=8.5,
                bbox=dict(boxstyle="round,pad=0.35",
                          facecolor="#FDEBD0" if is_crit else "#ECF0F1",
                          edgecolor="#E74C3C" if is_crit else "#576574",
                          linewidth=1.8 if is_crit else 0.8), zorder=4)

    # 终点节点
    ends = [a for a in acts if not any(a["id"] in b.get("predecessors", []) for b in acts)]
    for a in ends:
        x, y = pos[a["id"]]
        c = Circle((x + 0.9, y), 0.08, color="#2F3542", zorder=5)
        ax.add_patch(c)
        ax.text(x + 0.9, y - 0.4, f"总工期 {T}d", ha="center", fontsize=9,
                color="#C0392B", fontweight="bold")

    ax.set_title(title + f"（总工期 {T} 天，红色为关键线路）", fontsize=13,
                 fontweight="bold")
    ax.axis("off")
    all_x = [p[0] for p in pos.values()]
    all_y = [p[1] for p in pos.values()]
    ax.set_xlim(min(all_x) - 1.5, max(all_x) + 2)
    ax.set_ylim(min(all_y) - 1.2, max(all_y) + 1.2)

    plt.tight_layout()
    plt.savefig(out, bbox_inches="tight")
    plt.close(fig)

    # 输出时间参数表（JSON，供写入标书）
    table = [{"id": a["id"], "name": a["name"], "duration": a.get("duration", 0),
              "ES": a["ES"], "EF": a["EF"], "LS": a["LS"], "LF": a["LF"],
              "TF": a["TF"], "critical": a["TF"] == 0} for a in acts]
    print(f"OK: {out}（总工期{T}天，关键线路: {'→'.join(critical)}）")
    print(json.dumps(table, ensure_ascii=False, indent=2))


def main():
    ap = argparse.ArgumentParser(description="双代号网络图生成")
    ap.add_argument("activities", help="activities.json 路径")
    ap.add_argument("-o", "--output", default="网络图.png")
    ap.add_argument("--title", default="施工网络计划图")
    args = ap.parse_args()

    p = Path(args.activities)
    if not p.exists():
        print(f"文件不存在: {p}", file=sys.stderr)
        sys.exit(1)
    acts = json.loads(p.read_text(encoding="utf-8-sig"))
    generate(acts, args.output, args.title)


if __name__ == "__main__":
    main()
