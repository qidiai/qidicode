# Video System Examples

## img2video
```powershell
.\video_system.ps1 -Mode img2video -Image "C:\photos\landscape.jpg" -Prompt "slow camera push-in, cinematic, golden hour" -Duration 10 -Resolution 720p
```

## text2video
```powershell
.\video_system.ps1 -Mode text2video -Prompt "cyberpunk city night, rain, neon lights reflecting on wet streets" -Duration 10 -Resolution 720p
```

## ref2video
```powershell
.\video_system.ps1 -Mode ref2video -Images @("C:\photos\scene1.jpg","C:\photos\scene2.jpg","C:\photos\scene3.jpg") -Prompt "cinematic transitions, day to night" -Duration 10 -Resolution 720p
```

## Backend Selection
```powershell
# Force Agnes AI backend
.\video_system.ps1 -Mode text2video -Prompt "a cat on the beach" -Backend agnes

# Force qidi_build backend
.\video_system.ps1 -Mode img2video -Image "photo.jpg" -Prompt "animate" -Backend qidi_build
```
