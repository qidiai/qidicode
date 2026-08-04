<#
logging.ps1 - Request log (structured console line) + generation log (output/generation_log.jsonl).
Dot-sourced by scripts/media_system.ps1.
Depends on script-scope variable: $keyName (set by the entry script).
#>

function Write-RequestLog {
    param(
        [string]$Label,
        [int]$Status,
        [long]$DurationMs
    )
    $ts = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
    Write-Host "[$ts] [$keyName] [$Label] [$Status] [${DurationMs}ms]"
}

function Write-GenerationLog {
    param(
        [string]$Mode,
        [string]$Prompt,
        [string]$Duration,
        [string]$Resolution,
        [string]$KeyName,
        [bool]$Success,
        [double]$ElapsedSec,
        [string]$OutputPath
    )
    $logPath = Get-GenerationLogPath
    $entry = [ordered]@{
        timestamp     = (Get-Date -Format "o")
        mode          = $Mode
        prompt        = $Prompt
        duration      = $Duration
        resolution    = $Resolution
        key_name      = $KeyName
        success       = $Success
        elapsed_sec   = [math]::Round($ElapsedSec, 2)
        output_path   = $OutputPath
    } | ConvertTo-Json -Compress
    Add-Content -Path $logPath -Value $entry -Encoding UTF8
}
