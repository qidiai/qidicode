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

**6. 空 `models` 现在是合法状态（2026-11-01 修复）**
旧行为：`_load_providers()` 要求 `base_url and keys and models` 三者齐备，否则**整个 provider 被丢弃**并打 WARNING。而控制台"添加通道"表单只收集 `name / base_url / api_key / enabled`（`static/app.js:950-1000`），写入的 `models` 恒为 `[]` → **新建的通道永远不出现在列表里，也就永远填不上模型**（先有鸡还是先有蛋）。
现行为（`proxy_core/providers.py:440`，`if base_url and keys:`）：空 `models` 的 provider 正常加载、卡片可见，再点「↻ 刷新模型」从上游 `/v1/models` 回填。
- 已验证：临时注入 `models: []` 的探针 provider，加载数 21 → 22 且该 provider 在列；移除后回到 21（配置逐字节还原）。
- 副作用审查：全部 `models` 消费者（`match`、`all_models`、`water.py`、`routes/admin.py`、`routes/proxy.py:1377`）都走 `.get("models") or []`，空列表不会抛 `KeyError`，只是不可路由。
- 因此**空 `models` + `enabled=false` 现在是推荐的"先建卡、后填模型"写法**，不再需要绕过控制台手改 JSON。

**7. 写入后必须回读**`settings.py` 在配置解析失败时会**静默回落到默认值**（`persist` 相关路径同理），所以"写入成功"不等于"配置生效"。标准动作：
```bash
sudo cp config.json config.json.bak-<用途>-$(date +%y%m%d-%H%M%S)
sudo python3 - <<'PY'
import json; c=json.load(open('/opt/codely-relay/config.json'))
print(len(c['providers']), c['providers']['<name>']['enabled'])
PY
```

**8. 验证判据（唯一）**
```
relay 日志出现：POST https://<上游host>/v1/chat/completions "HTTP/1.1 200"
```
- **客户端回了话不算证据**（可能是兜底在回答）。
- 日志：`sudo grep <时间> /opt/codely-relay/proxy.log`；查 `model=<名>` 的请求与紧随的 `POST https://…`。
- 若返回 `model_not_found` / `GROUP_DISABLED` / `400`，先查 §七 的错误码表，**不要**改客户端。

---

## 五、现有 provider 清单（21 个，2026-11-01 快照 / `sudo python3 /tmp/list.py` 实测）

| # | provider | 上游 host | 模型数 | 别名数 | enabled | media | 备注 |
|---|---|---|---|---|---|---|---|
| 1 | `amd` | developer.amd.com.cn | 15 | 15 | True | - | 2026-10-31 接入；别名 `amd/*`；**上游当日 500/502，待厂商修复** |
| 2 | `kimi-ai` | api.kimi.com/coding/v1 | 6 | 6 | True | False | Kimi **Code** 订阅线（`sk-kimi-*`）；实测仅 `kimi-for-coding` / `k3-256k` 200，其余 403 套餐档位 |
| 3 | `modelscope` | api-inference.modelscope.cn | 37 | 0 | True | - | 魔粒池，每日 00:00 恢复 |
| 4 | `52duck` | api.52duck.cn | 2 | 0 | True | - | **登记名与上游不一致**：登记 `Deepseek-flash`/`GLM-5.2`，该 key 实际只服务 `deepseek-v4.1-flash`（需别名，见 §八） |
| 5 | `stepplan` | api.stepfun.com | 5 | 0 | **False** | - | 阶跃 Step Plan |
| 6 | `openscale` | api.openscale.so | 8 | 0 | True | - | 声明 `deepseek-v4.1-flash`、`z-ai/glm-5.3-flash`（抢同名） |
| 7 | `antling` | api.ant-ling.com | 5 | 0 | True | - | |
| 8 | `apigoto` | www.apigoto.com | 20 | 0 | True | - | **每日推广收益到账、余额按天波动**；欠费日整批 429；抢 `deepseek-flash`/`glm-5.3` |
| 9 | `openrouter` | openrouter.ai | 3 | 0 | True | - | |
| 10 | `sensenova` | token.sensenova.cn | 7 | 0 | True | - | tpm/rpm 滑动窗限流；抢 `deepseek-v4-flash`、`glm-5.2`、`deepseek-v4.1-flash` |
| 11 | `stepfun` | api.stepfun.com | 3 | 0 | **False** | - | |
| 12 | `agnes` | api.agnes-ai.cn | 7 | 0 | True | **True** | 免费档 512K；图像/视频也已开通（受 media 闸门控制） |
| 13 | `intern` | discovery-api.intern-ai.org.cn | 6 | 2 | True | - | 月额度；抢 `kimi-k2.6`、`deepseek-v4-flash-0731`、`deepseek-v4-pro-0813` |
| 14 | `opencode` | 127.0.0.1:4096 | 11 | 0 | True | - | 本机 |
| 15 | `npu` | 127.0.0.1:18765 | 1 | 0 | True | - | 自建 NPU 端点 |
| 16 | `opencode-go` | opencode.ai | 4 | 38 | True | - | **全部 401 `Go requires a credential`（不可用）** |
| 17 | `gs` | generativelanguage.googleapis.com | 14 | 14 | True | - | Google，经首尔 SOCKS 出口 |
| 18 | `jiuwei` | jiuweiapi.top | 5 | 5 | True | - | `jw/*` 别名 |
| 19 | `unisound` | maas-api.unisound.com | 1 | 0 | True | - | 仅 `u2-flash` 有权限 |
| 20 | `lyrouter` | api.lyrouter.com | 27 | 0 | True | **False** | 在表尾 → 同名被前面抢；媒体闸门关 |
| 21 | `dimleap` | api.dimleap.cn | 18 | 12 | True | False | 2026-10-31 接入；**免费额度站**；18 个模型是用户裁剪后的口径（勿恢复 43 个）；`dimleap/*` 别名 |

---

## 六、兜底、冷却与注水检测

- **触发兜底的状态**：`401/402/403/429/503`（`routes/proxy.py:3201`）；**最多 3 跳**（`_MAX_FALLBACK_DEPTH = 3`）。
- **`model_fallbacks` 现有 27 条**，形如：
  `Qwen/Qwen3.8-Flash-Next -> ['Qwen/Qwen3.8-27B', 'amd/Qwen3.8-27B']`。
  （2026-11-04 后已无 `gs/*` 跳点，见 §6.4。）
- **跨族兜底的风险（实际发生过两次）**：编码手任务在 `deepseek-flash` 429 后被兜到 `agnes-3.0-flash`（免费档）跑了 4 轮工具调用，最后撞上 `gs/gemini-3.5-flash` 接不住 `tools` 载荷（400）整链失败。**派编码任务时，目标模型不要配跨族兜底，宁可响亮失败。**

### 6.1 2026-11-04 兜底链重构（用户裁定：agnes 能力不足，会造成信息污染）

**决策**：`agnes-3.0-flash` 从**全部 12 条**兜底链移除，替换为 amd 同族**真模型**。
**依据**（用户原话）："它的能力太差了，会造成信息污染的"；"兜底链 amd 的模型 deepseek-v4.1-flash 不要用假模型代替"。

| 主模型 | 替换前 → 替换后 |
|---|---|
| `deepseek-ai/DeepSeek-V4.1-Flash-1017` | `deepseek-flash` → ~~`agnes-3.0-flash`~~ **`amd/DeepSeek-V4.1-Flash`** → `gs/gemini-3.5-flash` → `deepseek/deepseek-v4.1-flash-1017` |
| `Qwen/Qwen3.8-27B` | `qwen3.8-max` → **`amd/Qwen3.8-27B`** → `gs/...` → `qwen/qwen3.8-max` |
| `Qwen/Qwen3.8-Flash-Next` | `Qwen/Qwen3.8-27B` → **`amd/Qwen3.8-27B`** → `gs/...` |
| `Qwen/Qwen3.5-397B-A17B` | `Qwen/Qwen3.8-27B` → **`amd/Qwen3.8-27B`** → `gs/...` |
| `ZhipuAI/GLM-5.2` | `glm-5.3` → **`amd/GLM-5.3-Flash`** → `gs/...` → `z-ai/glm-5.3` |
| `deepseek-flash` | `Deepseek-v4-flash` → **`amd/DeepSeek-V4.1-Flash`** → `gs/...` |
| `qwen3.8-max` / `qwen3.7-plus` | `Qwen/Qwen3.8-27B` → `Deepseek-v4-flash` → **`amd/Qwen3.8-27B`** |
| `glm-5.3` / `glm-5.3-flash` | `deepseek/deepseek-chat` → **`amd/GLM-5.3-Flash`** |
| `kimi-k3` / `claude-haiku-5-5` | `jw/kimi-k3-1` → **`amd/DeepSeek-V4.1-Flash`** |

**校验**：改后 `残余含 agnes 的链 = 0`；`model_fallbacks` 总条数仍 27。
**备份**：`/tmp/config.json.bak-fb-20261104-043605`（改前完整配置；`/tmp` 会被清，长期留档以本文为准）。
**热加载**：`chain = (load_config().get("model_fallbacks") or {}).get(model)`（`routes/proxy.py:3293`）**每次请求重读**，改完**不需要重启**。

### 6.2 AMD 池实测特性（2026-11-04）

| 特性 | 实测 |
|---|---|
| **并发上限** | **32**；满了返回 `429 Model 'X' is at its concurrency limit (32)`，需重试 |
| **响应波动** | `amd/DeepSeek-V4.1-Flash` 12–68s；`amd/DeepSeek-V4-Flash` 18.8s |
| **偶发坏响应** | `amd/GLM-5.3-Flash` 有一次返回**字面量字符串 `'None'`**（非空、非错误），后两次正常返回 `Hello!` |
| **V4.1 与 V4 独立** | `amd/DeepSeek-V4.1-Flash` → served `self-dploy/DeepSeek-V4.1-Flash`；`amd/DeepSeek-V4-Flash` → served `self-dploy/DeepSeek-V4-Flash`。**两个独立条目，不得互相顶替** |

### 6.3 推理模型的 max_tokens 陷阱（2026-11-04 实测）

`deepseek-ai/DeepSeek-V4.1-Flash-1017` 与 `deepseek/deepseek-v4.1-flash-1017` 都是**推理模型**：先输出 `reasoning_content`，再输出 `content`。

```
max_tokens=8   → finish_reason: length, content='' ，reasoning 占满预算
max_tokens=32  → finish_reason: stop,   content='PONG' ✅
max_tokens=64  → 正常
```

**后果**：客户端传小 `max_tokens` 时会拿到 **HTTP 200 + 空正文**，比报错更难发现。
relay 对空 content 有处理（`EMPTY_COOLDOWN_S = 30`，`routes/proxy.py:2327` 冷却换号），
但**反复触发会把好 key 打入冷却**，表现为"额度还在却一直失败"。

**处置**：客户端/网关侧对推理模型设 `max_tokens` 下限（≥64）；排障时先看 `finish_reason` 是否为 `length`。

- **注水检测**：relay 会对"请求名 ≠ 返回名"打 WARNING（`注水嫌疑（模型名不符）`）。实测：
  - `bytedance/doubao-seed-2.1-pro` → 返回 `doubao-seed-2-1-pro-260915`（同族全名，良性）
  - `lyrouter/auto` → 返回 `glm-5.2`（**平台自选模型，不可预测，别当确定型号用**）
- **冷却**：provider 失败后进入冷却，冷却期内该 provider 被 `match()` 跳过；日志里的"不是可用外部通道"常常就是冷却中的表现。
  - 常量：`EMPTY_COOLDOWN_S = 30`（空响应）、`ERROR_COOLDOWN_S = 60`（5xx）、`QUOTA_COOLDOWN_S = 600`（配额耗尽）
  - 冷却状态落盘 `cooldowns.json`（provider 级），`saved_at` 字段可判断快照时间

### 6.4 2026-11-04 停用 provider 后兜底链静默死亡（"停掉了还总是出错"）

**现象**：用户把云端 `gs` 停掉后，长任务编码手仍然报错：
`API error (status 400): proxy_upstream_error: upstream returned 400 for model 'gs/gemini-3.5-flash'`，
`--debug-file` 抓到更底层的一条：`status_code=503 error="proxy_upstream_error: no usable credential for provider 'gs'"`。

**根因**：`providers.gs.enabled = false` 只停用了供应商本身，**不会**把 `model_fallbacks` 里指向它的跳点摘掉。
当时 27 条链里有 **9 条**仍以 `gs/gemini-3.5-flash` 为其中一跳。链条只有在**前置跳点失败时**才会走到 gs
（amd 并发打满 429、sensenova 配额冷却 503 等），所以：

| 疑点 | 解释 |
|---|---|
| 为什么"换什么模型 slug 都一样" | 问题不在模型，在链；9 条链覆盖了 deepseek/Qwen/GLM/MiniMax/Step 全族 |
| 为什么是间歇的 | 主跳点成功时根本走不到 gs；只有主跳点失败才暴露 |
| 为什么停用了还报错 | 停用 ≠ 移除引用，链走到该跳即"无可用凭据"当场死亡 |
| 为什么整条请求死而不是降级 | 该跳是链尾或链中最后一跳，走完即返回致命错误，客户端 `is_retryable=false` |

**处置**：从 9 条链移除 `gs/gemini-3.5-flash` 这一跳，**未动 `providers.gs` 的禁用状态**（停用是用户有意的）。
`providers.gs.models` 的 14 个条目原样保留（那是 provider 目录，不是兜底链）。

**验证**：
- 结构化审计：全库**被引用但 provider 已禁用的目标 = 0**（12 个禁用 provider 逐一核对）
- 功能实测：`deepseek-flash` 从"gs 凭据致命错误"变为"amd 429 可重试限流"——链条到 amd 即结束，证明 gs 已不在链中
- 服务 `systemctl is-active qidi-relay` = `active`，未重启（mtime 热加载）

**备份**：`/opt/codely-relay/config.json.bak-20261104-171525`（root:root 0600，改前完整配置）。
**回滚**：`sudo cp /opt/codely-relay/config.json.bak-20261104-171525 /opt/codely-relay/config.json`（热加载生效）。

**教训（写入纪律）**：
> **停用任何 provider 后，必须同步审计 `model_fallbacks` 的引用。**
> 禁用是"关闭一扇门"，兜底链是"写在纸上的路线"——门关了，路线还在，走到尽头就是静默死亡。
> 每次 `enabled=false` 都应当作一次配置变更的**一半**，另一半是清引用。

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
5. ~~**dimleap 填 key 后**：先验请求名形态~~ → **已完成**：已启用（18 个模型 + 12 个别名），别名形态经实测可用。
6. **`amd` 上游 500/502**：等厂商修复；模型与别名已登记（15+15），恢复即用。`kimi-ai` 只有 2 个模型能真正调通（其余 403 套餐档位），派活前先探活。

---

## 九、变更记录

| 日期 | 变更 |
|---|---|
| 2026-10-17 | 账号池分级与 `intern` 月额度处理（见 `luyou-account-pools.md`） |
| 2026-10-22 | 成本口径 P-06；52duck 线路改名（`Deepseek-v4-flash` → `Deepseek-flash`），**兜底链未同步** |
| 2026-10-28 | 媒体默认拒绝闸门 `media_enabled`（¥15 事故后） |
| 2026-10-30 | 确认 apigoto 每日推广收益、余额按天波动；小写 `deepseek-flash` 实际由 apigoto 服务 |
| 2026-10-31 | 接入 `dimleap`（43 个 chat：31 真名 + 12 独占别名，含 `dimleap/glm-5.3`、`glm-5.3-flash`）；`enabled=false` 等 key；24 个媒体模型未登记（闸门） |
| 2026-11-01 | **修复"空 models 通道不可见"**（`providers.py:440` 去掉 `and models`，备份 `providers.py.bak-fixA-20261031`）；接入 `kimi-ai`（Kimi Code 线）与 `amd`（15 模型 + `amd/*` 别名）；`dimleap` 启用于用户裁剪后的 18 模型；provider 总数 19 → 21 |
| **2026-11-04** | **兜底链重构**：`agnes-3.0-flash` 从全部 **12 条** `model_fallbacks` 移除（用户裁定：能力不足会造成信息污染），替换为 amd 同族真模型（DeepSeek→`amd/DeepSeek-V4.1-Flash`、Qwen→`amd/Qwen3.8-27B`、GLM→`amd/GLM-5.3-Flash`）。备份 `/tmp/config.json.bak-fb-20261104-043605`。同时记录 AMD 池三项特性（并发上限 32 / 偶发坏响应 / V4.1 与 V4 不得互相顶替）与推理模型 `max_tokens` 陷阱（见 §6.1–6.3） |
| **2026-11-04（晚）** | **清除停用 provider 的兜底链残留引用**：用户停用 `gs` 后仍报 `no usable credential for provider 'gs'`——根因是 9 条链仍以 `gs/gemini-3.5-flash` 为跳点（禁用不停引用）。从 9 条链移除该跳，`providers.gs` 禁用状态与 14 模型目录未动；失效引用 12 个禁用 provider 逐一核对后归零。备份 `config.json.bak-20261104-171525`，mtime 热加载未重启。新增「兜底链完整性自检」命令（§十）与 §6.4 事故记录 |

---

## 十、快速命令

```bash
# 结构自检
sudo python3 -c "import json;c=json.load(open('/opt/codely-relay/config.json'));\
print(len(c['providers']),'providers');[print(k,len(v.get('models',[])),len(v.get('model_aliases',{})),v.get('enabled',True)) for k,v in c['providers'].items()]"

# 兜底链完整性自检（停用 provider 后必跑；输出"失效引用 0"才算干净）
sudo python3 -c "
import json,re
c=json.load(open('/opt/codely-relay/config.json'))
off={n for n,p in c['providers'].items() if p.get('enabled') is False}
bad=[t for k,v in c['model_fallbacks'].items() for t in (v if isinstance(v,list) else [v])
     if re.match(r'^([^/]+)/',str(t)) and re.match(r'^([^/]+)/',str(t)).group(1) in off]
print('禁用 providers:',sorted(off)); print('失效引用:',bad or 0)"

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

---

## 十一、模型清单同步（本地 config.toml ← 中转）

**问题**：中转的 `providers[*].models` 随上游更新，而 qidicode 的 `~/.qidi/config.toml` 是手写清单。两份清单互不知道对方，漂移出两类故障：

- 中转已下线、本地还留着 → 选中即 `404 model_not_found`
- 中转新增、本地没有 → 明明能用却选不到

2026-11-01 实测：本地 134 条指向中转的条目中 **23 条已失效**（dimleap 15、openai-next 3、无组 4、codely 1）；`intern` 组另有 5 条失效、4 个新模型缺席——即当时用户遇到的现象。

**工具**：`tools/model-sync/sync_relay_models.py`（Python 3.11+ 标准库，无第三方依赖）

| 动作 | 作用 | 默认行为 |
|---|---|---|
| `check` | 拉中转 `/v1/models`，逐条比对本地 `model` 字段，列出失效条目与受影响 group | 只读 |
| `add --provider X [--from-ssh HOST] [--group G]` | 把 provider 的新模型补进 config.toml 托管块（可重复执行、自动去重） | dry-run |
| `prune --group G` | 把失效条目注释掉（注释保留、随时可恢复） | dry-run |

```powershell
python tools/model-sync/sync_relay_models.py check
python tools/model-sync/sync_relay_models.py add --provider intern --from-ssh ubuntu@111.231.166.204 --group intern --apply
python tools/model-sync/sync_relay_models.py prune --group intern --apply
```

**纪律**：

1. 写盘前必须 `tomllib` 自检 + 备份（工具内建；校验不过就不写）。
2. "中转登记了"**不等于**能用：新条目必须实发一次 `POST /v1/chat/completions` 才算同步完成（历史上多次出现登记齐全但 401/404/空回复）。
3. 新增条目要**重启 qidicode** 才进模型注册表（启动时缓存）。
4. 只做"中转 → 本地"单向同步；中转配置本身仍按 §四 的流程人工审慎变更。
5. 推理类模型（如 `Agents-A1`）`max_tokens` 给小了会全部消耗在 `reasoning_content` 上、`content` 为空——验收时给到 64 以上。
6. 上游 `/v1/models` 的清单**不等于**该 key 的授权清单（unisound 列 29 个、实际只有 1 个有权限；jiuwei 列 5 个、实际全 403）——append 前先实发一次 POST 验证。

**残留待清（2026-11-01）**：`openai-next` 3（该 provider 已从云端删除）、无组 4（`GLM-5.3` / `GLM-5.3-FLASH` / `KIMI-K3` / `step-5-preview` 旧名）、`codely` 1（`glm-5.3-flashx`）。（`dimleap` 15 属用户主动裁剪口径，保留注释不清；`intern` 5 条已随 2026-11-01 同步恢复。）

---

## 十二、中转侧模型清单自同步（model_sync）

**位置**：`/opt/codely-relay/model_sync_linux.py`（仓内版本 `tools/relay-model-sync/model_sync_linux.py`，改完要 scp 上去再生效）

**职责边界**：只做"上游 → 中转 config.json"。本地 qidicode 的 `config.toml` 不归它管（用第十一节的 `sync_relay_models.py`）。

**规则**：
- **append-only**：上游新增 → 自动并入 `provider.models`；上游消失 → 只写日志报告，绝不删（一次 401 就会误删一整个 provider 的模型，代价太大）。
- 单 provider 单次上限 40 个，`CHAT_HINT` 过滤杂项，名字含 `:`（`:free`/`:batch`/`:fast`）的一律不加。
- **同名冲突规避**：候选名若已被**任何其他** provider（不论排序前后，含 disabled）占住，一律跳过并记账。只看前序会出问题——给靠前的 provider 追加一个靠后 provider 已在服务的同名模型，等于静默改写流量归属（审计 D3）。
- 写盘走同目录 `mkstemp` + **写前 `chmod 0600` + `chown root:root`** + fsync + `os.replace`，再配 `flock` 防 cron 与手动执行重叠；每日一份备份（`config.json.bak-modelsync-<日期>`）。（2026-11-01 k3.1 审计 D4：早期版本用 `open()` 建 tmp，umask 会把 config.json 变成 644，已修。）

**策略表**（在脚本 `POLICY` 里维护）：

| 策略 | provider |
|---|---|
| `append` | intern、kimi-ai、modelscope、antling、openscale、sensenova |
| `report` | 52duck、apigoto、lyrouter、agnes、openrouter、jiuwei |
| `skip` | dimleap（用户裁剪过）、amd（上游故障）、gs（无首尔出口）、unisound（授权面窄）、本地端点类 |

**已知局限（审计 D8）**：append 只依据上游 `/v1/models`，不代表该 key 真有权限。`kimi-ai` 列为 append 但其 6 个模型实测只有 2 个可用，所以**新出现的条目仍需人工实发一次 POST 才算可用**；授权语义不明的通道请留在 `report`。

**触发**：
- 每日 cron（`/etc/cron.d/qidi-model-sync`，03:30 低峰）：`/usr/bin/python3 /opt/codely-relay/model_sync_linux.py`，日志 `/opt/codely-relay/model_sync.log`。
- 手动：`sudo python3 /opt/codely-relay/model_sync_linux.py [--dry-run]`。

**闭环**：cron 写入后 `_load_providers()` 的 mtime 热加载立即生效（无需重启服务）；本地侧先跑 `tools/model-sync/sync_relay_models.py check` 看失效面，再用 `add --provider intern --from-ssh ubuntu@111.231.166.204 --group intern`（dry-run 看增量）→ 加 `--apply`。`add` 必须给 `--models` 或 `--from-ssh` 之一。

---

## 十三、审计记录（2026-11-01，k3.1 独立审计）

审计对象：`a6d3889` 的模型清单同步实现与文档主张。结论：**初版不通过**，13 条缺陷，已修 7 条高/中级。

| 编号 | 严重度 | 问题 | 处置 |
|---|---|---|---|
| D1 | 高 | `prune` 用 marker 顶替表头，`restore` 却假定表头存在 → 还原时 key/value 漂进上一张表，`tomllib` 报 `Cannot overwrite a value`（本机已实测复现） | 已修：prune 保留 `# [model."X"]`；restore 校验表头、遇到下一个 marker 即停；旧格式块拒绝自动恢复。回归测试 `tools/model-sync/test_sync_relay_models.py` 覆盖往返等价 |
| D2 | 高 | 重复 `add` 会整块重写托管块，只写本轮增量 → 之前加过的模型消失 | 已修：重写前先收集块内已有 model 名做并集；测试覆盖"第二轮 A 不丢" |
| D3 | 高 | `claimed_by` 只查前序 provider，给前序 provider 追加后序已在服务的同名模型会改道 | 已修：扫描任何其他 provider（含 disabled），一律跳过 |
| D4 | 高 | `open()` 建 tmp 受 umask 影响，把 config.json 从 600 变 644（已实测发生） | 已修：`mkstemp` + 写前 `chmod 0600` + `chown root` + fsync + replace |
| D5 | 中 | 中转返回空目录时会把本地条目批量注释掉 | 已修：目录 < 50 个模型即 fail-closed 中止 |
| D10 | 中 | 固定 tmp 名、无锁，cron 与手动执行可互相截断 | 已修：`flock` + `mkstemp` 唯一名 |
| D11 | 中 | 本地三种写盘为直接 truncate，进程崩溃可留半截 config | 已修：同为 tmp + fsync + 保留 mode + replace |
| D6/D7 | 中 | TOML 表头变体/多行字符串会误判；restore 无表头时静默错位 | 部分修（表头校验 + marker 边界 + `tomllib` 兜底）；表头写法变体仍未全覆盖，遇到非标准写法会因校验失败而不写盘（fail-safe） |
| D8 | 中 | append 只依据上游清单，不验证该 key 是否真有权限 | 未修（成本高）：降级为文档警示 + 关键通道留 `report` |
| D9 | 中 | 新条目 ctx 默认取同组样本（曾为 1M），可能高估 | 已改为显式 `--ctx`，缺省 128K 并提示 |
| D12 | 低 | provider 故障时退出码仍为 0 | 未修：已写日志与 `report["errors"]`，后续接监控再加非零退出 |
| D13 | 低 | 文档闭环命令缺必填参数 | 已修 |
