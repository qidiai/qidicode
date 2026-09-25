<#
.SYNOPSIS
Comprehensive tests for media_system.ps1.
#>
[CmdletBinding()]
param(
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$script:Failed = $false
$script:TestsPassed = 0
$script:TestsFailed = 0

function Test-Command {
    param([string]$Name, [scriptblock]$Script)
    Write-Host ""
    Write-Host "=== $Name ===" -ForegroundColor Cyan
    try {
        & $Script
        Write-Host "[PASS] $Name" -ForegroundColor Green
        $script:TestsPassed++
    } catch {
        Write-Host "[FAIL] $Name : $_" -ForegroundColor Red
        $script:TestsFailed = $true
        $script:Failed = $true
    }
}

if ($DryRun) {
    # Syntax and structural validation
    Test-Command "media_system.ps1 syntax" {
        $errors = $null
        $null = [System.Management.Automation.PSParser]::Tokenize((Get-Content ".\scripts\media_system.ps1" -Raw), [ref]$errors)
        if ($errors.Count -gt 0) { throw "Parse errors: $($errors | ForEach-Object { $_.ToString() })" }
    }

    Test-Command "agnes_config.json valid" {
        $null = Get-Content ".\agnes_config.json" -Raw | ConvertFrom-Json
    }

    Test-Command "key failover function exists" {
        $content = Get-Content ".\scripts\media_system.ps1" -Raw
        if ($content -notmatch "Invoke-AgnesRest") { throw "Invoke-AgnesRest not found" }
        if ($content -notmatch "Get-HttpStatusCode") { throw "Get-HttpStatusCode not found" }
        if ($content -notmatch "429.*503.*500") { throw "Failover status codes not found" }
    }

    Test-Command "API retry with exponential backoff" {
        $content = Get-Content ".\scripts\media_system.ps1" -Raw
        if ($content -notmatch "Invoke-ApiWithRetry") { throw "Invoke-ApiWithRetry not found" }
        if ($content -notmatch "Write-RequestLog") { throw "Write-RequestLog not found" }
        if ($content -notmatch "Write-GenerationLog") { throw "Write-GenerationLog not found" }
    }

    Test-Command "structured log format" {
        $content = Get-Content ".\scripts\media_system.ps1" -Raw
        if ($content -notmatch "\[timestamp\]") { throw "timestamp token not found" }
        if ($content -notmatch "\[key_name\]") { throw "key_name token not found" }
        if ($content -notmatch "\[endpoint\]") { throw "endpoint token not found" }
        if ($content -notmatch "\[status\]") { throw "status token not found" }
        if ($content -notmatch "\[duration_ms\]") { throw "duration_ms token not found" }
    }

    Test-Command "unified output directory" {
        $content = Get-Content ".\scripts\media_system.ps1" -Raw
        if ($content -notmatch 'output\\images') { throw "output/images path not found" }
        if ($content -notmatch 'output\\videos') { throw "output/videos path not found" }
        if ($content -match 'scripts\\images') { throw "Legacy scripts/images path still present" }
        if ($content -match 'scripts\\videos') { throw "Legacy scripts/videos path still present" }
    }

    Test-Command "DryRun parameter exists" {
        $content = Get-Content ".\scripts\media_system.ps1" -Raw
        if ($content -notmatch '\[switch\]\$DryRun') { throw "DryRun switch parameter not found" }
        if ($content -notmatch '\[DRY RUN\]') { throw "DryRun output marker not found" }
    }

    Test-Command "parameter validation exists" {
        $content = Get-Content ".\scripts\media_system.ps1" -Raw
        if ($content -notmatch 'Missing -Prompt') { throw "Missing -Prompt check not found" }
        if ($content -notmatch 'Missing -Image') { throw "Missing -Image check not found" }
        if ($content -notmatch 'Need at least 2 -Images') { throw "ref2video image count check not found" }
    }

    Test-Command "generation log jsonl exists" {
        $content = Get-Content ".\scripts\media_system.ps1" -Raw
        if ($content -notmatch 'generation_log.jsonl') { throw "generation_log.jsonl not found" }
        if ($content -notmatch 'Write-GenerationLog') { throw "Write-GenerationLog call not found" }
    }

    Test-Command "video poll timeout hint" {
        $content = Get-Content ".\scripts\media_system.ps1" -Raw
        if ($content -notmatch '\$retries % 10') { throw "Poll progress hint not found" }
    }

    Test-Command "img2video aspect_ratio included" {
        $content = Get-Content ".\scripts\media_system.ps1" -Raw
        $idx = $content.IndexOf('"img2video"')
        $segment = $content.Substring($idx, [Math]::Min(800, $content.Length - $idx))
        if ($segment -notmatch 'aspect_ratio') { throw "img2video missing aspect_ratio" }
    }

    Test-Command "ref2video aspect_ratio included" {
        $content = Get-Content ".\scripts\media_system.ps1" -Raw
        $idx = $content.IndexOf('"ref2video"')
        $segment = $content.Substring($idx, [Math]::Min(800, $content.Length - $idx))
        if ($segment -notmatch 'aspect_ratio') { throw "ref2video missing aspect_ratio" }
    }

    Test-Command "img2img mime detection exists" {
        $content = Get-Content ".\scripts\media_system.ps1" -Raw
        if ($content -notmatch 'Get-MimeType') { throw "Get-MimeType not found" }
    }

    Test-Command "DryRun text2img" {
        .\scripts\media_system.ps1 -DryRun -Mode text2img -Prompt "a red dot on white background, minimalist"
    }

    Test-Command "DryRun img2img" {
        .\scripts\media_system.ps1 -DryRun -Mode img2img -Image ".\scripts\images\text2img_20260731_161134.png" -Prompt "oil painting style"
    }

    Test-Command "DryRun img2video" {
        .\scripts\media_system.ps1 -DryRun -Mode img2video -Image ".\scripts\images\text2img_20260731_161134.png" -Prompt "slow zoom" -Duration 10 -Resolution 720p
    }

    Test-Command "DryRun text2video" {
        .\scripts\media_system.ps1 -DryRun -Mode text2video -Prompt "a cat on the beach at sunset" -Duration 10 -Resolution 720p
    }

    Test-Command "DryRun ref2video" {
        .\scripts\media_system.ps1 -DryRun -Mode ref2video -Images @(".\scripts\images\text2img_20260731_161134.png",".\scripts\images\text2img_20260731_161134.png") -Prompt "cinematic transition" -Duration 10 -Resolution 720p
    }
} else {
    # Live test (requires valid Agnes key)
    Test-Command "text2img" {
        .\scripts\media_system.ps1 -Mode text2img -Prompt "a red dot on white background, minimalist"
    }

    Test-Command "text2video" {
        .\scripts\media_system.ps1 -Mode text2video -Prompt "a cat on the beach at sunset" -Duration 6 -Resolution 480p
    }
}

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "Tests Passed: $script:TestsPassed" -ForegroundColor Green
if ($script:TestsFailed) {
    Write-Host "Tests FAILED." -ForegroundColor Red
    exit 1
} else {
    Write-Host "All tests PASSED." -ForegroundColor Green
}
