# SPEC-WB1 — workbuddy2api.py + wb_checkin.py 全量规格书

> 指挥官已完成协议逆向+实测。核心事实全部经过真机验证（标注 ✓实测）。
> 编码手铁律：只做规格内的事；自测跑真实命令贴真实输出；不要 git commit。
> 结果以 `CODER RESULT: DONE` 或 `CODER RESULT: SPEC GAP` 结尾。

## 一、背景（已验证事实）

WorkBuddy = 腾讯 CodeBuddy 团队桌面 AI 编程助手（Electron），业务 API 在 `https://copilot.tencent.com`：
- 模型调用：**Anthropic Messages 协议** `POST /v1/messages`，**仅支持流式**（stream:false 返回 400 code=11101）✓实测
- 签到：`POST /v2/billing/meter/daily-checkin`（body `{}`）✓实测（HTTP 400 code=10001 "今天已签到" = 端点与鉴权正确）
- 签到状态：`POST /v2/billing/meter/checkin-status` ✓实测 200
- 余额：`POST /v2/billing/meter/get-user-resource` ✓实测 200（2808 credits / 9月周期）
- 鉴权头（✓实测有效，无需其它头）：
  ```
  Authorization: Bearer <accessToken>
  X-User-Id: <uid>
  Content-Type: application/json
  ```
- **Token 来源（关键）**：明文 JSON 文件
  `C:\Users\28970\AppData\Local\CodeBuddyExtension\Data\Public\auth\workbuddy-desktop.info`
  结构：`{account:{uid,...}, auth:{accessToken, refreshToken, expiresIn:5184000(60天), tokenType:"Bearer", ...}}`
  **app 运行时会自动刷新此文件** → 适配器每次请求重读该文件即可自动续期，无需硬编码。
- 模型清单（48 个内置，credits 倍率）：`hy3`(x0.00免费) `hy4-preview`(x0.00) `hy3-x`(x0.05) `hy4-preview-x`(x0.29) `glm-5.3-flash`(x0.06) `deepseek-v4-flash`(x0.17) `minimax-m3`(x0.25) `minimax-m2.5`(x0.18) `fast-model`(x0.21) `balanced-model`(x0.65) `deep-model`(x1.20) `glm-5.3`(x0.79) `glm-5.2`(x0.79) `glm-5.1`(x0.79) `glm-4.6v`(x0.11) `glm-4.6`(x0.23) `glm-5v-turbo`(x0.71) `glm-5.0-turbo`(x0.95) `kimi-k3-1`(x1.62) `kimi-k2.7`(x0.57) `kimi-k2.6`(x0.52) `kimi-k2.5`(x0.45) `kimi-k2-thinking`(x0.54) `minimax-m2.7`(x0.26) `deepseek-v4-pro`(x0.51) `deepseek-v3-2-volc`(x0.29) `deepseek-v3-1-volc`(x0.52) `deepseek-v3-1-lkeap`(x0.52) `deepseek-v3-1`(x0.52) `deepseek-v3-0324-lkeap`(x0.52) —— 30 个对外（排除 codewise-*/hunyuan-image/completion-gf/internal 等非对话模型）

## 二、改动范围

目录 `G:\程序开发\本地局域网中转-v3\wb\`（新建）：
1. `workbuddy2api.py` — OpenAI→Anthropic 协议适配器（单文件，仅 aiohttp，~350 行）
2. `wb_checkin.py` — 签到脚本（纯标准库）
3. `run_checkin.vbs` — 计划任务无窗口包装（纯 ASCII）
4. `README.md` — 简要说明

**不许动**：中转的 config.json / proxy_core / 路由（中转接入由指挥官随后做）；不许动 CodeBuddyExtension 的任何文件（只读）。

## 三、workbuddy2api.py 规格

### 3.1 常量与配置
```python
AUTH_FILE = r"C:\Users\28970\AppData\Local\CodeBuddyExtension\Data\Public\auth\workbuddy-desktop.info"
UPSTREAM = "https://copilot.tencent.com"
LISTEN_HOST = "127.0.0.1"; LISTEN_PORT = 8768
```
监听 127.0.0.1:8768（vibex2api 在 8787，不冲突）。

### 3.2 Token 读取（每次请求实时读文件）
```python
def load_auth():
    d = json.load(open(AUTH_FILE, encoding="utf-8"))
    return d["auth"]["accessToken"], d["account"]["uid"]
```
每次上游请求都调用（不缓存）→ app 刷新 token 后适配器自动跟进，**永不过期**。读失败抛 502 并提示"请登录 WorkBuddy 桌面端"。

### 3.3 端点

**GET /v1/models** → 30 模型（3.6 清单），OpenAI 格式，`owned_by:"workbuddy"`，附 `wb_credits` 字段（如 "x0.00"）。

**GET /healthz** → `{"status":"ok","token_configured":true,"uid_prefix":"<uid前8位>"}`

**POST /v1/chat/completions**（核心转换）：
1. 解析 OpenAI 请求：model / messages / stream / max_tokens
2. model 白名单校验：不在 30 清单 → 400（列出可用模型）
3. 转 Anthropic Messages：
   - system 消息拼接 → `"system": "<文本>"`（顶层字段）
   - 其余 user/assistant → `"messages":[{"role","content"}]`（content 为字符串；若 OpenAI content 是数组则按 3.4 提取文本）
   - `"max_tokens"`：OpenAI max_tokens 缺省 → 4096；上限 64000
   - `"stream": true`（**强制**，上游只支持流式）
4. 上游请求：`POST {UPSTREAM}/v1/messages`，头 = 3.2 + `Accept: text/event-stream`
5. **流式响应（stream=true）**：把上游 Anthropic SSE 翻译成 OpenAI SSE：
   - message_start → 首块 `{"choices":[{"delta":{"role":"assistant"},"index":0}]}`（含 id=`chatcmpl-`+uuid16, model 回显）
   - content_block_delta / thinking_delta → 若请求带 `"include_thinking": true`（自定义扩展字段）→ `delta.reasoning_content`；默认**丢弃 thinking**
   - content_block_delta / text_delta → `delta.content`
   - message_delta.usage → 记录（input_tokens/output_tokens）
   - message_stop → 尾块 `finish_reason:"stop"` + `usage:{prompt_tokens,completion_tokens,total_tokens}` + `data: [DONE]`
   - 上游 SSE 每行 `event: X\ndata: {...}`；解析 `data:` 行 JSON
6. **非流式（stream=false/缺省）**：内部仍发流式，聚合 text_delta → 完整 OpenAI completion JSON 返回（content=拼接文本，usage 同上）
7. 上游错误：非 200 → 透传状态码与 body（`{"error":{"message":...}}` 格式包装）；body 里 `type:"error"` → 502

### 3.4 content 数组提取
OpenAI content 为 list 时：取各块 `.text` 拼接；`image_url` 块 → 忽略并计入占位（Anthropic 侧暂不支持图，hy3 系是文本模型）。

### 3.5 并发与超时
- 每请求独立 aiohttp ClientSession（或共享一个全局 session，二选一）
- 上游超时：连接 30s / 总 300s（deep-model 慢）
- 信号量并发上限 4，满则 429

### 3.6 对外模型清单（30 个，id 原样，**不加前缀**——中转侧会加 wb- 前缀映射，适配器内保持原名）
hy3, hy4-preview, hy3-x, hy4-preview-x, glm-5.3-flash, deepseek-v4-flash, minimax-m2.5, minimax-m2.7, minimax-m3, fast-model, balanced-model, deep-model, glm-5.3, glm-5.2, glm-5.1, glm-5.0-turbo, glm-4.6v, glm-4.6, glm-5v-turbo, kimi-k3-1, kimi-k2.7, kimi-k2.6, kimi-k2.5, kimi-k2-thinking, deepseek-v4-pro, deepseek-v3-2-volc, deepseek-v3-1-volc, deepseek-v3-1-lkeap, deepseek-v3-1, deepseek-v3-0324-lkeap
（credits 倍率常量字典照抄第一节）

## 四、wb_checkin.py 规格（纯标准库）

```
流程:
1. load_auth()（同 3.2，AUTH_FILE 路径一致）
2. POST /v2/billing/meter/checkin-status → 解析 data
   - HTTP 非200 / code!=0 → 日志 "status_check_failed: <body[:200]>" 退出 2
3. POST /v2/billing/meter/daily-checkin → 解析响应
   - code==0 → 日志 "CHECKIN_OK <data 摘要(credit/streak 字段若有)>"
   - code==10001 或 msg 含 "已签到" → 日志 "ALREADY_CHECKED_IN"（正常，退出 0）
   - 其它 → 日志 "CHECKIN_FAIL <code> <msg>" 退出 1
4. 顺带 GET 式调用 get-user-resource（POST）解析 TotalDosage → 日志 "credits_total=<值>"
   （响应结构 data.Response.Data.TotalDosage，容错：解析失败写 "credits_total=unknown"）
日志: 同目录 checkin.log 追加一行 "YYYY-MM-DD HH:MM:SS  <结果>"
退出码: 0=成功或已签 / 1=签到失败 / 2=状态检查失败
全部 HTTP 头同 3.2；超时 30s
stdout 编码: try reconfigure utf-8
```

## 五、run_checkin.vbs（纯 ASCII，模板同 vibex2api 的 run_sentinel.vbs）

```vbs
' Launch wb_checkin.py with no console window.
Set fso = CreateObject("Scripting.FileSystemObject")
Set sh  = CreateObject("WScript.Shell")
here = fso.GetParentFolderName(WScript.ScriptFullName)
sh.Run """C:\Program Files\Python312\python.exe"" """ & here & "\wb_checkin.py""", 0, False
```
注意引号配对（脚本路径要成对引号：`""..."" & here & """\wb_checkin.py"""`——按 vibex2api/run_sentinel.vbs 的正确形态照抄改文件名）。

## 六、README.md（简短）

内容：两段式——① 签到自动化（计划任务注册命令 `schtasks /create /f /tn "wb-checkin" /tr "wscript.exe <path>\run_checkin.vbs" /sc daily /st 09:30`，日志 checkin.log）② 反代服务（python workbuddy2api.py 启动，127.0.0.1:8768/v1，中转 providers 配置示例 base_url=http://127.0.0.1:8768/v1，模型加 wb- 前缀说明，token 自动跟随桌面端 60 天免维护）。附风险声明（非官方用法、 credits 倍率即计费倍率、hy3/hy4-preview x0.00 免费）。

## 七、自测验收（真实命令+输出）

1. `python -c "import ast; ast.parse(open(r'G:\程序开发\本地局域网中转-v3\wb\workbuddy2api.py',encoding='utf-8').read()); print('SYNTAX OK')"` + wb_checkin.py 同样
2. `python G:\程序开发\本地局域网中转-v3\wb\wb_checkin.py` → 今日已签 → 贴日志行（ALREADY_CHECKED_IN）退出码 0
3. 启动适配器（后台）→ 打：
   - `GET http://127.0.0.1:8768/healthz` → 200
   - `GET http://127.0.0.1:8768/v1/models` → 30 个模型
   - `POST /v1/chat/completions` 非流式（model=hy3, max_tokens=32, "只回复两个字：收到"）→ 200 + content 非空 + usage（贴全文）
   - `POST /v1/chat/completions` 流式（stream=true, model=glm-5.3-flash）→ 贴 SSE 前 6 行（验证 OpenAI chunk 格式）
   - 非法 model → 400
4. 测完杀掉适配器进程，贴 service 停止证明
5. `git status`（在中转目录跑）→ 新增仅 wb/ 目录
