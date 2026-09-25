<#
.SYNOPSIS
Unified media generation with key failover. Supports text2img, img2img, img2video, text2video, ref2video.

.PARAMETER Mode
Generation mode: text2img, img2img, img2video, text2video, ref2video.

.PARAMETER Prompt
Text prompt for generation.

.PARAMETER Image
Path to a single input image for img2img or img2video.

.PARAMETER Images
Array of input image paths for ref2video (at least 2 required).

.PARAMETER AspectRatio
Aspect ratio for video generation. Default: auto.

.PARAMETER Duration
Video duration in seconds. Default: 6. Allowed: 6, 10.

.PARAMETER Resolution
Video resolution. Default: 480p. Allowed: 480p, 720p.

.PARAMETER KeyName
API key name from agnes_config.json.

.PARAMETER DryRun
Print request details without sending API calls.

.OUTPUT
Generated files are saved to output/images/ or output/videos/ with timestamped filenames.
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
    [string]$KeyName,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$PSScriptRoot = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
$projectRoot  = Split-Path $PSScriptRoot -Parent
$configPath   = Join-Path $projectRoot "agnes_config.json"
if (-not (Test-Path $configPath)) { Write-Error "Config not found: $configPath"; exit 1 }
$config = Get-Content $configPath -Raw | ConvertFrom-Json

$keyName = $KeyName
if (-not $keyName) { $keyName = $config.default_key_name }
$entry = $config.keys | Where-Object { $_.name -eq $keyName }
if (-not $entry) { Write-Error "Key not found in config: $keyName"; exit 1 }

$imageModel = $entry.models.image
$videoModel = $entry.models.video

function Get-HttpStatusCode {
    param($Exception)
    if ($null -eq $Exception) { return 0 }
    if ($Exception.Response) {
        return [int]$Exception.Response.StatusCode
    }
    if ($Exception.StatusCode) {
        return [int]$Exception.StatusCode
    }
    return 0
}

function Invoke-WithRetry {
    param(
        [Parameter(Mandatory=$true)][scriptblock]$Action,
        [int]$MaxRetries = 3
    )
    $attempt = 0
    while ($true) {
        $attempt++
        try {
            return & $Action
        } catch {
            $status = Get-HttpStatusCode -Exception $_.Exception
            if ($status -in @(429, 503, 500) -and $attempt -lt $MaxRetries) {
                Write-Host "  Download attempt $attempt failed with HTTP $status, retrying in 3s..." -ForegroundColor DarkYellow
                Start-Sleep -Seconds 3
                continue
            }
            throw
        }
    }
}

function Invoke-ApiWithRetry {
    param(
        [Parameter(Mandatory=$true)][scriptblock]$Action,
        [string]$Label
    )
    $attempt = 0
    $maxRetries = 2
    $baseDelay = 3
    while ($true) {
        $attempt++
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        try {
            $result = & $Action
            $sw.Stop()
            Write-RequestLog -Label $Label -Status 200 -DurationMs $sw.ElapsedMilliseconds
            return $result
        } catch {
            $sw.Stop()
            $status = Get-HttpStatusCode -Exception $_.Exception
            Write-RequestLog -Label $Label -Status $status -DurationMs $sw.ElapsedMilliseconds
            if ($status -in @(429, 503, 500) -and $attempt -le $maxRetries) {
                $delay = $baseDelay * [math]::Pow(2, $attempt - 1)
                Write-Host "  API attempt $attempt failed with HTTP $status, retrying in ${delay}s..." -ForegroundColor DarkYellow
                Start-Sleep -Seconds $delay
                continue
            }
            throw
        }
    }
}

function Write-RequestLog {
    param(
        [string]$Label,
        [int]$Status,
        [long]$DurationMs
    )
    $ts = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
    Write-Host "[$ts] [$keyName] [$Label] [$Status] [${DurationMs}ms]"
}

function Invoke-AgnesRest {
    param(
        [Parameter(Mandatory=$true)][ValidateSet("GET","POST")][string]$Method,
        [Parameter(Mandatory=$true)][string]$Endpoint,
        [string]$Body,
        [hashtable]$Query,
        [int]$MaxFailover = 3
    )
    $validKeys = @($config.keys | Where-Object { $_.api_key -and $_.api_key.Trim() -ne '' })
    if ($validKeys.Count -eq 0) { Write-Error "No valid API keys configured in $configPath"; exit 1 }

    $startIdx = 0
    for ($i = 0; $i -lt $validKeys.Count; $i++) {
        if ($validKeys[$i].name -eq $keyName) { $startIdx = $i; break }
    }

    $tried = 0
    for ($offset = 0; $offset -lt $validKeys.Count -and $tried -lt $MaxFailover; $offset++) {
        $idx = ($startIdx + $offset) % $validKeys.Count
        $kEntry = $validKeys[$idx]
        $kKey   = $kEntry.api_key
        $kBase  = $kEntry.base_url
        $kAgnes = $kEntry.agnes_api
        if (-not $kAgnes) { $kAgnes = "$kBase/agnesapi" }

        $uri = if ($Method -eq "GET" -and $Endpoint -eq "/agnesapi") { $kAgnes } else { $kBase + $Endpoint }
        if ($Query) {
            $qs = ($Query.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join "&"
            $uri += "?" + $qs
        }

        $headers = @{ Authorization = "Bearer $kKey" }
        try {
            if ($Method -eq "POST") {
                return Invoke-RestMethod -Uri $uri -Method POST -ContentType "application/json" -Body $Body -Headers $headers
            } else {
                return Invoke-RestMethod -Uri $uri -Method GET -Headers $headers
            }
        } catch {
            $status = Get-HttpStatusCode -Exception $_.Exception
            if ($status -in @(429, 503, 500)) {
                $tried++
                Write-Host "  [Key '$($kEntry.name)' failed with HTTP $status, rotating to next key...]" -ForegroundColor DarkYellow
                continue
            }
            throw
        }
    }
    Write-Error "API request failed after $MaxFailover failover attempts"
    exit 1
}

function Get-MimeType {
    param([string]$Path)
    $ext = [IO.Path]::GetExtension($Path).ToLower()
    $map = @{
        ".png"  = "image/png"
        ".jpg"  = "image/jpeg"
        ".jpeg" = "image/jpeg"
        ".webp" = "image/webp"
        ".gif"  = "image/gif"
        ".bmp"  = "image/bmp"
    }
    if ($map.ContainsKey($ext)) { return $map[$ext] }
    return "image/jpeg"
}

function Get-OutputImagePath {
    param([string]$Prefix)
    $ts   = Get-Date -Format 'yyyyMMdd_HHmmss'
    $rand = -join ((1..4) | ForEach-Object { '{0:X}' -f (Get-Random -Maximum 16) })
    $dir  = Join-Path $projectRoot "output\images"
    New-Item -ItemType Directory -Path $dir -Force | Out.Null
    return Join-Path $dir "${Prefix}_${ts}_$rand.png"
}

function Get-OutputVideoPath {
    param([string]$Prefix)
    $ts   = Get-Date -Format 'yyyyMMdd_HHmmss'
    $rand = -join ((1..4) | ForEach-Object { '{0:X}' -f (Get-Random -Maximum 16) })
    $dir  = Join-Path $projectRoot "output\videos"
    New-Item -ItemType Directory -Path $dir -Force | Out.Null
    return Join-Path $dir "${Prefix}_${ts}_$rand.mp4"
}

function Get-GenerationLogPath {
    $dir = Join-Path $projectRoot "output"
    New-Item -ItemType Directory -Path $dir -Force | Out.Null
    return Join-Path $dir "generation_log.jsonl"
}

function Write-GenerationLog {
    param(
        [string]$Mode,
        [string]$Prompt,
        [string]$Duration,
        [string]$Resolution,
        [string]$KeyName,
        [bool]$Success,
        [double]$ElapsedSec,
        [string]$OutputPath
    )
    $logPath = Get-GenerationLogPath
    $entry = [ordered]@{
        timestamp     = (Get-Date -Format "o")
        mode          = $Mode
        prompt        = $Prompt
        duration      = $Duration
        resolution    = $Resolution
        key_name      = $KeyName
        success       = $Success
        elapsed_sec   = [math]::Round($ElapsedSec, 2)
        output_path   = $OutputPath
    } | ConvertTo-Json -Compress
    Add-Content -Path $logPath -Value $entry -Encoding UTF8
}

function Write-FriendlyError {
    param($Exception)
    $status = Get-HttpStatusCode -Exception $Exception
    if ($status -in @(400, 422)) {
        $msg = $Exception.ErrorDetails.Message
        if ($msg) {
            try {
                $parsed = $msg | ConvertFrom-Json
                if ($parsed.error.message) { return $parsed.error.message }
                if ($parsed.error) { return $parsed.error }
                return $msg
            } catch {
                return $msg
            }
        }
        return "API request failed (HTTP $status). Check prompt and parameters."
    }
    return $Exception.Message
}

Write-Host ""
Write-Host "=== Media System ===" -ForegroundColor Cyan
Write-Host "Mode: $Mode"
Write-Host "KeyName: $keyName"
Write-Host "BaseUrl: $($entry.base_url)"
if ($DryRun) { Write-Host "DRY RUN: No API calls will be made." -ForegroundColor Yellow }
Write-Host ""

$globalTimer = [System.Diagnostics.Stopwatch]::StartNew()

switch ($Mode) {
    "text2img" {
        if (-not $Prompt) { Write-Error "Missing -Prompt"; exit 1 }
        if ($Prompt.Trim() -eq "") { Write-Error "-Prompt must not be empty"; exit 1 }
        $body = @{
            model = $imageModel
            prompt = $Prompt
            size = "1024x1024"
            n = 1
        } | ConvertTo-Json
        if ($DryRun) {
            $outPath = Get-OutputImagePath -Prefix "text2img"
            Write-Host "[DryRun] Endpoint: $($entry.base_url)/images/generations" -ForegroundColor Cyan
            Write-Host "[DryRun] Body: $($body.Substring(0, [Math]::Min(200, $body.Length)))..." -ForegroundColor DarkGray
            Write-Host "[DryRun] Output: $outPath"
            $globalTimer.Stop()
            Write-GenerationLog -Mode $Mode -Prompt $Prompt -Duration "N/A" -Resolution "1024x1024" -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
            exit 0
        }
        Write-Host "Creating image..." -ForegroundColor Green
        $resp = Invoke-ApiWithRetry -Action { Invoke-AgnesRest -Method POST -Endpoint "/images/generations" -Body $body } -Label "POST /images/generations"
        $item  = $resp.data[0]
        $imgUrl = $item.url
        $b64   = $item.b64_json
        if (-not $imgUrl -and -not $b64) { Write-Error "No image url or base64 returned"; exit 1 }
        if ($imgUrl) {
            Write-Host "Image URL: $imgUrl" -ForegroundColor Green
            $outPath = Get-OutputImagePath -Prefix "text2img"
            Invoke-WithRetry -Action { Invoke-WebRequest -Uri $imgUrl -OutFile $outPath }
            Write-Host "Saved to: $outPath"
        }
        if ($b64) {
            $bytes   = [Convert]::FromBase64String($b64)
            $outPath = Get-OutputImagePath -Prefix "text2img"
            [IO.File]::WriteAllBytes($outPath, $bytes)
            Write-Host "Saved to: $outPath"
        }
        $globalTimer.Stop()
        Write-GenerationLog -Mode $Mode -Prompt $Prompt -Duration "N/A" -Resolution "1024x1024" -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
    }
    "img2img" {
        if (-not $Image)  { Write-Error "Missing -Image"; exit 1 }
        if (-not (Test-Path $Image)) { Write-Error "Image not found: $Image"; exit 1 }
        if (-not $Prompt) { Write-Error "Missing -Prompt"; exit 1 }
        if ($Prompt.Trim() -eq "") { Write-Error "-Prompt must not be empty"; exit 1 }
        $abs = (Resolve-Path $Image).Path
        $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($abs))
        $mime = Get-MimeType -Path $abs
        $body = @{
            model    = $imageModel
            prompt   = $Prompt
            image    = "data:$mime;base64,$b64"
            size     = "1024x1024"
            n        = 1
        } | ConvertTo-Json
        if ($DryRun) {
            $outPath = Get-OutputImagePath -Prefix "img2img"
            Write-Host "[DryRun] Endpoint: $($entry.base_url)/images/generations" -ForegroundColor Cyan
            Write-Host "[DryRun] Body: $($body.Substring(0, [Math]::Min(200, $body.Length)))..." -ForegroundColor DarkGray
            Write-Host "[DryRun] Output: $outPath"
            $globalTimer.Stop()
            Write-GenerationLog -Mode $Mode -Prompt $Prompt -Duration "N/A" -Resolution "1024x1024" -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
            exit 0
        }
        Write-Host "Creating image from reference..." -ForegroundColor Green
        try {
            $resp = Invoke-ApiWithRetry -Action { Invoke-AgnesRest -Method POST -Endpoint "/images/generations" -Body $body } -Label "POST /images/generations"
        } catch {
            Write-Error (Write-FriendlyError -Exception $_.Exception)
        }
        $item    = $resp.data[0]
        $imgUrl  = $item.url
        $b64Out  = $item.b64_json
        if (-not $imgUrl -and -not $b64Out) { Write-Error "No image url or base64 returned"; exit 1 }
        if ($imgUrl) {
            Write-Host "Image URL: $imgUrl" -ForegroundColor Green
            $outPath = Get-OutputImagePath -Prefix "img2img"
            Invoke-WithRetry -Action { Invoke-WebRequest -Uri $imgUrl -OutFile $outPath }
            Write-Host "Saved to: $outPath"
        }
        if ($b64Out) {
            $bytes   = [Convert]::FromBase64String($b64Out)
            $outPath = Get-OutputImagePath -Prefix "img2img"
            [IO.File]::WriteAllBytes($outPath, $bytes)
            Write-Host "Saved to: $outPath"
        }
        $globalTimer.Stop()
        Write-GenerationLog -Mode $Mode -Prompt $Prompt -Duration "N/A" -Resolution "1024x1024" -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
    }
    "img2video" {
        if (-not $Image)  { Write-Error "Missing -Image"; exit 1 }
        if (-not (Test-Path $Image)) { Write-Error "Image not found: $Image"; exit 1 }
        if (-not $Prompt) { Write-Error "Missing -Prompt"; exit 1 }
        if ($Prompt.Trim() -eq "") { Write-Error "-Prompt must not be empty"; exit 1 }
        $abs = (Resolve-Path $Image).Path
        $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($abs))
        $mime = Get-MimeType -Path $abs
        $body = @{
            model           = $videoModel
            prompt          = $Prompt
            image           = "data:$mime;base64,$b64"
            duration        = $Duration
            resolution      = $Resolution
            aspect_ratio    = $AspectRatio
        } | ConvertTo-Json
        if ($DryRun) {
            $outPath = Get-OutputVideoPath -Prefix "img2video"
            Write-Host "[DryRun] Endpoint: $($entry.base_url)/videos" -ForegroundColor Cyan
            Write-Host "[DryRun] Body: $($body.Substring(0, [Math]::Min(200, $body.Length)))..." -ForegroundColor DarkGray
            Write-Host "[DryRun] Output: $outPath"
            $globalTimer.Stop()
            Write-GenerationLog -Mode $Mode -Prompt $Prompt -Duration $Duration -Resolution $Resolution -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
            exit 0
        }
        Write-Host "Creating video task..." -ForegroundColor Green
        try {
            $resp = Invoke-ApiWithRetry -Action { Invoke-AgnesRest -Method POST -Endpoint "/videos" -Body $body } -Label "POST /videos"
        } catch {
            Write-Error (Write-FriendlyError -Exception $_.Exception)
        }
        $videoId = $resp.video_id
        if (-not $videoId) { $videoId = $resp.id }
        Write-Host "Task created: $videoId" -ForegroundColor Cyan
        Write-Host "Polling for completion..." -ForegroundColor Yellow
        $done = $false; $result = $null; $retries = 0
        $pollInterval = if ($config.video_poll_interval_sec) { [int]$config.video_poll_interval_sec } else { 5 }
        $maxRetries = 60
        while (-not $done -and $retries -lt $maxRetries) {
            $retries++
            Start-Sleep -Seconds $pollInterval
            try {
                $result = Invoke-AgnesRest -Method GET -Endpoint "/agnesapi" -Query @{ video_id = $videoId }
            } catch {
                Write-Host "  Poll attempt $retries/$maxRetries failed: $_" -ForegroundColor DarkYellow
                continue
            }
            $status = $result.internal_status
            if (-not $status) { $status = $result.status }
            if ($retries % 10 -eq 0) {
                $elapsed = $retries * $pollInterval
                $remaining = ($maxRetries - $retries) * $pollInterval
                Write-Host "  [Progress] Poll $retries/$maxRetries, elapsed ~${elapsed}s, max remaining ~${remaining}s" -ForegroundColor DarkGray
            }
            Write-Host "  Status: $status ($retries/$maxRetries)"
            if ($status -eq "completed") { $done = $true }
            if ($status -eq "failed")   { Write-Error "Video generation failed: $($result.error)"; exit 1 }
        }
        if (-not $done) { Write-Error "Video generation timed out after $maxRetries attempts"; exit 1 }
        $videoUrl = $result.url
        Write-Host ""
        Write-Host "Video ready!" -ForegroundColor Green
        Write-Host "URL: $videoUrl"
        $outPath = Get-OutputVideoPath -Prefix "img2video"
        Invoke-WithRetry -Action { Invoke-WebRequest -Uri $videoUrl -OutFile $outPath }
        Write-Host "Saved to: $outPath"
        $globalTimer.Stop()
        Write-GenerationLog -Mode $Mode -Prompt $Prompt -Duration $Duration -Resolution $Resolution -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
    }
    "text2video" {
        if (-not $Prompt) { Write-Error "Missing -Prompt"; exit 1 }
        if ($Prompt.Trim() -eq "") { Write-Error "-Prompt must not be empty"; exit 1 }
        $body = @{
            model           = $videoModel
            prompt          = $Prompt
            duration        = $Duration
            resolution      = $Resolution
            aspect_ratio    = $AspectRatio
        } | ConvertTo-Json
        if ($DryRun) {
            $outPath = Get-OutputVideoPath -Prefix "text2video"
            Write-Host "[DryRun] Endpoint: $($entry.base_url)/videos" -ForegroundColor Cyan
            Write-Host "[DryRun] Body: $($body.Substring(0, [Math]::Min(200, $body.Length)))..." -ForegroundColor DarkGray
            Write-Host "[DryRun] Output: $outPath"
            $globalTimer.Stop()
            Write-GenerationLog -Mode $Mode -Prompt $Prompt -Duration $Duration -Resolution $Resolution -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
            exit 0
        }
        Write-Host "Creating video task..." -ForegroundColor Green
        try {
            $resp = Invoke-ApiWithRetry -Action { Invoke-AgnesRest -Method POST -Endpoint "/videos" -Body $body } -Label "POST /videos"
        } catch {
            Write-Error (Write-FriendlyError -Exception $_.Exception)
        }
        $videoId = $resp.video_id
        if (-not $videoId) { $videoId = $resp.id }
        Write-Host "Task created: $videoId" -ForegroundColor Cyan
        Write-Host "Polling for completion..." -ForegroundColor Yellow
        $done = $false; $result = $null; $retries = 0
        $pollInterval = if ($config.video_poll_interval_sec) { [int]$config.video_poll_interval_sec } else { 5 }
        $maxRetries = 60
        while (-not $done -and $retries -lt $maxRetries) {
            $retries++
            Start-Sleep -Seconds $pollInterval
            try {
                $result = Invoke-AgnesRest -Method GET -Endpoint "/agnesapi" -Query @{ video_id = $videoId }
            } catch {
                Write-Host "  Poll attempt $retries/$maxRetries failed: $_" -ForegroundColor DarkYellow
                continue
            }
            $status = $result.internal_status
            if (-not $status) { $status = $result.status }
            if ($retries % 10 -eq 0) {
                $elapsed = $retries * $pollInterval
                $remaining = ($maxRetries - $retries) * $pollInterval
                Write-Host "  [Progress] Poll $retries/$maxRetries, elapsed ~${elapsed}s, max remaining ~${remaining}s" -ForegroundColor DarkGray
            }
            Write-Host "  Status: $status ($retries/$maxRetries)"
            if ($status -eq "completed") { $done = $true }
            if ($status -eq "failed")   { Write-Error "Video generation failed: $($result.error)"; exit 1 }
        }
        if (-not $done) { Write-Error "Video generation timed out after $maxRetries attempts"; exit 1 }
        $videoUrl = $result.url
        Write-Host ""
        Write-Host "Video ready!" -ForegroundColor Green
        Write-Host "URL: $videoUrl"
        $outPath = Get-OutputVideoPath -Prefix "text2video"
        Invoke-WithRetry -Action { Invoke-WebRequest -Uri $videoUrl -OutFile $outPath }
        Write-Host "Saved to: $outPath"
        $globalTimer.Stop()
        Write-GenerationLog -Mode $Mode -Prompt $Prompt -Duration $Duration -Resolution $Resolution -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
    }
    "ref2video" {
        if (-not $Images -or $Images.Count -lt 2) { Write-Error "Need at least 2 -Images"; exit 1 }
        if (-not $Prompt) { Write-Error "Missing -Prompt"; exit 1 }
        if ($Prompt.Trim() -eq "") { Write-Error "-Prompt must not be empty"; exit 1 }
        $abs = @(); foreach ($img in $Images) { if (-not (Test-Path $img)) { Write-Error "Image not found: $img"; exit 1 }; $abs += (Resolve-Path $img).Path }
        $refs = @(); foreach ($path in $abs) { $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($path)); $mime = Get-MimeType -Path $path; $refs += "data:$mime;base64,$b64" }
        $body = @{
            model           = $videoModel
            prompt          = $Prompt
            reference_images = $refs
            duration        = $Duration
            resolution      = $Resolution
            aspect_ratio    = $AspectRatio
        } | ConvertTo-Json
        if ($DryRun) {
            $outPath = Get-OutputVideoPath -Prefix "ref2video"
            Write-Host "[DryRun] Endpoint: $($entry.base_url)/videos" -ForegroundColor Cyan
            Write-Host "[DryRun] Body: $($body.Substring(0, [Math]::Min(200, $body.Length)))..." -ForegroundColor DarkGray
            Write-Host "[DryRun] Output: $outPath"
            $globalTimer.Stop()
            Write-GenerationLog -Mode $Mode -Prompt $Prompt -Duration $Duration -Resolution $Resolution -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
            exit 0
        }
        Write-Host "Creating video task..." -ForegroundColor Green
        try {
            $resp = Invoke-ApiWithRetry -Action { Invoke-AgnesRest -Method POST -Endpoint "/videos" -Body $body } -Label "POST /videos"
        } catch {
            Write-Error (Write-FriendlyError -Exception $_.Exception)
        }
        $videoId = $resp.video_id
        if (-not $videoId) { $videoId = $resp.id }
        Write-Host "Task created: $videoId" -ForegroundColor Cyan
        Write-Host "Polling for completion..." -ForegroundColor Yellow
        $done = $false; $result = $null; $retries = 0
        $pollInterval = if ($config.video_poll_interval_sec) { [int]$config.video_poll_interval_sec } else { 5 }
        $maxRetries = 60
        while (-not $done -and $retries -lt $maxRetries) {
            $retries++
            Start-Sleep -Seconds $pollInterval
            try {
                $result = Invoke-AgnesRest -Method GET -Endpoint "/agnesapi" -Query @{ video_id = $videoId }
            } catch {
                Write-Host "  Poll attempt $retries/$maxRetries failed: $_" -ForegroundColor DarkYellow
                continue
            }
            $status = $result.internal_status
            if (-not $status) { $status = $result.status }
            if ($retries % 10 -eq 0) {
                $elapsed = $retries * $pollInterval
                $remaining = ($maxRetries - $retries) * $pollInterval
                Write-Host "  [Progress] Poll $retries/$maxRetries, elapsed ~${elapsed}s, max remaining ~${remaining}s" -ForegroundColor DarkGray
            }
            Write-Host "  Status: $status ($retries/$maxRetries)"
            if ($status -eq "completed") { $done = $true }
            if ($status -eq "failed")   { Write-Error "Video generation failed: $($result.error)"; exit 1 }
        }
        if (-not $done) { Write-Error "Video generation timed out after $maxRetries attempts"; exit 1 }
        $videoUrl = $result.url
        Write-Host ""
        Write-Host "Video ready!" -ForegroundColor Green
        Write-Host "URL: $videoUrl"
        $outPath = Get-OutputVideoPath -Prefix "ref2video"
        Invoke-WithRetry -Action { Invoke-WebRequest -Uri $videoUrl -OutFile $outPath }
        Write-Host "Saved to: $outPath"
        $globalTimer.Stop()
        Write-GenerationLog -Mode $Mode -Prompt $Prompt -Duration $Duration -Resolution $Resolution -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
    }
}
