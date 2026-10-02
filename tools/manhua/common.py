# -*- coding: utf-8 -*-
"""漫剧流水线公共模块：路径常量 / 分辨率约束 / 磁盘预检 / 日志 / 会话工厂。

所有 ffmpeg 调用一律 subprocess.run(列表参数)，禁 shell 字符串。
"""
import os
import shutil
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

import httpx

# ---------------------------------------------------------------- ffmpeg
# Task A 实测落位（BtbN static build, N-127054-g9d3f0f2c58-20261001）
FFMPEG = r"F:\AI\tools\ffmpeg\ffmpeg-master-latest-win64-gpl\bin\ffmpeg.exe"
FFPROBE = r"F:\AI\tools\ffmpeg\ffmpeg-master-latest-win64-gpl\bin\ffprobe.exe"

# ---------------------------------------------------------------- 服务地址
COMFYUI_URL = "http://127.0.0.1:8188"
# 中转（免费池）：环境变量可覆盖
RELAY_BASE_URL = os.environ.get(
    "MANHUA_RELAY_BASE_URL", "http://192.168.1.222:8765/v1")
RELAY_STORYBOARD_MODEL = os.environ.get("MANHUA_STORYBOARD_MODEL",
                                        "GLM-5.3-FLASH")
RELAY_VISION_MODEL = os.environ.get("MANHUA_VISION_MODEL",
                                    "agnes-2.5-pro-beta")

# ---------------------------------------------------------------- 编码常量
# Task C 实测验证（方案 §3.2 编码模板）
V_CODEC = "libx264"
CRF = "23"
PRESET = "medium"
PROFILE = "high"
LEVEL = "4.0"
PIX_FMT = "yuv420p"
FPS = 30
A_CODEC = "aac"
A_BITRATE = "192k"
A_SAMPLE_RATE = 48000

# ---------------------------------------------------------------- 几何常量
# 出图 1024² 方图（1.25x 预放大 -> 1280²，crop 9:16 窗 576x1024）
IMG_W = IMG_H = 1024
PRE_W = PRE_H = 1280          # 1.25x 预放大
CROP_W, CROP_H = 576, 1024    # 9:16 裁切窗（最小窗）
OUT_W, OUT_H = 720, 1280      # 成片竖屏 9:16

# 构图/framing 枚举（schema 契约）
FRAMINGS = ("center", "left", "right", "top", "bottom")
CAMERAS = ("zoom-in", "zoom-out", "pan-left", "pan-right", "static")
EASINGS = ("linear", "ease-in-out")


# ---------------------------------------------------------------- 基础校验
def assert_dim_16x(w: int, h: int, what: str = "dim") -> None:
    """分辨率必须为 16 的倍数（H.264 高档约束，方案 R5）。"""
    if w % 16 != 0 or h % 16 != 0:
        raise ValueError(f"{what}: {w}x{h} not multiple of 16")


def check_disk_gb(path, min_gb: float = 10.0) -> float:
    """落盘前查 path 所在磁盘剩余 >= min_gb GB，返回剩余 GB 数。

    不足抛 RuntimeError（方案 P2-9：批量出图前磁盘预检）。
    """
    p = Path(path)
    p.mkdir(parents=True, exist_ok=True)
    free_gb = shutil.disk_usage(str(p)).free / (1024 ** 3)
    if free_gb < min_gb:
        raise RuntimeError(
            f"disk space {free_gb:.2f}GB < required {min_gb}GB on {p}")
    return free_gb


# ---------------------------------------------------------------- 日志
def _log(level: str, msg: str) -> None:
    ts = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    print(f"[{ts}] [{level}] {msg}", flush=True)


def log_info(msg: str) -> None:
    _log("INFO", msg)


def log_warn(msg: str) -> None:
    _log("WARN", msg)


def log_error(msg: str) -> None:
    _log("ERROR", msg)


# ---------------------------------------------------------------- httpx
def make_session(timeout: float = 60.0) -> httpx.Client:
    """统一 httpx 会话工厂（中转 / ComfyUI 共用）。"""
    return httpx.Client(timeout=timeout)


# ---------------------------------------------------------------- subprocess
def subprocess_run(cmd: list, timeout: float = 300.0):
    """subprocess.run(列表参数) 统一封装，禁 shell 字符串。"""
    return subprocess.run(
        cmd, capture_output=True, text=True,
        encoding="utf-8", timeout=timeout)


# ---------------------------------------------------------------- ffprobe
def probe_duration(path, timeout: float = 30.0) -> float:
    """ffprobe 实测媒体时长（秒，毫秒精度）。"""
    r = subprocess.run(
        [FFPROBE, "-v", "error", "-show_entries", "format=duration",
         "-of", "default=noprint_wrappers=1:nokey=1", str(path)],
        capture_output=True, text=True, encoding="utf-8", timeout=timeout)
    if r.returncode != 0 or not r.stdout.strip():
        raise RuntimeError(f"ffprobe duration failed: {path}: {r.stderr}")
    return float(r.stdout.strip())


def probe_video_stream(path, timeout: float = 30.0) -> dict:
    """ffprobe 实测视频流参数（w/h/fps/pix_fmt/codec/duration）。"""
    r = subprocess.run(
        [FFPROBE, "-v", "error", "-select_streams", "v:0",
         "-show_entries",
         "stream=codec_name,width,height,pix_fmt,r_frame_rate",
         "-show_entries", "format=duration",
         "-of", "json", str(path)],
        capture_output=True, text=True, encoding="utf-8", timeout=timeout)
    if r.returncode != 0:
        raise RuntimeError(f"ffprobe stream failed: {path}: {r.stderr}")
    d = json_loads(r.stdout)
    st = d["streams"][0]
    st["duration"] = float(d["format"]["duration"])
    num, den = st["r_frame_rate"].split("/")
    st["fps"] = float(num) / float(den)
    return st


def json_loads(s: str):
    import json
    return json.loads(s)


def sys_exit(msg: str, code: int = 1) -> None:
    log_error(msg)
    sys.exit(code)
