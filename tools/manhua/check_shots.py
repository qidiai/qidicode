# -*- coding: utf-8 -*-
"""S3 质检：分镜图随机抽检（辅助标记，非门槛，方案 P1-2）。

- 随机抽 10%（至少 1 张，可复现：固定种子）
- base64 调中转 vision（model=agnes-2.5-pro-beta，
  OpenAI vision messages 格式）
- 输出乱字/鬼脸/水印/构图建议分（0-10）
- 结果写 shot_review.md 供人工参考

  ⚠ 辅助标记非门槛：vision 评分仅作人工终审的参考标记，
  不做硬性拦截；人工验收清单（方案 §2.4）为准。

用法：
    python check_shots.py [--project-dir DIR] [--seed 42]
"""
import argparse
import base64
import json
import random
import sys
from pathlib import Path

import common

REVIEW_PROMPT = (
    "你是分镜图质检员。检查这张动漫风格分镜图，按 JSON 输出："
    '{"garbled_text": 0-10乱字程度分(越高越差), '
    '"ghost_face": 0-10鬼脸/畸形脸程度分, '
    '"watermark": 0-10水印/签名/logo程度分, '
    '"composition": 0-10构图建议分(越高越好), '
    '"suggestions": "一句话改进建议（中文）"}。'
    "只输出 JSON。"
)


def call_vision(session, image_bytes: bytes) -> dict:
    """调中转 vision，返回解析后的评分 dict。"""
    b64 = base64.b64encode(image_bytes).decode()
    payload = {
        "model": common.RELAY_VISION_MODEL,
        "messages": [
            {"role": "user", "content": [
                {"type": "text", "text": REVIEW_PROMPT},
                {"type": "image_url", "image_url": {
                    "url": "data:image/png;base64," + b64}},
            ]},
        ],
        "max_tokens": 512,
    }
    r = session.post(
        f"{common.RELAY_BASE_URL}/chat/completions",
        json=payload)
    r.raise_for_status()
    content = r.json()["choices"][0]["message"]["content"]
    if content.strip().startswith("```"):
        content = content.split("\n", 1)[1]
        content = content.rsplit("```", 1)[0]
    return json.loads(content.strip())


def main() -> int:
    ap = argparse.ArgumentParser(description="分镜图抽检")
    ap.add_argument("--project-dir", default=r"qidiwork-docs\manhua\proj1")
    ap.add_argument("--seed", type=int, default=42,
                    help="随机种子（可复现）")
    ap.add_argument("--ratio", type=float, default=0.1,
                    help="抽检比例")
    args = ap.parse_args()

    proj = Path(args.project_dir)
    shots_dir = proj / "shots"
    with open(proj / "storyboard.json", "r", encoding="utf-8") as f:
        sb = json.load(f)
    shot_ids = [s["shot_id"] for s in sb.get("shots", [])]
    images = [p for p in shots_dir.glob("*.png")]
    if not images:
        common.log_error("shots/ 下无图片，先跑 gen_shots.py")
        return 1

    # 随机抽 10%（至少 1 张）
    rng = random.Random(args.seed)
    k = max(1, int(round(len(images) * args.ratio)))
    sampled = sorted(rng.sample(sorted(images), min(k, len(images))))
    common.log_info(f"抽检 {len(sampled)}/{len(images)} 张 "
                    f"(seed={args.seed})")

    results = []
    with common.make_session() as sess:
        for img in sampled:
            sid = img.stem
            try:
                scores = call_vision(sess, img.read_bytes())
                results.append({"shot_id": sid, **scores})
                common.log_info(
                    f"{sid}: 乱字={scores.get('garbled_text')} "
                    f"鬼脸={scores.get('ghost_face')} "
                    f"水印={scores.get('watermark')} "
                    f"构图={scores.get('composition')}")
            except Exception as e:  # noqa: BLE001
                common.log_warn(f"{sid}: vision 失败 err={e!r}")
                results.append({"shot_id": sid, "error": repr(e)})

    # 写 shot_review.md（辅助标记非门槛）
    lines = [
        "# 分镜图抽检报告（辅助标记，非门槛）",
        "",
        "> ⚠ 本报告由 vision 模型自动打分，仅作人工终审的",
        "> 参考标记，**不做硬性门槛**；",
        "> 人工验收清单（方案 §2.4：相似度/乱码/手指/线条/服化道）",
        "> 为准。",
        "",
        f"- 模型: {common.RELAY_VISION_MODEL}",
        f"- 抽检: {len(results)}/{len(images)} 张 "
        f"(seed={args.seed})",
        "",
        "| 镜头 | 乱字↑ | 鬼脸↑ | 水印↑ | 构图↓ | 建议 |",
        "|---|---|---|---|---|---|",
    ]
    for r in results:
        if "error" in r:
            lines.append(f"| {r['shot_id']} | ERR | ERR | ERR "
                         f"| ERR | {r['error'][:60]} |")
        else:
            lines.append(
                f"| {r['shot_id']} "
                f"| {r.get('garbled_text')} "
                f"| {r.get('ghost_face')} "
                f"| {r.get('watermark')} "
                f"| {r.get('composition')} "
                f"| {str(r.get('suggestions', ''))[:80]} |")
    lines += [
        "",
        "人工终审清单（逐项过，方案 §2.4）：",
        "1. 角色相似度目测 ≥80%",
        "2. 无乱码文字",
        "3. 无畸形手指",
        "4. 线条闭合",
        "5. 服化道与角色卡一致",
        "不合格 → 重生成队列（fallback 链见方案 §2.4）",
    ]
    out = proj / "shot_review.md"
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")
    common.log_info(f"抽检报告: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
