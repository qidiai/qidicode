#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""进度S曲线生成器 — 计划vs实际累计进度对比。

用法:
    python gen_scurve.py schedule.json -o S曲线.png

schedule.json: {"project_name", "total_duration", "monthly_planned": [%], "monthly_actual": [%]}
依赖: pip install matplotlib
"""

import argparse
import json
import sys
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

plt.rcParams["font.sans-serif"] = ["SimSun", "Microsoft YaHei"]
plt.rcParams["axes.unicode_minus"] = False


def generate(data, out):
    planned = data["monthly_planned"]
    actual = data.get("monthly_actual", [])

    def cum(seq):
        s, r = 0, []
        for v in seq:
            s += v
            r.append(min(s, 100))
        return r

    cp, ca = cum(planned), cum(actual)
    months_p = [f"第{i+1}月" for i in range(len(cp))]
    months_a = [f"第{i+1}月" for i in range(len(ca))]

    fig, ax = plt.subplots(figsize=(11, 6), dpi=150)
    ax.plot(range(len(cp)), cp, "o-", color="#54A0FF", lw=2.2, ms=6, label="计划累计进度")
    if ca:
        ax.plot(range(len(ca)), ca, "s-", color="#FF6B6B", lw=2.2, ms=6, label="实际累计进度")
        # 当前偏差标注
        i = len(ca) - 1
        dev = ca[i] - cp[i]
        ax.annotate(f"偏差 {dev:+.0f}%",
                    xy=(i, ca[i]), xytext=(i + 0.2, ca[i] - 8),
                    fontsize=10, color="#C0392B", fontweight="bold",
                    arrowprops=dict(arrowstyle="->", color="#C0392B"))
    # 关键节点线
    for pct, label in [(50, "50%"), (80, "80%")]:
        ax.axhline(pct, ls="--", lw=0.8, color="#BDC3C7")
        ax.text(len(cp) - 0.5, pct + 1, label, fontsize=8, color="#95A5A6")

    ax.set_xticks(range(len(cp)))
    ax.set_xticklabels(months_p, fontsize=9)
    ax.set_xlabel("月份", fontsize=10)
    ax.set_ylabel("累计完成比例（%）", fontsize=10)
    ax.set_ylim(0, 105)
    ax.set_title(f"{data.get('project_name', '')}进度S曲线（计划vs实际）",
                 fontsize=13, fontweight="bold")
    ax.grid(ls="--", alpha=0.4)
    ax.legend(loc="lower right", fontsize=10)

    plt.tight_layout()
    plt.savefig(out, bbox_inches="tight")
    plt.close(fig)
    print(f"OK: {out}（计划{len(cp)}期，实际{len(ca)}期）")


def main():
    ap = argparse.ArgumentParser(description="进度S曲线生成")
    ap.add_argument("schedule", help="schedule.json 路径")
    ap.add_argument("-o", "--output", default="S曲线.png")
    args = ap.parse_args()

    p = Path(args.schedule)
    if not p.exists():
        print(f"文件不存在: {p}", file=sys.stderr)
        sys.exit(1)
    generate(json.loads(p.read_text(encoding="utf-8-sig")), args.output)


if __name__ == "__main__":
    main()
