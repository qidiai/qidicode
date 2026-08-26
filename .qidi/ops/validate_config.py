# -*- coding: utf-8 -*-
"""Validate config.toml after dead-endpoint fixes."""
import sys, tomllib, json
sys.stdout.reconfigure(encoding="utf-8")

cfg = tomllib.load(open(r".qidi/config.toml", "rb"))
models = cfg["model"]  # entries live under [model.*] (singular)
top = cfg["models"]
print(f"默认模型: {top.get('default')} | web_search: {top.get('web_search')}")
print(f"模型条目数: {len(models)}")

ids = set(models)
dead_ports, all_fallbacks = [], []
for mid, m in models.items():
    url = m.get("base_url", "")
    if ":9800" in url or ":8790" in url:
        dead_ports.append((mid, url))
    fb = m.get("fallback_models", [])
    for f in fb:
        all_fallbacks.append((mid, f))

print("\n--- 死端口残留 ---")
print(dead_ports if dead_ports else "无 ✓")
print("\n--- fallback 引用完整性 ---")
bad = [(m, f) for m, f in all_fallbacks if f not in ids]
for m, f in all_fallbacks:
    print(f"  {m} -> {f}  {'✓' if f in ids else '✗ 不存在!'}")
if not all_fallbacks:
    print("  (无 fallback 配置)")
print("\n--- 192.168.1.222 残留 ---")
raw = open(r".qidi/config.toml", encoding="utf-8").read()
print("无 ✓" if "192.168.1.222" not in raw else "仍有残留!")
print(f"\n结论: {'PASS' if not dead_ports and not bad and '192.168.1.222' not in raw else 'FAIL'}")
