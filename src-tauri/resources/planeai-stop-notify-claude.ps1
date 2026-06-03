# planeai stop-hook for Claude Code: notifies planeai when Claude needs attention.
# Installed by planeai. Safe to delete — notifications will fall back to silence detection.
$sid = $env:PLANEAI_SESSION_ID
if (-not $sid) { exit 0 }
$sock = $env:PLANEAI_SOCKET
if (-not $sock) { exit 0 }
$input_text = [Console]::In.ReadToEnd()
$event = ($input_text | ConvertFrom-Json).hook_event_name
$e = switch ($event) {
    "Stop" { "stop" }
    "UserPromptSubmit" { "busy" }
    default { "notification" }
}
$msg = '{"session_id":"' + $sid + '","event":"' + $e + '"}'
try {
    $pipe = New-Object System.IO.Pipes.NamedPipeClientStream(".", ($sock -replace '^\\\\.\\pipe\\',''), [System.IO.Pipes.PipeDirection]::Out)
    $pipe.Connect(1000)
    $writer = New-Object System.IO.StreamWriter($pipe)
    $writer.WriteLine($msg)
    $writer.Flush()
    $writer.Close()
    $pipe.Close()
} catch { }
exit 0
