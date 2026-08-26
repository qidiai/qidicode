$ErrorActionPreference = "SilentlyContinue"

try {
    $r = Invoke-WebRequest -Uri "http://127.0.0.1:8765/v1/models" -Headers @{Authorization="Bearer sk-codely-proxy"} -TimeoutSec 10 -UseBasicParsing
    Write-Output "STATUS: $($r.StatusCode)"
    Write-Output $r.Content.Substring(0, [Math]::Min(300, $r.Content.Length))
} catch {
    Write-Output "MODELS FAIL: $($_.Exception.Message)"
}

Write-Output ""

try {
    $r2 = Invoke-WebRequest -Uri "http://127.0.0.1:8766/v1/models" -Headers @{Authorization="Bearer sk-codely-proxy"} -TimeoutSec 10 -UseBasicParsing
    Write-Output "SHIM STATUS: $($r2.StatusCode)"
    Write-Output $r2.Content.Substring(0, [Math]::Min(200, $r2.Content.Length))
} catch {
    Write-Output "SHIM FAIL: $($_.Exception.Message)"
}
