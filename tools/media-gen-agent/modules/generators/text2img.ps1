<#
generators/text2img.ps1 - text2img mode: POST /images/generations, save url or base64 output.
Returns $true when a DryRun finished the case (entry script then breaks the switch),
$false after a real run.
Depends on script-scope: $imageModel, $entry, $keyName, $globalTimer.
#>

function Invoke-Text2Img {
    param(
        [Parameter(Mandatory=$true)][string]$Prompt,
        [switch]$DryRun
    )
    if (-not $Prompt) { Write-Error "Missing -Prompt"; exit 1 }
    if ($Prompt.Trim() -eq "") { Write-Error "-Prompt must not be empty"; exit 1 }
    $body = @{
        model  = $imageModel
        prompt = $Prompt
        size   = "1024x1024"
        n      = 1
    } | ConvertTo-Json
    if ($DryRun) {
        $outPath = Get-OutputImagePath -Prefix "text2img"
        Write-Host "[DryRun] Endpoint: $($entry.base_url)/images/generations" -ForegroundColor Cyan
        Write-Host "[DryRun] Body: $($body.Substring(0, [Math]::Min(200, $body.Length)))..." -ForegroundColor DarkGray
        Write-Host "[DryRun] Output: $outPath"
        $globalTimer.Stop()
        Write-GenerationLog -Mode "text2img" -Prompt $Prompt -Duration "N/A" -Resolution "1024x1024" -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
        return $true
    }
    Write-Host "Creating image..." -ForegroundColor Green
    $resp = Invoke-ApiWithRetry -Action { Invoke-AgnesRest -Method POST -Endpoint "/images/generations" -Body $body } -Label "POST /images/generations"
    $item  = $resp.data[0]
    $imgUrl = $item.url
    $b64   = $item.b64_json
    if (-not $imgUrl -and -not $b64) { Write-Error "No image url or base64 returned"; exit 1 }
    if ($imgUrl) {
        Write-Host "Image URL: $imgUrl" -ForegroundColor Green
        $outPath = Get-OutputImagePath -Prefix "text2img"
        Invoke-WithRetry -Action { Invoke-WebRequest -Uri $imgUrl -OutFile $outPath }
        Write-Host "Saved to: $outPath"
    }
    if ($b64) {
        $bytes   = [Convert]::FromBase64String($b64)
        $outPath = Get-OutputImagePath -Prefix "text2img"
        [IO.File]::WriteAllBytes($outPath, $bytes)
        Write-Host "Saved to: $outPath"
    }
    $globalTimer.Stop()
    Write-GenerationLog -Mode "text2img" -Prompt $Prompt -Duration "N/A" -Resolution "1024x1024" -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
    return $false
}
