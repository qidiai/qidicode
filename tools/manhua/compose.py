# -*- coding: utf-8 -*-
"""S5: 合成成片（方案 §3.2 合成工程规范）。

每镜头：
  图(1024²) → scale 预放大（zoom 时逐帧动画 1.0x↔1.25x）
            → crop 9:16 窗（framing 基准 + camera 逐帧 offset）
            → scale=720:1280 → fade（转场）→ 帧数精确截断
  音：aresample=48000 + apad 到视频长度
镜头时长 = max(audio_sec + 0.5, min_duration_sec)
帧数 = ceil(镜头时长 × 30)（精确帧边界，方案 §3.2 对轴）

concat：demuxer + 统一重编码（同 fps/时间基/像素格式）。
字幕：ASS（距底 200px、白字黑描边、单行 ≤15 字自动折行；
      framing=bottom 时 MarginV 上移）。

framing 语义（观察项①，方图阶段）：
  center/left/right → crop x 基准偏移
  top/bottom → 仅提示词约束（出图阶段已生效），裁切退化 center
  并打 warning（原生竖屏出图通过后再启用裁切语义）

后置校验：ffprobe 实测成片时长 vs storyboard 累计时长，
误差 <100ms 否则 exit 1。

M6 I2V 混编：i2v=true 的镜头以 videos/{sid}.mp4 为视觉源
（运动已烘焙进视频，不再做肯布拉斯运镜），音轨仍用该镜头
配音（视频原生音轨丢弃）；统一 30fps/720x1280/sar=1 归一
后进 concat。视频时长不足需求帧数时由后置校验兜底报错。

用法：
    python compose.py [--project-dir DIR] [--out-name final_v2]
"""
import argparse
import json
import math
import sys
from pathlib import Path

import common

HERE = Path(__file__).resolve().parent
# 中间片段落 target/tmp/manhua/segments/<项目名>/（方案 §4）。
# 锚定到仓库根，避免依赖调用时的 CWD；按项目名分子目录避免多项目互踩。
REPO_ROOT = HERE.parent.parent
SEG_ROOT = REPO_ROOT / "target" / "tmp" / "manhua" / "segments"

FONT_NAME = "Source Han Sans CN"   # 思源黑体（本机已装）
FONT_FALLBACK = "Microsoft YaHei"
SUB_MARGIN_V = 200        # 距底 200px（方案 P2-3）
SUB_MARGIN_V_BOTTOM = 400  # framing=bottom 上移
SUB_LINE_MAX = 15          # 单行 ≤15 字
SUB_FONT_SIZE = 52
SUB_OUTLINE = 4


# ---------------------------------------------------------------- 时长/帧数
def shot_duration(shot: dict) -> float:
    """镜头时长 = max(audio_sec + 0.5, min_duration_sec)。"""
    audio_sec = shot.get("audio_sec") or 0.0
    return max(audio_sec + 0.5, float(shot["min_duration_sec"]))


def frame_count(dur: float) -> int:
    """帧数 = ceil(时长 × fps)，精确帧边界。"""
    return math.ceil(dur * common.FPS - 1e-9)


# ---------------------------------------------------------------- I2V 镜头（M6）
def build_i2v_filter(shot: dict, seg_dur: float,
                         fade_in: float, fade_out: float) -> str:
    """I2V 镜头 filter chain：运动已烘焙在视频里，
    不做 scale 预放大/crop 运镜，仅归一
    30fps / 720x1280 / sar=1，后接转场 fade。"""
    chain = (
        f"fps={common.FPS},"
        f"scale={common.OUT_W}:{common.OUT_H}"
        f":force_original_aspect_ratio=decrease"
        f":flags=lanczos,"
        f"pad={common.OUT_W}:{common.OUT_H}"
        f":(ow-iw)/2:(oh-ih)/2,setsar=1")
    if fade_in > 0:
        chain += f",fade=t=in:st=0:d={fade_in}"
    if fade_out > 0:
        chain += (f",fade=t=out:st={max(seg_dur - fade_out, 0)}"
                  f":d={fade_out}")
    chain += f",format={common.PIX_FMT}"
    return chain


# ---------------------------------------------------------------- 运镜表达式
def _t_guard() -> str:
    return "if(isnan(t),0,t)"


def easing_expr(easing: str, dur: float) -> str:
    """缓动曲线 ffmpeg 表达式（0 -> 1）。"""
    t = _t_guard()
    if easing == "linear":
        return f"{t}/{dur}"
    return f"0.5*(1-cos(PI*{t}/{dur}))"


def build_video_filter(shot: dict, seg_dur: float,
                         fade_in: float, fade_out: float) -> str:
    """组装单镜头视频 filter chain。"""
    if shot.get("i2v"):
        # M6：I2V 镜头运动已烘焙进视频，只归一
        return build_i2v_filter(shot, seg_dur,
                                fade_in, fade_out)

    camera = shot.get("camera", "static")
    easing = shot.get("camera_easing", "linear")
    framing = shot.get("framing", "center")
    ez = easing_expr(easing, seg_dur)

    # ---- scale 预放大（zoom 逐帧动画）----
    if camera == "zoom-in":
        # 1.0x -> 1.25x（Task C 验证配方）
        pre = (f"scale=w='iw*(1+0.25*{ez})'"
               f":h='ih*(1+0.25*{ez})'"
               f":eval=frame:flags=lanczos")
    elif camera == "zoom-out":
        # 1.25x -> 1.0x
        pre = (f"scale=w='iw*(1.25-0.25*{ez})'"
               f":h='ih*(1.25-0.25*{ez})'"
               f":eval=frame:flags=lanczos")
    else:
        pre = f"scale={common.PRE_W}:{common.PRE_H}" \
              f":flags=lanczos"

    # ---- crop：framing 定基准，camera 定 offset ----
    if framing == "left":
        x_base = "0"
    elif framing == "right":
        x_base = "(iw-ow)"
    elif framing in ("top", "bottom"):
        # 观察项①：方图阶段 top/bottom 裁切退化 center
        common.log_warn(
            f"{shot['shot_id']}: framing={framing} 在方图阶段"
            "仅作提示词约束，裁切退化 center（观察项①）")
        x_base = "(iw-ow)/2"
    else:
        x_base = "(iw-ow)/2"
    y_base = "(ih-oh)/2"  # 垂直方向始终居中（方图阶段）

    if camera == "pan-right":
        # 从 framing 基准匀速/缓动扫到最右
        x = f"{x_base}+((iw-ow)-({x_base}))*{ez}"
    elif camera == "pan-left":
        # 从 framing 基准扫到最左
        x = f"{x_base}-({x_base})*{ez}"
    else:
        x = x_base

    crop = (f"crop={common.CROP_W}:{common.CROP_H}"
            f":x='{x}':y='{y_base}'")

    chain = (f"fps={common.FPS},{pre},{crop},"
             f"scale={common.OUT_W}:{common.OUT_H}"
             f":flags=lanczos")

    # ---- 转场 fade（dip-to-black 近似，样片期）----
    if fade_in > 0:
        chain += f",fade=t=in:st=0:d={fade_in}"
    if fade_out > 0:
        chain += (f",fade=t=out:st={max(seg_dur - fade_out, 0)}"
                  f":d={fade_out}")
    chain += f",format={common.PIX_FMT}"
    return chain


# ---------------------------------------------------------------- 字幕
def wrap_subtitle(text: str) -> str:
    r"""单行 ≤15 字自动折行（ASS \N）。"""
    text = text.strip()
    if not text:
        return ""
    lines = []
    while len(text) > SUB_LINE_MAX:
        cut = text.rfind("，", 0, SUB_LINE_MAX)
        cut = cut if cut >= SUB_LINE_MAX // 2 else -1
        if cut < 0:
            cut = text.rfind("。", 0, SUB_LINE_MAX)
            cut = cut if cut >= SUB_LINE_MAX // 2 else -1
        if cut < 0:
            cut = SUB_LINE_MAX
        lines.append(text[:cut + 1])
        text = text[cut + 1:]
    if text:
        lines.append(text)
    return "\\N".join(lines)


def ass_time(sec: float) -> str:
    """秒 -> ASS H:MM:SS.cc。"""
    cs = int(round(sec * 100))
    h = cs // 360000
    cs %= 360000
    m = cs // 6000
    cs %= 6000
    s = cs // 100
    c = cs % 100
    return f"{h}:{m:02d}:{s:02d}.{c:02d}"


def build_ass(sb: dict, seg_durs: list) -> str:
    """生成整片 ASS 字幕。"""
    header = (
        "[Script Info]\n"
        "ScriptType: v4.00+\n"
        f"PlayResX: {common.OUT_W}\n"
        f"PlayResY: {common.OUT_H}\n"
        "ScaledBorderAndShadow: yes\n\n"
        "[V4+ Styles]\n"
        "Format: Name, Fontname, Fontsize, PrimaryColour, "
        "SecondaryColour, OutlineColour, BackColour, Bold, "
        "Italic, Underline, StrikeOut, ScaleX, ScaleY, "
        "Spacing, Angle, BorderStyle, Outline, Shadow, "
        "Alignment, MarginL, MarginR, MarginV, Encoding\n"
        f"Style: Default,{FONT_NAME},{SUB_FONT_SIZE},"
        "&H00FFFFFF,&H000000FF,&H00000000,&H00000000,"
        "0,0,0,0,100,100,0,0,1,"
        f"{SUB_OUTLINE},0,2,60,60,{SUB_MARGIN_V},1\n\n"
        "[Events]\n"
        "Format: Layer, Start, End, Style, MarginL, MarginR, "
        "MarginV, Effect, Text\n"
    )
    events = []
    t0 = 0.0
    for shot, seg_dur in zip(sb["shots"], seg_durs):
        text = shot.get("subtitle") or ""
        if not text:
            text = " ".join(
                d.get("line", "")
                for d in shot.get("dialogue") or [])
        text = wrap_subtitle(text)
        if text:
            margin_v = (SUB_MARGIN_V_BOTTOM
                        if shot.get("framing") == "bottom"
                        else SUB_MARGIN_V)
            events.append(
                f"Dialogue: 0,{ass_time(t0)},"
                f"{ass_time(t0 + seg_dur)},"
                f"Default,,0,0,{margin_v},,{text}")
        t0 += seg_dur
    return header + "\n".join(events) + "\n"


# ---------------------------------------------------------------- 主流程
def main() -> int:
    ap = argparse.ArgumentParser(description="合成成片")
    ap.add_argument("--project-dir", default=r"qidiwork-docs\manhua\proj1")
    ap.add_argument("--out-name", default="final",
                    help="成片文件名前缀（final_v2 等）")
    args = ap.parse_args()

    proj = Path(args.project_dir)
    with open(proj / "storyboard.json", "r", encoding="utf-8") as f:
        sb = json.load(f)

    shots = sb.get("shots", [])
    SEG_DIR = SEG_ROOT / proj.name
    SEG_DIR.mkdir(parents=True, exist_ok=True)

    # 转场表：after_shot -> (type, duration)
    trans = {}
    for tr in sb.get("transitions") or []:
        trans[tr["after_shot"]] = tr

    # ---- 逐镜头渲染中间片段 ----
    seg_files = []
    seg_durs = []
    for i, shot in enumerate(shots):
        sid = shot["shot_id"]
        # ---- 视觉源：I2V 视频 或 静帧图 ----
        if shot.get("i2v"):
            visual = proj / "videos" / f"{sid}.mp4"
            if not visual.exists():
                common.log_error(
                    f"{sid}: i2v 视频缺失 {visual}，"
                    "先跑 gen_video.py")
                return 1
        else:
            visual = proj / "shots" / f"{sid}.png"
            if not visual.exists():
                common.log_error(
                    f"{sid}: 分镜图缺失 {visual}，"
                    "先跑 gen_shots.py")
                return 1
        audio = proj / "audio" / f"{sid}.mp3"
        has_audio = audio.exists()

        shot_dur = shot_duration(shot)
        n = frame_count(shot_dur)
        seg_dur = n / common.FPS
        seg_durs.append(seg_dur)

        # 转场 fade
        fade_out = 0.0
        tr = trans.get(sid)
        if tr:
            if tr.get("type") == "fade":
                fade_out = float(tr.get("duration", 0.3))
            else:
                common.log_warn(
                    f"{sid}: 转场类型 '{tr.get('type')}' "
                    "样片期仅支持 fade，跳过")
        fade_in = 0.0
        if i > 0:
            prev_tr = trans.get(shots[i - 1]["shot_id"])
            if prev_tr and prev_tr.get("type") == "fade":
                fade_in = float(prev_tr.get("duration", 0.3))

        vf = build_video_filter(shot, seg_dur, fade_in, fade_out)
        seg = SEG_DIR / f"seg_{sid}.mp4"

        cmd = [common.FFMPEG, "-y"]
        if shot.get("i2v"):
            # I2V 视频源：原生音轨丢弃（显式映射只取
            # filtergraph 的 [v]/[a]，音轨用该镜头配音）
            cmd += ["-i", str(visual)]
        else:
            cmd += [
                "-loop", "1",
                "-framerate", str(common.FPS),
                "-i", str(visual),
            ]
        if has_audio:
            cmd += ["-i", str(audio)]
            afilter = "[1:a]aresample=" \
                      f"{common.A_SAMPLE_RATE},apad[a]"
        else:
            cmd += ["-f", "lavfi", "-t", str(seg_dur), "-i",
                    f"anullsrc=r={common.A_SAMPLE_RATE}:cl=stereo"]
            afilter = "[1:a]anull[a]"

        cmd += [
            "-filter_complex", f"[0:v]{vf}[v];{afilter}",
            # 必须映射 filtergraph 输出（[v]/[a]），
            # 映射原始输入会绕过滤波导致输出未连接
            "-map", "[v]", "-map", "[a]",
            "-c:v", common.V_CODEC, "-crf", common.CRF,
            "-preset", common.PRESET,
            "-profile:v", common.PROFILE, "-level:v", common.LEVEL,
            "-pix_fmt", common.PIX_FMT, "-r", str(common.FPS),
            "-c:a", common.A_CODEC, "-b:a", common.A_BITRATE,
            "-frames:v", str(n), "-shortest",
            str(seg),
        ]
        common.log_info(
            f"{sid}: {seg_dur:.3f}s -> {n} 帧 "
            f"(audio={has_audio}, "
            f"src={'i2v' if shot.get('i2v') else 'img'})")
        r = common.subprocess_run(cmd, timeout=900)
        if r.returncode != 0:
            common.log_error(f"{sid}: 渲染失败\n{r.stderr[-1500:]}")
            return 1
        seg_files.append(seg)

    # ---- concat demuxer + 统一重编码 ----
    list_file = SEG_DIR / "concat.txt"
    list_file.write_text(
        "\n".join(f"file '{s.name}'" for s in seg_files),
        encoding="utf-8")
    out_dir = proj / "output"
    out_dir.mkdir(parents=True, exist_ok=True)
    out_name = args.out_name or "final"
    final = out_dir / f"{out_name}.mp4"
    final_sub = out_dir / f"{out_name}_sub.mp4"

    cmd = [
        common.FFMPEG, "-y",
        "-f", "concat", "-safe", "0", "-i", str(list_file),
        "-c:v", common.V_CODEC, "-crf", common.CRF,
        "-preset", common.PRESET,
        "-profile:v", common.PROFILE, "-level:v", common.LEVEL,
        "-pix_fmt", common.PIX_FMT, "-r", str(common.FPS),
        "-c:a", common.A_CODEC, "-b:a", common.A_BITRATE,
        "-ar", str(common.A_SAMPLE_RATE),
        str(final),
    ]
    common.log_info(f"concat -> {final}")

    r = common.subprocess_run(cmd, timeout=1800)
    if r.returncode != 0:
        common.log_error(f"concat 失败\n{r.stderr[-1500:]}")
        return 1

    # ---- ASS 字幕 ----
    ass = proj / "subtitle.ass"
    ass.write_text(build_ass(sb, seg_durs), encoding="utf-8")
    # subtitles 滤镜的路径在 filtergraph 内解析：Windows 盘符冒号
    # 会被当成选项分隔符（subtitles=G:/x → filename=G），必须转义为
    # 'G\:/x'（引号内转义冒号）。相对路径无盘符则不受影响。
    ass_arg = ass.as_posix().replace(":", "\\:")
    cmd = [
        common.FFMPEG, "-y", "-i", str(final),
        "-vf", f"subtitles='{ass_arg}'",
        "-c:v", common.V_CODEC, "-crf", common.CRF,
        "-preset", common.PRESET,
        "-profile:v", common.PROFILE, "-level:v", common.LEVEL,
        "-pix_fmt", common.PIX_FMT,
        "-c:a", "copy",
        str(final_sub),
    ]
    common.log_info(f"烧录字幕 {ass}")
    r = common.subprocess_run(cmd, timeout=1800)
    if r.returncode != 0:
        common.log_error(f"字幕烧录失败\n{r.stderr[-1500:]}")
        return 1

    # ---- 后置校验：实测时长 vs 累计时长 <100ms ----
    cumulative = sum(seg_durs)
    st = common.probe_video_stream(final)
    err_ms = abs(st["duration"] - cumulative) * 1000
    common.log_info(
        f"后置校验: 实测 {st['duration']:.3f}s vs "
        f"累计 {cumulative:.3f}s, 误差 {err_ms:.1f}ms")
    if err_ms >= 100:
        common.log_error("时长误差 >=100ms，校验失败")
        return 1
    common.log_info(
        f"成片完成: {final_sub} "
        f"({st['width']}x{st['height']}, {st['fps']}fps, "
        f"{st['duration']:.3f}s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
