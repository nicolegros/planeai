#!/bin/bash
# planeai stop-hook for Claude Code: notifies planeai when Claude needs attention.
# Installed by planeai. Safe to delete — notifications will fall back to silence detection.
SESSION_ID="$PLANEAI_SESSION_ID"
[ -z "$SESSION_ID" ] && exit 0
SOCK="${PLANEAI_SOCKET:-}"
[ -z "$SOCK" ] && exit 0
EVENT=$(jq -r '.hook_event_name // ""' 2>/dev/null)
case "$EVENT" in
  Stop) E="stop" ;;
  UserPromptSubmit) E="busy" ;;
  *)    E="notification" ;;
esac
[ -S "$SOCK" ] && printf '{"session_id":"%s","event":"%s"}\n' "$SESSION_ID" "$E" | nc -U "$SOCK" -w1 2>/dev/null
exit 0
