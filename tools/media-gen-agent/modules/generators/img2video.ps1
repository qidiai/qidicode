<#
generators/img2video.ps1 - img2video mode: POST /videos with base64 data-uri image, poll, download.
Returns $true when a DryRun finished the case, $false after a real run.
Depends on script-scope: $videoModel, $entry, $keyName, $globalTimer.
#>

function Invoke-Img2Video {
    param(
        [Parameter(Mandatory=$true)][string]$Prompt,
        [Parameter(Mandatory=$true)][string]$Image,
        [int]$Duration = 6,
        [string]$Resolution = "480p",
        [string]$AspectRatio = "auto",
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
        model        = $videoModel
        prompt       = $Prompt
        image        = "data:$mime;base64,$b64"
        duration     = $Duration
        resolution   = $Resolution
        aspect_ratio = $AspectRatio
    } | ConvertTo-Json
    if ($DryRun) {
        $outPath = Get-OutputVideoPath -Prefix "img2video"
        Write-Host "[DryRun] Endpoint: $($entry.base_url)/videos" -ForegroundColor Cyan
        Write-Host "[DryRun] Body: $($body.Substring(0, [Math]::Min(200, $body.Length)))..." -ForegroundColor DarkGray
        Write-Host "[DryRun] Output: $outPath"
        $globalTimer.Stop()
        Write-GenerationLog -Mode "img2video" -Prompt $Prompt -Duration $Duration -Resolution $Resolution -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
        return $true
    }
    Write-Host "Creating video task..." -ForegroundColor Green
    try {
        $resp = Invoke-ApiWithRetry -Action { Invoke-AgnesRest -Method POST -Endpoint "/videos" -Body $body } -Label "POST /videos"
    } catch {
        Write-Error (Write-FriendlyError -Exception $_.Exception)
    }
    $videoId = $resp.video_id
    if (-not $videoId) { $videoId = $resp.id }
    Write-Host "Task created: $videoId" -ForegroundColor Cyan
    $result = Wait-VideoTask -VideoId $videoId
    $videoUrl = $result.url
    Write-Host ""
    Write-Host "Video ready!" -ForegroundColor Green
    Write-Host "URL: $videoUrl"
    $outPath = Get-OutputVideoPath -Prefix "img2video"
    Save-RemoteFile -Uri $videoUrl -OutFile $outPath
    Write-Host "Saved to: $outPath"
    $globalTimer.Stop()
    Write-GenerationLog -Mode "img2video" -Prompt $Prompt -Duration $Duration -Resolution $Resolution -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
    return $false
}
