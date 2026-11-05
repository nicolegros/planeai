#!/bin/bash
# planeai stop-hook for Claude Code: notifies planeai when Claude needs attention.
# Installed by planeai. Safe to delete — notifications will fall back to silence detection.
SESSION_ID="${PLANEAI_SESSION_ID:-$(tmux show-environment PLANEAI_SESSION_ID 2>/dev/null | cut -d= -f2)}"
[ -z "$SESSION_ID" ] && exit 0
SOCK="${PLANEAI_SOCKET:-$HOME/Library/Application Support/ca.nicolegros.planeai/notify.sock}"
INPUT=$(cat)
EVENT=$(printf '%s' "$INPUT" | jq -r '.hook_event_name // ""' 2>/dev/null)
# Handle both camelCase and PascalCase event names
case "$EVENT" in
  Stop|stop) E="stop" ;;
  UserPromptSubmit|userPromptSubmit|PostToolUse|postToolUse) E="busy" ;;
  Notification|notification|StopFailure|stopFailure) E="notification" ;;
  *)    exit 0 ;;  # unknown events must never alert
esac
# The transcript tells planeai whether background subagents still run when a turn ends.
MSG=$(printf '%s' "$INPUT" | jq -c --arg s "$SESSION_ID" --arg e "$E" \
  '{session_id: $s, event: $e} + ({transcript_path, notification_type} | with_entries(select(.value | type == "string")))' 2>/dev/null)
[ -S "$SOCK" ] && [ -n "$MSG" ] && printf '%s\n' "$MSG" | nc -U "$SOCK" -w1 2>/dev/null
exit 0
