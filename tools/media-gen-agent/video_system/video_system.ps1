<#
.SYNOPSIS
Video generation system with multiple backends.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)]
    [ValidateSet("img2video","text2video","ref2video")]
    [string]$Mode,

    [string]$Prompt,
    [string]$Image,
    [string[]]$Images,
    [ValidateSet("auto","1:1","16:9","9:16","3:2","2:3")]
    [string]$AspectRatio = "auto",
    [ValidateSet(6,10)]
    [int]$Duration = 6,
    [ValidateSet("480p","720p")]
    [string]$Resolution = "480p",
    [ValidateSet("qidi_build","agnes","openai","auto")]
    [string]$Backend = "auto",
    [string]$ApiKey
)

function Get-Env([string]$Name) {
    return [Environment]::GetEnvironmentVariable($Name, "Process")
}

function Resolve-Backend {
    param([string]$Preferred, [string]$ProvidedAgnesKey, [string]$ProvidedOpenAiKey, [string]$ProvidedXaiKey)
    if ($Preferred -ne "auto") { return $Preferred }
    if ($ProvidedXaiKey) { return "qidi_build" }
    if ($ProvidedAgnesKey) { return "agnes" }
    if ($ProvidedOpenAiKey) { return "openai" }
    if (Get-Env "XAI_API_KEY") { return "qidi_build" }
    if (Get-Env "AGNES_API_KEY") { return "agnes" }
    if (Get-Env "OPENAI_API_KEY") { return "openai" }
    return "qidi_build"
}

$backend = Resolve-Backend -Preferred $Backend -ProvidedAgnesKey $ApiKey -ProvidedOpenAiKey $ApiKey -ProvidedXaiKey $ApiKey
Write-Host ""
Write-Host "=== Video System ===" -ForegroundColor Cyan
Write-Host "Backend: $backend"
Write-Host "Mode: $Mode"
Write-Host ""

switch ($backend) {
    "qidi_build" {
        Write-Host "[qidi_build] Using native qidi_build video tools" -ForegroundColor Yellow
        switch ($Mode) {
            "img2video" {
                if (-not $Image) { Write-Error "Missing -Image"; exit 1 }
                if (-not (Test-Path $Image)) { Write-Error "Image not found: $Image"; exit 1 }
                $abs = (Resolve-Path $Image).Path
                Write-Host "Please call qidi_build:ImageToVideo tool with:" -ForegroundColor Green
                Write-Host "  image: $abs"
                Write-Host "  prompt: $Prompt"
                Write-Host "  duration: $Duration"
                Write-Host "  resolution_name: $Resolution"
                Write-Host ""
                Write-Host "Invoking qidi_build:ImageToVideo..." -ForegroundColor Green
            }
            "text2video" {
                Write-Host "Please call image_gen first to create a first frame, then pass it to qidi_build:ImageToVideo" -ForegroundColor Green
                Write-Host "  Step 1: image_gen(prompt=`"$Prompt`")"
                Write-Host "  Step 2: qidi_build:ImageToVideo(image=<result>, prompt=`"$Prompt`", duration=$Duration, resolution_name=$Resolution)"
            }
            "ref2video" {
                if (-not $Images -or $Images.Count -lt 2) { Write-Error "Need at least 2 -Images"; exit 1 }
                $abs = @(); foreach ($img in $Images) { if (-not (Test-Path $img)) { Write-Error "Image not found: $img"; exit 1 }; $abs += (Resolve-Path $img).Path }
                Write-Host "Please call qidi_build:reference_to_video tool with:" -ForegroundColor Green
                Write-Host "  images: $($abs -join ', ')"
                Write-Host "  prompt: $Prompt"
                Write-Host "  duration: $Duration"
                Write-Host "  resolution_name: $Resolution"
            }
        }
    }
    "agnes" {
        Write-Host "[Agnes AI] Using agnes-video-v2.0 API" -ForegroundColor Yellow
        $agnesKey = $ApiKey
        if (-not $agnesKey) { $agnesKey = Get-Env "AGNES_API_KEY" }
        if (-not $agnesKey) { Write-Error "AGNES_API_KEY not set"; exit 1 }
        $baseUrl = "https://api.agnes-ai.cn/v1"
        $agnesApi = "https://api.agnes-ai.cn/agnesapi"
        switch ($Mode) {
            "img2video" {
                if (-not $Image) { Write-Error "Missing -Image"; exit 1 }
                if (-not (Test-Path $Image)) { Write-Error "Image not found: $Image"; exit 1 }
                $abs = (Resolve-Path $Image).Path
                $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($abs))
                $mime = "image/jpeg"
                $body = @{
                    model = "agnes-video-v2.0"
                    prompt = $Prompt
                    image = "data:$mime;base64,$b64"
                    duration = $Duration
                    resolution = $Resolution
                } | ConvertTo-Json
                Write-Host "Creating video task..." -ForegroundColor Green
                $resp = Invoke-RestMethod -Uri "$baseUrl/videos" -Method POST -ContentType "application/json" -Body $body -Headers @{ Authorization = "Bearer $agnesKey" }
                $videoId = $resp.video_id
                if (-not $videoId) { $videoId = $resp.id }
                Write-Host "Task created: $videoId" -ForegroundColor Cyan
                Write-Host "Polling for completion..." -ForegroundColor Yellow
                $done = $false; $result = $null
                while (-not $done) {
                    Start-Sleep -Seconds 5
                    $result = Invoke-RestMethod -Uri "$agnesApi`?video_id=$videoId" -Method GET -Headers @{ Authorization = "Bearer $agnesKey" }
                    $status = $result.internal_status
                    if (-not $status) { $status = $result.status }
                    Write-Host "  Status: $status"
                    if ($status -eq "completed") { $done = $true }
                    if ($status -eq "failed") { Write-Error "Video generation failed: $($result.error)"; exit 1 }
                }
                $videoUrl = $result.url
                Write-Host ""
                Write-Host "Video ready!" -ForegroundColor Green
                Write-Host "URL: $videoUrl"
                $outPath = Join-Path $PSScriptRoot "videos\$($Mode)_$(Get-Date -Format 'yyyyMMdd_HHmmss').mp4"
                New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
                Invoke-WebRequest -Uri $videoUrl -OutFile $outPath
                Write-Host "Saved to: $outPath"
            }
            "text2video" {
                $body = @{
                    model = "agnes-video-v2.0"
                    prompt = $Prompt
                    duration = $Duration
                    resolution = $Resolution
                    aspect_ratio = $AspectRatio
                } | ConvertTo-Json
                Write-Host "Creating video task..." -ForegroundColor Green
                $resp = Invoke-RestMethod -Uri "$baseUrl/videos" -Method POST -ContentType "application/json" -Body $body -Headers @{ Authorization = "Bearer $agnesKey" }
                $videoId = $resp.video_id
                if (-not $videoId) { $videoId = $resp.id }
                Write-Host "Task created: $videoId" -ForegroundColor Cyan
                Write-Host "Polling for completion..." -ForegroundColor Yellow
                $done = $false; $result = $null
                while (-not $done) {
                    Start-Sleep -Seconds 5
                    $result = Invoke-RestMethod -Uri "$agnesApi`?video_id=$videoId" -Method GET -Headers @{ Authorization = "Bearer $agnesKey" }
                    $status = $result.internal_status
                    if (-not $status) { $status = $result.status }
                    Write-Host "  Status: $status"
                    if ($status -eq "completed") { $done = $true }
                    if ($status -eq "failed") { Write-Error "Video generation failed: $($result.error)"; exit 1 }
                }
                $videoUrl = $result.url
                Write-Host ""
                Write-Host "Video ready!" -ForegroundColor Green
                Write-Host "URL: $videoUrl"
                $outPath = Join-Path $PSScriptRoot "videos\$($Mode)_$(Get-Date -Format 'yyyyMMdd_HHmmss').mp4"
                New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
                Invoke-WebRequest -Uri $videoUrl -OutFile $outPath
                Write-Host "Saved to: $outPath"
            }
            "ref2video" {
                if (-not $Images -or $Images.Count -lt 2) { Write-Error "Need at least 2 -Images"; exit 1 }
                $abs = @(); foreach ($img in $Images) { if (-not (Test-Path $img)) { Write-Error "Image not found: $img"; exit 1 }; $abs += (Resolve-Path $img).Path }
                $refs = @(); foreach ($path in $abs) { $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($path)); $refs += "data:image/jpeg;base64,$b64" }
                $body = @{
                    model = "agnes-video-v2.0"
                    prompt = $Prompt
                    reference_images = $refs
                    duration = $Duration
                    resolution = $Resolution
                } | ConvertTo-Json
                Write-Host "Creating video task..." -ForegroundColor Green
                $resp = Invoke-RestMethod -Uri "$baseUrl/videos" -Method POST -ContentType "application/json" -Body $body -Headers @{ Authorization = "Bearer $agnesKey" }
                $videoId = $resp.video_id
                if (-not $videoId) { $videoId = $resp.id }
                Write-Host "Task created: $videoId" -ForegroundColor Cyan
                Write-Host "Polling for completion..." -ForegroundColor Yellow
                $done = $false; $result = $null
                while (-not $done) {
                    Start-Sleep -Seconds 5
                    $result = Invoke-RestMethod -Uri "$agnesApi`?video_id=$videoId" -Method GET -Headers @{ Authorization = "Bearer $agnesKey" }
                    $status = $result.internal_status
                    if (-not $status) { $status = $result.status }
                    Write-Host "  Status: $status"
                    if ($status -eq "completed") { $done = $true }
                    if ($status -eq "failed") { Write-Error "Video generation failed: $($result.error)"; exit 1 }
                }
                $videoUrl = $result.url
                Write-Host ""
                Write-Host "Video ready!" -ForegroundColor Green
                Write-Host "URL: $videoUrl"
                $outPath = Join-Path $PSScriptRoot "videos\$($Mode)_$(Get-Date -Format 'yyyyMMdd_HHmmss').mp4"
                New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
                Invoke-WebRequest -Uri $videoUrl -OutFile $outPath
                Write-Host "Saved to: $outPath"
            }
        }
    }
    "openai" {
        Write-Host "[OpenAI] Using OpenAI video API (if available)" -ForegroundColor Yellow
        $openaiKey = $ApiKey
        if (-not $openaiKey) { $openaiKey = Get-Env "OPENAI_API_KEY" }
        if (-not $openaiKey) { Write-Error "OPENAI_API_KEY not set"; exit 1 }
        $baseUrl = "https://api.openai.com/v1"
        switch ($Mode) {
            "img2video" {
                if (-not $Image) { Write-Error "Missing -Image"; exit 1 }
                if (-not (Test-Path $Image)) { Write-Error "Image not found: $Image"; exit 1 }
                $abs = (Resolve-Path $Image).Path
                $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($abs))
                $body = @{
                    model = "sora-1.0"
                    prompt = $Prompt
                    image = "data:image/jpeg;base64,$b64"
                    duration = $Duration
                    resolution = $Resolution
                } | ConvertTo-Json
                Write-Host "Creating video task..." -ForegroundColor Green
                $resp = Invoke-RestMethod -Uri "$baseUrl/videos" -Method POST -ContentType "application/json" -Body $body -Headers @{ Authorization = "Bearer $openaiKey" }
                $videoId = $resp.id
                Write-Host "Task created: $videoId"
                Write-Host "Polling for completion..." -ForegroundColor Yellow
                $done = $false; $result = $null
                while (-not $done) {
                    Start-Sleep -Seconds 5
                    $result = Invoke-RestMethod -Uri "$baseUrl/videos/$videoId" -Method GET -Headers @{ Authorization = "Bearer $openaiKey" }
                    $status = $result.status
                    Write-Host "  Status: $status"
                    if ($status -eq "completed") { $done = $true }
                    if ($status -eq "failed") { Write-Error "Video generation failed: $($result.error)"; exit 1 }
                }
                $videoUrl = $result.url
                Write-Host ""
                Write-Host "Video ready!" -ForegroundColor Green
                Write-Host "URL: $videoUrl"
                $outPath = Join-Path $PSScriptRoot "videos\$($Mode)_$(Get-Date -Format 'yyyyMMdd_HHmmss').mp4"
                New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
                Invoke-WebRequest -Uri $videoUrl -OutFile $outPath
                Write-Host "Saved to: $outPath"
            }
            "text2video" {
                $body = @{
                    model = "sora-1.0"
                    prompt = $Prompt
                    duration = $Duration
                    resolution = $Resolution
                    aspect_ratio = $AspectRatio
                } | ConvertTo-Json
                Write-Host "Creating video task..." -ForegroundColor Green
                $resp = Invoke-RestMethod -Uri "$baseUrl/videos" -Method POST -ContentType "application/json" -Body $body -Headers @{ Authorization = "Bearer $openaiKey" }
                $videoId = $resp.id
                Write-Host "Task created: $videoId"
                Write-Host "Polling for completion..." -ForegroundColor Yellow
                $done = $false; $result = $null
                while (-not $done) {
                    Start-Sleep -Seconds 5
                    $result = Invoke-RestMethod -Uri "$baseUrl/videos/$videoId" -Method GET -Headers @{ Authorization = "Bearer $openaiKey" }
                    $status = $result.status
                    Write-Host "  Status: $status"
                    if ($status -eq "completed") { $done = $true }
                    if ($status -eq "failed") { Write-Error "Video generation failed: $($result.error)"; exit 1 }
                }
                $videoUrl = $result.url
                Write-Host ""
                Write-Host "Video ready!" -ForegroundColor Green
                Write-Host "URL: $videoUrl"
                $outPath = Join-Path $PSScriptRoot "videos\$($Mode)_$(Get-Date -Format 'yyyyMMdd_HHmmss').mp4"
                New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
                Invoke-WebRequest -Uri $videoUrl -OutFile $outPath
                Write-Host "Saved to: $outPath"
            }
            "ref2video" {
                if (-not $Images -or $Images.Count -lt 2) { Write-Error "Need at least 2 -Images"; exit 1 }
                $abs = @(); foreach ($img in $Images) { if (-not (Test-Path $img)) { Write-Error "Image not found: $img"; exit 1 }; $abs += (Resolve-Path $img).Path }
                $refs = @(); foreach ($path in $abs) { $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($path)); $refs += "data:image/jpeg;base64,$b64" }
                $body = @{
                    model = "sora-1.0"
                    prompt = $Prompt
                    reference_images = $refs
                    duration = $Duration
                    resolution = $Resolution
                } | ConvertTo-Json
                Write-Host "Creating video task..." -ForegroundColor Green
                $resp = Invoke-RestMethod -Uri "$baseUrl/videos" -Method POST -ContentType "application/json" -Body $body -Headers @{ Authorization = "Bearer $openaiKey" }
                $videoId = $resp.id
                Write-Host "Task created: $videoId"
                Write-Host "Polling for completion..." -ForegroundColor Yellow
                $done = $false; $result = $null
                while (-not $done) {
                    Start-Sleep -Seconds 5
                    $result = Invoke-RestMethod -Uri "$baseUrl/videos/$videoId" -Method GET -Headers @{ Authorization = "Bearer $openaiKey" }
                    $status = $result.status
                    Write-Host "  Status: $status"
                    if ($status -eq "completed") { $done = $true }
                    if ($status -eq "failed") { Write-Error "Video generation failed: $($result.error)"; exit 1 }
                }
                $videoUrl = $result.url
                Write-Host ""
                Write-Host "Video ready!" -ForegroundColor Green
                Write-Host "URL: $videoUrl"
                $outPath = Join-Path $PSScriptRoot "videos\$($Mode)_$(Get-Date -Format 'yyyyMMdd_HHmmss').mp4"
                New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
                Invoke-WebRequest -Uri $videoUrl -OutFile $outPath
                Write-Host "Saved to: $outPath"
            }
        }
    }
    default {
        Write-Error "Unknown backend: $backend"
        exit 1
    }
}
