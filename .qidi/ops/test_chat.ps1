$ErrorActionPreference = "SilentlyContinue"

$body = @{
    model = "codely-core"
    messages = @(@{ role = "user"; content = "say hi" })
    max_tokens = 10
} | ConvertTo-Json -Depth 5

try {
    $r = Invoke-WebRequest -Uri "http://127.0.0.1:8765/v1/chat/completions" `
        -Method POST `
        -Headers @{Authorization="Bearer sk-codely-proxy"; "Content-Type"="application/json"} `
        -Body $body `
        -TimeoutSec 20 -UseBasicParsing
    Write-Output "CHAT STATUS: $($r.StatusCode)"
    Write-Output $r.Content.Substring(0, [Math]::Min(400, $r.Content.Length))
} catch {
    Write-Output "CHAT FAIL: $($_.Exception.Message)"
    if ($_.Exception.Response) {
        $reader = New-Object System.IO.StreamReader($_.Exception.Response.GetResponseStream())
        Write-Output ("BODY: " + $reader.ReadToEnd())
        $reader.Close()
    }
}
