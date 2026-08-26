# Pre-commit hook: Check for secrets before git commit
# Place this file at .git/hooks/pre-commit

Write-Host "[Pre-commit] Checking for secrets..." -ForegroundColor Cyan

$stagedFiles = git diff --cached --name-only
if (-not $stagedFiles) {
    Write-Host "[Pre-commit] No staged files." -ForegroundColor Gray
    exit 0
}

$secretsFound = $false
$errors = @()

# Patterns to detect (case-insensitive)
$patterns = @(
    @{ Name="OpenAI/Agnes Key"; Pattern='sk-[a-zA-Z0-9]{20,}' },
    @{ Name="AWS Access Key"; Pattern='AKIA[A-Z0-9]{16}' },
    @{ Name="Generic API Key"; Pattern='api[_-]?key\s*[:=]\s*["'']?[a-zA-Z0-9]{20,}["'']?' },
    @{ Name="JWT Token"; Pattern='eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+' },
    @{ Name="Private Key"; Pattern='-----BEGIN (RSA|EC|OPENSSH) PRIVATE KEY-----' },
    @{ Name="DeepSeek Key"; Pattern='sk-[a-zA-Z0-9]{20,}' },
    @{ Name="Password in config"; Pattern='password\s*[:=]\s*["'']?[^\s"'']{8,}["'']?' }
)

# Skip these extensions (templates/examples)
$skipExtensions = @('.example', '.template', '.sample', '.md')

foreach ($file in $stagedFiles) {
    # Check if file is a template/example
    $isSkip = $false
    foreach ($ext in $skipExtensions) {
        if ($file.EndsWith($ext)) { $isSkip = $true; break }
    }
    if ($isSkip) { continue }

    # Get the diff content for this file
    $diff = git diff --cached -- "$file"
    if (-not $diff) { continue }

    foreach ($p in $patterns) {
        $matches = [regex]::Matches($diff, $p.Pattern, 'IgnoreCase')
        foreach ($m in $matches) {
            # Skip obvious false positives (docs, examples)
            $line = $m.Value
            if ($line -match 'sk-\.\.\.' -or $line -match 'your-.*-here' -or $line -match 'CHANGE_ME') {
                continue
            }
            $errors += "  [!! $($p.Name)] in $file`:$($m.Index)"
            $errors += "    Match: $($m.Value.Substring(0, [Math]::Min(30, $m.Value.Length)))..."
            $secretsFound = $true
        }
    }
}

if ($secretsFound) {
    Write-Host ""
    Write-Host "[FAIL] Potential secrets detected!" -ForegroundColor Red
    Write-Host "Please remove or mask the following before committing:" -ForegroundColor Yellow
    $errors | ForEach-Object { Write-Host $_ }
    Write-Host ""
    Write-Host "To skip (use only if you're sure): git commit --no-verify" -ForegroundColor Gray
    exit 1
}

Write-Host "[Pre-commit] No secrets detected. Good to go!" -ForegroundColor Green
exit 0
