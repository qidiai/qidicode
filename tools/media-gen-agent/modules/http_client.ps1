<#
http_client.ps1 - HTTP requests / retry / 429/503/500 key failover / video task polling.
Dot-sourced by scripts/media_system.ps1.
Depends on script-scope variables: $config, $configPath, $keyName (set by the entry script).
#>

function Get-HttpStatusCode {
    param($Exception)
    if ($null -eq $Exception) { return 0 }
    $resp = $null
    try { $resp = $Exception.Response } catch { }
    if ($resp) {
        return [int]$resp.StatusCode
    }
    $sc = $null
    try { $sc = $Exception.StatusCode } catch { }
    if ($sc) {
        return [int]$sc
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

function Invoke-AgnesRest {
    param(
        [Parameter(Mandatory=$true)][ValidateSet("GET","POST")][string]$Method,
        [Parameter(Mandatory=$true)][string]$Endpoint,
        [string]$Body,
        [hashtable]$Query,
        [int]$MaxFailover = 3
    )
    $validKeys = Get-ValidApiKeys -Config $config
    if ($validKeys.Count -eq 0) { Write-Error "No valid API keys configured in $configPath"; exit 1 }

    $startIdx = Get-FailoverStartIndex -Keys $validKeys -KeyName $keyName

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

function Wait-VideoTask {
    param(
        [Parameter(Mandatory=$true)][string]$VideoId,
        [int]$MaxAttempts = 60
    )
    $pollInterval = if ($config.video_poll_interval_sec) { [int]$config.video_poll_interval_sec } else { 5 }
    Write-Host "Polling for completion..." -ForegroundColor Yellow
    $done = $false; $result = $null; $retries = 0
    while (-not $done -and $retries -lt $MaxAttempts) {
        $retries++
        Start-Sleep -Seconds $pollInterval
        try {
            $result = Invoke-AgnesRest -Method GET -Endpoint "/agnesapi" -Query @{ video_id = $VideoId }
        } catch {
            Write-Host "  Poll attempt $retries/$MaxAttempts failed: $_" -ForegroundColor DarkYellow
            continue
        }
        $status = $result.internal_status
        if (-not $status) { $status = $result.status }
        if ($retries % 10 -eq 0) {
            $elapsed = $retries * $pollInterval
            $remaining = ($MaxAttempts - $retries) * $pollInterval
            Write-Host "  [Progress] Poll $retries/$MaxAttempts, elapsed ~${elapsed}s, max remaining ~${remaining}s" -ForegroundColor DarkGray
        }
        Write-Host "  Status: $status ($retries/$MaxAttempts)"
        if ($status -eq "completed") { $done = $true }
        if ($status -eq "failed")   { Write-Error "Video generation failed: $($result.error)"; exit 1 }
    }
    if (-not $done) { Write-Error "Video generation timed out after $MaxAttempts attempts"; exit 1 }
    return $result
}

function Save-RemoteFile {
    param(
        [Parameter(Mandatory=$true)][string]$Uri,
        [Parameter(Mandatory=$true)][string]$OutFile
    )
    Invoke-WithRetry -Action { Invoke-WebRequest -Uri $Uri -OutFile $OutFile }
}
