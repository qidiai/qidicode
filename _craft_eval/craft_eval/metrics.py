# -*- coding: utf-8 -*-
"""metrics.py — 判据 (T1 规格书 §4).

本模块是"规整规则"的唯一实现 (norm)，truth.py 引用它，避免两份实现漂移。
"""
import re

# U+FFFD REPLACEMENT CHARACTER
FFFD = "\ufffd"

# 空白规整：re.sub(r'\s','',raw)。
# Python3 str 模式下 \s 默认匹配 Unicode 空白，已覆盖全角空格 U+3000(IDEOGRAPHIC SPACE)。
_WS_RE = re.compile(r"\s")

# CJK 字符区间（真值漂移对照用；仅统计，不做放行判据）：
#   3400-4DBF  扩展 A
#   4E00-9FFF  基本区
#   F900-FAFF  兼容表意
#   20000-2A6DF SIP 扩展 B ..
_CJK_RE = re.compile(
    r"[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff"
    r"\U00020000-\U0002FFFF]"
)


def norm(text):
    """规整规则唯一实现：去除全部空白字符后返回。

    规格书 §4: `norm(text) -> str`（规整规则唯一实现，truth.py 引用它）。
    """
    if not text:
        return ""
    return _WS_RE.sub("", text)


def u_fffd_count(text):
    """U+FFFD 替换字符计数（放行要求 ==0）。规格书 §4。"""
    if not text:
        return 0
    return text.count(FFFD)


def completeness(craft_text, truth_norm_chars):
    """完整率 = norm(craft) 字符数 / truth_norm_chars。

    规格书 §4: `completeness(craft_text, truth_norm_chars) -> {"ratio","craft_norm"}`
    （norm(craft)/truth，>1.0 记录原值）。
    T2 Part C: truth==0 且 craft 空 → ratio=None（空对空 N/A）；truth==0 且 craft 非空 → inf。
    """
    craft_norm = len(norm(craft_text or ""))
    if truth_norm_chars:
        ratio = craft_norm / truth_norm_chars
    else:
        # truth 为空：T2 Part C 空对空 → None（N/A，不计入均值）；craft 非空 → inf
        ratio = None if craft_norm == 0 else float("inf")
    return {"ratio": ratio, "craft_norm": craft_norm}


def cjk_ratio(text):
    """CJK 字符占比（CJK 字符数 / 总字符数）。规格书 §4。"""
    if not text:
        return 0.0
    total = len(text)
    if total == 0:
        return 0.0
    return len(_CJK_RE.findall(text)) / total
