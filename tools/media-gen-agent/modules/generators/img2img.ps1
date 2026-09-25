<#
generators/img2img.ps1 - img2img mode: POST /images/generations with base64 data-uri image.
Returns $true when a DryRun finished the case, $false after a real run.
Depends on script-scope: $imageModel, $entry, $keyName, $globalTimer.
#>

function Invoke-Img2Img {
    param(
        [Parameter(Mandatory=$true)][string]$Prompt,
        [Parameter(Mandatory=$true)][string]$Image,
        [switch]$DryRun
    )
    if (-not $Image)  { Write-Error "Missing -Image"; exit 1 }
    if (-not (Test-Path $Image)) { Write-Error "Image not found: $Image"; exit 1 }
    if (-not $Prompt) { Write-Error "Missing -Prompt"; exit 1 }
    if ($Prompt.Trim() -eq "") { Write-Error "-Prompt must not be empty"; exit 1 }
    $abs = (Resolve-Path $Image).Path
    $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($abs))
    $mime = Get-MimeType -Path $abs
    $body = @{
        model    = $imageModel
        prompt   = $Prompt
        image    = "data:$mime;base64,$b64"
        size     = "1024x1024"
        n        = 1
    } | ConvertTo-Json
    if ($DryRun) {
        $outPath = Get-OutputImagePath -Prefix "img2img"
        Write-Host "[DryRun] Endpoint: $($entry.base_url)/images/generations" -ForegroundColor Cyan
        Write-Host "[DryRun] Body: $($body.Substring(0, [Math]::Min(200, $body.Length)))..." -ForegroundColor DarkGray
        Write-Host "[DryRun] Output: $outPath"
        $globalTimer.Stop()
        Write-GenerationLog -Mode "img2img" -Prompt $Prompt -Duration "N/A" -Resolution "1024x1024" -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
        return $true
    }
    Write-Host "Creating image from reference..." -ForegroundColor Green
    try {
        $resp = Invoke-ApiWithRetry -Action { Invoke-AgnesRest -Method POST -Endpoint "/images/generations" -Body $body } -Label "POST /images/generations"
    } catch {
        Write-Error (Write-FriendlyError -Exception $_.Exception)
    }
    $item    = $resp.data[0]
    $imgUrl  = $item.url
    $b64Out  = $item.b64_json
    if (-not $imgUrl -and -not $b64Out) { Write-Error "No image url or base64 returned"; exit 1 }
    if ($imgUrl) {
        Write-Host "Image URL: $imgUrl" -ForegroundColor Green
        $outPath = Get-OutputImagePath -Prefix "img2img"
        Invoke-WithRetry -Action { Invoke-WebRequest -Uri $imgUrl -OutFile $outPath }
        Write-Host "Saved to: $outPath"
    }
    if ($b64Out) {
        $bytes   = [Convert]::FromBase64String($b64Out)
        $outPath = Get-OutputImagePath -Prefix "img2img"
        [IO.File]::WriteAllBytes($outPath, $bytes)
        Write-Host "Saved to: $outPath"
    }
    $globalTimer.Stop()
    Write-GenerationLog -Mode "img2img" -Prompt $Prompt -Duration "N/A" -Resolution "1024x1024" -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
    return $false
}
