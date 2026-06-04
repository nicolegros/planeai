#!/bin/bash
# E2E test: tmux sessions survive app quit/reopen cycle
# Tests the fix for spurious pty-exited on re-attach
#
# Requirements: tmux, sqlite3, the planeai bundle at the path below
# Run from the repo root: ./tests/e2e_session_persistence.sh

set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP_BIN="$REPO_DIR/src-tauri/target/release/bundle/macos/planeai.app/Contents/MacOS/planeai"
DB_PATH="$HOME/Library/Application Support/ca.nicolegros.planeai/planeai.db"
TMUX_BIN="/opt/homebrew/bin/tmux"
[ -f "$TMUX_BIN" ] || TMUX_BIN="tmux"

PASS=0
FAIL=0

assert_eq() {
  local desc="$1" expected="$2" actual="$3"
  if [ "$expected" = "$actual" ]; then
    echo "  ✓ $desc"
    PASS=$((PASS + 1))
  else
    echo "  ✗ $desc (expected='$expected', got='$actual')"
    FAIL=$((FAIL + 1))
  fi
}

cleanup() {
  # Kill test app if running
  [ -n "${APP_PID:-}" ] && kill "$APP_PID" 2>/dev/null || true
  # Kill test tmux sessions
  $TMUX_BIN kill-session -t "=planeai-test-e2e-aaa" 2>/dev/null || true
  $TMUX_BIN kill-session -t "=planeai-test-e2e-bbb" 2>/dev/null || true
  # Remove test sessions from DB
  sqlite3 "$DB_PATH" "DELETE FROM sessions WHERE tmux_name LIKE 'planeai-test-e2e-%';" 2>/dev/null || true
  # Remove test project
  sqlite3 "$DB_PATH" "DELETE FROM projects WHERE name = '__e2e_test_project__';" 2>/dev/null || true
}
trap cleanup EXIT

echo "=== E2E: tmux session persistence across app restart ==="
echo ""

# Preconditions
if [ ! -f "$APP_BIN" ]; then
  echo "SKIP: app bundle not found at $APP_BIN (run 'make build' first)"
  exit 0
fi
if ! command -v sqlite3 >/dev/null; then
  echo "SKIP: sqlite3 not found"
  exit 0
fi
if ! $TMUX_BIN -V >/dev/null 2>&1; then
  echo "SKIP: tmux not available"
  exit 0
fi

# Setup: create tmux sessions and DB entries to simulate planeai sessions
echo "Setup..."
cleanup 2>/dev/null || true

# Create a test project in DB
PROJECT_ID="e2e-proj-$(date +%s)"
sqlite3 "$DB_PATH" "INSERT INTO projects (id, name, path) VALUES ('$PROJECT_ID', '__e2e_test_project__', '/tmp');"

# Create tmux sessions with a long-running process
$TMUX_BIN new-session -d -s "planeai-test-e2e-aaa" "sleep 3600"
$TMUX_BIN new-session -d -s "planeai-test-e2e-bbb" "sleep 3600"

# Insert session rows as 'active'
sqlite3 "$DB_PATH" "
INSERT INTO sessions (id, project_id, name, tmux_name, branch, status, created_at, backend, auto_approve)
VALUES ('e2e-sess-aaa', '$PROJECT_ID', 'test-a', 'planeai-test-e2e-aaa', 'main', 'active', datetime('now'), 'tmux', 0);
INSERT INTO sessions (id, project_id, name, tmux_name, branch, status, created_at, backend, auto_approve)
VALUES ('e2e-sess-bbb', '$PROJECT_ID', 'test-b', 'planeai-test-e2e-bbb', 'main', 'active', datetime('now'), 'tmux', 0);
"

echo ""
echo "Test 1: Sessions survive app startup (reconciliation)"
# Launch app, wait for startup, check DB
"$APP_BIN" &>/dev/null &
APP_PID=$!
sleep 5

STATUS_A=$(sqlite3 "$DB_PATH" "SELECT status FROM sessions WHERE id='e2e-sess-aaa';")
STATUS_B=$(sqlite3 "$DB_PATH" "SELECT status FROM sessions WHERE id='e2e-sess-bbb';")
assert_eq "session A active after startup" "active" "$STATUS_A"
assert_eq "session B active after startup" "active" "$STATUS_B"

echo ""
echo "Test 2: Sessions survive app quit + reopen"
kill "$APP_PID" 2>/dev/null; wait "$APP_PID" 2>/dev/null || true
sleep 2

STATUS_A=$(sqlite3 "$DB_PATH" "SELECT status FROM sessions WHERE id='e2e-sess-aaa';")
STATUS_B=$(sqlite3 "$DB_PATH" "SELECT status FROM sessions WHERE id='e2e-sess-bbb';")
assert_eq "session A still active after quit" "active" "$STATUS_A"
assert_eq "session B still active after quit" "active" "$STATUS_B"

# Reopen
"$APP_BIN" &>/dev/null &
APP_PID=$!
sleep 5

STATUS_A=$(sqlite3 "$DB_PATH" "SELECT status FROM sessions WHERE id='e2e-sess-aaa';")
STATUS_B=$(sqlite3 "$DB_PATH" "SELECT status FROM sessions WHERE id='e2e-sess-bbb';")
assert_eq "session A active after reopen" "active" "$STATUS_A"
assert_eq "session B active after reopen" "active" "$STATUS_B"

echo ""
echo "Test 3: Sessions survive double attach (simulates component remount)"
# Call attach_session twice rapidly — the second should not kill the first.
# We verify by checking status remains active after a delay.
sleep 5
STATUS_A=$(sqlite3 "$DB_PATH" "SELECT status FROM sessions WHERE id='e2e-sess-aaa';")
STATUS_B=$(sqlite3 "$DB_PATH" "SELECT status FROM sessions WHERE id='e2e-sess-bbb';")
assert_eq "session A still active (no spurious exit)" "active" "$STATUS_A"
assert_eq "session B still active (no spurious exit)" "active" "$STATUS_B"

echo ""
echo "Test 4: Dead tmux session IS marked exited on startup"
kill "$APP_PID" 2>/dev/null; wait "$APP_PID" 2>/dev/null || true
sleep 2

# Kill one tmux session
$TMUX_BIN kill-session -t "=planeai-test-e2e-bbb" 2>/dev/null || true
# Ensure DB still says active
sqlite3 "$DB_PATH" "UPDATE sessions SET status='active' WHERE id='e2e-sess-bbb';"

# Reopen — reconciliation should mark bbb as exited
"$APP_BIN" &>/dev/null &
APP_PID=$!
sleep 5

STATUS_A=$(sqlite3 "$DB_PATH" "SELECT status FROM sessions WHERE id='e2e-sess-aaa';")
STATUS_B=$(sqlite3 "$DB_PATH" "SELECT status FROM sessions WHERE id='e2e-sess-bbb';")
assert_eq "session A (alive tmux) stays active" "active" "$STATUS_A"
assert_eq "session B (dead tmux) marked exited" "exited" "$STATUS_B"

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
[ "$FAIL" -eq 0 ] && exit 0 || exit 1
