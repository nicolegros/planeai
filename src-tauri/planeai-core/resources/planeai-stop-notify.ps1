# planeai stop-hook: notifies planeai when kiro finishes a turn.
# Installed by planeai. Safe to delete — notifications will fall back to silence detection.
$input_text = [Console]::In.ReadToEnd()
$event = try { ($input_text | ConvertFrom-Json).hook_event_name } catch { "" }
# switch is case-insensitive: matches Kiro CLI v2 camelCase and v3 PascalCase
$e = switch ($event) {
    "stop" { "stop" }
    "userPromptSubmit" { "busy" }
    default { exit 0 }  # unknown events must never alert
}
$sid = if ($env:PLANEAI_SESSION_ID) { $env:PLANEAI_SESSION_ID } else { "" }
if (-not $sid) { exit 0 }
$sock = if ($env:PLANEAI_SOCKET) { $env:PLANEAI_SOCKET } else { "\\.\pipe\planeai-notify" }
$pipeName = $sock -replace '^\\\\.\\pipe\\',''
$msg = '{"session_id":"' + $sid + '","event":"' + $e + '"}'
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
