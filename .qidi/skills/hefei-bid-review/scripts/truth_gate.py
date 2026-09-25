# -*- coding: utf-8 -*-
"""
truth_gate.py — 真值断言门禁（hefei-bid-review skill 配套，2026-09-19 新增）

来源：9.3 临湖项目 _final_gate.py（78 项写死断言，真实技术分 90.15 的交付门禁）通用化。
核心思想：A 组"真值在场+旧错数清零"点对点断言 + 算式/日期复算 + 关键词计数 + 态度词密度，
全部改为外部断言文件驱动，任何项目可复用。FAIL=退出码1，禁止定稿交付。

断言文件格式（UTF-8 文本，每行一条，# 开头为注释）：
    <ID> <检查名> :: <类型>: <参数>
类型：
    IN:    <文本>            真值文本必须在场（空白归一后匹配，兼容换行断词）
    NOT:   <文本>            旧错数/违禁句必须清零
    ALL:   <a>;<b>;<c>       多段文本必须全部在场
    ONEOF: <a>;<b>           至少一段在场
    CALC:  <算式>==<期望值>   纯算术复算（沙箱eval，例：15*20+80*85==6800）
    DATE:  <起始日>+<天>=<期望日>  日期复算（第N天=起始+ N-1，格式 2026-09-10+104=2026-12-23）
    CNT>=: <词>,<n>          关键词计数下限
    CNT<=: <词>,<n>          关键词计数上限（0=清零）
    DENS<: <词1;词2>,<千分值> 态度词密度上限（次/千字符）

用法：
    断言检查（docx 或 txt 全文均可）：
        python truth_gate.py run <方案.docx|方案.txt> <assertions.txt> [--out result.json]
    从 boq_gate 基线生成候选断言（人工确认措辞后使用）：
        python truth_gate.py gen <_boq_baseline.json> [--top 40] [--out _truth_assertions.txt]
"""
import argparse
import io
import json
import os
import re
import sys
from datetime import date, datetime, timedelta

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")


def norm(s: str) -> str:
    """空白归一（含全角空格/换行），使 IN/NOT 匹配不受排版断行影响。"""
    return re.sub(r"[\s\u3000]+", "", s or "")


def load_full_text(path: str) -> str:
    ext = os.path.splitext(path)[1].lower()
    if ext == ".docx":
        from docx import Document  # pip install python-docx
        doc = Document(path)
        parts = [p.text for p in doc.paragraphs]
        for t in doc.tables:
            for row in t.rows:
                for c in row.cells:
                    parts.append(c.text)
        return "\n".join(parts)
    raw = open(path, "rb").read()
    for enc in ("utf-8-sig", "utf-8", "gb18030"):
        try:
            return raw.decode(enc)
        except UnicodeDecodeError:
            continue
    return raw.decode("utf-8", errors="replace")


def sandbox_eval(expr: str):
    """纯算术沙箱：仅允许数字与 +-*/().,% ，禁止名称访问。"""
    if not re.fullmatch(r"[0-9+\-*/().,%\s]+", expr):
        raise ValueError("CALC 表达式含非法字符（仅允许数字与 +-*/().,）")
    if "**" in expr.replace(" ", ""):
        raise ValueError("CALC 禁用 ** 幂运算（防大数挂死）")
    return eval(expr.replace(",", ""), {"__builtins__": {}}, {})


def run_assertion(line_type: str, param: str, ntext: str, chars: int):
    """执行单条断言，返回 (ok, detail)。ntext=归一化全文。"""
    if line_type == "IN":
        return (norm(param) in ntext), f"IN {param[:40]}"
    if line_type == "NOT":
        return (norm(param) not in ntext), f"NOT {param[:40]}"
    if line_type == "ALL":
        parts = [x.strip() for x in param.split(";") if x.strip()]
        if not parts:
            return False, "ALL 参数为空（拒绝真空通过）"
        missing = [x for x in parts if norm(x) not in ntext]
        return (not missing), f"缺失:{missing[:2]}" if missing else f"ALL×{len(parts)}"
    if line_type == "ONEOF":
        parts = [x.strip() for x in param.split(";") if x.strip()]
        if not parts:
            return False, "ONEOF 参数为空（拒绝真空通过）"
        hit = [x for x in parts if norm(x) in ntext]
        return (bool(hit)), f"命中:{hit[:1]}" if hit else "全部缺失"
    if line_type == "CALC":
        m = re.fullmatch(r"(.+?)==(-?[0-9.]+)(?:±([0-9.]+))?", param.replace(" ", ""))
        if not m:
            return False, "CALC 格式应为 <算式>==<期望值>（可选 ±容差）"
        try:
            got = round(float(sandbox_eval(m.group(1))), 6)
        except Exception as e:
            return False, f"eval错误:{e}"
        want = float(m.group(2))
        tol = float(m.group(3)) if m.group(3) else max(abs(want) * 1e-6, 1e-9)
        return (abs(got - want) <= tol), f"={got} 期望{want}±{tol:.2g}"
    if line_type == "DATE":
        m = re.fullmatch(r"(\d{4}-\d{2}-\d{2})\+(\d+)=((\d{4})-(\d{2})-(\d{2}))", param.replace(" ", ""))
        if not m:
            return False, "DATE 格式应为 起始日+天数=期望日"
        try:
            d0 = date(int(m.group(1)[:4]), int(m.group(1)[5:7]), int(m.group(1)[8:10]))
            want = date(int(m.group(4)), int(m.group(5)), int(m.group(6)))
        except ValueError as e:
            return False, f"非法日期:{e}"
        got = d0 + timedelta(days=int(m.group(2)))
        return (got == want), f"={got} 期望{want}"
    if line_type in ("CNT>=", "CNT<="):
        m = re.fullmatch(r"(.*),\s*(\d+)", param)
        if not m:
            return False, "CNT 格式应为 <词>,<n>"
        word, n = m.group(1).strip(), int(m.group(2))
        if not word:
            return False, "CNT 词为空（拒绝真空通过）"
        c = ntext.count(norm(word))
        return (c >= n) if line_type == "CNT>=" else (c <= n), f"count={c}"
    if line_type == "DENS<":
        m = re.fullmatch(r"(.*),\s*([0-9.]+)", param)
        if not m:
            return False, "DENS 格式应为 <词1;词2>,<千分值>"
        words = [w.strip() for w in m.group(1).split(";") if w.strip()]
        if not words:
            return False, "DENS 词表为空（拒绝真空通过）"
        limit = float(m.group(2))
        cnt = sum(ntext.count(norm(w)) for w in words)
        dens = cnt / max(1, chars) * 1000
        return (dens < limit), f"density={dens:.2f}‰ <{limit}‰"
    return False, f"未知类型 {line_type}"


def cmd_run(args):
    if not os.path.exists(args.doc):
        print(f"ERROR: 方案文件不存在: {args.doc}")
        return 2
    full = load_full_text(args.doc)
    ntext = norm(full)
    chars = len(ntext)
    checks, bad_lines = [], []
    if not os.path.exists(args.assertions):
        print(f"ERROR: 断言文件不存在: {args.assertions}")
        return 2
    for ln, raw in enumerate(open(args.assertions, encoding="utf-8-sig").read().splitlines(), 1):
        line = raw.strip()
        mc = re.match(r"^(.*?)\s+#", line)  # 剥离行尾 " #" 注释（S1#等无空格编码不受影响）
        if mc and mc.group(1).strip():
            line = mc.group(1).strip()
        if not line or line.startswith("#"):
            continue
        m = re.fullmatch(r"(\S+)\s+(.+?)\s*::\s*([A-Za-z]+[<>=]*):\s*(.+)", line)
        if not m:
            bad_lines.append(ln)
            continue
        cid, name, ltype, param = m.groups()
        ok, detail = run_assertion(ltype, param.strip(), ntext, chars)
        checks.append({"id": cid, "name": name.strip(), "type": ltype, "ok": bool(ok), "detail": str(detail)[:120]})
    if not checks:
        print("ERROR: 断言文件无有效断言行——门禁不得空跑（空断言=0防御）")
        return 2
    fails = [c for c in checks if not c["ok"]]
    print("=" * 64)
    print(f"真值断言门禁 — {os.path.basename(args.doc)} × {os.path.basename(args.assertions)}")
    print(f"全文 {chars} 字（归一） / 断言 {len(checks)} 条 / FAIL {len(fails)} / 格式错误行 {len(bad_lines)}")
    print("=" * 64)
    for c in checks:
        mark = "✅" if c["ok"] else "❌"
        print(f"{mark} {c['id']} {c['name']}  | {c['detail']}")
    if bad_lines:
        print(f"⚠️ 无法解析的行号: {bad_lines}")
    verdict = "PASS" if (not fails and not bad_lines) else "FAIL"
    print(f"\n===== 门禁判定：{verdict} =====")
    if args.out:
        json.dump({"verdict": verdict, "total": len(checks), "fail": len(fails), "chars": chars,
                   "doc": os.path.abspath(args.doc), "assertions": os.path.abspath(args.assertions),
                   "generated_at": datetime.now().astimezone().isoformat(timespec="seconds"),
                   "parser": "truth_gate.py v1.2", "checks": checks},
                  open(args.out, "w", encoding="utf-8"), ensure_ascii=False, indent=1)
        print(f"结果已写出: {args.out}")
    return 0 if verdict == "PASS" else 1


def cmd_gen(args):
    b = json.load(open(args.baseline, encoding="utf-8-sig"))
    items = b.get("items", [])
    by_key = {}
    for it in items:
        if not it.get("name") or it.get("qty") is None:
            continue
        key = (it["name"], it["unit"])
        agg = by_key.setdefault(key, 0.0)
        by_key[key] = agg + it["qty"]
    rows = sorted(by_key.items(), key=lambda kv: -kv[1])[: args.top]
    out_lines = [
        "# truth_gate 断言候选（自动生成自 boq_gate 基线，需人工确认措辞/去噪后启用）",
        f"# 源: {os.path.basename(args.baseline)}  生成 TOP{args.top}（按数量降序）",
        "# 格式: <ID> <名称> :: IN: <真值文本>   —— 确认方案应以此数值口径出现；已知错数请另加 NOT: 行",
        "",
    ]
    for i, ((name, unit), qty) in enumerate(rows, 1):
        q = f"{qty:.3f}".rstrip("0").rstrip(".")
        nm = name[:12] + ("…" if len(name) > 12 else "")
        out_lines.append(f"# A{i:02d} {name}（{q}{unit}，{len([1 for k in by_key if k[0]==name])}个单位组）")
        out_lines.append(f"A{i:02d} {nm}真值{q}{unit}在场 :: IN: {nm}{q}")
    open(args.out, "w", encoding="utf-8").write("\n".join(out_lines) + "\n")
    print(f"[OK] 已生成 {len(rows)} 条候选断言 -> {args.out}")
    print("[提示] 逐条人工确认：①名称在方案中的实际措辞 ②数量口径（含损耗须按方案口径改写）")
    print("       ③删除评审噪声项 ④补充 NOT: 旧错数行与 CALC/DATE 复算行")
    return 0


def main():
    ap = argparse.ArgumentParser(description="真值断言门禁")
    sub = ap.add_subparsers(dest="cmd", required=True)
    p1 = sub.add_parser("run")
    p1.add_argument("doc")
    p1.add_argument("assertions")
    p1.add_argument("--out")
    p2 = sub.add_parser("gen")
    p2.add_argument("baseline")
    p2.add_argument("--top", type=int, default=40)
    p2.add_argument("--out", default="_truth_assertions.txt")
    args = ap.parse_args()
    return cmd_run(args) if args.cmd == "run" else cmd_gen(args)


if __name__ == "__main__":
    sys.exit(main())
