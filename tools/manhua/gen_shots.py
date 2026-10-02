# -*- coding: utf-8 -*-
"""S3: ComfyUI 批量出图（参考图工作流 / 纯文生图工作流）。

关键工程要求（方案 §3 / P1-5 / P2-8 / P2-9）：
- 启动时校验 manifest.json：workflow 文件存在 + sha256 匹配、
  custom_nodes 目录存在、模型文件存在；不一致拒绝运行
- 断点续传：state.json 记录每镜头状态（pending/done/failed），
  已 done 自动跳过
- 单图 3 次重试
- 心跳：每次轮询前探 ComfyUI 存活（/health；0.37.0 无此路由
  返回 404 也算在线），连续 5 分钟无响应 → 告警退出
  （不自动重启 ComfyUI，留给人）
- 落盘前 check_disk_gb（默认 10GB）
- 参考图经 ComfyUI /upload/image API 上传（ComfyUI 自管
  input 目录，本脚本不直接写 ComfyUI 文件）

用法：
    python gen_shots.py [--project-dir DIR] [--dry-run]
"""
import argparse
import hashlib
import json
import sys
import time
from pathlib import Path

import common

HERE = Path(__file__).resolve().parent
MANIFEST = HERE / "manifest.json"
WORKFLOWS_DIR = HERE / "workflows"
COMFY_INPUT = "ComfyUI/input"   # ComfyUI 侧约定，仅用于日志

# 工作流模板占位符（seed 带引号：替换后为裸整数，KSampler 要求数值）
PH_PROMPT = "__PROMPT__"
PH_NEGATIVE = "__NEGATIVE__"
PH_SEED = '"__SEED__"'
PH_PREFIX = "__FILENAME_PREFIX__"
PH_REF = "__REF_IMAGE__"
# QwenImage21Cache 的 KV cache 落点（复审观察项②：auto vs cpu
# 显存/耗时对照；gen_shots 默认 auto）
PH_CACHE_DEVICE = "__CACHE_DEVICE__"

NEGATIVE = ("text, watermark, logo, signature, blurry, "
            "low quality, jpeg artifacts, extra fingers, "
            "deformed hands, bad anatomy, mutated, "
            "multiple girls, duplicate")

POLL_INTERVAL = 5.0     # s
HEALTH_TIMEOUT = 300.0  # 5 分钟无响应 → 告警退出
MAX_RETRY = 3


# ---------------------------------------------------------------- manifest
def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def validate_manifest() -> None:
    """校验 manifest 与本机环境一致性，不一致拒绝运行。"""
    with open(MANIFEST, "r", encoding="utf-8") as f:
        man = json.load(f)
    errs = []
    # 1) workflow 文件存在 + sha256
    for rel, want in man.get("workflows", {}).items():
        p = HERE / rel
        if not p.exists():
            errs.append(f"workflow 缺失: {rel}")
            continue
        got = sha256_file(p)
        if got != want:
            errs.append(f"workflow sha256 不匹配: {rel}\n"
                        f"  manifest: {want}\n  本机:   {got}")
    # 2) custom_nodes
    cn_root = Path(
        r"F:\AI\ComfyUI\ComfyUI_windows_portable"
        r"\ComfyUI\custom_nodes")
    for node in man.get("custom_nodes", []):
        if not (cn_root / node["name"]).exists():
            errs.append(f"custom_node 缺失: {node['name']}")
    # 3) 模型文件（只校验存在，不校验大文件 hash）
    m_root = Path(
        r"F:\AI\ComfyUI\ComfyUI_windows_portable"
        r"\ComfyUI\models")
    for m in man.get("models", []):
        if not (m_root / m["path"]).exists():
            errs.append(f"模型缺失: {m['path']}")
    if errs:
        for e in errs:
            common.log_error(f"manifest 校验失败: {e}")
        common.log_error(
            "环境与 manifest 不一致，拒绝运行。"
            "请按 manifest.json 恢复环境或更新 manifest。")
        sys.exit(1)
    common.log_info("manifest 校验通过"
                    f"（{len(man.get('workflows', {}))} workflow，"
                    f"{len(man.get('models', []))} 模型）")


# ---------------------------------------------------------------- prompt 组装
def load_character_card(card_path: Path) -> dict:
    """读角色卡（image_prompt 含外观锁定短语）。"""
    if not card_path.exists():
        common.log_warn(f"角色卡缺失: {card_path}（仅用 storyboard "
                        "提示词）")
        return {}
    with open(card_path, "r", encoding="utf-8") as f:
        return json.load(f)


def build_prompt(shot: dict, cards: dict,
                   char_by_id: dict) -> str:
    """拼接：角色锁定短语 + 镜头提示词 + 构图约束。"""
    parts = []
    for rid in shot.get("use_ref") or []:
        card = cards.get(rid, {})
        lock = card.get("image_prompt", "")
        if lock:
            parts.append(lock)
    parts.append(shot["image_prompt"])
    # 竖屏安全区构图约束（方案 §2.1）
    framing = shot.get("framing", "center")
    compose_hint = {
        "center": "subject centered in the middle "
                    "576x1024 vertical safe area",
        "left": "subject on the left third, "
                "keep the middle 576x1024 safe area clear",
        "right": "subject on the right third, "
                 "keep the middle 576x1024 safe area clear",
        # top/bottom 在方图阶段仅作提示词主体偏移约束
        # （观察项①：裁切退化 center，见 compose.py）
        "top": "subject in the upper part of the frame",
        "bottom": "subject in the lower part of the frame",
    }[framing]
    parts.append(compose_hint)
    return ", ".join(p for p in parts if p)


def render_workflow(template_name: str, **kw) -> dict:
    """读 workflow 模板并替换占位符。"""
    p = WORKFLOWS_DIR / template_name
    text = p.read_text(encoding="utf-8")
    for ph, val in kw.items():
        text = text.replace(ph, str(val))
    return json.loads(text)


# ---------------------------------------------------------------- ComfyUI
class ComfyClient:
    """ComfyUI API 客户端（/prompt /history /view /upload /health）。"""

    def __init__(self, session):
        self.sess = session
        self.base = common.COMFYUI_URL

    def health(self) -> bool:
        """存活探测。实测 ComfyUI 0.37.0 没有 /health 路由
        （GET /health 返回 404）——能收到任何 HTTP 响应都代表
        服务在线，只有连接异常才算掉线；404 时再探 /object_info
        确认服务真正可用。"""
        try:
            r = self.sess.get(f"{self.base}/health")
            if r.status_code == 404:
                r2 = self.sess.get(f"{self.base}/object_info")
                return r2.status_code == 200
            return r.status_code == 200
        except Exception:  # noqa: BLE001
            return False

    def upload_image(self, path: Path) -> str:
        """上传参考图到 ComfyUI input 目录，返回文件名。"""
        with open(path, "rb") as f:
            files = {"image": (path.name, f, "image/png")}
            data = {"overwrite": "true"}
            r = self.sess.post(
                f"{self.base}/upload/image",
                files=files, data=data)
            r.raise_for_status()
            return r.json()["name"]

    def submit(self, wf: dict) -> str:
        r = self.sess.post(
            f"{self.base}/prompt",
            json={"prompt": wf, "client_id": "manhua-gen"})
        r.raise_for_status()
        return r.json()["prompt_id"]

    def history(self, prompt_id: str) -> dict:
        r = self.sess.get(f"{self.base}/history/{prompt_id}")
        r.raise_for_status()
        return r.json()

    def fetch_image(self, filename: str, subfolder: str,
                      ftype: str) -> bytes:
        r = self.sess.get(f"{self.base}/view",
                          params={"filename": filename,
                                  "subfolder": subfolder,
                                  "type": ftype})
        r.raise_for_status()
        return r.content


# ---------------------------------------------------------------- 主流程
def load_state(proj: Path) -> dict:
    p = proj / "state.json"
    if p.exists():
        with open(p, "r", encoding="utf-8") as f:
            return json.load(f)
    return {"shots": {}}


def save_state(proj: Path, state: dict) -> None:
    with open(proj / "state.json", "w", encoding="utf-8") as f:
        json.dump(state, f, ensure_ascii=False, indent=2)


def process_shot(shot: dict, client: ComfyClient,
                   cards: dict, char_by_id: dict,
                   proj: Path, state: dict,
                   dry_run: bool,
                   cache_device: str = "auto") -> bool:
    """单镜头出图。成功 True；失败 False。"""
    sid = shot["shot_id"]
    st = state["shots"].setdefault(
        sid, {"status": "pending", "attempts": 0})
    if st["status"] == "done":
        common.log_info(f"{sid}: state=done，跳过（断点续传）")
        return True

    prompt = build_prompt(shot, cards, char_by_id)
    seed = int(hashlib.sha256(sid.encode()).hexdigest()[:8], 16)
    prefix = f"manhua_{sid}"

    # 按 use_ref 选 workflow
    if shot.get("use_ref"):
        ref_char = char_by_id[shot["use_ref"][0]]
        ref_sheet = proj / ref_char["ref_sheet"]
        if not ref_sheet.exists():
            common.log_error(
                f"{sid}: 参考图缺失 {ref_sheet}，"
                "先跑 S2 角色设定图")
            st.update(status="failed",
                      error=f"missing ref {ref_sheet}")
            return False
        wf_name = "workflow_refimg.json"
    else:
        ref_sheet = None
        wf_name = "workflow_text2img.json"

    shots_dir = proj / "shots"
    shots_dir.mkdir(parents=True, exist_ok=True)
    out_png = shots_dir / f"{sid}.png"

    for attempt in range(1, MAX_RETRY + 1):
        st["attempts"] = st.get("attempts", 0) + 1
        try:
            if dry_run:
                # dry-run 不写 state.json（否则会污染断点续传，
                # 真跑时误判为已完成而全部跳过）
                common.log_info(
                    f"{sid}: [dry-run] workflow={wf_name} "
                    f"seed={seed} prompt={prompt[:120]}...")
                return True

            # 上传参考图（如需）
            ref_name = None
            if ref_sheet is not None:
                ref_name = client.upload_image(ref_sheet)
                common.log_info(
                    f"{sid}: 参考图已上传 {ref_sheet.name}"
                    f" -> ComfyUI {COMFY_INPUT}/{ref_name}")

            wf = render_workflow(
                wf_name,
                **{PH_PROMPT: prompt,
                   PH_NEGATIVE: NEGATIVE,
                   PH_SEED: seed,
                   PH_PREFIX: prefix,
                   PH_CACHE_DEVICE: cache_device})
            if ref_name is not None:
                # 替换 LoadImage 节点输入
                for node in wf.values():
                    if node.get("class_type") == "LoadImage":
                        node["inputs"]["image"] = ref_name

            common.log_info(f"{sid}: 提交 attempt={attempt} "
                            f"wf={wf_name}")
            pid = client.submit(wf)
            st["prompt_id"] = pid
            save_state(proj, state)

            # 轮询 + 心跳
            deadline = time.time() + 3600  # 单图 1h 上限
            last_health_ok = time.time()
            while time.time() < deadline:
                # 心跳：每次轮询前查 /health
                if client.health():
                    last_health_ok = time.time()
                elif (time.time() - last_health_ok
                        > HEALTH_TIMEOUT):
                    common.log_error(
                        f"ComfyUI /health 连续 "
                        f"{HEALTH_TIMEOUT:.0f}s 无响应，"
                        "告警退出（不自动重启，请人工介入，"
                        "重启后重跑本脚本自动续传）")
                    st.update(status="failed",
                              error="comfyui heartbeat lost")
                    save_state(proj, state)
                    sys.exit(2)
                hist = client.history(pid)
                entry = hist.get(pid)
                if entry:
                    status_str = entry.get(
                        "status", {}).get("status_str")
                    if status_str == "success":
                        break
                    if status_str in ("error", "failed"):
                        raise RuntimeError(
                            f"ComfyUI 节点执行失败: "
                            f"{status_str}")
                time.sleep(POLL_INTERVAL)
            else:
                raise RuntimeError("单图超时 1h")

            # 取结果
            entry = client.history(pid)[pid]
            imgs = []
            for node_out in entry.get("outputs", {}).values():
                imgs.extend(node_out.get("images", []))
            if not imgs:
                raise RuntimeError("ComfyUI 无输出图")
            img = imgs[0]
            data = client.fetch_image(
                img["filename"], img.get("subfolder", ""),
                img.get("type", "output"))

            # 落盘前磁盘预检（P2-9）
            common.check_disk_gb(shots_dir)
            out_png.write_bytes(data)
            st.update(status="done", image=str(out_png.relative_to(proj)))
            save_state(proj, state)
            common.log_info(f"{sid}: 出图完成 {out_png} "
                            f"({len(data)}B)")
            return True
        except Exception as e:  # noqa: BLE001  单图重试
            common.log_warn(f"{sid}: attempt={attempt} "
                            f"失败 err={e!r}")
            time.sleep(2 ** (attempt - 1))
    st.update(status="failed", error="3 retries exhausted")
    save_state(proj, state)
    return False


def main() -> int:
    ap = argparse.ArgumentParser(description="ComfyUI 批量出图")
    ap.add_argument("--project-dir", default=r"qidiwork-docs\manhua\proj1")
    ap.add_argument("--dry-run", action="store_true",
                    help="只校验 manifest + 组装 prompt，"
                         "不提交 ComfyUI")
    ap.add_argument("--cache-device", default="auto",
                    choices=["auto", "gpu", "cpu", "off"],
                    help="QwenImage21Cache 的 KV cache 落点"
                         "（默认 auto；复审观察项②对照用 cpu）")
    args = ap.parse_args()

    # 启动即校验 manifest（P2-8）
    validate_manifest()

    if not args.dry_run:
        client = ComfyClient(common.make_session())
        if not client.health():
            common.log_error(
                "ComfyUI 离线（/health 不通）。\n"
                "启动方式（幂等，直接跑）：\n"
                "  powershell -File "
                r"F:\AI\ComfyUI\start_comfyui.ps1\n"
                "等 /health 就绪后重跑本脚本。")
            return 1
        common.log_info("ComfyUI /health 就绪")
    else:
        client = None
        common.log_info("[dry-run] 跳过 ComfyUI 连接检查")

    proj = Path(args.project_dir)
    with open(proj / "storyboard.json", "r", encoding="utf-8") as f:
        sb = json.load(f)
    char_by_id = {c["id"]: c for c in sb.get("characters", [])}
    cards = {rid: load_character_card(proj / c["card"])
             for rid, c in char_by_id.items()}

    state = load_state(proj)
    ok = True
    for shot in sb.get("shots", []):
        if not process_shot(shot, client, cards, char_by_id,
                            proj, state, args.dry_run,
                            cache_device=args.cache_device):
            ok = False
    save_state(proj, state)

    if not ok:
        common.log_error("存在失败镜头（见 state.json），"
                         "退出码 1；修复后重跑自动续传")
        return 1
    common.log_info("全部镜头出图完成")
    return 0


if __name__ == "__main__":
    sys.exit(main())
