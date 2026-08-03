# Media Gen Agent

本地化多模态媒体生成系统，统一入口 `scripts/media_system.ps1`，支持 5 种模式：

| 模式 | 说明 |
|------|------|
| `text2img` | 文生图 |
| `img2img` | 图生图 |
| `text2video` | 文生视频 |
| `img2video` | 图生视频 |
| `ref2video` | 多图生视频 |

后端对接 **Agnes AI 中国站**（`api.agnes-ai.cn`），图片模型与视频模型严格区分。

## 前置条件

- Windows 10/11 + PowerShell 5.1+
- 有效 Agnes API key

## 快速开始

```powershell
cd tools/media-gen-agent

# 文生图
.\scripts\media_system.ps1 -Mode text2img -Prompt "一只穿宇航服的猫站在月球上"

# 文生视频
.\scripts\media_system.ps1 -Mode text2video -Prompt "赛博朋克城市夜景，雨夜" -Duration 10 -Resolution 720p
```

## 配置

复制 `agnes_config.json.example` → `agnes_config.json`，填入你的 API key。支持多 key 故障切换（429/503 自动轮询）。

## 输出

所有生成文件统一输出到 `output/images/` 和 `output/videos/`，文件名带时间戳，不会覆盖。

## 架构

详见 [DESIGN.md](DESIGN.md)。
