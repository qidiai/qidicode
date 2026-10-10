# -*- coding: utf-8 -*-
"""ab_eval_27b.py — 27B 嫁接 A/B 对比评测（容器跑）。
设计（用户定案）：简单题只当对照组，代码题（执行验证）+ 真实任务当主战场。
双臂：pass1 = 裸底模（无 adapter）；pass2 = 底模 + LoRA 插件（peft 挂载）。
断点续传：每 10 题原子保存；重启后按 id 跳过已完成的。
代码题评分：提取答案代码块 -> 与 test harness 拼装 -> 子进程执行（15s 超时）-> pass/fail。
开放题/风格题：答案留存（开放题 rubric 后评；风格题启发式三段式打分 0-3）。
输出：ab_exam_results.json（双臂分节通过率 + diff 表）
运行：python3 -X utf8 assets/ab_eval_27b.py --adapter /opt/atomgit/npu_m4/qwen38_27b_lora_adapter/
"""
import argparse
import io
import json
import os
import subprocess
import sys
import tempfile
import time

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace", line_buffering=True)
import torch

BASE_MODEL = "/tmp/qwen38_27b"
EXAM = os.path.join(os.path.dirname(os.path.abspath(__file__)), "ab_exam_27b.jsonl")
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "ab_exam_results.json")
CODE_SECTIONS = {"he", "real_exec"}


def log(msg):
    print(f"[{time.strftime('%H:%M:%S')}] {msg}", flush=True)


def load_results():
    if os.path.exists(OUT):
        try:
            with open(OUT, encoding="utf-8") as f:
                return json.load(f)
        except Exception:
            pass
    return {"meta": {}, "answers": {}}


def save_results(res):
    tmp = OUT + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(res, f, ensure_ascii=False, indent=1)
    os.replace(tmp, OUT)


def extract_code(answer, entry_point):
    """从答案中提取定义了 entry_point 的 python 代码块。"""
    blocks = []
    if "```" in answer:
        parts = answer.split("```")
        for i in range(1, len(parts), 2):
            block = parts[i]
            if block.startswith("python"):
                block = block[6:]
            blocks.append(block)
    if not blocks:
        blocks = [answer]
    for b in blocks:
        if f"def {entry_point}" in b or f"class {entry_point}" in b:
            return b
    return blocks[0] if blocks else answer


def exec_verify(code, test, entry_point, timeout=15):
    """子进程执行验证：candidate 代码 + test harness。返回 (ok, detail)。"""
    harness = f"{code}\n\n{test}\n\ntry:\n    check({entry_point})\n    print('EXEC_PASS')\nexcept Exception as e:\n    print('EXEC_FAIL', repr(e))\n"
    with tempfile.NamedTemporaryFile("w", suffix=".py", delete=False, encoding="utf-8", dir="/tmp") as f:
        f.write(harness)
        path = f.name
    try:
        r = subprocess.run([sys.executable, "-X", "utf8", path],
                           capture_output=True, text=True, timeout=timeout)
        out = (r.stdout or "") + (r.stderr or "")
        return ("EXEC_PASS" in out), out[-200:]
    except subprocess.TimeoutExpired:
        return False, "TIMEOUT"
    except Exception as e:
        return False, repr(e)
    finally:
        try:
            os.unlink(path)
        except Exception:
            pass


def style_score(answer):
    """三段式启发式：结论行 + 分点 + 代码块，各 1 分。"""
    s = 0
    lines = [ln.strip() for ln in answer.splitlines() if ln.strip()]
    if lines:
        s += 1  # 有首行
    if any(ln.startswith(("-", "*", "•", "1.", "1、")) for ln in lines[1:6]):
        s += 1  # 前 6 行内有分点
    if "```" in answer:
        s += 1  # 有代码块
    return s


@torch.no_grad()
def generate(model, tokenizer, prompt, max_new_tokens=512):
    text = tokenizer.apply_chat_template(
        [{"role": "user", "content": prompt}],
        tokenize=False, add_generation_prompt=True)
    enc = tokenizer(text, return_tensors="pt", truncation=True, max_length=2048).to(model.device)
    out = model.generate(**enc, max_new_tokens=max_new_tokens, do_sample=False,
                         pad_token_id=tokenizer.pad_token_id or tokenizer.eos_token_id)
    gen = out[0][enc["input_ids"].shape[1]:]
    return tokenizer.decode(gen, skip_special_tokens=True)


def run_pass(model, tokenizer, exam, pass_name, res, adapter_tag):
    done = res["answers"].get(pass_name, {})
    n = 0
    for item in exam:
        pid = item["id"]
        if pid in done:
            continue
        try:
            ans = generate(model, tokenizer, item["prompt"])
        except Exception as e:
            ans = f"<GEN_ERROR {e!r}>"
        entry = {"answer": ans}
        if item["section"] in CODE_SECTIONS:
            code = extract_code(ans, item["entry_point"])
            ok, detail = exec_verify(code, item["test"], item["entry_point"])
            entry["exec_pass"] = ok
            entry["detail"] = detail
        elif item["section"] == "style":
            entry["style_score"] = style_score(ans)
        done[pid] = entry
        n += 1
        if n % 10 == 0:
            res["answers"][pass_name] = done
            res["meta"]["adapter"] = adapter_tag
            save_results(res)
            log(f"{pass_name}: {n} 题完成（中间保存）")
    res["answers"][pass_name] = done
    save_results(res)
    log(f"{pass_name} 完成: 共 {len(done)} 题")


def summarize(res):
    from collections import defaultdict
    out = {}
    for pass_name in ("base", "graft"):
        done = res["answers"].get(pass_name, {})
        if not done:
            continue
        sec = defaultdict(lambda: {"pass": 0, "total": 0, "style_sum": 0})
        for pid, e in done.items():
            s = pid.split("/")[0]
            sec[s]["total"] += 1
            if e.get("exec_pass"):
                sec[s]["pass"] += 1
            if "style_score" in e:
                sec[s]["style_sum"] += e["style_score"]
        out[pass_name] = {k: {"rate": round(v["pass"] / v["total"], 3) if v["total"] else None,
                              "pass": v["pass"], "total": v["total"],
                              "style_avg": round(v["style_sum"] / v["total"], 2) if v["total"] else None}
                          for k, v in sec.items()}
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--adapter", default="/opt/atomgit/npu_m4/qwen38_27b_lora_adapter/")
    ap.add_argument("--skip-graft", action="store_true")
    args = ap.parse_args()

    import torch
    from transformers import AutoModelForCausalLM, AutoTokenizer

    exam = []
    with open(EXAM, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                exam.append(json.loads(line))
    log(f"考卷 {len(exam)} 题（30 HE + 10 real_exec + 10 real_open + 10 style + 10 control）")

    res = load_results()

    tokenizer = AutoTokenizer.from_pretrained(BASE_MODEL, trust_remote_code=True)
    log("加载裸底模（device_map=auto → 2×NPU）...")
    model = AutoModelForCausalLM.from_pretrained(BASE_MODEL, torch_dtype=torch.bfloat16,
                                                 device_map="auto", trust_remote_code=True)
    model.eval()

    # pass 1: 裸底模
    if len(res["answers"].get("base", {})) < len(exam):
        log("========== PASS 1: 裸底模 ==========")
        run_pass(model, tokenizer, exam, "base", res, "none")
    else:
        log("PASS 1 已完成（断点续传跳过）")

    # pass 2: + LoRA 插件
    if not args.skip_graft:
        log(f"========== PASS 2: + LoRA ({args.adapter}) ==========")
        from peft import PeftModel
        model = PeftModel.from_pretrained(model, args.adapter)
        model.eval()
        run_pass(model, tokenizer, exam, "graft", res, args.adapter)

    # 汇总
    summary = summarize(res)
    res["summary"] = summary
    save_results(res)
    log("========== A/B 对比表 ==========")
    print(json.dumps(summary, ensure_ascii=False, indent=1), flush=True)
    print("AB_EVAL_DONE", flush=True)


if __name__ == "__main__":
    main()
