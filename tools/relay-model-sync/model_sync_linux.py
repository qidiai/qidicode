#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""云端中转模型清单自同步（Linux / 只写中转 config.json）。

对比台式机版 model_sync.py 的差异：
  - 路径改为 /opt/codely-relay，不再写 qidicode 的 config.toml（本地清单由
    tools/model-sync/sync_relay_models.py 负责）
  - 只做 append-only：上游新增 → 自动并入 provider.models；上游消失 → 只报告
    （一次 401/超时就把某 provider 的模型全删掉，代价太大，不做）
  - 名字冲突规避：候选名若已被更靠前的 provider 抢走，跳过并记账
  - 单次单 provider 上限 40，防上游一次吐几百个

策略表 POLICY 决定每个 provider 的行为：
  append  自动追加新模型
  report  只报告，不写盘
  skip    完全不碰（本地端点、用户亲自裁剪过的、已知上游故障的）

用法：
  python3 model_sync_linux.py            # 实际写入
  python3 model_sync_linux.py --dry-run  # 只报告
"""
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import sys
import time

import httpx

BASE_DIR = "/opt/codely-relay"
CFG_FILE = os.path.join(BASE_DIR, "config.json")
LOG_FILE = os.path.join(BASE_DIR, "model_sync.log")
MAX_NEW_PER_PROVIDER = 40

CHAT_HINT = re.compile(
    r"glm|deepseek|kimi|minimax|mimo|qwen|claude|fable|opus|sonnet|haiku|grok|gpt|"
    r"doubao|ernie|yi-|intern|longcat|agnes|spark|gemini|llama|mistral|step|seed|"
    r"atria|sage|agents|codely|baichuan|hunyuan|granite|phi-|command|nova|o[134]-",
    re.I,
)

# 每个 provider 的同步策略。未列出的默认 report（先观察，再逐个人工放行）。
#
# append 只给"上游 /v1/models 就是该 key 的授权清单"的通道。以下实测反例说明
# 为什么不能一律 append：
#   unisound  上游列 29 个，key 实际只有 u2-flash 有权限（其余 404 model_not_found）
#   jiuwei    2026-10-31 起全系 403，清单看着全、实际全不可用
#   openrouter 一次吐 300+，含 :batch 变体，追加会把目录冲烂
#   dimleap   18 个是用户亲自裁剪后的口径，加回去等于覆盖用户决定
POLICY = {
    "intern": "append",
    "kimi-ai": "append",
    "modelscope": "append",
    "antling": "append",
    "openscale": "append",
    "sensenova": "append",
    "52duck": "report",
    "apigoto": "report",
    "lyrouter": "report",
    "agnes": "report",
    "openrouter": "report",
    "jiuwei": "report",
    "unisound": "skip",
    # 用户亲自裁剪过（18 个模型是裁剪后的口径），不要自动加回去
    "dimleap": "skip",
    # 本地端点 / 已知上游故障
    "amd": "skip",
    "opencode": "skip",
    "npu": "skip",
    "opencode-go": "skip",
    "stepplan": "skip",
    "stepfun": "skip",
    # 服务器上没有首尔出口，直连 generativelanguage 必然 Network unreachable
    "gs": "skip",
}


def log(msg: str) -> None:
    line = "[%s] %s" % (time.strftime("%m-%d %H:%M:%S"), msg)
    print(line)
    try:
        with open(LOG_FILE, "a", encoding="utf-8") as f:
            f.write(line + "\n")
    except OSError:
        pass


def atomic_write_json(path: str, obj) -> None:
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(obj, f, ensure_ascii=False, indent=2)
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, path)


def provider_keys(p: dict) -> list[str]:
    out = []
    for field in ("api_keys", "keys"):
        for item in (p.get(field) or []):
            key = item.get("key") if isinstance(item, dict) else item
            if key:
                out.append(key)
    return out


def fetch_upstream(base_url: str, key: str, timeout: int = 20) -> list[str]:
    headers = {"Authorization": "Bearer " + key} if key else {}
    r = httpx.get(base_url.rstrip("/") + "/models", headers=headers, timeout=timeout)
    r.raise_for_status()
    data = r.json()
    items = data.get("data") or data.get("models") or []
    out = []
    for m in items:
        mid = m.get("id") if isinstance(m, dict) else str(m)
        if mid:
            out.append(mid)
    return out


def claimed_by(providers: dict, name: str, before: str) -> str | None:
    """名字是否已被某个更靠前的 provider 抢走（first-match-wins）。"""
    for pname, p in providers.items():
        if pname == before:
            return None
        if not p.get("enabled", True):
            continue
        if name in (p.get("models") or []) or name in (p.get("model_aliases") or {}):
            return pname
    return None


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args(argv)

    with open(CFG_FILE, encoding="utf-8") as f:
        cfg = json.load(f)
    providers = cfg.get("providers") or {}

    report = {"added": {}, "removed": {}, "collisions": {}, "errors": [], "policy": {}}
    changed = False

    for pname, p in providers.items():
        policy = POLICY.get(pname, "report")
        report["policy"][pname] = policy
        if policy == "skip":
            continue
        base_url = (p.get("base_url") or "").rstrip("/")
        keys = provider_keys(p)
        if not base_url or not keys:
            continue
        try:
            upstream = fetch_upstream(base_url, keys[0])
        except Exception as exc:  # 网络/鉴权/上游故障一律只记录
            report["errors"].append("%s: %s: %s" % (pname, type(exc).__name__, str(exc)[:120]))
            log("[ERR] %s 探测失败：%s" % (pname, str(exc)[:120]))
            continue

        if not upstream:
            report["errors"].append("%s: 上游返回空列表，跳过" % pname)
            continue

        old = list(p.get("models") or [])
        old_set, up_set = set(old), set(upstream)
        deleted = sorted(old_set - up_set)
        candidates = sorted(up_set - old_set)
        candidates = [m for m in candidates if CHAT_HINT.search(m)]
        # 冒号后缀是上游的变体档（:free / :batch / :fast），不是独立模型名，
        # 追加进来只会在选择器里制造噪音。
        candidates = [m for m in candidates if ":" not in m]

        if deleted:
            report["removed"][pname] = deleted
            log("%s: 上游已无 %s（仅报告，不自动删）" % (pname, deleted[:8]))

        if policy == "report":
            if candidates:
                log("%s: 发现新模型 %s（report 模式，未写入）" % (pname, candidates[:8]))
            continue

        accepted, collisions = [], []
        for name in candidates:
            other = claimed_by(providers, name, pname)
            if other:
                collisions.append("%s<-%s" % (name, other))
                continue
            accepted.append(name)
        if len(accepted) > MAX_NEW_PER_PROVIDER:
            accepted = accepted[:MAX_NEW_PER_PROVIDER]
        if collisions:
            report["collisions"][pname] = collisions
            log("%s: 跳过同名占位 %s" % (pname, collisions[:6]))
        if accepted:
            p["models"] = sorted(old_set | set(accepted))
            report["added"][pname] = accepted
            changed = True
            log("%s: 追加 %d 个 → %s" % (pname, len(accepted), accepted[:8]))

    if changed and not args.dry_run:
        stamp = time.strftime("%Y%m%d")
        backup = CFG_FILE + ".bak-modelsync-" + stamp
        if not os.path.exists(backup):
            shutil.copy2(CFG_FILE, backup)
        atomic_write_json(CFG_FILE, cfg)
        log("config.json 已写入（热加载生效）")
    elif args.dry_run:
        log("dry-run：未写盘")

    print(json.dumps(report, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())