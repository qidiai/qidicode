# -*- coding: utf-8 -*-
"""M6: agnes-video-2.5 图生视频（I2V）批量生成。

读 storyboard 中 i2v=true 的镜头，逐镜：
  POST /v1/videos 创建（mode=keyframe, image=公网URL）
  → GET /agnesapi 轮询（20s 间隔，单镜总超时 20 分钟）
  → completed 后从 tolerance.url 下载 mp4 到 videos/{sid}.mp4
  → ffprobe 验证（h264 / 9:16 / 时长≈seconds 且 ≥ 混编需求时长）

已知坑（内置退避）：
  - 创建失败（429/503/网络）重试 5 次，指数退避 10/30/60/120s
    （429 中转自动轮换 key，客户端重试即过；503 video_queue_full 重试即过）
  - 轮询偶发 404「任务不存在」容忍退避（多节点复制延迟），
    连 3 次 404 才告警，继续轮询
  - seconds 必须字符串 "4"-"12"

成本：seconds × 0.15 元/镜，5 镜预估 ¥4.5；累计超 ¥8 立即中止。

断点续传：状态落 proj1/videos_state.json
  done   → 跳过（mp4 校验存在）
  created→ 续轮询（已有 video_id）
  failed → 重跑

用法：
    python gen_video.py [--project-dir DIR] [--only S04,S11] [--dry-run]
"""
import argparse
import json
import sys
import time
from datetime import datetime
from pathlib import Path

import httpx

import common

# ---------------------------------------------------------------- 常量
BASE = "https://luyou.qidiai.ltd:8443"
CREATE_URL = BASE + "/v1/videos"
POLL_URL = BASE + "/agnesapi"
MODEL = "agnes-video-2.5"

COST_PER_SEC = 0.15      # 元/秒
BUDGET_LIMIT = 8.0       # 累计成本闸（元）

CREATE_RETRIES = 5
CREATE_BACKOFF = [10, 30, 60, 120]   # 指数退避（秒）
POLL_INTERVAL = 20.0                 # 轮询间隔（秒）
POLL_TIMEOUT = 1200.0                # 单镜总超时 20 分钟
POLL_404_ALERT = 3                   # 连续 N 次 404 才告警

DURATION_TOLERANCE = 1.5   # 时长 vs seconds 允许偏差（秒，下限侧）
DURATION_TOLERANCE_UP = 3.0  # 上限侧（封装/容器开销）


# ---------------------------------------------------------------- 状态
def load_state(proj: Path) -> dict:
    p = proj / "videos_state.json"
    if p.exists():
        with open(p, "r", encoding="utf-8") as f:
            return json.load(f)
    return {"shots": {}}


def save_state(proj: Path, state: dict) -> None:
    """原子写盘（tmp + rename），防中断写残。"""
    p = proj / "videos_state.json"
    tmp = p.with_suffix(".json.tmp")
    with open(tmp, "w", encoding="utf-8", newline="\n") as f:
        json.dump(state, f, ensure_ascii=False, indent=2)
        f.write("\n")
    tmp.replace(p)


# ---------------------------------------------------------------- 响应解析
def _sources(d: dict) -> list:
    return [d, d.get("data") or {}, d.get("result") or {}]


def extract_video_id(d: dict):
    for src in _sources(d):
        v = (src.get("video_id") or src.get("id")
             or src.get("task_id"))
        if v:
            return str(v)
    return None


def extract_status(d: dict) -> str:
    for src in _sources(d):
        s = (src.get("status") or src.get("task_status")
             or src.get("state"))
        if s:
            return str(s).lower()
    return ""


def extract_mp4_url(d: dict):
    for src in _sources(d):
        tol = src.get("tolerance") or {}
        if isinstance(tol, dict):
            u = (tol.get("url") or tol.get("mp4")
                 or tol.get("video_url"))
            if u:
                return str(u)
        u = (src.get("url") or src.get("mp4_url")
             or src.get("video_url"))
        if u:
            return str(u)
    return None


# ---------------------------------------------------------------- API
def create_video(session: httpx.Client, shot: dict,
                 image_url: str) -> str:
    """创建 I2V 任务，返回 video_id（5 次指数退避）。"""
    body = {
        "model": MODEL,
        "prompt": shot["video_prompt"],
        "mode": "keyframe",
        "image": image_url,
        "seconds": str(shot["video_seconds"]),
        "size": "720P",
        "aspect_ratio": "9:16",
    }
    last = None
    for attempt in range(CREATE_RETRIES):
        try:
            r = session.post(CREATE_URL, json=body)
            if r.status_code == 429 or r.status_code >= 500:
                raise httpx.HTTPStatusError(
                    f"retryable {r.status_code}",
                    request=r.request, response=r)
            r.raise_for_status()
            d = r.json()
            vid = extract_video_id(d)
            if not vid:
                raise RuntimeError(
                    f"响应无 video_id: {json.dumps(d,
                                                     ensure_ascii=False)[:500]}")
            return vid
        except (httpx.HTTPError, RuntimeError) as e:
            last = e
            if attempt < CREATE_RETRIES - 1:
                wait = CREATE_BACKOFF[attempt]
                common.log_warn(
                    f"{shot['shot_id']}: 创建失败 "
                    f"attempt={attempt + 1} err={e!r} "
                    f"退避 {wait}s")
                time.sleep(wait)
    raise RuntimeError(
        f"创建失败 {CREATE_RETRIES} 次: {last!r}")


def poll_video(session: httpx.Client, video_id: str,
               shot_id: str) -> str:
    """轮询到 completed，返回 mp4 下载地址。"""
    deadline = time.monotonic() + POLL_TIMEOUT
    consec_404 = 0
    n = 0
    while True:
        if time.monotonic() >= deadline:
            raise TimeoutError(
                f"{shot_id}: 轮询超时 "
                f"({POLL_TIMEOUT / 60:.0f} 分钟, video_id={video_id})")
        try:
            r = session.get(POLL_URL, params={
                "video_id": video_id,
                "model_name": MODEL,
            })
            if r.status_code == 404:
                # 已知坑：多节点复制延迟偶发 404，容忍退避
                consec_404 += 1
                if consec_404 == POLL_404_ALERT:
                    common.log_warn(
                        f"{shot_id}: 连续 {consec_404} 次 404 "
                        "「任务不存在」（多节点复制延迟，"
                        "继续容忍退避）")
            else:
                consec_404 = 0
                if r.status_code == 429 or r.status_code >= 500:
                    common.log_warn(
                        f"{shot_id}: poll {r.status_code}，"
                        "退避重试")
                else:
                    r.raise_for_status()
                    d = r.json()
                    st = extract_status(d)
                    if st in ("completed", "succeeded",
                              "success"):
                        url = extract_mp4_url(d)
                        if not url:
                            raise RuntimeError(
                                "completed 但无 url: "
                                + json.dumps(
                                    d, ensure_ascii=False)
                                [:500])
                        return url
                    if st in ("failed", "error"):
                        raise RuntimeError(
                            "任务失败: "
                            + json.dumps(
                                d, ensure_ascii=False)
                            [:500])
                    n += 1
                    if n % 15 == 1:
                        common.log_info(
                            f"{shot_id}: status={st} "
                            f"({n} 次轮询)")
        except httpx.HTTPStatusError as e:
            if e.response.status_code in (429,) or \
                    e.response.status_code >= 500:
                common.log_warn(
                    f"{shot_id}: poll {e.response.status_code}"
                    "，退避重试")
            else:
                raise
        except httpx.HTTPError as e:
            common.log_warn(
                f"{shot_id}: poll 网络错误 {e!r}，"
                "退避重试")
        time.sleep(POLL_INTERVAL)


def download_mp4(session: httpx.Client, url: str,
                 dest: Path) -> None:
    with session.stream("GET", url) as r:
        r.raise_for_status()
        with open(dest, "wb") as f:
            for chunk in r.iter_bytes(1 << 20):
                f.write(chunk)


# ---------------------------------------------------------------- 校验
def verify_mp4(path: Path, seconds_str: str,
               needed_dur: float, shot_id: str) -> dict:
    """ffprobe 验证：h264 / 9:16 / 时长≈seconds 且 ≥ 混编需求。"""
    st = common.probe_video_stream(str(path))
    dur = float(st["duration"])
    secs = float(seconds_str)
    problems = []
    if st.get("codec_name") != "h264":
        problems.append(f"codec={st.get('codec_name')} != h264")
    ratio = st["width"] / st["height"]
    if abs(ratio - 9.0 / 16.0) > 0.02:
        problems.append(f"比例 {st['width']}x{st['height']} "
                        f"非 9:16")
    if not (secs - DURATION_TOLERANCE
            <= dur <= secs + DURATION_TOLERANCE_UP):
        problems.append(f"时长 {dur:.2f}s vs 请求 {secs}s")
    if dur < needed_dur:
        problems.append(
            f"时长 {dur:.3f}s < 混编需求 {needed_dur:.3f}s")
    if problems:
        raise RuntimeError("; ".join(problems))
    return st


# ---------------------------------------------------------------- 主流程
def main() -> int:
    ap = argparse.ArgumentParser(
        description="agnes-video-2.5 I2V 批量生成")
    ap.add_argument(
        "--project-dir", default=r"qidiwork-docs\manhua\proj1")
    ap.add_argument("--only", default=None,
                    help="逗号分隔 shot_id，仅处理指定镜")
    ap.add_argument("--dry-run", action="store_true",
                    help="只打印计划与成本，不调用 API")
    args = ap.parse_args()

    proj = Path(args.project_dir)
    with open(proj / "storyboard.json", "r",
              encoding="utf-8") as f:
        sb = json.load(f)

    shots = [s for s in sb.get("shots", []) if s.get("i2v")]
    if args.only:
        only = set(args.only.split(","))
        shots = [s for s in shots if s["shot_id"] in only]
    if not shots:
        common.log_error("无 i2v 镜头可处理")
        return 1

    # ---- 成本预估 ----
    plan = []
    for s in shots:
        secs = float(s["video_seconds"])
        plan.append((s["shot_id"], secs, secs * COST_PER_SEC))
    est = sum(c for _, _, c in plan)
    common.log_info(
        f"I2V 计划: {len(plan)} 镜, 预估 ¥{est:.2f} "
        f"(预算闸 ¥{BUDGET_LIMIT:.2f})")
    for sid, secs, cost in plan:
        common.log_info(
            f"  {sid}: {secs:.0f}s x ¥{COST_PER_SEC}/s "
            f"= ¥{cost:.2f}")
    if args.dry_run:
        return 0
    if est > BUDGET_LIMIT:
        common.log_error(f"预估 ¥{est:.2f} 超预算闸，"
                         "中止")
        return 2

    videos_dir = proj / "videos"
    videos_dir.mkdir(parents=True, exist_ok=True)
    common.check_disk_gb(videos_dir, min_gb=5.0)
    state = load_state(proj)

    # 创建/轮询/下载共用：180s 读超时（与中转 relay 上游
    # 180s 对齐——官方 API 偶发 >120s 沉默后仍会成功响应，
    # 120s 会误杀；整体仍受单镜 20 分钟超时约束）
    session = common.make_session(timeout=180.0)
    cum = 0.0
    done_list = []
    for shot in shots:
        sid = shot["shot_id"]
        secs = float(shot["video_seconds"])
        cost = secs * COST_PER_SEC
        # 混编需求时长（compose 侧口径：audio+0.5 与 min_duration 取大）
        audio_sec = shot.get("audio_sec") or 0.0
        needed_dur = max(audio_sec + 0.5,
                         float(shot["min_duration_sec"]))
        image_url = (f"{BASE}/mshots/{sid}.png")

        rec = state["shots"].get(sid, {})
        if rec.get("status") == "done" and \
                (videos_dir / f"{sid}.mp4").exists():
            cum += cost
            common.log_info(
                f"{sid}: 已完成跳过（已花 ¥{cost:.2f}，"
                f"累计 ¥{cum:.2f}）")
            done_list.append(sid)
            continue

        # ---- 成本闸：累计将超预算即中止 ----
        if cum + cost > BUDGET_LIMIT:
            common.log_error(
                f"{sid}: 累计成本将达 ¥{cum + cost:.2f} "
                f"超预算闸 ¥{BUDGET_LIMIT:.2f}，中止")
            return 2

        t0 = time.monotonic()
        # ---- 创建（或续轮询）----
        if rec.get("status") == "created" and \
                rec.get("video_id"):
            video_id = rec["video_id"]
            common.log_info(
                f"{sid}: 断点续传，续轮询 "
                f"video_id={video_id}")
        else:
            common.log_info(
                f"{sid}: 创建 I2V 任务 ({secs:.0f}s, "
                f"¥{cost:.2f})")
            video_id = create_video(session, shot, image_url)
            rec = {
                "status": "created",
                "video_id": video_id,
                "seconds": str(shot["video_seconds"]),
                "cost_yuan": round(cost, 2),
                "created_at":
                    datetime.now().isoformat(timespec="seconds"),
            }
            state["shots"][sid] = rec
            save_state(proj, state)
            common.log_info(
                f"{sid}: video_id={video_id}")

        # ---- 轮询 ----
        common.log_info(f"{sid}: 轮询中（间隔 {POLL_INTERVAL:.0f}s，"
                        f"超时 {POLL_TIMEOUT / 60:.0f} 分钟）")
        mp4_url = poll_video(session, video_id, sid)
        common.log_info(f"{sid}: 完成，下载 {mp4_url}")

        # ---- 下载 ----
        mp4 = videos_dir / f"{sid}.mp4"
        download_mp4(session, mp4_url, mp4)

        # ---- 校验 ----
        st = verify_mp4(mp4, str(shot["video_seconds"]),
                        needed_dur, sid)
        elapsed = time.monotonic() - t0
        rec.update({
            "status": "done",
            "mp4_url": mp4_url,
            "mp4": f"videos/{sid}.mp4",
            "duration": round(st["duration"], 3),
            "elapsed_sec": round(elapsed, 1),
        })
        state["shots"][sid] = rec
        save_state(proj, state)

        cum += cost
        done_list.append(sid)
        common.log_info(
            f"{sid}: ✅ {st['width']}x{st['height']} "
            f"{st['codec_name']} {st['duration']:.3f}s "
            f"耗时 {elapsed:.0f}s 成本 ¥{cost:.2f} "
            f"累计 ¥{cum:.2f}")

    common.log_info(
        f"全部完成: {done_list} 总成本 ¥{cum:.2f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
