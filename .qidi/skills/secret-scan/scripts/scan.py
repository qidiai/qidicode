import json, subprocess, sys, urllib.request, urllib.error, tempfile, os, re, urllib.parse
from pathlib import Path

API_BASE = "https://api.github.com"
SECRET_PATTERNS = {
    "OpenAI/Agnes Key": re.compile(r"sk-[a-zA-Z0-9]{20,}"),
    "AWS Access Key": re.compile(r"AKIA[A-Z0-9]{16}"),
    "JWT Token": re.compile(r"eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+"),
    "Private Key": re.compile(r"-----BEGIN (RSA|EC|OPENSSH) PRIVATE KEY-----"),
    "Google API Key": re.compile(r"AIza[a-zA-Z0-9_-]{30,}"),
    "Stripe Key": re.compile(r"sk_(live|test)[a-zA-Z0-9]{24,}"),
}
SKIP_PATTERNS = [re.compile(r"\.(example|template|sample|md)$", re.I), re.compile(r"/docs/", re.I)]

def api_get(path, params=None):
    url = API_BASE + path
    if params:
        pairs = []
        for k, v in params.items():
            pairs.append(urllib.parse.quote(str(k)) + "=" + urllib.parse.quote(str(v)))
        url += "?" + "&".join(pairs)
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "SecretScanner/1.0"})
        with urllib.request.urlopen(req, timeout=30) as resp:
            return json.loads(resp.read().decode())
    except urllib.error.HTTPError as e:
        print(f"  API Error {e.code}", file=sys.stderr)
        return None
    except Exception as e:
        print(f"  Error: {e}", file=sys.stderr)
        return None

def search_repos(query, limit=10):
    data = api_get("/search/repositories", {"q": query, "sort": "updated", "order": "desc", "per_page": limit})
    return data.get("items", []) if data else []

def scan_content(content, filepath):
    findings = []
    for name, pattern in SECRET_PATTERNS.items():
        for match in pattern.finditer(content):
            val = match.group(0)
            lower = val.lower()
            if any(x in lower for x in ["your-", "change-me", "xxx", "todo", "example", "sk-...", "canary", "leak"]):
                continue
            line_num = content[:match.start()].count("\n") + 1
            findings.append({"type": name, "file": filepath, "match": val[:40] + "..." if len(val) > 40 else val, "line": line_num})
    return findings

def scan_repo(repo_url, temp_dir):
    repo_name = repo_url.replace("https://github.com/", "").replace(".git", "")
    repo_dir = Path(temp_dir) / repo_name.replace("/", "_")
    print(f"[*] Cloning {repo_url} ...", end=" ", flush=True)
    try:
        subprocess.run(["git", "clone", "--depth", "50", repo_url, str(repo_dir)], capture_output=True, timeout=120)
    except Exception as e:
        print(f"Failed: {e}")
        return []
    print("OK")
    findings = []
    for f in repo_dir.rglob("*"):
        if f.is_file():
            try:
                if f.stat().st_size > 1024 * 1024:
                    continue
                if any(p.match(str(f)) for p in SKIP_PATTERNS):
                    continue
                content = f.read_text(errors="ignore")
                results = scan_content(content, str(f.relative_to(repo_dir)))
                findings.extend(results)
            except:
                pass
    return findings

def main():
    query = sys.argv[1] if len(sys.argv) > 1 else "api_key config json created:>=2025-01-01"
    print(f"[Secret Scanner] Searching: {query}\n")
    repos = search_repos(query, limit=10)
    if not repos:
        print("No repos found.")
        return
    print(f"Found {len(repos)} repos. Scanning top 5...\n")
    all_findings = []
    temp_dir = tempfile.mkdtemp(prefix="secret_scan_")
    try:
        for repo in repos[:5]:
            url = repo["html_url"]
            print(f"  Repo: {repo['full_name']} ({repo['stargazers_count']} stars)")
            findings = scan_repo(url, temp_dir)
            all_findings.extend(findings)
            print(f"    -> {len(findings)} finding(s)")
    finally:
        subprocess.run(["rm", "-rf", temp_dir], shell=True, capture_output=True)
    print(f"\n{'='*60}")
    print(f"  TOTAL: {len(all_findings)} secret(s) found")
    print(f"{'='*60}")
    for f in all_findings[:20]:
        print(f"  [{f['type']}] {f['file']}:{f['line']} -> {f['match']}")

if __name__ == "__main__":
    main()
