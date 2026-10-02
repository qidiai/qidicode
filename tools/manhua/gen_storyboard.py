# -*- coding: utf-8 -*-
"""S1: 生成分镜草稿 storyboard.json（方案 §2.2 schema v2）。

优先级：storyboard.override.yaml（人工覆盖，与 JSON 同 schema，人工拍板）
        > 中转 /chat/completions 生成草稿（LLM 只产草稿）

- model 可参数化（--model / 环境变量 MANHUA_STORYBOARD_MODEL，
  默认 GLM-5.3-FLASH；注意池内无字面 "GLM-5.3"）
- 指数退避 1s/2s/4s，连续 3 次失败熔断退出并给人工指引（方案 §2.5）
- 手写 schema 校验：必填字段 / framing / camera / easing 枚举 /
  角色引用存在性 / 输出分辨率 16 倍数

用法：
    python gen_storyboard.py [--project-dir DIR] [--model M]
"""
import argparse
import json
import sys
import time
from pathlib import Path

import yaml

import common


# ---------------------------------------------------------------- schema 校验
def validate_storyboard(sb: dict) -> list:
    """手写 schema 校验，返回错误列表（空 = 通过）。"""
    errs = []

    def need(d, key, where, typ=str):
        if key not in d or d[key] in (None, ""):
            errs.append(f"{where}: missing required field '{key}'")
        elif not isinstance(d[key], typ):
            if isinstance(typ, type):
                name = typ.__name__
            else:
                name = " or ".join(t.__name__ for t in typ)
            errs.append(f"{where}: field '{key}' should be {name}")

    if not isinstance(sb, dict):
        return ["storyboard: not a dict"]
    need(sb, "project", "storyboard")
    need(sb, "aspect", "storyboard")
    if sb.get("aspect") not in ("9:16",):
        errs.append("storyboard: aspect must be '9:16'（当前仅支持竖屏）")
    need(sb, "fps", "storyboard", int)
    if sb.get("fps") != common.FPS:
        errs.append(f"storyboard: fps must be {common.FPS}")

    chars = sb.get("characters")
    if not isinstance(chars, list) or not chars:
        errs.append("storyboard: characters must be non-empty list")
    else:
        for i, c in enumerate(chars):
            where = f"characters[{i}]"
            for k in ("id", "name", "card", "ref_sheet",
                      "tts_voice", "tts_fallback",
                      "tts_rate", "tts_pitch"):
                need(c, k, where)
            if len(chars) > 5:
                errs.append("storyboard: 主要角色 ≤5（方案 §2.3），"
                            "超出请用 fallback 音色")

    shots = sb.get("shots")
    if not isinstance(shots, list) or not shots:
        errs.append("storyboard: shots must be non-empty list")
    else:
        char_ids = {c.get("id") for c in (chars or [])}
        for i, s in enumerate(shots):
            where = f"shots[{i}]"
            shot_types = {
                "shot_id": str, "image_prompt": str, "use_ref": list,
                "framing": str, "camera": str, "camera_easing": str,
                "dialogue": list, "min_duration_sec": (int, float),
            }
            for k, typ in shot_types.items():
                need(s, k, where, typ)
            if s.get("framing") not in common.FRAMINGS:
                errs.append(f"{where}: framing '{s.get('framing')}' "
                            f"not in {common.FRAMINGS}")
            if s.get("camera") not in common.CAMERAS:
                errs.append(f"{where}: camera '{s.get('camera')}' "
                            f"not in {common.CAMERAS}")
            if s.get("camera_easing") not in common.EASINGS:
                errs.append(f"{where}: camera_easing "
                            f"'{s.get('camera_easing')}' "
                            f"not in {common.EASINGS}")
            if not isinstance(s.get("use_ref"), list):
                errs.append(f"{where}: use_ref must be list")
            else:
                for rid in s["use_ref"]:
                    if rid not in char_ids:
                        errs.append(f"{where}: use_ref '{rid}' "
                                    "not in characters[]")
            dlg = s.get("dialogue")
            if isinstance(dlg, list):
                for j, d in enumerate(dlg):
                    if d.get("character") not in char_ids:
                        errs.append(f"{where}.dialogue[{j}]: character "
                                    f"'{d.get('character')}' not in "
                                    "characters[]")
                    need(d, "line", f"{where}.dialogue[{j}]")
            mds = s.get("min_duration_sec")
            if isinstance(mds, (int, float)) and mds <= 0:
                errs.append(f"{where}: min_duration_sec must be > 0")

    common.assert_dim_16x(common.OUT_W, common.OUT_H, "output")
    return errs


def normalize_storyboard(sb: dict) -> dict:
    """补默认值，保证下游契约完整。"""
    sb.setdefault("transitions", [])
    for s in sb.get("shots", []):
        s.setdefault("narration", "")
        s.setdefault("sfx", "")
        s.setdefault("subtitle", "")
    return sb


# ---------------------------------------------------------------- LLM 草稿
_SYS = "你是分镜师。按用户要求输出 JSON 分镜，不要输出 JSON 之外的任何内容。"


def _llm_prompt(brief: str) -> str:
    return (
        "生成漫剧分镜 JSON（schema v2，务必严格遵守）：\n"
        "{\n"
        '  "project": "proj1", "aspect": "9:16", "fps": 30,\n'
        '  "characters": [{"id": "CH-01", "name": "角色名", '
        '"card": "character-cards/cards/ch-01.json", '
        '"ref_sheet": "character-cards/images/ch01-sheet.png", '
        '"tts_voice": "zh-CN-XiaoxiaoNeural", '
        '"tts_fallback": "zh-CN-XiaoyiNeural", '
        '"tts_rate": "+0%", "tts_pitch": "+0Hz"}],\n'
        '  "shots": [{"shot_id": "S01", '
        '"image_prompt": "英文提示词（角色外观锁定短语由脚本拼接，"'
        '此处只写场景/动作/景别）", "use_ref": ["CH-01"], '
        '"framing": "center|left|right|top|bottom", '
        '"camera": "zoom-in|zoom-out|pan-left|pan-right|static", '
        '"camera_easing": "linear|ease-in-out", '
        '"dialogue": [{"character": "CH-01", "line": "中文台词"}], '
        '"narration": "旁白可空", "sfx": "可空", '
        '"min_duration_sec": 3.0, "subtitle": "画面字幕可空"}],\n'
        '  "transitions": [{"after_shot": "S01", "type": "fade", '
        '"duration": 0.3}]\n'
        "}\n"
        "构图约束：主体集中在中央 576x1024 竖屏安全区（9:16），"
        "image_prompt 末尾追加构图约束短语。\n"
        f"创意要求：{brief}\n"
        "只输出 JSON。"
    )


def _extract_json(text: str) -> dict:
    """从 LLM 回复中提取 JSON（容忍 markdown 代码围栏）。"""
    t = text.strip()
    if t.startswith("```"):
        t = t.split("\n", 1)[1] if "\n" in t else t[3:]
        t = t.rsplit("```", 1)[0]
    return json.loads(t)


def llm_gen_storyboard(brief: str, model: str) -> dict:
    """调中转 /chat/completions 生成分镜草稿；指数退避 3 次后熔断。"""
    url = f"{common.RELAY_BASE_URL}/chat/completions"
    payload = {
        "model": model,
        "messages": [
            {"role": "system", "content": _SYS},
            {"role": "user", "content": _llm_prompt(brief)},
        ],
        # 中转推理模型 max_tokens 过小会返回空 content（实测）
        "max_tokens": 8192,
    }
    last_err = None
    for attempt in range(3):
        try:
            with common.make_session() as sess:
                r = sess.post(url, json=payload)
                r.raise_for_status()
                data = r.json()
            content = data["choices"][0]["message"]["content"]
            sb = _extract_json(content)
            common.log_info(f"LLM 草稿生成成功 model={model}")
            return sb
        except Exception as e:  # noqa: BLE001  退避重试
            last_err = e
            wait = 2 ** attempt  # 1s / 2s / 4s
            common.log_warn(f"LLM 调用失败 attempt={attempt + 1} "
                            f"err={e!r} 退避 {wait}s")
            time.sleep(wait)
    # 熔断：连续 3 次失败，给人工指引（方案 §2.5）
    common.log_error(
        "LLM 分镜生成熔断（连续 3 次失败）。\n"
        "人工指引：\n"
        "  1) 检查中转可达性：curl "
        f"{common.RELAY_BASE_URL}/models\n"
        "  2) 手工编写 storyboard.override.yaml（schema 同 JSON）"
        " 放到项目目录，\n"
        "     脚本会优先采用人工覆盖（YAML 可绕过 LLM）；\n"
        f"  3) 或换模型重试 --model（当前 {model}）。\n"
        f"最后错误：{last_err!r}")
    sys.exit(1)


# ---------------------------------------------------------------- main
def main() -> int:
    ap = argparse.ArgumentParser(description="生成分镜 storyboard.json")
    ap.add_argument("--project-dir", default=r"qidiwork-docs\manhua\proj1",
                    help="项目工作区（ASCII 路径）")
    ap.add_argument("--brief", default="30 秒竖屏漫剧样片：重逢戏，"
                                       "女主人公在雨夜车站等三年未见的恋人",
                    help="创意简报（仅 LLM 路线）")
    ap.add_argument("--model", default=common.RELAY_STORYBOARD_MODEL,
                    help="中转模型 id")
    args = ap.parse_args()

    proj = Path(args.project_dir)
    proj.mkdir(parents=True, exist_ok=True)

    override = proj / "storyboard.override.yaml"
    out = proj / "storyboard.json"

    if override.exists():
        common.log_info(f"发现人工覆盖 {override}，优先生效")
        with open(override, "r", encoding="utf-8") as f:
            sb = yaml.safe_load(f)
        source = "override-yaml"
    else:
        sb = llm_gen_storyboard(args.brief, args.model)
        source = f"llm:{args.model}"

    sb = normalize_storyboard(sb)
    errs = validate_storyboard(sb)
    if errs:
        for e in errs:
            common.log_error(f"schema 校验失败: {e}")
        common.log_error(
            "人工指引：修正 storyboard.override.yaml 或重跑 LLM 草稿")
        return 1

    with open(out, "w", encoding="utf-8") as f:
        json.dump(sb, f, ensure_ascii=False, indent=2)
    common.log_info(f"storyboard.json 已落盘（来源 {source}，"
                    f"{len(sb['shots'])} 镜头）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
