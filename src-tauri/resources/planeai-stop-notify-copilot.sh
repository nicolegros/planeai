#!/bin/bash
# planeai stop-hook for Copilot CLI: notifies planeai when Copilot needs attention.
# Installed by planeai. Safe to delete — notifications will fall back to silence detection.
SESSION_ID="${PLANEAI_SESSION_ID:-$(tmux show-environment PLANEAI_SESSION_ID 2>/dev/null | cut -d= -f2)}"
[ -z "$SESSION_ID" ] && exit 0
SOCK="${PLANEAI_SOCKET:-$HOME/Library/Application Support/ca.nicolegros.planeai/notify.sock}"
case "$1" in
  stop) E="stop" ;;
  busy) E="busy" ;;
  *)    E="notification" ;;
esac
[ -S "$SOCK" ] && printf '{"session_id":"%s","event":"%s"}\n' "$SESSION_ID" "$E" | nc -U "$SOCK" -w1 2>/dev/null
exit 0
