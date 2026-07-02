; NSIS installer hooks for planeai.
; Kill detached sidecar processes before copying files so the installer can
; overwrite their binaries. Without this, Windows file-locking prevents
; the NSIS File command from writing planeai-daemon.exe and planeai-cli.exe,
; producing "error opening file for writing" during upgrades/reinstalls.

!macro NSIS_HOOK_PREINSTALL
  ; Kill planeai-daemon.exe (runs as a detached background process).
  ; Use ExecToStack + Pop to suppress error output and normalize the exit
  ; code when the process isn't running (taskkill returns non-zero in that case).
  nsExec::ExecToStack 'taskkill /F /IM "planeai-daemon.exe"'
  Pop $0
  ; Kill planeai-cli.exe (another sidecar binary)
  nsExec::ExecToStack 'taskkill /F /IM "planeai-cli.exe"'
  Pop $0
  ; Brief pause to let the OS release file handles
  Sleep 500
!macroend
