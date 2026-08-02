<#
.SYNOPSIS
Image generation system using Agnes AI image models.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)]
    [ValidateSet("text2img","img2img")]
    [string]$Mode,

    [string]$Prompt,
    [string]$Image,
    [ValidateSet("auto","1:1","16:9","9:16","3:2","2:3","2:1","1:2","19.5:9","9:19.5","20:9","9:20")]
    [string]$AspectRatio = "auto",
    [string]$ApiKey,
    [string]$Model = "agnes-image-2.1-flash"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-Env([string]$Name) {
    return [Environment]::GetEnvironmentVariable($Name, "Process")
}

$key = $ApiKey
if (-not $key) { $key = Get-Env "AGNES_API_KEY" }
if (-not $key) { Write-Error "AGNES_API_KEY not set"; exit 1 }

$baseUrl = "https://api.agnes-ai.cn/v1"
Write-Host ""
Write-Host "=== Image System ===" -ForegroundColor Cyan
Write-Host "Model: $Model"
Write-Host "Mode: $Mode"
Write-Host ""

switch ($Mode) {
    "text2img" {
        if (-not $Prompt) { Write-Error "Missing -Prompt"; exit 1 }
        $body = @{
            model = $Model
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
            $outPath = Join-Path $PSScriptRoot "images\$($Mode)_$(Get-Date -Format 'yyyyMMdd_HHmmss').png"
            New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
            Invoke-WebRequest -Uri $imgUrl -OutFile $outPath
            Write-Host "Saved to: $outPath"
        }
        if ($b64) {
            $bytes = [Convert]::FromBase64String($b64)
            $outPath = Join-Path $PSScriptRoot "images\$($Mode)_$(Get-Date -Format 'yyyyMMdd_HHmmss').png"
            New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
            [IO.File]::WriteAllBytes($outPath, $bytes)
            Write-Host "Saved to: $outPath"
        }
    }
    "img2img" {
        if (-not $Image) { Write-Error "Missing -Image"; exit 1 }
        if (-not (Test-Path $Image)) { Write-Error "Image not found: $Image"; exit 1 }
        $abs = (Resolve-Path $Image).Path
        $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($abs))
        $mime = "image/jpeg"
        $body = @{
            model = $Model
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
            $outPath = Join-Path $PSScriptRoot "images\$($Mode)_$(Get-Date -Format 'yyyyMMdd_HHmmss').png"
            New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
            Invoke-WebRequest -Uri $imgUrl -OutFile $outPath
            Write-Host "Saved to: $outPath"
        }
        if ($b64Out) {
            $bytes = [Convert]::FromBase64String($b64Out)
            $outPath = Join-Path $PSScriptRoot "images\$($Mode)_$(Get-Date -Format 'yyyyMMdd_HHmmss').png"
            New-Item -ItemType Directory -Path (Split-Path $outPath) -Force | Out-Null
            [IO.File]::WriteAllBytes($outPath, $bytes)
            Write-Host "Saved to: $outPath"
        }
    }
}
