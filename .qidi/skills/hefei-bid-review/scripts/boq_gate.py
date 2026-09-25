# -*- coding: utf-8 -*-
"""
boq_gate.py — 清单核对门禁脚本（hefei-bid-review skill 配套，2026-09-19 新增）

防"模型偷懒不核清单"的确定性工具：清单解析与覆盖率统计不依赖模型自觉。

用法：
  1) 生成清单基线（评审/修改前必须先做，产物落盘）：
     python boq_gate.py make-baseline <清单XML(.18zhxj/.18zhzb)> <输出json路径>
  2) 覆盖率与零响应检查（对照方案全文txt，PDF用pymupdf提取后传入）：
     python boq_gate.py check <baseline.json> <方案全文.txt> [--top 30] [--csv out.csv]

退出码：0=正常产出；2=参数/解析错误。
"""
import argparse
import json
import re
import sys
import io
import xml.etree.ElementTree as ET
from datetime import datetime, timezone

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")


def norm_text(s: str) -> str:
    if not s:
        return ""
    s = s.replace("㎡", "m2").replace("m²", "m2").replace("m³", "m3")
    return re.sub(r"\s+", "", s)


def parse_qty(sl: str):
    if sl is None:
        return None
    s = str(sl).strip().replace(",", "")
    if not s:
        return None
    try:
        return round(float(s), 3)
    except ValueError:
        return None


def iter_qdmx(root):
    for dw in root.iter("Dwgcxx"):
        dw_name = (dw.get("Dwgcmc") or "").strip()
        for qx in dw.iter("Qdmx"):
            feat = (qx.get("Xmtz") or "").replace("\n", " / ").strip()
            yield {
                "dw": dw_name,
                "code": (qx.get("Qdbm") or "").strip(),
                "name": (qx.get("Mc") or "").strip(),
                "unit": (qx.get("Dw") or "").strip(),
                "qty": parse_qty(qx.get("Sl")),
                "feat": feat[:200],
            }


def cmd_make_baseline(args):
    tree = ET.parse(args.boq_xml)
    root = tree.getroot()
    items = list(iter_qdmx(root))
    dws = []
    for dw in root.iter("Dwgcxx"):
        n = (dw.get("Dwgcmc") or "").strip()
        if n and n not in dws:
            dws.append(n)
    if not items:
        print("ERROR: 未解析到任何 Qdmx 条目，请确认输入为清单XML(.18zhxj/.18zhzb)")
        return 2
    baseline = {
        "source_file": args.boq_xml,
        "parsed_at": datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds"),
        "parser": "boq_gate.py v1.0 (ElementTree)",
        "total_items": len(items),
        "unit_works": dws,
        "items": items,
    }
    with open(args.out_json, "w", encoding="utf-8") as f:
        json.dump(baseline, f, ensure_ascii=False, indent=1)
    print(f"[OK] 基线已生成: {args.out_json}")
    print(f"     源文件: {args.boq_xml}")
    print(f"     清单条目(Qdmx): {len(items)}  单位工程: {len(dws)}")
    return 0


def load_text(path: str) -> str:
    raw = open(path, "rb").read()
    for enc in ("utf-8-sig", "utf-8", "gb18030"):
        try:
            return raw.decode(enc)
        except UnicodeDecodeError:
            continue
    return raw.decode("utf-8", errors="replace")


def cmd_check(args):
    baseline = json.load(open(args.baseline, encoding="utf-8"))
    items = baseline.get("items", [])
    raw_text = load_text(args.plan_text)
    text = norm_text(raw_text)
    if not items:
        print("ERROR: 基线为空，先执行 make-baseline")
        return 2
    if not text:
        print("ERROR: 方案全文为空")
        return 2

    # 聚合键=(名称,单位)：防止同名不同单位(如"矩形柱"下 m³混凝土 与 m²模板)混加成脏数据
    by_key = {}
    for it in items:
        key = (it["name"], it["unit"])
        if not it["name"]:
            continue
        agg = by_key.setdefault(key, {"qty_total": 0.0, "codes": set()})
        if it["code"]:
            agg["codes"].add(it["code"])
        if it["qty"] is not None:
            agg["qty_total"] += it["qty"]

    # 名称级覆盖率（按唯一名称统计；同名称多单位只算一次）
    names = sorted({k[0] for k in by_key})
    hit = [n for n in names if n in text]
    missed = [n for n in names if n not in text]
    cov_name = len(hit) / len(names) if names else 0.0

    # 编码级覆盖率（披露用）
    code_hit = sum(1 for it in items if it["code"] and it["code"] in text)
    cov_code = code_hit / len(items) if items else 0.0

    # 零响应清单项（按(名称,单位)聚合数量降序；missed 为名称集合，展开其全部单位组）
    missed_rows = sorted(
        (
            (name, by_key[(name, unit)]["qty_total"], unit, len(by_key[(name, unit)]["codes"]))
            for name in missed
            for unit in {u for (n2, u) in by_key if n2 == name}
        ),
        key=lambda r: -r[1],
    )

    # 无源数字抽查（方案中带单位数字与清单数量库比对，容差2%，仅参考）
    qty_set = [it["qty"] for it in items if it["qty"] is not None]
    nums = re.findall(r"(\d+(?:\.\d+)?)\s*(m2|m3|t|根|m|个|套|座|台|批|项)(?![\d.])", norm_text(raw_text))
    unit_nums = {"m2", "m3", "t", "m", "根", "个", "套", "座", "台", "批", "项"}
    checked = sourced = 0
    for v, u in nums:
        if u not in unit_nums:
            continue
        val = float(v)
        if val < 5:  # 小数值多为参数(厚度/间距/高度)，跳过
            continue
        checked += 1
        ok = any(abs(val - q) <= max(0.02 * max(val, 1), 0.05) for q in qty_set)
        if ok:
            sourced += 1

    print("=" * 62)
    print("清单核对门禁报告 (boq_gate.py check)")
    print("=" * 62)
    print(f"基线: {baseline.get('source_file')}  条目 {len(items)} 条 / 名称去重 {len(names)} 种 / (名称×单位)组 {len(by_key)} 组")
    print(f"名称级覆盖率(方案提及的清单名称占比): {cov_name*100:.1f}%  ({len(hit)}/{len(names)})")
    print(f"编码级覆盖率(披露参考,通常接近0):     {cov_code*100:.1f}%")
    if checked:
        print(f"带单位数字抽查: {checked} 个, 其中可在清单数量库匹配 {sourced} 个 ({sourced/checked*100:.0f}%, 仅参考)")
    print("-" * 62)
    print(f"零响应清单项 TOP{args.top}(按合并数量降序,需逐项给处置建议:补入/说明)")
    print(f"{'名称':<30}{'合计数量':>12}{'单位':>8}{'编码种数':>10}")
    for n, q, u, c in missed_rows[: args.top]:
        print(f"{n[:30]:<30}{q:>12.2f}{u:>8}{c:>8}")
    print("-" * 62)
    print("[产物要求] 本报告+baseline+crosscheck.csv 三件齐备=评审过程合规;")
    print("[评分口径] 覆盖率仅作披露指标，不得直接当分数计入评分（同名跨单位合并、")
    print("           功能描述代指等噪声须人工甄别）；只有逐项复核确认的工程量偏差项")
    print("           与零响应大项才进入扣分表（详见 SKILL.md Step 4 双口径区分）。")
    print("[数字抽查免责] 无源数字匹配率为全库±2%容差+单位混池的粗筛，95%级匹配属")
    print("           结构性常态，不构成“数据有源”证明；定论前必须逐条人工溯源。")

    if args.csv:
        hit_set = set(hit)
        with open(args.csv, "w", encoding="utf-8-sig") as f:
            f.write("名称,合计数量,单位,编码种数(去重),方案是否提及\n")
            rows = sorted(
                (
                    (name, by_key[(name, unit)]["qty_total"], unit, len(by_key[(name, unit)]["codes"]), "是" if name in hit_set else "否")
                    for (name, unit) in by_key
                ),
                key=lambda r: -r[1],
            )
            for n, q, u, c, m in rows:
                f.write(f"{n},{q:.2f},{u},{c},{m}\n")
        print(f"[OK] 交叉对照骨架已写出: {args.csv} (评审席须在此基础上逐行填方案引用值/偏差%/判定)")
    return 0


def main():
    ap = argparse.ArgumentParser(description="清单核对门禁脚本")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("make-baseline").add_argument("boq_xml")
    import sys as _s
    # 重新绑定参数名（位置参数按序）
    p1 = sub._name_parser_map["make-baseline"]
    p1.add_argument("out_json")
    p2 = sub.add_parser("check")
    p2.add_argument("baseline")
    p2.add_argument("plan_text")
    p2.add_argument("--top", type=int, default=30)
    p2.add_argument("--csv")
    args = ap.parse_args()
    if args.cmd == "make-baseline":
        return cmd_make_baseline(args)
    if args.cmd == "check":
        return cmd_check(args)
    return 2


if __name__ == "__main__":
    sys.exit(main())
