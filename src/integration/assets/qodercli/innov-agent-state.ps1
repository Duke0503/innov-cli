# installed by innov
# managed by innov; reinstalling or updating the integration overwrites this file.
# add custom hooks beside this file instead of editing it.
# INNOV_INTEGRATION_ID=qodercli
# INNOV_INTEGRATION_VERSION=3

param([string]$Action = "")

if ($Action -ne "session") { exit 0 }
if ($env:INNOV_ENV -ne "1") { exit 0 }
if ([string]::IsNullOrWhiteSpace($env:INNOV_PANE_ID)) { exit 0 }

$inputText = [Console]::In.ReadToEnd()
try {
    $payload = if ([string]::IsNullOrWhiteSpace($inputText)) { $null } else { $inputText | ConvertFrom-Json }
} catch {
    $payload = $null
}

if ($null -eq $payload -or [string]::IsNullOrWhiteSpace($payload.session_id)) { exit 0 }

$seq = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$innov = if ([string]::IsNullOrWhiteSpace($env:INNOV_BIN_PATH)) { "innov" } else { $env:INNOV_BIN_PATH }
try {
    & $innov pane report-agent-session $env:INNOV_PANE_ID --source herdr:qodercli --agent qodercli --agent-session-id $payload.session_id --seq $seq 2>$null | Out-Null
} catch {
}
