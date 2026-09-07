# -*- coding: utf-8 -*-
"""从 TUNA 下载 MSYS2 mingw64 gcc 编译器全家桶，解压合并到 C:\\mingw-tools（已有 binutils）。"""
import re
import subprocess
import sys
from pathlib import Path

TUNA = "https://mirrors.tuna.tsinghua.edu.cn/msys2/mingw/mingw64"
OUT_DIR = Path(r"C:\mingw-tools")
DL_DIR = Path(r".qidi\tmp\msys_pkgs")
DL_DIR.mkdir(parents=True, exist_ok=True)

WANTED = [
    "mingw-w64-x86_64-gcc",
    "mingw-w64-x86_64-crt",
    "mingw-w64-x86_64-gmp",
    "mingw-w64-x86_64-isl",
    "mingw-w64-x86_64-mpc",
    "mingw-w64-x86_64-mpfr",
    "mingw-w64-x86_64-windows-default-manifest",
    "mingw-w64-x86_64-winpthreads",
    "mingw-w64-x86_64-zlib",
]

import httpx

r = httpx.get(f"{TUNA}/", timeout=30)
r.raise_for_status()
pkgs = re.findall(r'href="([^"]+\.pkg\.tar\.zst)"', r.text)
print(f"目录共 {len(pkgs)} 包")

chosen = {}
for name in WANTED:
    cands = [p for p in pkgs if re.match(re.escape(name) + r"-\d", p)]
    if not cands:
        print(f"[MISS] {name}")
        continue
    chosen[name] = sorted(cands)[-1]

for name, fname in chosen.items():
    dest = DL_DIR / fname
    if dest.exists() and dest.stat().st_size > 1000:
        print(f"[SKIP] {fname}")
        continue
    with httpx.stream("GET", f"{TUNA}/{fname}", timeout=300) as resp:
        resp.raise_for_status()
        with open(dest, "wb") as f:
            for chunk in resp.iter_bytes(65536):
                f.write(chunk)
    print(f"[DL]   {fname} ({dest.stat().st_size//1024//1024}MB)")

import zstandard
import tarfile
import io

for fname in chosen.values():
    pkg = DL_DIR / fname
    raw = pkg.read_bytes()
    dctx = zstandard.ZstdDecompressor()
    tar_bytes = io.BytesIO(dctx.stream_reader(io.BytesIO(raw)).read())
    with tarfile.open(fileobj=tar_bytes, mode="r:") as tf:
        tf.extractall(OUT_DIR)
    print(f"[EXT]  {fname}")

gcc = OUT_DIR / "mingw64" / "bin" / "gcc.exe"
print(f"\ngcc.exe: {'✅' if gcc.exists() else '❌'}")
if gcc.exists():
    r = subprocess.run([str(gcc), "--version"], capture_output=True, text=True)
    print(r.stdout.splitlines()[0] if r.returncode == 0 else f"FAIL: {r.stderr[:200]}")
    # 试编译一个 C 文件
    test_c = OUT_DIR / "test.c"
    test_c.write_text('#include <stdio.h>\nint main(){printf("ok\\n");return 0;}\n')
    r2 = subprocess.run([str(gcc), str(test_c), "-o", str(OUT_DIR / "test.exe")],
                        capture_output=True, text=True,
                        env={"PATH": str(OUT_DIR / "mingw64" / "bin") + ";" + __import__("os").environ["PATH"]})
    if r2.returncode == 0:
        r3 = subprocess.run([str(OUT_DIR / "test.exe")], capture_output=True, text=True)
        print("试编译+运行:", r3.stdout.strip() or "FAIL")
        (OUT_DIR / "test.c").unlink(missing_ok=True)
        (OUT_DIR / "test.exe").unlink(missing_ok=True)
    else:
        print("试编译失败:", r2.stderr[:300])
        sys.exit(1)
