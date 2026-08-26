#!/usr/bin/env python3
\"\"\"
secret_scan.py - GitHub 公开仓库密钥泄漏扫描器
用法: python secret_scan.py --repo URL 或 --org NAME 或 --recent DAYS
\"\"\"
import argparse, json, subprocess, sys
from pathlib import Path

FINDINGS_FILE = Path.home() / \".secret-scan\" / \"findings.json\"

def ensure_dir():
    FINDINGS_FILE.parent.mkdir(parents=True, exist_ok=True)

def run_trufflehog(args):
    cmd = [\"trufflehog\", \"github\"] + args
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
        findings = []
        for line in result.stdout.splitlines():
            line = line.strip()
            if not line: continue
            try:
                data = json.loads(line)
                if data.get(\"Type\") and data.get(\"Verified\", False):
                    findings.append(data)
            except json.JSONDecodeError: continue
        return findings
    except subprocess.TimeoutExpired:
        print(f\"[ERROR] Timeout\", file=sys.stderr); return []
    except FileNotFoundError:
        print(\"[ERROR] trufflehog not found. pip install trufflehog\", file=sys.stderr)
        sys.exit(1)

def scan_repo(url, max_depth=500):
    print(f\"[*] Scanning {url} ...\")
    return run_trufflehog([\"--repo\", url, \"--only-verified\", \"--max_depth\", str(max_depth), \"--json\"])

def scan_org(org):
    print(f\"[*] Scanning org: {org}\")
    return run_trufflehog([\"--org\", org, \"--only-verified\", \"--json\"])

def save_findings(findings):
    ensure_dir()
    existing = json.loads(FINDINGS_FILE.read_text()) if FINDINGS_FILE.exists() else []
    combined = existing + findings
    seen, unique = set(), []
    for f in combined:
        key = f\"{f.get('Type')}_{f.get('Commit','')[:8]}_{f.get('File','')}\"
        if key not in seen:
            seen.add(key); unique.append(f)
    FINDINGS_FILE.write_text(json.dumps(unique, indent=2, ensure_ascii=False))
    print(f\"[+] Saved {len(unique)} findings to {FINDINGS_FILE}\")

def print_summary(findings):
    if not findings:
        print(\"\\n[OK] No verified secrets found.\")
        return
    print(f\"\\n{'='*60}\")
    print(f\"  SECRET SCAN: {len(findings)} verified finding(s)\")
    print(f\"{'='*60}\")
    by_type = {}
    for f in findings:
        t = f.get(\"Type\", \"Unknown\"); by_type[t] = by_type.get(t, 0) + 1
    print(\"\\n[By type]:\")
    for t, c in sorted(by_type.items(), key=lambda x: -x[1]):
        print(f\"  - {t}: {c}\")
    print(\"\\n[Details - first 20]:\")
    for f in findings[:20]:
        print(f\"  * [{f.get('Type','?')}] {f.get('Repo','?')}\")
        print(f\"    Commit: {f.get('Commit','')[:8]} | {f.get('File','')}:{f.get('Line','?')}\")
    if len(findings) > 20:
        print(f\"  ... and {len(findings)-20} more (see {FINDINGS_FILE})\")
    print(f\"\\n[+] Full report: {FINDINGS_FILE}\")

def main():
    parser = argparse.ArgumentParser()
    g = parser.add_mutually_exclusive_group(required=True)
    g.add_argument(\"--repo\", help=\"Single repo URL\")
    g.add_argument(\"--org\", help=\"GitHub org name\")
    g.add_argument(\"--recent\", type=int, help=\"Scan repos with recent commits (days)\")
    parser.add_argument(\"--save\", action=\"store_true\")
    parser.add_argument(\"--max-depth\", type=int, default=500)
    args = parser.parse_args()
    findings = []
    if args.repo: findings = scan_repo(args.repo, args.max_depth)
    elif args.org: findings = scan_org(args.org)
    print_summary(findings)
    if args.save and findings: save_findings(findings)

if __name__ == \"__main__\":
    main()
