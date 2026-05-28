# planeai stop-hook: notifies planeai when kiro finishes a turn.
# Installed by planeai. Safe to delete — notifications will fall back to silence detection.
$input_text = [Console]::In.ReadToEnd()
$event = ($input_text | ConvertFrom-Json).hook_event_name
if ($event -ne "stop") { exit 0 }
$sid = if ($env:PLANEAI_SESSION_ID) { $env:PLANEAI_SESSION_ID } else { "" }
if (-not $sid) { exit 0 }
$pipeName = "planeai-notify"
try {
    $pipe = New-Object System.IO.Pipes.NamedPipeClientStream(".", $pipeName, [System.IO.Pipes.PipeDirection]::Out)
    $pipe.Connect(1000)
    $writer = New-Object System.IO.StreamWriter($pipe)
    $writer.WriteLine($sid)
    $writer.Flush()
    $writer.Close()
    $pipe.Close()
} catch { }
exit 0
