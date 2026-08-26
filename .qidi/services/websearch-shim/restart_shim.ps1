# Restart shim.py detached from the current terminal job object.
#
# WHY WMI: Start-Process children die with the calling command's job object
# (observed 2026-08-26: shim spawned via Start-Process vanished when the
# terminal command finished). WMI-created processes are parented by
# WmiPrvSE.exe and survive across commands.
#
# v2: kill any existing listener on 8766 FIRST, otherwise the new process
# fails to bind the port and exits silently (false "restart succeeded").

# $PSScriptRoot = the dir this script lives in (services/websearch-shim).
# Location-independent: works after the 2026-08-27 tmp -> services migration.
$dir = $PSScriptRoot

# 1) stop existing shim (whoever owns port 8766)
$conn = Get-NetTCPConnection -LocalPort 8766 -State Listen -ErrorAction SilentlyContinue
if ($conn) {
    $oldPid = ($conn | Select-Object -First 1).OwningProcess
    Write-Host "killing old shim pid: $oldPid"
    Stop-Process -Id $oldPid -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 800
}

# 2) spawn new shim via WMI (survives this script's lifetime)
$r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{
    CommandLine    = "python shim.py"
    CurrentDirectory = $dir
}
if ($r.ReturnValue -ne 0) {
    Write-Host "spawn FAILED, code=$($r.ReturnValue)"
    exit 1
}
Write-Host "spawned pid: $($r.ProcessId)"

# 3) verify health
Start-Sleep -Seconds 3
try {
    $h = Invoke-RestMethod "http://127.0.0.1:8766/health" -TimeoutSec 5
    Write-Host "health OK: version=$($h.version) pid=$($h.pid)"
} catch {
    Write-Host "health check FAILED: $($_.Exception.Message)"
    exit 1
}
