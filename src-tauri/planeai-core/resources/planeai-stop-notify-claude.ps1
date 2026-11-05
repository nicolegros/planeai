# planeai stop-hook for Claude Code: notifies planeai when Claude needs attention.
# Installed by planeai. Safe to delete — notifications will fall back to silence detection.
$sid = $env:PLANEAI_SESSION_ID
if (-not $sid) { exit 0 }
$sock = if ($env:PLANEAI_SOCKET) { $env:PLANEAI_SOCKET } else { "\\.\pipe\planeai-notify" }
$input_text = [Console]::In.ReadToEnd()
$hook = $input_text | ConvertFrom-Json
$event = $hook.hook_event_name
$e = switch ($event) {
    "Stop" { "stop" }
    "UserPromptSubmit" { "busy" }
    "PostToolUse" { "busy" }
    "Notification" { "notification" }
    "StopFailure" { "notification" }
    default { exit 0 }  # unknown events must never alert
}
# The transcript tells planeai whether background subagents still run when a turn ends.
$fields = [ordered]@{ session_id = $sid; event = $e }
if ($hook.transcript_path -is [string]) { $fields.transcript_path = $hook.transcript_path }
if ($hook.notification_type -is [string]) { $fields.notification_type = $hook.notification_type }
$msg = $fields | ConvertTo-Json -Compress
$pipeName = $sock -replace '^\\\\.\\pipe\\',''
try {
    $pipe = New-Object System.IO.Pipes.NamedPipeClientStream(".", $pipeName, [System.IO.Pipes.PipeDirection]::Out)
    $pipe.Connect(1000)
    $writer = New-Object System.IO.StreamWriter($pipe)
    $writer.WriteLine($msg)
    $writer.Flush()
    $writer.Close()
    $pipe.Close()
} catch { }
exit 0
