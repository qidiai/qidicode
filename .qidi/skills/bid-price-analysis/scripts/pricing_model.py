#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
报价数学模型引擎 — PricingModel
基于 numpy/scipy 的蒙特卡洛报价优化系统 + ARX 时序预测。

模型链：
  历史数据 → CompetitorModel(分布拟合) → BenchmarkEstimator(基准价估计)
           → MonteCarloEngine(模拟) → PricingOptimizer(求最优报价)
           → TrendAnalyzer(ARX+集合预测趋势)
           → KnowledgeGraph(报价图谱关联) → StrategyLearner(自校准)

核心数学：
  评分函数 S(p, B, R) = max(0, max_score - D(p, B, R))
  扣分函数 D(p, B, R) = |p - B| / B / step * deduction
  最优报价 p* = argmax E[S(p, B̂, R)]  (B̂ 为随机基准价)
  趋势预测 ARX: y(t) = Σaᵢy(t-i) + Σbⱼxⱼ(t) + e(t)
  集合预测: 200成员带噪声ARX → 1/99分位线

借鉴自进化实验 5 层正交架构：
  Layer 1: ARX(p) 自回归（含外生变量）
  Layer 2: 对数差分预处理（去趋势转平稳）
  Layer 3: 200成员集合预测（1/99分位线）
  Layer 4: 同类近期均价基线 + skill score
"""

from typing import Dict, List, Optional, Tuple
from dataclasses import dataclass, field

import numpy as np
from scipy import stats
from scipy.optimize import minimize_scalar

try:
    from utils.logger import get_logger
except ImportError:  # 技能独立运行时的桩实现
    import logging
    def get_logger(name=None):
        return logging.getLogger(name or 'pricing_model')

logger = get_logger(__name__)


# ============================================================
# 1. 评分函数模型
# ============================================================

class ScoringFunction:
    """报价评分函数 S(p, B, R) → score

    将招标文件中的报价扣分规则形式化为分段线性函数：

        ⎧ max_score                              p = B
        ⎨ max_score - (p-B)/B/above_step*above_ded   p > B
        ⎩ max_score - (B-p)/B/below_step*below_ded   p < B

    其中：
        p = 投标报价
        B = 评标基准价
        step = 扣分步长（%）
        deduction = 每步扣分值
    """

    def __init__(self, max_score: float = 80.0,
                 above_step: float = 1.0, above_deduction: float = 2.0,
                 below_step: float = 1.0, below_deduction: float = 1.0):
        self.max_score = max_score
        self.above_step = above_step
        self.above_deduction = above_deduction
        self.below_step = below_step
        self.below_deduction = below_deduction

    @classmethod
    def from_deduction_parsed(cls, parsed: Dict) -> 'ScoringFunction':
        """从 parse_deduction_rule 的输出构建"""
        above = parsed.get('above_benchmark', {}) or {}
        below = parsed.get('below_benchmark', {}) or {}
        return cls(
            above_step=above.get('deviation_percent', 1.0),
            above_deduction=above.get('deduction_per_unit', 2.0),
            below_step=below.get('deviation_percent', 1.0),
            below_deduction=below.get('deduction_per_unit', 1.0),
        )

    def score(self, price: float, benchmark: float) -> float:
        """计算给定报价在给定基准价下的得分"""
        if benchmark <= 0:
            return 0.0
        deviation_pct = (price - benchmark) / benchmark * 100  # 偏差百分比
        if deviation_pct > 0:
            deduction = (deviation_pct / self.above_step) * self.above_deduction
        elif deviation_pct < 0:
            deduction = (abs(deviation_pct) / self.below_step) * self.below_deduction
        else:
            deduction = 0.0
        return max(0.0, self.max_score - deduction)

    def score_array(self, prices: np.ndarray, benchmark) -> np.ndarray:
        """向量化计算（Monte Carlo 用）

        benchmark 可以是标量或 np.ndarray（每个模拟场景一个基准价）。
        """
        bench = np.asarray(benchmark, dtype=np.float64)
        if bench.ndim == 0:
            if float(bench) <= 0:
                return np.zeros_like(prices, dtype=np.float64)
            bench_full = np.full_like(prices, float(bench), dtype=np.float64)
        else:
            # broadcast: prices (n_candidates,) × bench (n_simulations,)
            bench_full = bench[np.newaxis, :]  # (1, n_sim)
            prices_2d = np.asarray(prices)[:, np.newaxis]  # (n_cand, 1)
            dev = (prices_2d - bench_full) / bench_full * 100
            ded = np.where(
                dev > 0,
                (dev / self.above_step) * self.above_deduction,
                np.where(dev < 0, (np.abs(dev) / self.below_step) * self.below_deduction, 0.0)
            )
            return np.maximum(0.0, self.max_score - ded)

        dev = (np.asarray(prices, dtype=np.float64) - bench_full) / bench_full * 100
        ded = np.where(
            dev > 0,
            (dev / self.above_step) * self.above_deduction,
            np.where(dev < 0, (np.abs(dev) / self.below_step) * self.below_deduction, 0.0)
        )
        return np.maximum(0.0, self.max_score - ded)

    def marginal_score(self, price: float, benchmark: float) -> float:
        """边际得分：报价每降低1元增加多少分

        在基准价附近，降价总是增加得分（高于或等于基准价时走 above 规则，
        低于基准价时走 below 规则）。
        """
        if benchmark <= 0 or price <= 0:
            return 0.0
        deviation_pct = (price - benchmark) / benchmark * 100
        if deviation_pct >= 0:
            # 在基准价上方或等于：降价走 above 规则
            return self.above_deduction / (self.above_step * benchmark / 100)
        else:
            return self.below_deduction / (self.below_step * benchmark / 100)

    def zero_score_range(self, benchmark: float) -> Tuple[float, float]:
        """得分为0的临界报价范围 (price_min, price_max)"""
        if benchmark <= 0:
            return (0, 0)
        # max_score = (dev / step) * deduction → dev = max_score * step / deduction
        above_dev = self.max_score * self.above_step / self.above_deduction if self.above_deduction > 0 else 100
        below_dev = self.max_score * self.below_step / self.below_deduction if self.below_deduction > 0 else 100
        return (benchmark * (1 - below_dev / 100), benchmark * (1 + above_dev / 100))

    def to_dict(self) -> Dict:
        return {
            'max_score': self.max_score,
            'above_step': self.above_step,
            'above_deduction': self.above_deduction,
            'below_step': self.below_step,
            'below_deduction': self.below_deduction,
        }


# ============================================================
# 2. 竞争对手报价分布模型
# ============================================================

class CompetitorModel:
    """竞争对手报价比率分布模型

    从历史中标价/控制价比率拟合统计分布，用于模拟竞争对手报价行为。
    比率 r = bid / control_price ∈ (0, 1]，典型范围 0.80~0.98。
    """

    def __init__(self, ratios: np.ndarray):
        """初始化并拟合分布

        Args:
            ratios: 历史报价比率数组 (bid/control_price)
        """
        self.ratios = np.array(ratios, dtype=np.float64)
        self.ratios = self.ratios[(self.ratios > 0.3) & (self.ratios <= 1.0)]

        self._dist = None
        self._dist_name = 'unknown'
        self._params = {}
        self._ks_pvalue = 0.0

        if len(self.ratios) >= 10:
            self._fit()
        else:
            # 数据不足时使用经验先验
            logger.warning("CompetitorModel: only %d samples, using prior defaults", len(self.ratios))
            self._dist = stats.norm(loc=0.92, scale=0.04)
            self._dist_name = 'norm_prior'
            self._params = {'mean': 0.92, 'std': 0.04}

    def _fit(self):
        """拟合分布：Beta vs TruncatedNormal，KS检验选优"""
        # --- 方案A: Beta 分布（天然定义在 [0,1]）---
        try:
            # 用矩估计做初始值，再 MLE 精化
            mean_r = np.mean(self.ratios)
            var_r = np.var(self.ratios, ddof=1)
            if var_r > 0:
                common = mean_r * (1 - mean_r) / var_r - 1
                a0 = max(0.1, mean_r * common)
                b0 = max(0.1, (1 - mean_r) * common)
                a_fit, b_fit, loc_fit, scale_fit = stats.beta.fit(
                    self.ratios, a0, b0, floc=0, fscale=1
                )
                ks_beta = stats.kstest(self.ratios, 'beta', args=(a_fit, b_fit, 0, 1))
                beta_dist = stats.beta(a_fit, b_fit, 0, 1)
                beta_p = ks_beta.pvalue
            else:
                beta_p = 0
        except Exception:
            beta_p = 0
            a_fit = b_fit = 0

        # --- 方案B: Truncated Normal ---
        try:
            mu = float(np.mean(self.ratios))
            sigma = max(float(np.std(self.ratios, ddof=1)), 0.01)
            a_tn = (0.3 - mu) / sigma
            b_tn = (1.0 - mu) / sigma
            tn_dist = stats.truncnorm(a_tn, b_tn, loc=mu, scale=sigma)
            ks_tn = stats.kstest(self.ratios, tn_dist.cdf)
            tn_p = ks_tn.pvalue
        except Exception:
            tn_p = 0
            mu = float(np.mean(self.ratios))
            sigma = max(float(np.std(self.ratios, ddof=1)), 0.01)

        # 选 KS p-value 更大的（更好拟合）
        if beta_p >= tn_p and beta_p > 0:
            self._dist = beta_dist
            self._dist_name = 'beta'
            self._params = {'alpha': a_fit, 'beta': b_fit}
            self._ks_pvalue = beta_p
        elif tn_p > 0:
            self._dist = tn_dist
            self._dist_name = 'truncnorm'
            self._params = {'mean': mu, 'std': sigma}
            self._ks_pvalue = tn_p
        else:
            # 都不理想，用 Normal 兜底
            mu = float(np.mean(self.ratios))
            sigma = max(float(np.std(self.ratios, ddof=1)), 0.01)
            self._dist = stats.norm(loc=mu, scale=sigma)
            self._dist_name = 'norm_fallback'
            self._params = {'mean': mu, 'std': sigma}
            self._ks_pvalue = 0

        logger.info("CompetitorModel fitted: %s, params=%s, KS p=%.4f, n=%d",
                     self._dist_name, {k: round(v, 4) for v in [self._params.get(k, 0)] for k in self._params} if False else self._params,
                     self._ks_pvalue, len(self.ratios))

    def sample_ratios(self, n: int, rng: Optional[np.random.Generator] = None) -> np.ndarray:
        """采样 n 个竞争对手的报价比率"""
        if rng is None:
            rng = np.random.default_rng()
        if self._dist is None:
            return rng.normal(0.92, 0.04, n)
        samples = self._dist.rvs(size=n, random_state=rng)
        # 裁剪到合理范围
        return np.clip(samples, 0.50, 1.00)

    def expected_ratio(self) -> float:
        """期望比率 E[r]"""
        if self._dist is None:
            return 0.92
        return float(self._dist.mean())

    def ratio_interval(self, confidence: float = 0.90) -> Tuple[float, float]:
        """比率的置信区间"""
        if self._dist is None:
            return (0.85, 0.98)
        alpha = (1 - confidence) / 2
        lo = float(self._dist.ppf(alpha))
        hi = float(self._dist.ppf(1 - alpha))
        return (max(0.5, lo), min(1.0, hi))

    def to_dict(self) -> Dict:
        return {
            'distribution': self._dist_name,
            'params': {k: round(float(v), 4) for k, v in self._params.items()},
            'ks_pvalue': round(self._ks_pvalue, 4),
            'sample_count': len(self.ratios),
            'mean': round(float(np.mean(self.ratios)), 4) if len(self.ratios) > 0 else 0.92,
            'std': round(float(np.std(self.ratios, ddof=1)), 4) if len(self.ratios) > 1 else 0.04,
            'expected_ratio': round(self.expected_ratio(), 4),
            'ci_90': [round(x, 4) for x in self.ratio_interval(0.9)],
        }


# ============================================================
# 3. 基准价估计器
# ============================================================

class BenchmarkEstimator:
    """评标基准价估计器

    根据评标方法，模拟基准价分布。

    评标方法：
        - arithmetic_mean: B = mean(有效报价) * control_price（可能去最高最低后取均值）
        - composite: B = w1 * control_price + w2 * mean(报价) * control_price
        - lowest_price: B = min(有效报价) * control_price
        - fixed: B = control_price * coefficient
    """

    def __init__(self, control_price: float,
                 method: str = 'composite',
                 competitor_model: Optional[CompetitorModel] = None,
                 formula_params: Optional[Dict] = None):
        self.control_price = control_price
        self.method = method
        self.competitor_model = competitor_model or CompetitorModel(np.array([]))
        self.formula_params = formula_params or {}

    def estimate_distribution(self, n_competitors: int = 5,
                              n_simulations: int = 5000,
                              rng: Optional[np.random.Generator] = None) -> np.ndarray:
        """模拟基准价分布

        Returns:
            np.ndarray: n_simulations 个模拟基准价
        """
        if rng is None:
            rng = np.random.default_rng()

        if self.method == 'fixed' or self.competitor_model._dist is None:
            coeff = self.formula_params.get('coefficient', 0.95)
            return np.full(n_simulations, self.control_price * coeff)

        # 采样竞争对手比率
        n_total = n_simulations * n_competitors
        all_ratios = self.competitor_model.sample_ratios(n_total, rng)
        ratio_matrix = all_ratios.reshape(n_simulations, n_competitors)

        if self.method == 'arithmetic_mean':
            # 去最高最低后取均值（n<=2 时不去掉）
            if n_competitors > 4:
                trimmed = np.sort(ratio_matrix, axis=1)[:, 1:-1]
                benchmarks = np.mean(trimmed, axis=1) * self.control_price
            else:
                benchmarks = np.mean(ratio_matrix, axis=1) * self.control_price

        elif self.method == 'composite':
            w_ctrl = self.formula_params.get('control_price_weight', 0.5)
            w_avg = self.formula_params.get('avg_price_weight', 0.5)
            # 复合基准价 = w1*控制价 + w2*有效报价均值
            avg_ratios = np.mean(ratio_matrix, axis=1)
            coeff = self.formula_params.get('coefficient', 1.0)
            benchmarks = (w_ctrl * self.control_price +
                          w_avg * avg_ratios * self.control_price) * coeff

        elif self.method == 'lowest_price':
            benchmarks = np.min(ratio_matrix, axis=1) * self.control_price

        else:
            # 默认综合评估法
            benchmarks = np.mean(ratio_matrix, axis=1) * self.control_price

        return benchmarks

    def expected_benchmark(self, n_competitors: int = 5) -> float:
        """基准价期望值"""
        dist = self.estimate_distribution(n_competitors, n_simulations=2000)
        return float(np.median(dist))

    def benchmark_interval(self, n_competitors: int = 5,
                           confidence: float = 0.90) -> Tuple[float, float]:
        """基准价置信区间"""
        dist = self.estimate_distribution(n_competitors, n_simulations=2000)
        alpha = (1 - confidence) / 2
        return (float(np.percentile(dist, alpha * 100)),
                float(np.percentile(dist, (1 - alpha) * 100)))


# ============================================================
# 4. 蒙特卡洛模拟引擎
# ============================================================

class MonteCarloEngine:
    """报价蒙特卡洛模拟引擎

    对每个候选报价，模拟大量场景（基准价随机），计算得分分布和胜率。
    """

    def __init__(self, scoring_fn: ScoringFunction,
                 benchmark_estimator: BenchmarkEstimator,
                 n_simulations: int = 10000):
        self.scoring_fn = scoring_fn
        self.benchmark_estimator = benchmark_estimator
        self.n_simulations = n_simulations

    def simulate(self, candidate_prices: List[float],
                 n_competitors: int = 5,
                 rng: Optional[np.random.Generator] = None) -> Dict:
        """对每个候选报价模拟 n_simulations 次

        Returns:
            {
                'prices': [...],
                'expected_scores': [...],
                'score_p5': [...], 'score_p50': [...], 'score_p95': [...],
                'win_probabilities': [...],
                'best_price_index': int,
            }
        """
        if rng is None:
            rng = np.random.default_rng()

        prices = np.array(candidate_prices, dtype=np.float64)

        # 1. 生成基准价分布
        benchmarks = self.benchmark_estimator.estimate_distribution(
            n_competitors, self.n_simulations, rng
        )

        # 2. 对所有候选价格批量计算得分（向量化）
        # prices: (n_candidates,), benchmarks: (n_simulations,)
        # score_array 支持 broadcast → scores: (n_candidates, n_simulations)
        all_scores = self.scoring_fn.score_array(prices, benchmarks)

        expected_scores = []
        score_p5s, score_p50s, score_p95s = [], [], []
        win_probs = []

        # 2026-08-27 审计 B7 修复：原字段名 win_probabilities 语义误导——
        # 它是"报价得分≥满分60%的达标概率"，不是"击败对手的中标概率"。
        # 重命名为 score_pass_probabilities，并保留旧键作兼容别名。
        threshold = self.scoring_fn.max_score * 0.6

        for i in range(len(prices)):
            scores_i = all_scores[i]  # (n_simulations,)
            expected_scores.append(float(np.mean(scores_i)))
            score_p5s.append(float(np.percentile(scores_i, 5)))
            score_p50s.append(float(np.percentile(scores_i, 50)))
            score_p95s.append(float(np.percentile(scores_i, 95)))
            win_probs.append(float(np.mean(scores_i >= threshold)))

        expected_scores = np.array(expected_scores)
        best_idx = int(np.argmax(expected_scores))

        return {
            'prices': [float(p) for p in prices],
            'expected_scores': [round(s, 2) for s in expected_scores],
            'score_p5': [round(s, 2) for s in score_p5s],
            'score_p50': [round(s, 2) for s in score_p50s],
            'score_p95': [round(s, 2) for s in score_p95s],
            # B7：得分达标概率（≥满分60%），非中标概率；旧名保留兼容
            'score_pass_probabilities': [round(p, 4) for p in win_probs],
            'win_probabilities': [round(p, 4) for p in win_probs],
            'best_price_index': best_idx,
            'n_simulations': self.n_simulations,
        }

    def optimize(self, price_range: Tuple[float, float],
                 n_competitors: int = 5,
                 strategy: str = 'max_expected_score',
                 n_grid: int = 50) -> Dict:
        """求最优报价

        strategy:
            'max_expected_score' — 最大化期望得分
            'max_win_prob' — 最大化胜率
            'max_utility' — 最大化期望效用 = E[score] * P(win)
        """
        rng = np.random.default_rng(42)  # 可复现

        # 1. 粗粒度网格搜索
        lo, hi = price_range
        grid = np.linspace(lo, hi, n_grid)

        result = self.simulate(list(grid), n_competitors, rng)

        if strategy == 'max_win_prob':
            objective = np.array(result['win_probabilities'])
        elif strategy == 'max_utility':
            objective = (np.array(result['expected_scores']) *
                         np.array(result['win_probabilities']))
        else:
            objective = np.array(result['expected_scores'])

        best_idx = int(np.argmax(objective))
        best_price = float(grid[best_idx])

        # 2. 黄金分割细化
        delta = (hi - lo) / n_grid * 2
        lo2 = max(lo, best_price - delta)
        hi2 = min(hi, best_price + delta)

        def neg_objective(p):
            r = self.simulate([float(p)], n_competitors, rng)
            if strategy == 'max_win_prob':
                return -r['win_probabilities'][0]
            elif strategy == 'max_utility':
                return -(r['expected_scores'][0] * r['win_probabilities'][0])
            else:
                return -r['expected_scores'][0]

        try:
            res = minimize_scalar(neg_objective, bounds=(lo2, hi2), method='bounded')
            if res.success and lo <= res.x <= hi:
                best_price = float(res.x)
                # 重新计算最终结果
                final = self.simulate([best_price], n_competitors, rng)
                best_expected = final['expected_scores'][0]
                best_p5 = final['score_p5'][0]
                best_p50 = final['score_p50'][0]
                best_p95 = final['score_p95'][0]
                best_win = final['win_probabilities'][0]
            else:
                best_expected = result['expected_scores'][best_idx]
                best_p5 = result['score_p5'][best_idx]
                best_p50 = result['score_p50'][best_idx]
                best_p95 = result['score_p95'][best_idx]
                best_win = result['win_probabilities'][best_idx]
        except Exception:
            best_expected = result['expected_scores'][best_idx]
            best_p5 = result['score_p5'][best_idx]
            best_p50 = result['score_p50'][best_idx]
            best_p95 = result['score_p95'][best_idx]
            best_win = result['win_probabilities'][best_idx]

        # 3. 推荐报价区间（得分在最优值 95% 以上的价格范围）
        good_mask = np.array(result['expected_scores']) >= best_expected * 0.95
        good_prices = grid[good_mask]
        if len(good_prices) >= 2:
            opt_range = [float(good_prices[0]), float(good_prices[-1])]
        else:
            opt_range = [best_price * 0.98, best_price * 1.02]

        # 4. 生成可解释理由
        ratio = best_price / self.benchmark_estimator.control_price
        reasoning = self._build_reasoning(
            best_price, ratio, best_expected, best_win, n_competitors, strategy
        )

        return {
            'optimal_price': round(best_price, 2),
            'optimal_price_range': [round(opt_range[0], 2), round(opt_range[1], 2)],
            'expected_score': round(best_expected, 2),
            'score_distribution': {
                'p5': round(best_p5, 2),
                'p50': round(best_p50, 2),
                'p95': round(best_p95, 2),
            },
            # B7：得分达标概率（≥满分60%），非中标概率
            'win_probability': round(best_win, 4),
            'score_pass_probability': round(best_win, 4),
            'bid_ratio': round(ratio, 4),
            'strategy': strategy,
            'reasoning': reasoning,
            'n_simulations': self.n_simulations,
            'grid_result': {
                'prices': [round(p, 0) for p in result['prices']],
                'expected_scores': result['expected_scores'],
                'win_probabilities': result['win_probabilities'],
            },
        }

    def _build_reasoning(self, price, ratio, score, win_prob, n_comp, strategy):
        """生成可解释的推荐理由"""
        risk_pct = (1 - ratio) * 100
        lines = [
            f"推荐报价 {price:,.0f} 元（控制价的 {ratio:.2%}，下浮 {risk_pct:.1f}%）",
            f"基于 {self.n_simulations} 次蒙特卡洛模拟，模拟 {n_comp} 家竞争对手参与",
            f"预期报价得分 {score:.1f} 分（P5-P95: 见分布）",
            f"得分达标概率（≥满分60%，非中标概率）: {win_prob:.1%}",
        ]
        if ratio > 0.97:
            lines.append("⚠️ 报价接近控制价上限，利润空间大但竞争力低")
        elif ratio < 0.80:
            lines.append("⚠️ 报价下浮较大，竞争力强但需确认利润率")
        else:
            lines.append("✅ 报价在合理竞争区间，平衡了得分和竞争力")
        return '；'.join(lines)


# ============================================================
# 5a. ARX 时序模型（移植自自进化实验 src/bidding/arx_model.py）
# ============================================================

class ARXModel:
    """带外生变量的自回归模型 ARX(p, m)

    y(t) = a1*y(t-1) + ... + ap*y(t-p) + b1*x1(t) + ... + bm*xk(t) + e(t)

    用于报价比率时序预测。外生变量可选：竞争者数、预算规模分档、季节因子。
    OLS 求解 + Tikhonov 正则化防止过拟合。
    """

    def __init__(self, order: int = 3, n_exog: int = 0):
        self.order = order
        self.n_exog = n_exog
        self.coeffs_ar: Optional[np.ndarray] = None
        self.coeffs_exog: Optional[np.ndarray] = None
        self.intercept = 0.0
        self.residual_std = 0.0
        self.n_params = order + n_exog

    def fit(self, y: np.ndarray, X: Optional[np.ndarray] = None) -> Dict:
        """拟合 ARX 模型

        Returns: {'order', 'n_exog', 'residual_std', 'r2'}
        """
        p = self.order
        m = self.n_exog
        n = len(y)

        if X is None:
            X = np.zeros((n, 0))
        elif X.shape[0] != n:
            return {'error': '外生变量行数不匹配', 'residual_std': 0, 'r2': 0}

        if n < p + m + 3:
            return {'error': '数据不足', 'residual_std': 0, 'r2': 0}

        n_rows = n - p
        n_cols = p + m
        M = np.zeros((n_rows, n_cols))
        target = np.zeros(n_rows)

        for i in range(p, n):
            row = i - p
            M[row, :p] = y[i - p:i][::-1]
            if m > 0:
                M[row, p:] = X[i]
            target[row] = y[i]

        MtM = M.T @ M + 1e-6 * np.eye(n_cols)
        Mty = M.T @ target
        try:
            beta = np.linalg.solve(MtM, Mty)
        except np.linalg.LinAlgError:
            beta = np.linalg.lstsq(M, target, rcond=None)[0]

        self.coeffs_ar = beta[:p]
        self.coeffs_exog = beta[p:] if m > 0 else np.zeros(0)

        y_pred = M @ beta
        residuals = target - y_pred
        self.residual_std = float(np.std(residuals))
        # 防止完美拟合时预测区间坍缩
        self.residual_std = max(self.residual_std, 0.01)
        ss_res = np.sum(residuals ** 2)
        ss_tot = np.sum((target - np.mean(target)) ** 2)
        r2 = 1 - ss_res / ss_tot if ss_tot > 0 else 0

        return {'order': p, 'n_exog': m, 'residual_std': self.residual_std, 'r2': float(r2)}

    def predict(self, y_history: np.ndarray, X_future: Optional[np.ndarray] = None,
                steps: int = 1) -> np.ndarray:
        """向前预测 steps 步"""
        if self.coeffs_ar is None:
            raise ValueError('模型未训练')

        p = self.order
        m = self.n_exog
        if X_future is None:
            X_future = np.zeros((steps, m))

        x_buf = list(y_history[-p:])
        predictions = []
        for t in range(steps):
            ar_val = np.dot(self.coeffs_ar, np.array(x_buf[-p:][::-1]))
            exog_val = np.dot(self.coeffs_exog, X_future[t]) if m > 0 else 0
            val = self.intercept + ar_val + exog_val
            predictions.append(val)
            x_buf.append(val)
        return np.array(predictions)

    def predict_with_noise(self, y_history: np.ndarray, X_future: Optional[np.ndarray] = None,
                           steps: int = 1, noise_std: Optional[float] = None,
                           rng: Optional[np.random.Generator] = None) -> np.ndarray:
        """带噪声预测（用于集合预测）"""
        if noise_std is None:
            noise_std = self.residual_std
        if rng is None:
            rng = np.random.default_rng()

        p = self.order
        m = self.n_exog
        if X_future is None:
            X_future = np.zeros((steps, m))

        x_buf = list(y_history[-p:])
        predictions = []
        for t in range(steps):
            ar_val = np.dot(self.coeffs_ar, np.array(x_buf[-p:][::-1]))
            exog_val = np.dot(self.coeffs_exog, X_future[t]) if m > 0 else 0
            val = self.intercept + ar_val + exog_val + rng.normal(0, noise_std)
            predictions.append(val)
            x_buf.append(val)
        return np.array(predictions)


# ============================================================
# 5b. 预处理管线（移植自自进化实验 src/bidding/preprocess.py）
# ============================================================

class LogDiffTransform:
    """对数差分变换 — 去趋势转平稳

    y'(t) = log(y(t)) - log(y(t-1))
    反变换：y(t) = y(t-1) * exp(y'(t))
    """

    def transform(self, data: np.ndarray) -> Tuple[np.ndarray, float]:
        if np.any(data <= 0):
            data = data + (1.0 - np.min(data))
        log_p = np.log(data)
        diff = np.diff(log_p)
        return diff, float(data[-1])

    def inverse(self, diffs: np.ndarray, last_anchor: float,
                steps: Optional[int] = None) -> np.ndarray:
        n = steps if steps is not None else len(diffs) + 1
        out = np.zeros(n)
        out[0] = last_anchor
        cur = last_anchor
        for i in range(min(len(diffs), n - 1)):
            cur = cur * np.exp(diffs[i])
            out[i + 1] = cur
        if n > len(diffs) + 1:
            out[len(diffs) + 1:] = cur
        return out


class PreprocessPipeline:
    """预处理管线 — 对数差分转平稳"""

    def __init__(self, use_log_diff: bool = True):
        self.use_log_diff = use_log_diff
        self._log_diff = LogDiffTransform()
        self._last_anchor: float = 0.0

    def transform(self, data: np.ndarray) -> np.ndarray:
        out = np.asarray(data, dtype=np.float64)
        if self.use_log_diff:
            out, anchor = self._log_diff.transform(out)
            self._last_anchor = anchor
        else:
            self._last_anchor = float(out[-1]) if len(out) > 0 else 0.0
        return out

    def inverse(self, transformed: np.ndarray, steps: Optional[int] = None) -> np.ndarray:
        if self.use_log_diff:
            return self._log_diff.inverse(transformed, self._last_anchor, steps)
        else:
            n = steps if steps is not None else len(transformed)
            out = np.zeros(n)
            out[:len(transformed)] = transformed[:n]
            if n > len(transformed):
                out[len(transformed):] = self._last_anchor
            return out


# ============================================================
# 5c. 集合预测器（移植自自进化实验 src/bidding/ensemble.py）
# ============================================================

class EnsembleForecaster:
    """200成员带噪声ARX集合预测

    输出1/5/50/95/99分位线。投标违约损失大，用1/99保守分位。
    """

    def __init__(self, arx_model: ARXModel, n_members: int = 200):
        self.arx = arx_model
        self.n_members = n_members

    def forecast(self, y_history: np.ndarray,
                 X_future: Optional[np.ndarray] = None,
                 steps: int = 1,
                 rng: Optional[np.random.Generator] = None) -> Dict:
        if rng is None:
            rng = np.random.default_rng()

        members = np.zeros((self.n_members, steps))
        for i in range(self.n_members):
            members[i] = self.arx.predict_with_noise(
                y_history, X_future, steps,
                noise_std=self.arx.residual_std, rng=rng,
            )

        mean = members.mean(axis=0)
        std = members.std(axis=0)

        return {
            'mean': mean,
            'lower_1': np.percentile(members, 1, axis=0),
            'lower_5': np.percentile(members, 5, axis=0),
            'median': np.percentile(members, 50, axis=0),
            'upper_95': np.percentile(members, 95, axis=0),
            'upper_99': np.percentile(members, 99, axis=0),
            'std': std,
            'members': members,
        }


# ============================================================
# 5d. 同类近期均价基线（移植自自进化实验 src/bidding/baseline.py）
# ============================================================

class RecentMeanBaseline:
    """同类近期均价基线 — 替代持久性预报"""

    def __init__(self, window: int = 10):
        self.window = window

    def predict(self, history: np.ndarray, steps: int = 1) -> np.ndarray:
        if len(history) == 0:
            return np.full(steps, 0.92)
        recent = history[-self.window:]
        return np.full(steps, float(np.mean(recent)))


# ============================================================
# 5e. 趋势分析器（重构：ARX + 集合预测 + 基线评估）
# ============================================================

class TrendAnalyzer:
    """报价趋势分析器 — ARX + 集合预测

    借鉴自进化实验的5层正交架构：
      Layer 1: ARX(p) 自回归（含可选外生变量）
      Layer 2: 对数差分预处理（去趋势转平稳）
      Layer 3: 200成员集合预测（1/99分位线）
      Layer 4: 同类近期均价基线 + skill score 评估
    """

    def __init__(self, timeline: List[Dict], exog: Optional[np.ndarray] = None):
        """初始化

        Args:
            timeline: BidHistory.get_bid_timeline() 返回的月度数据
            exog: 外生变量矩阵，形状 (n_months, n_exog)；如竞争者数
        """
        self.timeline = timeline
        self.exog = exog
        self._months: List[str] = []
        self._ratios: np.ndarray = np.array([])
        self._counts: np.ndarray = np.array([])

        if len(timeline) >= 3:
            self._months = [t['month'] for t in timeline]
            self._ratios = np.array([t['avg_ratio'] for t in timeline])
            self._counts = np.array([t['count'] for t in timeline])

    def analyze_trend(self) -> Dict:
        """分析趋势 — ARX 集合预测"""
        if len(self._ratios) < 5:
            return self._fallback_trend()

        n = len(self._ratios)
        n_exog = self.exog.shape[1] if self.exog is not None else 0

        # Layer 2: 预处理（对数差分）
        pipeline = PreprocessPipeline(use_log_diff=True)
        y_stationary = pipeline.transform(self._ratios)

        # 同步外生变量（对数差分后长度-1）
        X = None
        if self.exog is not None and n_exog > 0:
            X = self.exog[1:]  # 对数差分丢掉第一行，外生变量同步

        # Layer 1: ARX 拟合
        ar_order = min(3, max(1, n - 5))
        arx = ARXModel(order=ar_order, n_exog=n_exog)
        fit_result = arx.fit(y_stationary, X)

        if 'error' in fit_result:
            return self._fallback_trend()

        # Layer 3: 集合预测（200成员）
        ensemble = EnsembleForecaster(arx, n_members=200)
        rng = np.random.default_rng(42)
        ens = ensemble.forecast(y_stationary, None, steps=3, rng=rng)

        # 反预处理：平稳空间 → 原始比率
        predicted_mean = pipeline.inverse(ens['mean'], steps=3)
        predicted_l1 = pipeline.inverse(ens['lower_1'], steps=3)
        predicted_l5 = pipeline.inverse(ens['lower_5'], steps=3)
        predicted_u95 = pipeline.inverse(ens['upper_95'], steps=3)
        predicted_u99 = pipeline.inverse(ens['upper_99'], steps=3)

        # 如果集合预测分位数过于接近（逆变换压缩），用原始空间残差扩展
        # 注意: predicted_*[0] 是锚点(当前值)，[1] 是第一个预测步
        raw_mean = float(predicted_mean[1])
        raw_spread = max(float(predicted_u99[1]) - float(predicted_l1[1]), 0.001)
        if raw_spread < 0.02:
            # 原始比率空间的残差标准差作为最小扩散
            raw_residuals = self._ratios[1:] - self._ratios[:-1]  # 简单差分
            raw_std = max(float(np.std(raw_residuals)), 0.02)
            from scipy.stats import norm as _norm
            predicted_l1[1] = max(0.5, raw_mean + _norm.ppf(0.01) * raw_std)
            predicted_l5[1] = max(0.5, raw_mean + _norm.ppf(0.05) * raw_std)
            predicted_u95[1] = min(1.0, raw_mean + _norm.ppf(0.95) * raw_std)
            predicted_u99[1] = min(1.0, raw_mean + _norm.ppf(0.99) * raw_std)

        # Layer 4: 基线对比
        baseline = RecentMeanBaseline(window=min(6, n))
        baseline_pred = baseline.predict(self._ratios, steps=1)

        # 趋势方向：优先用集合均值变化方向，但小数据回退到线性斜率
        if n >= 8:
            # 数据充足时用 ARX 集合预测方向
            if predicted_mean[1] > predicted_mean[0] + 0.001:
                direction = 'rising'
            elif predicted_mean[1] < predicted_mean[0] - 0.001:
                direction = 'falling'
            else:
                direction = 'stable'
        else:
            # 数据少时用线性回归斜率
            x = np.arange(n, dtype=np.float64)
            slope_lr = np.polyfit(x, self._ratios, 1)[0]
            if abs(slope_lr) < 0.001:
                direction = 'stable'
            elif slope_lr > 0:
                direction = 'rising'
            else:
                direction = 'falling'

        # ARX R² 作为置信度
        r2 = fit_result.get('r2', 0)
        confidence = max(0.0, min(1.0, r2))

        # 季节性
        seasonality = self._calc_seasonality()

        # 数据点（含 ARX 拟合线）
        data_points = []
        # 线性回归趋势线（作为ARX拟合线的fallback/补充）
        x_lr = np.arange(n, dtype=np.float64)
        lr_slope, lr_intercept = np.polyfit(x_lr, self._ratios, 1) if n >= 2 else (0.0, float(self._ratios[-1]) if n > 0 else 0.92)
        for i, t in enumerate(self.timeline):
            dp = {
                'month': t['month'],
                'ratio': round(t['avg_ratio'], 4),
                'count': t['count'],
                'trend_line': round(float(lr_slope * i + lr_intercept), 4),
            }
            data_points.append(dp)

        return {
            'direction': direction,
            'slope': round(float(predicted_mean[1] - predicted_mean[0]), 6),
            'slope_per_month_pct': round(float((predicted_mean[1] - predicted_mean[0]) * 100), 3),
            'current_avg_ratio': round(float(self._ratios[-1]), 4),
            'predicted_ratio': {
                'p1': round(max(0.5, float(predicted_l1[1])), 4),
                'p5': round(max(0.5, float(predicted_l5[1])), 4),
                'p10': round(max(0.5, float(predicted_l5[1])), 4),
                'p50': round(float(predicted_mean[1]), 4),
                'p90': round(min(1.0, float(predicted_u95[1])), 4),
                'p99': round(min(1.0, float(predicted_u99[1])), 4),
            },
            'baseline_ratio': round(float(baseline_pred[0]), 4),
            'arx_r2': round(float(r2), 4),
            'arx_residual_std': round(float(fit_result.get('residual_std', 0)), 4),
            'arx_order': ar_order,
            'n_exog': n_exog,
            'ensemble_members': 200,
            'seasonality': seasonality,
            'data_points': data_points,
            'r_squared': round(float(r2), 4),
            'confidence': round(float(confidence), 2),
            'model': 'ARX+Ensemble',
        }

    def _calc_seasonality(self) -> Dict:
        """计算季节性因子"""
        if len(self._months) < 12:
            return {'available': False, 'reason': '数据不足12个月'}

        month_avg = {}
        for t in self.timeline:
            m = t['month'][5:7]
            month_avg.setdefault(m, []).append(t['avg_ratio'])

        overall_mean = float(np.mean(self._ratios))
        factors = {}
        for m, vals in sorted(month_avg.items()):
            m_mean = float(np.mean(vals))
            factors[m] = round(m_mean - overall_mean, 4)

        return {
            'available': True,
            'monthly_factors': factors,
            'strongest_month': max(factors, key=lambda k: abs(factors[k])),
        }

    def _fallback_trend(self) -> Dict:
        """数据不足时的默认趋势"""
        current = float(self._ratios[-1]) if len(self._ratios) > 0 else 0.92
        n = len(self._ratios)
        # 线性回归趋势线（即使数据少也提供）
        if n >= 2:
            x_lr = np.arange(n, dtype=np.float64)
            lr_slope, lr_intercept = np.polyfit(x_lr, self._ratios, 1)
        else:
            lr_slope, lr_intercept = 0.0, current
        data_points = []
        for i, t in enumerate(self.timeline):
            data_points.append({
                'month': t['month'],
                'ratio': round(t['avg_ratio'], 4),
                'count': t['count'],
                'trend_line': round(float(lr_slope * i + lr_intercept), 4),
            })

        # 判断趋势方向（用线性斜率）
        if abs(lr_slope) < 0.001:
            direction = 'stable'
        elif lr_slope > 0:
            direction = 'rising'
        else:
            direction = 'falling'

        # 预测区间（用线性回归 + 残差估计）
        from scipy.stats import norm
        residuals = self._ratios - (lr_slope * np.arange(n) + lr_intercept)
        res_std = max(float(np.std(residuals, ddof=2)) if n > 2 else 0.04, 0.02)
        next_x = n
        p50 = lr_slope * next_x + lr_intercept
        p10 = p50 + norm.ppf(0.10) * res_std
        p90 = p50 + norm.ppf(0.90) * res_std

        return {
            'direction': direction,
            'slope': round(float(lr_slope), 6),
            'slope_per_month_pct': round(float(lr_slope * 100), 3),
            'current_avg_ratio': round(current, 4),
            'predicted_ratio': {
                'p1': round(max(0.5, p50 + norm.ppf(0.01) * res_std), 4),
                'p5': round(max(0.5, p50 + norm.ppf(0.05) * res_std), 4),
                'p10': round(max(0.5, float(p10)), 4),
                'p50': round(float(p50), 4),
                'p90': round(min(1.0, float(p90)), 4),
                'p99': round(min(1.0, p50 + norm.ppf(0.99) * res_std), 4),
            },
            'baseline_ratio': round(current, 4),
            'arx_r2': 0.0,
            'arx_residual_std': round(res_std, 4),
            'arx_order': 0,
            'n_exog': 0,
            'ensemble_members': 0,
            'seasonality': {'available': False, 'reason': '数据不足'},
            'data_points': data_points,
            'r_squared': 0.0,
            'confidence': 0.0,
            'model': 'linear_fallback',
            'trend_line_intercept': round(float(lr_intercept), 4),
        }


# ============================================================
# 6. 报价优化器（编排层）
# ============================================================

class PricingOptimizer:
    """报价优化器 — 编排全部模型

    从报价规则（extract_price_calculation_rules 输出）出发，
    自动构建评分函数、竞争对手模型、基准价估计器、蒙特卡洛引擎，
    输出完整分析结果。
    """

    def __init__(self, control_price: float,
                 scoring_fn: ScoringFunction,
                 benchmark_method: str = 'composite',
                 formula_params: Optional[Dict] = None,
                 competitor_model: Optional[CompetitorModel] = None,
                 region: Optional[str] = None,
                 project_type: Optional[str] = None):
        self.control_price = control_price
        self.scoring_fn = scoring_fn
        self.benchmark_method = benchmark_method
        self.formula_params = formula_params or {}
        self.project_type = project_type

        # 竞争对手模型（按工程类型从历史数据构建）
        if competitor_model is None:
            from services.bid_history import BidHistory
            if project_type:
                ratios = BidHistory.get_bid_ratios_by_region_type(
                    region or '合肥市', project_type)
            else:
                ratios = BidHistory.get_bid_ratios(region=region)
            competitor_model = CompetitorModel(ratios)
        self.competitor_model = competitor_model

        # 基准价估计器
        self.benchmark_estimator = BenchmarkEstimator(
            control_price=control_price,
            method=benchmark_method,
            competitor_model=self.competitor_model,
            formula_params=self.formula_params,
        )

        # 蒙特卡洛引擎
        self.mc_engine = MonteCarloEngine(
            scoring_fn=self.scoring_fn,
            benchmark_estimator=self.benchmark_estimator,
        )

    @classmethod
    def from_price_rules(cls, price_rules: Dict,
                         region: Optional[str] = None,
                         project_type: Optional[str] = None) -> 'PricingOptimizer':
        """从 extract_price_calculation_rules 的输出构建"""
        control_price = price_rules.get('control_price') or 0
        if control_price <= 0:
            # 尝试从文本提取
            import re
            text = price_rules.get('control_price_text', '')
            m = re.search(r'(\d+(?:\.\d+)?)\s*[万亿元]', text)
            if m:
                num = float(m.group(1))
                if '亿' in text:
                    num *= 1e8
                elif '万' in text:
                    num *= 1e4
                control_price = num

        scoring_fn = ScoringFunction.from_deduction_parsed(
            price_rules.get('deduction_rule_parsed', {})
        )
        if price_rules.get('max_price_score'):
            scoring_fn.max_score = price_rules['max_price_score']

        # 映射评标方法
        method_map = {
            '算术平均值法': 'arithmetic_mean',
            '复合基准价法': 'composite',
            '合理低价法': 'lowest_price',
            '综合评估法': 'composite',
            '未识别': 'composite',
        }
        raw_method = price_rules.get('benchmark_method', '未识别')
        benchmark_method = method_map.get(raw_method, 'composite')

        formula_params = price_rules.get('benchmark_formula_parsed', {}).get('parameters', {})

        return cls(
            control_price=control_price,
            scoring_fn=scoring_fn,
            benchmark_method=benchmark_method,
            formula_params=formula_params,
            region=region,
            project_type=project_type,
        )

    def analyze(self, n_competitors: int = 5,
                n_simulations: int = 10000,
                strategy: str = 'max_expected_score') -> Dict:
        """完整报价分析

        Returns:
            {
                'control_price': float,
                'optimal_price': float,
                'optimal_price_range': [lo, hi],
                'expected_score': float,
                'score_distribution': {'p5','p50','p95'},
                'win_probability': float,
                'bid_ratio': float,
                'reasoning': str,
                'scenarios': [...],
                'competitor_model': {...},
                'benchmark_estimation': {...},
                'n_simulations': int,
            }
        """
        if self.control_price <= 0:
            return {
                'control_price': 0,
                'optimal_price': None,
                'error': '未能提取控制价，无法进行报价分析',
            }

        # 用调用方指定的 n_simulations 重建蒙特卡洛引擎
        self.mc_engine = MonteCarloEngine(
            scoring_fn=self.scoring_fn,
            benchmark_estimator=self.benchmark_estimator,
            n_simulations=n_simulations,
        )

        # 1. 最优报价
        price_range = (self.control_price * 0.75, self.control_price * 0.99)
        optimal = self.mc_engine.optimize(
            price_range, n_competitors, strategy
        )

        # 2. 多场景对比
        scenarios = self._build_scenarios(n_competitors)

        # 3. 基准价估计
        expected_bench = self.benchmark_estimator.expected_benchmark(n_competitors)
        bench_lo, bench_hi = self.benchmark_estimator.benchmark_interval(n_competitors)

        # 4. 风险等级量化
        risk_level = self._calc_risk_level(optimal['bid_ratio'], optimal['win_probability'])

        return {
            'control_price': round(self.control_price, 2),
            'control_price_text': f'{self.control_price:,.0f} 元',
            'optimal_price': optimal['optimal_price'],
            'optimal_price_text': f'{optimal["optimal_price"]:,.0f} 元',
            'optimal_price_range': optimal['optimal_price_range'],
            'optimal_price_range_text': f'{optimal["optimal_price_range"][0]:,.0f} ~ {optimal["optimal_price_range"][1]:,.0f} 元',
            'expected_score': optimal['expected_score'],
            'score_distribution': optimal['score_distribution'],
            'win_probability': optimal['win_probability'],
            'bid_ratio': optimal['bid_ratio'],
            'risk_level': risk_level,
            'reasoning': optimal['reasoning'],
            'scenarios': scenarios,
            'competitor_model': self.competitor_model.to_dict(),
            'benchmark_estimation': {
                'method': self.benchmark_method,
                'expected_benchmark': round(expected_bench, 2),
                'benchmark_interval': [round(bench_lo, 2), round(bench_hi, 2)],
                'formula_params': self.formula_params,
            },
            'scoring_function': self.scoring_fn.to_dict(),
            'n_simulations': n_simulations,
            'n_competitors': n_competitors,
            'strategy': strategy,
            'grid_result': optimal.get('grid_result'),
        }

    def _build_scenarios(self, n_competitors: int) -> List[Dict]:
        """构建多场景对比表"""
        deviations = [-5, -4, -3, -2, -1, 0, 1, 2]
        candidate_prices = [self.control_price * (1 + d / 100) for d in deviations]

        result = self.mc_engine.simulate(candidate_prices, n_competitors)

        scenarios = []
        for i, (dev, price) in enumerate(zip(deviations, candidate_prices)):
            scenarios.append({
                'deviation': f'{dev:+d}%',
                'bid_price': round(price, 2),
                'bid_price_text': f'{price:,.0f} 元',
                'expected_score': result['expected_scores'][i],
                'score_p5': result['score_p5'][i],
                'score_p50': result['score_p50'][i],
                'score_p95': result['score_p95'][i],
                'win_probability': result['win_probabilities'][i],
            })
        return scenarios

    def _calc_risk_level(self, ratio: float, win_prob: float) -> Dict:
        """量化风险等级"""
        if win_prob >= 0.7:
            level = 'low'
            label = '低风险'
        elif win_prob >= 0.4:
            level = 'medium'
            label = '中等风险'
        else:
            level = 'high'
            label = '高风险'

        var_5pct = self.scoring_fn.max_score * 0.05  # VaR 简化

        return {
            'level': level,
            'label': label,
            'win_probability': round(win_prob, 4),
            'var_5pct': round(var_5pct, 2),
            'description': f'胜率 {win_prob:.1%}，5% 概率得分低于 {var_5pct:.1f}',
        }
