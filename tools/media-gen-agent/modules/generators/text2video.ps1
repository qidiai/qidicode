<#
generators/text2video.ps1 - text2video mode: POST /videos, poll for completion, download mp4.
Returns $true when a DryRun finished the case, $false after a real run.
Depends on script-scope: $videoModel, $entry, $keyName, $globalTimer.
Polling logic lives in http_client.ps1 (Wait-VideoTask).
#>

function Invoke-Text2Video {
    param(
        [Parameter(Mandatory=$true)][string]$Prompt,
        [int]$Duration = 6,
        [string]$Resolution = "480p",
        [string]$AspectRatio = "auto",
        [switch]$DryRun
    )
    if (-not $Prompt) { Write-Error "Missing -Prompt"; exit 1 }
    if ($Prompt.Trim() -eq "") { Write-Error "-Prompt must not be empty"; exit 1 }
    $body = @{
        model        = $videoModel
        prompt       = $Prompt
        duration     = $Duration
        resolution   = $Resolution
        aspect_ratio = $AspectRatio
    } | ConvertTo-Json
    if ($DryRun) {
        $outPath = Get-OutputVideoPath -Prefix "text2video"
        Write-Host "[DryRun] Endpoint: $($entry.base_url)/videos" -ForegroundColor Cyan
        Write-Host "[DryRun] Body: $($body.Substring(0, [Math]::Min(200, $body.Length)))..." -ForegroundColor DarkGray
        Write-Host "[DryRun] Output: $outPath"
        $globalTimer.Stop()
        Write-GenerationLog -Mode "text2video" -Prompt $Prompt -Duration $Duration -Resolution $Resolution -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
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
    $outPath = Get-OutputVideoPath -Prefix "text2video"
    Save-RemoteFile -Uri $videoUrl -OutFile $outPath
    Write-Host "Saved to: $outPath"
    $globalTimer.Stop()
    Write-GenerationLog -Mode "text2video" -Prompt $Prompt -Duration $Duration -Resolution $Resolution -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
    return $false
}
