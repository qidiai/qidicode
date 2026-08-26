# test_chat_models.ps1 - smoke-test chat completion for models in .qidi/config.toml
#
# v2 (2026-08-27): sanitized + modernized.
#   - keys read at runtime from config.toml (no credentials in this file)
#   - probes each model's own (base_url, api_key); supports substring filter
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File .qidi\ops\test_chat_models.ps1            # all models
#   powershell -ExecutionPolicy Bypass -File .qidi\ops\test_chat_models.ps1 -Filter glm  # subset

param([string]$Filter = "")

$ErrorActionPreference = "SilentlyContinue"
$cfg = Join-Path $PSScriptRoot "..\config.toml"
if (-not (Test-Path $cfg)) { Write-Output "config.toml not found: $cfg"; exit 1 }

# ---- parse [model."name"] blocks (quoted or bare) ----
$models = [ordered]@{}
$cur = $null
foreach ($line in Get-Content $cfg -Encoding UTF8) {
    if ($line -match '^\s*\[model\."?([^"\]]+)"?\]') {
        $cur = $Matches[1]; $models[$cur] = [ordered]@{ url = $null; key = $null; mid = $null }
    }
    elseif ($cur -and $line -match '^\s*base_url\s*=\s*"([^"]+)"') { $models[$cur].url = $Matches[1] }
    elseif ($cur -and $line -match '^\s*api_key\s*=\s*"([^"]+)"')  { $models[$cur].key = $Matches[1] }
    elseif ($cur -and $line -match '^\s*model\s*=\s*"([^"]+)"')    { $models[$cur].mid = $Matches[1] }
    elseif ($line -match '^\s*\[') { $cur = $null }
}

$targets = @($models.Keys | Where-Object { -not $Filter -or $_ -like "*$Filter*" })
Write-Output ("chat smoke test: {0} models" -f $targets.Count)
Write-Output ""

$okN = 0; $failN = 0
foreach ($name in $targets) {
    $e = $models[$name]
    $mid = if ($e.mid) { $e.mid } else { $name }
    $headers = @{ "Content-Type" = "application/json" }
    if ($e.key) { $headers["Authorization"] = "Bearer $($e.key)" }
    $body = @{ model = $mid; messages = @(@{ role = "user"; content = "hi" }); max_tokens = 5 } |
        ConvertTo-Json -Depth 5
    try {
        $r = Invoke-WebRequest -Uri "$($e.url)/chat/completions" -Method POST -Headers $headers `
            -Body $body -TimeoutSec 30 -UseBasicParsing
        $okN++
        Write-Output ("CHAT_OK   | {0} | via {1}" -f $name, $e.url)
    } catch {
        $failN++
        $code = "N/A"; if ($_.Exception.Response) { $code = [int]$_.Exception.Response.StatusCode }
        Write-Output ("CHAT_FAIL | {0} | via {1} | HTTP {2} | {3}" -f $name, $e.url, $code, $_.Exception.Message)
    }
}
Write-Output ""
Write-Output ("summary: {0} OK, {1} FAIL" -f $okN, $failN)
