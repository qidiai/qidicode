# Video System

Unified video generation system supporting multiple backends:
- **qidi_build native tools**: `qidi_build:ImageToVideo`, `reference_to_video` (xAI backend)
- **Agnes AI**: `agnes-video-v2.0` via OpenAI-compatible API
- **OpenAI**: `sora-*` models if available

## Quick Start

```powershell
# Generate video from image
.\video_system.ps1 -Mode img2video -Image "C:\photos\scene.jpg" -Prompt "slow camera push-in, cinematic"

# Generate video from text (via image first)
.\video_system.ps1 -Mode text2video -Prompt "cyberpunk city night, rain, neon lights" -Duration 10

# Generate video from multiple reference images
.\video_system.ps1 -Mode ref2video -Images @("a.jpg","b.jpg","c.jpg") -Prompt "cinematic transitions" -Duration 10
```

## Backend Configuration

Set environment variables to enable backends:
- `XAI_API_KEY` - enables qidi_build native video tools
- `AGNES_API_KEY` - enables Agnes AI video backend
- `OPENAI_API_KEY` - enables OpenAI video backend (if Sora access available)

## Architecture

```
video_system/
  video_system.ps1      # Main entry point
  backends/
    qidi_build.ps1      # Wraps qidi_build:ImageToVideo / reference_to_video
    agnes.ps1           # Calls agnes-video-v2.0 API
    openai.ps1          # Calls Sora API if available
  examples/
    demo.ps1            # Demo script
```
