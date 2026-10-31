"""回归测试：审计 D1（prune/restore 往返）与 D2（重复 add 丢条目）。
用 monkeypatch 替换 fetch_catalog，不碰网络、不碰真实配置。
"""
import argparse
import io
import os
import sys
import tempfile
import tomllib

sys.path.insert(0, r"G:\qidicode\tools\model-sync")
import sync_relay_models as m

RELAY = "https://relay.test/v1"
FAILS = []


def check(name, cond, detail=""):
    print(("PASS " if cond else "FAIL ") + name + ("" if cond else " | " + detail))
    if not cond:
        FAILS.append(name)


def base_cfg(path):
    text = """[model."alive-222"]
model = "alive"
base_url = "https://relay.test/v1"
api_key = "k"
group = "intern"
context_window = 131072

[model."dead-222"]
model = "dead"
base_url = "https://relay.test/v1"
api_key = "k"
group = "intern"
context_window = 131072

[ui]
theme = "dark"
"""
    io.open(path, "w", encoding="utf-8", newline="\n").write(text)


def ns(path, **kw):
    d = dict(config=path, relay=RELAY, api_key="k", group="intern", provider="intern",
             models=None, from_ssh=None, suffix="-222", ctx=131072, apply=True)
    d.update(kw)
    return argparse.Namespace(**d)


def keys(path):
    return set(tomllib.load(open(path, "rb")).get("model", {}))


def test_prune_restore_roundtrip():
    tmp = tempfile.mkdtemp(prefix="qms-test-")
    path = os.path.join(tmp, "config.toml")
    base_cfg(path)
    before = keys(path)

    m.fetch_catalog = lambda relay, key, timeout=30: {"alive"}  # dead 不在目录里
    m.cmd_prune(ns(path))
    mid = keys(path)
    check("prune 移除 dead", "dead-222" not in mid and "alive-222" in mid, str(mid))
    check("prune 后 TOML 合法", True)

    m.fetch_catalog = lambda relay, key, timeout=30: {"alive", "dead"}  # dead 回来了
    m.cmd_restore(ns(path))
    after = keys(path)
    check("restore 还原 dead 且表头正确", "dead-222" in after, str(after))
    check("prune→restore 往返等价", after == before, "%s != %s" % (after, before))
    return tmp


def test_add_is_idempotent():
    tmp = tempfile.mkdtemp(prefix="qms-test-")
    path = os.path.join(tmp, "config.toml")
    base_cfg(path)

    # 第一轮：目录里只有 A
    m.fetch_catalog = lambda relay, key, timeout=30: {"alive", "dead", "A"}
    m.cmd_add(ns(path, models="A"))
    first = keys(path)
    check("首轮加入 A", "A-222" in first, str(first))

    # 第二轮：目录新增 B，A 应保留
    m.fetch_catalog = lambda relay, key, timeout=30: {"alive", "dead", "A", "B"}
    m.cmd_add(ns(path, models="A,B"))
    second = keys(path)
    check("第二轮 A 未丢失（D2）", "A-222" in second, str(second))
    check("第二轮加入 B", "B-222" in second, str(second))

    # 第三轮：无新增，不应改变
    m.fetch_catalog = lambda relay, key, timeout=30: {"alive", "dead", "A", "B"}
    m.cmd_add(ns(path, models="A,B"))
    check("第三轮稳定", keys(path) == second, "%s != %s" % (keys(path), second))
    return tmp


if __name__ == "__main__":
    test_prune_restore_roundtrip()
    test_add_is_idempotent()
    print()
    print("FAILED:", FAILS if FAILS else "none")
    sys.exit(1 if FAILS else 0)