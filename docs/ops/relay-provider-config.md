# 云端中转（qidi-relay）配置参考

> 适用：云端 `111.231.166.204` 上的 qidi-relay（`systemctl` 单元 `qidi-relay.service`，`WorkingDirectory=/opt/codely-relay`）
> 对外入口：`https://luyou.qidiai.ltd:8443/v1`（客户端一律走这里，**不直连各厂商**）
> 快照时间：2026-10-31 10:45（配置 mtime 同刻）；本文替换 `README.md`/`BLUEPRINT.md` 里关于 provider 的过时描述
> 相关：`docs/ops/luyou-account-pools.md`（账号池分级）、`docs/ops/deepseek-flash-evolution.md`（DeepSeek 线路）、`方案标书/GEO优化/`（私有库，GEO 侧）

---

## 一、部署形态

| 项 | 值 |
|---|---|
| 进程 | `/usr/bin/python3 /opt/codely-relay/main.py`（Flask，systemd 常驻） |
| 端口 | `config.json` 的 `"port": 8765` |
| 配置 | `/opt/codely-relay/config.json`，**`root:root 600`**（改它要 `sudo`） |
| 密钥 | `api_keys[]`（relay 自己的客户端 key，给 qidicode/脚本用）与各 `providers.<name>.api_keys[]`（上游 key）**分开存放** |
| 热加载 | **`_load_providers()` 按 config.json 的 mtime 自动重载**（`proxy_core/providers.py:349`）→ 改配置**不需要重启服务** |
| 重启影响 | 重启会打断所有经 8443 的在跑请求（含其他会话的长任务）；改 provider 一般**不需要**重启 |
| 备份纪律 | 每次改前 `cp config.json config.json.bak-<用途>-<yymmdd-HHMMSS>`；改后**必须回读解析**（见 §四.6） |

---

## 二、provider 配置结构

```jsonc
"providers": {
  "<name>": {
    "base_url": "https://api.example.com/v1",   // 必填；OpenAI 兼容端点须带 /v1
    "models": ["model-a", "model-b"],           // 必填；客户端请求名（大小写敏感）
    "model_aliases": {"pfx/model-a": "model-a"},// 可选；别名 -> 上游真名，转发前重写请求体
    "api_keys": [{"label": "主key", "key": "sk-..."}],
    "enabled": true,                            // 可选，默认 true；false 时 match() 直接跳过
    "media_enabled": false                      // 可选；图像/视频闸门，默认拒绝
  }
}
```

要点：

- **没有 `api_kind`/`type` 字段**，全部按 OpenAI `chat_completions` 走；非 OpenAI 协议需在 `routes/proxy.py` 适配。
- `model_aliases` 的**键**是客户端可见名，**值**是发给上游的真名；命中后 `data["model"] = _amap[...]` 重写（`routes/proxy.py:2880`）。
- `media_enabled` 是 2026-10-28 ¥15 事故（3 条 Seedance + 3 张图）之后加的**默认拒绝**闸门；开它之前先报单价。

---

## 三、名字解析规则（最容易踩的一节）

**`providers.match(model)`（`proxy_core/providers.py:518`）**：

```
按 providers 的插入顺序遍历
  跳过 enabled=false
  model ∈ prov["models"] 或 model ∈ prov["model_aliases"] → 立即返回该 provider
```

由此产生四个硬约束：

1. **大小写敏感**。`52duck` 声明 `Deepseek-flash`（大写 D），`apigoto`/`sensenova` 声明 `deepseek-flash`（小写）——请求小写名**永远不会**到 52duck（实测日志：打到 `www.apigoto.com`）。
2. **先到先得**。新 provider 永远追加在表尾（当前顺序见 §五），所以**它声明任何已被前面 provider 占用的名字 = 永远轮不到它**。
3. **同名跨厂商是常态**：`deepseek-flash`、`deepseek-v4.1-flash`、`glm-5.3`、`qwen3.8-max` 等都被 2 家以上重复声明。
4. **pool 模型不能当兜底目标**：兜底链里的名字若解析到池模型，会打 `模型兜底: X 的备用 'Y' 不是可用外部通道，跳过`（`routes/proxy.py:3235`）。文案有歧义——它真实含义常常是"该 provider 在冷却"或"那是 pool 模型"，不是"这个通道不存在"。

**结论：给新通道起名，优先用独占前缀**（现用前缀：`go/`、`gs/`、`jw/`、`duck/`（待批）、`dimleap/`），不要赌真名。

---

## 四、接入新中转站标准流程

> 本节来自 2026-10-30/31 接 lyrouter、dimleap 的实操，每一步都踩过坑。

**1. 探模型清单 —— 注意分页**
```
GET <base_url>/v1/models
```
dimleap 的响应是自定义结构 `{"success":true,"data":{"models":[...],"total":67,"page":1,"pageSize":20}}`。
**只读第 1 页会得到"只有 20 个模型"的错误结论**（本人 2026-10-31 犯过：漏掉 47 个，恰好漏了 `glm-5.3`）。
一律加 `?page=1&pageSize=100` 并核对 `total`。

**2. 探协议形态**
- 无 key 打 `/v1/models`：能读 → 说明可先配清单；401/403 → 需先拿 key。
- 用假 key 打 `/v1/chat/completions`：`401 invalid or expired API key` = OpenAI 兼容且鉴权正常。
- 确认 `base_url` 是否要带 `/v1`（dimleap 要，且 `/docs` 在 api 主机上是 404）。

**3. 名字抢占检测（必做）**
```python
# 对上游每个 chat 模型名，扫全部 provider 的 models 与 model_aliases
# 有 owner -> 必须走别名；无 owner -> 可用真名
```
（脚本：`target/tmp/dimleap/add_all.py` 内联实现；结论应在文档/提交信息里留档。）

**4. 冲突处理：独占别名**
```jsonc
"model_aliases": {"dimleap/glm-5.3": "glm-5.3"}
```
前缀不得与现有 provider 冲突。**不要**把真名直接加进 `models` 抢占别家（会把现网同名的所有流量改道）。

**5. 未填 key 时：`enabled: false` + 占位 key**
原因：`_FALLBACK_TRIGGER_STATUS = {401,402,403,429,503}`（`routes/proxy.py:3201`）**含 401**。若 enabled=true 而 key 是空的，请求得到 401 → 触发兜底 → **静默换成 agnes 回答，调用方以为线路正常**。
`enabled=false` 的行为已验证：`404 {"code":"model_not_found"}` —— 响亮失败，不兜底。

**6. 写入后必须回读**
`settings.py` 在配置解析失败时会**静默回落到默认值**（`persist` 相关路径同理），所以"写入成功"不等于"配置生效"。标准动作：
```bash
sudo cp config.json config.json.bak-<用途>-$(date +%y%m%d-%H%M%S)
sudo python3 - <<'PY'
import json; c=json.load(open('/opt/codely-relay/config.json'))
print(len(c['providers']), c['providers']['<name>']['enabled'])
PY
```

**7. 验证判据（唯一）**
```
relay 日志出现：POST https://<上游host>/v1/chat/completions "HTTP/1.1 200"
```
- **客户端回了话不算证据**（可能是兜底在回答）。
- 日志：`sudo grep <时间> /opt/codely-relay/proxy.log`；查 `model=<名>` 的请求与紧随的 `POST https://…`。
- 若返回 `model_not_found` / `GROUP_DISABLED` / `400`，先查 §七 的错误码表，**不要**改客户端。

---

## 五、现有 provider 清单（19 个，2026-10-31 快照）

| # | provider | 上游 host | 模型数 | 别名数 | enabled | media | 备注 |
|---|---|---|---|---|---|---|---|
| 1 | `modelscope` | api-inference.modelscope.cn | 37 | 0 | True | - | 魔粒池，每日 00:00 恢复 |
| 2 | `52duck` | api.52duck.cn | 2 | 0 | True | - | **登记名与上游不一致**：登记 `Deepseek-flash`/`GLM-5.2`，该 key 实际只服务 `deepseek-v4.1-flash`（需别名，见 §八） |
| 3 | `stepplan` | api.stepfun.com | 5 | 0 | **False** | - | 阶跃 Step Plan |
| 4 | `openscale` | api.openscale.so | 8 | 0 | True | - | 声明 `deepseek-v4.1-flash`、`z-ai/glm-5.3-flash`（抢同名） |
| 5 | `antling` | api.ant-ling.com | 5 | 0 | True | - | |
| 6 | `apigoto` | www.apigoto.com | 20 | 0 | True | - | **每日推广收益到账、余额按天波动**；欠费日整批 429；抢 `deepseek-flash`/`glm-5.3` |
| 7 | `openrouter` | openrouter.ai | 3 | 0 | True | - | |
| 8 | `sensenova` | token.sensenova.cn | 7 | 0 | True | - | tpm/rpm 滑动窗限流；抢 `deepseek-v4-flash`、`glm-5.2`、`deepseek-v4.1-flash` |
| 9 | `stepfun` | api.stepfun.com | 3 | 0 | **False** | - | |
| 10 | `agnes` | api.agnes-ai.cn | 7 | 0 | True | **True** | 免费档 512K；图像/视频也已开通（受 media 闸门控制） |
| 11 | `intern` | discovery-api.intern-ai.org.cn | 6 | 2 | True | - | 月额度；抢 `kimi-k2.6`、`deepseek-v4-flash-0731`、`deepseek-v4-pro-0813` |
| 12 | `opencode` | 127.0.0.1:4096 | 11 | 0 | True | - | 本机 |
| 13 | `npu` | 127.0.0.1:18765 | 1 | 0 | True | - | 自建 NPU 端点 |
| 14 | `opencode-go` | opencode.ai | 4 | 38 | True | - | **全部 401 `Go requires a credential`（不可用）** |
| 15 | `gs` | generativelanguage.googleapis.com | 14 | 14 | True | - | Google，经首尔 SOCKS 出口 |
| 16 | `jiuwei` | jiuweiapi.top | 5 | 5 | True | - | `jw/*` 别名 |
| 17 | `unisound` | maas-api.unisound.com | 1 | 0 | True | - | 仅 `u2-flash` 有权限 |
| 18 | `lyrouter` | api.lyrouter.com | 27 | 0 | True | **False** | 在表尾 → 同名被前面抢；媒体闸门关 |
| 19 | `dimleap` | api.dimleap.cn | 31 | 12 | **False** | False | 2026-10-31 接入；**免费额度站**；43 个 chat 全可路由；等 key 后开 |

---

## 六、兜底、冷却与注水检测

- **触发兜底的状态**：`401/402/403/429/503`（`routes/proxy.py:3201`）；**最多 3 跳**（`_MAX_FALLBACK_DEPTH = 3`）。
- **`model_fallbacks` 现有 27 条**，形如：
  `Qwen/Qwen3.8-Flash-Next -> ['Qwen/Qwen3.8-27B', 'agnes-3.0-flash', 'gs/gemini-3.5-flash']`。
- **跨族兜底的风险（实际发生过两次）**：编码手任务在 `deepseek-flash` 429 后被兜到 `agnes-3.0-flash`（免费档）跑了 4 轮工具调用，最后撞上 `gs/gemini-3.5-flash` 接不住 `tools` 载荷（400）整链失败。**派编码任务时，目标模型不要配跨族兜底，宁可响亮失败。**
- **注水检测**：relay 会对"请求名 ≠ 返回名"打 WARNING（`注水嫌疑（模型名不符）`）。实测：
  - `bytedance/doubao-seed-2.1-pro` → 返回 `doubao-seed-2-1-pro-260915`（同族全名，良性）
  - `lyrouter/auto` → 返回 `glm-5.2`（**平台自选模型，不可预测，别当确定型号用**）
- **冷却**：provider 失败后进入冷却，冷却期内该 provider 被 `match()` 跳过；日志里的"不是可用外部通道"常常就是冷却中的表现。

---

## 七、排障速查（错误码 → 真实含义 → 处置）

| 现象 | 真实含义 | 处置 |
|---|---|---|
| `404 model_not_found`（relay 返回） | relay 侧没有可路由的 provider（未登记 / 目标 provider `enabled=false` / 名字被抢但你没用别名） | 查 §三/§五；确认 `enabled` 与别名 |
| `403 {"code":"GROUP_DISABLED","message":"API Key 所属分组已停用"}` | **上游中转站返回**，多数是"你请求的模型不在该 key 的分组里"（不是 key 失效） | 到中转站面板核对分组/模型；或换用其实际可服务名（52duck 案例：`deepseek-v4.1-flash`） |
| `429 Credits不足，账户余额 0.1 不足 1 元` | apigoto 欠费（**余额按天恢复**，推广收益到账后当天可用） | 当天重测；不要把 apigoto 当兜底 |
| `429 inference exceeds tpm/rpm limit` | sensenova 滑动窗限流 | 退避或换线 |
| `401 Go requires a credential` | `opencode-go` 上游缺凭证 | 该通道不可用，勿配 |
| `502` + 上游无监听 | 自建端点挂了（例：`/baize-npu/v1/models` 长期 502、`/ws` 代理到 `127.0.0.1:24190` 无进程） | 修上游或摘路由 |
| 请求明明 200 但答案来自别的模型 | 命中兜底链 | 看日志"模型兜底"；给该名**不配**兜底 |

**已知观测缺陷（未修）**：
1. `nginx.conf` 未定义 `log_format`，access log 是默认 combined，**没有 `$host`** → "这个 200 是哪个站点给的"不可查。
2. `go-clicks.log` **有 UA 列**，但 GEO 日报脚本在点击侧只滤自 IP 与 `curl`，爬虫（bingbot/ClaudeBot/PetalBot）被算作真人转化。
3. 对 52duck 每 3 分钟探 OpenAI 式 billing 端点，今天 **1020 次 404** 打在对方身上（它没这个端点），且我们对它的额度状态一直是瞎的。

---

## 八、待办（按优先级，需批准）

1. **52duck 可达性**：加 `"duck/deepseek-v4.1-flash": "deepseek-v4.1-flash"`（否则该通道永远到不了——真名被 sensenova 抢、登记名是幻影）。
2. **`model_fallbacks['deepseek-flash']` 里的旧名 `Deepseek-v4-flash`**（52duck 2026-10-18 改名后失配，兜底永远跳过它）；同时给 `Deepseek-flash` **不配**兜底。
3. **apigoto 的小写 `deepseek-flash`** 是否摘掉：欠费日会白吃 400+ 次 429，但它是"当日可用"的波浪型资源。
4. **billing 探测按 provider 适配**：对 52duck 停止空探。
5. **dimleap 填 key 后**：先验请求名形态（裸名 vs `vendor/name`），再决定是否批量改 `models`/`model_aliases`。

---

## 九、变更记录

| 日期 | 变更 |
|---|---|
| 2026-10-17 | 账号池分级与 `intern` 月额度处理（见 `luyou-account-pools.md`） |
| 2026-10-22 | 成本口径 P-06；52duck 线路改名（`Deepseek-v4-flash` → `Deepseek-flash`），**兜底链未同步** |
| 2026-10-28 | 媒体默认拒绝闸门 `media_enabled`（¥15 事故后） |
| 2026-10-30 | 确认 apigoto 每日推广收益、余额按天波动；小写 `deepseek-flash` 实际由 apigoto 服务 |
| 2026-10-31 | 接入 `dimleap`（43 个 chat：31 真名 + 12 独占别名，含 `dimleap/glm-5.3`、`glm-5.3-flash`）；`enabled=false` 等 key；24 个媒体模型未登记（闸门） |

---

## 十、快速命令

```bash
# 结构自检
sudo python3 -c "import json;c=json.load(open('/opt/codely-relay/config.json'));\
print(len(c['providers']),'providers');[print(k,len(v.get('models',[])),len(v.get('model_aliases',{})),v.get('enabled',True)) for k,v in c['providers'].items()]"

# 某模型会落到哪个 provider（不真的发请求）
sudo python3 -c "
import sys; sys.path.insert(0,'/opt/codely-relay')
from proxy_core.providers import match
p=match('dimleap/glm-5.3'); print(p and p.get('base_url'))"

# 路由实测（把 <KEY> 换成 relay 客户端 key）
curl -s -X POST https://luyou.qidiai.ltd:8443/v1/chat/completions \
  -H "Authorization: Bearer <KEY>" -H 'Content-Type: application/json' \
  -d '{"model":"dimleap/glm-5.3","messages":[{"role":"user","content":"reply PONG"}],"max_tokens":8}'
# 判据：sudo grep 'api.dimleap.cn/v1/chat' /opt/codely-relay/proxy.log | tail -1   -> 200
```