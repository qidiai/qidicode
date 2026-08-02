# Examples: media-gen-agent

## Image System (Agnes AI)

```powershell
# Text to Image
.\image_system.ps1 -Mode text2img -Prompt "a red dot on white background" -ApiKey "sk-..."

# Image to Image
.\image_system.ps1 -Mode img2img -Image "C:\photos\cat.jpg" -Prompt "oil painting style" -ApiKey "sk-..."
```

## Video System (Agnes AI)

```powershell
# Text to Video
.\video_system\video_system.ps1 -Mode text2video -Prompt "a cat on the beach at sunset" -Backend agnes -ApiKey "sk-..."

# Image to Video
.\video_system\video_system.ps1 -Mode img2video -Image "C:\photos\landscape.jpg" -Prompt "slow camera push-in" -Duration 10 -Resolution 720p -Backend agnes -ApiKey "sk-..."

# Reference Images to Video
.\video_system\video_system.ps1 -Mode ref2video -Images @("C:\photos\scene1.jpg","C:\photos\scene2.jpg") -Prompt "cinematic transitions" -Duration 10 -Backend agnes -ApiKey "sk-..."
```

## Notes
- Image model: `agnes-image-2.1-flash`
- Video model: `agnes-video-v2.0`
- Set `AGNES_API_KEY` environment variable to avoid passing `-ApiKey` every time.
