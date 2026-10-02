# -*- coding: utf-8 -*-
"""S4: edge-tts 逐镜头配音 + 时长回填 storyboard。

- 每条 dialogue/narration 调 edge-tts（python API），
  音色/rate/pitch 从 characters[] 读，失败自动切 tts_fallback
- 429/网络错误指数退避（1s/2s/4s）
- mp3 落 audio/，段间 0.15s 静音Gap，拼接为整镜音轨
  audio/{shot_id}.mp3（统一重采样 48kHz，方案 §3）
- ffprobe 实测时长回填 storyboard 每镜头 audio_sec
  （镜头时长 = max(audio_sec + 0.5, min_duration_sec)，
  取 max 逻辑在 compose 侧执行）

用法：
    python gen_tts.py [--project-dir DIR]
"""
import argparse
import asyncio
import json
import sys
import time
from pathlib import Path

import edge_tts

import common

GAP_SEC = 0.15          # 段间静音
GAP_FILE = "_gap.mp3"   # 静音Gap缓存名


async def _tts_once(text: str, voice: str, rate: str, pitch: str,
                    out: Path) -> None:
    comm = edge_tts.Communicate(text, voice, rate=rate, pitch=pitch)
    await comm.save(str(out))


async def synth_segment(text: str, char: dict, out: Path) -> dict:
    """合成单段：主音色失败退避 3 次，再切 tts_fallback 3 次。

    返回实际使用的音色信息。
    """
    voices = [
        (char.get("tts_voice"), "primary"),
        (char.get("tts_fallback"), "fallback"),
    ]
    for voice, kind in voices:
        if not voice:
            continue
        for attempt in range(3):
            try:
                await _tts_once(
                    text, voice,
                    char.get("tts_rate", "+0%"),
                    char.get("tts_pitch", "+0Hz"),
                    out)
                common.log_info(
                    f"TTS {out.name} voice={voice}({kind}) "
                    f"size={out.stat().st_size}B")
                return {"voice": voice, "kind": kind}
            except Exception as e:  # noqa: BLE001  退避重试
                wait = 2 ** attempt
                common.log_warn(
                    f"TTS 失败 {out.name} voice={voice} "
                    f"attempt={attempt + 1} err={e!r} "
                    f"退避 {wait}s")
                time.sleep(wait)
        common.log_warn(f"音色 {voice} 不可用，切 "
                        f"tts_fallback 重试")
    raise RuntimeError(f"TTS 全部失败: {out.name}")


def make_gap(audio_dir: Path) -> Path:
    """生成 0.15s 静音 mp3 作段间 Gap。"""
    gap = audio_dir / GAP_FILE
    if not gap.exists():
        r = common.subprocess_run(
            [common.FFMPEG, "-y", "-f", "lavfi", "-i",
             f"anullsrc=r={common.A_SAMPLE_RATE}:cl=mono",
             "-t", str(GAP_SEC), "-c:a", "libmp3lame",
             str(gap)])
        if r.returncode != 0:
            raise RuntimeError("生成静音 Gap 失败")
    return gap


def concat_shot_audio(segments: list, gap: Path,
                      out: Path) -> float:
    """段间插静音Gap，concat + 重采样 48kHz，返回实测时长。"""
    list_file = out.with_suffix(".txt")
    lines = []
    for i, seg in enumerate(segments):
        lines.append(f"file '{seg.name}'")
        if i < len(segments) - 1:
            lines.append(f"file '{gap.name}'")
    list_file.write_text("\n".join(lines), encoding="utf-8")
    r = common.subprocess_run(
        [common.FFMPEG, "-y", "-f", "concat", "-safe", "0",
         "-i", str(list_file),
         "-ar", str(common.A_SAMPLE_RATE),
         "-c:a", "libmp3lame", "-b:a", "128k", str(out)])
    if r.returncode != 0:
        raise RuntimeError(f"拼接音轨失败: {out.name}")
    list_file.unlink(missing_ok=True)
    return common.probe_duration(out)


def process_shot(shot: dict, char_by_id: dict,
                 audio_dir: Path) -> float:
    """单镜头配音，返回 audio_sec（无台词为 0.0）。"""
    sid = shot["shot_id"]
    segments = []  # (Path, meta)
    tasks = []
    for i, d in enumerate(shot.get("dialogue") or []):
        char = char_by_id.get(d["character"])
        if char is None:
            raise RuntimeError(
                f"{sid}: dialogue 引用未知角色 {d['character']}")
        p = audio_dir / f"{sid}_d{i}_{d['character']}.mp3"
        tasks.append((d["line"], char, p, f"d{i}"))
    if shot.get("narration"):
        # 旁白用第一个角色音色（可后续扩展独立旁白音色）
        first_char = char_by_id.get(
            (shot.get("dialogue") or [{}])[0].get("character"))
        p = audio_dir / f"{sid}_n.mp3"
        if first_char is not None:
            tasks.append((shot["narration"], first_char, p, "n"))
        else:
            common.log_warn(f"{sid}: 有旁白但无角色可借音色，"
                            "跳过旁白")

    if not tasks:
        common.log_info(f"{sid}: 无台词/旁白，audio_sec=0")
        return 0.0

    for text, char, p, tag in tasks:
        meta = asyncio.run(synth_segment(text, char, p))
        segments.append((p, meta))

    combined = audio_dir / f"{sid}.mp3"
    audio_sec = concat_shot_audio(
        [p for p, _ in segments], make_gap(audio_dir), combined)
    common.log_info(f"{sid}: 音轨 {combined.name} "
                    f"audio_sec={audio_sec:.3f}s "
                    f"({len(segments)} 段)")
    # 回填 storyboard：段明细 + 实测时长
    shot["audio_sec"] = round(audio_sec, 3)
    shot["audio_segments"] = [
        {"file": p.name, "sec": round(common.probe_duration(p), 3),
         "voice": m["voice"], "role": tag}
        for (p, m), tag in zip(segments,
                               [t[3] for t in tasks])]
    return audio_sec


def main() -> int:
    ap = argparse.ArgumentParser(description="edge-tts 配音")
    ap.add_argument("--project-dir", default=r"qidiwork-docs\manhua\proj1")
    args = ap.parse_args()

    proj = Path(args.project_dir)
    sb_path = proj / "storyboard.json"
    audio_dir = proj / "audio"
    audio_dir.mkdir(parents=True, exist_ok=True)

    with open(sb_path, "r", encoding="utf-8") as f:
        sb = json.load(f)
    char_by_id = {c["id"]: c for c in sb.get("characters", [])}

    total = 0.0
    for shot in sb.get("shots", []):
        sec = process_shot(shot, char_by_id, audio_dir)
        total += sec
    with open(sb_path, "w", encoding="utf-8") as f:
        json.dump(sb, f, ensure_ascii=False, indent=2)
    common.log_info(f"配音完成：{len(sb['shots'])} 镜头，"
                    f"总配音 {total:.1f}s，audio_sec 已回填 "
                    f"{sb_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
