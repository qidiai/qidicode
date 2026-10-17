# wss_test.ps1 - public wss + ACP handshake E2E check (bin/remote deploy kit)
# Usage: powershell -File wss_test.ps1 [-Url wss://ba.qidiai.ltd/ws]
# Secret: %USERPROFILE%\.qidi\remote_secret.txt (single line, shared with qidi_serve.bat)
# NOTE: keep this file pure ASCII - UTF-8 Chinese comments break PowerShell 5.1
#       on GBK consoles (multi-byte sequences can eat newlines inside param blocks).
param(
    [string]$Url = "wss://ba.qidiai.ltd/ws"
)
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13

$secretFile = Join-Path $env:USERPROFILE ".qidi\remote_secret.txt"
if (-not (Test-Path $secretFile)) { Write-Output "[ERROR] missing $secretFile"; exit 1 }
$secret = (Get-Content $secretFile -Raw).Trim()

$uri = [Uri]"$($Url)?server-key=$secret"
Write-Output "Connect: $uri"

$ws = New-Object System.Net.WebSockets.ClientWebSocket
$ws.ConnectAsync($uri, [System.Threading.CancellationToken]::None).Wait()
Write-Output "WS_STATE: $($ws.State)"

$msg = '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{}}}'
$bytes = [Text.Encoding]::UTF8.GetBytes($msg)
$ws.SendAsync([ArraySegment[byte]]::new($bytes), [System.Net.WebSockets.WebSocketMessageType]::Text, $true, [System.Threading.CancellationToken]::None).Wait()
Write-Output "SENT initialize"

$buf = New-Object byte[] 262144
$task = $ws.ReceiveAsync([ArraySegment[byte]]::new($buf), [System.Threading.CancellationToken]::None)
if ($task.Wait(15000)) {
    $txt = [Text.Encoding]::UTF8.GetString($buf, 0, $task.Result.Count)
    Write-Output ("RECV: " + $txt.Substring(0, [Math]::Min(300, $txt.Length)))
    if ($txt -match 'agentCapabilities') { Write-Output "E2E_RESULT: PASS" } else { Write-Output "E2E_RESULT: NO_CAPABILITIES" }
} else {
    Write-Output "E2E_RESULT: TIMEOUT"
}
$ws.Dispose()
