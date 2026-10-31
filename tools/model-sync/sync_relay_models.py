#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""云端中转模型清单同步器（qidicode config.toml ← luyou 云中转）。

背景：中转的 provider models 会被上游随时改动，而 qidicode 的 config.toml 是
手写清单。两边不同步会出现两类故障：
  1. 中转已下线的模型还留在本地 → 选中即 404 model_not_found
  2. 中转新增的模型本地没有 → 明明能用却选不到

本工具的三个动作：
  check  只读体检：本地每个指向中转的条目，其 model 是否仍在 /v1/models 里
  add    把某 provider 的新模型补进 config.toml（写入托管块，可重复执行）
  prune  把本地已失效的条目注释掉（注释而非删除，随时可恢复）

设计取舍：只做"中转 → 本地"单向同步。中转 provider 的新增/下线仍由中转侧
决定，本工具不碰中转配置。

用法：
  python tools/model-sync/sync_relay_models.py check
  python tools/model-sync/sync_relay_models.py add --provider intern --from-ssh ubuntu@HOST --apply
  python tools/model-sync/sync_relay_models.py prune --group intern --apply

默认 dry-run，加 --apply 才写盘；写盘前自动备份并用 tomllib 校验。
"""
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import ssl
import subprocess
import sys
import time
import tomllib
import urllib.request

DEFAULT_RELAY = "https://luyou.qidiai.ltd:8443/v1"
DEFAULT_CONFIG = os.path.expanduser(r"~\.qidi\config.toml")
MANAGED_BEGIN = "# >>> qidi-model-sync managed"
MANAGED_END = "# <<< qidi-model-sync managed"
DUMP_REMOTE = r'''
import json
cfg = json.load(open("/opt/codely-relay/config.json"))
p = (cfg.get("providers") or {})[PROVIDER]
print(json.dumps({"models": p.get("models") or [],
                  "aliases": p.get("model_aliases") or {},
                  "enabled": p.get("enabled", True)}, ensure_ascii=False))
'''


def log(msg: str) -> None:
    print(msg)


def load_config(path: str) -> dict:
    with open(path, "rb") as f:
        return tomllib.load(f)


def relay_entries(cfg: dict, relay: str) -> dict[str, dict]:
    host = relay.split("//", 1)[-1].split("/", 1)[0]
    out = {}
    for mid, conf in (cfg.get("model") or {}).items():
        url = str(conf.get("base_url") or "")
        if host in url:
            out[mid] = conf
    return out


def fetch_catalog(relay: str, key: str, timeout: int = 30) -> set[str]:
    req = urllib.request.Request(
        relay.rstrip("/") + "/models",
        headers={"Authorization": "Bearer " + key, "Accept": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=timeout, context=ssl.create_default_context()) as r:
        data = json.loads(r.read().decode("utf-8"))
    return {m["id"] for m in data.get("data", []) if isinstance(m, dict) and m.get("id")}


def pick_client_key(entries: dict[str, dict]) -> str:
    for conf in entries.values():
        key = conf.get("api_key")
        if key:
            return key
    raise SystemExit("config.toml 里没有可用的中转 api_key，请用 --api-key 指定")


def remote_provider_models(host: str, provider: str) -> dict:
    script = DUMP_REMOTE.replace("PROVIDER", json.dumps(provider))
    proc = subprocess.run(
        ["ssh", "-o", "BatchMode=yes", host, "sudo python3 -"],
        input=script, capture_output=True, text=True, encoding="utf-8",
    )
    if proc.returncode != 0:
        raise SystemExit("远程读取 provider 失败：" + (proc.stderr or "")[:300])
    return json.loads(proc.stdout.strip().splitlines()[-1])


def cmd_check(args) -> int:
    cfg = load_config(args.config)
    entries = relay_entries(cfg, args.relay)
    key = args.api_key or pick_client_key(entries)
    catalog = fetch_catalog(args.relay, key)
    log("中转目录 %d 个模型；本地指向中转的条目 %d 个" % (len(catalog), len(entries)))

    dead, alive = [], []
    for mid, conf in sorted(entries.items()):
        (alive if str(conf.get("model") or "") in catalog else dead).append(mid)
    log("可用 %d / 失效 %d" % (len(alive), len(dead)))
    if dead:
        log("\n失效条目（选中即 404）：")
        for mid in dead:
            group = entries[mid].get("group") or "-"
            log("  %-34s model=%s  group=%s" % (mid, entries[mid].get("model"), group))
        groups = sorted({(entries[m].get("group") or "-") for m in dead})
        log("\n受影响的 group：%s" % ", ".join(groups))
        log("修复：python %s prune --group <group> --apply" % os.path.basename(__file__))
    return 0


def render_block(mid: str, model: str, group: str, base_url: str, api_key: str, ctx: int) -> str:
    return (
        '[model."%s"]\n'
        'model = "%s"\n'
        'base_url = "%s"\n'
        'api_key = "%s"\n'
        'name = "%s (Server Relay)"\n'
        'group = "%s"\n'
        'api_backend = "chat_completions"\n'
        'auth_scheme = "bearer"\n'
        "supported_in_api = true\n"
        "context_window = %d\n"
    ) % (mid, model, base_url, api_key, model, group, ctx)


def strip_managed(text: str, group: str) -> str:
    pattern = re.compile(
        r"\n?" + re.escape(MANAGED_BEGIN) + r" group=" + re.escape(group) + r".*?"
        + re.escape(MANAGED_END) + r"\n?",
        re.S,
    )
    return pattern.sub("\n", text)


def cmd_add(args) -> int:
    cfg = load_config(args.config)
    entries = relay_entries(cfg, args.relay)
    if not entries:
        raise SystemExit("config.toml 里没有指向 %s 的条目" % args.relay)
    key = args.api_key or pick_client_key(entries)
    catalog = fetch_catalog(args.relay, key)

    if args.models:
        remote = {"models": [m.strip() for m in args.models.split(",") if m.strip()]}
    elif args.from_ssh:
        remote = remote_provider_models(args.from_ssh, args.provider)
    else:
        raise SystemExit("需要 --models 或 --from-ssh")

    group = args.group or args.provider
    same_group = [c for c in entries.values() if (c.get("group") or "") == group]
    sample = same_group[0] if same_group else next(iter(entries.values()))
    base_url = str(sample.get("base_url") or args.relay)
    ctx = int(args.ctx or sample.get("context_window") or 1048576)

    existing_models = {str(c.get("model")) for c in entries.values()}
    missing = [m for m in remote["models"] if m not in existing_models and m in catalog]
    skipped = [m for m in remote["models"] if m not in catalog]

    log("provider %s：中转登记 %d，本地已覆盖 %d，待新增 %d"
        % (args.provider, len(remote["models"]), len(remote["models"]) - len(missing), len(missing)))
    if skipped:
        log("跳过（中转自己也不路由）：%s" % ", ".join(skipped))
    if not missing:
        log("无需新增。")
        return 0

    blocks = []
    for model in missing:
        mid = model + args.suffix
        blocks.append(render_block(mid, model, group, base_url, key, ctx))
        log("  + [model.\"%s\"]  model=%s" % (mid, model))

    if not args.apply:
        log("\n(dry-run) 加 --apply 写入")
        return 0

    text = open(args.config, encoding="utf-8").read()
    text = strip_managed(text, group)
    managed = "\n%s group=%s\n# auto-generated by tools/model-sync/sync_relay_models.py at %s\n%s\n%s\n" % (
        MANAGED_BEGIN, group, time.strftime("%Y-%m-%d %H:%M:%S"), "\n".join(blocks), MANAGED_END,
    )
    new_text = text.rstrip("\n") + "\n" + managed
    tomllib.loads(new_text)  # 语法自检，坏了就不写

    bak = "%s.bak-%s" % (args.config, time.strftime("%Y%m%d-%H%M%S"))
    shutil.copy2(args.config, bak)
    open(args.config, "w", encoding="utf-8", newline="\n").write(new_text)
    log("\n已写入 %s（备份 %s）" % (args.config, os.path.basename(bak)))
    log("注意：qidicode 的模型注册表在启动时缓存，需要重启才会出现新条目。")
    return 0


DISABLED_MARK = "# [disabled by qidi-model-sync"


def cmd_restore(args) -> int:
    """把 prune 注释掉的条目还原（仅还原中转已重新路由的那些）。"""
    cfg = load_config(args.config)
    entries = relay_entries(cfg, args.relay)
    key = args.api_key or pick_client_key(entries)
    catalog = fetch_catalog(args.relay, key)

    lines = open(args.config, encoding="utf-8").read().splitlines(keepends=True)
    out, i, restored, kept = [], 0, [], []
    while i < len(lines):
        line = lines[i]
        if line.startswith(DISABLED_MARK):
            m = re.search(r"model=(\S+)", line)
            model = m.group(1) if m else None
            j = i + 1
            block = []
            while j < len(lines) and lines[j].startswith("#"):
                block.append(lines[j])
                j += 1
            # 注释体里第一行就是被注释掉的 [model."X"] 表头；若丢掉它，
            # key/value 就会漂到上一个表格里（TOML 不允许跨表写 key）。
            block_lines = [b[2:] if b.startswith("# ") else b[1:] if b.startswith("#") else b for b in block]
            group_ok = args.group is None or (
                len(block_lines) > 1 and "group = \"{}\"".format(args.group) in block_lines
            )
            if model and model in catalog and group_ok:
                restored.append(model)
                out.extend(block_lines)
            else:
                kept.append(model)
                out.append(line)
                out.extend(block)
            i = j
            continue
        out.append(line)
        i += 1

    if not restored:
        log("没有可还原的条目（剩余注释 %d 条）" % len(kept))
        return 0

    log("可还原 %d 条：%s" % (len(restored), ", ".join(restored)))
    if not args.apply:
        log("(dry-run) 加 --apply 写入")
        return 0

    new_text = "".join(out)
    tomllib.loads(new_text)
    bak = "%s.bak-%s" % (args.config, time.strftime("%Y%m%d-%H%M%S"))
    shutil.copy2(args.config, bak)
    open(args.config, "w", encoding="utf-8", newline="\n").write(new_text)
    log("已写入 %s（备份 %s）" % (args.config, os.path.basename(bak)))
    return 0


def cmd_prune(args) -> int:
    cfg = load_config(args.config)
    entries = relay_entries(cfg, args.relay)
    key = args.api_key or pick_client_key(entries)
    catalog = fetch_catalog(args.relay, key)

    targets = {
        mid for mid, conf in entries.items()
        if str(conf.get("model")) not in catalog
        and (args.group is None or (conf.get("group") or "") == args.group)
    }
    if not targets:
        log("没有需要下线的条目。")
        return 0

    text = open(args.config, encoding="utf-8").read()
    lines = text.splitlines(keepends=True)
    out, i, hit = [], 0, 0
    while i < len(lines):
        header = re.match(r'\[model\."?([^\]]+?)"?\]\s*$', lines[i].strip())
        if header and header.group(1) in targets:
            out.append("# [disabled by qidi-model-sync %s] 中转已不路由 model=%s\n"
                       % (time.strftime("%Y-%m-%d"), entries[header.group(1)].get("model")))
            j = i + 1
            while j < len(lines) and not lines[j].lstrip().startswith("["):
                out.append("# " + lines[j] if lines[j].strip() else lines[j])
                j += 1
            hit += 1
            i = j
            continue
        out.append(lines[i])
        i += 1

    log("待下线 %d 条（注释保留）：%s" % (len(targets), ", ".join(sorted(targets))))
    if not args.apply:
        log("(dry-run) 加 --apply 写入")
        return 0

    new_text = "".join(out)
    tomllib.loads(new_text)
    bak = "%s.bak-%s" % (args.config, time.strftime("%Y%m%d-%H%M%S"))
    shutil.copy2(args.config, bak)
    open(args.config, "w", encoding="utf-8", newline="\n").write(new_text)
    log("已写入 %s（备份 %s），实际注释 %d 条" % (args.config, os.path.basename(bak), hit))
    return 0


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description="云端中转模型清单同步器")
    p.add_argument("action", choices=["check", "add", "prune", "restore"])
    p.add_argument("--relay", default=DEFAULT_RELAY)
    p.add_argument("--config", default=DEFAULT_CONFIG)
    p.add_argument("--api-key", default=None)
    p.add_argument("--group", default=None)
    p.add_argument("--provider", default="intern")
    p.add_argument("--models", default=None, help="逗号分隔的模型名，免 ssh")
    p.add_argument("--from-ssh", default=None, help="例如 ubuntu@1.2.3.4，自动读取中转 provider 清单")
    p.add_argument("--suffix", default="-222")
    p.add_argument("--ctx", type=int, default=None)
    p.add_argument("--apply", action="store_true")
    args = p.parse_args(argv)
    return {"check": cmd_check, "add": cmd_add, "prune": cmd_prune, "restore": cmd_restore}[args.action](args)


if __name__ == "__main__":
    raise SystemExit(main())