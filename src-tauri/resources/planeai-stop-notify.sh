#!/bin/bash
# planeai stop-hook: notifies planeai when kiro finishes a turn.
# Installed by planeai. Safe to delete — notifications will fall back to silence detection.
INPUT=$(cat)
EVENT=$(echo "$INPUT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('hook_event_name',''))" 2>/dev/null)
[ "$EVENT" != "stop" ] && exit 0
SESSION_ID="${PLANEAI_SESSION_ID:-$(tmux show-environment PLANEAI_SESSION_ID 2>/dev/null | cut -d= -f2)}"
[ -z "$SESSION_ID" ] && exit 0
SOCK="$HOME/Library/Application Support/ca.nicolegros.planeai/notify.sock"
[ -S "$SOCK" ] && echo "$SESSION_ID" | nc -U "$SOCK" -w1 2>/dev/null
exit 0
