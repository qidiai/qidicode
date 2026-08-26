# ensure_services.ps1 - self-healing launcher for qidi's local services.
# Called by 启动qidi.bat before qidi.exe starts; also safe to run manually.
#
# Services managed:
#   8765  QiDi Relay (multi-account LAN proxy)  -> G:\程序开发\本地局域网中转-v3
#   8766  websearch shim (Responses->chat + real web search) -> .qidi\(services|tmp)\websearch-shim
#
# WHY WMI SPAWN: processes started via Start-Process from a job-object context
# (e.g. agent terminal tools) die when the job ends. Win32_Process.Create parents
# the child to WmiPrvSE.exe, so it survives any caller. Verified 2026-08-26.
#
# NOTE: this file contains a Chinese path; keep it saved as UTF-8 WITH BOM
# (re-encode after editing, else PowerShell 5.1 misreads it as GBK).

$ErrorActionPreference = "Continue"
$qidiHome = (Resolve-Path $PSScriptRoot).Path            # .qidi (absolute)
$root = Split-Path -Parent $qidiHome                     # workspace root
$relayDir = "G:\程序开发\本地局域网中转-v3"

function Test-PortUp([int]$port) {
    try { $c = New-Object Net.Sockets.TcpClient; $c.Connect("127.0.0.1", $port); $c.Close(); return $true }
    catch { return $false }
}

function Spawn-Detached([string]$cmd, [string]$dir) {
    if (-not (Test-Path $dir)) { Write-Output "  [spawn] dir not found: $dir"; return $false }
    $r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{
        CommandLine = $cmd; CurrentDirectory = $dir }
    if ($r.ReturnValue -eq 0) { Write-Output "  [spawn] ok pid=$($r.ProcessId)" }
    else { Write-Output "  [spawn] FAILED code=$($r.ReturnValue)" }
}

function Wait-Port([int]$port, [int]$secs) {
    for ($i = 0; $i -lt ($secs * 4); $i++) {
        if (Test-PortUp $port) { return $true }
        Start-Sleep -Milliseconds 250
    }
    return $false
}

# ---- 1) QiDi Relay on 8765 (upstream of everything) ----
Write-Output "[8765] QiDi Relay ..."
if (Test-PortUp 8765) {
    Write-Output "  already up"
} else {
    Write-Output "  down -> starting in $relayDir"
    Spawn-Detached "py -3 main.py" $relayDir
    if (Wait-Port 8765 20) { Write-Output "  started ok" }
    else { Write-Output "  START FAILED (relay not listening after 20s)" }
}

# ---- 2) websearch shim on 8766 ----
Write-Output "[8766] websearch shim ..."
$shimUp = $false
try {
    $null = Invoke-WebRequest -Uri "http://127.0.0.1:8766/health" -TimeoutSec 2 -UseBasicParsing
    $shimUp = $true
} catch { $shimUp = $false }

if ($shimUp) {
    Write-Output "  already up"
} else {
    # resolve shim dir: prefer services/ (post-migration), fall back to tmp/
    $shimDir = Join-Path $qidiHome "services\websearch-shim"
    if (-not (Test-Path (Join-Path $shimDir "shim.py"))) {
        $shimDir = Join-Path $qidiHome "tmp\websearch-shim"
    }
    Write-Output "  down -> starting in $shimDir"
    Spawn-Detached "python shim.py" $shimDir
    $ok = $false
    for ($i = 0; $i -lt 16 -and -not $ok; $i++) {
        Start-Sleep -Milliseconds 500
        try { $null = Invoke-WebRequest -Uri "http://127.0.0.1:8766/health" -TimeoutSec 2 -UseBasicParsing; $ok = $true } catch { }
    }
    if ($ok) { Write-Output "  started ok" } else { Write-Output "  START FAILED (no /health after 8s)" }
}

# ---- summary ----
$r8765 = if (Test-PortUp 8765) { "UP" } else { "DOWN" }
$r8766 = if (Test-PortUp 8766) { "UP" } else { "DOWN" }
Write-Output "RESULT relay(8765)=$r8765 shim(8766)=$r8766"
