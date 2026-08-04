<#
output.ps1 - Output paths / idempotent directory creation / file format helpers (MIME).
Dot-sourced by scripts/media_system.ps1.
Depends on script-scope variable: $projectRoot (set by the entry script).
All outputs land under output/images/ and output/videos/; directories are created
idempotently with -Force.
#>

function Get-OutputImagePath {
    param([string]$Prefix)
    $ts   = Get-Date -Format 'yyyyMMdd_HHmmss'
    $rand = -join ((1..4) | ForEach-Object { '{0:X}' -f (Get-Random -Maximum 16) })
    $dir  = Join-Path $projectRoot "output\images"
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    return Join-Path $dir "${Prefix}_${ts}_$rand.png"
}

function Get-OutputVideoPath {
    param([string]$Prefix)
    $ts   = Get-Date -Format 'yyyyMMdd_HHmmss'
    $rand = -join ((1..4) | ForEach-Object { '{0:X}' -f (Get-Random -Maximum 16) })
    $dir  = Join-Path $projectRoot "output\videos"
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    return Join-Path $dir "${Prefix}_${ts}_$rand.mp4"
}

function Get-GenerationLogPath {
    $dir = Join-Path $projectRoot "output"
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    return Join-Path $dir "generation_log.jsonl"
}

function Get-MimeType {
    param([string]$Path)
    $ext = [IO.Path]::GetExtension($Path).ToLower()
    $map = @{
        ".png"  = "image/png"
        ".jpg"  = "image/jpeg"
        ".jpeg" = "image/jpeg"
        ".webp" = "image/webp"
        ".gif"  = "image/gif"
        ".bmp"  = "image/bmp"
    }
    if ($map.ContainsKey($ext)) { return $map[$ext] }
    return "image/jpeg"
}
