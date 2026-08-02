# Media Gen Agent

Multi-modal generation skill: 文生图、图生图、文生视频、图生视频。

## 触发条件

用户提到以下任一需求时启用本 skill：
- 文生图 / text-to-image / 生成图片 / 画一张 / 生成图像
- 图生图 / image-to-image / 基于图片生成 / 图片变体
- 文生视频 / text-to-video / 生成视频 / 让画面动起来
- 图生视频 / image-to-video / 把图片转视频 / 让照片动起来

## 工具映射

| 需求 | 主入口 | 脚本 | 模型 |
|------|--------|------|------|
| 文生图 | `media_system.ps1` | `-Mode text2img` | `agnes-image-2.1-flash` |
| 图生图 | `media_system.ps1` | `-Mode img2img` | `agnes-image-2.1-flash` |
| 图生视频 | `media_system.ps1` | `-Mode img2video` | `agnes-video-v2.0` |
| 多图生视频 | `media_system.ps1` | `-Mode ref2video` | `agnes-video-v2.0` |
| 文生视频 | `media_system.ps1` | `-Mode text2video` | `agnes-video-v2.0` |
| qidi_build 原生 | `qidi_build:ImageToVideo` / `reference_to_video` | 原生工具 | xAI Imagine Video |

## 执行流程

### 1. 文生图
1. 调用 `media_system.ps1 -Mode text2img`，使用 `agnes-image-2.1-flash`。
2. 传入 `-Prompt`（详细描述期望画面）。
3. 可选：追加 `-AspectRatio`（默认 `auto`）。
4. 返回生成的图片路径，向用户确认结果。

### 2. 图生图
1. 提供源图片（本地路径）。
2. 调用 `media_system.ps1 -Mode img2img -Image <path>`，使用 `agnes-image-2.1-flash`。
3. 传入 `-Prompt`（变换描述）。
4. 可选：追加 `-AspectRatio`。
5. 返回编辑后的图片路径。

### 3. 图生视频
1. 提供源图片（本地路径）。
2. 调用 `media_system.ps1 -Mode img2video`，使用 `agnes-video-v2.0`。
3. 传入 `-Image`、`-Prompt`、`-Duration`、`-Resolution`。
4. 返回生成的视频路径。

### 4. 多图生视频
1. 提供 2-7 张参考图片。
2. 调用 `media_system.ps1 -Mode ref2video`，使用 `agnes-video-v2.0`。
3. 传入 `-Images`、`-Prompt`、`-Duration`、`-Resolution`。
4. 返回生成的视频路径。

### 5. 文生视频
1. 调用 `media_system.ps1 -Mode text2video`，使用 `agnes-video-v2.0`。
2. 传入 `-Prompt`、`-Duration`、`-Resolution`。
3. 返回生成的视频路径。

### 6. qidi_build 原生（备选）
如果配置了 `XAI_API_KEY`，也可走 qidi_build 原生工具：
- `qidi_build:ImageToVideo`
- `reference_to_video`
注意：原生工具依赖 xAI Imagine Video API，可能受 tier 限制。

## 参数速查

### image_gen
- `prompt` (string, 必填): 画面描述
- `aspect_ratio` (string, 可选): `auto`, `1:1`, `16:9`, `9:16`, `3:2`, `2:3`, `2:1`, `1:2`, `19.5:9`, `9:19.5`, `20:9`, `9:20`

### image_edit
- `image` (string, 必填): 源图片路径
- `prompt` (string, 必填): 编辑描述
- `aspect_ratio` (string, 可选): 同 image_gen

### qidi_build:ImageToVideo
- `image` (string, 必填): 源图片路径（绝对路径）
- `prompt` (string, 可选): 动画描述
- `duration` (int, 可选): 6 或 10，默认 6
- `resolution_name` (string, 可选): `480p` 或 `720p`，默认 `480p`

### reference_to_video
- `images` (array, 必填): 2-7 张图片路径
- `prompt` (string, 必填): 视频描述
- `aspect_ratio` (string, 可选): 同 image_gen
- `duration` (int, 可选): 6 或 10，默认 6
- `resolution_name` (string, 可选): `480p` 或 `720p`，默认 `480p`

## 辅助脚本

见 `scripts/generate.ps1`，可用 PowerShell 快速调用各模式。

```powershell
# 文生图
.\scripts\generate.ps1 -Mode text2img -Prompt "一只穿宇航服的猫站在月球上"

# 图生图
.\scripts\generate.ps1 -Mode img2img -Image "C:\path\to\photo.jpg" -Prompt "油画风格"

# 图生视频
.\scripts\generate.ps1 -Mode img2video -Image "C:\path\to\photo.jpg" -Prompt "镜头缓慢推进" -Duration 10 -Resolution 720p

# 多图生视频
.\scripts\generate.ps1 -Mode ref2video -Images @("a.jpg","b.jpg","c.jpg") -Prompt "电影感转场" -Duration 10

# 文生视频（两步）
.\scripts\generate.ps1 -Mode text2video -Prompt "赛博朋克城市夜景，雨夜" -Duration 10
```

## 多 Key 配置

见 `scripts/media_system.ps1` 与 `agnes_config.json`。

```powershell
# 使用默认 key
.\scripts\media_system.ps1 -Mode text2img -Prompt "a red dot on white background"

# 指定 key
.\scripts\media_system.ps1 -Mode text2video -Prompt "a cat on the beach" -KeyName key2
```

`agnes_config.json` 结构：
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

## 外部视频系统

见 `video_system/video_system.ps1`，支持多后端视频生成：

```powershell
# 自动选择后端（优先 XAI，其次 Agnes AI，最后 OpenAI）
.\video_system\video_system.ps1 -Mode text2video -Prompt "a cat on the beach at sunset"

# 强制使用 Agnes AI（中国站）
.\video_system\video_system.ps1 -Mode text2video -Prompt "a cat on the beach" -Backend agnes -ApiKey "sk-..."

# 强制使用 qidi_build 原生工具
.\video_system\video_system.ps1 -Mode img2video -Image "photo.jpg" -Prompt "animate" -Backend qidi_build
```

### Agnes AI 中国站已打通
- Base URL：`https://api.agnes-ai.cn/v1`
- 图片模型：`agnes-image-2.1-flash`
- 视频模型：`agnes-video-v2.0`
- 轮询地址：`https://api.agnes-ai.cn/agnesapi?video_id=<video_id>`
- 视频下载：轮询返回 `url` 字段，直链为 `.mp4`

### 后端优先级
1. `qidi_build` - 原生 xAI Imagine Video API（需 `XAI_API_KEY`）
2. `agnes` - Agnes AI `agnes-video-v2.0`（需 `AGNES_API_KEY`）
3. `openai` - OpenAI Sora API（需 `OPENAI_API_KEY` 且有 Sora 访问权限）

### 注意事项

- Windows 路径需使用绝对路径或正确转义。
- 视频生成耗时较长，需耐心等待工具返回。
- `image_gen` 与 `image_edit` 的输出路径由工具返回，可直接用于后续 pipeline。
- 如工具调用失败，检查输入路径是否存在、描述是否符合工具约束。
- qidi_build 原生视频工具依赖 xAI API，如未配置 `XAI_API_KEY` 会失败。
- Agnes AI 中国站已实测可用：聊天、图片、视频任务创建/轮询/下载均已打通。
