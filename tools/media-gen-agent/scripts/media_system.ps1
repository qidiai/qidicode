<#
.SYNOPSIS
Media generation helper using local agnes_config.json.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)]
    [ValidateSet("text2img","img2img","img2video","text2video","ref2video")]
    [string]$Mode,

    [string]$Prompt,
    [string]$Image,
    [string[]]$Images,
    [ValidateSet("auto","1:1","16:9","9:16","3:2","2:3","2:1","1:2","19.5:9","9:19.5","20:9","9:20")]
    [string]$AspectRatio = "auto",
    [ValidateSet(6,10)]
    [int]$Duration = 6,
    [ValidateSet("480p","720p")]
    [string]$Resolution = "480p",
    [string]$KeyName
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$PSScriptRoot = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
$configPath = Join-Path $PSScriptRoot "..\agnes_config.json"
if (-not (Test-Path $configPath)) { Write-Error "Config not found: $configPath"; exit 1 }
$config = Get-Content $configPath -Raw | ConvertFrom-Json

$keyName = $KeyName
if (-not $keyName) { $keyName = $config.default_key_name }
$entry = $config.keys | Where-Object { $_.name -eq $keyName }
if (-not $entry) { Write-Error "Key not found in config: $keyName"; exit 1 }

$key = $entry.api_key
$baseUrl = $entry.base_url
$agnesApi = $entry.agnes_api
if (-not $agnesApi) { $agnesApi = "$baseUrl/agnesapi" }
$imageModel = $entry.models.image
$videoModel = $entry.models.video

function Get-OutputImagePath {
    param([string]$Prefix)
    $ts = Get-Date -Format 'yyyyMMdd_HHmmss'
    $rand = -join ((1..4) | ForEach-Object { '{0:X}' -f (Get-Random -Maximum 16) })
    return Join-Path $PSScriptRoot "images\${Prefix}_${ts}_$rand.png"
}

function Get-OutputVideoPath {
    param([string]$Prefix)
    $ts = Get-Date -Format 'yyyyMMdd_HHmmss'
    $rand = -join ((1..4) | ForEach-Object { '{0:X}' -f (Get-Random -Maximum 16) })
    return Join-Path $PSScriptRoot "videos\${Prefix}_${ts}_$rand.mp4"
}

Write-Host ""
Write-Host "=== Media System ===" -ForegroundColor Cyan
Write-Host "Mode: $Mode"
Write-Host "KeyName: $keyName"
Write-Host "BaseUrl: $baseUrl"
Write-Host ""

switch ($Mode) {
    "text2img" {
        if (-not $Prompt) { Write-Error "Missing -Prompt"; exit 1 }
        $body = @{
            model = $imageModel
            prompt = $Prompt
            size = "1024x1024"
            n = 1
        } | ConvertTo-Json
        Write-Host "Creating image..." -ForegroundColor Green
        $resp = Invoke-RestMethod -Uri "$baseUrl/images/generations" -Method POST -ContentType "application/json" -Body $body -Headers @{ Authorization = "Bearer $key" }
        $item = $resp.data[0]
        $imgUrl = $item.url
        $b64 = $item.b64_json
        if (-not $imgUrl -and -not $b64) { Write-Error "No image url or base64 returned"; exit 1 }
        if ($imgUrl) {
            Write-Host "Image URL: $imgUrl" -ForegroundColor Green
            $outPath = Get-OutputImagePath -Prefix "text2img"
            New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out.Null
            Invoke-WebRequest -Uri $imgUrl -OutFile $outPath
            Write-Host "Saved to: $outPath"
        }
        if ($b64) {
            $bytes = [Convert]::FromBase64String($b64)
            $outPath = Get-OutputImagePath -Prefix "text2img"
            New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out.Null
            [IO.File]::WriteAllBytes($outPath, $bytes)
            Write-Host "Saved to: $outPath"
        }
    }
    "img2img" {
        if (-not $Image) { Write-Error "Missing -Image"; exit 1 }
        if (-not (Test-Path $Image)) { Write-Error "Image not found: $Image"; exit 1 }
        if (-not $Prompt) { Write-Error "Missing -Prompt"; exit 1 }
        $abs = (Resolve-Path $Image).Path
        $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($abs))
        $mime = "image/jpeg"
        $body = @{
            model = $imageModel
            prompt = $Prompt
            image = "data:$mime;base64,$b64"
            size = "1024x1024"
            n = 1
        } | ConvertTo-Json
        Write-Host "Creating image from reference..." -ForegroundColor Green
        $resp = Invoke-RestMethod -Uri "$baseUrl/images/generations" -Method POST -ContentType "application/json" -Body $body -Headers @{ Authorization = "Bearer $key" }
        $item = $resp.data[0]
        $imgUrl = $item.url
        $b64Out = $item.b64_json
        if (-not $imgUrl -and -not $b64Out) { Write-Error "No image url or base64 returned"; exit 1 }
        if ($imgUrl) {
            Write-Host "Image URL: $imgUrl" -ForegroundColor Green
            $outPath = Get-OutputImagePath -Prefix "img2img"
            New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out.Null
            Invoke-WebRequest -Uri $imgUrl -OutFile $outPath
            Write-Host "Saved to: $outPath"
        }
        if ($b64Out) {
            $bytes = [Convert]::FromBase64String($b64Out)
            $outPath = Get-OutputImagePath -Prefix "img2img"
            New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out.Null
            [IO.File]::WriteAllBytes($outPath, $bytes)
            Write-Host "Saved to: $outPath"
        }
    }
    "img2video" {
        if (-not $Image) { Write-Error "Missing -Image"; exit 1 }
        if (-not (Test-Path $Image)) { Write-Error "Image not found: $Image"; exit 1 }
        $abs = (Resolve-Path $Image).Path
        $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($abs))
        $mime = "image/jpeg"
        $body = @{
            model = $videoModel
            prompt = $Prompt
            image = "data:$mime;base64,$b64"
            duration = $Duration
            resolution = $Resolution
        } | ConvertTo-Json
        Write-Host "Creating video task..." -ForegroundColor Green
        $resp = Invoke-RestMethod -Uri "$baseUrl/videos" -Method POST -ContentType "application/json" -Body $body -Headers @{ Authorization = "Bearer $key" }
        $videoId = $resp.video_id
        if (-not $videoId) { $videoId = $resp.id }
        Write-Host "Task created: $videoId" -ForegroundColor Cyan
        Write-Host "Polling for completion..." -ForegroundColor Yellow
        $done = $false; $result = $null; $retries = 0; $maxRetries = 60
        while (-not $done -and $retries -lt $maxRetries) {
            $retries++
            Start-Sleep -Seconds 5
            try {
                $result = Invoke-RestMethod -Uri "$agnesApi`?video_id=$videoId" -Method GET -Headers @{ Authorization = "Bearer $key" }
            } catch {
                Write-Host "  Poll attempt $retries/$maxRetries failed: $_" -ForegroundColor DarkYellow
                continue
            }
            $status = $result.internal_status
            if (-not $status) { $status = $result.status }
            Write-Host "  Status: $status ($retries/$maxRetries)"
            if ($status -eq "completed") { $done = $true }
            if ($status -eq "failed") { Write-Error "Video generation failed: $($result.error)"; exit 1 }
        }
        if (-not $done) { Write-Error "Video generation timed out after $maxRetries attempts"; exit 1 }
        $videoUrl = $result.url
        Write-Host ""
        Write-Host "Video ready!" -ForegroundColor Green
        Write-Host "URL: $videoUrl"
        $outPath = Get-OutputVideoPath -Prefix "img2video"
        New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out.Null
        Invoke-WebRequest -Uri $videoUrl -OutFile $outPath
        Write-Host "Saved to: $outPath"
    }
    "text2video" {
        $body = @{
            model = $videoModel
            prompt = $Prompt
            duration = $Duration
            resolution = $Resolution
            aspect_ratio = $AspectRatio
        } | ConvertTo-Json
        Write-Host "Creating video task..." -ForegroundColor Green
        $resp = Invoke-RestMethod -Uri "$baseUrl/videos" -Method POST -ContentType "application/json" -Body $body -Headers @{ Authorization = "Bearer $key" }
        $videoId = $resp.video_id
        if (-not $videoId) { $videoId = $resp.id }
        Write-Host "Task created: $videoId" -ForegroundColor Cyan
        Write-Host "Polling for completion..." -ForegroundColor Yellow
        $done = $false; $result = $null; $retries = 0; $maxRetries = 60
        while (-not $done -and $retries -lt $maxRetries) {
            $retries++
            Start-Sleep -Seconds 5
            try {
                $result = Invoke-RestMethod -Uri "$agnesApi`?video_id=$videoId" -Method GET -Headers @{ Authorization = "Bearer $key" }
            } catch {
                Write-Host "  Poll attempt $retries/$maxRetries failed: $_" -ForegroundColor DarkYellow
                continue
            }
            $status = $result.internal_status
            if (-not $status) { $status = $result.status }
            Write-Host "  Status: $status ($retries/$maxRetries)"
            if ($status -eq "completed") { $done = $true }
            if ($status -eq "failed") { Write-Error "Video generation failed: $($result.error)"; exit 1 }
        }
        if (-not $done) { Write-Error "Video generation timed out after $maxRetries attempts"; exit 1 }
        $videoUrl = $result.url
        Write-Host ""
        Write-Host "Video ready!" -ForegroundColor Green
        Write-Host "URL: $videoUrl"
        $outPath = Get-OutputVideoPath -Prefix "text2video"
        New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out.Null
        Invoke-WebRequest -Uri $videoUrl -OutFile $outPath
        Write-Host "Saved to: $outPath"
    }
    "ref2video" {
        if (-not $Images -or $Images.Count -lt 2) { Write-Error "Need at least 2 -Images"; exit 1 }
        $abs = @(); foreach ($img in $Images) { if (-not (Test-Path $img)) { Write-Error "Image not found: $img"; exit 1 }; $abs += (Resolve-Path $img).Path }
        $refs = @(); foreach ($path in $abs) { $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($path)); $refs += "data:image/jpeg;base64,$b64" }
        $body = @{
            model = $videoModel
            prompt = $Prompt
            reference_images = $refs
            duration = $Duration
            resolution = $Resolution
        } | ConvertTo-Json
        Write-Host "Creating video task..." -ForegroundColor Green
        $resp = Invoke-RestMethod -Uri "$baseUrl/videos" -Method POST -ContentType "application/json" -Body $body -Headers @{ Authorization = "Bearer $key" }
        $videoId = $resp.video_id
        if (-not $videoId) { $videoId = $resp.id }
        Write-Host "Task created: $videoId" -ForegroundColor Cyan
        Write-Host "Polling for completion..." -ForegroundColor Yellow
        $done = $false; $result = $null; $retries = 0; $maxRetries = 60
        while (-not $done -and $retries -lt $maxRetries) {
            $retries++
            Start-Sleep -Seconds 5
            try {
                $result = Invoke-RestMethod -Uri "$agnesApi`?video_id=$videoId" -Method GET -Headers @{ Authorization = "Bearer $key" }
            } catch {
                Write-Host "  Poll attempt $retries/$maxRetries failed: $_" -ForegroundColor DarkYellow
                continue
            }
            $status = $result.internal_status
            if (-not $status) { $status = $result.status }
            Write-Host "  Status: $status ($retries/$maxRetries)"
            if ($status -eq "completed") { $done = $true }
            if ($status -eq "failed") { Write-Error "Video generation failed: $($result.error)"; exit 1 }
        }
        if (-not $done) { Write-Error "Video generation timed out after $maxRetries attempts"; exit 1 }
        $videoUrl = $result.url
        Write-Host ""
        Write-Host "Video ready!" -ForegroundColor Green
        Write-Host "URL: $videoUrl"
        $outPath = Get-OutputVideoPath -Prefix "ref2video"
        New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out.Null
        Invoke-WebRequest -Uri $videoUrl -OutFile $outPath
        Write-Host "Saved to: $outPath"
    }
}
