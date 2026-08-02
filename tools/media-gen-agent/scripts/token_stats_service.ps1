<#
.SYNOPSIS
Local token usage proxy + stats service.
Listens on 127.0.0.1:9800, forwards requests to the real model API,
and logs token usage per model/session.

Usage:
  .\token_stats_service.ps1 -UpstreamBaseUrl "https://api.deepseek.com" -UpstreamApiKey "sk-..."
#>
[CmdletBinding()]
param(
    [string]$UpstreamBaseUrl = "",
    [string]$UpstreamApiKey = "",
    [string]$ListenHost = "127.0.0.1",
    [int]$ListenPort = 9800,
    [string]$DbPath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$PSScriptRoot = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
if (-not $DbPath) { $DbPath = Join-Path $PSScriptRoot "token_stats.db" }

# ---- SQLite setup ----
$null = [System.Reflection.Assembly]::LoadWithPartialName("System.Data.SQLite")
$connStr = "Data Source=$DbPath;Version=3;"
$conn = New-Object System.Data.SQLite.SQLiteConnection($connStr)
$conn.Open()
$cmd = $conn.CreateCommand()
$cmd.CommandText = @"
CREATE TABLE IF NOT EXISTS requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts TEXT NOT NULL,
    session_id TEXT,
    model TEXT,
    prompt_tokens INTEGER,
    completion_tokens INTEGER,
    total_tokens INTEGER,
    status INTEGER,
    upstream_ms INTEGER,
    endpoint TEXT,
    error TEXT
);
"@
$cmd.ExecuteNonQuery() | Out-Null
$conn.Close()

function Save-Request {
    param(
        [string]$SessionId,
        [string]$Model,
        [string]$PromptTokens,
        [string]$CompletionTokens,
        [string]$TotalTokens,
        [int]$Status,
        [long]$UpstreamMs,
        [string]$Endpoint,
        [string]$Error
    )
    $c = New-Object System.Data.SQLite.SQLiteConnection($connStr)
    $c.Open()
    $cmd = $c.CreateCommand()
    $cmd.CommandText = @"
INSERT INTO requests (ts, session_id, model, prompt_tokens, completion_tokens, total_tokens, status, upstream_ms, endpoint, error)
VALUES (@ts, @session_id, @model, @prompt_tokens, @completion_tokens, @total_tokens, @status, @upstream_ms, @endpoint, @error);
"@
    $cmd.Parameters.AddWithValue("@ts", (Get-Date -Format "o")) | Out-Null
    $cmd.Parameters.AddWithValue("@session_id", $SessionId) | Out-Null
    $cmd.Parameters.AddWithValue("@model", $Model) | Out-Null
    $cmd.Parameters.AddWithValue("@prompt_tokens", $PromptTokens) | Out-Null
    $cmd.Parameters.AddWithValue("@completion_tokens", $CompletionTokens) | Out-Null
    $cmd.Parameters.AddWithValue("@total_tokens", $TotalTokens) | Out-Null
    $cmd.Parameters.AddWithValue("@status", $Status) | Out-Null
    $cmd.Parameters.AddWithValue("@upstream_ms", $UpstreamMs) | Out-Null
    $cmd.Parameters.AddWithValue("@endpoint", $Endpoint) | Out-Null
    $cmd.Parameters.AddWithValue("@error", $Error) | Out-Null
    $cmd.ExecuteNonQuery() | Out-Null
    $c.Close()
}

function Get-Stats {
    param([string]$Since)
    $c = New-Object System.Data.SQLite.SQLiteConnection($connStr)
    $c.Open()
    $cmd = $c.CreateCommand()
    if ($Since) {
        $cmd.CommandText = "SELECT model, COUNT(*) as cnt, COALESCE(SUM(prompt_tokens),0) as prompt_tokens, COALESCE(SUM(completion_tokens),0) as completion_tokens, COALESCE(SUM(total_tokens),0) as total_tokens, COALESCE(AVG(upstream_ms),0) as avg_ms FROM requests WHERE ts >= @since GROUP BY model ORDER BY total_tokens DESC;"
        $cmd.Parameters.AddWithValue("@since", $Since) | Out-Null
    } else {
        $cmd.CommandText = "SELECT model, COUNT(*) as cnt, COALESCE(SUM(prompt_tokens),0) as prompt_tokens, COALESCE(SUM(completion_tokens),0) as completion_tokens, COALESCE(SUM(total_tokens),0) as total_tokens, COALESCE(AVG(upstream_ms),0) as avg_ms FROM requests GROUP BY model ORDER BY total_tokens DESC;"
    }
    $reader = $cmd.ExecuteReader()
    $rows = @(); while ($reader.Read()) { $rows += [PSCustomObject]@{ model = $reader.GetString(0); cnt = $reader.GetInt32(1); prompt_tokens = $reader.GetInt64(2); completion_tokens = $reader.GetInt64(3); total_tokens = $reader.GetInt64(4); avg_ms = [long]$reader.GetDouble(5) } }
    $reader.Close(); $c.Close(); return $rows
}

# ---- HTTP listener ----
$listener = New-Object System.Net.HttpListener
$listener.Prefixes.Add("http://$ListenHost`:$ListenPort/")
$listener.Start()
Write-Host ""
Write-Host "=== Token Stats Service ===" -ForegroundColor Cyan
Write-Host "Listening: http://$ListenHost`:$ListenPort"
Write-Host "DB: $DbPath"
if ($UpstreamBaseUrl) { Write-Host "Upstream: $UpstreamBaseUrl" } else { Write-Host "Upstream: (passthrough mode, no forwarding)" }
Write-Host ""
Write-Host "Endpoints:" -ForegroundColor Yellow
Write-Host "  POST /v1/chat/completions  -> forward + log token usage"
Write-Host "  GET  /stats                -> JSON token stats"
Write-Host "  GET  /stats?since=ISO8601   -> stats since date"
Write-Host ""

while ($true) {
    $ctx = $listener.GetContext()
    $req = $ctx.Request
    $res = $ctx.Response
    try {
        $path = $req.Url.AbsolutePath
        if ($path -eq "/favicon.ico") { $res.StatusCode = 404; $res.Close(); continue }

        if ($path -eq "/stats" -and $req.HttpMethod -eq "GET") {
            $since = $req.Url.Query -replace '^\?since=', '' -replace '^&since=', ''
            $rows = Get-Stats -Since $since
            $payload = @{ rows = $rows } | ConvertTo-Json -Compress
            $buf = [System.Text.Encoding]::UTF8.GetBytes($payload)
            $res.ContentType = "application/json"
            $res.ContentLength64 = $buf.Length
            $res.OutputStream.Write($buf, 0, $buf.Length)
            $res.StatusCode = 200
            $res.Close()
            continue
        }

        if ($path -eq "/v1/chat/completions" -and $req.HttpMethod -eq "POST") {
            $body = ""
            if ($req.HasEntityBody) {
                $sr = New-Object System.IO.StreamReader($req.InputStream, $req.ContentEncoding)
                $body = $sr.ReadToEnd(); $sr.Close()
            }
            $sessionId = $req.Headers["X-Session-Id"]
            $modelHeader = $req.Headers["X-Model-Id"]

            if (-not $UpstreamBaseUrl) {
                $res.StatusCode = 500
                $out = [System.Text.Encoding]::UTF8.GetBytes('{"error":"no upstream configured"}')
                $res.ContentType = "application/json"; $res.ContentLength64 = $out.Length; $res.OutputStream.Write($out, 0, $out.Length); $res.Close(); continue
            }

            $upstreamUrl = $UpstreamBaseUrl.TrimEnd("/") + "/chat/completions"
            $headers = @{ "Content-Type" = "application/json" }
            if ($UpstreamApiKey) { $headers["Authorization"] = "Bearer $UpstreamApiKey" }
            $t0 = [DateTime]::UtcNow
            try {
                $resp = Invoke-RestMethod -Uri $upstreamUrl -Method POST -ContentType "application/json" -Body $body -Headers $headers
                $t1 = [DateTime]::UtcNow
                $upstreamMs = ($t1 - $t0).TotalMilliseconds
                $usage = $resp.usage
                $model = if ($modelHeader) { $modelHeader } elseif ($resp.model) { $resp.model } else { "" }
                Save-Request -SessionId $sessionId -Model $model `
                    -PromptTokens (if ($usage) { $usage.prompt_tokens } else { "" }) `
                    -CompletionTokens (if ($usage) { $usage.completion_tokens } else { "" }) `
                    -TotalTokens (if ($usage) { $usage.total_tokens } else { "" }) `
                    -Status 200 -UpstreamMs ([long]$upstreamMs) -Endpoint $path -Error ""
                $payload = $resp | ConvertTo-Json -Depth 10 -Compress
                $out = [System.Text.Encoding]::UTF8.GetBytes($payload)
                $res.ContentType = "application/json"; $res.ContentLength64 = $out.Length; $res.StatusCode = 200; $res.OutputStream.Write($out, 0, $out.Length); $res.Close()
            } catch {
                $t1 = [DateTime]::UtcNow
                Save-Request -SessionId $sessionId -Model $modelHeader -PromptTokens "" -CompletionTokens "" -TotalTokens "" -Status 500 -UpstreamMs ([long](($t1 - $t0).TotalMilliseconds)) -Endpoint $path -Error $_.Exception.Message
                $err = @{ error = $_.Exception.Message } | ConvertTo-Json
                $out = [System.Text.Encoding]::UTF8.GetBytes($err)
                $res.ContentType = "application/json"; $res.ContentLength64 = $out.Length; $res.StatusCode = 500; $res.OutputStream.Write($out, 0, $out.Length); $res.Close()
            }
            continue
        }

        $res.StatusCode = 404
        $out = [System.Text.Encoding]::UTF8.GetBytes('{"error":"not found"}')
        $res.ContentType = "application/json"; $res.ContentLength64 = $out.Length; $res.OutputStream.Write($out, 0, $out.Length); $res.Close()
    } catch {
        try { $res.StatusCode = 500; $res.Close() } catch {}
    }
}
