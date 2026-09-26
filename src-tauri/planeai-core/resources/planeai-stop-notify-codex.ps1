# planeai stop-hook for Codex CLI: notifies planeai when Codex is busy or needs attention.
# Installed by planeai. Safe to delete - notifications will fall back to silence detection.
# Must print nothing: Codex reads hook stdout as a decision for PermissionRequest.
$sid = $env:PLANEAI_SESSION_ID
if (-not $sid) { exit 0 }
$sock = if ($env:PLANEAI_SOCKET) { $env:PLANEAI_SOCKET } else { "\\.\pipe\planeai-notify" }
$e = switch ($args[0]) {
    "stop" { "stop" }
    "busy" { "busy" }
    default { "notification" }
}
$msg = '{"session_id":"' + $sid + '","event":"' + $e + '"}'
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
