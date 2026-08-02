<#
.SYNOPSIS
Unified media generation system: image and video.
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
    [string]$ApiKeyName,
    [string]$ApiKey,
    [string]$ConfigPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$PSScriptRoot = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
if (-not $ConfigPath) { $ConfigPath = Join-Path $PSScriptRoot "agnes_config.json" }

function Get-Env([string]$Name) {
    return [Environment]::GetEnvironmentVariable($Name, "Process")
}

function Load-Config([string]$Path) {
    if (-not (Test-Path $Path)) {
        Write-Error "Config not found: $Path"
        exit 1
    }
    return Get-Content $Path -Raw | ConvertFrom-Json
}

$config = Load-Config -Path $ConfigPath

function Get-Key([string]$PreferredName, [string]$ProvidedKey) {
    if ($ProvidedKey) { return $ProvidedKey }
    if ($PreferredName) {
        $entry = $config.keys | Where-Object { $_.name -eq $PreferredName }
        if ($entry) { return $entry.api_key }
    }
    if ($config.default_key_name) {
        $entry = $config.keys | Where-Object { $_.name -eq $config.default_key_name }
        if ($entry) { return $entry.api_key }
    }
    return $null
}

function Get-BaseUrl([string]$PreferredName) {
    if ($PreferredName) {
        $entry = $config.keys | Where-Object { $_.name -eq $PreferredName }
        if ($entry) { return $entry.base_url }
    }
    if ($config.default_key_name) {
        $entry = $config.keys | Where-Object { $_.name -eq $config.default_key_name }
        if ($entry) { return $entry.base_url }
    }
    return "https://api.agnes-ai.cn/v1"
}

function Get-AgnesApi([string]$PreferredName) {
    if ($PreferredName) {
        $entry = $config.keys | Where-Object { $_.name -eq $PreferredName }
        if ($entry) { return $entry.agnes_api }
    }
    if ($config.default_key_name) {
        $entry = $config.keys | Where-Object { $_.name -eq $config.default_key_name }
        if ($entry) { return $entry.agnes_api }
    }
    return "https://api.agnes-ai.cn/agnesapi"
}

function Get-ImageModel([string]$PreferredName) {
    if ($PreferredName) {
        $entry = $config.keys | Where-Object { $_.name -eq $PreferredName }
        if ($entry -and $entry.models.image) { return $entry.models.image }
    }
    if ($config.default_key_name) {
        $entry = $config.keys | Where-Object { $_.name -eq $config.default_key_name }
        if ($entry -and $entry.models.image) { return $entry.models.image }
    }
    return "agnes-image-2.1-flash"
}

function Get-VideoModel([string]$PreferredName) {
    if ($PreferredName) {
        $entry = $config.keys | Where-Object { $_.name -eq $PreferredName }
        if ($entry -and $entry.models.video) { return $entry.models.video }
    }
    if ($config.default_key_name) {
        $entry = $config.keys | Where-Object { $_.name -eq $config.default_key_name }
        if ($entry -and $entry.models.video) { return $entry.models.video }
    }
    return "agnes-video-v2.0"
}

$key = Get-Key -PreferredName $ApiKeyName -ProvidedKey $ApiKey
if (-not $key) { Write-Error "No API key provided. Use -ApiKey, -ApiKeyName, or set AGNES_API_KEY."; exit 1 }

$baseUrl = Get-BaseUrl -PreferredName $ApiKeyName
$agnesApi = Get-AgnesApi -PreferredName $ApiKeyName
$imageModel = Get-ImageModel -PreferredName $ApiKeyName
$videoModel = Get-VideoModel -PreferredName $ApiKeyName

Write-Host ""
Write-Host "=== Media System ===" -ForegroundColor Cyan
Write-Host "Mode: $Mode"
if ($ApiKeyName) {
    Write-Host "KeyName: $ApiKeyName"
} else {
    Write-Host "Key: inline/env"
}
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
            $outPath = Join-Path $PSScriptRoot "images\text2img_$(Get-Date -Format 'yyyyMMdd_HHmmss').png"
            New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
            Invoke-WebRequest -Uri $imgUrl -OutFile $outPath
            Write-Host "Saved to: $outPath"
        }
        if ($b64) {
            $bytes = [Convert]::FromBase64String($b64)
            $outPath = Join-Path $PSScriptRoot "images\text2img_$(Get-Date -Format 'yyyyMMdd_HHmmss').png"
            New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
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
            $outPath = Join-Path $PSScriptRoot "images\img2img_$(Get-Date -Format 'yyyyMMdd_HHmmss').png"
            New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
            Invoke-WebRequest -Uri $imgUrl -OutFile $outPath
            Write-Host "Saved to: $outPath"
        }
        if ($b64Out) {
            $bytes = [Convert]::FromBase64String($b64Out)
            $outPath = Join-Path $PSScriptRoot "images\img2img_$(Get-Date -Format 'yyyyMMdd_HHmmss').png"
            New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
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
        $done = $false; $result = $null
        while (-not $done) {
            Start-Sleep -Seconds 5
            $result = Invoke-RestMethod -Uri "$agnesApi`?video_id=$videoId" -Method GET -Headers @{ Authorization = "Bearer $key" }
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
        $outPath = Join-Path $PSScriptRoot "videos\img2video_$(Get-Date -Format 'yyyyMMdd_HHmmss').mp4"
        New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
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
        $done = $false; $result = $null
        while (-not $done) {
            Start-Sleep -Seconds 5
            $result = Invoke-RestMethod -Uri "$agnesApi`?video_id=$videoId" -Method GET -Headers @{ Authorization = "Bearer $key" }
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
        $outPath = Join-Path $PSScriptRoot "videos\text2video_$(Get-Date -Format 'yyyyMMdd_HHmmss').mp4"
        New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
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
        $done = $false; $result = $null
        while (-not $done) {
            Start-Sleep -Seconds 5
            $result = Invoke-RestMethod -Uri "$agnesApi`?video_id=$videoId" -Method GET -Headers @{ Authorization = "Bearer $key" }
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
        $outPath = Join-Path $PSScriptRoot "videos\ref2video_$(Get-Date -Format 'yyyyMMdd_HHmmss').mp4"
        New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
        Invoke-WebRequest -Uri $videoUrl -OutFile $outPath
        Write-Host "Saved to: $outPath"
    }
}
