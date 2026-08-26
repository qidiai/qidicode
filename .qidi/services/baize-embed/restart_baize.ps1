# Restart baize-embed server.py detached from the current terminal job object.
# Pattern identical to websearch-shim/restart_shim.ps1 (WMI spawn, survives callers).

$dir = $PSScriptRoot

# 1) stop existing server (whoever owns port 8800)
$conn = Get-NetTCPConnection -LocalPort 8800 -State Listen -ErrorAction SilentlyContinue
if ($conn) {
    $oldPid = ($conn | Select-Object -First 1).OwningProcess
    Write-Host "killing old baize-embed pid: $oldPid"
    Stop-Process -Id $oldPid -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 800
}

# 2) spawn new server via WMI (survives this script's lifetime)
$r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{
    CommandLine      = "python server.py"
    CurrentDirectory = $dir
}
if ($r.ReturnValue -ne 0) {
    Write-Host "spawn FAILED, code=$($r.ReturnValue)"
    exit 1
}
Write-Host "spawned pid: $($r.ProcessId)"

# 3) verify health (model load takes ~3-5s)
$ok = $false
for ($i = 0; $i -lt 20 -and -not $ok; $i++) {
    Start-Sleep -Milliseconds 500
    try {
        $h = Invoke-RestMethod "http://127.0.0.1:8800/health" -TimeoutSec 3
        if ($h.ok) { $ok = $true; Write-Host "health OK: dim=$($h.dim) engine=$($h.engine.model)" }
    } catch { }
}
if (-not $ok) { Write-Host "health check FAILED (no ok /health after 10s)"; exit 1 }
