<#
.SYNOPSIS
Cleanup redundant media-gen-agent files after consolidation into scripts/media_system.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$targets = @(
    ".\image_system.ps1",
    ".\media_system.ps1",
    ".\video_system\video_system.ps1",
    ".\video_system\README.md",
    ".\video_system\examples\demo.ps1"
)

foreach ($t in $targets) {
    if (Test-Path $t) {
        Remove-Item $t -Force
        Write-Host "Deleted: $t"
    } else {
        Write-Host "Skipped (not found): $t"
    }
}

# Remove empty directories
$dirs = @(".\video_system\examples", ".\video_system")
foreach ($d in $dirs) {
    if (Test-Path $d) {
        Remove-Item $d -Force -Recurse
        Write-Host "Removed directory: $d"
    }
}

Write-Host ""
Write-Host "Cleanup complete."
