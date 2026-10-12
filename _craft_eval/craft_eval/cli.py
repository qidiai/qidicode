# -*- coding: utf-8 -*-
"""cli.py — wordcraft-cli 子进程包装 (T1 规格书 §3).

关键点（规格书 §3）：
- 一律 subprocess.run(args, capture_output=True, timeout=...)；
  读满输出，禁止提前关管道（wordcraft 把 stdout 关闭当成功，是假成功陷阱）。
- 退出码语义写死常量表：0=成功，1=业务失败，101=Rust panic，其他原样记录。
- TimeoutExpired 捕获后返回 rc="TIMEOUT"，不抛。
"""
import json
import os
import subprocess
import time

DEFAULT_EXE = r"G:\程序开发\artcraft\bin\wordcraft-0.5.0-windows-x64-portable\wordcraft-cli.exe"

# 退出码语义（写死常量表）
RC_SUCCESS = 0
RC_BIZ_FAIL = 1
RC_PANIC = 101
RC_NAMES = {
    0: "success",
    1: "business_fail",
    101: "rust_panic",
}
# 非进程退出码（本包装内部语义，非 wordcraft 定义）
RC_TIMEOUT = "TIMEOUT"
RC_NOEXE = "NOEXE"

_HEAD = 300  # stdout_head / stderr_head / parse_error 截取长度


class WordcraftCLI:
    """wordcraft-cli 子进程包装。"""

    def __init__(self, exe_path=None):
        # 优先级：显式参数 > env WORDCRAFT_CLI > 默认路径
        self.exe = exe_path or os.environ.get("WORDCRAFT_CLI") or DEFAULT_EXE

    # -- 内部 -------------------------------------------------------------
    def exists(self):
        return os.path.isfile(self.exe)

    def _spawn(self, args, timeout):
        """跑一次子进程，返回 dict(rc,out_b,err_b,ms,spawn_error)。

        - subprocess.run 会读满管道后再等待进程退出（不会提前关管道）。
        - timeout 触发 → rc="TIMEOUT"（附已捕获输出）。
        - exe 不存在 → rc="NOEXE"。
        """
        argv = [self.exe] + [str(a) for a in args]
        t0 = time.monotonic()
        try:
            p = subprocess.run(argv, capture_output=True, timeout=timeout)
            return {
                "rc": p.returncode,
                "out_b": p.stdout or b"",
                "err_b": p.stderr or b"",
                "ms": round((time.monotonic() - t0) * 1000, 1),
                "spawn_error": None,
            }
        except subprocess.TimeoutExpired as e:
            return {
                "rc": RC_TIMEOUT,
                "out_b": e.stdout or b"",
                "err_b": e.stderr or b"",
                "ms": round((time.monotonic() - t0) * 1000, 1),
                "spawn_error": None,
            }
        except (FileNotFoundError, NotADirectoryError, PermissionError, OSError) as e:
            return {
                "rc": RC_NOEXE,
                "out_b": b"",
                "err_b": ("%s: %s" % (type(e).__name__, e)).encode("utf-8", "replace"),
                "ms": round((time.monotonic() - t0) * 1000, 1),
                "spawn_error": str(e),
            }

    @staticmethod
    def _dec(b):
        # UTF-8 + replace：CLI 若自身吐坏字会被计成 U+FFFD，正是我们要观测的
        return (b or b"").decode("utf-8", "replace")

    # -- 公开 API ---------------------------------------------------------
    def info(self, path, timeout=180):
        """`info <path>` → {"rc","json"|None,"parse_error","stdout_head","stderr_head","ms"}。"""
        r = self._spawn(["info", path], timeout)
        out = self._dec(r["out_b"])
        err = self._dec(r["err_b"])
        js = None
        parse_error = None
        try:
            js = json.loads(out.strip()) if out.strip() else None
        except Exception:
            parse_error = out[:_HEAD]
        return {
            "rc": r["rc"],
            "json": js,
            "parse_error": parse_error,
            "stdout_head": out[:_HEAD],
            "stderr_head": err[:_HEAD],
            "ms": r["ms"],
        }

    def text(self, path, timeout=180):
        """`text <path>` → {"rc","out","err","ms"}。"""
        r = self._spawn(["text", path], timeout)
        return {
            "rc": r["rc"],
            "out": self._dec(r["out_b"]),
            "err": self._dec(r["err_b"]),
            "ms": r["ms"],
        }

    def render(self, path, out_png, page, scale=2.0, timeout=180):
        """`render <file> <out.png> --page <N> --scale <S>` → {"rc","err","ms","png_bytes"}。"""
        r = self._spawn(["render", path, out_png, "--page", page, "--scale", scale],
                        timeout)
        size = 0
        try:
            if os.path.exists(out_png):
                size = os.path.getsize(out_png)
        except OSError:
            size = 0
        return {
            "rc": r["rc"],
            "err": self._dec(r["err_b"]),
            "ms": r["ms"],
            "png_bytes": size,
        }
