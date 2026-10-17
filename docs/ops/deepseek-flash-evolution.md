# DeepSeek Flash 系列官方演进史 + API 名策略（运维知识）

> 2026-10-17。来源：https://api-docs.deepseek.com/zh-cn/updates/（官方更新日志全文抓取）——**deepseek-v4.1-flash 系 401 根源的官方文档级实锤**。

## 官方演进史（Flash 系）

| 时间 | 版本 | API 名状态 |
|---|---|---|
| 2026-04-24 | DeepSeek-V4（Pro+Flash）发布 | deepseek-v4-flash 上线；**deepseek-chat/reasoner 旧名三个月后（07-24）停用**（过渡期临时路由到 v4-flash） |
| 2026-07-31 | V4-Flash 正式版（0731） | deepseek-v4-flash（滚动更新） |
| 2026-08-21 | V4-Flash-Vision-Exp | deepseek-v4-flash-vision-exp |
| 2026-08-13 | V4-Pro 正式版 + **峰谷定价**（闲时半价，08-17 生效） | deepseek-v4-pro |
| **2026-09-10** | **V4.1-Flash 发布（新结构系列最小尺寸，原生多模态）** | **`deepseek-flash` 上线（官方恒定 API 名）**；旧名 V4 Flash/Vision Exp 下线；`deepseek-v4-flash`/`deepseek-v4-flash-vision-exp` **暂时路由**（兼容期） |
| **2026-10-17** | **V4.1-Flash-1017 更新（帕累托改进：更快/输出更短/更低成本；针对 DeepSeek Harness/腾讯 CodeBuddy/WorkBuddy 框架优化）** | **仍用 `deepseek-flash`**（滚动指针自动指向新版） |

## API 名策略（官方口径）

- **`deepseek-flash` = 官方承诺的恒定 API 名**（模型名稳定，版本代号滚动——API 名是滚动指针）
- **版本代号名（v4.1-flash/260910 等）不在官方承诺内**——过渡路由期结束即撤下——**引用版本代号名 = 401 的必然**（我们 AGENTS 主力条目连续 401 的根源，已勘误）
- **先例**：2026-04 的 deepseek-chat/reasoner→deepseek-v4-flash 改名（旧名三个月停用）——同款操作

## 对 qidiwork/qidicode 的策略结论

1. **编码手锚定 `deepseek-flash`（官方 API 名）**——滚动更新自动跟进（今天已吃到 1017 帕累托版）
2. **deepseek-flash-official 直连（api.deepseek.com）= apigoto 20 号过期的最优保底主力**：官方名恒定+1M+1017 新版+reasoning_efforts 支持
3. **原生多模态**（V4.1-Flash 系列）：**ling 下线后的多模态替代位**（视觉理解能力内置）
4. **峰谷定价**：批量任务排闲时（成本减半）——与载荷单价 tap 方向同频
5. config 侧条目更新：deepseek-v4-1-flash-260910-222 条目 model 已切 `deepseek/deepseek-v4.1-flash`（前缀形态）；**建议主力条目再切 `deepseek-flash`（官方名，恒定）**
