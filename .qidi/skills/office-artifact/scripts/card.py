#!/usr/bin/env python3
"""office-artifact: 产物卡片与任务工作区登记簿。

子命令:
  add <文件...> [--task 名称] [--skill 来源技能] [--note 摘要]   登记产物
  show [--task 名称] [--last N]                                   卡片总览(任务收尾必调)
  open <编号|路径>                                                 用系统默认程序打开
  list [--task 名称]                                              JSON 登记簿
  clean [--task 名称] --yes                                       清空工作区登记

工作区: ~/.qidi/office-workspaces/<task>/manifest.json (默认 default)
"""
import argparse
import json
import os
import subprocess
import sys
import time

HOME = os.path.expanduser("~")
ROOT = os.path.join(HOME, ".qidi", "office-workspaces")
DEFAULT_TASK = "default"

# 常见产物扩展名(登记时校验;不在名单内的会给出警告但仍登记)
KNOWN_EXT = {
    ".docx", ".doc", ".xlsx", ".xls", ".pptx", ".ppt", ".pdf", ".png", ".jpg",
    ".jpeg", ".gif", ".svg", ".html", ".md", ".csv", ".txt", ".zip", ".7z",
    ".wav", ".mp3", ".mp4", ".webm", ".dwg", ".dxf",
}


def task_dir(task: str) -> str:
    return os.path.join(ROOT, task)


def manifest_path(task: str) -> str:
    return os.path.join(task_dir(task), "manifest.json")


def load(task: str) -> dict:
    p = manifest_path(task)
    if os.path.isfile(p):
        try:
            with open(p, encoding="utf-8") as f:
                return json.load(f)
        except Exception:
            pass
    return {"task": task, "artifacts": []}


def save(task: str, data: dict) -> None:
    d = task_dir(task)
    os.makedirs(d, exist_ok=True)
    # 原子写(tmp + os.replace):并发读取方(GUI/TUI/第三方)永远看到
    # 完整 JSON,不会读到半截文件(QidiWork 方案 v2 D3)
    tmp = manifest_path(task) + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False, indent=1)
    os.replace(tmp, manifest_path(task))


def fmt_size(n: int) -> str:
    for unit in ("B", "KB", "MB", "GB"):
        if n < 1024 or unit == "GB":
            return f"{n:.1f} {unit}" if unit != "B" else f"{n} B"
        n /= 1024.0
    return f"{n} GB"


def fmt_time(ts: float) -> str:
    return time.strftime("%Y-%m-%d %H:%M", time.localtime(ts))


def resolve(path: str) -> str:
    return os.path.abspath(os.path.expanduser(path))


def cmd_add(args) -> int:
    task = args.task or DEFAULT_TASK
    data = load(task)
    added, skipped = [], []
    for raw in args.files:
        p = resolve(raw)
        if not os.path.isfile(p):
            skipped.append((raw, "文件不存在"))
            continue
        ext = os.path.splitext(p)[1].lower()
        st = os.stat(p)
        entry = {
            "path": p,
            "name": os.path.basename(p),
            "size": st.st_size,
            "mtime": st.st_mtime,
            "registered_at": time.time(),
            "skill": args.skill or "",
            "note": args.note or "",
        }
        data["artifacts"].append(entry)
        added.append((entry, ext))
    save(task, data)
    for entry, ext in added:
        mark = "" if ext in KNOWN_EXT else "  (非常见扩展名,确认是交付物?)"
        print(f"已登记  {entry['name']}  {fmt_size(entry['size'])}{mark}")
    for raw, why in skipped:
        print(f"跳过    {raw}: {why}", file=sys.stderr)
    print(f"工作区 [{task}] 现有 {len(data['artifacts'])} 个产物; 收尾请执行 card.py show")
    return 0 if added else 1


def cmd_show(args) -> int:
    task = args.task or DEFAULT_TASK
    data = load(task)
    arts = data["artifacts"]
    if args.last:
        arts = arts[-args.last:]
    if not arts:
        print(f"📦 产物卡片 [{task}] 空 —— 尚未登记任何产物")
        return 0
    print(f"📦 产物卡片 [{task}] 共 {len(arts)} 项")
    print("─" * 60)
    for i, a in enumerate(arts, 1):
        skill = f"  [{a['skill']}]" if a.get("skill") else ""
        note = f"\n    └ {a['note']}" if a.get("note") else ""
        print(f" {i:>2}. {a['name'][:36]:<38} {fmt_size(a['size']):>9}  "
              f"{fmt_time(a['mtime'])}{skill}{note}")
    print("─" * 60)
    print(f" 打开方式: python card.py open <编号>  |  登记簿: card.py list --task {task}")
    return 0


def cmd_open(args) -> int:
    task = args.task or DEFAULT_TASK
    data = load(task)
    arts = data["artifacts"]
    target = None
    if args.target.isdigit():
        idx = int(args.target)
        if 1 <= idx <= len(arts):
            target = arts[idx - 1]["path"]
        else:
            print(f"编号 {idx} 超出范围 (1-{len(arts)})", file=sys.stderr)
            return 2
    else:
        t = resolve(args.target)
        target = t if os.path.isfile(t) else args.target
    if not target or not os.path.isfile(target):
        print(f"找不到文件: {args.target}", file=sys.stderr)
        return 2
    try:
        if sys.platform == "win32":
            os.startfile(target)  # noqa: S606
        elif sys.platform == "darwin":
            subprocess.run(["open", target], check=False)
        else:
            subprocess.run(["xdg-open", target], check=False)
        print(f"已用系统默认程序打开: {os.path.basename(target)}")
        return 0
    except Exception as e:
        print(f"打开失败: {e}", file=sys.stderr)
        return 3


def cmd_list(args) -> int:
    task = args.task or DEFAULT_TASK
    print(json.dumps(load(task), ensure_ascii=False, indent=1))
    return 0


def cmd_clean(args) -> int:
    task = args.task or DEFAULT_TASK
    if not args.yes:
        print("需要 --yes 确认; 仅清空登记簿, 不删除文件本身", file=sys.stderr)
        return 2
    save(task, {"task": task, "artifacts": []})
    print(f"工作区 [{task}] 登记簿已清空(文件未动)")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="office-artifact 产物卡片")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("add", help="登记产物")
    p.add_argument("files", nargs="+")
    p.add_argument("--task", default=None)
    p.add_argument("--skill", default=None)
    p.add_argument("--note", default=None)
    p.set_defaults(fn=cmd_add)

    p = sub.add_parser("show", help="卡片总览(任务收尾必调)")
    p.add_argument("--task", default=None)
    p.add_argument("--last", type=int, default=0)
    p.set_defaults(fn=cmd_show)

    p = sub.add_parser("open", help="打开产物")
    p.add_argument("target")
    p.add_argument("--task", default=None)
    p.set_defaults(fn=cmd_open)

    p = sub.add_parser("list", help="JSON 登记簿")
    p.add_argument("--task", default=None)
    p.set_defaults(fn=cmd_list)

    p = sub.add_parser("clean", help="清空登记簿")
    p.add_argument("--task", default=None)
    p.add_argument("--yes", action="store_true")
    p.set_defaults(fn=cmd_clean)

    args = ap.parse_args()
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
