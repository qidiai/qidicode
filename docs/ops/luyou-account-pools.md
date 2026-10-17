# luyou 账号池分级模型（运维知识）

> 2026-10-17 深夜。服务端日志实锤 + 用户澄清。**AGENTS.md 处置说明**：因 AGENTS.md 多会话并发编辑竞态（两度写入被覆盖），本文暂置 `docs/ops/`——请极简模式会话接手将下述机制写入 AGENTS.md §9/§11（单会话串行写无冲突）。

## 三级账号池 + 动态回退机制

```
luyou.qidiai.ltd（云端中继 = codely-relay-server）
├─ intern key 池（ocr-4 等内部 key）
│    └─ 配额型 429："quota exceeded" 自动换下一 key（与 K3 无关的池）
├─ codely 上游（codely-core/codely-flash…）
│    ├─ free 账号组 → 只能调 GLM 系（codely-core = GLM/Z.ai 实锤）
│    │                K3 无权限（free 全冷却 → 回退 pro 是 GLM 的回退，与 K3 无关）
│    └─ pro/订阅组（'39640','58863'）→ 全模型可调 —— K3 的唯一供给
└─ 诊断口诀
     K3/kimi 系 429 = pro 组配额问题（等窗口/续订；step-5 订阅 10-20 过期，过期前配额紧张即持续 429）
     GLM 系 429 = free 组冷却（等回退或用 pro）
     DeepSeek 系 = 独立上游（deepseek-v4-pro 恒稳；deepseek-flash-official = 官方直连独立 key 1M，实测 200）
```

## 实测矩阵（2026-10-17）

| 模型名 | 状态 | 根因 |
|---|---|---|
| KIMI-K3（大写） | 429 | pro 组 K3 配额耗尽 |
| kimi-k3（小写） | 429 | 同上（三名同池） |
| onx-kimi-k3 | 401 | 同池不同上游账号形态 |
| GLM-5.3 | 429→挂 | free 组冷却回退 pro 后仍限流 |
| deepseek-v4-pro | ✅ 恒稳 | 独立 deepseek 上游（非 codely 池） |
| deepseek-flash / deepseek-flash-official | ✅ 200 | 官方直连独立 key（20 号后保底位） |

## codely 家族双后端实锤

- codely-core 后端 = **GLM（Z.ai）**（自报"基于 Z.ai 训练的 GLM 系列"）
- codely-flash 后端 = **DeepSeek**（reasoning 泄露"DeepSeek 最新版模型"）
- codely 家族 5 名（basic/core/flash/air/vl）不含 deepseek 位；**deepseek-flash 是 codely 平台 DeepSeek 位的独立名**（用户确认：codely 里的 deepseek 以 deepseek-flash 之名提供）

## 证据

- 服务端日志 2026-10-17 22:56：`[intern] key ocr-4 流式连接返回 429 换下一个 key: quota exceeded`；`分组回退: codely-core 的 free 账号已全部冷却/禁用，回退 pro 组 ['39640','58863']`
- 用户澄清：free 组用不了 K3，只有订阅组 pro 才可用 K3（**权限分级非配额分级**）
- 三代实测：KIMI-K3 429 / kimi-k3 429 / onx-kimi-k3 401 全封锁（pro 配额耗尽）


## ???2026-10-17 ????intern ???????????

- intern key ??ocr-4 ????????????????free ??????? GLM?+ pro/?????????K3 ?????
- luyou ??????????GLM ?? free ????K3 ? pro ?????DeepSeek ?????????
- ??????????????/???????pro ??/DeepSeek ???????
