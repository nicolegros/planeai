#!/bin/bash
# planeai hook for Codex CLI: notifies planeai when Codex is busy or needs attention.
# Installed by planeai. Safe to delete - notifications will fall back to silence detection.
# Must print nothing: Codex reads hook stdout as a decision for PermissionRequest.
SESSION_ID="${PLANEAI_SESSION_ID:-$(tmux show-environment PLANEAI_SESSION_ID 2>/dev/null | cut -d= -f2)}"
[ -z "$SESSION_ID" ] && exit 0
SOCK="${PLANEAI_SOCKET:-$HOME/Library/Application Support/ca.nicolegros.planeai/notify.sock}"
case "$1" in
  stop) E="stop" ;;
  busy) E="busy" ;;
  notification) E="notification" ;;
  *)    exit 0 ;;  # unknown events must never alert
esac
[ -S "$SOCK" ] && printf '{"session_id":"%s","event":"%s"}\n' "$SESSION_ID" "$E" | nc -U "$SOCK" -w1 >/dev/null 2>&1
exit 0
