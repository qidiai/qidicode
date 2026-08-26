# status_check.ps1 - health snapshot of qidicode local services.
#
# v2 (2026-08-27): rewritten for the current architecture:
#   8765  QiDi Relay (LAN proxy, runs from G:\程序开发\本地局域网中转-v3)
#   8766  websearch shim (.qidi/services/websearch-shim)
# (v1 probed the long-archived proxy-v2 build dirs; removed.)
#
# Usage:  powershell -ExecutionPolicy Bypass -File .qidi\ops\status_check.ps1

$ErrorActionPreference = "SilentlyContinue"

Write-Output "===== python processes ====="
Get-CimInstance Win32_Process -Filter "Name='python.exe' OR Name='py.exe'" |
  Select-Object ProcessId, CommandLine | Format-List

Write-Output "===== listening ports (8765/8766/8790/9800) ====="
$ports = netstat -ano | Select-String "LISTENING" | Select-String ":8765|:8766|:8790|:9800"
if ($ports) { $ports } else { Write-Output "none of 8765/8766/8790/9800 listening" }

Write-Output ""
Write-Output "===== websearch shim /health (127.0.0.1:8766) ====="
try {
    $h = Invoke-RestMethod "http://127.0.0.1:8766/health" -TimeoutSec 5
    Write-Output ("version={0} pid={1} ok={2} cache_hits={3} searches={4} cache_entries={5}" -f `
        $h.version, $h.pid, $h.ok, $h.cache_hits, $h.searches, $h.cache_entries)
} catch {
    Write-Output "unreachable: $($_.Exception.Message)"
}

Write-Output ""
Write-Output "===== QiDi relay (127.0.0.1:8765) ====="
try {
    $m = Invoke-RestMethod "http://127.0.0.1:8765/v1/models" `
        -Headers @{ Authorization = "Bearer sk-codely-proxy" } -TimeoutSec 8
    Write-Output ("/v1/models OK, {0} models available" -f @($m.data).Count)
} catch {
    Write-Output "/v1/models FAIL: $($_.Exception.Message)"
}

Write-Output ""
Write-Output "===== shim.log tail (last 15 lines) ====="
$log = Join-Path $PSScriptRoot "..\services\websearch-shim\shim.log"
if (Test-Path $log) {
    Get-Content $log -Tail 15 -Encoding UTF8
} else {
    Write-Output "no shim.log yet (shim has not served any request since start)"
}
