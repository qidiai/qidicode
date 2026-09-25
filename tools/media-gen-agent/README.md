# Media Gen Agent

本地化多模态媒体生成系统，统一入口 `scripts/media_system.ps1`，支�?5 种模式：

| 模式 | 说明 |
|------|------|
| `text2img` | 文生�?|
| `img2img` | 图生�?|
| `text2video` | 文生视频 |
| `img2video` | 图生视频 |
| `ref2video` | 多图生视�?|

后端对接 **Agnes AI 中国�?*（`api.agnes-ai.cn`），图片模型与视频模型严格区分�?
## 前置条件

- Windows 10/11 + PowerShell 5.1+
- 有效 Agnes API key

## 快速开�?
```powershell
cd tools/media-gen-agent

# 文生�?.\scripts\media_system.ps1 -Mode text2img -Prompt "一只穿宇航服的猫站在月球上"

# 文生视频
.\scripts\media_system.ps1 -Mode text2video -Prompt "赛博朋克城市夜景，雨�? -Duration 10 -Resolution 720p
```

## DryRun 用法

在不消�?API 额度的情况下验证参数构造与输出路径�?
```powershell
# 文生�?.\scripts\media_system.ps1 -DryRun -Mode text2img -Prompt "一只穿宇航服的猫站在月球上"

# 图生视频
.\scripts\media_system.ps1 -DryRun -Mode img2video -Image "C:\photos\landscape.jpg" -Prompt "镜头缓慢推进" -Duration 10

# 多图生视�?.\scripts\media_system.ps1 -DryRun -Mode ref2video -Images @("a.jpg","b.jpg","c.jpg") -Prompt "电影感转�? -Duration 10
```

DryRun 会打�?endpoint、body 摘要（前 200 字符）和预计输出路径，不发送任�?API 请求�?
## 配置

复制 `agnes_config.json.example` �?`agnes_config.json`，填入你�?API key。支持多 key 故障切换�?29/503 自动轮询）�?
配置文件结构�?
```json
{
  "default_key_name": "default",
  "keys": [
    {
      "name": "default",
      "api_key": "sk-...��ռλ�����滻Ϊ��ʵ key��",
      "base_url": "https://api.agnes-ai.cn/v1",
      "models": {
        "image": "agnes-image-2.1-flash",
        "video": "agnes-video-v2.0"
      }
    }
  ]
}
```

## 输出

所有生成文件统一输出�?`output/images/` �?`output/videos/`，文件名带时间戳，不会覆盖�?
## 日志与监�?
每次生成结束后，系统会自动将记录追加�?`output/generation_log.jsonl`。每条记录的格式如下�?
```json
{"timestamp":"2026-08-03T12:00:00","mode":"text2img","prompt":"...","duration":"N/A","resolution":"1024x1024","key_name":"default","success":true,"elapsed_sec":12.34,"output_path":"output/images/text2img_20260803_120000_1A2B.png"}
```

字段说明�?
| 字段 | 说明 |
|------|------|
| timestamp | ISO 8601 时间�?|
| mode | 生成模式 |
| prompt | 输入的提示词 |
| duration | 视频时长（图片模式为 N/A�?|
| resolution | 分辨�?|
| key_name | 使用�?API key 名称 |
| success | 是否成功 |
| elapsed_sec | 总耗时（秒�?|
| output_path | 生成文件路径 |

每次 API 请求还会输出结构化控制台日志�?
```
[2026-08-03 12:00:00] [default] [POST /images/generations] [200] [1234ms]
```

## 参数说明

| 参数 | 必填 | 说明 | 默认�?|
|------|------|------|--------|
| `-Mode` | �?| 生成模式 | �?|
| `-Prompt` | 部分模式 | 文本描述 | �?|
| `-Image` | img2img, img2video | 输入图片路径 | �?|
| `-Images` | ref2video | 输入图片数组（至�?2 张） | �?|
| `-AspectRatio` | 视频模式 | 画面比例 | auto |
| `-Duration` | 视频模式 | 时长（秒�?| 6 |
| `-Resolution` | 视频模式 | 分辨�?| 480p |
| `-KeyName` | �?| 指定 API key | config 默认�?|
| `-DryRun` | �?| 仅打印请求，不发�?API | 关闭 |

## 故障排查

### 400 Bad Request
通常原因�?- 提示词过长或包含非法字符
- 图片格式不被支持（建议使�?JPG/PNG�?- 参数组合不合法（例如 duration 超出允许范围�?
系统会将 Agnes 返回的错误信息友好化展示�?
### 422 Unprocessable Entity
通常原因�?- 请求体字段缺失或类型错误
- 视频任务参数不合�?
### 429 Too Many Requests / 503 Service Unavailable
系统会自动切换到下一个可�?key 并重试。如果所�?key 均失败，脚本会报错退出�?
### 视频生成超时
默认最多轮�?60 次（�?5 分钟，按默认 5 秒间隔）。超时后脚本会退出。如需调整，可�?`agnes_config.json` 中修�?`video_poll_interval_sec`�?
### 图片 MIME 类型检�?`img2img` �?`ref2video` 会自动根据输入文件扩展名选择 MIME 类型。支持的格式：`.png`、`.jpg`、`.jpeg`、`.webp`、`.gif`、`.bmp`。其他格式默认使�?`image/jpeg`�?
## 架构

详见 [DESIGN.md](DESIGN.md)�?
## 已知限制

- `img2img` 使用 `/images/generations` 端点传�?data URI，这�?OpenAI 标准�?`/images/edits` 不同。如�?Agnes 实际期望 multipart form-data，该模式可能需要调整�?- `img2img` �?`-AspectRatio` 参数被忽略，图片统一输出 1024x1024（Agnes 图片 API 限制）�?- 输入图片未做大小限制，过大的图片会导�?base64 内存占用较高�?

