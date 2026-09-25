# -*- coding: utf-8 -*-
"""从 TUNA 镜像下载 MSYS2 mingw64 binutils（含 as.exe）+ 依赖 DLL 包，解压到 C:\\mingw-tools。
用途：补全 rustup windows-gnu 工具链缺失的汇编器，解决 dlltool CreateProcess 失败。"""
import re
import subprocess
import sys
from pathlib import Path

TUNA = "https://mirrors.tuna.tsinghua.edu.cn/msys2/mingw/mingw64"
OUT_DIR = Path(r"C:\mingw-tools")
DL_DIR = Path(r".qidi\tmp\msys_pkgs")
DL_DIR.mkdir(parents=True, exist_ok=True)

# 需要的包（binutils 本体 + 常见运行时 DLL 依赖）
WANTED = [
    "mingw-w64-x86_64-binutils",
    "mingw-w64-x86_64-gettext-runtime",
    "mingw-w64-x86_64-libiconv",
    "mingw-w64-x86_64-zstd",
    "mingw-w64-x86_64-libwinpthread",
    "mingw-w64-x86_64-gcc-libs",
]

import httpx

r = httpx.get(f"{TUNA}/", timeout=30)
r.raise_for_status()
html = r.text
print(f"目录列表 {len(html)//1024}KB")

pkgs = re.findall(r'href="([^"]+\.pkg\.tar\.zst)"', html)
print(f"共 {len(pkgs)} 个包")

chosen = {}
for name in WANTED:
    cands = [p for p in pkgs if re.match(re.escape(name) + r"-\d", p)]
    if not cands:
        print(f"[MISS] {name}: 未找到")
        continue
    # 取版本号排序最大的（文件名含版本，粗排即可）
    chosen[name] = sorted(cands)[-1]
    print(f"[OK]   {name}: {chosen[name]}")

if len(chosen) < 3:
    print("关键包缺失，中止")
    sys.exit(1)

# 下载
files = []
for name, fname in chosen.items():
    dest = DL_DIR / fname
    if dest.exists() and dest.stat().st_size > 1000:
        print(f"[SKIP] {fname} 已存在")
    else:
        with httpx.stream("GET", f"{TUNA}/{fname}", timeout=120) as resp:
            resp.raise_for_status()
            with open(dest, "wb") as f:
                for chunk in resp.iter_bytes(65536):
                    f.write(chunk)
        print(f"[DL]   {fname} ({dest.stat().st_size//1024}KB)")
    files.append(dest)

# 解压（zstandard + tarfile）
try:
    import zstandard
except ImportError:
    subprocess.run([sys.executable, "-m", "pip", "install", "zstandard",
                    "-i", "https://pypi.tuna.tsinghua.edu.cn/simple", "-q"],
                   check=True)
    import zstandard

import tarfile
import io

OUT_DIR.mkdir(parents=True, exist_ok=True)
for pkg in files:
    raw = pkg.read_bytes()
    dctx = zstandard.ZstdDecompressor()
    tar_bytes = io.BytesIO(dctx.stream_reader(io.BytesIO(raw)).read())
    with tarfile.open(fileobj=tar_bytes, mode="r:") as tf:
        tf.extractall(OUT_DIR)
    print(f"[EXT]  {pkg.name}")

# 验证
as_exe = OUT_DIR / "mingw64" / "bin" / "as.exe"
dlltool_exe = OUT_DIR / "mingw64" / "bin" / "dlltool.exe"
print()
print(f"as.exe:      {'✅ ' + str(as_exe) if as_exe.exists() else '❌ 缺失'}")
print(f"dlltool.exe: {'✅ ' + str(dlltool_exe) if dlltool_exe.exists() else '❌ 缺失'}")

# 试运行 as --version（验证 DLL 依赖齐全）
if as_exe.exists():
    r = subprocess.run([str(as_exe), "--version"], capture_output=True, text=True)
    if r.returncode == 0:
        print(f"试运行: {r.stdout.splitlines()[0]}")
        print(f"\n将此目录加入 PATH: {OUT_DIR / 'mingw64' / 'bin'}")
    else:
        print(f"试运行失败(rc={r.returncode}): {r.stderr[:300]}")
        print("可能缺 DLL，按报错补充对应 msys2 包")
        sys.exit(1)
