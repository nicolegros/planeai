; NSIS installer hooks for planeai.
; Kill detached sidecar processes before copying files so the installer can
; overwrite their binaries. Without this, Windows file-locking prevents
; the NSIS File command from writing planeai-daemon.exe and planeai-cli.exe,
; producing "error opening file for writing" during upgrades/reinstalls.

!macro NSIS_HOOK_PREINSTALL
  ; Kill planeai-daemon.exe (runs as a detached background process)
  nsExec::ExecToLog 'taskkill /F /IM "planeai-daemon.exe"'
  ; Kill planeai-cli.exe (another sidecar binary)
  nsExec::ExecToLog 'taskkill /F /IM "planeai-cli.exe"'
  ; Brief pause to let the OS release file handles
  Sleep 500
!macroend
