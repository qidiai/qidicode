# -*- coding: utf-8 -*-
"""
合肥报价分析引擎（房建市政施工招标示范文本 202601 版 · 技术评分合理价格法）
================================================================
规则来源（2026-09-01 从合肥N1招标文件提取，与示范文本一致）：
  1. 异常低价线（评标办法前附表 3.6.3）：低于任一线触发异常低价评审
     ① 通过报价文件初步评审投标人投标报价的算术平均值 × 50%
     ② 通过报价文件初步评审投标人的次低投标报价 × 50%
     ③ 最高投标限价 × 45%
     ④ 评委基于专业判断（不可量化 → 用成本覆盖率提示）
     计算结果保留2位小数（第三位四舍五入）；未通过评审 → 否决投标
  2. 推荐规则（附件2）：S=进入报价评审家数
     S奇数：评标价不低于第(S+1)/2高报价的，按评标价由低到高推荐
     S偶数：评标价不低于第S/2高报价的，按评标价由低到高推荐
     评标价相同：技术详细评审得分高优先 → 评委投票
  3. 入围规则（附件1）：技术文件详细评审得分前5名（≥5家全入围）进入报价评审
  4. 承包人报价浮动率 L = (1 - 中标价/最高投标限价) × 100%（变更结算口径）
  5. 报价响应性（初步评审）：安全文明施工费/安责险符合、工日单价≥当地最低工资
     折算、暂列金额/暂估价不可改、增值税符合、总人工费不明显过低、
     报价缺漏≤3%且不影响排序
  6. 报价不参与评分（技术评分合理价格法）——报价目标 = 合规 + 进推荐序列
"""
from __future__ import annotations
import argparse
import io
import sys
from dataclasses import dataclass

if hasattr(sys.stdout, "buffer") and getattr(sys.stdout, "encoding", "").lower().replace("-", "") not in ("utf8",):
    # 已是 utf-8（如 PYTHONIOENCODING）时不二次包装，避免双重 TextIOWrapper GC 关闭底层流
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")


def r2(x: float) -> float:
    """合肥口径：保留2位小数，第三位四舍五入"""
    return round(x + 1e-9, 2)


# ---------------------------------------------------------------- 异常低价线
@dataclass
class AnomalyLines:
    avg_x50: float      # ① 算术平均×50%
    second_x50: float   # ② 次低×50%
    control_x45: float  # ③ 限价×45%
    effective: float    # 有效线 = max(①②③)（低于任一线即触发，最高线最紧）


def anomaly_lines(control_price: float, valid_prices: list) -> AnomalyLines:
    prices = sorted(valid_prices)
    if len(prices) < 2:
        raise ValueError("至少需要2家通过初步评审的报价（本家+至少1家竞对）")
    avg = sum(prices) / len(prices)
    return AnomalyLines(
        avg_x50=r2(avg * 0.50),
        second_x50=r2(prices[1] * 0.50),
        control_x45=r2(control_price * 0.45),
        effective=0.0,
    )


def check_price(price: float, control_price: float, valid_prices: list,
                my_cost: float | None = None) -> dict:
    """判定拟报价是否触发异常低价评审。my_cost=自家成本测算价（用于④风险提示）"""
    L = anomaly_lines(control_price, valid_prices)
    L.effective = max(L.avg_x50, L.second_x50, L.control_x45)
    rows = [
        ("① 初评报价算术平均×50%", L.avg_x50, price < L.avg_x50),
        ("② 次低报价×50%",        L.second_x50, price < L.second_x50),
        ("③ 最高投标限价×45%",     L.control_x45, price < L.control_x45),
    ]
    triggered = [r for r in rows if r[2]]
    cost_cover = (my_cost is not None and my_cost <= price)
    return {
        "price": price, "lines": rows, "effective_line": L.effective,
        "triggered": triggered,
        "need_clarification": bool(triggered),
        "must_prepare": ["报价合理性澄清说明", "成本测算书面说明（原材料/人工/制造费用）",
                         "合同履行能力与质量安全承诺"],
        "line4_risk": None if my_cost is None else ("可控：拟报价≥成本测算价" if cost_cover
                       else f"高风险：拟报价低于成本测算价 {my_cost:,.2f}（④评委专业判断最易否决）"),
        "float_rate": r2((1 - price / control_price) * 100),
    }


# ---------------------------------------------------------------- 推荐排序模拟
def recommendation_sim(my_price: float, competitor_prices: list,
                       my_tech_in_top5: bool = True) -> dict:
    """附件1+附件2 模拟。competitor_prices=通过技术详细评审的竞对评标价"""
    prices = sorted([my_price] + list(competitor_prices))          # 评标价由低到高
    S = len(prices)
    if not my_tech_in_top5:
        return {"in_review": False, "note": "技术详细评审未进前5 → 不进入报价评审（附件1）"}
    k = (S + 1) // 2 if S % 2 == 1 else S // 2                     # 第k高为门槛
    threshold = prices[-k]                                          # 第k高报价
    my_rank = prices.index(my_price) + 1                            # 由低到高位次
    in_zone = my_price >= threshold or my_rank == 1
    recommend_seq = [p for p in prices if p >= threshold]
    pos = recommend_seq.index(my_price) + 1 if my_price in recommend_seq else None
    return {
        "S": S, "k": k, "threshold": threshold,
        "my_rank_low_to_high": my_rank,
        "in_recommend_zone": in_zone,
        "recommend_count": len(recommend_seq),
        "my_recommend_position": pos,
        "sorted_prices": prices,
        "note": f"S={S}，门槛=第{k}高报价 {threshold:,.2f}；入围者中评标价不低于该门槛的按低到高推荐",
    }


# ---------------------------------------------------------------- 响应性清单
RESPONSE_CHECKLIST = [
    "安全文明施工费按规定足额报价（不可竞争）",
    "安全生产责任保险费按规定计取",
    "人工费工日单价 ≥ 合肥市最低工资标准折算单价",
    "暂列金额、暂估价未更改（与招标文件一致）",
    "增值税按一般计税方法报价",
    "总人工费与技术文件劳动力计划匹配（不明显过低 → 与劳动力工日口径联动）",
    "报价缺漏金额占比 ≤ 3%（多报不抵扣缺漏，按最高限价口径计算）且不影响排序",
    "工期/质量/投标有效期实质性响应",
]


# ---------------------------------------------------------------- 蒙特卡洛桥接（竞对样本可用时）
def monte_carlo_optimal(control_price: float, competitor_ratios: list):
    """复用标书软件 pricing_model（竞对比值=中标价/控制价历史样本）。
    N1类项目（报价不评分）无扣分函数时仅输出基准价分布参考。"""
    try:
        import numpy as np
        from pricing_model import (ScoringFunction, CompetitorModel,
                                   BenchmarkEstimator, MonteCarloEngine)
    except ImportError as e:
        return {"error": f"pricing_model 不可用: {e}", "fallback": "仅用规则层"}
    ratios = [r for r in competitor_ratios if 0.5 < r <= 1.0]
    if len(ratios) < 3:
        return {"error": "竞对样本<3，分布拟合不可靠"}
    comp = CompetitorModel(ratios=np.asarray(ratios))
    bench = BenchmarkEstimator(control_price=control_price, competitor_model=comp)
    engine = MonteCarloEngine(scoring_fn=ScoringFunction(), benchmark_estimator=bench)
    lo, hi = control_price * 0.80, control_price * 0.995
    best = engine.optimize(price_range=(lo, hi))
    return {"expected_benchmark": bench.expected_benchmark(n_competitors=5),
            "benchmark_interval": bench.benchmark_interval(n_competitors=5),
            "optimal_price": best}


# ---------------------------------------------------------------- 报告
def report(control_price: float, my_price: float, competitor_prices: list,
           my_cost: float | None = None, project: str = "未命名项目") -> str:
    chk = check_price(my_price, control_price, competitor_prices, my_cost)
    sim = recommendation_sim(my_price, competitor_prices)
    L = []
    a = L.append
    a(f"# 合肥报价分析报告 · {project}")
    a(f"> 控制价 {control_price:,.2f} 元 ｜ 拟报价 {my_price:,.2f} 元 ｜ "
      f"下浮率 L={chk['float_rate']}% ｜ 竞对样本 {len(competitor_prices)} 家")
    a("")
    a("## 一、异常低价四线判定（评标办法前附表 3.6.3）")
    a("| 线 | 金额（元） | 拟报价是否触发 |")
    a("|---|---|---|")
    for name, amt, hit in chk["lines"]:
        a(f"| {name} | {amt:,.2f} | {'⚠️ 触发' if hit else '✅ 未触发'} |")
    a(f"| **有效线（最紧）** | **{chk['effective_line']:,.2f}** | "
      f"**{'⚠️ 低于有效线' if my_price < chk['effective_line'] else '✅ 高于有效线'}** |")
    if chk["triggered"]:
        a("")
        a("**⚠️ 触发异常低价评审，须随投标文件准备：**")
        for x in chk["must_prepare"]:
            a(f"- {x}")
    else:
        a("")
        a("**✅ 高于全部可量化异常低价线，无需报价澄清预案**（④评委专业判断建议以成本测算备查）")
    if chk["line4_risk"]:
        a(f"- ④成本覆盖提示：{chk['line4_risk']}")
    a("")
    a("## 二、推荐排序模拟（附件1 + 附件2）")
    a(f"- S={sim['S']}（进入报价评审家数），门槛=第{sim['k']}高报价 {sim['threshold']:,.2f} 元")
    a(f"- 我方由低到高位次：第 {sim['my_rank_low_to_high']} 位")
    a(f"- 推荐区判定：{'✅ 进入推荐区' if sim['in_recommend_zone'] else '❌ 未进入推荐区'}"
      f"{'，推荐位次第 ' + str(sim['my_recommend_position']) + ' 位' if sim['my_recommend_position'] else ''}")
    a(f"- 排序明细（低→高）：{' < '.join(f'{p:,.0f}' for p in sim['sorted_prices'])}")
    a("")
    a("## 三、报价响应性清单（初步评审，逐项自查）")
    for x in RESPONSE_CHECKLIST:
        a(f"- [ ] {x}")
    a("")
    a("## 四、承包人报价浮动率（变更结算口径）")
    a(f"- L = (1 - {my_price:,.2f} / {control_price:,.2f}) × 100% = **{chk['float_rate']}%**"
      f"（工程变更单价按限价子目同比下浮 L）")
    return "\n".join(L)


# ---------------------------------------------------------------- CLI
def main():
    ap = argparse.ArgumentParser(description="合肥报价分析（技术评分合理价格法）")
    ap.add_argument("--control", type=float, required=True, help="最高投标限价")
    ap.add_argument("--my", type=float, required=True, help="拟投标报价")
    ap.add_argument("--competitors", type=str, default="", help="竞对报价逗号分隔（通过初评审口径）")
    ap.add_argument("--cost", type=float, default=None, help="自家成本测算价（④风险提示）")
    ap.add_argument("--project", type=str, default="未命名项目")
    ap.add_argument("--out", type=str, default=None, help="报告输出路径(.md)")
    a = ap.parse_args()
    comps = [float(x) for x in a.competitors.split(",") if x.strip()]
    rep = report(a.control, a.my, comps, a.cost, a.project)
    print(rep)
    if a.out:
        open(a.out, "w", encoding="utf-8").write(rep)
        print(f"\n[报告已写入] {a.out}")


if __name__ == "__main__":
    main()
