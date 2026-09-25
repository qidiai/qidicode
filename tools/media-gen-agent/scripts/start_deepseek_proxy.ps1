<#
.SYNOPSIS
Start DeepSeek token stats proxy.
#>
param(
    [string]$UpstreamApiKey = "sk-CHANGE_ME",
    [int]$ListenPort = 9801
)

$exe = "G:\qidicode\target\release\token_stats_service.exe"
if (-not (Test-Path $exe)) {
    Write-Error "Proxy exe not found: $exe"
    exit 1
}

$dbPath = "G:\qidicode\target\debug\media-gen-agent\token_stats_service\token_stats.jsonl"

# Check if port is already in use
$existing = netstat -ano | Select-String ":${ListenPort}\s" | Select-Object -First 1
if ($existing) {
    Write-Host "Port $ListenPort already in use, assuming proxy is running." -ForegroundColor Yellow
    try {
        $r = Invoke-RestMethod -Uri "http://127.0.0.1:${ListenPort}/stats" -Method GET
        Write-Host "Proxy is running. Stats:" -ForegroundColor Green
        $r | ConvertTo-Json
    } catch {
        Write-Host "Port $ListenPort occupied but proxy not responding. Kill the process or use -ListenPort to specify another port." -ForegroundColor Red
    }
    exit 0
}

Write-Host "Starting DeepSeek token proxy..."
Start-Process -FilePath $exe -ArgumentList @(
    "--upstream-base-url", "https://api.deepseek.com/v1",
    "--upstream-api-key", $UpstreamApiKey,
    "--listen-host", "127.0.0.1",
    "--listen-port", "$ListenPort",
    "--db-path", $dbPath
) -WindowStyle Hidden

Start-Sleep -Seconds 2
try {
    $r = Invoke-RestMethod -Uri "http://127.0.0.1:${ListenPort}/stats" -Method GET
    Write-Host "Proxy is running. Stats:" -ForegroundColor Green
    $r | ConvertTo-Json
} catch {
    Write-Host "Proxy started but stats not ready yet." -ForegroundColor Yellow
}
