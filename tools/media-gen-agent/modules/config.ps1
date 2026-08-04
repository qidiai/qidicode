<#
config.ps1 - Config loading / multi-key resolution / failover helpers.
Dot-sourced by scripts/media_system.ps1, sharing its script-scope variables
($projectRoot, $configPath, $keyName). agnes_config.json format is unchanged:
keys[] array + default_key_name + polling timeout settings.
#>

function Get-MediaConfig {
    param(
        [Parameter(Mandatory=$true)][string]$ConfigPath
    )
    if (-not (Test-Path $ConfigPath)) { Write-Error "Config not found: $ConfigPath"; exit 1 }
    $cfg = Get-Content $ConfigPath -Raw | ConvertFrom-Json
    return $cfg
}

function Resolve-KeyName {
    param(
        [Parameter(Mandatory=$true)][object]$Config,
        [string]$KeyName
    )
    if (-not $KeyName) { return $Config.default_key_name }
    return $KeyName
}

function Get-ApiKeyEntry {
    param(
        [Parameter(Mandatory=$true)][object]$Config,
        [Parameter(Mandatory=$true)][string]$KeyName
    )
    $entry = $Config.keys | Where-Object { $_.name -eq $KeyName }
    if (-not $entry) { Write-Error "Key not found in config: $KeyName"; exit 1 }
    return $entry
}

function Get-ValidApiKeys {
    param(
        [Parameter(Mandatory=$true)][object]$Config
    )
    return @($Config.keys | Where-Object { $_.api_key -and $_.api_key.Trim() -ne '' })
}

function Get-FailoverStartIndex {
    param(
        [Parameter(Mandatory=$true)][object[]]$Keys,
        [Parameter(Mandatory=$true)][string]$KeyName
    )
    for ($i = 0; $i -lt $Keys.Count; $i++) {
        if ($Keys[$i].name -eq $KeyName) { return $i }
    }
    return 0
}
