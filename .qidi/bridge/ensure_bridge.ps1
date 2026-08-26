# ensure_bridge.ps1 - start the local cloud-relay bridge on 127.0.0.1:8790 if not running.
# Called by the qidi .bat launchers before qidi.exe starts.
$port = 8790
$scriptDir = $PSScriptRoot

function Test-BridgePort {
    try {
        $c = New-Object Net.Sockets.TcpClient
        $c.Connect('127.0.0.1', $port)
        $c.Close()
        return $true
    } catch { return $false }
}

$ok = Test-BridgePort
if (-not $ok) {
    $py = 'py'
    if (-not (Get-Command py -ErrorAction SilentlyContinue)) { $py = 'python' }
    Start-Process -FilePath $py -ArgumentList @('-3', (Join-Path $scriptDir 'relay_bridge.py')) `
        -WindowStyle Hidden `
        -RedirectStandardOutput (Join-Path $scriptDir 'bridge.log') `
        -RedirectStandardError (Join-Path $scriptDir 'bridge.err.log') | Out-Null
    for ($i = 0; $i -lt 24 -and -not $ok; $i++) {
        Start-Sleep -Milliseconds 250
        $ok = Test-BridgePort
    }
}

if ($ok) { Write-Output 'bridge-ready' } else { Write-Output 'bridge-FAILED' }
