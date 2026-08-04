<#
error.ps1 - Error handling: convert API exceptions into friendly messages.
Dot-sourced by scripts/media_system.ps1.
#>

function Write-FriendlyError {
    param($Exception)
    $status = Get-HttpStatusCode -Exception $Exception
    if ($status -in @(400, 422)) {
        $msg = $Exception.ErrorDetails.Message
        if ($msg) {
            try {
                $parsed = $msg | ConvertFrom-Json
                if ($parsed.error.message) { return $parsed.error.message }
                if ($parsed.error) { return $parsed.error }
                return $msg
            } catch {
                return $msg
            }
        }
        return "API request failed (HTTP $status). Check prompt and parameters."
    }
    return $Exception.Message
}
