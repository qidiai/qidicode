<#
http_client.ps1 - HTTP requests / unified retry / adaptive polling / key failover.
Dot-sourced by scripts/media_system.ps1.
Depends on script-scope variables: $config, $configPath, $keyName.

Optimizations (2026-08-04):
- B1: Adaptive video polling (dynamic interval based on attempt count)
- B2: Image auto-compression for large inputs (>5MB)
- B3: Unified retry policy (Invoke-WithPolicy)
- B4: MaxFailover from config (max_failover key)
- B5: Adaptive video timeout based on duration
- B7: HTTP connection pooling
- B8: throw instead of exit(1) for graceful degradation
#>

# ── Connection pooling (B7) ───────────────────────────────────────────────────
[System.Net.ServicePointManager]::DefaultConnectionLimit = 10
[System.Net.ServicePointManager]::Expect100Continue = $false

# ── MaxFailover from config (B4) ─────────────────────────────────────────────
function Get-MaxFailover {
    param([int]$Default = 3)
    if ($config.max_failover -and [int]$config.max_failover -gt 0) {
        return [int]$config.max_failover
    }
    return $Default
}

# ── Image compression helper (B2) ────────────────────────────────────────────
function Compress-ImageIfNeeded {
    param(
        [string]$ImagePath,
        [long]$MaxBytes = 5MB
    )
    $info = Get-Item $ImagePath -ErrorAction SilentlyContinue
    if (-not $info) { return $ImagePath }
    if ($info.Length -le $MaxBytes) { return $ImagePath }

    Write-Host "  Image exceeds 5MB ($([math]::Round($info.Length/1KB,0))KB), compressing..." -ForegroundColor Yellow
    try {
        Add-Type -AssemblyName System.Drawing -ErrorAction Stop
        $img = [System.Drawing.Image]::FromFile($ImagePath)
        $bmp = New-Object System.Drawing.Bitmap(1024, 1024)
        $graphics = [System.Drawing.Graphics]::FromImage($bmp)
        $graphics.SmoothingMode = 'HighQuality'
        $graphics.InterpolationMode = 'HighQualityBicubic'
        $graphics.DrawImage($img, 0, 0, 1024, 1024)
        $graphics.Dispose()
        $img.Dispose()
        $ms = New-Object System.IO.MemoryStream
        $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Jpeg)
        $bmp.Dispose()
        $bytes = $ms.ToArray()
        $ms.Dispose()
        # Write compressed to temp file
        $tempPath = $ImagePath + ".compressed.jpg"
        [IO.File]::WriteAllBytes($tempPath, $bytes)
        Write-Host "  Compressed: $([math]::Round($bytes.Length/1KB,0))KB -> saved to $tempPath" -ForegroundColor Gray
        return $tempPath
    } catch {
        Write-Host "  Image compression failed, using original: $_" -ForegroundColor DarkYellow
        return $ImagePath
    }
}

function Get-ImageBytes {
    param([string]$Path)
    $compressed = Compress-ImageIfNeeded -ImagePath $Path
    return [Convert]::ToBase64String([IO.File]::ReadAllBytes($compressed))
}

# ── HTTP Status Code ──────────────────────────────────────────────────────────
function Get-HttpStatusCode {
    param($Exception)
    if ($null -eq $Exception) { return 0 }
    $resp = $null
    try { $resp = $Exception.Response } catch { }
    if ($resp) { return [int]$resp.StatusCode }
    $sc = $null
    try { $sc = $Exception.StatusCode } catch { }
    if ($sc) { return [int]$sc }
    return 0
}

# ── Unified Retry Policy (B3) ─────────────────────────────────────────────────
# Combines API retry (exponential backoff) and download retry (fixed interval)
function Invoke-WithPolicy {
    param(
        [Parameter(Mandatory=$true)][scriptblock]$Action,
        [string]$Label = "",
        [int]$MaxRetries = 3,
        [int]$BaseDelay = 3,
        [bool]$ExponentialBackoff = $true,
        [int[]]$RetryCodes = @(429, 503, 500)
    )
    $attempt = 0
    while ($true) {
        $attempt++
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        try {
            $result = & $Action
            $sw.Stop()
            if ($Label) {
                Write-RequestLog -Label $Label -Status 200 -DurationMs $sw.ElapsedMilliseconds
            }
            return $result
        } catch {
            $sw.Stop()
            $status = Get-HttpStatusCode -Exception $_.Exception
            if ($Label) {
                Write-RequestLog -Label $Label -Status $status -DurationMs $sw.ElapsedMilliseconds
            }
            if ($status -in $RetryCodes -and $attempt -le $MaxRetries) {
                if ($ExponentialBackoff) {
                    $delay = $BaseDelay * [math]::Pow(2, $attempt - 1)
                } else {
                    $delay = $BaseDelay
                }
                $type = if ($Label -match "Download") { "Download" } else { "API" }
                Write-Host "  $type attempt $attempt failed with HTTP $status, retrying in ${delay}s..." -ForegroundColor DarkYellow
                Start-Sleep -Seconds $delay
                continue
            }
            throw
        }
    }
}

# Aliases for backward compatibility
function Invoke-WithRetry {
    param(
        [Parameter(Mandatory=$true)][scriptblock]$Action,
        [int]$MaxRetries = 3
    )
    Invoke-WithPolicy -Action $Action -MaxRetries $MaxRetries -BaseDelay 3 -ExponentialBackoff $false -Label "Download"
}

function Invoke-ApiWithRetry {
    param(
        [Parameter(Mandatory=$true)][scriptblock]$Action,
        [string]$Label
    )
    Invoke-WithPolicy -Action $Action -Label $Label -MaxRetries 2 -BaseDelay 3 -ExponentialBackoff $true
}

# ── Agnes REST client with failover (B4: max_failover from config) ─────────────
function Invoke-AgnesRest {
    param(
        [Parameter(Mandatory=$true)][ValidateSet("GET","POST")][string]$Method,
        [Parameter(Mandatory=$true)][string]$Endpoint,
        [string]$Body,
        [hashtable]$Query,
        [int]$MaxFailover = 0  # 0 = read from config
    )
    if ($MaxFailover -eq 0) { $MaxFailover = Get-MaxFailover }

    $validKeys = Get-ValidApiKeys -Config $config
    if ($validKeys.Count -eq 0) {
        # B8: throw instead of exit(1) for graceful degradation
        throw "No valid API keys configured in $configPath"
    }

    $startIdx = Get-FailoverStartIndex -Keys $validKeys -KeyName $keyName
    $tried = 0
    for ($offset = 0; $offset -lt $validKeys.Count -and $tried -lt $MaxFailover; $offset++) {
        $idx = ($startIdx + $offset) % $validKeys.Count
        $kEntry = $validKeys[$idx]
$kKey   = Get-EffectiveApiKey $kEntry
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
    # B8: throw instead of exit(1)
    throw "API request failed after $MaxFailover failover attempts"
}

# ── Adaptive Video Polling (B1 + B5) ──────────────────────────────────────────
function Wait-VideoTask {
    param(
        [Parameter(Mandatory=$true)][string]$VideoId,
        [int]$MaxAttempts = 0,  # 0 = auto-calculate from duration
        [int]$Duration = 6
    )
    $pollInterval = if ($config.video_poll_interval_sec) { [int]$config.video_poll_interval_sec } else { 5 }

    # B5: Adaptive timeout based on duration
    if ($MaxAttempts -eq 0) {
        $timeoutSec = 60 + $Duration * 10  # 6s->120s, 10s->160s
        $MaxAttempts = [Math]::Ceiling($timeoutSec / $pollInterval)
    }

    Write-Host "Polling for completion (max ~$($MaxAttempts * $pollInterval)s)..." -ForegroundColor Yellow
    $done = $false; $result = $null; $retries = 0
    while (-not $done -and $retries -lt $MaxAttempts) {
        $retries++
        # B1: Adaptive polling interval
        if ($retries -le 5) {
            $sleep = 3
        } elseif ($retries -le 15) {
            $sleep = $pollInterval
        } elseif ($retries -le 30) {
            $sleep = 10
        } else {
            $sleep = 15
        }
        Start-Sleep -Seconds $sleep
        try {
            $result = Invoke-AgnesRest -Method GET -Endpoint "/agnesapi" -Query @{ video_id = $VideoId }
        } catch {
            Write-Host "  Poll attempt $retries/$MaxAttempts failed: $_" -ForegroundColor DarkYellow
            continue
        }
        $status = $result.internal_status
        if (-not $status) { $status = $result.status }
        if ($retries % 10 -eq 0) {
            $elapsed = ($retries - 1) * $sleep
            $remaining = ($MaxAttempts - $retries) * $sleep
            Write-Host "  [Progress] Poll $retries/$MaxAttempts, elapsed ~${elapsed}s, max remaining ~${remaining}s" -ForegroundColor DarkGray
        }
        Write-Host "  Status: $status ($retries/$MaxAttempts)"
        if ($status -eq "completed") { $done = $true }
        if ($status -eq "failed") {
            # B8: throw for graceful degradation
            throw "Video generation failed: $($result.error)"
        }
    }
    if (-not $done) {
        throw "Video generation timed out after $MaxAttempts attempts"
    }
    return $result
}

function Save-RemoteFile {
    param(
        [Parameter(Mandatory=$true)][string]$Uri,
        [Parameter(Mandatory=$true)][string]$OutFile
    )
    Invoke-WithRetry -Action { Invoke-WebRequest -Uri $Uri -OutFile $OutFile }
}