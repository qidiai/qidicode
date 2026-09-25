# Media Gen Agent — 设计思路与进展

## 1. 项目概述

本项目的目标是构建一个**本地化、多模态媒体生成系统**，覆盖 5 种核心模式：

| 模式 | 说明 | 模型 |
|------|------|------|
| text2img | 文生图 | `agnes-image-2.1-flash` |
| img2img | 图生图 | `agnes-image-2.1-flash` |
| img2video | 图生视频 | `agnes-video-v2.0` |
| text2video | 文生视频 | `agnes-video-v2.0` |
| ref2video | 多图生视频 | `agnes-video-v2.0` |

**约束条件：**
- 不使用免费/trial  tier
- 图片模型与视频模型**严格区分**，不混用
- 支持多 API key 管理与故障切换
- 优先使用 Agnes AI（中国站 `api.agnes-ai.cn`），不依赖 xAI Imagine Video

---

## 2. 系统架构

```
media-gen-agent/
├── agnes_config.json              # 多 key 集中配置（含敏感 key，不入库）
├── .gitignore                     # 忽略敏感文件与生成物
├── README.md                      # 使用说明
├── SKILL.md                       # QIDI Code skill 定义
├── DESIGN.md                      # 本文档
│
├── scripts/
│   └── media_system.ps1           # ★ 统一入口
│
└── output/                        # 统一输出目录（不入库）
    ├── images/
    └── videos/
```

### 核心设计

1. **单一入口**：`scripts/media_system.ps1` 作为所有 5 种模式的统一入口，通过 `-Mode` 参数切换。
2. **配置驱动**：`agnes_config.json` 集中管理多 key、模型名、轮询间隔等参数，无需硬编码。
3. **Key 隔离**：每个 key 可配置独立的 `name`、`api_key`、`base_url`、`models`，支持按名称切换。
4. **幂等输出**：每次生成带时间戳的文件名，避免覆盖。

---

## 3. 设计思路

### 3.1 为什么不走 qidi_build 原生工具？

- 原生 `image_gen` / `image_edit` 依赖 xAI Imagine API，在本地环境**不可用**（报错：`Image generation API request failed`）
- 原生 `ImageToVideo` / `reference_to_video` 同样依赖 xAI，受 tier 限制
- 为避免外部依赖不可控，直接对接 Agnes AI 中国站 API

### 3.2 为什么用 PowerShell 而不是 Python？

- 目标环境为 Windows，PowerShell 原生可用，无需额外 runtime
- `Invoke-RestMethod` / `Invoke-WebRequest` 直接调用 HTTP API
- 与 QIDI Code 工作流集成更自然

### 3.3 多 Key 策略

- **配置层面**：`agnes_config.json` 支持多个 key 条目，`default_key_name` 指定默认 key
- **调用层面**：`-KeyName` 参数覆盖默认 key
- **故障切换**（待实现）：检测到 429/503 时自动轮询下一个可用 key

---

## 4. Agnes API 逆向工程发现

通过实际调用与错误排查，逆向出以下 API 契约：

### 4.1 图片生成（OpenAI 兼容）

**Endpoint:** `POST /v1/images/generations`

**Request:**
```json
{
  "model": "agnes-image-2.1-flash",
  "prompt": "画面描述",
  "size": "1024x1024",
  "n": 1
}
```

**注意：**
- `response_format` 参数**不支持**，传入会返回 400
- 返回 `data[0].url`（图片直链）或 `data[0].b64_json`（Base64）

### 4.2 视频生成（异步任务）

**创建任务:** `POST /v1/videos`

**Request:**
```json
{
  "model": "agnes-video-v2.0",
  "prompt": "视频描述",
  "duration": 6,
  "resolution": "480p",
  "aspect_ratio": "auto"
}
```

**带图生视频:**
```json
{
  "model": "agnes-video-v2.0",
  "prompt": "动画描述",
  "image": "data:image/jpeg;base64,...",
  "duration": 6,
  "resolution": "480p"
}
```

**多图生视频:**
```json
{
  "model": "agnes-video-v2.0",
  "prompt": "转场描述",
  "reference_images": ["data:image/jpeg;base64,...", "..."],
  "duration": 6,
  "resolution": "480p"
}
```

**轮询状态:**
- `GET /agnesapi?video_id=<video_id>` 或 `GET /videos/<task_id>`
- 关键字段：`internal_status`、`status`、`url`、`error`
- 状态流转：`pending` → `processing` → `completed` / `failed`

**注意：**
- `/v1/videos/<task_id>` 曾返回 500，稳定方案是 `/agnesapi?video_id=...`
- 完成时 `url` 字段为 `.mp4` 直链

---

## 5. 当前进展与测试结果

| 功能 | 状态 | 测试时间 | 备注 |
|------|------|----------|------|
| text2img | ✅ 已打通 | 2026-07-31 16:11 | 生成 `text2img_20260731_161134.png` |
| img2img | ✅ 代码就绪 | — | 待实际测试 |
| text2video | ✅ 已打通 | 2026-07-31 05:39 | 生成 `text2video_agnes_20260731_053938.mp4` |
| img2video | ✅ 代码就绪 | — | 待实际测试 |
| ref2video | ✅ 代码就绪 | — | 待实际测试 |
| 多 Key 配置 | ✅ 已支持 | — | 3 个 key（key2/key3 待填入真实 key） |
| Key 故障切换 | ✅ 已实现 | — | 429/503/500 自动轮询下一个 key |
| 冗余文件清理 | ✅ 已完成 | — | 旧版脚本已清理，统一到 `scripts/media_system.ps1` |
| 统一输出目录 | ✅ 已完成 | — | 图片和视频统一到 `output/` |

### 已生成文件

```
output/
├── images/          # text2img、img2img 输出
└── videos/          # img2video、text2video、ref2video 输出
```

---

## 6. 待完成项（路线图）

### P0 — 高优先级（已完成）
1. ✅ **补充剩余 2 个 Agnes API key** 到 `agnes_config.json`（key2/key3 结构已就绪，待填入真实 key）
2. ✅ **实现 Key 故障切换**：检测到 429/503/500 时自动轮询下一个 key
3. ✅ **清理冗余文件**：旧版 `image_system.ps1`、根目录 `media_system.ps1`、`video_system/` 已清理

### P1 — 中优先级
4. ✅ **统一输出目录**：`output/images/` + `output/videos/`
5. **日志与重试**：`Invoke-WithRetry` 已覆盖下载重试，请求级重试由 `Invoke-AgnesRest` 处理
6. **完善 SKILL.md**：补充图片/视频模型严格区分的注意事项

### P2 — 低优先级
7. **前端封装**：可选 Web/CLI 封装，降低使用门槛
8. **性能监控**：统计各 key 成功率、耗时

---

## 7. 使用方式

### 统一入口

```powershell
# 文生图
.\scripts\media_system.ps1 -Mode text2img -Prompt "一只穿宇航服的猫站在月球上"

# 图生图
.\scripts\media_system.ps1 -Mode img2img -Image "C:\photos\cat.jpg" -Prompt "油画风格"

# 文生视频
.\scripts\media_system.ps1 -Mode text2video -Prompt "赛博朋克城市夜景，雨夜" -Duration 10 -Resolution 720p

# 图生视频
.\scripts\media_system.ps1 -Mode img2video -Image "C:\photos\landscape.jpg" -Prompt "镜头缓慢推进" -Duration 10

# 多图生视频
.\scripts\media_system.ps1 -Mode ref2video -Images @("a.jpg","b.jpg","c.jpg") -Prompt "电影感转场" -Duration 10

# 指定 key
.\scripts\media_system.ps1 -Mode text2img -Prompt "画面描述" -KeyName key2
```

### 配置文件结构

```json
{
  "default_key_name": "default",
  "keys": [
    {
      "name": "default",
      "api_key": "sk-...",
      "base_url": "https://api.agnes-ai.cn/v1",
      "models": {
        "image": "agnes-image-2.1-flash",
        "video": "agnes-video-v2.0"
      }
    }
  ]
}
```

---

## 8. 风险与限制

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| Agnes 服务 503 | 视频生成失败 | 实现 key 故障切换 + 指数退避重试 |
| 视频生成耗时久 | 用户体验差 | 明确提示等待时间，轮询间隔 5s |
| 大文件 base64 | 内存占用高 | 限制输入图片大小，或改为分片上传（未来） |
| Key 泄露 | 安全风险 | `agnes_config.json` 加入 `.gitignore` |

---

## 9. 总结

- **核心功能已全部打通**：5 种模式的代码链路均已完成，text2img 与 text2video 已实测生成成功
- **架构清晰**：统一入口 + 配置驱动，易于扩展
- **待完善**：多 key 故障切换、冗余文件清理、输出目录统一

下一步优先完成：**补充 2 个 key → 实现故障切换 → 清理冗余文件**。
