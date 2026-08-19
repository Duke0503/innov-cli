# installed by innov
# managed by innov; reinstalling or updating the integration overwrites this file.
# add custom hooks beside this file instead of editing it.
# INNOV_INTEGRATION_ID=codex
# INNOV_INTEGRATION_VERSION=8

param([string]$Action = "")

if ($Action -ne "session") { exit 0 }
if ($env:INNOV_ENV -ne "1") { exit 0 }
if ([string]::IsNullOrWhiteSpace($env:INNOV_PANE_ID)) { exit 0 }

$inputText = [Console]::In.ReadToEnd()
try {
    $payload = if ([string]::IsNullOrWhiteSpace($inputText)) { $null } else { $inputText | ConvertFrom-Json }
} catch {
    exit 0
}

if ($payload.hook_event_name -and $payload.hook_event_name -ne "SessionStart") { exit 0 }

$sessionId = $payload.session_id
if ([string]::IsNullOrWhiteSpace($sessionId)) { exit 0 }
if ([string]::IsNullOrWhiteSpace($payload.transcript_path)) { exit 0 }
if (-not [string]::IsNullOrWhiteSpace($env:CODEX_THREAD_ID) -and $env:CODEX_THREAD_ID -ne $sessionId) { exit 0 }

$seq = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$innov = if ([string]::IsNullOrWhiteSpace($env:INNOV_BIN_PATH)) { "innov" } else { $env:INNOV_BIN_PATH }
try {
    $args = @(
        "pane",
        "report-agent-session",
        $env:INNOV_PANE_ID,
        "--source",
        "herdr:codex",
        "--agent",
        "codex",
        "--seq",
        "$seq",
        "--agent-session-id",
        "$sessionId"
    )
    if ($payload.hook_event_name -eq "SessionStart" -and $payload.source -is [string] -and -not [string]::IsNullOrWhiteSpace($payload.source)) {
        $args += @("--session-start-source", "$($payload.source)")
    }
    & $innov @args 2>$null | Out-Null
} catch {
}
