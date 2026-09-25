#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
sync_codely_222.py — 一键同步局域网 Codely 代理(默认 192.168.1.222:8765)的模型
到 QIDI Code 配置:运行时 ~/.qidi/config.toml + 项目源码 default_models.json。

用法:
  python sync_codely_222.py                   # 同步两处(新增/更新 -222 条目)
  python sync_codely_222.py --dry-run         # 只预览将要做的改动,不写文件
  python sync_codely_222.py --only-config     # 只改 ~/.qidi/config.toml
  python sync_codely_222.py --only-source     # 只改 G:\\qidicode\\crates\\codegen\\cf-models\\default_models.json
  python sync_codely_222.py --remove-missing  # 同时删除上游已下线的 -222 条目
  python sync_codely_222.py --endpoint http://<host>:<port>   # 自定义端点

依赖:仅 Python 3.11+ 标准库(tomllib 解析校验,urllib 拉取,无第三方)。
每次写文件前自动备份到 <file>.bak-<timestamp>。
"""
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import sys
import time
import urllib.error
import urllib.request

DEFAULT_ENDPOINT = "http://192.168.1.222:8765"
CONFIG_TOML = os.path.expanduser("~/.qidi/config.toml")
DEFAULT_MODELS_JSON = r"G:\qidicode\crates\codegen\cf-models\default_models.json"
DEFAULT_CTX = 128000
SUFFIX = "-222"

# 展示名美化:codely-flash -> "Flash";未命中的退化用 title()
TITLE_MAP = {
    "codely-basic": "Basic",
    "codely-core": "Core",
    "codely-flash": "Flash",
    "codely-air": "Air",
    "codely-vl": "VL",
}


def log(msg: str) -> None:
    print(msg)


def backup(path: str) -> str:
    bak = f"{path}.bak-{time.strftime('%Y%m%d-%H%M%S')}"
    shutil.copy2(path, bak)
    return bak


def fetch_models(endpoint: str, timeout: int = 15) -> list[dict]:
    url = endpoint.rstrip("/") + "/v1/models"
    req = urllib.request.Request(url, headers={"Accept": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    arr = data.get("data") or data.get("models") or []
    models = [m for m in arr if isinstance(m, dict) and m.get("id")]
    return models


def ctx_of(m: dict) -> int:
    mml = m.get("max_model_len")
    return int(mml) if isinstance(mml, int) and mml > 0 else DEFAULT_CTX


def title_of(mid: str) -> str:
    return TITLE_MAP.get(mid, mid.replace("-", " ").title())


def make_config_block(mid: str, endpoint: str, ctx: int) -> str:
    key = f"{mid}{SUFFIX}"
    return (
        f'[model."{key}"]\n'
        f'model = "{mid}"\n'
        f'base_url = "{endpoint.rstrip("/")}/v1"\n'
        f'name = "CodeLy {title_of(mid)} (LAN 222)"\n'
        f'api_backend = "chat_completions"\n'
        f'auth_scheme = "bearer"\n'
        f"context_window = {ctx}\n"
    )


def make_json_entry(mid: str, endpoint: str, ctx: int) -> dict:
    host = endpoint.rstrip("/").replace("http://", "").replace("https://", "")
    return {
        "id": f"{mid}{SUFFIX}",
        "model": mid,
        "name": f"CodeLy {title_of(mid)} (LAN 222)",
        "description": f"CodeLy {title_of(mid)} via LAN proxy {host}",
        "base_url": f"{endpoint.rstrip('/')}/v1",
        "context_window": ctx,
        "api_backend": "chat_completions",
        "auth_scheme": "bearer",
        "supported_in_api": False,
    }


# ── config.toml:块级 upsert(兼容带引号/不带引号的 [model."X"] / [model.X]) ──
_BLOCK_RE = re.compile(r"\[model\.\"?" + re.escape(r"") + r"([^\]]*?)\"?\].*?(?=\n\[|\Z)", re.S)


def _key_of_header(line: str) -> str | None:
    m = re.match(r"\[model\.\"?([^\]]+?)\"?\]\s*$", line.strip())
    return m.group(1).strip() if m else None


def split_blocks(text: str) -> list[tuple[str | None, str]]:
    """把 TOML 文本切成 [(header_key_or_None, block_text)]。"""
    blocks: list[tuple[str | None, str]] = []
    cur_key: str | None = None
    cur_lines: list[str] = []
    for line in text.splitlines(keepends=True):
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            if cur_lines:
                blocks.append((cur_key, "".join(cur_lines)))
            cur_key = _key_of_header(line)
            cur_lines = [line]
        else:
            cur_lines.append(line)
    if cur_lines:
        blocks.append((cur_key, "".join(cur_lines)))
    return blocks


def upsert_config(text: str, key: str, block: str) -> str:
    blocks = split_blocks(text)
    out: list[str] = []
    replaced = False
    ui_idx = None
    for i, (k, b) in enumerate(blocks):
        if k == key:
            out.append(block if block.endswith("\n") else block + "\n")
            replaced = True
        else:
            out.append(b)
        if k == "ui":
            ui_idx = i
    if not replaced:
        if ui_idx is not None:
            # 在 [ui] 块前插入,保持手工添加时的位置
            out.insert(ui_idx, block if block.endswith("\n") else block + "\n")
        else:
            out.append(block if block.endswith("\n") else block + "\n")
    return "".join(out)


def remove_config_block(text: str, key: str) -> str:
    blocks = split_blocks(text)
    return "".join(b for k, b in blocks if k != key)


def sync_config(models: list[dict], endpoint: str, remove_missing: bool, dry: bool) -> None:
    path = CONFIG_TOML
    if not os.path.exists(path):
        log(f"[skip] 找不到 {path}")
        return
    text = open(path, encoding="utf-8").read()
    wanted = {f"{m['id']}{SUFFIX}": m for m in models}
    changes: list[str] = []
    new_text = text

    # 1) upsert 每个上游模型
    for m in models:
        mid = m["id"]
        key = f"{mid}{SUFFIX}"
        block = make_config_block(mid, endpoint, ctx_of(m))
        if _has_key(new_text, key):
            if _block_equal(new_text, key, block):
                continue
            changes.append(f"  update [model.\"{key}\"]")
        else:
            changes.append(f"  add    [model.\"{key}\"]")
        new_text = upsert_config(new_text, key, block)

    # 2) 可选:删除上游已不存在的 -222 条目
    if remove_missing:
        for k in _existing_222_keys(new_text):
            if k not in wanted:
                changes.append(f"  remove [model.\"{k}\"] (upstream gone)")
                new_text = remove_config_block(new_text, k)

    if not changes:
        log("[config.toml] 无变化")
        return
    log(f"[config.toml] {len(changes)} 处改动:")
    for c in changes:
        log(c)
    if dry:
        log("[dry-run] 不写文件")
        return
    bak = backup(path)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(new_text)
    log(f"[config.toml] 已写入(备份 {bak})")


def _has_key(text: str, key: str) -> bool:
    return any(k == key for k, _ in split_blocks(text))


def _existing_222_keys(text: str) -> list[str]:
    return [k for k, _ in split_blocks(text) if k and k.endswith(SUFFIX)]


def _block_equal(text: str, key: str, block: str) -> bool:
    for k, b in split_blocks(text):
        if k == key:
            return b.strip() == block.strip()
    return False


# ── default_models.json ──
def sync_source(models: list[dict], endpoint: str, remove_missing: bool, dry: bool) -> None:
    path = DEFAULT_MODELS_JSON
    if not os.path.exists(path):
        log(f"[skip] 找不到 {path}")
        return
    d = json.load(open(path, encoding="utf-8"))
    entries = d.setdefault("models", [])
    wanted = {f"{m['id']}{SUFFIX}": m for m in models}
    changes: list[str] = []
    existing = {e.get("id"): i for i, e in enumerate(entries)}

    for m in models:
        mid = m["id"]
        key = f"{mid}{SUFFIX}"
        entry = make_json_entry(mid, endpoint, ctx_of(m))
        if key in existing:
            if entries[existing[key]] == entry:
                continue
            changes.append(f"  update {key}")
            entries[existing[key]] = entry
        else:
            changes.append(f"  add    {key}")
            entries.append(entry)

    if remove_missing:
        stale = [
            eid for eid in existing
            if eid and eid.endswith(SUFFIX) and eid not in wanted
        ]
        for eid in stale:
            changes.append(f"  remove {eid} (upstream gone)")
        if stale:
            stale_set = set(stale)
            entries[:] = [e for e in entries if e.get("id") not in stale_set]

    if not changes:
        log("[default_models.json] 无变化")
        return
    log(f"[default_models.json] {len(changes)} 处改动:")
    for c in changes:
        log(c)
    if dry:
        log("[dry-run] 不写文件")
        return
    bak = backup(path)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(json.dumps(d, ensure_ascii=False, indent=2) + "\n")
    log(f"[default_models.json] 已写入(备份 {bak})")


def main() -> int:
    ap = argparse.ArgumentParser(description="同步 Codely LAN 代理模型到 QIDI Code 配置")
    ap.add_argument("--endpoint", default=DEFAULT_ENDPOINT, help=f"代理地址(默认 {DEFAULT_ENDPOINT})")
    ap.add_argument("--dry-run", action="store_true", help="只预览不写文件")
    ap.add_argument("--only-config", action="store_true", help="只同步 ~/.qidi/config.toml")
    ap.add_argument("--only-source", action="store_true", help="只同步 default_models.json")
    ap.add_argument("--remove-missing", action="store_true", help="删除上游已下线的 -222 条目")
    args = ap.parse_args()

    try:
        models = fetch_models(args.endpoint)
    except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError, OSError) as e:
        log(f"[error] 无法访问 {args.endpoint}/v1/models: {e}")
        return 1
    log(f"上游 {args.endpoint} 共 {len(models)} 个模型: {[m['id'] for m in models]}")

    # 校验 config.toml 现有内容可被 tomllib 解析(防呆)
    try:
        import tomllib
        with open(CONFIG_TOML, "rb") as f:
            tomllib.load(f)
    except Exception as e:
        log(f"[error] config.toml 解析失败,中止: {e}")
        return 1

    if not args.only_source:
        sync_config(models, args.endpoint, args.remove_missing, args.dry_run)
    if not args.only_config:
        sync_source(models, args.endpoint, args.remove_missing, args.dry_run)
    return 0


if __name__ == "__main__":
    sys.exit(main())
