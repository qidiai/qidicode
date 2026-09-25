#!/usr/bin/env python3
"""office-workbench: 浏览器办公工作台（豆包式三区布局）。

零构建链: 标准库 http.server + 单页内嵌 HTML/JS。
数据源: ~/.qidi/office-workspaces/ (office-artifact 登记的任务工作区)。
"""
import argparse
import html
import io
import json
import os
import subprocess
import sys
import threading
import time
import webbrowser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

HOME = os.path.expanduser("~")
WS_ROOT = os.path.join(HOME, ".qidi", "office-workspaces")
CANVAS = os.path.join(HOME, ".qidi", "skills", "office-canvas", "scripts", "canvas.py")

try:
    import mammoth  # docx -> html
    HAS_MAMMOTH = True
except ImportError:
    HAS_MAMMOTH = False
try:
    import openpyxl
    HAS_XLSX = True
except ImportError:
    HAS_XLSX = False

MIME = {
    ".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg",
    ".gif": "image/gif", ".svg": "image/svg+xml", ".webp": "image/webp",
    ".pdf": "application/pdf", ".html": "text/html; charset=utf-8",
    ".txt": "text/plain; charset=utf-8", ".md": "text/plain; charset=utf-8",
    ".csv": "text/plain; charset=utf-8",
}


def list_workspaces():
    out = []
    if not os.path.isdir(WS_ROOT):
        return out
    for n in sorted(os.listdir(WS_ROOT)):
        m = os.path.join(WS_ROOT, n, "manifest.json")
        if os.path.isfile(m):
            try:
                with open(m, encoding="utf-8") as f:
                    d = json.load(f)
                out.append({"task": n, "count": len(d.get("artifacts", []))})
            except Exception:
                pass
    return out


def load_manifest(task):
    m = os.path.join(WS_ROOT, task, "manifest.json")
    if not os.path.isfile(m):
        return {"task": task, "artifacts": []}
    with open(m, encoding="utf-8") as f:
        return json.load(f)


def safe_path(task, rel_or_abs):
    """仅允许已登记产物路径（大小写不敏感，适配 Windows）。"""
    data = load_manifest(task)
    want = os.path.normcase(os.path.abspath(os.path.expanduser(rel_or_abs)))
    for a in data.get("artifacts", []):
        if os.path.normcase(os.path.abspath(a["path"])) == want:
            return want
    return None


def docx_to_html(path):
    if not HAS_MAMMOTH:
        return None
    with open(path, "rb") as f:
        r = mammoth.convert_to_html(f)
    return r.value


# xlsx 预览阈值: 超过此限制截断并提示用户下载原文件查看
_XLSX_ROW_LIMIT = 200
_XLSX_COL_LIMIT = 50


def xlsx_to_html(path):
    if not HAS_XLSX:
        return None
    wb = openpyxl.load_workbook(path, data_only=True)
    parts = []
    for ws in wb.worksheets:
        total_rows = ws.max_row or 0
        total_cols = ws.max_column or 0
        row_limit = min(_XLSX_ROW_LIMIT, max(1, total_rows))
        col_limit = min(_XLSX_COL_LIMIT, max(1, total_cols))
        header = f"<h3>{html.escape(ws.title)}</h3>"
        if total_rows > _XLSX_ROW_LIMIT or total_cols > _XLSX_COL_LIMIT:
            header += (f'<div style="color:#b92828;padding:4px 0;font-size:12px">'
                       f'表格过大 ({total_rows}行 × {total_cols}列) 已截断至 '
                       f'{row_limit}行 × {col_limit}列，请下载原文件查看</div>')
        parts.append(header)
        parts.append('<table class="xlsx">')
        for row in ws.iter_rows(max_row=row_limit, max_col=col_limit):
            cells = "".join(
                f"<td>{html.escape(str(c.value) if c.value is not None else '')}</td>"
                for c in row)
            parts.append(f"<tr>{cells}</tr>")
        parts.append("</table>")
    return "".join(parts)


PAGE = """<!doctype html><html lang=zh><head><meta charset=utf-8>
<title>QIDI 办公工作台</title>
<style>
:root{--bg:#0f1115;--panel:#161a22;--card:#1d222d;--line:#262c3a;
--txt:#dfe4ee;--dim:#8b93a7;--acc:#4f8cff;--ok:#39c98e;--warn:#ffb454}
*{box-sizing:border-box;margin:0;padding:0}
body{background:var(--bg);color:var(--txt);font:14px/1.6 "Segoe UI",system-ui,sans-serif;
display:flex;height:100vh;overflow:hidden}
#left{width:210px;background:var(--panel);border-right:1px solid var(--line);
display:flex;flex-direction:column}
#left h1{font-size:15px;padding:14px 16px;border-bottom:1px solid var(--line)}
#tasks{flex:1;overflow-y:auto;padding:8px}
.task{padding:9px 12px;border-radius:8px;cursor:pointer;color:var(--dim);
display:flex;justify-content:space-between;align-items:center;margin-bottom:2px}
.task:hover{background:var(--card)}
.task.on{background:var(--card);color:var(--txt)}
.task .n{background:var(--line);border-radius:9px;font-size:11px;padding:0 7px}
#newtask{margin:10px;padding:8px;border:1px dashed var(--line);border-radius:8px;
background:none;color:var(--dim);cursor:pointer}
#newtask:hover{color:var(--acc);border-color:var(--acc)}
#mid{flex:1;display:flex;flex-direction:column;min-width:0}
#top{height:46px;background:var(--panel);border-bottom:1px solid var(--line);
display:flex;align-items:center;padding:0 16px;gap:10px}
#top .t{font-weight:600}
#top .sp{flex:1}
button.b{background:var(--card);border:1px solid var(--line);color:var(--txt);
border-radius:7px;padding:5px 13px;cursor:pointer}
button.b:hover{border-color:var(--acc);color:var(--acc)}
#view{flex:1;overflow:auto;background:var(--bg)}
#view .empty{color:var(--dim);padding:60px;text-align:center}
#view iframe{width:100%;height:100%;border:0}
#view img{max-width:92%;display:block;margin:24px auto;background:#fff}
#view .doc{max-width:860px;margin:24px auto;background:#fff;color:#111;
padding:48px 56px;border-radius:6px;min-height:70vh}
#view table.xlsx{border-collapse:collapse;margin:12px auto;background:#fff;color:#111}
#view table.xlsx td{border:1px solid #bbb;padding:4px 10px;font-size:13px}
#view pre{padding:24px;white-space:pre-wrap;font-family:Consolas,monospace;color:var(--txt)}
#right{width:260px;background:var(--panel);border-left:1px solid var(--line);
display:flex;flex-direction:column}
#right h2{font-size:13px;padding:12px 14px;border-bottom:1px solid var(--line);color:var(--dim)}
#cards{flex:1;overflow-y:auto;padding:8px}
.card{background:var(--card);border:1px solid var(--line);border-radius:9px;
padding:10px 12px;margin-bottom:8px;cursor:pointer}
.card:hover{border-color:var(--acc)}
.card.on{border-color:var(--acc)}
.card .nm{font-size:13px;word-break:break-all;margin-bottom:4px}
.card .mt{font-size:11px;color:var(--dim);display:flex;justify-content:space-between}
.card .ops{margin-top:7px;display:flex;gap:6px}
.card .ops button{flex:1;font-size:11px;padding:3px 0;border-radius:6px;
border:1px solid var(--line);background:none;color:var(--dim);cursor:pointer}
.card .ops button:hover{color:var(--acc);border-color:var(--acc)}
.hint{padding:10px 14px;font-size:11px;color:var(--dim);border-top:1px solid var(--line)}
</style></head><body>
<div id=left><h1>🗂 任务工作区</h1><div id=tasks></div>
<button id=newtask>＋ 新建工作区</button></div>
<div id=mid><div id=top>
<span class=t id=curtask>—</span><span class=sp></span>
<button class=b onclick=refresh()>⟳ 刷新</button>
<button class=b onclick=canvasCollect()>📝 读批注</button>
<button class=b onclick=canvasVerify()>✅ 验响应</button>
</div><div id=view><div class=empty>左侧选择工作区，右侧点击产物预览</div></div></div>
<div id=right><h2>📦 产物</h2><div id=cards></div>
<div class=hint>预览=工作台内打开<br>打开=系统程序(可加批注)</div></div>
<script>
let cur=null,curFile=null;
async function api(p,opt){const r=await fetch(p,opt);return r.json()}
async function loadTasks(){
 const d=await api('/api/workspaces');
 const el=document.getElementById('tasks');
 el.innerHTML=d.map(t=>`<div class="task ${cur==t.task?'on':''}" onclick="sel(this.dataset.task)" data-task="${esc(t.task)}"><span>${esc(t.task)}</span><span class=n>${t.count}</span></div>`).join('');
 if(!cur&&d.length)sel(d[0].task);}
function esc(s){return s.replace(/'/g,"\\\\'").replace(/</g,'&lt;')}
async function sel(t){cur=t;document.getElementById('curtask').textContent=t;
 await loadCards();loadTasks();}
async function loadCards(){
 const d=await api('/api/artifacts?task='+encodeURIComponent(cur));
 const el=document.getElementById('cards');
 if(!d.artifacts.length){el.innerHTML='<div class=card><div class=nm style=color:var(--dim)>暂无产物<br><br>用 card.py add 登记</div></div>';return}
 el.innerHTML=d.artifacts.map((a,i)=>`<div class="card ${curFile==a.path?'on':''}" onclick="pv(this.dataset.path)" data-path="${esc2(a.path)}">
 <div class=nm>${esc2(a.name)}</div>
 <div class=mt><span>${a.skill||''}</span><span>${a.kb}</span></div>
 <div class=ops><button onclick="event.stopPropagation();sysopen(this.dataset.path)" data-path="${esc2(a.path)}">打开</button></div>
 </div>`).join('');}
function esc2(s){return s.replace(/</g,'&lt;')}
async function pv(p){curFile=p;
 const v=document.getElementById('view');
 v.innerHTML='<div class=empty>加载中…</div>';
 const r=await api('/api/preview?task='+encodeURIComponent(cur)+'&path='+encodeURIComponent(p));
 if(!r.ok){v.innerHTML='<div class=empty>'+(r.msg||'不支持预览')+'<br><br><button class=b onclick=sysopen("'+esc(p)+'")>用系统程序打开</button></div>';return}
 if(r.type=='html')v.innerHTML=r.data;
 else if(r.type=='img')v.innerHTML='<img src="'+r.url+'">';
 else if(r.type=='pdf')v.innerHTML='<iframe src="'+r.url+'">';
 else if(r.type=='text')v.innerHTML='<pre>'+r.data+'</pre>';
 loadCards();}
async function sysopen(p){await api('/api/open',{method:'POST',body:JSON.stringify({path:p})});}
async function refresh(){await loadTasks();if(cur)await loadCards();}
async function canvasCollect(){
 if(!curFile){alert('先在右侧选择一个 docx 产物');return}
 const r=await api('/api/canvas',{method:'POST',body:JSON.stringify({cmd:'collect',path:curFile})});
 const w=window.open('about:blank');w.document.write('<pre style="padding:24px;font:13px Consolas">'+(r.out||r.msg)+'</pre>');}
async function canvasVerify(){
 if(!curFile){alert('先在右侧选择一个 docx 产物');return}
 const r=await api('/api/canvas',{method:'POST',body:JSON.stringify({cmd:'verify',path:curFile})});
 alert(r.out||r.msg);}
document.getElementById('newtask').onclick=async()=>{
 const n=prompt('新工作区名称:');if(!n)return;
 await api('/api/newtask',{method:'POST',body:JSON.stringify({task:n})});sel(n);};
loadTasks();
</script></body></html>"""


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _send(self, code, body, ctype="application/json; charset=utf-8"):
        if isinstance(body, str):
            body = body.encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        u = urlparse(self.path)
        if u.path == "/" or u.path == "/index.html":
            return self._send(200, PAGE, "text/html; charset=utf-8")
        q = parse_qs(u.query)
        if u.path == "/api/workspaces":
            return self._send(200, json.dumps(list_workspaces()))
        if u.path == "/api/artifacts":
            task = (q.get("task") or [""])[0]
            d = load_manifest(task)
            for a in d.get("artifacts", []):
                a["kb"] = f"{a.get('size',0)/1024:.0f}K"
            return self._send(200, json.dumps(d))
        if u.path == "/api/preview":
            task = (q.get("task") or [""])[0]
            path = safe_path(task, (q.get("path") or [""])[0])
            if not path:
                return self._send(200, json.dumps({"ok": False, "msg": "未登记的产物"}))
            ext = os.path.splitext(path)[1].lower()
            try:
                if ext == ".docx":
                    h = docx_to_html(path)
                    if h:
                        # 加载批注用于锚点高亮
                        comments_for_highlight = []
                        collect_root = os.path.join(os.path.expanduser("~/.qidi"), "office-canvas", "collect")
                        if os.path.isdir(collect_root):
                            base = os.path.splitext(os.path.basename(path))[0]
                            matches = []
                            for fn in os.listdir(collect_root):
                                if fn.startswith(base) and fn.endswith(".json"):
                                    matches.append(os.path.join(collect_root, fn))
                            if matches:
                                latest = max(matches, key=os.path.getmtime)
                                try:
                                    with open(latest, encoding="utf-8") as f2:
                                        comments_for_highlight = json.load(f2)
                                except Exception:
                                    pass
                        payload = {"ok": True, "type": "html",
                                   "data": f'<div class=doc>{h}</div>'}
                        if comments_for_highlight:
                            payload["comments"] = comments_for_highlight
                        return self._send(200, json.dumps(payload))
                    return self._send(200, json.dumps({"ok": False, "msg": "安装 mammoth 后可预览 docx"}))
                if ext in (".xlsx", ".xls"):
                    h = xlsx_to_html(path)
                    if h:
                        return self._send(200, json.dumps(
                            {"ok": True, "type": "html",
                             "data": f'<div class=doc>{h}</div>'}))
                    return self._send(200, json.dumps({"ok": False, "msg": "安装 openpyxl 后可预览表格"}))
                if ext in MIME:
                    if MIME[ext].startswith("image") or MIME[ext] == "application/pdf":
                        with open(path, "rb") as f:
                            return self._send(200, f.read(), MIME[ext])
                    if ext in (".txt", ".md", ".csv"):
                        with open(path, encoding="utf-8", errors="replace") as f:
                            return self._send(200, json.dumps(
                                {"ok": True, "type": "text", "data": f.read()[:200000]}))
                return self._send(200, json.dumps({"ok": False, "msg": f"暂不支持 {ext} 预览"}))
            except Exception as e:
                return self._send(200, json.dumps({"ok": False, "msg": str(e)}))
        if self.path.startswith("/api/comments"):
            # 批注数据端点：返回文件的 collect 存档 JSON（供前端锚点高亮）
            query = urlparse(self.path).query
            params = parse_qs(query)
            task = (params.get("task") or [""])[0]
            path_param = (params.get("path") or [""])[0]
            file_path = safe_path(task, path_param) if path_param else None
            if not file_path:
                return self._send(200, json.dumps({"ok": False, "msg": "未找到文件"}))
            collect_root = os.path.join(os.path.expanduser("~/.qidi"), "office-canvas", "collect")
            comments = []
            if os.path.isdir(collect_root):
                base = os.path.splitext(os.path.basename(file_path))[0]
                matches = []
                for fn in os.listdir(collect_root):
                    if fn.startswith(base) and fn.endswith(".json"):
                        matches.append(os.path.join(collect_root, fn))
                if matches:
                    latest = max(matches, key=os.path.getmtime)
                    try:
                        with open(latest, encoding="utf-8") as f2:
                            comments = json.load(f2)
                    except Exception:
                        pass
            return self._send(200, json.dumps({"ok": True, "comments": comments}))

        return self._send(404, "{}")

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        try:
            body = json.loads(self.rfile.read(n) or b"{}")
        except Exception:
            body = {}
        if self.path == "/api/open":
            p = os.path.expanduser(body.get("path", ""))
            if os.path.isfile(p):
                if sys.platform == "win32":
                    os.startfile(p)  # noqa: S606
                else:
                    subprocess.run(["open" if sys.platform == "darwin" else "xdg-open", p], check=False)
                return self._send(200, '{"ok":true}')
            return self._send(200, '{"ok":false}')
        if self.path == "/api/newtask":
            t = body.get("task", "").strip()
            if t and not os.path.sep in t and ".." not in t:
                os.makedirs(os.path.join(WS_ROOT, t), exist_ok=True)
                mp = os.path.join(WS_ROOT, t, "manifest.json")
                if not os.path.isfile(mp):
                    with open(mp, "w", encoding="utf-8") as f:
                        json.dump({"task": t, "artifacts": []}, f, ensure_ascii=False)
                return self._send(200, '{"ok":true}')
            return self._send(200, '{"ok":false}')
        if self.path == "/api/canvas":
            # 白名单: 仅允许合法子命令，阻断任意注入
            ALLOWED_CANVAS_CMDS = frozenset({"collect", "verify"})
            cmd = body.get("cmd")
            if cmd not in ALLOWED_CANVAS_CMDS:
                return self._send(200, json.dumps({"ok": False,
                                                  "msg": "命令不在白名单中"}))
            p = os.path.expanduser(body.get("path", ""))
            if not os.path.isfile(p) or not os.path.isfile(CANVAS):
                return self._send(200, json.dumps({"ok": False,
                                                  "msg": "文件或 canvas.py 路径不存在"}))
            r = subprocess.run([sys.executable, CANVAS, cmd, p],
                               capture_output=True, timeout=120)
            out = (r.stdout or b"").decode("utf-8", errors="replace")
            return self._send(200, json.dumps({"ok": True, "out": out}))
        return self._send(404, "{}")


def main():
    ap = argparse.ArgumentParser(description="office-workbench 浏览器工作台")
    ap.add_argument("--port", type=int, default=8710)
    ap.add_argument("--no-browser", action="store_true")
    args = ap.parse_args()

    srv = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    url = f"http://127.0.0.1:{args.port}"
    print(f"QIDI 办公工作台: {url}  (Ctrl+C 退出)")
    print(f"数据源: {WS_ROOT}")
    if not HAS_MAMMOTH:
        print("提示: pip install mammoth 可启用 docx 预览")
    if not HAS_XLSX:
        print("提示: pip install openpyxl 可启用表格预览")
    if not args.no_browser:
        threading.Timer(0.6, lambda: webbrowser.open(url)).start()
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        print("\n已退出")
        srv.server_close()


if __name__ == "__main__":
    main()
