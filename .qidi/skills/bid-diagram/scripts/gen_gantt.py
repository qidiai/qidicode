#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""横道图（甘特图）生成器 — 标书施工进度计划。

用法:
    python gen_gantt.py tasks.json -o 横道图.png --title "施工进度计划" --project "XX项目"

tasks.json: [{"name", "start_day", "duration", "category", "is_critical", "predecessor"}]
依赖: pip install matplotlib
"""

import argparse
import json
import sys
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import matplotlib.dates as mdates
from matplotlib.patches import FancyArrowPatch
from datetime import timedelta, date

plt.rcParams["font.sans-serif"] = ["SimSun", "Microsoft YaHei"]
plt.rcParams["axes.unicode_minus"] = False

# 施工标准配色（与标书软件 chart_generator 同源）
COLORS = {
    "土方工程": "#FF6B6B", "基础工程": "#4ECDC4", "主体结构": "#45B7D1",
    "装饰装修": "#96CEB4", "机电安装": "#FFEAA7", "室外工程": "#DDA0DD",
    "竣工验收": "#87CEEB", "default": "#A8D8EA",
    "critical": "#FF4757",
}

START = date(2026, 9, 1)  # 开工基准日（仅用于横轴日期显示，可自行调整）


def day_to_date(d):
    return START + timedelta(days=d - 1)


def generate(tasks, out, title, project):
    tasks = sorted(tasks, key=lambda t: (t.get("start_day", 0), -t.get("duration", 0)))
    n = len(tasks)
    fig_height = max(5, 0.55 * n + 2)
    fig, ax = plt.subplots(figsize=(14, fig_height), dpi=150)

    yticks, ylabels = [], []
    name_idx = {t["name"]: i for i, t in enumerate(tasks)}
    for i, t in enumerate(tasks):
        y = n - 1 - i
        yticks.append(y)
        ylabels.append(t["name"])
        s, d = t.get("start_day", 1), t.get("duration", 1)
        cat = t.get("category", "default")
        color = COLORS.get(cat, COLORS["default"])
        edge = "#2F3542"
        lw = 1.5
        if t.get("is_critical"):
            color = COLORS["critical"]
            edge = "#C0392B"
            lw = 2.2
        ax.barh(y, d, left=s - 1, height=0.62, color=color, edgecolor=edge,
                linewidth=lw, zorder=3)
        # 时长标注
        ax.text(s - 1 + d / 2, y, f"{d}d", ha="center", va="center",
                fontsize=8, zorder=4,
                color="white" if t.get("is_critical") else "#2F3542")

    # 逻辑关系箭头
    for t in tasks:
        pred = t.get("predecessor")
        if pred and pred in name_idx:
            j = name_idx[pred]
            x1 = tasks[j].get("start_day", 1) + tasks[j].get("duration", 1) - 1
            y1 = n - 1 - j
            x2 = t.get("start_day", 1) - 1
            y2 = n - 1 - name_idx[t["name"]]
            ax.annotate("", xy=(x2, y2), xytext=(x1, y1),
                        arrowprops=dict(arrowstyle="->", color="#7F8C8D", lw=0.8,
                                        connectionstyle="arc3,rad=0.15"),
                        zorder=2)

    total = max(t.get("start_day", 1) + t.get("duration", 1) - 1 for t in tasks)
    ax.set_xlim(0, total + 3)
    ax.set_yticks(yticks)
    ax.set_yticklabels(ylabels, fontsize=9)
    ax.set_xlabel("开工后第N天 / 日历日期", fontsize=10)

    # 上轴：日历日期
    sec = ax.secondary_xaxis("top")
    sec.set_xticks([0, total // 4, total // 2, total * 3 // 4, total])
    sec.set_xticklabels([day_to_date(max(1, d)).strftime("%m-%d")
                         for d in [1, total // 4, total // 2, total * 3 // 4, total]],
                        fontsize=8)

    ax.set_title(title + (f"（{project}）" if project else ""), fontsize=13,
                 fontweight="bold", pad=12)
    ax.grid(axis="x", linestyle="--", alpha=0.4, zorder=0)
    ax.set_axisbelow(True)

    # 图例（分类）
    from matplotlib.patches import Patch
    cats = sorted({t.get("category", "default") for t in tasks})
    handles = [Patch(facecolor=COLORS.get(c, COLORS["default"]), label=c) for c in cats]
    if any(t.get("is_critical") for t in tasks):
        handles.append(Patch(facecolor=COLORS["critical"], label="关键线路",
                             edgecolor="#C0392B", linewidth=2))
    ax.legend(handles=handles, loc="lower right", fontsize=8, ncol=min(4, len(handles)))

    plt.tight_layout()
    plt.savefig(out, bbox_inches="tight")
    plt.close(fig)
    print(f"OK: {out}（{n}道工序，总工期{total}天）")


def main():
    ap = argparse.ArgumentParser(description="标书横道图（甘特图）生成")
    ap.add_argument("tasks", help="tasks.json 路径")
    ap.add_argument("-o", "--output", default="横道图.png")
    ap.add_argument("--title", default="施工进度计划")
    ap.add_argument("--project", default="")
    args = ap.parse_args()

    p = Path(args.tasks)
    if not p.exists():
        print(f"文件不存在: {p}", file=sys.stderr)
        sys.exit(1)
    tasks = json.loads(p.read_text(encoding="utf-8-sig"))
    generate(tasks, args.output, args.title, args.project)


if __name__ == "__main__":
    main()
