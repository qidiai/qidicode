# check_models.ps1 - probe every model endpoint defined in .qidi/config.toml
#
# v2 (2026-08-27): sanitized. Keys are read at runtime from config.toml
# (the single source of truth); NO credentials are hardcoded in this file.
# Endpoints are deduplicated by (base_url, api_key) before probing.
#
# Usage:  powershell -ExecutionPolicy Bypass -File .qidi\ops\check_models.ps1

$ErrorActionPreference = "SilentlyContinue"
$cfg = Join-Path $PSScriptRoot "..\config.toml"
if (-not (Test-Path $cfg)) { Write-Output "config.toml not found: $cfg"; exit 1 }

# ---- parse [model."name"] blocks ----
$models = [ordered]@{}
$cur = $null
foreach ($line in Get-Content $cfg -Encoding UTF8) {
    if ($line -match '^\s*\[model\."?([^"\]]+)"?\]') {
        $cur = $Matches[1]; $models[$cur] = [ordered]@{ url = $null; key = $null }
    }
    elseif ($cur -and $line -match '^\s*base_url\s*=\s*"([^"]+)"') { $models[$cur].url = $Matches[1] }
    elseif ($cur -and $line -match '^\s*api_key\s*=\s*"([^"]+)"')  { $models[$cur].key = $Matches[1] }
    elseif ($line -match '^\s*\[') { $cur = $null }
}
Write-Output ("parsed {0} model entries from config.toml" -f $models.Count)

# ---- dedupe endpoints ----
$eps = [ordered]@{}
foreach ($m in $models.Keys) {
    $e = $models[$m]
    if (-not $e.url) { continue }
    $k = "$($e.url)|$($e.key)"
    if (-not $eps.Contains($k)) { $eps[$k] = [ordered]@{ url = $e.url; key = $e.key; names = @() } }
    $eps[$k].names += $m
}
Write-Output ("{0} unique endpoints" -f $eps.Count)
Write-Output ""

# ---- probe ----
$okN = 0; $failN = 0
foreach ($k in $eps.Keys) {
    $e = $eps[$k]
    try {
        $headers = @{ "Content-Type" = "application/json" }
        if ($e.key) { $headers["Authorization"] = "Bearer $($e.key)" }
        $r = Invoke-WebRequest -Uri "$($e.url)/models" -Method GET -Headers $headers -TimeoutSec 8 -UseBasicParsing
        $ids = @()
        try { $j = $r.Content | ConvertFrom-Json; if ($j.data) { $ids = @($j.data | ForEach-Object { $_.id }) } } catch {}
        $okN++
        Write-Output ("OK   | {0} | HTTP {1} | {2} models | for: {3}" -f $e.url, $r.StatusCode, $ids.Count, ($e.names -join ", "))
    } catch {
        $failN++
        $code = "N/A"; if ($_.Exception.Response) { $code = [int]$_.Exception.Response.StatusCode }
        Write-Output ("FAIL | {0} | HTTP {1} | {2} | for: {3}" -f $e.url, $code, $_.Exception.Message, ($e.names -join ", "))
    }
}
Write-Output ""
Write-Output ("summary: {0} OK, {1} FAIL" -f $okN, $failN)
