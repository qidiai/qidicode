# -*- coding: utf-8 -*-
"""
Responses API shim for qidi web_search.  (v2, 2026-08-26)
接收 qidi.exe 的 POST /v1/responses（Responses API 格式），
转发到 8765 代理的 /v1/chat/completions（chat 格式），
再把响应转回标准 Responses API 格式（usage 用 input_tokens/output_tokens）。

功能：
1. 请求/响应完整日志（shim.log），用于逆向 qidi 的实际行为
2. usage 字段映射：prompt_tokens -> input_tokens, completion_tokens -> output_tokens
3. 若请求带 tools 且含 web_search 类型 -> 先抓取搜索结果注入上下文（真实联网搜索）
4. 非 JSON 响应、错误响应的兜底处理

v2 变更（2026-08-26）：
- 三引擎并发竞速：360/搜狗/必应同时发起，取最快命中者；
  若更偏好的引擎在 PREF_GRACE 秒宽限内返回则替换（中文偏好 360，英文偏好必应）。
  最坏延迟从 3×15s 降为约 15s，常见延迟为最快引擎延迟。
- count 参数透传：读取 web_search 工具定义里的 count/max_results
  （qidi.exe 实际载荷形如 {"type":"web_search","filters":{}}，
  兼容 filters 内字段与 OpenAI 风格 search_context_size: low/medium->3/5/8）。
- 引用锚点精确化：解析输出正文中的 [n] 标注，把 url_citation 的
  start_index/end_index 定位到正文首次出现位置（此前恒为 0）；
  模型未标注时退化为全量引用列表。
- 120 秒查询缓存：同一 (query,count) 短时间内直接复用抓取结果。

v2.2 变更（2026-08-27）：
- 日志轮转：RotatingFileHandler（10MB × 5 备份），shim.log 不再无限增长。
- 迁移至 .qidi/services/websearch-shim/（生产位，随 git 追踪）。

v2.1 变更（2026-08-26 深度审计后）：
- 竞速循环修复：已有结果且剩余引擎都不比当前更偏好时立即结束，不再空等宽限期。
- /health 增加 cache_hits / searches 观测字段；版本号 bump 以便日志追溯。
"""
import json
import os
import re
import time
import uuid
import logging
import threading
import concurrent.futures
from datetime import datetime
from logging.handlers import RotatingFileHandler

import httpx
from flask import Flask, request, jsonify, Response

UPSTREAM = "http://127.0.0.1:8765/v1/chat/completions"
SHIM_MODEL = "deepseek-v4-flash"
SHIM_VERSION = "2.3"
PORT = 8766

# 竞速宽限期：首个结果到手后，再等更偏好引擎这么久（秒）
PREF_GRACE = 2.0
# 单引擎整体兜底超时（与 httpx 请求超时一致）
ENGINE_TIMEOUT = 15
# 查询缓存 TTL（秒）
CACHE_TTL = 120

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(message)s",
    handlers=[RotatingFileHandler(
        "shim.log", maxBytes=10 * 1024 * 1024, backupCount=5, encoding="utf-8")],
)
log = logging.getLogger("shim")

app = Flask(__name__)


# ---------------- 搜索抓取（必应/搜狗/360，无 key） ----------------

def _strip_tags(html: str) -> str:
    html = re.sub(r"<script[\s\S]*?</script>", "", html)
    html = re.sub(r"<style[\s\S]*?</style>", "", html)
    html = re.sub(r"<[^>]+>", " ", html)
    html = re.sub(r"&nbsp;|&#160;", " ", html)
    html = re.sub(r"&amp;", "&", html)
    html = re.sub(r"&#\d+;", " ", html)
    html = re.sub(r"\s+", " ", html)
    return html.strip()


def fetch_bing(query: str, count: int = 5):
    """抓取必应搜索结果页，返回 [(title, url, snippet)]。

    策略：抓前 count*4 条候选，按"标题/摘要与查询词的相关性"排序后取前 count 条，
    避免 b_algo 前几条被泛化推荐内容占据。
    """
    results = []
    try:
        r = httpx.get(
            "https://cn.bing.com/search",
            params={"q": query, "count": str(count * 4), "mkt": "zh-CN"},
            headers={"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36",
                     "Accept-Language": "zh-CN,zh;q=0.9"},
            timeout=15,
            follow_redirects=True,
        )
        if r.status_code != 200:
            log.warning("bing 搜索 HTTP %s", r.status_code)
            return results
        html = r.text
        # 每个 <li class="b_algo" ...> 块内：<h2><a href="URL">TITLE(含strong)</a></h2> ... <p>SNIPPET</p>
        items = html.split('<li class="b_algo"')[1:]
        # 黑名单域名：泛化推荐/低相关
        blacklist = ("baike.baidu.com", "map.baidu.com", "zhihu.com/topic",
                     "zhidao.baidu.com", "tieba.baidu.com")
        # 查询词分词（按空格拆 + 保留中文整串）
        terms = [q for q in query.split() if q] or [query]

        def score(entry):
            title, url_, snippet = entry
            text = title + " " + snippet
            s = 0
            for t in terms:
                if t in title:
                    s += 3
                if t in snippet:
                    s += 1
            if any(b in url_ for b in blacklist):
                s -= 10
            return s

        for it in items:
            m = re.search(r"<h2[^>]*>\s*<a[^>]*?href=[\"\x27]([^\"\x27]+)[\"\x27]", it)
            if not m:
                continue
            url_ = m.group(1)
            tm = re.search(r"<h2[^>]*>\s*<a[^>]*>([\s\S]*?)</a>", it)
            title = _strip_tags(tm.group(1)) if tm else ""
            sn = re.search(r"<p[^>]*>([\s\S]*?)</p>", it)
            snippet = _strip_tags(sn.group(1)) if sn else ""
            if title and url_.startswith("http"):
                results.append((title, url_, snippet))
        # 按相关性排序取前 count
        results.sort(key=score, reverse=True)
        return results[:count]
    except Exception as e:
        log.warning("bing 搜索异常: %s", e)
    return results


def fetch_baidu(query: str, count: int = 5):
    """抓取百度搜索结果页（备用，未进竞速链）"""
    results = []
    try:
        r = httpx.get(
            "https://www.baidu.com/s",
            params={"wd": query, "rn": str(count * 2)},
            headers={"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36"},
            timeout=15,
            follow_redirects=True,
        )
        if r.status_code != 200:
            return results
        html = r.text
        # 结果块: <h3 class="c-title ..."><a href="URL" ...>TITLE</a></h3>
        for m in re.finditer(r"<h3[^>]*>\s*<a[^>]+href=.([^\x22'\s]+).[^>]*>([\s\S]*?)</a>\s*</h3>", html):
            url_, title = m.group(1), _strip_tags(m.group(2))
            if title and url_.startswith("http"):
                # 取该块附近的摘要
                tail = html[m.end(): m.end() + 1200]
                sn = re.search(r"<span class=.content-right_[^>]*>([\s\S]*?)</span>", tail)
                snippet = _strip_tags(sn.group(1)) if sn else ""
                results.append((title, url_, snippet))
            if len(results) >= count:
                break
    except Exception as e:
        log.warning("baidu 搜索异常: %s", e)
    return results


def _resolve_sogou_link(href: str) -> str:
    """把 sogou 跳转链解析成真实 URL（HEAD 跟随重定向）"""
    if not href.startswith("https://www.sogou.com/link"):
        return href
    try:
        r = httpx.head(href, timeout=8, follow_redirects=True,
                       headers={"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/120.0"})
        if r.url and str(r.url) != href:
            return str(r.url)
    except Exception:
        pass
    return href


WEATHER_RE = re.compile(
    r"(\d{1,2}月\d{1,2}日)\s*\|\s*(周[一二三四五六日])\s*.*?(-?\d+)\s*℃\s*([^|]+?)\s*\|\s*(.+?)\s*(?:\|\s*(.+?)\s*)?(?:昨天|今日|今日|$|。)"
)


def extract_weather_card(html: str):
    """提取 360 搜索天气卡片。卡片由多个嵌套标签构成，先剥标签再用正则。"""
    # CSS 中也有 mh-time 字样；正文卡片用 class="mh-time" 属性形式定位
    anchor = html.find('class="mh-time"')
    if anchor < 0:
        return None
    seg = html[anchor: anchor + 8000]
    text = _strip_tags(seg)
    m = WEATHER_RE.search(text)
    if not m:
        return None
    date, weekday, temp, cond, wind, extra = m.groups()
    out = f"{date} {weekday} {temp}℃ {cond.strip()}"
    if wind:
        out += f"，{wind.strip()}"
    if extra:
        out += f"，{extra.strip()}"
    return out


def fetch_so360(query: str, count: int = 5):
    """抓取 360 搜索（so.com）结果页，返回 [(title, url, snippet)]。

    优势：中文召回好，data-mdurl 直接给出真实 URL，无需解跳转。
    """
    results = []
    try:
        r = httpx.get(
            "https://www.so.com/s",
            params={"q": query},
            headers={"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36",
                     "Accept-Language": "zh-CN,zh;q=0.9"},
            timeout=15,
            follow_redirects=True,
        )
        if r.status_code != 200:
            log.warning("so360 搜索 HTTP %s", r.status_code)
            return results
        html = r.text
        # 天气卡片：搜索词含"天气"时优先提取实时数据
        if "天气" in query:
            card = extract_weather_card(html)
            if card:
                results.append((f"{query} - 实时天气（360搜索卡片）", "https://www.so.com/s?q=" + query, card))
        blocks = html.split('<li class="res-list"')[1:]
        for b in blocks:
            m = re.search(r"<h3[^>]*>\s*<a[^>]*?href=[\"\x27]([^\"\x27]+)[\"\x27][^>]*>([\s\S]*?)</a>", b)
            if not m:
                continue
            title = _strip_tags(m.group(2))
            # 优先 data-mdurl 真实地址
            du = re.search(r"data-mdurl=[\"\x27]([^\"\x27]+)[\"\x27]", b)
            url_ = du.group(1) if du else m.group(1)
            sn = re.search(r"<p class=.res-desc[^>]*>([\s\S]*?)</p>", b)
            snippet = _strip_tags(sn.group(1)) if sn else ""
            if title and url_.startswith("http"):
                results.append((title, url_, snippet[:300]))
            if len(results) >= count:
                break
    except Exception as e:
        log.warning("so360 搜索异常: %s", e)
    return results


def fetch_sogou(query: str, count: int = 5):
    """抓取搜狗搜索结果页（中文召回好），返回 [(title, url, snippet)]"""
    results = []
    try:
        r = httpx.get(
            "https://www.sogou.com/web",
            params={"query": query, "num": str(count * 3)},
            headers={"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36",
                     "Accept-Language": "zh-CN,zh;q=0.9"},
            timeout=15,
            follow_redirects=True,
        )
        if r.status_code != 200:
            log.warning("sogou 搜索 HTTP %s", r.status_code)
            return results
        html = r.text
        if "antispider" in r.url.path or "验证" in html[:2000]:
            log.warning("sogou 触发反爬")
            return results
        # 结果块: <h3 class="vr-title"><a ... href="/link?url=..." ...>TITLE</a></h3>
        blocks = re.split(r"<div class=.vrwrap.>|<div class=.rb.>", html)[1:]
        for b in blocks:
            m = re.search(r"<h3[^>]*>\s*<a[^>]*?href=[\"\x27]([^\"\x27]+)[\"\x27][^>]*>([\s\S]*?)</a>", b)
            if not m:
                continue
            href, title = m.group(1), _strip_tags(m.group(2))
            if href.startswith("/link"):
                href = "https://www.sogou.com" + href
            sn = re.search(r"<p class=.star-wiki[^>]*>([\s\S]*?)</p>|<div class=.space-txt[^>]*>([\s\S]*?)</div>|<p[^>]*>([\s\S]*?)</p>", b)
            snippet = ""
            if sn:
                snippet = _strip_tags(sn.group(0))
            if title and href:
                href = _resolve_sogou_link(href)
                results.append((title, href, snippet[:300]))
            if len(results) >= count:
                break
    except Exception as e:
        log.warning("sogou 搜索异常: %s", e)
    return results


# ---------------- 竞速调度（v2） ----------------

ENGINES = {
    "so360": fetch_so360,
    "sogou": fetch_sogou,
    "bing": fetch_bing,
}
ENGINE_CN = ("so360", "sogou", "bing")   # 中文偏好顺序
ENGINE_EN = ("bing", "so360", "sogou")   # 英文偏好顺序


def do_web_search(query: str, count: int = 5):
    """三引擎并发竞速：取最快命中者；更偏好的引擎在宽限期内返回则替换。

    v1 为串行级联（最坏 3×15s）；v2 并发后最坏约 15s，常见为最快引擎的延迟。
    """
    t0 = time.perf_counter()
    has_cjk = any("\u4e00" <= ch <= "\u9fff" for ch in query)
    pref_names = ENGINE_CN if has_cjk else ENGINE_EN
    rank = {n: i for i, n in enumerate(pref_names)}

    results, used, best_rank = [], None, 99
    ex = concurrent.futures.ThreadPoolExecutor(max_workers=len(pref_names))
    try:
        futs = {ex.submit(ENGINES[n], query, count): n for n in pref_names}
        pending = set(futs)
        while pending:
            # 审计修复(v2.1)：已有结果且剩余引擎都不比当前更偏好时，立即结束，
            # 不再为永远不会被采用的引擎空等宽限期
            if results and not any(rank[futs[f]] < best_rank for f in pending):
                break
            # 已有结果时只等宽限期，否则等单引擎兜底超时（FIRST_COMPLETED 通常更快返回）
            timeout = PREF_GRACE if results else ENGINE_TIMEOUT
            done, pending = concurrent.futures.wait(
                pending, timeout=timeout,
                return_when=concurrent.futures.FIRST_COMPLETED)
            if not done:
                break  # 宽限期到：保留现有结果，不再等更慢的引擎
            for f in done:
                n = futs[f]
                try:
                    r = f.result() or []
                except Exception as e:
                    log.warning("engine %s 异常: %s", n, e)
                    r = []
                if r and (used is None or rank[n] < best_rank):
                    if used is not None:
                        log.info("引擎替换 %s -> %s（更偏好）", used, n)
                    results, used, best_rank = r, n, rank[n]
    finally:
        # 不等待未完成的引擎线程（cancel 未启动的任务）
        ex.shutdown(wait=False, cancel_futures=True)

    log.info("搜索 %r 命中 %d 条 (engine=%s), 耗时 %.1fs",
             query, len(results), used or "-", time.perf_counter() - t0)
    _STATS["searches"] += 1
    return results


# ---------------- 查询缓存（v2） ----------------

_CACHE = {}          # key -> (hits, expire_at, results)
_CACHE_LOCK = threading.Lock()
_STATS = {"cache_hits": 0, "searches": 0}   # 审计修复(v2.1)：暴露到 /health 便于观测


def _cache_get(query: str, count: int):
    key = (query.strip().lower(), count)
    with _CACHE_LOCK:
        item = _CACHE.get(key)
        if not item:
            return None
        hits, exp, val = item
        if time.time() > exp:
            _CACHE.pop(key, None)
            return None
        _CACHE[key] = (hits + 1, exp, val)
        _STATS["cache_hits"] += 1
        return val


def _cache_put(query: str, count: int, results):
    key = (query.strip().lower(), count)
    with _CACHE_LOCK:
        # 容量控制：超过 256 条时清掉最早过期的一半
        if len(_CACHE) > 256:
            for k in sorted(_CACHE, key=lambda k: _CACHE[k][1])[:128]:
                _CACHE.pop(k, None)
        _CACHE[key] = (0, time.time() + CACHE_TTL, results)


# ---------------- 请求转换 ----------------

def extract_query(data: dict) -> str:
    """从 Responses 请求中提取搜索查询词（最后一条 user 消息文本）"""
    inp = data.get("input", "")
    if isinstance(inp, str):
        return inp.strip()
    if isinstance(inp, list):
        texts = []
        for item in inp:
            if isinstance(item, str):
                texts.append(item)
            elif isinstance(item, dict):
                c = item.get("content", "")
                if isinstance(c, str):
                    texts.append(c)
                elif isinstance(c, list):
                    for part in c:
                        if isinstance(part, dict) and part.get("type") in ("input_text", "text"):
                            texts.append(part.get("text", ""))
        for t in reversed(texts):
            if t.strip():
                return t.strip()
    return ""


def build_messages(data: dict) -> list:
    """把 Responses input 转成 chat messages"""
    messages = []
    instr = data.get("instructions")
    if instr:
        messages.append({"role": "system", "content": instr})
    inp = data.get("input", "")
    if isinstance(inp, str):
        if inp:
            messages.append({"role": "user", "content": inp})
    elif isinstance(inp, list):
        for item in inp:
            if isinstance(item, str):
                messages.append({"role": "user", "content": item})
            elif isinstance(item, dict):
                role = item.get("role", "user")
                c = item.get("content", "")
                if isinstance(c, str):
                    messages.append({"role": role, "content": c})
                elif isinstance(c, list):
                    parts = []
                    for part in c:
                        if isinstance(part, dict):
                            if part.get("type") in ("input_text", "output_text", "text"):
                                parts.append(part.get("text", ""))
                            elif part.get("type") == "summary" and part.get("summary"):
                                parts.append(str(part.get("summary")))
                    messages.append({"role": role, "content": "\n".join(p for p in parts if p)})
    return messages


# ---------------- 响应转换 ----------------

def _search_count(tools) -> int:
    """从 web_search 工具定义中提取期望结果数。

    qidi.exe 实际载荷形如 {"type": "web_search", "filters": {}}；
    兼容顶层与 filters 内的 count/max_results，以及 OpenAI 风格
    search_context_size（low/medium/high -> 3/5/8）。默认 5。
    """
    for t in tools:
        if not (isinstance(t, dict) and t.get("type") == "web_search"):
            continue
        filters = t.get("filters") if isinstance(t.get("filters"), dict) else {}
        for src in (t, filters):
            for k in ("count", "max_results", "max_query_count"):
                v = src.get(k)
                if isinstance(v, (int, float)) and not isinstance(v, bool):
                    return max(1, min(10, int(v)))
        sz = t.get("search_context_size") or filters.get("search_context_size")
        if sz == "low":
            return 3
        if sz == "high":
            return 8
    return 5


def anchor_citations(text: str, citations):
    """把引用锚定到输出正文中首次出现 [n] 的位置；未被 [n] 引用的来源不生成锚点。"""
    anns = []
    for idx, (title, url) in enumerate(citations, 1):
        m = re.search(r"\[%d\]" % idx, text)
        if m:
            anns.append({"type": "url_citation", "url": url, "title": title,
                         "start_index": m.start(), "end_index": m.end()})
    return anns


def make_response_obj(chat_resp: dict, model: str, citations: list = None) -> dict:
    """chat.completion -> Responses API 格式（usage 用 input_tokens/output_tokens）

    v2：citations 优先按正文 [n] 标注锚定（真实偏移）；模型未标注时退化为全量列表。
    """
    choice = (chat_resp.get("choices") or [{}])[0]
    msg = choice.get("message") or {}
    text = msg.get("content") or ""
    usage = chat_resp.get("usage") or {}
    text_part = {"type": "output_text", "text": text, "annotations": []}
    if citations:
        anns = anchor_citations(text, citations)
        if anns:
            log.info("引用锚定 %d/%d 条到正文位置", len(anns), len(citations))
        else:
            anns = [{"type": "url_citation", "url": u, "title": t,
                     "start_index": 0, "end_index": 0} for t, u in citations]
            log.info("正文无 [n] 标注，退化为全量引用 %d 条", len(anns))
        text_part["annotations"] = anns
    rid = chat_resp.get("id") or f"resp_{uuid.uuid4().hex[:12]}"
    new_usage = {
        "input_tokens": usage.get("prompt_tokens", 0),
        "input_tokens_details": {
            "cached_tokens": usage.get("prompt_cache_hit_tokens", 0),
        },
        "output_tokens": usage.get("completion_tokens", 0),
        "output_tokens_details": {
            "reasoning_tokens": usage.get("completion_tokens_details", {}).get("reasoning_tokens", 0),
        },
        "total_tokens": usage.get("total_tokens", 0),
    }
    return {
        "id": rid,
        "object": "response",
        "created_at": chat_resp.get("created") or int(time.time()),
        "status": "completed",
        "model": model,
        "output": [{
            "id": f"msg_{rid}",
            "type": "message",
            "role": "assistant",
            "status": "completed",
            "content": [text_part],
        }],
        "usage": new_usage,
    }


# ---------------- 端点 ----------------

@app.post("/v1/responses")
def v1_responses():
    data = request.get_json(force=True, silent=True) or {}
    log.info("=== /v1/responses 请求 ===")
    log.info("model=%s stream=%s tools=%s", data.get("model"), data.get("stream"),
             json.dumps(data.get("tools"), ensure_ascii=False))
    log.info("keys=%s", list(data.keys()))
    log.info("input=%s", json.dumps(data.get("input"), ensure_ascii=False)[:2000])
    if data.get("instructions"):
        log.info("instructions=%s", str(data.get("instructions"))[:1500])

    model = data.get("model") or SHIM_MODEL
    messages = build_messages(data)

    tools = data.get("tools") or []
    wants_search = any(isinstance(t, dict) and t.get("type") == "web_search" for t in tools)
    citations = []
    if wants_search:
        query = extract_query(data)
        if query:
            count = _search_count(tools)
            results = _cache_get(query, count)
            if results is not None:
                log.info("缓存命中 %r (count=%d)", query, count)
            else:
                results = do_web_search(query, count=count)
                if results:
                    _cache_put(query, count, results)
            if results:
                citations = [(t, u) for t, u, _ in results]
                lines = ["以下是实时网络搜索结果（已抓取），请基于这些结果回答用户问题：", ""]
                for i, (t, u, s) in enumerate(results, 1):
                    lines.append(f"[{i}] {t}\n    URL: {u}\n    摘要: {s}")
                lines.append("")
                lines.append(f"用户问题：{query}")
                lines.append("请优先引用上面的搜索结果回答：正文中在对应结论处用 [编号] 标注，"
                             "并在末尾列出引用的编号与来源 URL。")
                messages = messages + [{"role": "user", "content": "\n".join(lines)}]
                log.info("注入搜索结果 %d 条 (count=%d)", len(results), count)
            else:
                log.warning("搜索无结果，退回原样提问")

    chat_req = {
        "model": model,
        "messages": messages,
        "max_tokens": data.get("max_output_tokens") or 4096,
        "stream": False,
    }
    log.info("转发 chat 请求: model=%s, %d 条消息", model, len(messages))

    try:
        r = httpx.post(UPSTREAM, json=chat_req, timeout=300)
    except Exception as e:
        log.error("上游请求异常: %s", e)
        return jsonify({"error": {"message": f"shim upstream error: {e}"}}), 502

    log.info("上游响应 HTTP %s, 长度 %d", r.status_code, len(r.content))
    if r.status_code != 200:
        log.warning("上游非200: %s", r.text[:1000])
        return Response(r.content, status=r.status_code,
                        content_type=r.headers.get("content-type", "application/json"))

    try:
        chat_resp = r.json()
    except Exception as e:
        log.error("上游响应不是 JSON: %s | body=%s", e, r.text[:500])
        return jsonify({"error": {"message": "shim: upstream non-JSON response"}}), 502

    resp_obj = make_response_obj(chat_resp, model, citations)
    out_text = resp_obj["output"][0]["content"][0]["text"]
    n_ann = len(resp_obj["output"][0]["content"][0]["annotations"])
    log.info("返回 Responses 对象: usage=%s, annotations=%d, text前300=%s",
             resp_obj["usage"], n_ann, out_text[:300])
    return jsonify(resp_obj)


# chat/completions 透传（调试用）
@app.post("/v1/chat/completions")
def v1_chat():
    data = request.get_json(force=True, silent=True) or {}
    r = httpx.post(UPSTREAM, json=data, timeout=300)
    return Response(r.content, status=r.status_code,
                    content_type=r.headers.get("content-type", "application/json"))


@app.get("/v1/models")
def v1_models():
    return jsonify({"object": "list", "data": [{"id": SHIM_MODEL, "object": "model", "owned_by": "shim"}]})


@app.get("/health")
def health():
    with _CACHE_LOCK:
        cache_n = len(_CACHE)
    return jsonify({"ok": True, "upstream": UPSTREAM, "time": datetime.now().isoformat(),
                    "version": SHIM_VERSION, "pid": os.getpid(), "cache_entries": cache_n,
                    "cache_hits": _STATS["cache_hits"], "searches": _STATS["searches"]})


if __name__ == "__main__":
    log.info("shim v%s 启动于 0.0.0.0:%d -> %s", SHIM_VERSION, PORT, UPSTREAM)
    print(f"shim v{SHIM_VERSION} listening on {PORT} -> {UPSTREAM}")
    app.run(host="127.0.0.1", port=PORT, threaded=True)
