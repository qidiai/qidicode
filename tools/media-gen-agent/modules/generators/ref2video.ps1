<#
generators/ref2video.ps1 - ref2video mode: POST /videos with >=2 base64 reference images, poll, download.
Returns $true when a DryRun finished the case, $false after a real run.
Depends on script-scope: $videoModel, $entry, $keyName, $globalTimer.
#>

function Invoke-Ref2Video {
    param(
        [Parameter(Mandatory=$true)][string]$Prompt,
        [Parameter(Mandatory=$true)][string[]]$Images,
        [int]$Duration = 6,
        [string]$Resolution = "480p",
        [string]$AspectRatio = "auto",
        [switch]$DryRun
    )
    if (-not $Images -or $Images.Count -lt 2) { Write-Error "Need at least 2 -Images"; exit 1 }
    if (-not $Prompt) { Write-Error "Missing -Prompt"; exit 1 }
    if ($Prompt.Trim() -eq "") { Write-Error "-Prompt must not be empty"; exit 1 }
    $abs = @(); foreach ($img in $Images) { if (-not (Test-Path $img)) { Write-Error "Image not found: $img"; exit 1 }; $abs += (Resolve-Path $img).Path }
    $refs = @(); foreach ($path in $abs) { $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($path)); $mime = Get-MimeType -Path $path; $refs += "data:$mime;base64,$b64" }
    $body = @{
        model            = $videoModel
        prompt           = $Prompt
        reference_images = $refs
        duration         = $Duration
        resolution       = $Resolution
        aspect_ratio     = $AspectRatio
    } | ConvertTo-Json
    if ($DryRun) {
        $outPath = Get-OutputVideoPath -Prefix "ref2video"
        Write-Host "[DryRun] Endpoint: $($entry.base_url)/videos" -ForegroundColor Cyan
        Write-Host "[DryRun] Body: $($body.Substring(0, [Math]::Min(200, $body.Length)))..." -ForegroundColor DarkGray
        Write-Host "[DryRun] Output: $outPath"
        $globalTimer.Stop()
        Write-GenerationLog -Mode "ref2video" -Prompt $Prompt -Duration $Duration -Resolution $Resolution -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
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
    $outPath = Get-OutputVideoPath -Prefix "ref2video"
    Save-RemoteFile -Uri $videoUrl -OutFile $outPath
    Write-Host "Saved to: $outPath"
    $globalTimer.Stop()
    Write-GenerationLog -Mode "ref2video" -Prompt $Prompt -Duration $Duration -Resolution $Resolution -KeyName $keyName -Success $true -ElapsedSec $globalTimer.Elapsed.TotalSeconds -OutputPath $outPath
    return $false
}
