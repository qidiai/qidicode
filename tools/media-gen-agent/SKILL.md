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
| 文生图 | `scripts/media_system.ps1` | `-Mode text2img` | `agnes-image-2.1-flash` |
| 图生图 | `scripts/media_system.ps1` | `-Mode img2img` | `agnes-image-2.1-flash` |
| 图生视频 | `scripts/media_system.ps1` | `-Mode img2video` | `agnes-video-v2.0` |
| 多图生视频 | `scripts/media_system.ps1` | `-Mode ref2video` | `agnes-video-v2.0` |
| 文生视频 | `scripts/media_system.ps1` | `-Mode text2video` | `agnes-video-v2.0` |
| qidi_build 原生 | `qidi_build:ImageToVideo` / `reference_to_video` | 原生工具 | xAI Imagine Video |

## 执行流程

### 1. 文生图
1. 调用 `scripts/media_system.ps1 -Mode text2img`，使用 `agnes-image-2.1-flash`。
2. 传入 `-Prompt`（详细描述期望画面）。
3. 可选：追加 `-AspectRatio`（默认 `auto`）。
4. 返回生成的图片路径，向用户确认结果。

### 2. 图生图
1. 提供源图片（本地路径）。
2. 调用 `scripts/media_system.ps1 -Mode img2img -Image <path>`，使用 `agnes-image-2.1-flash`。
3. 传入 `-Prompt`（变换描述）。
4. 可选：追加 `-AspectRatio`。
5. 返回编辑后的图片路径。

### 3. 图生视频
1. 提供源图片（本地路径）。
2. 调用 `scripts/media_system.ps1 -Mode img2video`，使用 `agnes-video-v2.0`。
3. 传入 `-Image`、`-Prompt`、`-Duration`、`-Resolution`。
4. 返回生成的视频路径。

### 4. 多图生视频
1. 提供 2-7 张参考图片。
2. 调用 `scripts/media_system.ps1 -Mode ref2video`，使用 `agnes-video-v2.0`。
3. 传入 `-Images`、`-Prompt`、`-Duration`、`-Resolution`。
4. 返回生成的视频路径。

### 5. 文生视频
1. 调用 `scripts/media_system.ps1 -Mode text2video`，使用 `agnes-video-v2.0`。
2. 传入 `-Prompt`、`-Duration`、`-Resolution`。
3. 返回生成的视频路径。

### 6. qidi_build 原生（备选）
如果配置了 `XAI_API_KEY`，也可走 qidi_build 原生工具：
- `qidi_build:ImageToVideo`
- `reference_to_video`
注意：原生工具依赖 xAI Imagine Video API，可能受 tier 限制。

## 参数速查

### media_system.ps1
- `Mode` (必填): `text2img` / `img2img` / `img2video` / `text2video` / `ref2video`
- `Prompt` (string): 画面/视频描述
- `Image` (string): 源图片路径（img2img / img2video）
- `Images` (string[]): 参考图片路径（ref2video，至少 2 张）
- `AspectRatio` (string): `auto`, `1:1`, `16:9`, `9:16`, `3:2`, `2:3`, `2:1`, `1:2`, `19.5:9`, `9:19.5`, `20:9`, `9:20`
- `Duration` (int): 6 或 10，默认 6
- `Resolution` (string): `480p` 或 `720p`，默认 `480p`
- `KeyName` (string): 指定 agnes_config.json 中的 key 名称

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

## 故障切换

当 API 返回 429 / 503 / 500 时，`scripts/media_system.ps1` 会自动轮询 `agnes_config.json` 中的下一个可用 key。已配置的 key 数量越多，切换成功率越高。

## 注意事项

- 图片模型与视频模型严格区分，不混用。
- Windows 路径需使用绝对路径或正确转义。
- 视频生成为异步任务，脚本会自动轮询状态，请耐心等待。
- 幂等输出：文件名带时间戳和随机后缀，避免覆盖。
- 视频轮询状态字段兼容 `internal_status` 与 `status`。
- Agnes AI 中国站已实测可用：聊天、图片、视频任务创建/轮询/下载均已打通。
