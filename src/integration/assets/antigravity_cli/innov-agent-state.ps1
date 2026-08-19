# installed by innov
# managed by innov; reinstalling or updating the integration overwrites this file.
# add custom hooks beside this file instead of editing it.
# INNOV_INTEGRATION_ID=antigravity_cli
# INNOV_INTEGRATION_VERSION=2

# Session-only: this hook reports the Antigravity conversation so Innov can
# resume the pane. Lifecycle state comes from Innov's screen detection.

param([string]$Action = "")

# Antigravity CLI expects a JSON object on stdout and this hook never injects
# anything, so every exit path emits an empty object.
function Exit-Hook {
    Write-Output "{}"
    exit 0
}

if ($Action -ne "session") { Exit-Hook }
if ($env:INNOV_ENV -ne "1") { Exit-Hook }
if ([string]::IsNullOrWhiteSpace($env:INNOV_PANE_ID)) { Exit-Hook }

$inputText = [Console]::In.ReadToEnd()
try {
    $payload = if ([string]::IsNullOrWhiteSpace($inputText)) { $null } else { $inputText | ConvertFrom-Json }
} catch {
    Exit-Hook
}

if ($null -eq $payload) { Exit-Hook }

$conversationId = if ($payload.conversationId -is [string]) { $payload.conversationId } else { $null }
if ([string]::IsNullOrWhiteSpace($conversationId)) { Exit-Hook }

$seq = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$innov = if ([string]::IsNullOrWhiteSpace($env:INNOV_BIN_PATH)) { "innov" } else { $env:INNOV_BIN_PATH }
try {
    $sessionArgs = @(
        "pane",
        "report-agent-session",
        $env:INNOV_PANE_ID,
        "--source",
        "herdr:antigravity_cli",
        "--agent",
        "agy",
        "--seq",
        "$seq",
        "--agent-session-id",
        "$conversationId"
    )
    if ($payload.transcriptPath -is [string] -and -not [string]::IsNullOrWhiteSpace($payload.transcriptPath)) {
        $sessionArgs += @("--agent-session-path", "$($payload.transcriptPath)")
    }
    & $innov @sessionArgs 2>$null | Out-Null
} catch {
}

Exit-Hook
